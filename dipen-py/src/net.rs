use std::sync::Arc;

use dipen::net::{self, ArcVariant, PetriNetBuilder};
use pyo3::prelude::*;

use crate::PyPetriResult;

#[pyclass(name = "PetriNetBuilder")]
pub struct PyPetriNetBuilder {
    pub net: Arc<net::PetriNetBuilder>,
}

#[pyclass(eq, eq_int, name = "ArcVariant")]
#[derive(PartialEq, Eq, Clone, Copy)]
pub enum PyArcVariant {
    In,
    Out,
    InOut,
    Cond,
    OutCond,
}

impl From<PyArcVariant> for ArcVariant {
    fn from(value: PyArcVariant) -> Self {
        match value {
            PyArcVariant::In => ArcVariant::In,
            PyArcVariant::Out => ArcVariant::Out,
            PyArcVariant::InOut => ArcVariant::InOut,
            PyArcVariant::Cond => ArcVariant::Cond,
            PyArcVariant::OutCond => ArcVariant::OutCond,
        }
    }
}

impl From<ArcVariant> for PyArcVariant {
    fn from(value: ArcVariant) -> Self {
        match value {
            ArcVariant::In => PyArcVariant::In,
            ArcVariant::Out => PyArcVariant::Out,
            ArcVariant::InOut => PyArcVariant::InOut,
            ArcVariant::Cond => PyArcVariant::Cond,
            ArcVariant::OutCond => PyArcVariant::OutCond,
        }
    }
}

#[pymethods]
impl PyPetriNetBuilder {
    #[new]
    fn new() -> Self {
        Self { net: Arc::new(PetriNetBuilder::default()) }
    }
    #[pyo3(signature = (name, output_locking=true))]
    fn insert_place(&mut self, name: &str, output_locking: bool) {
        Arc::<net::PetriNetBuilder>::make_mut(&mut self.net)
            .insert_place(net::Place::new(name, output_locking));
    }

    #[pyo3(signature = (name, region="default"))]
    fn insert_transition(&mut self, name: &str, region: &str) {
        Arc::<net::PetriNetBuilder>::make_mut(&mut self.net)
            .insert_transition(net::Transition::new(name, region));
    }

    #[pyo3(signature = (place, transition, variant, name=""))]
    fn insert_arc(
        &mut self,
        place: &str,
        transition: &str,
        variant: PyArcVariant,
        name: &str,
    ) -> PyPetriResult<()> {
        Arc::<net::PetriNetBuilder>::make_mut(&mut self.net).insert_arc(net::Arc::new(
            place,
            transition,
            variant.into(),
            name,
        ))?;
        Ok(())
    }
}

// helper functions not exposed to python
impl PyPetriNetBuilder {}
