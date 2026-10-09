# docxdriver-mcp

`docxdriver-mcp` is a native MCP server for the typed, source-bound
`docxdriver-core` DOCX engine. It exposes exactly five composable tools:

- `docx_create`
- `docx_read`
- `docx_find`
- `docx_edit`
- `docx_help`

The server is transport-independent. Choose stdio for a client-managed local
process or stateless Streamable HTTP for a reusable localhost endpoint.

## Build and install

From this repository:

```sh
cargo build -p docxdriver-mcp --release
```

The binary is `target/release/docxdriver-mcp`. It can also be installed directly
from this workspace:

```sh
cargo install --locked --path crates/docxdriver-mcp
```

Or installed from the Git repository:

```sh
cargo install --locked --git https://github.com/jhnoel/docxdriver --bin docxdriver-mcp
```

## stdio

stdio is the default transport. The MCP client starts the process and owns its
stdin/stdout connection. The workspace defaults to the process cwd:

```sh
docxdriver-mcp --transport stdio --root /path/to/project
```

Use the binary as the `command` for an MCP client that supports local stdio
servers. Do not add logging to stdout; stdout is reserved for MCP messages.

## localhost HTTP

Start a stateless Streamable HTTP endpoint bound to loopback:

```sh
docxdriver-mcp \
  --transport http \
  --root /path/to/project \
  --listen 127.0.0.1:39200
```

The endpoint is:

```text
http://127.0.0.1:39200/mcp
```

The server uses the current stateless MCP transport and does not depend on
protocol session IDs. It retains rmcp's loopback Host validation and accepts
only local Host values by default. Keep the listener on `127.0.0.1` unless a
separate authentication and network policy is added.

## Safety and editing workflow

Every document and TOML plan path must remain below `--root`, including after
symlink resolution. The server uses an opened capability-scoped root directory
for actual file operations, so concurrent path replacement cannot redirect
reads or writes outside the workspace. Creation is exclusive and never
overwrites an existing file. Existing documents are replaced through a synced
temporary file and an atomic rename.

`docx_edit` always previews first. An inline plan looks like:

```json
{
  "path": "draft.docx",
  "plan": {
    "operations": [
      {
        "op": "replace_text",
        "at": "0F537164",
        "select": "old",
        "with": "new"
      }
    ],
    "author": "docxdriver",
    "change_mode": "track"
  }
}
```

The preview returns a deterministic `preview_key`. Send the identical plan
and that key to commit. The server rereads the source and refuses the commit if
the source hash changed.

## Composition

The DOCX tools are one cohesive MCP interface. MCP clients can compose this
server with independent interfaces such as storage, OCR, or notification
servers. A tool call does not need to invoke MCP internally; all five tools call
the shared native core directly.
