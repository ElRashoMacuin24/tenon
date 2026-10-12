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
| `within` | The relative tolerance of this step's numeric expectations in place of 1e-6, at most 0.1. For values the kernel approximates (a coil's volume, a cut thread): say in the `note` why |
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

## Materials and design tables

`document.material` gives the part a material of the library by name (`document.materials`
lists them), or one of its own with a `density` (g/cm^3) and a `color`; `document.appearance`
gives it a colour of its own. `model.mass` then returns the mass in grams. A design table is
made from the part as it stands and grows a row per size; `table.activate` makes the part that
size.

```json
{ "run": "document.material", "with": { "name": "Stainless steel" } },
{ "run": "model.mass", "expect": { "mass": 7.57985618 }, "within": 0.0001 },
{ "run": "table.create", "with": { "columns": ["d0", "d1"], "row": "Soft" } },
{ "run": "table.add_row", "with": { "name": "Stiff", "values": { "d0": 3, "d1": 10 } } },
{ "run": "table.activate", "with": { "row": "Stiff" } }
```

In an assembly, `asm.bom` lists each part's material and the mass of one of it, and `asm.mass`
weighs the assembly with each component at its part's density.
`examples/m6-fittings/fittings.json` uses all of this, and every M6 feature.

## Standard parts, the print check and files for a slicer

`library.list` names the standard parts built into Tenon and the sizes of each.
`library.new` starts a part from one (an ordinary part: its sizes are the rows of its design
table, and a section's length is its parameter `L`). In an assembly, `asm.insert_standard`
places one in a size: the part's file is written once into the folder given by `path`, beside
the assembly, and every size comes from that file. `asm.set_row` changes a component's size.

```json
{ "run": "library.new", "with": { "part": "t-slot", "size": "20x20 slot 6" } },
{ "run": "param.set", "with": { "name": "L", "equation": "150 mm" } },
{ "run": "file.save", "with": { "path": "bar.tenon" } },
{ "run": "asm.new", "with": { "name": "Capped Bar" } },
{ "run": "asm.insert", "with": { "path": "bar.tenon" } },
{ "run": "asm.insert_standard", "with": { "part": "socket-screw-iso-4762", "size": "M5x12", "path": "standard", "at": [80, 0, 150] }, "as": "screw" },
{ "run": "asm.set_row", "with": { "component": "$screw.component", "row": "M5x16" } }
```

`print.check` reads the open part as a printer would: whether its surface is closed, what
overhangs past a limit (`overhang`, degrees from vertical, 45 unless given) with `up` one of
`x`, `y`, `z`, `-x`, `-y`, `-z`, how thin its thinnest wall is against `min_wall` (0.8 mm unless
given), and whether it fits a `bed` of `[x, y, height]`. Each finding has its own `ok` and names
the faces. Areas are of the part's triangles, so a round one is a little under its exact value:
give such a step a `within`.

```json
{ "run": "print.check", "expect": { "watertight.ok": true, "overhangs.area": 392.4, "walls.thinnest": 1.8 }, "within": 0.001 },
{ "run": "print.check", "with": { "up": "-z", "min_wall": 2, "bed": [220, 220, 250] }, "expect": { "walls.ok": false, "size.ok": true } },
{ "run": "export.3mf", "with": { "path": "end-cap.3mf" }, "expect": { "objects": 1, "watertight": true } },
{ "run": "export.stl", "with": { "path": "end-cap.stl" }, "expect": { "watertight": true } },
{ "run": "asm.export_3mf", "with": { "path": "capped-bar.3mf" }, "expect": { "objects": 3, "items": 3 } }
```

The exports take `linear` (mm) and `angular` (rad), the furthest and the sharpest a triangle may
depart from the surface; they are kept over the whole surface. `examples/m7-end-cap/end-cap.json`
uses all of this.

## Sketches on the part: projected edges, centre lines and the sketch tools

A sketch on a face can show the face's edges (`project_edges`), and `sketch.project` shows any
edge made before the sketch, or every edge of a face. What is shown is reference geometry: it
is found again each time the part is rebuilt, the sketch's own geometry is dimensioned to it,
and it bounds profiles. `sketch.info` marks it `reference` and lists the sketch's
`projections`, each with `lost` when its edge is gone. `sketch.break_link` makes it the
sketch's own.

```json
{ "run": "model.face_ref", "with": { "origin": { "type": "cap", "feature": "$block.feature", "end": "end" } }, "as": "top" },
{ "run": "sketch.create", "with": { "face": "$top", "project_edges": true }, "as": "s" },
{ "run": "sketch.circle", "with": { "sketch": "$s.feature", "cx": 5, "cy": 5, "r": 3 }, "as": "hole" },
{ "run": "sketch.constrain", "with": { "sketch": "$s.feature", "constraint": { "type": "distance", "a": "$hole.center", "b": "$s.projected.0", "value": 8 } } },
{ "run": "model.extrude", "with": { "sketch": "$s.feature", "through_all": true, "reverse": true, "operation": "cut" } }
```

`projected` lists the new reference curves in the order of the face's edges. The cut takes the
circle, not the face round it: with no `regions` given, a sketch's own closed geometry comes
before the outline it only shows.

`sketch.centerline` gives a line the Centerline format: `model.revolve` turns about it when no
axis is given, and a `linear_diameter` dimension (`point`, `axis`, `value`) is a diameter across
it.

The Modify and Pattern tools are commands too: `sketch.move`, `sketch.rotate` and
`sketch.scale` (each with `copy`), `sketch.extend`, `sketch.split`, `sketch.pattern.rect` and
`sketch.pattern.circular`; and so are the shapes of the drop-downs: `sketch.slot`,
`sketch.rectangle_center`, `sketch.rectangle3`, `sketch.arc_tangent`. Angles given to sketch
commands are in radians. [commands.md](commands.md) lists what each takes.

## Sheet metal and the flat pattern

`sheet.defaults` makes the part a sheet metal part and sets its rule. Each size is a number
(millimetres) or an equation, and is a parameter of the part from then on (`Thickness`,
`BendRadius`, ...), so `param.set` changes it and equations use it. `sheet.face` makes a sketch
profile one thickness thick; `sheet.flange` bends walls up from edges of the sheet; `sheet.hem`
and `sheet.contour_flange` are below, with the corner seam and rip, the bends, the fold and the
cuts of the sheet. Other cuts and holes are the ordinary commands.

```json
{ "run": "sheet.defaults", "with": { "thickness": 1.5, "corner": "round" }, "expect": { "bend_radius": 1.5, "gap": 1.5, "k_factor": 0.44 } },
{ "run": "sketch.create", "with": { "plane": "xy" }, "as": "s" },
{ "run": "sketch.rectangle", "with": { "sketch": "$s.feature", "x1": 0, "y1": 0, "x2": 120, "y2": 80 }, "as": "r" },
{ "run": "sheet.face", "with": { "sketch": "$s.feature" }, "as": "face" },
{ "run": "model.edge_ref", "with": { "faces": [{ "type": "cap", "feature": "$face.feature", "end": "end" }, { "type": "side", "feature": "$face.feature", "curve": "$r.lines.0" }] }, "as": "front" },
{ "run": "sheet.flange", "with": { "edges": ["$front"], "height": 25 } },
{ "run": "model.edge_ref", "with": { "at": [60, 0, 25] }, "as": "wall_top" },
{ "run": "sheet.flange", "with": { "edges": ["$wall_top"], "height": 12, "width": 40 } },
{ "run": "sheet.flat", "expect": { "bends.0.allowance": 3.39292006588 } },
{ "run": "sheet.export_dxf", "with": { "path": "flat.dxf" } }
```

A flange bends towards the side its edge is on: an edge of the face's top bends up. Its `angle`
is in radians (a quarter turn unless given), its `height` is overall unless `datum` says
otherwise, and `position` says where the bend lies (`inside`, the default, keeps the part's
outside size). The faces a flange makes have names a script cannot spell, so an edge or a face
on one is named by a point on it: `model.edge_ref` and `model.face_ref` take `at: [x, y, z]`.

Edges of one flange that meet at a corner of the sheet have their walls brought together until
the rule's `gap` is left between them (`miter: false` leaves each ending where its bend does),
and the corner gets the rule's relief, centred where the two bends' centre lines cross.

`sheet.hem` folds an edge back on itself (`type`: `single`, `double`, `rolled` or `teardrop`),
and `sheet.contour_flange` makes sheet along an open profile, with a bend at every corner. A
channel 30 wide and 20 high outside, 50 long, with a double hem along the top of one side:

```json
{ "run": "sheet.defaults", "with": { "thickness": 2, "bend_radius": 3 } },
{ "run": "sketch.create", "with": { "plane": "xz" }, "as": "s" },
{ "run": "sketch.line", "with": { "sketch": "$s.feature", "x1": 0, "y1": 20, "x2": 0, "y2": 0 }, "as": "a" },
{ "run": "sketch.line", "with": { "sketch": "$s.feature", "start": "$a.end", "x2": 30, "y2": 0 }, "as": "b" },
{ "run": "sketch.line", "with": { "sketch": "$s.feature", "start": "$b.end", "x2": 30, "y2": 20 } },
{ "run": "sheet.contour_flange", "with": { "sketch": "$s.feature", "distance": 50 } },
{ "run": "model.mass", "expect": { "bodies.0.volume": 6256.63706144 } },
{ "run": "model.edge_ref", "with": { "at": [28, -25, 20] }, "as": "top" },
{ "run": "sheet.hem", "with": { "edges": ["$top"], "type": "double", "gap": 1, "length": 12 } },
{ "run": "sheet.flat", "expect": { "length": 86.5725642002, "bends.2.angle": 3.14159265359 } }
```

The profile's curves must join end to end: start each where the last one ended (`"start":
"$a.end"`; `sketch.arc` returns its `start` and `end` too). The sheet goes on the inside of the
profile's first corner unless `position` is `flipped` or `symmetric`, so the profile above is
the outside of the channel. A hem's `length` is over all, from the outside of its fold.

Flanges made one after the other leave the corner between them open: the first runs the whole
length of its edge, and the second starts where the first one's bend left its own.
`sheet.corner_seam` closes it, given the edge at the end of each of the two walls (the first
wall's, then the second's), and makes of it the corner one flange makes of both edges:

```json
{ "run": "sheet.defaults", "with": { "thickness": 2, "bend_radius": 3, "corner": "round" } },
{ "run": "sketch.create", "with": { "plane": "xy" }, "as": "s" },
{ "run": "sketch.rectangle", "with": { "sketch": "$s.feature", "x1": 0, "y1": 0, "x2": 60, "y2": 40 }, "as": "r" },
{ "run": "sheet.face", "with": { "sketch": "$s.feature" }, "as": "face" },
{ "run": "model.edge_ref", "with": { "faces": [{ "type": "cap", "feature": "$face.feature", "end": "end" }, { "type": "side", "feature": "$face.feature", "curve": "$r.lines.0" }] }, "as": "front" },
{ "run": "sheet.flange", "with": { "edges": ["$front"], "height": 15 } },
{ "run": "model.edge_ref", "with": { "faces": [{ "type": "cap", "feature": "$face.feature", "end": "end" }, { "type": "side", "feature": "$face.feature", "curve": "$r.lines.1" }] }, "as": "right" },
{ "run": "sheet.flange", "with": { "edges": ["$right"], "height": 15 } },
{ "run": "model.edge_ref", "with": { "at": [60, 0, 10] }, "as": "front_end" },
{ "run": "model.edge_ref", "with": { "at": [60, 5, 10] }, "as": "right_end" },
{ "run": "sheet.corner_seam", "with": { "edges": ["$front_end", "$right_end"], "seam": "overlap" } },
{ "run": "model.mass", "expect": { "bodies.0.volume": 6880.36303831 } }
```

`seam` is `symmetric` (the default: both walls stop the gap apart), `overlap` (the first wall
runs across the end of the second) or `reverse_overlap`; `overlap` says how much of the other
wall's thickness is covered (1 unless given), `gap` and `relief`, `relief_size` stand in for
the rule's. `sheet.flange` takes the same five for the corners it makes itself, where the
first wall is the one whose edge comes first in `edges`.

A face made apart from the rest of the sheet is a second piece of it, and `sheet.bend` joins
two such faces: given a straight edge of the rim of each (edges that run alongside one
another), each face is trimmed or extended to where the bend begins. Here a wall stands over
the plate's left edge, 10 above it; the plate is cut back by 5 and the wall let down by 5 to a
bend of inside radius 3:

```json
{ "run": "sheet.defaults", "with": { "thickness": 2, "bend_radius": 3 } },
{ "run": "sketch.create", "with": { "plane": "xy" }, "as": "s" },
{ "run": "sketch.rectangle", "with": { "sketch": "$s.feature", "x1": 0, "y1": 0, "x2": 40, "y2": 30 } },
{ "run": "sheet.face", "with": { "sketch": "$s.feature" } },
{ "run": "sketch.create", "with": { "plane": "yz" }, "as": "w" },
{ "run": "sketch.rectangle", "with": { "sketch": "$w.feature", "x1": 0, "y1": 10, "x2": 30, "y2": 40 } },
{ "run": "sheet.face", "with": { "sketch": "$w.feature" } },
{ "run": "model.edge_ref", "with": { "at": [0, 15, 2] }, "as": "plate" },
{ "run": "model.edge_ref", "with": { "at": [2, 15, 10] }, "as": "wall" },
{ "run": "sheet.bend", "with": { "edges": ["$plate", "$wall"] } },
{ "run": "model.mass", "expect": { "bodies.0.volume": 4576.99111843 } },
{ "run": "sheet.flat", "expect": { "length": 76.0946897480, "width": 30 } }
```

Faces in planes that cross take one bend. Faces in parallel planes take two, as `double` says:
`fix_edges` (the default: two equal bends between the edges as they are), `45_degree` or
`90_degree` (bends of that angle with a face between them), or, for faces one over the other,
`full_radius` (half a circle as wide inside as the faces are apart) or `90_degree`. Where a
face is trimmed or extended to the bends it is the second one, or with `flip_fixed` the first.
`radius` is the bend's own inside radius in place of the rule's.

`sheet.cut_across_bend` cuts a profile out of the sheet as it lies flat, so the cut goes round
the bends it is drawn over. Its sketch is on a side of the sheet (a face of it, or a plane that
lies on one: here the XY plane, the underside of the plate), and what is drawn past an edge
that is a bend line lies on the bend, then on the face beyond. `sketch.project` with
`flat: true` shows a face of the part where it lies on the sheet laid flat, to draw on. This
circle is centred on the bend line: half of it is cut out of the plate and half out of the
bend, and in the flat pattern it is the circle that was drawn:

```json
{ "run": "sheet.defaults", "with": { "thickness": 2, "bend_radius": 3 } },
{ "run": "sketch.create", "with": { "plane": "xy" }, "as": "s" },
{ "run": "sketch.rectangle", "with": { "sketch": "$s.feature", "x1": 0, "y1": 0, "x2": 60, "y2": 40 }, "as": "r" },
{ "run": "sheet.face", "with": { "sketch": "$s.feature" }, "as": "face" },
{ "run": "model.edge_ref", "with": { "faces": [{ "type": "cap", "feature": "$face.feature", "end": "end" }, { "type": "side", "feature": "$face.feature", "curve": "$r.lines.1" }] }, "as": "right" },
{ "run": "sheet.flange", "with": { "edges": ["$right"], "height": 15 } },
{ "run": "sketch.create", "with": { "plane": "xy" }, "as": "c" },
{ "run": "model.face_ref", "with": { "at": [60, 20, 10] }, "as": "wall" },
{ "run": "sketch.project", "with": { "sketch": "$c.feature", "face": "$wall", "flat": true } },
{ "run": "sketch.circle", "with": { "sketch": "$c.feature", "cx": 55, "cy": 20, "r": 4 } },
{ "run": "sheet.cut_across_bend", "with": { "sketch": "$c.feature" } },
{ "run": "model.mass", "expect": { "bodies.0.volume": 5600.56925724 } },
{ "run": "sheet.flat", "expect": { "holes": 1, "area": 2793.52210746 } }
```

`distance` makes it a pocket that deep from the side the sketch is on; without it, or at the
thickness or more, the cut goes through. The profile is of lines, arcs and circles.

`sheet.cut_normal` cuts a profile out of the sheet where it is seen on it, looking along its
sketch's normal, and cuts every face and bend it is seen on square to itself. Its sketch may
be on any plane that looks at the sheet, at any angle. Here the same circle is on a work plane
30 above the plate: half of it is seen on the plate, and the rest on the inside of the bend,
which is seen more and more edge on, so the hole is longer in the flat pattern than the circle:

```json
{ "run": "sheet.defaults", "with": { "thickness": 2, "bend_radius": 3 } },
{ "run": "sketch.create", "with": { "plane": "xy" }, "as": "s" },
{ "run": "sketch.rectangle", "with": { "sketch": "$s.feature", "x1": 0, "y1": 0, "x2": 60, "y2": 40 }, "as": "r" },
{ "run": "sheet.face", "with": { "sketch": "$s.feature" }, "as": "face" },
{ "run": "model.edge_ref", "with": { "faces": [{ "type": "cap", "feature": "$face.feature", "end": "end" }, { "type": "side", "feature": "$face.feature", "curve": "$r.lines.1" }] }, "as": "right" },
{ "run": "sheet.flange", "with": { "edges": ["$right"], "height": 15 } },
{ "run": "work.plane", "with": { "base": "xy", "distance": 30 }, "as": "above" },
{ "run": "sketch.create", "with": { "work_plane": "$above.feature" }, "as": "c" },
{ "run": "sketch.circle", "with": { "sketch": "$c.feature", "cx": 55, "cy": 20, "r": 4 } },
{ "run": "sheet.cut_normal", "with": { "sketch": "$c.feature" } },
{ "run": "model.mass", "expect": { "bodies.0.volume": 5568.00712667 }, "within": 1e-4, "note": "the hole's walls on the bend are curves through points" },
{ "run": "sheet.flat", "expect": { "holes": 1, "area": 2777.72964342 }, "within": 1e-4 }
```

`sheet.fold` folds a face along a line drawn across it, from edge to edge, in a sketch on a
side of the sheet. Nothing is added: the sheet beyond the line turns about a bend, and laid
flat the part is the plate it was. `angle` is in radians (a quarter turn unless given),
`radius` stands in for the rule's, `location` says where the line is in the bend's zone
(`centerline`, the default, `start` or `end`), `flip_side` folds the sheet on the other side
of the line and `flip_direction` folds it down. A sketch with more than one line says which
with `line`:

```json
{ "run": "sheet.defaults", "with": { "thickness": 2, "bend_radius": 3 } },
{ "run": "sketch.create", "with": { "plane": "xy" }, "as": "s" },
{ "run": "sketch.rectangle", "with": { "sketch": "$s.feature", "x1": 0, "y1": 0, "x2": 60, "y2": 40 } },
{ "run": "sheet.face", "with": { "sketch": "$s.feature" } },
{ "run": "model.face_ref", "with": { "at": [30, 20, 2] }, "as": "top" },
{ "run": "sketch.create", "with": { "face": "$top", "project_edges": false }, "as": "l" },
{ "run": "sketch.line", "with": { "sketch": "$l.feature", "x1": 40, "y1": 0, "x2": 40, "y2": 40 } },
{ "run": "sheet.fold", "with": { "sketch": "$l.feature" } },
{ "run": "model.mass", "expect": { "bodies.0.volume": 4815.07964474 } },
{ "run": "sheet.flat", "expect": { "area": 2400, "cut_length": 200, "bends.0.angle": 1.57079632679 } }
```

`sheet.contour_flange` with `edges` runs its profile along edges of the sheet in place of
pushing it out of its sketch. The sketch is square to the first edge and the profile starts
at it; the side of the sheet the edge is on runs on into the profile, with a bend where the
profile leaves the edge at an angle. `width` and `offsets` say how much of each edge, as a
flange's do. A wall 15 high and a lip 15 out along the plate's right edge:

```json
{ "run": "sheet.defaults", "with": { "thickness": 2, "bend_radius": 3 } },
{ "run": "sketch.create", "with": { "plane": "xy" }, "as": "s" },
{ "run": "sketch.rectangle", "with": { "sketch": "$s.feature", "x1": 0, "y1": 0, "x2": 60, "y2": 40 } },
{ "run": "sheet.face", "with": { "sketch": "$s.feature" } },
{ "run": "sketch.create", "with": { "plane": "xz" }, "as": "p" },
{ "run": "sketch.line", "with": { "sketch": "$p.feature", "x1": 60, "y1": 2, "x2": 60, "y2": 17 }, "as": "a" },
{ "run": "sketch.line", "with": { "sketch": "$p.feature", "start": "$a.end", "x2": 75, "y2": 17 } },
{ "run": "model.edge_ref", "with": { "at": [60, 20, 2] }, "as": "right" },
{ "run": "sheet.contour_flange", "with": { "sketch": "$p.feature", "edges": ["$right"] } },
{ "run": "model.mass", "expect": { "bodies.0.volume": 6925.30964915 } },
{ "run": "sheet.flat", "expect": { "length": 86.1893794959, "width": 40 } }
```

`sheet.corner_rip` opens a square corner where two walls meet with no seam between them: the
corners of a box that was shelled, not folded. Each edge given is the outside or the inside
edge of such a corner, and the rip is as long as the edge. `seam`, `overlap` and `gap` are a
corner seam's: `symmetric` cuts both walls back until their inner faces are the gap apart,
`overlap` keeps one wall whole and stops the other the gap short of it (`reverse_overlap`
the other way round). The edges where the walls meet the floor stay sharp: `sheet.sharp_bend`,
below, makes bends of them:

```json
{ "run": "sheet.defaults", "with": { "thickness": 2, "gap": 1 } },
{ "run": "sketch.create", "with": { "plane": "xy" }, "as": "s" },
{ "run": "sketch.rectangle", "with": { "sketch": "$s.feature", "x1": 0, "y1": 0, "x2": 60, "y2": 40 } },
{ "run": "model.extrude", "with": { "sketch": "$s.feature", "distance": 20 } },
{ "run": "model.face_ref", "with": { "at": [30, 20, 20] }, "as": "top" },
{ "run": "model.shell", "with": { "remove": ["$top"], "thickness": 2 } },
{ "run": "model.edge_ref", "with": { "at": [0, 0, 10] }, "as": "corner" },
{ "run": "sheet.corner_rip", "with": { "edges": ["$corner"] } },
{ "run": "model.mass", "expect": { "bodies.0.volume": 11574.4314575 } },
{ "run": "model.edge_ref", "with": { "at": [60, 0, 10] }, "as": "next" },
{ "run": "sheet.corner_rip", "with": { "edges": ["$next"], "seam": "overlap" } },
{ "run": "model.mass", "expect": { "bodies.0.volume": 11534.4314575 } }
```

`sheet.sharp_bend` makes bends of sharp edges: each edge given is where two faces of the sheet
meet with no bend between them, its outside edge or its inside one. Every corner is cleared
back to where a bend begins on either face and the bend put in, tangent to both, so the outer
faces stay where they are. Edges that meet at a corner of the sheet are given together: their
bends then end where their bend lines cross, and the walls hang on past them. With the
corners ripped first, a shelled box becomes the tray a flange would make, and lays flat:

```json
{ "run": "sheet.defaults", "with": { "thickness": 2, "bend_radius": 3, "gap": 1 } },
{ "run": "sketch.create", "with": { "plane": "xy" }, "as": "s" },
{ "run": "sketch.rectangle", "with": { "sketch": "$s.feature", "x1": 0, "y1": 0, "x2": 60, "y2": 40 } },
{ "run": "model.extrude", "with": { "sketch": "$s.feature", "distance": 20 } },
{ "run": "model.face_ref", "with": { "at": [30, 20, 20] }, "as": "top" },
{ "run": "model.shell", "with": { "remove": ["$top"], "thickness": 2 } },
{ "run": "model.edge_ref", "with": { "at": [0, 0, 10] }, "as": "c1" },
{ "run": "model.edge_ref", "with": { "at": [60, 0, 10] }, "as": "c2" },
{ "run": "model.edge_ref", "with": { "at": [60, 40, 10] }, "as": "c3" },
{ "run": "model.edge_ref", "with": { "at": [0, 40, 10] }, "as": "c4" },
{ "run": "sheet.corner_rip", "with": { "edges": ["$c1", "$c2", "$c3", "$c4"] } },
{ "run": "model.edge_ref", "with": { "at": [30, 0, 0] }, "as": "front" },
{ "run": "model.edge_ref", "with": { "at": [60, 20, 0] }, "as": "right" },
{ "run": "model.edge_ref", "with": { "at": [30, 40, 0] }, "as": "back" },
{ "run": "model.edge_ref", "with": { "at": [0, 20, 0] }, "as": "left" },
{ "run": "sheet.sharp_bend", "with": { "edges": ["$front", "$right", "$back", "$left"] } },
{ "run": "model.mass", "expect": { "bodies.0.volume": 10360.9136708 } },
{ "run": "sheet.flat", "expect": { "length": 92.1893794959, "width": 72.1893794959, "bends.3.angle": 1.57079632679 } }
```

A bend whose end would lie against sheet that runs on beside it is refused, saying so: a
corner that was not ripped, or the corner of the next edge when the edges are bent one at a
time. `radius` is the bends' own inside radius in place of the rule's.

`sheet.flat` lays the part flat and returns the size of the sheet (`length`, `width`), its
`area`, the `cut_length`, the number of cut-outs (`holes`) and every bend's `angle`, `radius`,
`allowance` and whether it is `up`; with `curves: true`, the outlines too. `sheet.export_dxf`
and `sheet.export_svg` write it. `sheet.a_side` chooses the face that is up.
`examples/m8-tray/tray.json` uses all of this, with every number worked out by hand in the
README beside it.
