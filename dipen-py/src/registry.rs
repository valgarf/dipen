use std::{any::Any, collections::HashMap, sync::Arc};

use dipen::{error::PetriError, runner::ExecutorRegistry};
use pyo3::{prelude::*, types::PyType};

use crate::{exec::PyTransitionDispatcher, PyPetriError};

pub type PyRegistryMap = HashMap<String, (Py<PyType>, PyObject)>;
#[pyclass(name = "ExecutorRegistry")]
#[derive(Default)]
pub struct PyExecutorRegistry {
    pub transitions: Arc<PyRegistryMap>,
}

#[pymethods]
impl PyExecutorRegistry {
    #[new]
    pub fn new() -> Self {
        Self::default()
    }
    pub fn register(
        &mut self,
        transition_name: String,
        transition_class: Py<PyType>,
        data: PyObject,
    ) -> PyResult<()> {
        Arc::get_mut(&mut self.transitions)
            .ok_or(PyPetriError(PetriError::Other(
                "Cannot register any more transitions, registry is in use.".into(),
            )))?
            .insert(transition_name, (transition_class, data));
        Ok(())
    }
}

impl PyExecutorRegistry {
    pub fn to_rust_registry(&self) -> ExecutorRegistry {
        let mut registry = ExecutorRegistry::new();
        for tr_name in self.transitions.keys() {
            registry.register::<PyTransitionDispatcher>(
                tr_name,
                Some(Arc::clone(&self.transitions) as Arc<dyn Any + Send + Sync>),
            );
        }
        registry
    }
}
