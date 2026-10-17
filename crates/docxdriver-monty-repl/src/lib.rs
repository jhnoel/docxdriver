//! Python sources for Rust hosts running docxdriver plans in a Monty sandbox.
//!
//! ```text
//!   sandbox (Monty)                         host (Rust)
//!   +-----------------------------+         +---------------------------+
//!   | PLAN: Plan, ReplaceText,... |         |                           |
//!   | HOST_WRAPPERS: docx_review  | ------> | _docx_review(path, state) |
//!   |   calls _docx_* callbacks   | <------ |   docxdriver-core         |
//!   +-----------------------------+         +---------------------------+
//! ```
//!
//! [`PLAN`] stands alone: `Plan(...).to_dict()` yields
//! `{"author", "change_mode", "ops"}`, a docxdriver-core plan without `base`.
//! [`HOST_WRAPPERS`] defines `docx_*`/`quote_*` over private callbacks
//! (`_docx_read`, `_docx_review`, ...) the host must provide.

/// Plan vocabulary: op dataclasses, `Revise`, `AcceptRevision`/`RejectRevision`,
/// `Quote`/`Term`/`Inline`, `Plan`. Identical to the `docxdriver.plan` module.
pub const PLAN: &str = include_str!("../sandbox/plan.py");

/// Monty type-checker stubs for [`PLAN`].
pub const PLAN_STUBS: &str = include_str!("../sandbox/plan.pyi");

/// `docx_*`, `quote_*`, `term_render`, `inline_validate` over host callbacks,
/// plus the values they return (`ReadResult`, `ReviewResult`, `QuoteResult`, ...).
pub const HOST_WRAPPERS: &str = include_str!("../sandbox/host_wrappers.py");

/// [`PLAN`] followed by [`HOST_WRAPPERS`].
pub const PRELUDE: &str = concat!(
    include_str!("../sandbox/plan.py"),
    "\n",
    include_str!("../sandbox/host_wrappers.py"),
);

/// Stubs for [`PRELUDE`], including the host callbacks.
pub const STUBS: &str = concat!(
    include_str!("../sandbox/plan.pyi"),
    "\n",
    include_str!("../sandbox/host_wrappers.pyi"),
);

/// Agent-facing Markdown reference for [`PRELUDE`].
pub const REFERENCE: &str = concat!(
    include_str!("../sandbox/plan.md"),
    "\n",
    include_str!("../sandbox/host_reference.md"),
);
