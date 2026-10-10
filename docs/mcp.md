# MCP server

`tenon-cli mcp` is a [Model Context Protocol](https://modelcontextprotocol.io) server on
stdin/stdout, so AI agents can build, inspect and render parts. It holds one part in memory
with full undo, and uses the same command registry as the app ([commands.md](commands.md)).

```sh
tenon-cli mcp                    # start with an empty part
tenon-cli mcp bracket.tenon      # start with a project open
```

Example client configuration (Claude Code and others use the same shape):

```json
{
  "mcpServers": {
    "tenon": { "command": "pixi", "args": ["run", "--manifest-path", "/path/to/tenon/pixi.toml", "mcp"] }
  }
}
```

The `mcp` pixi task builds `tenon-cli` in release mode on first use (a few minutes) and runs it
with the OpenCASCADE libraries on the library path. With an installed build, run `tenon-cli mcp`
directly. Relative paths given to the server resolve against the repository root when it is
started through the pixi task.

## Tools

| Tool | Arguments | Returns |
|---|---|---|
| `list_commands` | | Every command with its parameters |
| `run_command` | `command`, `params` | The command's result (new feature and entity ids, ...) |
| `model_tree` | | Regenerates; the feature tree with each feature's status, body count, first error |
| `query_topology` | `body` (optional) | Solids, faces, edges, vertices, validity, bounding box per body; with `body`, its faces with persistent names, surface type, area, centroid |
| `measure` | `density` (g/cm^3, optional: the part's material's, or 1 without one) | Volume (mm^3), area, mass (g), centre of mass, inertia per body; the total mass and the material; bounding boxes |
| `render_png` | `view`, `width`, `height` | A PNG image (software renderer; iso, front, back, left, right, top, bottom) |
| `export` | `format` (`tenon`, `step`, `stl`), `path` | Writes the file |

A command that fails returns a tool result with `isError: true` and the reason, so the agent can
correct itself. Protocol errors are reserved for malformed requests and unknown tools or methods.
Results that are data also come as `structuredContent`.

## Details

- Transport: newline-delimited JSON-RPC 2.0. Stdout carries only protocol messages; logs go to
  stderr. A line over 16 MiB is rejected; the part beyond the limit is discarded unread.
- Protocol revisions: 2025-11-25, 2025-06-18, 2025-03-26, 2024-11-05. The server answers with
  the client's revision if it supports it, otherwise the newest.
- Capabilities: tools only (no resources, prompts or sampling yet).
- Files: `export`, `file.save` and `file.open` read and write wherever the given path points,
  relative to the server's working directory. The server runs with the user's permissions, as
  any local tool does; do not expose it to untrusted clients.
- Tests: `apps/tenon-cli/tests/m1.rs` drives a full session through the stdio transport (build,
  measure, query, render, export, errors, garbage input).
