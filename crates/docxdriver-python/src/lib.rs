//! Native bytes/JSON boundary over the same typed engine used by the WASM adapter.
use docxdriver_core::{Case, Diagnostic, Document, PreviewKey, Source, View};
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

// Engine functions behind `docxdriver.document`: bytes in, JSON text or
// bytes out, `ValueError("code: message")` on a diagnostic.

#[pyfunction]
#[pyo3(signature = (paragraphs, html))]
fn _docx_create(py: Python<'_>, paragraphs: Option<Vec<String>>, html: Option<String>) -> PyResult<Py<PyBytes>> {
    let source = match html {
        Some(html) => Source::Html(html),
        None => Source::Paragraphs(paragraphs.unwrap_or_default()),
    };
    let bytes = py.detach(|| docxdriver_core::create(source)).map_err(diagnostic)?;
    Ok(PyBytes::new(py, &bytes).unbind())
}

#[pyfunction]
fn _docx_markup(py: Python<'_>, data: &[u8], view: &str) -> PyResult<String> {
    let view = match view {
        "markup" => View::Markup,
        "final" => View::Final,
        "original" => View::Original,
        other => return Err(PyValueError::new_err(format!("unknown view: {other}"))),
    };
    py.detach(|| Document::open(data).markup(view)).map_err(diagnostic)
}

#[pyfunction]
fn _docx_comments(py: Python<'_>, data: &[u8]) -> PyResult<String> {
    json(py.detach(|| Document::open(data).comments()))
}

#[pyfunction]
fn _docx_revisions(py: Python<'_>, data: &[u8]) -> PyResult<String> {
    json(py.detach(|| Document::open(data).revisions()))
}

#[pyfunction]
fn _docx_styles(py: Python<'_>, data: &[u8]) -> PyResult<String> {
    json(py.detach(|| Document::open(data).styles()))
}

#[pyfunction]
fn _docx_assets(py: Python<'_>, data: &[u8]) -> PyResult<String> {
    json(py.detach(|| Document::open(data).assets()))
}

#[pyfunction]
fn _docx_find(py: Python<'_>, data: &[u8], query: &str, ignore_case: bool) -> PyResult<String> {
    let case = if ignore_case { Case::Insensitive } else { Case::Sensitive };
    json(py.detach(|| Document::open(data).find(query, case)))
}

/// `{"key": ..., "report": ...}` for a plan's JSON without `base`.
#[pyfunction]
fn _docx_review(py: Python<'_>, data: &[u8], plan: &str) -> PyResult<String> {
    let review = py.detach(|| {
        let document = Document::open(data);
        document.review(&document.plan(plan)?)
    });
    json(review.map(|review| serde_json::json!({"key": review.key.0, "report": review.report})))
}

#[pyfunction]
fn _docx_commit(py: Python<'_>, data: &[u8], plan: &str, key: &str) -> PyResult<Py<PyBytes>> {
    let committed = py.detach(|| {
        let document = Document::open(data);
        document.commit(&document.plan(plan)?, &PreviewKey(key.into()))
    });
    Ok(PyBytes::new(py, &committed.map_err(diagnostic)?).unbind())
}

fn json<T: serde::Serialize>(result: Result<T, Diagnostic>) -> PyResult<String> {
    let value = result.map_err(diagnostic)?;
    serde_json::to_string(&value).map_err(|error| PyValueError::new_err(error.to_string()))
}

fn diagnostic(diagnostic: Diagnostic) -> PyErr {
    PyValueError::new_err(format!("{}: {}", diagnostic.code, diagnostic.message))
}

#[pymodule]
fn _core(module: &Bound<'_, PyModule>) -> PyResult<()> {
    module.add_function(wrap_pyfunction!(execute_request, module)?)?;
    module.add_function(wrap_pyfunction!(canonical_plan_json, module)?)?;
    module.add_function(wrap_pyfunction!(_docx_create, module)?)?;
    module.add_function(wrap_pyfunction!(_docx_markup, module)?)?;
    module.add_function(wrap_pyfunction!(_docx_comments, module)?)?;
    module.add_function(wrap_pyfunction!(_docx_revisions, module)?)?;
    module.add_function(wrap_pyfunction!(_docx_styles, module)?)?;
    module.add_function(wrap_pyfunction!(_docx_assets, module)?)?;
    module.add_function(wrap_pyfunction!(_docx_find, module)?)?;
    module.add_function(wrap_pyfunction!(_docx_review, module)?)?;
    module.add_function(wrap_pyfunction!(_docx_commit, module)?)?;
    module.add("__version__", env!("CARGO_PKG_VERSION"))?;
    Ok(())
}
