mod endless_loop;
mod error;
mod etcd;
mod logging;
mod net;
mod run;

use error::*;
use etcd::PyETCDGateConfig;
use logging::RustTracingToLoguru;
use net::{PyArcVariant, PyPetriNetBuilder};
use pyo3::prelude::*;

#[pymodule]
fn dipen(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_function(wrap_pyfunction!(run::run, m)?)?;
    m.add_class::<RustTracingToLoguru>()?;
    m.add_class::<PyPetriNetBuilder>()?;
    m.add_class::<PyArcVariant>()?;
    m.add_class::<PyETCDGateConfig>()?;
    Ok(())
}
