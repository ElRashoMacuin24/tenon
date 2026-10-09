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

## Changing the format

Every schema change bumps `version`, adds a migration from the previous version, and adds a
round-trip test. Changing the container needs the project owner's approval.
