use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use futures::future::select_all;
use futures::FutureExt;
use tokio::select;
use tokio::sync::{RwLock, RwLockWriteGuard};
use tokio::task::JoinSet;
use tokio_util::sync::CancellationToken;
use tracing::{debug, error, trace, warn};

use crate::error::{PetriError, Result};
use crate::net::{ArcVariant, NetChangeEvent, PetriNet, PetriNetBuilder, PlaceId, TransitionId};
use crate::{ETCDGate, ETCDTransitionGate};

type SenderMap = HashMap<PlaceId, tokio::sync::watch::Sender<u64>>;
type ReceiverMap = HashMap<PlaceId, tokio::sync::watch::Receiver<u64>>;

#[tracing::instrument(level = "info", skip(net_builder, etcd, cancel_token), fields(region=etcd.config.region))]
pub async fn run(
    net_builder: &PetriNetBuilder,
    mut etcd: ETCDGate,
    cancel_token: CancellationToken,
) -> Result<()> {
    if etcd.config.region.is_empty() {
        return Err(PetriError::ConfigError(
            "Method 'run' requires a region to be set.".to_string(),
        ));
    }
    // TODO: validate assignement (all transitions have a runner assigned, its validate function
    // works) We don't want to connect to etcd if we have an inconsistent net to begin with.
    let net_builder = net_builder.only_region(&etcd.config.region)?;
    let cancel_token_clone = cancel_token.clone();
    let result = async {
        // connect, obtain leadership for our region, create a net using the ids stored on etcd
        etcd.connect(Some(cancel_token_clone)).await?;
        etcd.campaign_for_region().await?;
        let net_ids = etcd.assign_ids(&net_builder).await?;
        let net = net_builder.build(&net_ids)?;

        // get the current state and wrap it into events to be handled later.
        let (tx, mut rx) = tokio::sync::mpsc::channel::<NetChangeEvent>(128);
        etcd.load_data(
            tx,
            net.places().keys().copied().collect(),
            net.transitions().keys().copied().collect(),
        )
        .await?;

        // start a runner for each transition
        let (pl_tx, pl_rx): (SenderMap, ReceiverMap) = net
            .places()
            .keys()
            .map(|&pl_id| {
                let (tx, rx) = tokio::sync::watch::channel(0);
                ((pl_id, tx), (pl_id, rx))
            })
            .unzip();
        
        let mut receivers = HashMap::<TransitionId,ReceiverMap>::new();
        for (&(pl_id, tr_id), arc) in net.arcs() {
            if arc.variant() == ArcVariant::Out {
                continue;
            }
            receivers.entry(tr_id).or_default().insert(pl_id, pl_rx[&pl_id].clone());
        }
        let transition_ids: Vec<TransitionId> = net.transitions().keys().copied().collect();
        let net_lock = Arc::new(RwLock::new(net));
        let mut transition_tasks = JoinSet::<()>::new();
        for transition_id in transition_ids {
            let place_rx = receivers.remove(&transition_id).unwrap();
            let net_lock = Arc::clone(&net_lock);
            let cancel_token = cancel_token.clone();
            let etcd_gate = etcd.create_transition_gate(transition_id)?;
            let runner = TransitionRunner {cancel_token, net_lock, etcd_gate, transition_id, place_rx };
            transition_tasks.spawn(runner.run_transition()); 
        }

        // main loop
        // Note: a starting 'reset' event and load events should already be in the change event
        // channel. The reset event will also automatically mark all places as 'to check', so they
        // should be checked at least once.

        let mut event_buffer = Vec::<NetChangeEvent>::new();
        let mut net_guard: Option<RwLockWriteGuard<PetriNet>> = None;

        loop {
            select! {
                count = rx.recv_many(&mut event_buffer, 10) => {
                    if count == 0 {
                        // channel closed
                        trace!("Main net change channel closed");
                        cancel_token.cancel();
                        return Err(PetriError::Cancelled());
                    }
                },
                _ = cancel_token.cancelled() => {
                    return Err(PetriError::Cancelled());
                },
                res = transition_tasks.join_next() => {
                    warn!("Transition task ended");
                    cancel_token.cancel();
                    match res {
                        None => {
                            return Err(PetriError::Other("No transition task running.".into()))
                        },
                        Some(res) => {
                            if let Err(err) = res {
                                return Err(PetriError::Other(format!("Transition task finished unexpectedly: {}", err)))
                            }
                        }
                    };
                    return Err(PetriError::Cancelled());
                }
            }
            if !event_buffer.is_empty() {
                if net_guard.is_none() {
                    net_guard = Some(net_lock.write().await);
                }
                for evt in event_buffer.drain(..) {
                    debug!("Change: {}", evt);
                    let pl_id_to_notify = net_guard.as_mut().unwrap().apply_change_event(evt)?;
                    for pl_id in pl_id_to_notify {
                        let _ = pl_tx[&pl_id].send(net_guard.as_mut().unwrap().revision());
                        // Note: error means 'all receivers closed'. Depending on net layout, this
                        // might be the case all the time for some places.
                        // we are watching for crashed transition runners another way.
                    }
                }
                if !rx.is_empty() {
                    // always handle change events before doing anything else
                    continue;
                }
            }
            drop(net_guard.take());
        }
    }
    .await;

    // campaign for given election name

    // make sure all places / transitions exist on etcd

    // - lock all non-multiplex transitions
    // - wait for all others to be free of locks?
    // -> inform user about starting delay if waiting here
    // -> timeout?

    // start listening to state token data / place / transition changes
    // load current state for places + transitions -> take maximum revision
    // load token data for that revision
    // listen for changes starting from that revision
    // changes to a given place -> send place over channel to main loop

    // list all transitions as "to_check"
    // main loop:
    // - if cancelled:
    //   -> cancel all running transitions (they should have separate cancel tokens)
    // - check all transitions in "to_check"
    // - record for which places the transition needs to wait (possible to all of them) / possibly timeouts
    //   -> start (and remember) a task for each transition that seems ready
    // - go through channel and put all relevant transitions into 'to_check'
    // - check timeouts and put transitions into 'to_check'
    // if 'to_check' is empty:
    // wait for lowest timeout / next message on channel / cancellation

    // release lease

    // finally:
    etcd.disconnect().await;

    match result {
        Err(PetriError::Cancelled()) => Ok(()),
        _ => result,
    }
}

pub struct TransitionRunner {
    cancel_token: CancellationToken,
    transition_id: TransitionId,
    etcd_gate: ETCDTransitionGate,
    net_lock: Arc<RwLock<PetriNet>>,
    place_rx: HashMap<PlaceId, tokio::sync::watch::Receiver<u64>>,
}


impl TransitionRunner {

    #[tracing::instrument(level = "debug", skip(self), fields(transition_id=self.transition_id.0))]
    async fn run_transition(mut self) {
        let mut wait_for: Option<PlaceId> = None;
        loop {
            tokio::time::sleep(Duration::from_secs(1)).await;
            let wait_for_change = async {
                match wait_for {
                    None => {
                        let futures: Vec<_> = self.place_rx.values_mut().map(|rec| {
                            rec.changed().boxed()
                        }).collect();
                        let _ = select_all(futures).await;
                    }
                    Some(pl_id) => {
                        let _ = self.place_rx.get_mut(&pl_id).unwrap().changed().await;
                    }
                    // TODO: raise an error in case of failures? Or log them at least?
                    // Should not fail without logic errors in the program. But might help debugging
                }
            };

            select! {
                _ = wait_for_change => {},
                _ = self.cancel_token.cancelled()  => {return;}
            }
            for rec in self.place_rx.values_mut() {
                rec.mark_unchanged();
            }

            let net = self.net_lock.read().await;
            // check if we can start
            let mut startable = true;
            for &pl_id in self.place_rx.keys() {
                let pl = net.places().get(&pl_id).unwrap();
                if pl.token_ids().is_empty() {
                    wait_for = Some(pl_id);
                    trace!(place=pl_id.0, "Place empty.");
                    startable = false;
                    break;
                }
            }
            if !startable {
                continue;
            }
            // TODO: get locks (input & conditions) and check again (drop net in between)
            trace!("Running transition.");
            
            let take_tokens = self.place_rx.keys().flat_map(|&pl_id| {
                let pl = net.places().get(&pl_id).unwrap();
                pl.token_ids().iter().map(move |&to_id| (to_id, pl_id))
            }).collect::<Vec<_>>();
            let mut output_pl_id: Option<PlaceId> = None;
            for &(pl_id, tr_id) in net.arcs().keys() {
                if tr_id == self.transition_id && !self.place_rx.contains_key(&pl_id){
                    output_pl_id = Some(pl_id);
                }
            }
            let place_tokens = self.place_rx.keys().flat_map(|&pl_id| {
                let pl = net.places().get(&pl_id).unwrap();
                pl.token_ids().iter().map(move |&to_id| (to_id, pl_id, output_pl_id.unwrap(), format!("placed by tr {}", self.transition_id.0).into()))
            }).collect::<Vec<_>>();


            // Check place state before starting?!?
            let _ = self.etcd_gate.start_transition(take_tokens).await;             
            // TODO handle errors!
            
            drop(net);
            // TODO release locks

            // actual transition running
            tokio::time::sleep(Duration::from_secs(1)).await;

            // obtain placement locks (input & output)
            let _ = self.etcd_gate.end_transition(place_tokens).await;

            trace!("Finished transition.");
            tokio::time::sleep(Duration::from_millis(100)).await;

        }
    }
}
