use dipen::{
    exec::{ValidateArcContext, ValidateContext, ValidatePlaceContext, ValidationResult},
    net,
};
use pyo3::prelude::*;

use crate::net::PyArcVariant;

#[pyclass(name = "ValidateContext")]
#[derive(Clone)]
pub struct PyValidateContext {
    pub transition_name: String,
    pub arcs: Vec<PyValidateArcContext>,
}

#[pyclass(name = "ValidateArcContext")]
#[derive(Clone)]
pub struct PyValidateArcContext {
    pub arc_name: String,
    pub arc_variant: net::ArcVariant,
    pub place: PyValidatePlaceContext,
}

#[pyclass(name = "ValidatePlaceContext")]
#[derive(Clone)]
pub struct PyValidatePlaceContext {
    place_name: String,
}

#[pyclass(name = "ValidationResult")]
#[derive(Clone)]
pub struct PyValidationResult {
    pub inner: ValidationResult,
}

impl PyValidateContext {
    pub fn new(value: &impl ValidateContext) -> Self {
        Self {
            transition_name: value.transition_name().into(),
            arcs: value.arcs().map(|a| PyValidateArcContext::new(&a)).collect(),
        }
    }
}

impl PyValidateArcContext {
    pub fn new(value: &impl ValidateArcContext) -> Self {
        Self {
            arc_name: value.arc_name().into(),
            arc_variant: value.variant(),
            place: PyValidatePlaceContext::new(&value.place_context()),
        }
    }
}

impl PyValidatePlaceContext {
    pub fn new(value: &impl ValidatePlaceContext) -> Self {
        Self { place_name: value.place_name().into() }
    }
}

#[pymethods]
impl PyValidateContext {
    #[getter]
    fn transition_name(&self) -> &str {
        &self.transition_name
    }

    #[getter]
    fn arcs(&self) -> Vec<PyValidateArcContext> {
        self.arcs.clone()
    }

    #[getter]
    fn arcs_in(&self) -> Vec<PyValidateArcContext> {
        self.arcs.iter().filter(|a| a.variant().is_in()).cloned().collect()
    }
    #[getter]
    fn arcs_out(&self) -> Vec<PyValidateArcContext> {
        self.arcs.iter().filter(|a| a.variant().is_out()).cloned().collect()
    }
    #[getter]
    fn arcs_cond(&self) -> Vec<PyValidateArcContext> {
        self.arcs.iter().filter(|a| a.variant().is_cond()).cloned().collect()
    }
    fn arcs_by_name(&self, name: &str) -> Vec<PyValidateArcContext> {
        self.arcs.iter().filter(move |a| a.arc_name() == name).cloned().collect()
    }
    fn arcs_by_place_name(&self, name: &str) -> Vec<PyValidateArcContext> {
        self.arcs.iter().filter(move |a| a.place_context().place_name() == name).cloned().collect()
    }
}

#[pymethods]
impl PyValidateArcContext {
    #[getter]
    fn arc_name(&self) -> &str {
        &self.arc_name
    }

    #[getter]
    fn variant(&self) -> PyArcVariant {
        self.arc_variant.into()
    }

    #[getter]
    fn place_context(&self) -> PyValidatePlaceContext {
        self.place.clone()
    }
}

#[pymethods]
impl PyValidatePlaceContext {
    #[getter]
    fn place_name(&self) -> &str {
        &self.place_name
    }
}

#[pymethods]
impl PyValidationResult {
    #[staticmethod]
    pub fn succeeded() -> PyValidationResult {
        PyValidationResult { inner: ValidationResult::succeeded() }
    }

    #[staticmethod]
    pub fn failed(reason: &str) -> PyValidationResult {
        PyValidationResult { inner: ValidationResult::failed(reason) }
    }
}
