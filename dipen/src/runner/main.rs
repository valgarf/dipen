use std::collections::HashMap;
use std::sync::Arc;
use tokio::select;
use tokio::sync::{RwLock, RwLockWriteGuard};
use tokio::task::JoinSet;
use tokio_util::sync::CancellationToken;
use tracing::{debug, trace, warn};

use crate::error::{PetriError, Result};
use crate::net::{ArcVariant, NetChangeEvent, PetriNet, PetriNetBuilder, PlaceId, Revision, TransitionId};
use crate::storage::etcd::ETCDStorageClient;
use crate::storage::traits::StorageClient as _;
use super::ExecutorRegistry;
use super::transition::TransitionRunner;

type SenderMap = HashMap<PlaceId,  tokio::sync::watch::Sender<Revision>>;
type ReceiverMap = HashMap<PlaceId, tokio::sync::watch::Receiver<Revision>>;

#[tracing::instrument(level = "info", skip_all, fields(region=etcd.config.region, node=etcd.config.node_name))]
pub async fn run(
    net_builder: Arc<PetriNetBuilder>,
    mut etcd: ETCDStorageClient,
    executors: ExecutorRegistry,
    cancel_token: CancellationToken,
) -> Result<()> {
    if etcd.config.region.is_empty() {
        return Err(PetriError::ConfigError(
            "Method 'run' requires a region to be set.".to_string(),
        ));
    }
    let region_name = etcd.config.region.clone();
    let node_name = etcd.config.node_name.clone();
    // validate assignement (all transitions have an executor assigned and its validate function
    // succeeds): We don't want to connect to etcd if we have an inconsistent net to begin with.
    let net_builder = net_builder.only_region(&etcd.config.region)?;

    let cancel_token_clone = cancel_token.clone();
    let result = async {
        // connect, obtain leadership for our region, create a net using the ids stored on etcd
        etcd.connect(Some(cancel_token_clone)).await?;
        let net_ids = etcd.assign_ids(&net_builder).await?;
        let net = net_builder.build(&net_ids)?;

        // get the current state and wrap it into events to be handled later.
        let (tx, mut rx) = tokio::sync::mpsc::channel::<NetChangeEvent>(128);
        etcd.load_data(
            tx,
            net.places().map(|t|t.0).collect(),
            net.transitions().map(|t|t.0).collect(),
        )
        .await?;

        // start a runner for each transition
        let (pl_tx, pl_rx): (SenderMap, ReceiverMap) = net
            .places()
            .map(|(pl_id, _)| {
                let (tx, rx) = tokio::sync::watch::channel(Revision(0));
                ((pl_id, tx), (pl_id, rx))
            })
            .unzip();
        
        let transition_ids: Vec<TransitionId> = net.transitions().map(|t|t.0).collect();
        let (tx_revision, rx_revision) = tokio::sync::watch::channel(Revision(0));
        let (tx_leader, rx_leader) = tokio::sync::watch::channel(false);
        let net_lock = Arc::new(RwLock::new(net));
        let net_read_guard = net_lock.read().await;
        let mut transition_tasks = JoinSet::<()>::new();
        for transition_id in transition_ids {
            let mut place_locks = HashMap::new();
            let mut receivers = HashMap::new();
            for (pl_id, arc) in net_read_guard.arcs_for(transition_id) {
                place_locks.insert(pl_id, etcd.place_lock_client(pl_id)?);
                if arc.variant() != ArcVariant::Out {
                    receivers.insert(pl_id, pl_rx[&pl_id].clone());
                }                
            }
            let tr_name = net_read_guard.transition(transition_id).unwrap().name();
            let exec = executors.dispatcher.get(tr_name).ok_or_else(|| PetriError::ConfigError(format!("Could not find an executor for transition {}", tr_name)))?;
            let mut runner = TransitionRunner {
                cancel_token: cancel_token.clone(), 
                net_lock:Arc::clone(&net_lock), 
                transition_client: etcd.create_transition_client(transition_id)?, 
                transition_id,
                rx_place: receivers,
                rx_revision: rx_revision.clone(), 
                rx_leader: rx_leader.clone(),
                exec: exec.clone_empty(), 
                place_locks,
                region_name: region_name.clone(),
                node_name: node_name.clone(),
                transition_name: tr_name.into(),
                run_data: Default::default()
            };
            runner.create_executor().await?;
            transition_tasks.spawn(runner.run_transition()); 
        }

        drop(net_read_guard);

        let election_fut = etcd.campaign_for_region(tx_leader);
        let mut election_completed = false;
        tokio::pin!(election_fut);

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
                },
                res = &mut election_fut, if !election_completed => {
                    res?;
                    election_completed = true;
                }
            }
            if !event_buffer.is_empty() {
                if net_guard.is_none() {
                    net_guard = Some(net_lock.write().await);
                }
                let mut revision = Revision(0);
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
                // Note: tx_revision can only be closed if all transition runners are done.
                // Error can be ignored here, we handle that case elsewhere.
            }
            drop(net_guard.take());
        }
    }
    .await;

    // finally:
    etcd.disconnect().await;

    match result {
        Err(PetriError::Cancelled()) => Ok(()),
        _ => result,
    }
}