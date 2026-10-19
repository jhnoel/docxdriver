//! docxdriver-core: typed, source-bound DOCX Request/Plan engine.
//!
//! The stable contract is the source-bound typed [`Request`] and [`RequestResult`]
//! API. Requests are either a typed one-command envelope or a validated Plan;
//! plans support canonical JSON/TOML, deterministic preview keys, atomic folds,
//! typed diagnostics, and complete output metadata. [`Document`] is the same
//! engine as plain methods on one document's bytes.

mod api;
mod commands;
mod document;
mod facade;
mod html;
pub mod integrity;
mod media;
mod outcome;
mod package;
mod segments;

pub use api::*;
pub use facade::{create, Case, Document, Review, Source, View};
