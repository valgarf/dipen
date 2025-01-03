use std::sync::Arc;

use dipen::{
    error::Result as PetriResult, etcd::ETCDGate, net::PetriNetBuilder, runner::ExecutorRegistry,
};
use tokio_util::sync::CancellationToken;
use tracing::{error, info};

#[path = "common/mod.rs"]
pub mod common;

#[tracing::instrument(level = "info", skip_all)]
pub async fn run_example(
    net: Arc<PetriNetBuilder>,
    etcd: ETCDGate,
    executors: ExecutorRegistry,
    cancel_token: CancellationToken,
) -> PetriResult<()> {
    // let mut executors = ExecutorRegistry::new();
    // executors.register::<common::transitions::DelayedMove>("tr1", None);
    // executors.register::<common::transitions::DelayedMove>("tr2", None);
    // executors.register::<common::transitions::Initialize>("tr-init", None);

    let run = dipen::runner::run(Arc::clone(&net), etcd, executors, cancel_token);
    match run.await {
        Ok(_) => {}
        Err(err) => {
            error!("Run finished with error: {}", err);
        }
    }

    info!("Bye.");
    Ok(())
}
