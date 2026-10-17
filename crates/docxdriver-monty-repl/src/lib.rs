//! The docxdriver plan vocabulary as Python source, for Rust hosts that run
//! a Monty sandbox.
//!
//! ```text
//!   Monty sandbox                      Rust host
//!   +---------------------------+      +--------------------------------+
//!   | PLAN preloaded            |      |                                |
//!   | Plan(ops=[ReplaceText(..)]| ---> | plan["base"] = "sha256:<hex>"  |
//!   |   ).to_dict()             | dict | docxdriver_core::parse_plan_json|
//!   +---------------------------+      +--------------------------------+
//! ```
//!
//! `Plan(...).to_dict()` yields `{"author", "change_mode", "ops"}`: a core
//! plan without `base`, which the host adds from the source document bytes.

/// Plan vocabulary: op dataclasses, `Revise`, `AcceptRevision`/`RejectRevision`,
/// `Plan`. Same text as the `docxdriver.plan` module of the PyPI package.
pub const PLAN: &str = include_str!("../sandbox/plan.py");

/// Monty type-checker stubs for [`PLAN`].
pub const PLAN_STUBS: &str = include_str!("../sandbox/plan.pyi");

/// Agent-facing Markdown reference for [`PLAN`].
pub const PLAN_REFERENCE: &str = include_str!("../sandbox/plan.md");
