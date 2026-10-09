//! Native bytes/JSON boundary over the same typed engine used by the WASM adapter.
use pyo3::{exceptions::PyValueError, prelude::*, types::PyBytes};

#[pyfunction]
fn execute_request(
    py: Python<'_>,
    input: Option<&Bound<'_, PyBytes>>,
    request_json: &str,
) -> PyResult<(String, Option<Py<PyBytes>>)> {
    let bytes = input.map(|value| value.as_bytes().to_vec());
    let (json, output) = py
        .detach(|| {
            let request = serde_json::from_str::<docxdriver_core::Request>(request_json);
            match request {
                Ok(request) => match docxdriver_core::execute_request(bytes.as_deref(), &request) {
                    docxdriver_core::RequestResult::Command(result) => {
                        let json = serde_json::to_string(&result)?;
                        let output = match result {
                            docxdriver_core::CommandResult::Completed { bytes, .. } => bytes,
                            _ => None,
                        };
                        Ok((json, output))
                    }
                    docxdriver_core::RequestResult::Plan(result) => {
                        let json = serde_json::to_string(&result)?;
                        let output = match result {
                            docxdriver_core::PlanResult::Committed { bytes, .. } => Some(bytes),
                            _ => None,
                        };
                        Ok((json, output))
                    }
                },
                Err(error) => Ok((
                    serde_json::json!({"outcome":"rejected","diagnostic":{
                        "code":"invalid_request", "message":error.to_string()
                    }})
                    .to_string(),
                    None,
                )),
            }
        })
        .map_err(|error: serde_json::Error| PyValueError::new_err(error.to_string()))?;
    Ok((json, output.map(|value| PyBytes::new(py, &value).unbind())))
}

#[pyfunction]
fn canonical_plan_json(py: Python<'_>, plan_json: &str) -> String {
    py.detach(|| {
        serde_json::from_str::<docxdriver_core::Plan>(plan_json)
            .map_err(|error| error.to_string())
            .and_then(|plan| docxdriver_core::canonical_json(&plan))
            .unwrap_or_else(|error| serde_json::json!({"error":error}).to_string())
    })
}

#[pymodule]
fn _core(module: &Bound<'_, PyModule>) -> PyResult<()> {
    module.add_function(wrap_pyfunction!(execute_request, module)?)?;
    module.add_function(wrap_pyfunction!(canonical_plan_json, module)?)?;
    module.add("__version__", env!("CARGO_PKG_VERSION"))?;
    Ok(())
}
