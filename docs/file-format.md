# Native project format (version 1)

A Tenon project is a **zip file** with the extension `.tenon`. The container was approved in
DEC-004; changing it needs the project owner's approval. Implemented in
`crates/io/src/project.rs`, tested in `crates/io/tests/project.rs`.

## Container

| Entry | Required | Content |
|---|---|---|
| `project.json` | yes | The whole document (below), UTF-8 JSON. Deflate-compressed. |
| `cache/*` | no | Reserved for kernel B-rep caches keyed by backend and version. Never authoritative; ignored by version 1. |
| `thumbnail.png` | no | Reserved. |

Limits when reading:
- `project.json` at most 64 MiB uncompressed;
- the file at most 256 MiB;
- at most 10 000 features, and 20 000 entities and 20 000 constraints per sketch.

Files over a limit are rejected with an error, never a crash.

## `project.json`

```json
{
  "format": "tenon",
  "version": 1,
  "generator": "tenon 0.1.0",
  "document": { "name": "Part1", "features": [ ... ], "next_feature": 4 }
}
```

- `format` must be `"tenon"`.
- `version` is an integer. A file with a newer version than the reader supports is refused with a
  clear message.
- Unknown **top-level** fields are kept when the file is saved again. Unknown fields inside
  `document` are not kept in version 1.

### Units and ids

- Millimetres and radians everywhere.
- Feature ids, sketch entity ids and constraint ids are positive integers.
- Ids are never reused within their counter (`next_feature`, and per sketch `next_entity` and
  `next_constraint`), because persistent references and profile-curve tags refer to them.
- Kernel indices are never stored.

### Features

Each feature is:

```json
{ "id": 3, "name": "Extrusion1", "suppressed": false, "kind": { "type": "...", ... } }
```

| `kind.type` | Fields |
|---|---|
| `sketch` | `plane`, `sketch` |
| `extrude` | `sketch` (feature id), `regions`, `extent`, `reverse`, `operation` |
| `revolve` | `sketch`, `regions`, `axis`, `angle`, `operation` |

Field values:

- **`plane`:**
  - `{"origin": "XY" | "YZ" | "XZ"}`; or
  - `{"face": FaceRef}`, a planar face of the part (below).
- **`regions`:**
  - `"default"`: every region at even nesting depth; or
  - `{"keys": [[curve ids], ...]}`: each key is the sorted ids of a region's outer boundary.
- **`extent`:**
  - `{"distance": d}`
  - `{"symmetric": d}`
  - `{"two_sided": {"forward": a, "backward": b}}`
  - `"through_all"`
- **`operation`:** `"join"` (default), `"cut"`, `"new_body"` or `"intersect"`.
- **`axis`:**
  - `{"sketch_line": entity id}`; or
  - `{"origin": "X" | "Y" | "Z"}`.
- **`angle`:** `"full"`, `{"angle": a}` or `{"symmetric": a}`.

Features may only refer to features that come before them.

### Sketches

```json
{
  "entities":    [[1, {"geometry": {"type": "point", "pos": {"x": 0, "y": 0}}, "construction": false}], ...],
  "constraints": [[1, {"type": "horizontal", "line": 5}], ...],
  "next_entity": 9,
  "next_constraint": 4
}
```

Entities and constraints are lists of `[id, value]` pairs, not JSON objects keyed by id. JSON
object keys would be strings, and those do not round-trip as integer ids.

Geometry types:

| `type` | Fields |
|---|---|
| `point` | `pos` |
| `line` | `start`, `end` (point ids) |
| `circle` | `center` (point id), `radius` |
| `arc` | `center`, `start`, `end` (point ids; counter-clockwise; radius is \|start − center\|) |
| `spline` | `poles` (point ids), `degree` (clamped uniform B-spline) |

Constraint types:

- **Geometric:** `coincident`, `point_on_curve`, `horizontal`, `vertical`, `parallel`,
  `perpendicular`, `collinear`, `tangent`, `concentric`, `equal`, `symmetric`, `midpoint`, `fix`.
- **Dimensional:** `distance`, `horizontal_distance`, `vertical_distance`, `length`, `angle`,
  `radius`, `diameter`. Each has a `value`.
- **Field names:** listed in `crates/sketch/src/constraint.rs`.

### Face references

```json
{ "origin": { "type": "cap", "feature": 2, "end": "end" },
  "fingerprint": { "surface": "plane", "direction": {"x":0,"y":0,"z":1},
                   "centroid": {"x":30,"y":20,"z":8}, "area": 2400.0 } }
```

`origin` is one of:
- `{"type": "cap", "feature", "end": "start" | "end"}`;
- `{"type": "side", "feature", "curve"}`, the face swept from a sketch curve.

The fingerprint only breaks ties between pieces of a split face (docs/persistent-naming.md).

## Loading

After parsing, the document is validated:
- ids are unique and within their counters;
- references point to earlier features;
- sketch references point to the right entity types;
- numbers are finite and in range.

A file that fails validation is reported as damaged.

## Assemblies (`.tenonasm`, version 1)

An assembly is its own file (DEC-024): the same zip container, with `project.json` holding

```json
{
  "format": "tenon-assembly",
  "version": 1,
  "generator": "tenon 0.1.0",
  "assembly": {
    "name": "Pivot",
    "components": [
      { "id": 1, "name": "base:1", "part": "base.tenon",
        "placement": { "origin": {...}, "x": {...}, "y": {...}, "z": {...} },
        "grounded": true, "visible": true }
    ],
    "relationships": [
      { "id": 1, "name": "Rotational:1", "suppressed": false,
        "kind": { "type": "joint", "joint": "revolute", "a": Target, "b": Target,
                  "flip": false, "offset": 0.0, "angle": 0.0 } }
    ],
    "explode": [ { "components": [2, 3], "direction": {...}, "distance": 30.0 } ],
    "next_component": 6,
    "next_relationship": 6
  }
}
```

Implemented in `crates/io/src/asm.rs` and `crates/assembly/src/model.rs`; tested in
`crates/io/tests/asm.rs`.

- **`part`** is the part file's path relative to the assembly file's folder, with `/`
  separators. A part on another drive is stored as a full path. Opening resolves it against
  the folder the assembly is opened from, so the folder can move. A part that cannot be read
  is reported and its component shown as missing.
- **`placement`** maps part coordinates to assembly coordinates: a right-handed orthonormal
  frame. A frame that is not orthonormal (to 1e-6) or has coordinates beyond 1 km is refused.
- **`name`** is the part file's name and an occurrence number.
- **Ids** are positive and never reused within their counter.
- **`kind.type`:**
  - `mate`, `flush` (`offset`);
  - `angle` (`angle` in radians, `reference`: a unit vector in A's part coordinates);
  - `insert` (`offset`, `aligned`);
  - `joint`, with `joint` being `rigid`, `revolute`, `slider`, `cylindrical`, `planar` or
    `ball`.
- **A `Target`** is `{ "component": id, "geom": Geom }`. Without `component` it is the
  assembly's own origin geometry (only `plane`, `axis` and `origin`). `Geom` is one of:
  - `{ "type": "face", "face": FaceRef }`;
  - `{ "type": "edge", "edge": EdgeRef }` (persistent references into the part, as in part
    files);
  - `{ "type": "plane", "plane": "XY" | "YZ" | "XZ" }`;
  - `{ "type": "axis", "axis": "X" | "Y" | "Z" }`;
  - `{ "type": "origin" }`;
  - `{ "type": "work", "feature": id }`.
- **Validation:** relationships must name existing components, and never join a component to
  itself. Exploded-view steps need a unit `direction`. Limits: 5 000 components and 20 000
  relationships.

## Drawings (`.tenondrw`, version 1)

A drawing is its own file (DEC-026), in the same zip container. It holds the sheets, the views
and the annotations, never geometry: views are computed again from the model files each time the
drawing is opened or updated, so they always show the models as they are.

```json
{
  "format": "tenon-drawing",
  "version": 1,
  "generator": "tenon 0.1.0",
  "drawing": {
    "name": "Plate",
    "standard": "ansi",
    "props": { "title": "MOUNTING PLATE", "number": "TN-0004", "revision": "A",
               "company": "", "drawn_by": "TENON", "date": "2026-10-08" },
    "sheets": [
      { "id": 1, "name": "Sheet:1",
        "size": { "name": "B", "width": 431.8, "height": 279.4 },
        "title_block": { "name": "ANSI", "lines": [ { "a": {...}, "b": {...} } ],
                         "fields": [ { "key": "title", "label": "TITLE", "at": {...}, "height": 5.0 } ],
                         "projection_symbol": { "x": -32.5, "y": 18.0 } },
        "border": true }
    ],
    "views": [
      { "id": 1, "sheet": 1, "name": "VIEW1", "model": "plate.tenon",
        "kind": { "type": "base", "orientation": "front" },
        "scale": 1.0, "center": { "x": 100.0, "y": 70.0 },
        "hidden": true, "tangent": false, "centerlines": true, "label": false },
      { "id": 2, ..., "kind": { "type": "projected", "parent": 1, "side": "above" } },
      { "id": 5, ..., "kind": { "type": "section", "parent": 2, "a": {...}, "b": {...}, "flip": true } },
      { "id": 6, ..., "kind": { "type": "detail", "parent": 1, "center": {...}, "radius": 10.0 } }
    ],
    "annotations": [
      { "id": 1, "kind": { "type": "dimension", "view": 2, "dim": "horizontal",
                           "a": GeomPick, "offset": { "x": 0.0, "y": 52.0 }, "precision": 2 } },
      { "id": 5, "kind": { "type": "hole_table", "view": 2, "at": {...} } },
      { "id": 6, "kind": { "type": "note", "sheet": 1, "at": {...}, "text": "...", "height": 3.0 } },
      { "id": 7, "kind": { "type": "parts_list", "view": 7, "at": {...} } },
      { "id": 8, "kind": { "type": "balloon", "view": 7, "component": 1,
                           "attach": {...}, "offset": {...} } }
    ],
    "next_sheet": 3, "next_view": 8, "next_annotation": 10
  }
}
```

Implemented in `crates/io/src/drw.rs` and `crates/drawing/src/model.rs`; tested in
`crates/io/tests/drw.rs`.

- **Units.** Sheet positions are millimetres of paper from the sheet's bottom-left corner.
  Title block geometry is measured from the bottom-right corner (negative `x`). Section lines and
  detail circles are in their parent view's own coordinates, in model millimetres.
- **`model`** is the part or assembly file's path relative to the drawing's folder, with `/`
  separators, as in assemblies. A model that cannot be read is reported, and its views are empty
  with the reason.
- **`standard`**: `ansi` (third-angle projection) or `iso` (first-angle) (DEC-027). It decides
  where projected views look from and which projection symbol the title block shows.
- **View kinds:**
  - `base`, with `orientation` one of `front`, `back`, `top`, `bottom`, `left`, `right`, `iso`;
  - `projected`, with `parent` and `side` (`right`, `left`, `above`, `below`, or a corner such
    as `above_right` for an isometric view);
  - `section`, with `parent`, the cutting line `a`-`b`, and `flip` (seen towards the left of
    `a`-`b` instead of the right);
  - `detail`, with `parent`, `center` and `radius`.
  Parents come before their children, so there are no cycles; a view shows the same model as its
  parent.
- **A `GeomPick`** is `{ "edge": EdgeRef, "point": "whole" | "start" | "end" | "mid" |
  "center", "component": id }`: a persistent edge reference into the part (as in part files),
  the component for assembly views, and which point of the edge is meant. Dimensions keep picks,
  never values: a value is measured again from the model whenever the drawing is shown.
- **Annotation kinds:** `dimension` (`dim`: `horizontal`, `vertical`, `aligned`, `diameter`,
  `radius`, `angle`; `a`, optional `b`; `offset` from the view's centre; optional `text`, where
  `<>` stands for the value; `precision`), `hole_table`, `parts_list`, `balloon` (`attach` in
  the component's part coordinates, `offset` from the view's centre), `note`.
- **Validation:** ids are positive, unique and below their counter; views sit on existing
  sheets and refer to earlier views; annotations refer to existing views or sheets; scales are
  between 1e-4 and 1e4; sizes, positions and text lengths are bounded. Limits: 500 sheets,
  5 000 views and 50 000 annotations.

## Drawing templates

A drawing template is an ordinary `.tenondrw` file. Starting a drawing from one
(`drw.new` with `template`, File > New Drawing from Template) takes its standard, properties and
sheets (sizes, borders, title blocks) with the notes on them; views, and the dimensions, tables
and balloons that need a model, are left out, and no model file is read. `drw.save_template`
writes such a file from the open drawing. Tenon ships an ANSI B and an ISO A3 template in
`assets/templates`. Implemented in `crates/io/src/drw.rs`; tested in `crates/io/tests/drw.rs`.

## Title block templates (`.json`, version 1)

A title block template is a plain JSON file (`drw.template.save` writes one; Manage > Apply
Template or `drw.template.apply` uses one on a drawing's sheets):

```json
{
  "format": "tenon-title-block",
  "version": 1,
  "title_block": {
    "name": "ACME",
    "lines": [ { "a": { "x": -180.0, "y": 10.0 }, "b": { "x": -10.0, "y": 10.0 } } ],
    "fields": [
      { "key": "text", "label": "ACME WIDGETS", "at": { "x": -178.0, "y": 40.0 }, "height": 5.0 },
      { "key": "title", "label": "TITLE", "at": { "x": -98.0, "y": 40.0 }, "height": 5.0 }
    ],
    "projection_symbol": { "x": -32.5, "y": 18.0 }
  }
}
```

Positions are millimetres from the sheet's bottom-right corner, so one template fits every sheet
size. A field's `key` says what fills it: a drawing property (`title`, `number`, `revision`,
`company`, `drawn_by`, `date`), a computed value (`scale`, `sheet`, `size`, `units`), or `text`
for the label alone. Each sheet keeps its own copy, so a drawing does not depend on the template
file. Implemented in `crates/io/src/drw.rs`; tested in `crates/io/tests/drw.rs`.

## Changing the format

Every schema change bumps `version`, adds a migration from the previous version, and adds a
round-trip test. Changing the container needs the project owner's approval.
