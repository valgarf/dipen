use dipen::{
    error::PetriError,
    exec::{RunContext, RunResult, RunResultBuilder, RunTokenContext},
    net::{self, PlaceId},
};
use pyo3::prelude::*;

use crate::{PyPetriError, PyPetriResult};

#[pyclass(name = "RunTokenContext")]
#[derive(Clone)]
pub struct PyRunTokenContext {
    token_id: net::TokenId,
    data: Vec<u8>,
    orig_place_id: net::PlaceId,
}
#[pyclass(name = "RunContext")]
#[derive(Clone)]
pub struct PyRunContext {
    tokens: Vec<PyRunTokenContext>,
}

#[pymethods]
impl PyRunTokenContext {
    #[getter]
    fn token_id(&self) -> u64 {
        self.token_id.0
    }
    #[getter]
    fn data(&self) -> &[u8] {
        &self.data
    }
    #[getter]
    fn orig_place_id(&self) -> u64 {
        self.orig_place_id.0
    }
}

#[pymethods]
impl PyRunContext {
    #[getter]
    fn tokens(&self) -> Vec<PyRunTokenContext> {
        self.tokens.clone()
    }
}

impl RunTokenContext for PyRunTokenContext {
    fn token_id(&self) -> net::TokenId {
        self.token_id
    }

    fn data(&self) -> &[u8] {
        &self.data
    }

    fn orig_place_id(&self) -> net::PlaceId {
        self.orig_place_id
    }
}

impl PyRunTokenContext {
    pub fn new(ctx: &impl RunTokenContext) -> Self {
        Self {
            token_id: ctx.token_id(),
            data: ctx.data().into(),
            orig_place_id: ctx.orig_place_id(),
        }
    }
}

impl PyRunContext {
    pub fn new(ctx: &impl RunContext) -> Self {
        Self { tokens: ctx.tokens().map(PyRunTokenContext::new).collect() }
    }
}

#[pyclass(name = "RunResultBuilder")]
#[derive(Default)]
pub struct PyRunResultBuilder {
    inner: Option<RunResultBuilder>,
}

#[pyclass(name = "RunResult")]
#[derive(Clone)]
pub struct PyRunResult {
    pub inner: RunResult,
}

#[pymethods]
impl PyRunResult {
    #[staticmethod]
    pub fn build() -> PyRunResultBuilder {
        PyRunResultBuilder { inner: Some(RunResult::build()) }
    }
}

impl PyRunResultBuilder {
    fn get_inner_mut(&mut self) -> PyPetriResult<&mut RunResultBuilder> {
        self.inner.as_mut().ok_or(PyPetriError(PetriError::Other(
            "Failed to access PyRunResultBuilder.inner".into(),
        )))
    }
    fn take_inner(&mut self) -> PyPetriResult<RunResultBuilder> {
        self.inner.take().ok_or(PyPetriError(PetriError::Other(
            "Failed to access PyRunResultBuilder.inner".into(),
        )))
    }
}
#[pymethods]
impl PyRunResultBuilder {
    pub fn place(&mut self, token: &PyRunTokenContext, place_id: u64) -> PyResult<()> {
        self.get_inner_mut()?.place(token, PlaceId(place_id));
        Ok(())
    }

    pub fn update(&mut self, token: &PyRunTokenContext, data: Vec<u8>) -> PyResult<()> {
        self.get_inner_mut()?.update(token, data);
        Ok(())
    }

    pub fn place_new(&mut self, place_id: u64, data: Vec<u8>) -> PyResult<()> {
        self.get_inner_mut()?.place_new(PlaceId(place_id), data);
        Ok(())
    }

    pub fn result(&mut self) -> PyResult<PyRunResult> {
        Ok(PyRunResult { inner: self.take_inner()?.result() })
    }
}
