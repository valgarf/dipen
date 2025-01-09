use std::sync::{Arc, OnceLock};

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
use pyo3_async_runtimes::{into_future_with_locals, TaskLocals};
pub struct PyTransitionDispatcher {
    // transition_name: String,
    // transition_id: TransitionId,
    py_obj: PyObject,
}

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
            let locals = TaskLocals::new(RUNNING_LOOP.get().unwrap().bind(py).clone());
            into_future_with_locals(&locals, awaitable)
                .expect("Conversion to future failed. TODO: error handling for python exceptions.")
        });
        let py_res =
            fut.await.expect("Running failed. TODO: error handling for python exceptions.");
        let res: PyRunResult = Python::with_gil(|py| {
            py_res.bind(py).extract().expect(
                "Unwrapping PyRunResult failed. TODO: error handling for python exceptions.",
            )
        });
        res.inner
    }
}
