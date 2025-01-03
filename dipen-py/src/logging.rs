use std::{collections::HashMap, ffi::CStr, str::FromStr};

use dipen::error::PetriError;
use pyo3::{ffi::c_str, intern, prelude::*, types::PyDict};

use dipen::Result;
use tracing::Level;
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt, Layer};

use crate::{PyPetriError, PyPetriResult};

struct PyDictVisitor<'a> {
    fields: Bound<'a, PyDict>,
}

macro_rules! set_field {
    ($dict: expr, $key: expr, $value: expr) => {
        if let Err(err) = $dict.set_item($key, $value) {
            print!(
                "Unable to write field '{}' (value={:?}) into a python dictionary, error: {}",
                $key, $value, err
            )
        }
    };
}

impl tracing::field::Visit for PyDictVisitor<'_> {
    fn record_f64(&mut self, field: &tracing::field::Field, value: f64) {
        set_field!(self.fields, field.name(), value);
    }

    fn record_i64(&mut self, field: &tracing::field::Field, value: i64) {
        set_field!(self.fields, field.name(), value);
    }

    fn record_u64(&mut self, field: &tracing::field::Field, value: u64) {
        set_field!(self.fields, field.name(), value);
    }

    fn record_bool(&mut self, field: &tracing::field::Field, value: bool) {
        set_field!(self.fields, field.name(), value);
    }

    fn record_str(&mut self, field: &tracing::field::Field, value: &str) {
        set_field!(self.fields, field.name(), value);
    }

    fn record_error(
        &mut self,
        field: &tracing::field::Field,
        value: &(dyn std::error::Error + 'static),
    ) {
        set_field!(self.fields, field.name(), value.to_string());
    }

    fn record_debug(&mut self, field: &tracing::field::Field, value: &dyn std::fmt::Debug) {
        set_field!(self.fields, field.name(), format!("{:?}", value));
    }
}

#[pyclass]
pub struct RustTracingToLoguru {
    pub global_level: Level,
    pub target_levels: HashMap<String, Level>,
    pub logger: Py<PyAny>,
}

impl<S> Layer<S> for RustTracingToLoguru
where
    S: tracing::Subscriber,
{
    fn on_event(&self, event: &tracing::Event<'_>, ctx: tracing_subscriber::layer::Context<'_, S>) {
        if !self.enabled(event.metadata(), ctx) {
            // Note: the comparison is correct, more verbose logging levels seem to be considered
            // larger than the less verbose ones in the tracing library.
            return;
        }
        Python::with_gil(|py| {
            let fields = PyDict::new(py);
            let mut visitor = PyDictVisitor { fields };
            event.record(&mut visitor);
            let PyDictVisitor { fields } = visitor;
            let msg = fields
                .as_any() //PyAny::get_item(fields, "message")
                .get_item("message")
                .unwrap_or(intern!(py, "<log message missing>").as_any().clone());
            let record = PyDict::new(py);
            let mut level = event.metadata().level().to_string();
            if level == "WARN" {
                level = "WARNING".into()
            }
            set_field!(record, intern!(py, "level"), &level);
            set_field!(record, intern!(py, "target"), event.metadata().target().to_string());
            set_field!(record, intern!(py, "name"), event.metadata().name());
            set_field!(record, intern!(py, "file"), event.metadata().file());
            set_field!(record, intern!(py, "line"), event.metadata().line());
            set_field!(record, intern!(py, "module_path"), event.metadata().module_path());
            set_field!(record, intern!(py, "fields"), &fields);
            let kwargs = PyDict::new(py);
            set_field!(kwargs, intern!(py, "_rust_record"), &record);
            if let Err(err) =
                self.logger.call_method(py, intern!(py, "log"), (&level, msg), Some(&kwargs))
            {
                println!("Unable to log message with loguru. Error: {}", err)
            }
            // let _ = py.run(
            //     c_str!("logger.log(record['level'],record['fields']['message'])"),
            //     None,
            //     Some(&locals),
            // );
            // logger.log(record['level'],record['fields']['message'])
        });
        // println!("  level={:?}", event.metadata().level());
        // println!("  target={:?}", event.metadata().target());
        // println!("  name={:?}", event.metadata().name());
        // for field in event.fields() {
        //     println!("  field={}", field.name());
        // }
    }

    fn enabled(
        &self,
        metadata: &tracing::Metadata<'_>,
        _: tracing_subscriber::layer::Context<'_, S>,
    ) -> bool {
        let min_level = self
            .target_levels
            .iter()
            .find(|&(prefix, _)| metadata.target().starts_with(prefix))
            .map(|(_, &level)| level)
            .unwrap_or(self.global_level);
        *metadata.level() <= min_level
        // Note: the comparison is correct, more verbose logging levels seem to be considered
        // larger than the less verbose ones in the tracing library.
    }
}

const CODE: &CStr = c_str!(
    r#"
from pathlib import Path

class ModuleMock:
    def __init__(self, name):
        self.__name__ = name

def patch(record):
    rust = record.get('extra',{}).get('_rust_record')
    if rust is None:
        print("No record ?!?!", )
        return
    
    if (f:=rust.get('file')) is not None:
        p = Path(f)
        record['file'].name = p.name
        record['file'].path = rust.get('file')
    elif not isinstance(record['file'], str):
        record['file'].name = "<rust file>"
        record['file'].path = "<rust file>"
    record['line'] = rust.get('line', 0)

    # not quite sure what to put into 'module'
    # record['module'] = ModuleMock(rust.get('module_path', '__rust__'))

    record['name'] = rust.get('module_path', '__rust__')
    record['function'] = '<rust>'
"#
);

impl RustTracingToLoguru {
    fn clone_ref(&self, py: Python<'_>) -> Self {
        Self {
            global_level: self.global_level,
            target_levels: self.target_levels.clone(),
            logger: self.logger.clone_ref(py),
        }
    }
}

#[pymethods]
impl RustTracingToLoguru {
    #[new]
    fn new(py: Python<'_>) -> PyResult<Self> {
        let loguru = py.import("loguru")?;
        let module = PyModule::from_code(py, CODE, c_str!(""), c_str!(""))?;
        let logger =
            loguru.getattr("logger")?.call_method1("patch", (module.getattr("patch")?,))?;
        Ok(RustTracingToLoguru {
            global_level: Level::INFO,
            logger: logger.as_any().clone().into(),
            target_levels: Default::default(),
        })
    }

    #[getter]
    fn get_log_level(&self) -> &'static str {
        _log_level_to_str(self.global_level)
    }

    #[setter]
    fn set_log_level(&mut self, value: &str) -> PyPetriResult<()> {
        self.global_level = _parse_log_level(value)?;
        Ok(())
    }

    fn set_target_log_level(&mut self, prefix: &str, value: &str) -> PyPetriResult<()> {
        self.target_levels.insert(prefix.into(), _parse_log_level(value)?);
        Ok(())
    }

    fn get_target_log_level(&mut self, prefix: &str) -> Option<&'static str> {
        self.target_levels.get(prefix).map(|l| l.as_str())
    }

    fn install(&self, py: Python<'_>) -> PyResult<()> {
        tracing_subscriber::registry()
            .with(self.clone_ref(py))
            .try_init()
            .map_err(|e| PyPetriError(PetriError::Other(e.to_string())))?;
        Ok(())
    }
}

fn _log_level_to_str(level: Level) -> &'static str {
    if level == Level::WARN {
        "warning"
    } else {
        level.as_str()
    }
}

fn _parse_log_level(value: &str) -> Result<Level> {
    if value.eq_ignore_ascii_case("warning") {
        Ok(Level::WARN)
    } else {
        Level::from_str(value).map_err(|_| PetriError::Other("Log level must be one of".into()))
    }
}
