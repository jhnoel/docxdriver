# docxdriver-core

The public contract is `Request::{Command, Plan}` in `src/api.rs`. Commands
are typed one-shot actions; plans are source-bound, preview-keyed atomic folds
over one parsed `WorkState`. JSON and TOML are adapters to this model, not
alternate execution engines.

Core owns document semantics and never writes paths. CLI and Pi own host I/O.
