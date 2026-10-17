//! The docxdriver Python API as source, for Rust hosts that run a Monty
//! sandbox: the same `docxdriver.plan` and `docxdriver.document` modules
//! the PyPI package ships.
//!
//! ```text
//!   Monty sandbox                          Rust host
//!   +-------------------------------+      +------------------------------+
//!   | PLAN, DOCUMENT preloaded      |      |                              |
//!   | doc = Document(data)          |      |                              |
//!   | doc.review(Plan(ops=[...]))   | ---> | _docx_review(data, plan)     |
//!   |                               |      |   Document::open(data)       |
//!   |                               | <--- |   .review(&doc.plan(plan)?)  |
//!   +-------------------------------+ JSON +------------------------------+
//! ```
//!
//! The host defines each name in [`ENGINE`] as an external function that
//! calls `docxdriver_core::Document`; see [`DOCUMENT`] for each signature.

/// Plan vocabulary: op dataclasses, `Revise`, `AcceptRevision`/`RejectRevision`,
/// `Plan`. Same text as the `docxdriver.plan` module of the PyPI package.
pub const PLAN: &str = include_str!("../sandbox/plan.py");

/// Monty type-checker stubs for [`PLAN`].
pub const PLAN_STUBS: &str = include_str!("../sandbox/plan.pyi");

/// Agent-facing Markdown reference for [`PLAN`].
pub const PLAN_REFERENCE: &str = include_str!("../sandbox/plan.md");

/// `Document`, `Review` and `create` over .docx bytes. Same text as the
/// `docxdriver.document` module of the PyPI package.
pub const DOCUMENT: &str = include_str!("../sandbox/document.py");

/// External functions [`DOCUMENT`] calls; each raises `ValueError` on a
/// core diagnostic.
///
/// | name               | arguments                | returns                   |
/// |--------------------|--------------------------|---------------------------|
/// | `_docx_create`     | paragraphs, html         | bytes                     |
/// | `_docx_markup`     | data, view               | str                       |
/// | `_docx_comments`   | data                     | JSON list                 |
/// | `_docx_revisions`  | data                     | JSON list                 |
/// | `_docx_styles`     | data                     | JSON list                 |
/// | `_docx_assets`     | data                     | JSON list                 |
/// | `_docx_find`       | data, query, ignore_case | JSON list                 |
/// | `_docx_review`     | data, plan JSON          | JSON `{"key", "report"}`  |
/// | `_docx_commit`     | data, plan JSON, key     | bytes                     |
pub const ENGINE: [&str; 9] = [
    "_docx_create",
    "_docx_markup",
    "_docx_comments",
    "_docx_revisions",
    "_docx_styles",
    "_docx_assets",
    "_docx_find",
    "_docx_review",
    "_docx_commit",
];
