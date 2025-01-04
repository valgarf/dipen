use std::{collections::HashMap, time::Duration};

use dipen::{
    error::PetriError,
    exec::{
        CheckStartResult, CheckStartResultBuilder, StartContext, StartTakenTokenContext,
        StartTokenContext,
    },
    net::{self},
};
use pyo3::prelude::*;

use crate::{PyPetriError, PyPetriResult};

#[pyclass(name = "StartTokenContext")]
#[derive(Clone)]
pub struct PyStartTokenContext {
    token_id: net::TokenId,
    place_id: net::PlaceId,
    data: Vec<u8>,
}
#[pyclass(name = "StartTakenTokenContext")]
#[derive(Clone)]
pub struct PyStartTakenTokenContext {
    token_id: net::TokenId,
    place_id: net::PlaceId,
    transition_id: net::TransitionId,
    data: Vec<u8>,
}

#[pyclass(name = "StartContext")]
#[derive(Clone)]
pub struct PyStartContext {
    tokens_at: HashMap<net::PlaceId, Vec<PyStartTokenContext>>,
    taken_tokens_at: HashMap<net::PlaceId, Vec<PyStartTakenTokenContext>>,
}

#[pyclass(name = "CheckStartResultBuilder")]
#[derive(Default)]
pub struct PyCheckStartResultBuilder {
    inner: Option<CheckStartResultBuilder>,
}

#[pyclass(name = "CheckStartResult")]
#[derive(Clone)]
pub struct PyCheckStartResult {
    pub inner: CheckStartResult,
}

impl StartTokenContext for PyStartTokenContext {
    fn token_id(&self) -> net::TokenId {
        self.token_id
    }

    fn place_id(&self) -> net::PlaceId {
        self.place_id
    }

    fn data(&self) -> &[u8] {
        &self.data
    }
}

#[pymethods]
impl PyStartTokenContext {
    #[getter]
    fn token_id(&self) -> u64 {
        self.token_id.0
    }

    #[getter]
    fn place_id(&self) -> u64 {
        self.place_id.0
    }

    #[getter]
    fn data(&self) -> &[u8] {
        &self.data
    }
}

#[pymethods]
impl PyStartTakenTokenContext {
    #[getter]
    fn token_id(&self) -> u64 {
        self.token_id.0
    }

    #[getter]
    fn place_id(&self) -> u64 {
        self.place_id.0
    }

    #[getter]
    fn transition_id(&self) -> u64 {
        self.transition_id.0
    }

    #[getter]
    fn data(&self) -> &[u8] {
        &self.data
    }
}

#[pymethods]
impl PyStartContext {
    fn tokens_at(&self, place_id: u64) -> Vec<PyStartTokenContext> {
        self.tokens_at.get(&net::PlaceId(place_id)).unwrap_or(&vec![]).clone()
    }
    fn taken_tokens_at(&self, place_id: u64) -> Vec<PyStartTakenTokenContext> {
        self.taken_tokens_at.get(&net::PlaceId(place_id)).unwrap_or(&vec![]).clone()
    }
    #[getter]
    fn tokens(&self) -> Vec<PyStartTokenContext> {
        self.tokens_at.values().flatten().cloned().collect()
    }
    #[getter]
    fn taken_tokens(&self) -> Vec<PyStartTakenTokenContext> {
        self.taken_tokens_at.values().flatten().cloned().collect()
    }
}

impl PyStartContext {
    pub fn new(ctx: &impl StartContext) -> Self {
        let mut tokens_at: HashMap<net::PlaceId, Vec<PyStartTokenContext>> = HashMap::new();
        for t in ctx.tokens() {
            tokens_at.entry(t.place_id()).or_default().push(PyStartTokenContext::new(&t));
        }
        let mut taken_tokens_at: HashMap<net::PlaceId, Vec<PyStartTakenTokenContext>> =
            HashMap::new();
        for t in ctx.taken_tokens() {
            taken_tokens_at
                .entry(t.place_id())
                .or_default()
                .push(PyStartTakenTokenContext::new(&t));
        }
        Self { tokens_at, taken_tokens_at }
    }
}

impl PyStartTokenContext {
    pub fn new(ctx: &impl StartTokenContext) -> Self {
        Self { data: ctx.data().into(), place_id: ctx.place_id(), token_id: ctx.token_id() }
    }
}

impl PyStartTakenTokenContext {
    pub fn new(ctx: &impl StartTakenTokenContext) -> Self {
        Self {
            data: ctx.data().into(),
            place_id: ctx.place_id(),
            token_id: ctx.token_id(),
            transition_id: ctx.transition_id(),
        }
    }
}

#[pymethods]
impl PyCheckStartResult {
    #[staticmethod]
    pub fn build() -> PyCheckStartResultBuilder {
        PyCheckStartResultBuilder { inner: Some(CheckStartResult::build()) }
    }
}

impl PyCheckStartResultBuilder {
    fn get_inner_mut(&mut self) -> PyPetriResult<&mut CheckStartResultBuilder> {
        self.inner.as_mut().ok_or(PyPetriError(PetriError::Other(
            "Failed to access PyCheckStartResultBuilder.inner".into(),
        )))
    }
    fn take_inner(&mut self) -> PyPetriResult<CheckStartResultBuilder> {
        self.inner.take().ok_or(PyPetriError(PetriError::Other(
            "Failed to access PyCheckStartResultBuilder.inner".into(),
        )))
    }
}
#[pymethods]
impl PyCheckStartResultBuilder {
    pub fn take(&mut self, token: &PyStartTokenContext) -> PyResult<()> {
        self.get_inner_mut()?.take(token);
        Ok(())
    }

    pub fn enabled(&mut self) -> PyResult<PyCheckStartResult> {
        Ok(PyCheckStartResult { inner: self.take_inner()?.enabled() })
    }

    #[pyo3(signature = (wait_for=None, auto_recheck=None))]
    pub fn disabled(
        &mut self,
        wait_for: Option<u64>,
        auto_recheck: Option<Duration>,
    ) -> PyResult<PyCheckStartResult> {
        Ok(PyCheckStartResult {
            inner: self.take_inner()?.disabled(wait_for.map(net::PlaceId), auto_recheck),
        })
    }
}
