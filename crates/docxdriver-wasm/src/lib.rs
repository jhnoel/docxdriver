//! wasm-bindgen surface for the typed Request/Plan API.

use wasm_bindgen::prelude::*;

/// JSON serialization adapter for the typed Request API.
#[wasm_bindgen]
pub struct TypedOutput {
    result_json: String,
    bytes: Option<Vec<u8>>,
}

#[wasm_bindgen]
impl TypedOutput {
    #[wasm_bindgen(getter, js_name = resultJson)]
    pub fn result_json(&self) -> String {
        self.result_json.clone()
    }
    #[wasm_bindgen(getter)]
    pub fn bytes(&self) -> Option<Vec<u8>> {
        self.bytes.clone()
    }
}

#[wasm_bindgen(js_name = executeRequest)]
pub fn execute_request(input: Option<Vec<u8>>, request_json: &str) -> TypedOutput {
    let request = serde_json::from_str::<docxdriver_core::Request>(request_json);
    match request {
        Ok(request) => match docxdriver_core::execute_request(input.as_deref(), &request) {
            docxdriver_core::RequestResult::Command(result) => TypedOutput {
                result_json: serde_json::to_string(&result).unwrap_or_else(|e| serde_json::json!({"error":e.to_string()}).to_string()),
                bytes: match result { docxdriver_core::CommandResult::Completed { bytes, .. } => bytes, _ => None },
            },
            docxdriver_core::RequestResult::Plan(result) => TypedOutput {
                result_json: serde_json::to_string(&result).unwrap_or_else(|e| serde_json::json!({"error":e.to_string()}).to_string()),
                bytes: match result { docxdriver_core::PlanResult::Committed { bytes, .. } => Some(bytes), _ => None },
            },
        },
        Err(error) => TypedOutput {
            result_json: serde_json::json!({"outcome":"rejected","diagnostic":{"code":"invalid_request","message":error.to_string()}}).to_string(),
            bytes: None,
        },
    }
}

#[wasm_bindgen(js_name = parsePlanJson)]
pub fn parse_plan_json(text: &str) -> String {
    match docxdriver_core::parse_plan_json(text) {
        Ok(plan) => serde_json::to_string(&plan).unwrap(),
        Err(error) => serde_json::to_string(&error).unwrap(),
    }
}

#[wasm_bindgen(js_name = parsePlanToml)]
pub fn parse_plan_toml(text: &str) -> String {
    match docxdriver_core::parse_plan_toml(text) {
        Ok(plan) => serde_json::json!({"outcome":"parsed", "plan": plan}).to_string(),
        Err(diagnostic) => {
            serde_json::json!({"outcome":"rejected", "diagnostic": diagnostic}).to_string()
        }
    }
}

#[wasm_bindgen(js_name = canonicalPlanJson)]
pub fn canonical_plan_json(plan_json: &str) -> String {
    serde_json::from_str::<docxdriver_core::Plan>(plan_json)
        .map_err(|e| e.to_string())
        .and_then(|p| docxdriver_core::canonical_json(&p))
        .unwrap_or_else(|e| serde_json::json!({"error":e}).to_string())
}

#[wasm_bindgen(js_name = canonicalPlanToml)]
pub fn canonical_plan_toml(plan_json: &str) -> String {
    serde_json::from_str::<docxdriver_core::Plan>(plan_json)
        .map_err(|e| e.to_string())
        .and_then(|p| docxdriver_core::canonical_toml(&p))
        .unwrap_or_else(|e| format!("# error: {e}"))
}

#[wasm_bindgen(js_name = previewKey)]
pub fn preview_key(plan_json: &str) -> String {
    serde_json::from_str::<docxdriver_core::Plan>(plan_json)
        .map(|p| docxdriver_core::PreviewKey::new(&p).0)
        .unwrap_or_default()
}
