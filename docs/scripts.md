# Command scripts

A script is a JSON list of [commands](commands.md) that `tenon-cli run` applies to a new part, in
order. Scripts are how the demos are built, how CI checks modelling end to end, and a simple way
for tools to drive Tenon without the [MCP server](mcp.md).

```sh
tenon-cli run part.json --out build/        # relative paths in the script resolve against build/
tenon-cli run part.json --json              # every step's result as JSON
```

## Format

```json
{
  "name": "optional title",
  "description": "optional",
  "steps": [
    { "run": "sketch.create", "with": { "plane": "xy" }, "as": "s" },
    { "run": "sketch.rectangle", "with": { "sketch": "$s.feature", "x1": 0, "y1": 0, "x2": 20, "y2": 10 } },
    { "run": "model.extrude", "with": { "sketch": "$s.feature", "distance": 5 } },
    { "run": "model.mass", "expect": { "bodies.0.volume": 1000 } },
    { "run": "file.save", "with": { "path": "box.tenon" }, "note": "relative to --out" }
  ]
}
```

A bare list of steps is accepted too. Each step has:

| Key | |
|---|---|
| `run` | Command id ([commands.md](commands.md)), required |
| `with` | Parameters (an object; default `{}`) |
| `as` | Saves the step's result under a name for later steps |
| `expect` | Checks on the result: `{ "path": value }`. Numbers match within a relative 1e-6 (absolute 1e-6 below 1); anything else must be equal |
| `note` | A comment; ignored |

Unknown keys are errors, so typos do not pass silently.

**References.** A string `"$name.path"` anywhere in `with` is replaced by that part of the result
saved as `name`: object keys and array indices separated by dots, e.g. `"$r.lines.0"`. The
replacement keeps its JSON type, so `"$face"` can stand for a whole face-reference object. Write
`"$$"` for a literal leading dollar sign.

**Failure.** The run stops at the first failing step; the error names the step number (from 1)
and the command, and the exit code is non-zero. Steps before the failure have taken effect;
nothing after it runs.

**Paths.** A relative `path` parameter resolves against `--out` (default: the current directory),
which is created if missing.

Besides the registry, scripts can use `render.png`, which renders the part with the software
rasteriser (no GPU needed).

The format is a tooling convenience, not a stored document format: it may change between
milestones, with the change noted in [decisions.md](decisions.md). See
[examples/m1-bracket/bracket.json](../examples/m1-bracket/bracket.json) for a complete part.
