use std::sync::{Arc, OnceLock};

use crate::{
    contexts::{
        create::PyCreateContext,
        run::{PyRunContext, PyRunResult},
        start::{PyCheckStartResult, PyStartContext},
        validate::{PyValidateContext, PyValidationResult},
    },
    registry::PyRegistryMap,
};
use dipen::exec::{CheckStartResult, RunResult, TransitionExecutor, ValidationResult};
use pyo3::prelude::*;
use pyo3_async_runtimes::{into_future_with_locals, TaskLocals};
pub struct PyTransitionDispatcher {
    // transition_name: String,
    // transition_id: TransitionId,
    py_obj: PyObject,
}

pub static RUNNING_LOOP: OnceLock<PyObject> = OnceLock::new();

impl TransitionExecutor for PyTransitionDispatcher {
    fn validate(ctx: &impl dipen::exec::ValidateContext) -> ValidationResult
    where
        Self: Sized,
    {
        let data = ctx.registry_data().expect("Missing registry data");
        let data: Arc<PyRegistryMap> = data.downcast().expect("Registry data has wrong type");
        match data.get(ctx.transition_name()) {
            None => {
                ValidationResult::failed(format!("Transition '{}' missing", ctx.transition_name()))
            }
            Some((py_cls, _py_data)) => {
                // store py data in PyValidateContext?
                let py_ctx = PyValidateContext::new(ctx);
                Python::with_gil(|py| {
                    let validate_res = py_cls
                        .bind(py)
                        .call_method1("validate", (py_ctx,))
                        .and_then(|py_obj| py_obj.extract::<PyValidationResult>());

                    match validate_res {
                        Err(py_err) => ValidationResult::failed(py_err.to_string()),
                        Ok(res) => res.inner,
                    }
                })
            }
        }
    }

    fn new(ctx: &impl dipen::exec::CreateContext) -> Self
    where
        Self: Sized,
    {
        let data = ctx.registry_data().expect("Missing registry data");
        let data: Arc<PyRegistryMap> = data.downcast().expect("Registry data has wrong type");
        match data.get(ctx.transition_name()) {
            None => {
                panic!("create failed, transition {} missing from registry. TODO: error handling for python exceptions.", ctx.transition_name())
            }
            Some((py_cls, _py_data)) => {
                // store py data in PyValidateContext?
                let py_ctx = PyCreateContext::new(ctx);
                Python::with_gil(|py| {
                    let res = py_cls
                        .bind(py)
                        .call1((py_ctx,))
                        .expect(
                            "creating instance failed. TODO: error handling for python exceptions.",
                        )
                        .unbind();
                    Self {
                        // transition_name: ctx.transition_name().into(),
                        // transition_id: ctx.transition_id(),
                        py_obj: res,
                    }
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
