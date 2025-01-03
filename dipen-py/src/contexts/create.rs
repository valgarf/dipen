use std::{any::Any, sync::Arc};

use dipen::{
    exec::{CreateArcContext, CreateContext, CreatePlaceContext},
    net::{self, PlaceId, TransitionId},
};
use pyo3::prelude::*;

use crate::net::PyArcVariant;

#[pyclass(name = "CreateContext")]
#[derive(Clone)]
pub struct PyCreateContext {
    pub transition_name: String,
    pub transition_id: TransitionId,
    pub arcs: Vec<PyCreateArcContext>,
    pub registry_data: Option<Arc<dyn Any + Send + Sync>>, // TODO: expose
}

#[pyclass(name = "CreateArcContext")]
#[derive(Clone)]
pub struct PyCreateArcContext {
    pub arc_name: String,
    pub arc_variant: net::ArcVariant,
    pub place: PyCreatePlaceContext,
}

#[pyclass(name = "CreatePlaceContext")]
#[derive(Clone)]
pub struct PyCreatePlaceContext {
    place_name: String,
    place_id: PlaceId,
}

impl PyCreateContext {
    pub fn new(value: &impl CreateContext) -> Self {
        Self {
            transition_name: value.transition_name().into(),
            transition_id: value.transition_id(),
            arcs: value.arcs().map(|a| PyCreateArcContext::new(&a)).collect(),
            registry_data: value.registry_data(),
        }
    }
}

impl PyCreateArcContext {
    pub fn new(value: &impl CreateArcContext) -> Self {
        Self {
            arc_name: value.arc_name().into(),
            arc_variant: value.variant(),
            place: PyCreatePlaceContext::new(&value.place_context()),
        }
    }
}

impl PyCreatePlaceContext {
    pub fn new(value: &impl CreatePlaceContext) -> Self {
        Self { place_name: value.place_name().into(), place_id: value.place_id() }
    }
}

#[pymethods]
impl PyCreateContext {
    #[getter]
    fn transition_name(&self) -> &str {
        &self.transition_name
    }

    #[getter]
    fn transition_id(&self) -> u64 {
        self.transition_id.0
    }

    #[getter]
    fn arcs(&self) -> Vec<PyCreateArcContext> {
        self.arcs.clone()
    }

    #[getter]
    fn arcs_in(&self) -> Vec<PyCreateArcContext> {
        self.arcs.iter().filter(|a| a.variant().is_in()).cloned().collect()
    }
    #[getter]
    fn arcs_out(&self) -> Vec<PyCreateArcContext> {
        self.arcs.iter().filter(|a| a.variant().is_out()).cloned().collect()
    }
    #[getter]
    fn arcs_cond(&self) -> Vec<PyCreateArcContext> {
        self.arcs.iter().filter(|a| a.variant().is_cond()).cloned().collect()
    }
    fn arcs_by_name(&self, name: &str) -> Vec<PyCreateArcContext> {
        self.arcs.iter().filter(move |a| a.arc_name() == name).cloned().collect()
    }
    fn arcs_by_place_name(&self, name: &str) -> Vec<PyCreateArcContext> {
        self.arcs.iter().filter(move |a| a.place_context().place_name() == name).cloned().collect()
    }
}

#[pymethods]
impl PyCreateArcContext {
    #[getter]
    fn arc_name(&self) -> &str {
        &self.arc_name
    }

    #[getter]
    fn variant(&self) -> PyArcVariant {
        self.arc_variant.into()
    }

    #[getter]
    fn place_context(&self) -> PyCreatePlaceContext {
        self.place.clone()
    }
}

#[pymethods]
impl PyCreatePlaceContext {
    #[getter]
    fn place_name(&self) -> &str {
        &self.place_name
    }
    #[getter]
    fn place_id(&self) -> u64 {
        self.place_id.0
    }
}
