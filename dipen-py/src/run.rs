use std::sync::Arc;
use std::thread::{self, JoinHandle};

use crate::error::*;
use crate::etcd::PyETCDGateConfig;
use crate::exec::RUNNING_LOOP;
use crate::net::PyPetriNetBuilder;
use crate::registry::PyExecutorRegistry;
use dipen::error::{PetriError, Result as PetriResult};
use dipen::etcd::ETCDGate;
use dipen::net::PetriNetBuilder;
use dipen::runner::ExecutorRegistry;
use pyo3::prelude::*;
use pyo3_async_runtimes::get_running_loop;
use tokio::runtime::Runtime;
use tokio_util::sync::CancellationToken;
use tracing::{error, info};

#[tracing::instrument(level = "info", skip_all)]
pub async fn run_async(
    net: Arc<PetriNetBuilder>,
    etcd: ETCDGate,
    executors: ExecutorRegistry,
    cancel_token: CancellationToken,
) -> PetriResult<()> {
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

#[pyfunction]
pub fn start(
    py: Python<'_>,
    net: &PyPetriNetBuilder,
    etcd: &PyETCDGateConfig,
    executors: &PyExecutorRegistry,
) -> PyResult<RunHandle> {
    let l = get_running_loop(py)?;
    RUNNING_LOOP.get_or_init(|| l.unbind());

    let cloned_net = Arc::clone(&net.net);
    let etcd = etcd.config.clone();
    let cancel_token = CancellationToken::new();
    let cancel_token_cloned = cancel_token.clone();
    let executors = executors.to_rust_registry();
    let join_handle = py.allow_threads(|| {
        thread::spawn(|| {
            let rt = Runtime::new().expect("Failed to create tokio runtime");
            rt.block_on(run_async(cloned_net, ETCDGate::new(etcd), executors, cancel_token_cloned))
                .map_err(|e| e.into())
        })
    });
    Ok(RunHandle { join_handle: Some(join_handle), cancel_token })
}

#[pyclass]
pub struct RunHandle {
    join_handle: Option<JoinHandle<PyPetriResult<()>>>,
    cancel_token: CancellationToken,
}

#[pymethods]
impl RunHandle {
    fn cancel(&mut self) {
        self.cancel_token.cancel();
    }

    fn join(&mut self, py: Python<'_>) -> PyResult<()> {
        if let Some(join_handle) = self.join_handle.take() {
            py.allow_threads(|| {
                join_handle.join().map_err(|e| {
                    PyPetriError(PetriError::Other(format!("dipen main thread crashed: {:?}", e)))
                })
            })??;
        }
        Ok(())
    }
}