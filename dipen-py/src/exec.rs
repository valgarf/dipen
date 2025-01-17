use std::{
    future::Future,
    sync::{Arc, OnceLock},
};

use crate::{
    contexts::{
        create::PyCreateContext,
        run::{PyRunContext, PyRunResult},
        start::{PyCheckStartResult, PyStartContext},
    },
    registry::PyRegistryMap,
};
use dipen::exec::{CheckStartResult, CreationError, RunResult, TransitionExecutor};
use pyo3::prelude::*;
use tokio::{select, sync::oneshot};
use tokio_util::sync::CancellationToken;
use tracing::warn;
pub struct PyTransitionDispatcher {
    // transition_name: String,
    // transition_id: TransitionId,
    py_obj: PyObject,
}

pub(crate) static ASYNCIO: OnceLock<Py<PyModule>> = OnceLock::new();
pub static RUNNING_LOOP: OnceLock<PyObject> = OnceLock::new();

impl TransitionExecutor for PyTransitionDispatcher {
    fn new(ctx: &impl dipen::exec::CreateContext) -> Result<Self, CreationError>
    where
        Self: Sized,
    {
        let data =
            ctx.registry_data().ok_or_else(|| CreationError::new("Missing registry data"))?;
        let data: Arc<PyRegistryMap> = data.downcast().map_err(|err| {
            CreationError::new(format!("Registry data has wrong type: {:?}", err))
        })?;
        match data.get(ctx.transition_name()) {
            None => Err(CreationError::new("create failed, transitionmissing from registry.")),
            Some((py_cls, _py_data)) => {
                // store py data in PyValidateContext?
                let py_ctx = PyCreateContext::new(ctx);
                Python::with_gil(|py| {
                    let res = py_cls
                        .bind(py)
                        .call1((py_ctx,))
                        .map_err(|py_err| {
                            CreationError::new(format!(
                                "Creation of python instance failed: {}",
                                py_err // TODO: format traceback!
                            ))
                        })?
                        .unbind();
                    Ok(Self {
                        // transition_name: ctx.transition_name().into(),
                        // transition_id: ctx.transition_id(),
                        py_obj: res,
                    })
                })
            }
        }
    }

    fn check_start(&mut self, ctx: &mut impl dipen::exec::StartContext) -> CheckStartResult {
        let res: PyCheckStartResult = Python::with_gil(|py| {
            let py_ctx = PyStartContext::new(ctx);
            self.py_obj
                .bind(py)
                .call_method1("check_start", (py_ctx,))
                .expect("Check start failed. TODO: error handling for python exceptions.")
                .extract()
                .expect("Unwrapping PyCheckStartResult failed. TODO: error handling for python exceptions.")
        });
        res.inner
    }

    async fn run(&mut self, ctx: &mut impl dipen::exec::RunContext) -> RunResult {
        let fut = Python::with_gil(|py| {
            let py_ctx = PyRunContext::new(ctx);
            let awaitable = self.py_obj.bind(py).call_method1("run", (py_ctx,)).expect(
                "Creating run coroutine failed. TODO: error handling for python exceptions.",
            );
            into_cancellable_future_with_locals(ctx.cancellation_token(), awaitable)
                .expect("Conversion to future failed. TODO: error handling for python exceptions.")
        });
        // Note: needs to be spawned, on cancellation this function is not polled from rust anymore.
        let py_res = tokio::spawn(fut)
            .await
            .expect("Join failed. TODO: error handling for python exceptions.")
            .expect("Running failed. TODO: error handling for python exceptions.");
        let res: PyRunResult = Python::with_gil(|py| {
            py_res.bind(py).extract().expect(
                "Unwrapping PyRunResult failed. TODO: error handling for python exceptions.",
            )
        });
        res.inner
    }
}

// Code below is adapted from pyo3_async_runtimes' into_future_with_locals function.
// The function below does not take any context byt the python future is cancelled when the input
// CancellationToken is cancelled.

pub fn into_cancellable_future_with_locals(
    cancel_token: CancellationToken,
    awaitable: Bound<PyAny>,
) -> PyResult<impl Future<Output = PyResult<PyObject>> + Send> {
    let py = awaitable.py();
    let (tx, mut rx) = oneshot::channel();

    let py_fut =
        asyncio(py).call_method1("run_coroutine_threadsafe", (awaitable, event_loop(py)))?;
    let on_complete = PyTaskCompleter { tx: Some(tx) };
    py_fut.call_method1("add_done_callback", (on_complete,))?;

    let unbound_fut = py_fut.unbind();
    Ok(async move {
        select! {
            res = &mut rx => { match res {
                Ok(item) => item,
                Err(_) => Python::with_gil(|py| {
                    Err(PyErr::from_value(asyncio(py).call_method0("CancelledError")?))
                })
            }},
            _ = cancel_token.cancelled() => {
                warn!("Running python transition is being cancelled.");
                Python::with_gil(|py| {
                    unbound_fut.bind(py).call_method0("cancel")?;
                    Err(PyErr::from_value(asyncio(py).call_method0("CancelledError")?))
                })
            }
        }
    })
}

fn asyncio(py: Python<'_>) -> Bound<'_, PyModule> {
    ASYNCIO.get().unwrap().clone_ref(py).into_bound(py)
}

fn event_loop(py: Python<'_>) -> Bound<'_, PyAny> {
    RUNNING_LOOP.get().unwrap().clone_ref(py).into_bound(py)
}

#[pyclass]
struct PyTaskCompleter {
    tx: Option<oneshot::Sender<PyResult<PyObject>>>,
}

#[pymethods]
impl PyTaskCompleter {
    #[pyo3(signature = (task))]
    pub fn __call__(&mut self, task: &Bound<PyAny>) -> PyResult<()> {
        debug_assert!(task.call_method0("done")?.extract()?);
        let result = match task.call_method0("result") {
            Ok(val) => Ok(val.into()),
            Err(e) => Err(e),
        };

        // unclear to me whether or not this should be a panic or silent error.
        //
        // calling PyTaskCompleter twice should not be possible, but I don't think it really hurts
        // anything if it happens.
        if let Some(tx) = self.tx.take() {
            if tx.send(result).is_err() {
                // cancellation is not an error
            }
        }

        Ok(())
    }
}
