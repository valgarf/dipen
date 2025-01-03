use futures::future::select_all;
use futures::FutureExt;
use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use std::time::Duration;
use tokio::select;
use tokio::sync::{MutexGuard, RwLock};
use tokio_util::sync::CancellationToken;
use tracing::{error, info, trace};

use super::context::*;
use super::dispatch::TransitionExecutorDispatch;
use crate::error::{PetriError, Result};
use crate::etcd::{ETCDTransitionGate, FencingToken};
use crate::etcd::{PlaceLock, PlaceLockData};
use crate::exec::{CheckStartChoice, RunResult, RunResultData};
use crate::net::{self, PlaceId, Revision, TokenId};

pub(crate) struct TransitionRunner {
    pub cancel_token: CancellationToken,
    pub transition_id: net::TransitionId,
    pub etcd_gate: ETCDTransitionGate,
    pub net_lock: Arc<RwLock<net::PetriNet>>,
    pub place_rx: HashMap<net::PlaceId, tokio::sync::watch::Receiver<Revision>>,
    pub exec: Box<dyn TransitionExecutorDispatch>,
    pub rx_revision: tokio::sync::watch::Receiver<Revision>,
    pub place_locks: HashMap<net::PlaceId, Arc<PlaceLock>>,
    // used for logging and other messages
    pub region_name: String,
    pub node_name: String,
    pub transition_name: String,
    pub run_data: RunData,
}

#[derive(Default)]
pub(crate) struct RunData {
    wait_for: Option<net::PlaceId>,
    auto_recheck: Duration,
}

// ## run context

// ## dispatch

// runner implementation
impl TransitionRunner {
    #[tracing::instrument(level = "debug", skip(self), fields(transition=format!("{} ({})", self.transition_name, self.transition_id.0), region=self.region_name, node=self.node_name))]
    pub async fn run_transition(mut self) {
        let cancel_token = self.cancel_token.clone();
        select! {
            res = self._run_transition() => {
                if let Err(err) = res {
                    if ! matches!(err, PetriError::Cancelled()) {
                        error!("Running transition failed with error: {}", err);
                    }
                }
            }
            _ = cancel_token.cancelled() => {}
        }
    }

    async fn _run_transition(&mut self) -> Result<()> {
        self._create_executor().await;
        // loading event should have been handled before checking for transitions to cancel
        TransitionRunner::_wait_for_revision(&mut self.rx_revision, Revision(1)).await?;
        // We are just starting. If any token is still taken by our transitions, we need
        // to give it back to the input place. No one else should be running!
        self._cancel_running().await?;

        loop {
            // wait for any change on a relevant place (relevant -> input / condition arcs)
            self._wait_for_change().await?;
            // check if we can run without locking anything
            if self._check_start().await?.is_none() {
                // most of the time we cannot run, continue waiting
                continue;
            }
            // we could run in the current state, lock all relevant places
            let mut start_locks = self._cond_place_locks();
            let guards =
                TransitionRunner::_lock(&mut start_locks, Some(&mut self.rx_revision)).await?;
            // after locking, we check again
            let take = match self._check_start().await? {
                None => {
                    // state has changed while acquiring locks, we cannot run. Continue waiting
                    continue;
                }
                // we can run, _check_start returned the tokens the transition wants to take
                Some(take) => take,
            };
            let fencing_tokens = TransitionRunner::_get_fencing_tokens(&guards);
            trace!("Starting transition.");
            // update state of taken tokens on etcd
            let revision = self._start_transition(&take, fencing_tokens).await?;
            // get the run context, afterwards we don't need to hold the locks any longer
            let mut ctx = self._run_context(&take).await?;
            TransitionRunner::_release_locks(guards, revision);
            drop(start_locks);

            // actually run the transition
            let res: RunResultData = self._run(&mut ctx).await?.into();

            // we are done, we need to update the state of the target places, i.e.
            // locking output places and sending an update to etcd
            let mut finish_locks = self._target_place_locks(&res).await;
            let guards = TransitionRunner::_lock(&mut finish_locks, None).await?;
            let fencing_tokens = TransitionRunner::_get_fencing_tokens(&guards);
            let new_revision = self._finish_transition(res, &take, fencing_tokens).await?;
            TransitionRunner::_release_locks(guards, new_revision);

            trace!("Finished transition.");
            // we wait for the local net to update to the latest revision we created
            TransitionRunner::_wait_for_revision(&mut self.rx_revision, new_revision).await?;
        }
    }

    async fn _wait_for_revision(
        rx_revision: &mut tokio::sync::watch::Receiver<Revision>,
        revision: Revision,
    ) -> Result<()> {
        rx_revision
            .wait_for(move |&rev| rev >= revision)
            .await
            .map_err(|_| PetriError::Cancelled())?;
        Ok(())
    }

    async fn _create_executor(&mut self) {
        let net = self.net_lock.read().await;
        let ctx = create::CreateContextStruct {
            net: &net,
            transition_name: net.transition(self.transition_id).unwrap().name(),
            transition_id: self.transition_id,
            arcs: net.arcs_for(self.transition_id).collect(),
            registry_data: None, // will be replaced by the dispatcher
        };
        self.exec.create(ctx);
    }

    async fn _cancel_running(&mut self) -> Result<()> {
        let net = self.net_lock.read().await;
        let token_ids = net.transition(self.transition_id).unwrap().token_ids();
        if !token_ids.is_empty() {
            info!(
                "Cancelled transition on startup, returning tokens: [{}]",
                token_ids
                    .iter()
                    .map(|to_id| to_id.0.to_string())
                    .collect::<Vec<String>>()
                    .join(",")
            );
            let res = RunResultData {
                place: token_ids
                    .iter()
                    .map(|&to_id| {
                        let to = net.token(to_id).unwrap();
                        (to_id, to.last_place(), to.last_place(), to.data().into())
                    })
                    .collect(),
                create: vec![],
            };
            drop(net);
            let mut finish_locks = self._target_place_locks(&res).await;
            let guards = TransitionRunner::_lock(&mut finish_locks, None).await?;
            let fencing_tokens = TransitionRunner::_get_fencing_tokens(&guards);
            let taken = vec![];
            let rev = self._finish_transition(res, &taken, fencing_tokens).await?;
            TransitionRunner::_wait_for_revision(&mut self.rx_revision, rev).await?;
        }
        Ok(())
    }

    async fn _wait_for_change(&mut self) -> Result<()> {
        let wait_for_change = async {
            match self.run_data.wait_for {
                None => {
                    let futures: Vec<_> =
                        self.place_rx.values_mut().map(|rec| rec.changed().boxed()).collect();
                    let _ = select_all(futures).await;
                }
                Some(pl_id) => {
                    let _ = self.place_rx.get_mut(&pl_id).unwrap().changed().await;
                }
            }
        };

        let auto_recheck_fut = async {
            if !self.run_data.auto_recheck.is_zero() {
                tokio::time::sleep(self.run_data.auto_recheck).await;
            } else {
                loop {
                    tokio::time::sleep(Duration::from_secs(3600)).await;
                }
            }
        };
        select! {
            _ = wait_for_change => {},
            _ = auto_recheck_fut => {}
        }
        // we will check if we can run with the current state. Throw away all additional change
        // notifications
        for rec in self.place_rx.values_mut() {
            rec.mark_unchanged();
        }
        // we unset the wait for data. Any failure on '_check_start' will set a new value.
        // If we actually run the transition, we want to recheck on any change.
        self.run_data.wait_for = None;
        self.run_data.auto_recheck = Duration::default();
        Ok(())
    }

    async fn _check_start(&mut self) -> Result<Option<Vec<(PlaceId, TokenId)>>> {
        let net = self.net_lock.read().await;
        let mut ctx = start::StartContextStruct::new(&net, self.transition_id);
        match self.exec.check_start(&mut ctx).to_choice() {
            CheckStartChoice::Disabled(data) => {
                self.run_data.wait_for = data.wait_for;
                self.run_data.auto_recheck = data.auto_recheck;
                Ok(None)
            }
            CheckStartChoice::Enabled(data) => Ok(Some(data.take)),
        }
    }

    fn _cond_place_ids(&self) -> Vec<PlaceId> {
        self.place_rx.keys().copied().collect()
    }

    fn _cond_place_locks(&self) -> Vec<Arc<PlaceLock>> {
        self._cond_place_ids()
            .iter()
            .map(|pl_id| {
                Arc::clone(self.place_locks.get(pl_id).expect("Place if not found in place locks"))
            })
            .collect()
    }

    async fn _lock<'a>(
        locks: &'a mut Vec<Arc<PlaceLock>>,
        rx_revision: Option<&mut tokio::sync::watch::Receiver<Revision>>,
    ) -> Result<Vec<MutexGuard<'a, PlaceLockData>>> {
        locks.sort_by_key(|pl_lock| pl_lock.place_id());
        let mut guards: Vec<MutexGuard<PlaceLockData>> = Vec::with_capacity(locks.len());
        for l in locks {
            guards.push(l.acquire().await?);
        }
        // Note: locks come with a minimum revision. The local net needs to be up to date with this
        // revision at least, otherwise the local net is still in a state we had before we acquired
        // the locks.
        // The revision is taken from etcd if the lock was newly acquired. If some other task held
        // the lock before, it should have updated the revision to the last change it uploaded to
        // etcd.
        let min_revision = guards.iter().map(|g| g.min_revision()).max().unwrap_or(Revision(0));
        if let Some(rx_revision) = rx_revision {
            TransitionRunner::_wait_for_revision(rx_revision, min_revision).await?;
        }
        Ok(guards)
    }

    fn _get_fencing_tokens<'a>(
        guards: &'a Vec<MutexGuard<PlaceLockData>>,
    ) -> Vec<&'a FencingToken> {
        guards.iter().map(|g| g.fencing_token()).collect()
    }

    fn _release_locks(mut guards: Vec<MutexGuard<PlaceLockData>>, revision: Revision) {
        for g in &mut guards {
            g.set_min_revision(revision);
        }
    }

    async fn _start_transition(
        &mut self,
        take: &[(PlaceId, TokenId)],
        fencing_tokens: Vec<&FencingToken>,
    ) -> Result<Revision> {
        self.etcd_gate.start_transition(take.iter().copied(), &fencing_tokens).await
    }

    async fn _run_context(&mut self, take: &[(PlaceId, TokenId)]) -> Result<run::RunContextStruct> {
        let net = self.net_lock.read().await;
        Ok(run::RunContextStruct {
            tokens: take
                .iter()
                .map(|&(orig_pl_id, to_id)| run::RunTokenContextStruct {
                    token_id: to_id,
                    orig_place_id: orig_pl_id,
                    data: net.token(to_id).unwrap().data().into(),
                })
                .collect(),
        })
    }
    async fn _run(&mut self, ctx: &mut run::RunContextStruct) -> Result<RunResult> {
        Ok(self.exec.run(ctx).await)
    }

    async fn _target_place_locks(&self, res: &RunResultData) -> Vec<Arc<PlaceLock>> {
        // TODO: is it necessary to lock the places we took tokens from?
        // We will be moving / destroying those tokens and the transitions start method's may use
        // their existence to decide wether to start or not.
        let target_places: HashSet<PlaceId> = res
            .place
            .iter()
            .map(|&(_, _, target, _)| target)
            .chain(res.create.iter().map(|&(target, _)| target))
            .collect();

        let net = self.net_lock.read().await;
        let mut locks = target_places
            .iter()
            .filter(|&pl_id| net.place(*pl_id).map(|pl| pl.output_locking()).unwrap_or(true))
            .map(|pl_id| Arc::clone(self.place_locks.get(pl_id).unwrap()))
            .collect::<Vec<_>>();
        locks.sort_by_key(|pl_lock| pl_lock.place_id());
        locks
    }

    async fn _finish_transition(
        &mut self,
        res: RunResultData,
        taken: &[(PlaceId, TokenId)],
        fencing_tokens: Vec<&FencingToken>,
    ) -> Result<Revision> {
        let placed_to_ids: HashSet<TokenId> =
            res.place.iter().map(|&(to_id, _, _, _)| to_id).collect();
        let destroy: HashSet<(PlaceId, TokenId)> =
            taken.iter().filter(|(_, to_id)| !placed_to_ids.contains(to_id)).copied().collect();
        // Should we check that the token's 'last_place' is a valid incoming arc?
        // Could only be a problem if the locking is somehow broken / manual modification
        let revision =
            self.etcd_gate.end_transition(res.place, res.create, destroy, &fencing_tokens).await?;
        Ok(revision)
    }
}
