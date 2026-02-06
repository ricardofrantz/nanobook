use nanobook::Event;
use pyo3::prelude::*;
use pyo3::IntoPyObjectExt;

#[pyclass(name = "Event")]
#[derive(Clone)]
pub struct PyEvent {
    pub inner: Event,
}

#[pymethods]
impl PyEvent {
    #[getter]
    fn kind(&self) -> String {
        match &self.inner {
            Event::SubmitLimit { .. } => "submit_limit".to_string(),
            Event::SubmitMarket { .. } => "submit_market".to_string(),
            Event::Cancel { .. } => "cancel".to_string(),
            Event::Modify { .. } => "modify".to_string(),
            Event::SubmitStopMarket { .. } => "submit_stop_market".to_string(),
            Event::SubmitStopLimit { .. } => "submit_stop_limit".to_string(),
            Event::SubmitTrailingStopMarket { .. } => "submit_trailing_stop_market".to_string(),
            Event::SubmitTrailingStopLimit { .. } => "submit_trailing_stop_limit".to_string(),
        }
    }

    fn __repr__(&self) -> String {
        format!("{:?}", self.inner)
    }

    pub fn __getstate__(&self, py: Python) -> PyResult<PyObject> {
        let json = serde_json::to_string(&self.inner)
            .map_err(|e| pyo3::exceptions::PyValueError::new_err(e.to_string()))?;
        Ok(json.into_py_any(py)?)
    }

    pub fn __setstate__(&mut self, state: Bound<'_, PyAny>) -> PyResult<()> {
        let json: String = state.extract()?;
        self.inner = serde_json::from_str(&json)
            .map_err(|e| pyo3::exceptions::PyValueError::new_err(e.to_string()))?;
        Ok(())
    }
}
