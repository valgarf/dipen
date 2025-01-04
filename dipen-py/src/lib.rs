mod asyncio;
mod contexts;
mod error;
mod etcd;
mod exec;
mod logging;
mod net;
mod registry;
mod run;

use contexts::{
    create::{PyCreateArcContext, PyCreateContext, PyCreatePlaceContext},
    run::{PyRunContext, PyRunResult, PyRunResultBuilder, PyRunTokenContext},
    start::{
        PyCheckStartResult, PyCheckStartResultBuilder, PyStartContext, PyStartTakenTokenContext,
        PyStartTokenContext,
    },
    validate::{
        PyValidateArcContext, PyValidateContext, PyValidatePlaceContext, PyValidationResult,
    },
};
use error::*;
use etcd::PyETCDGateConfig;
use logging::RustTracingToLoguru;
use net::{PyArcVariant, PyPetriNetBuilder};
use pyo3::prelude::*;
use registry::PyExecutorRegistry;
use run::RunHandle;

#[pymodule]
fn _dipen_py_internal(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_function(wrap_pyfunction!(run::start, m)?)?;
    m.add_class::<RustTracingToLoguru>()?;
    m.add_class::<PyPetriNetBuilder>()?;
    m.add_class::<PyArcVariant>()?;
    m.add_class::<PyETCDGateConfig>()?;
    m.add_class::<PyValidateContext>()?;
    m.add_class::<PyValidateArcContext>()?;
    m.add_class::<PyValidatePlaceContext>()?;
    m.add_class::<PyValidationResult>()?;
    m.add_class::<PyCreateContext>()?;
    m.add_class::<PyCreateArcContext>()?;
    m.add_class::<PyCreatePlaceContext>()?;
    m.add_class::<PyStartContext>()?;
    m.add_class::<PyStartTokenContext>()?;
    m.add_class::<PyStartTakenTokenContext>()?;
    m.add_class::<PyCheckStartResult>()?;
    m.add_class::<PyCheckStartResultBuilder>()?;
    m.add_class::<PyRunContext>()?;
    m.add_class::<PyRunTokenContext>()?;
    m.add_class::<PyRunResult>()?;
    m.add_class::<PyRunResultBuilder>()?;
    m.add_class::<PyExecutorRegistry>()?;
    m.add_class::<RunHandle>()?;
    Ok(())
}
