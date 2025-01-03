use std::sync::Arc;

use crate::endless_loop::run_example;
use crate::error::*;
use crate::etcd::PyETCDGateConfig;
use crate::net::PyPetriNetBuilder;
use dipen::etcd::ETCDGate;
use pyo3::prelude::*;
use tokio::runtime::Runtime;

#[pyfunction]
pub fn run(py: Python<'_>, net: &PyPetriNetBuilder, etcd: &PyETCDGateConfig) -> PyPetriResult<()> {
    let cloned_net = Arc::clone(&net.net);
    let etcd = etcd.config.clone();
    py.allow_threads(|| {
        let rt = Runtime::new().expect("Failed to create tokio runtime");
        rt.block_on(run_example(cloned_net, ETCDGate::new(etcd))).map_err(|e| e.into())
    })
}
