use std::collections::HashMap;
use std::sync::Arc;
use tokio::select;
use tokio::sync::{RwLock, RwLockWriteGuard};
use tokio::task::JoinSet;
use tokio_util::sync::CancellationToken;
use tracing::{debug, trace, warn};

use crate::error::{PetriError, Result};
use crate::net::{ArcVariant, NetChangeEvent, PetriNet, PetriNetBuilder, PlaceId, TransitionId};
use crate::place_locks::PlaceLock;
use crate::transition::TransitionExecutor;
use crate::transition_runner::{TransitionExecutorDispatch, TransitionExecutorDispatchStruct, TransitionRunner, ValidateContextStruct};
use crate::ETCDGate;

type SenderMap = HashMap<PlaceId,  tokio::sync::watch::Sender<u64>>;
type ReceiverMap = HashMap<PlaceId, tokio::sync::watch::Receiver<u64>>;

#[derive(Default)]
pub struct ExecutorRegistry {
    dispatcher: HashMap<String, Box<dyn TransitionExecutorDispatch>>
}

impl ExecutorRegistry {
    pub fn new() -> Self {Self::default()}
    pub fn register<T: TransitionExecutor + Send + Sync + 'static>(&mut self, transition_name: &str) {
        self.dispatcher.insert(
            transition_name.into(), 
            Box::new(
                TransitionExecutorDispatchStruct::<T>{executor: None}
            )
        );
    }
}

#[tracing::instrument(level = "info", skip_all, fields(region=etcd.config.region))]
pub async fn run(
    net_builder: Arc<PetriNetBuilder>,
    mut etcd: ETCDGate,
    executors: ExecutorRegistry,
    cancel_token: CancellationToken,
) -> Result<()> {
    if etcd.config.region.is_empty() {
        return Err(PetriError::ConfigError(
            "Method 'run' requires a region to be set.".to_string(),
        ));
    }
    // validate assignement (all transitions have a runner assigned and its validate function
    // succeeds): We don't want to connect to etcd if we have an inconsistent net to begin with.
    let net_builder = net_builder.only_region(&etcd.config.region)?;
    for tr_name in net_builder.transitions().keys() {
        // TODO: could be optimized by going through all the arcs once and storing the relevant ones
        // for each transition.
        let arcs = net_builder.arcs().values().filter(|a| {a.transition() == tr_name}).collect::<Vec<_>>();
        let ctx = ValidateContextStruct{
            net: &net_builder,
            transition_name: tr_name,
            arcs
        };
        if let Some(exec) = executors.dispatcher.get(tr_name) {
            let res = exec.validate(&ctx);
            if !res.success {
                return Err(PetriError::ConfigError(format!("Validation for transition {} failed with reason: {}.", tr_name, res.reason)));
            }
        }
        else {
            return Err(PetriError::ConfigError(format!("Could not find an executor for transition {}", tr_name)));
        }
    }
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

        let mut place_locks_tr: HashMap<TransitionId, HashMap<PlaceId, Arc<PlaceLock>>> = HashMap::new();
        for &(pl_id, tr_id) in net.arcs().keys() {
            let place_locks = place_locks_tr.entry(tr_id).or_default();
            place_locks.insert(pl_id, etcd.place_lock(pl_id)?);
        }

        let transition_ids: Vec<TransitionId> = net.transitions().keys().copied().collect();
        let (tx_revision, rx_revision) = tokio::sync::watch::channel(0u64);
        let net_lock = Arc::new(RwLock::new(net));
        let net_read_guard = net_lock.read().await;
        let mut transition_tasks = JoinSet::<()>::new();
        for transition_id in transition_ids {
            let place_rx = receivers.remove(&transition_id).unwrap();
            let place_locks = place_locks_tr.remove(&transition_id).unwrap();
            let net_lock = Arc::clone(&net_lock);
            let cancel_token = cancel_token.clone();
            let etcd_gate = etcd.create_transition_gate(transition_id)?;
            let tr_name = net_read_guard.transitions().get(&transition_id).unwrap().name();
            let exec = executors.dispatcher.get(tr_name).unwrap().clone_empty();
            let rx_revision = rx_revision.clone();
            let runner = TransitionRunner {cancel_token, net_lock, etcd_gate, transition_id, place_rx, exec, rx_revision, place_locks};
            transition_tasks.spawn(runner.run_transition()); 
        }

        drop(net_read_guard);
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
                let mut revision = 0u64;
                for evt in event_buffer.drain(..) {
                    debug!("Change: {}", evt);
                    revision = evt.revision;
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
                let _ = tx_revision.send(revision); 
                // NOTE: tx_revision can only be closed if all transition runners are done.
                // Error can be ignored here, we handle that case elsewhere.
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