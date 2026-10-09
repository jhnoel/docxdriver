//! docxdriver-core: typed, source-bound DOCX Request/Plan engine.
//!
//! The stable contract is the source-bound typed [`Request`] and [`RequestResult`]
//! API. Requests are either a typed one-command envelope or a validated Plan;
//! plans support canonical JSON/TOML, deterministic preview keys, atomic folds,
//! typed diagnostics, and complete output metadata.

mod api;
mod commands;
mod document;
mod html;
mod media;
mod outcome;
mod package;
mod segments;

pub use api::*;
