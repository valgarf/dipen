use dipen::error::PetriError;
use pyo3::{exceptions::PyRuntimeError, PyErr};

pub struct PyPetriError(pub PetriError);

impl From<PyPetriError> for PyErr {
    fn from(error: PyPetriError) -> Self {
        PyRuntimeError::new_err(error.0.to_string())
    }
}

impl From<PetriError> for PyPetriError {
    fn from(other: PetriError) -> Self {
        Self(other)
    }
}

pub type PyPetriResult<T> = Result<T, PyPetriError>;
