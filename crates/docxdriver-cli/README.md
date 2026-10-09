# docxdriver CLI

The native MCP distribution is available from the sibling
[`docxdriver-mcp`](../docxdriver-mcp/README.md) crate; it exposes the same five
DOCX interfaces over stdio or stateless localhost HTTP.

The CLI is a thin host adapter over the typed `docxdriver-core::Request` API. Its
only commands are `create`, `read`, `find`, `plan`, and one command for each
typed edit operation:

- `replace-text`
- `replace-paragraph`
- `format-text`
- `format-paragraph`
- `insert-paragraph`
- `delete-paragraphs`
- `set-page-margins`
- `set-even-and-odd-headers`
- `set-header`
- `set-footer`
- `clear-header`
- `clear-footer`
- `comment-add`
- `comment-reply`
- `comment-set-status`
- `comment-delete`
- `revision-settle`

All paths must resolve beneath the current working directory. Existing paths
are checked through canonical symlink resolution; output paths are checked
lexically and through their canonical parent.

## Commands

Create refuses to overwrite an existing destination:

~~~
docxdriver create draft.docx --html '<p>Hello world</p>'
~~~

`read` and `find` map directly to typed `Command::Read` and `Command::Find`:

~~~
docxdriver read draft.docx
docxdriver read draft.docx --kind document
docxdriver read draft.docx --kind styles
docxdriver read draft.docx --kind comments
docxdriver read draft.docx --kind revisions
docxdriver find draft.docx world --ignore-case
~~~

`read --kind` accepts `document`, `styles`, `comments`, or
`revisions`. Default document reads return one complete renderable HTML fragment
with native MathML and equation addresses. `--kind document_ui` explicitly requests
CSS, asset manifests and UI selection metadata. The optional `--view markup|final|original`
is passed through to core for document and find requests.

Every mutating command accepts `--dry-run`, `--expect-source sha256:<64
lowercase hex>`, `--output PATH`, `--author`, and `--change-mode track|direct`.
Without `--output`, successful output atomically replaces the input. The host
re-reads the input immediately before replacement and returns a typed
`source_changed_before_commit` rejection if it changed. Filesystem failures
remain infrastructure errors.

~~~
docxdriver replace-text draft.docx --at 0F537164 --select world --with globe \
  --expect-source sha256:… --dry-run
~~~

Linear measurements use points and must be multiples of 0.05. Paragraph IDs
are eight uppercase hexadecimal characters. Comment and revision IDs are
decimal strings. `insert-paragraph` uses exactly one of `--before` or `--after`
(accepting a paragraph id, alias, or `table:N` body-level table anchor);
`revision-settle` uses `--target ID|all` and `--action accept|reject`.

## Plans

A plan is previewed by default and committed only with its matching preview
key. TOML is parsed by core into the same typed `Plan` used by JSON and native
callers:

~~~
base = "sha256:<64 lowercase hex>"
author = "docxdriver"
change_mode = "track"

[[ops]]
op = "replace_text"
at = "0F537164"
select = "world"
with = "globe"
~~~

~~~
docxdriver plan draft.docx plan.toml
docxdriver plan draft.docx plan.toml --commit p1:sha256:…
~~~

A successful preview never writes a document. Commit verifies the preview key,
re-reads the source, writes a unique temporary in the destination directory,
flushes it, and atomically renames it. Temporary files are removed after every
failure. `--json` prints the serialized core `CommandResult` or `PlanResult`
without adding status or bytes-written fields; expected refusals are typed
rejections and exit 2, while infrastructure failures exit 1.
