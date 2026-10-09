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

## Parameters and equations

Every dimension and numeric feature value gets a parameter name (`d0`, `d1`, ... in creation
order); `sketch.constrain` returns the new dimension's name. `param.add` makes a user parameter,
`param.set` gives any parameter an equation, and `param.list` shows them all. Every feature
command (`model.*`, `work.*`, `feature.add`, `feature.update`) also takes `equations`, a map from
a value of the feature to an equation, applied in the same undo step:

```json
{ "run": "param.add", "with": { "name": "t", "equation": "8 mm" } },
{ "run": "param.set", "with": { "name": "$thickness.name", "equation": "t" } },
{ "run": "model.rib", "with": { "sketch": "$rib_line.feature", "thickness": 4, "equations": { "/thickness": "t / 2" } } }
```

The value names are JSON pointers into the feature's definition (`/radius`, `/extent/distance`,
`/kind/counterbore/depth`); `param.list` shows which value each parameter drives. Lengths are in
millimetres and angles in degrees. `examples/m2-mount/mount.json` uses all of this.

## Assemblies

A script has one part document, one assembly and one drawing. `file.new` starts a new part (scripts build
several parts and save each). `asm.*` commands work on the assembly. `render.png` shows whichever
was worked on last, and takes `exploded: true` for an assembly.

```json
{ "run": "asm.new", "with": { "name": "Pivot" } },
{ "run": "asm.insert", "with": { "path": "base.tenon" }, "as": "base" },
{ "run": "asm.insert", "with": { "path": "arm.tenon" }, "as": "arm" },
{ "run": "asm.geom", "with": { "component": "$base.component", "edge": [{ "type": "cap", "feature": 2, "end": "end" }, { "type": "side", "feature": 2, "curve": 11 }] }, "as": "hole" },
{ "run": "asm.geom", "with": { "component": "$arm.component", "edge": [{ "type": "cap", "feature": 2, "end": "start" }, { "type": "side", "feature": 2, "curve": 11 }] }, "as": "arm_hole" },
{ "run": "asm.joint", "with": { "type": "revolute", "a": "$hole", "b": "$arm_hole" }, "expect": { "dof": 1 } },
{ "run": "asm.save", "with": { "path": "pivot.tenonasm" } }
```

`asm.geom` names a component's geometry (faces by origin, edges by their two faces) and returns
a target for `asm.constrain` and `asm.joint`. Every command that moves components returns the
assembly's remaining degrees of freedom (`dof`). `asm.edit_part` runs a part command on a
component's part, as editing in place does. `examples/m3-pivot/pivot.json` uses all of this.

## Drawings

A script also has one drawing; `drw.*` commands work on it. `drw.view.base` takes a part or
assembly file (`model`), read relative to the output folder. Sheet positions (`at`) are millimetres
of paper from the sheet's bottom-left corner. `render.png` after a drawing command draws a sheet
(`sheet`, `width`).

```json
{ "run": "drw.new", "with": { "name": "Plate", "standard": "ansi", "size": "B" } },
{ "run": "drw.view.base", "with": { "model": "plate.tenon", "orientation": "front", "scale": 1, "at": [100, 70] }, "as": "front" },
{ "run": "drw.view.projected", "with": { "parent": "$front.view", "side": "above" }, "as": "top" },
{ "run": "drw.pick", "with": { "view": "$top.view", "view_at": [60, 80] }, "as": "edge", "expect": { "kind": "line", "length": 120 } },
{ "run": "drw.dimension", "with": { "view": "$top.view", "type": "horizontal", "a": "$edge", "by": [0, 12] }, "expect": { "value": 120 } },
{ "run": "drw.export.pdf", "with": { "path": "plate.pdf" } }
```

`drw.pick` finds the edge drawn nearest a point, as a click does: `at` on the sheet, or
`view_at` in the view's own coordinates (model millimetres; `drw.to_sheet` and `drw.to_view`
convert). `drw.dimension` places the dimension at `at`, or `by` an offset from the middle of what
it measures. `drw.info` reports every view and every dimension's value and hole table's rows as
they are now; after a model file changes, `drw.update` reads it again and the values follow.
`examples/m4-plate/plate.json` uses all of this.