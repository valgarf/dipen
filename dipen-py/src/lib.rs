mod endless_loop;
mod error;
mod logging;
mod net;

use std::sync::Arc;

use crate::endless_loop::main;
use error::*;
use logging::RustTracingToLoguru;
use net::{PyArcVariant, PyPetriNetBuilder};
use pyo3::prelude::*;

/// Formats the sum of two numbers as string.
#[pyfunction]
fn sum_as_string(a: usize, b: usize) -> PyResult<String> {
    Ok((a + b).to_string())
}

#[pyfunction]
fn run(py: Python<'_>, net: &PyPetriNetBuilder) -> PyPetriResult<()> {
    py.allow_threads(|| main(Arc::clone(&net.net)).map_err(|e| e.into()))
}

/// A Python module implemented in Rust.
#[pymodule]
fn dipen(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_function(wrap_pyfunction!(sum_as_string, m)?)?;
    m.add_function(wrap_pyfunction!(run, m)?)?;
    m.add_class::<RustTracingToLoguru>()?;
    m.add_class::<PyPetriNetBuilder>()?;
    m.add_class::<PyArcVariant>()?;
    Ok(())
}
