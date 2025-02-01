use std::sync::Arc;
use std::thread;

use crate::error::*;
use crate::etcd::PyETCDConfig;
use crate::exec::{ASYNCIO, RUNNING_LOOP};
use crate::in_memory::PyInMemoryStorageClient;
use crate::net::PyPetriNetBuilder;
use crate::registry::PyExecutorRegistry;
use dipen::error::{PetriError, Result as PetriResult};
use dipen::net::PetriNetBuilder;
use dipen::runner::ExecutorRegistry;
use dipen::storage::etcd::ETCDStorageClient;
use dipen::storage::traits::StorageClient;
use pyo3::prelude::*;
use pyo3_async_runtimes::get_running_loop;
use tokio::runtime::Runtime;
use tokio::sync::oneshot;
use tokio_util::sync::CancellationToken;
use tracing::{error, info};

#[tracing::instrument(level = "info", skip_all)]
pub async fn run_async<S: StorageClient + 'static>(
    net: Arc<PetriNetBuilder>,
    storage_client: S,
    executors: ExecutorRegistry,
    cancel_token: CancellationToken,
) -> PetriResult<()> {
    let run = dipen::runner::run(Arc::clone(&net), storage_client, executors, cancel_token);
    match run.await {
        Ok(_) => {}
        Err(err) => {
            error!("Run finished with error: {}", err);
        }
    }

    info!("Bye.");
    Ok(())
}

#[derive(FromPyObject)]
#[allow(clippy::large_enum_variant)]
pub enum StorageBackends {
    #[pyo3(transparent, annotation = "ETCDConfig")]
    Etcd(PyETCDConfig),
    #[pyo3(transparent, annotation = "InMemoryStorageClient")]
    InMemory(PyInMemoryStorageClient),
    // put the other cases here
}

#[pyfunction]
pub fn start(
    py: Python<'_>,
    net: &PyPetriNetBuilder,
    storage: StorageBackends,
    executors: &PyExecutorRegistry,
) -> PyResult<RunHandle> {
    let l = get_running_loop(py)?;
    let asyncio = py.import("asyncio")?;
    ASYNCIO.get_or_init(|| asyncio.unbind());
    RUNNING_LOOP.get_or_init(|| l.unbind());

    let cloned_net = Arc::clone(&net.net);
    let cancel_token = CancellationToken::new();
    let cancel_token_cloned = cancel_token.clone();
    let executors = executors.to_rust_registry();
    let (tx, rx) = oneshot::channel();
    py.allow_threads(|| {
        thread::spawn(|| {
            let rt = Runtime::new().expect("Failed to create tokio runtime");
            let start_res = match storage {
                StorageBackends::Etcd(etcd) => rt.block_on(run_async(
                    cloned_net,
                    ETCDStorageClient::new(etcd.config.clone()),
                    executors,
                    cancel_token_cloned,
                )),
                StorageBackends::InMemory(in_memory) => rt.block_on(run_async(
                    cloned_net,
                    in_memory.storage.clone(),
                    executors,
                    cancel_token_cloned,
                )),
            };

            let res: PyPetriResult<()> = start_res.map_err(|e| e.into());
            let _ = tx.send(res); // we don't really care if anyone is waiting for the result.
        })
    });
    Ok(RunHandle { cancel_token, rx: Some(rx) })
}

#[pyclass]
pub struct RunHandle {
    cancel_token: CancellationToken,
    rx: Option<oneshot::Receiver<PyPetriResult<()>>>,
}

#[pymethods]
impl RunHandle {
    fn cancel(&mut self) {
        self.cancel_token.cancel();
    }

    fn join(&mut self, py: Python<'_>) -> PyPetriResult<()> {
        if let Some(rx) = self.rx.take() {
            py.allow_threads(|| {
                rx.blocking_recv()
                    .map_err(|_| {
                        PyPetriError(PetriError::Other("dipen main thread crashed.".to_string()))
                    })
                    .and_then(|r| r)
            })
        } else {
            Err(PyPetriError(PetriError::Other("Already joining elsewhere".into())))
        }
    }

    fn join_async<'py>(&mut self, py: Python<'py>) -> PyResult<Bound<'py, PyAny>> {
        if let Some(rx) = self.rx.take() {
            pyo3_async_runtimes::tokio::future_into_py(py, async move {
                rx.await
                    .map_err(|_| {
                        PyPetriError(PetriError::Other("dipen main thread crashed.".to_string()))
                    })
                    .and_then(|r| r)
                    .map_err(|e| e.into())
            })
        } else {
            Err(PyPetriError(PetriError::Other("Already joining elsewhere".into())).into())
        }
    }
}
