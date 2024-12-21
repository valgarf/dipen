use tokio::select;
use tokio::sync::{RwLock, RwLockWriteGuard};
use tokio_util::sync::CancellationToken;
use tracing::debug;

use crate::error::{PetriError, Result};
use crate::net::{NetChangeEvent, PetriNet, PetriNetBuilder};
use crate::ETCDGate;

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
    let net_builder = net_builder.only_region(&etcd.config.region)?;
    let cancel_token_clone = cancel_token.clone();
    let result = async {
        etcd.connect(Some(cancel_token_clone)).await?;
        etcd.campaign_for_region().await?;
        let net_ids = etcd.assign_ids(&net_builder).await?;
        let net = net_builder.build(&net_ids)?;
        let (tx, mut rx) = tokio::sync::mpsc::channel::<NetChangeEvent>(128);
        etcd.load_data(
            tx,
            net.places().keys().copied().collect(),
            net.transitions().keys().copied().collect(),
        )
        .await?;

        // main loop
        // Note: load events should already be in the buffer
        let net_lock = RwLock::new(net);
        let mut event_buffer = Vec::<NetChangeEvent>::new();
        let mut net_guard: Option<RwLockWriteGuard<PetriNet>> = None;
        loop {
            select! {
                count = rx.recv_many(&mut event_buffer, 10) => {
                    if count == 0 {
                        // channel closed
                        cancel_token.cancel();
                        return Err(PetriError::Cancelled());
                    }
                },
                _ = cancel_token.cancelled() => {
                    return Err(PetriError::Cancelled());
                }
            }
            if !event_buffer.is_empty() {
                if net_guard.is_none() {
                    net_guard = Some(net_lock.write().await);
                }
                for evt in event_buffer.drain(..) {
                    debug!("Change: {:?}", evt);
                    net_guard.as_mut().unwrap().apply_change_event(evt)?;
                    // TODO select transitions to check
                }
                if !rx.is_empty() {
                    // always handle change events before doing anything else
                    continue;
                }
            }
            net_guard.take(); // drop lock
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
