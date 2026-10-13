# MCP server

`tenon-cli mcp` is a [Model Context Protocol](https://modelcontextprotocol.io) server on
stdin/stdout, so an AI agent can build, inspect, repair and render parts, assemblies and
drawings. Everything it does is a command of the same registry the app uses
([commands.md](commands.md)), checked against that command's schema
([schemas](schemas/README.md)) before it runs, and undoable.

It works in one of two places:

```sh
tenon-cli mcp                    # on its own: an empty part in memory
tenon-cli mcp bracket.tenon      # on its own, with a project open
tenon-cli mcp --attach           # in the app that is running: what is done shows in its window
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
| `list_commands` | `prefix` (optional, e.g. `sketch.`) | Every command: its id, what it takes in words, whether it is an undoable edit |
| `describe_command` | `command` | That command's parameters as JSON Schema, with its description |
| `run_command` | `command`, `params` | The command's result (new feature and entity ids, names, ...) |
| `run_commands` | `steps`, `as_one` (default true) | Every step's result. As one: all of them as a single undo step, or, when one fails, none of them |
| `model_tree` | | Regenerates; the feature tree with each feature's status, the body count, the first error |
| `query_topology` | `body` (optional) | Solids, faces, edges, vertices, validity, bounding box per body; with `body`, its faces with persistent names, surface type, area, centroid |
| `find_geometry` | as `model.find` | Faces or edges by what they are and where, each with a `reference` that commands take |
| `measure` | `density` (g/cm³, optional: the part's material's, or 1 without one) | Volume (mm³), area, mass (g), centre of mass, inertia per body; the total mass and the material; bounding boxes |
| `broken_references` | | The first failing feature's references that no longer find their face or edge, each with the nearest replacements and whether the nearest is clearly the one meant |
| `repair_reference` | `feature`, `path`, and `candidate` or `with` | Puts a face or edge in place of the broken reference, as one undoable edit; the tree as it then is |
| `render_png` | `view`, `width`, `height`, `highlight_faces`, `highlight_edges`, `body`, `exploded`, `sheet` | A PNG image (software renderer; iso, front, back, left, right, top, bottom) |
| `export` | `format` (`tenon`, `step`, `stl`, `3mf`), `path` | Writes the file |
| `session_log` | `path`, `all` (both optional) | What was done through this server, as a script `tenon-cli run` replays |

A command that fails returns a tool result with `isError: true` and the reason, so the agent can
correct itself. Protocol errors are reserved for malformed requests and unknown tools or methods.
Results that are data also come as `structuredContent`.

One resource: `tenon://schemas/commands`, the schema of every command in one document (the same
as [schemas/commands.json](schemas/commands.json)).

### Building

`run_commands` takes the steps of a [script](scripts.md): `{"run": id, "with": params, "as":
name}`, and a string `"$name.path"` in a later step's `with` is that part of an earlier
result. Run as one, the steps are a single entry in the undo history, and a failure anywhere
leaves the document as it was.

A plate with a hole through it and its upright edges rounded, in four steps:

```json
{ "steps": [
  { "run": "model.box", "with": { "width": 60, "depth": 40, "height": 8 } },
  { "run": "model.cylinder", "with": { "x": 30, "y": 20, "diameter": 10, "height": 8, "operation": "cut" } },
  { "run": "model.find", "with": { "what": "edges", "kind": "line", "along": "z" }, "as": "up" },
  { "run": "model.fillet", "with": { "radius": 3, "edges": ["$up.found.0.reference", "$up.found.1.reference", "$up.found.2.reference", "$up.found.3.reference"] } }
] }
```

The helpers are ordinary commands, so they are also there for scripts and for the app:

- `model.box`, `model.cylinder`, `model.sphere`, `model.torus` each make a dimensioned sketch
  and the feature on it, in one undo step. What is left in the tree is what someone drawing it
  by hand would have left: a sketch to edit and an extrusion or a revolution.
- `model.find` finds faces and edges by description: `{what: faces, facing: top}`,
  `{what: edges, kind: line, along: z}`, `{what: faces, hole: true, radius: 3}`,
  `{what: faces, kind: plane, most: largest}`, `{what: faces, feature: id}`. Each comes with a
  reference to pass on (to a fillet's `edges`, a shell's `remove`, a sketch's `face`).

There is no parser of sentences in Tenon. The client's language model turns "a 60 by 40 plate,
8 thick, with a 10 mm hole in the middle" into the four steps above; these commands are what
make that a short and dependable translation (DEC-084).

### Repairing a reference

When a face or edge a feature refers to is no longer there (an earlier feature changed, and the
edge it filleted is now two), the feature fails and says so in `model_tree`.
`broken_references` then lists, for the first failing feature, each reference that finds
nothing, and for each the nearest faces or edges of the kind it was, with:

- `reference`, to put in its place;
- `body` and `face` or `edge`: its number, for `highlight_faces` / `highlight_edges` of
  `render_png`, to look at which is which;
- what it is (its surface or curve, area or length, where its middle is) and `distance`: how
  far it is from where the old one was;
- `confident` on the reference: true when the first candidate is clearly the one meant: within
  5 mm of where the old one was, and the next more than four times as far. (Edges that share a
  face with the old edge are listed first, then the others, each by distance.)

`repair_reference` with `candidate` (0 is the nearest) or `with` (any reference, from
`find_geometry` for instance) is one undoable edit. Without either it takes the nearest only
when that is `confident`, and otherwise refuses and lists the choices: a wrong guess here
gives a part that rebuilds and is wrong, which is worse than one that fails.

### A stress study

Where a part is held and what pushes on it is set up by the `study.*` commands (through
`run_command`), on faces found with `find_geometry`:

- `study.new`; then `study.fix`, `study.pin` (round faces) or `study.frictionless` (flat
  or round faces) to hold it, and `study.force`, `study.pressure` or `study.gravity` to
  load it. `study.set` changes one of them, or suppresses it; `study.copy` makes the study
  again, to change and compare.
- `study.check` holds the study against the part as it now is and says whether it can be
  worked out: `ready`, and in `needs` what it lacks in words (a support, a load, a material
  that says how strong it is, faces that a later change took off the part).

Working a study out is not there yet (ROADMAP, M11).

### What was done

`session_log` gives the commands that changed something, in order, with their parameters, as a
script. Every tool that changes the document is logged as the command it ran, so the log is the
whole of what the agent did: it can be read, kept with the part, and replayed with
`tenon-cli run` from the same starting point. Steps run as one are logged as one `batch.run`.

## Attached to the running app

```sh
tenon --control              # the app, with its control port open
tenon-cli mcp --attach       # the MCP server, driving that app
```

From the repository: `pixi run app --control`, and in the client's configuration
`"args": ["run", "--manifest-path", "/path/to/tenon/pixi.toml", "mcp", "--attach"]`. The app is
started first; the server says so and stops when it finds none.

Attached, the server holds no document of its own. Each command is sent to the app and run on
what is open in its window, so the person there sees every step as it is made, finds it in the
undo history (`Ctrl+Z` takes back the agent's last step; steps run as one are one), and can
carry on by hand from where the agent stopped. The status bar says `Remote:` and the command
last run.

The same tools are there. The differences:

- `render_png` (and `render.png`) can take `view: "current"`: the model as the window shows it
  now, from where the person is looking.
- `control.state` (through `run_command`) says what is open (a part, an assembly or a drawing,
  its name and file), what has unsaved changes, whether the app is busy rebuilding, whether a
  sketch or a command's panel is open, and the versions on the app's side.
- Relative paths are taken from the app's working folder: give full paths.

What the app refuses over the port, each time saying why:

- a command that changes the document while a command's panel is open in the app (the person
  is in the middle of something: finish or cancel it first);
- `file.new` and `file.open` (and the assembly's and drawing's) while there are unsaved
  changes: nothing the person has not saved is thrown away from outside;
- `library.new`, `asm.new`, `drw.new`: started in the app, which sets up their windows;
- a command whose turn came after the person changed the part while it was running: nothing is
  done, and it can be run again;
- `asm.interference`, `asm.export_step`, `asm.export_stl` and `asm.export_3mf`, which are
  not run over the port yet: they run in the app, or on the saved file with `tenon-cli`.

### The control port

Off unless the app is started with `--control [PORT]` (without a port, any that is free).

- It listens on the loopback address (127.0.0.1) only: nothing off the machine can reach it.
- Every request must carry a token of 128 random bits the app makes up when it starts. A
  request without it is refused, whoever on the machine sends it, before anything is looked at.
- The app writes the port and the token to a file in the user's own settings folder, where
  `--attach` finds them, and takes the file away when it closes. On Unix the file is readable
  by the user alone (mode 0600). The token is not printed (unless there is no settings folder
  to write it in, when printing it is the only way left to say it).
  - Windows: `%APPDATA%\Tenon\control.json`
  - macOS: `~/Library/Application Support/Tenon/control.json`
  - elsewhere: `$XDG_RUNTIME_DIR/tenon/control.json`, or under `$XDG_CONFIG_HOME` (or
    `~/.config`)
  - `TENON_CONTROL_FILE` names another file, for both programs (to run two apps at once).
- Whoever has the token can do what the commands can: change the open document, and read and
  write files with the user's permissions (`file.save`, `export.*`, `render.png` with a path).
  It is a way for the user's own tools to drive their own app, not a boundary to put an
  untrusted program behind.

The wire is one JSON object a line each way, for anything that would rather not speak MCP:

```text
-> {"id": 7, "token": "...", "run": "model.box", "with": {"width": 60, "depth": 40, "height": 8}}
<- {"id": 7, "ok": true, "result": {"sketch": 1, "feature": 2, ...}}
<- {"id": 7, "ok": false, "error": "model.box needs `height`: out of the sketch"}
```

Requests are done one at a time, in the order they come. A command that needs the geometry
kernel runs on the geometry thread, on a copy of the part, and what it changed is put into the
open part as one edit when it comes back; the window goes on drawing meanwhile. On an assembly
or a drawing, each command waits until the app has rebuilt the parts and drawn the views that
the one before it changed, so it sees what a person at the window would see. A request line
over 16 MiB is refused and its connection closed.

## Details

- Transport: newline-delimited JSON-RPC 2.0. Stdout carries only protocol messages; logs go to
  stderr. A line over 16 MiB is rejected; the part beyond the limit is discarded unread.
- Protocol revisions: 2025-11-25, 2025-06-18, 2025-03-26, 2024-11-05. The server answers with
  the client's revision if it supports it, otherwise the newest.
- Capabilities: tools and resources (no prompts or sampling).
- Parameters: every command is checked against its schema before it runs. A name that is not
  one of the command's is refused with the one that was probably meant
  (``model.shell has no parameter `thicknes`: did you mean `thickness`?``), and nothing is
  changed.
- Files: `export`, `file.save`, `file.open` and `session_log` with a path read and write
  wherever the path points, relative to the server's working directory. The server runs with
  the user's permissions, as any local tool does; do not expose it to untrusted clients.
- Versions: attached, the server warns on stderr when the app's command schemas are another
  version than its own.
- Tests: `apps/tenon-cli/tests/m1.rs` drives a full session through the stdio transport (build,
  measure, query, render, export, errors, garbage input); `apps/tenon-cli/tests/m10.rs` the
  tools above (what the server offers, building as one step and the log replayed, a broken
  reference seen and repaired, and not guessed at when two replacements are equally near); `apps/tenon-cli/tests/m10_attach.rs` an agent attached to a
  workbench that goes on taking frames; `crates/ui/src/control_tests.rs` the port itself, on
  a part, an assembly and a drawing.
