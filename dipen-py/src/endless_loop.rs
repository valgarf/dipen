use std::sync::Arc;

use dipen::{
    error::Result as PetriResult,
    etcd::{ETCDConfigBuilder, ETCDGate},
    net::PetriNetBuilder,
    runner::ExecutorRegistry,
};
use tokio::{runtime::Runtime, signal};
use tokio_util::sync::CancellationToken;
use tracing::{error, info, warn};

#[path = "common/mod.rs"]
mod common;

#[tracing::instrument(level = "info", skip(net))]
pub async fn run_example(net: Arc<PetriNetBuilder>, etcd: ETCDGate) -> PetriResult<()> {
    let shutdown_token = CancellationToken::new();
    let shutdown_token_clone = shutdown_token.clone();
    tokio::spawn(async move {
        tokio::select! {
            _ = signal::ctrl_c() => {
                warn!("Shutting down. Ctrl+C pressed (or corresponding signal sent).")
            },
            _ = shutdown_token_clone.cancelled() => {
                warn!("Shutting down. Shutdown token has been invoked (probably due to some previous error).")
            },
        }
        shutdown_token_clone.cancel();
    });

    let mut executors = ExecutorRegistry::new();
    executors.register::<common::transitions::DelayedMove>("tr1", None);
    executors.register::<common::transitions::DelayedMove>("tr2", None);
    executors.register::<common::transitions::Initialize>("tr-init", None);

    let run = dipen::runner::run(Arc::clone(&net), etcd, executors, shutdown_token.clone());
    match run.await {
        Ok(_) => {}
        Err(err) => {
            error!("Run finished with error: {}", err);
        }
    }

    info!("Bye.");
    Ok(())
}
