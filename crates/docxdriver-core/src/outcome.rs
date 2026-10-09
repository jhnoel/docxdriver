use serde_json::Value;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Status {
    Ok,
    Blocked,
    Error,
}

impl Status {
    pub fn as_str(self) -> &'static str {
        match self {
            Status::Ok => "ok",
            Status::Blocked => "blocked",
            Status::Error => "error",
        }
    }
}

/// The engine's answer: the JSON half of the ABI plus the optional output bytes.
#[derive(Debug)]
pub struct Outcome {
    pub status: Status,
    pub summary: String,
    pub result: Option<Value>,
    pub bytes: Option<Vec<u8>>,
}

impl Outcome {
    pub fn ok(summary: impl Into<String>, result: Value) -> Self {
        Outcome {
            status: Status::Ok,
            summary: summary.into(),
            result: Some(result),
            bytes: None,
        }
    }

    pub fn ok_bytes(summary: impl Into<String>, result: Value, bytes: Vec<u8>) -> Self {
        Outcome {
            status: Status::Ok,
            summary: summary.into(),
            result: Some(result),
            bytes: Some(bytes),
        }
    }

    pub fn blocked(summary: impl Into<String>) -> Self {
        Outcome {
            status: Status::Blocked,
            summary: summary.into(),
            result: None,
            bytes: None,
        }
    }

    pub fn error(summary: impl Into<String>) -> Self {
        Outcome {
            status: Status::Error,
            summary: summary.into(),
            result: None,
            bytes: None,
        }
    }

    /// The DocxCommandResult JSON (bytes travel out-of-band).
    pub fn result_json(&self) -> Value {
        let mut object = serde_json::Map::new();
        object.insert("status".into(), Value::String(self.status.as_str().into()));
        object.insert("summary".into(), Value::String(self.summary.clone()));
        if let Some(result) = &self.result {
            object.insert("result".into(), result.clone());
        }
        Value::Object(object)
    }
}
