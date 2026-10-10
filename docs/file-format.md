# Tenon file formats (version 2)

Parts (`.tenon`), assemblies (`.tenonasm`) and drawings (`.tenondrw`) are plain UTF-8 text files
in TOML, with a layout Tenon always writes the same way (DEC-031, chosen by the owner). They
diff and merge in Git like source code. Version-1 files (zips) are still read and are upgraded
when saved (see [Version 1](#version-1-read-only)).

Implemented in `crates/io/src/{text,layout,project,asm,drw}.rs`. Tested in
`crates/io/src/{text,layout}.rs` and `crates/io/tests/{format_v2,project,asm,drw}.rs`.

## Text files

The rules for all three kinds of document:

- **Encoding.** UTF-8. Files are written with LF line ends and a final newline. CRLF (as Git
  writes on Windows) and a byte-order mark are read too.
- **First line.** A comment naming the kind of file and its format version. Comments are not
  kept when the file is saved again.
- **Order.**
  1. `format` and `version`.
  2. The document's plain fields (`name`, the id counters).
  3. Its sections, such as `[parameters]` or `[props]`.
  4. Its records: one table per feature, component, sheet, view and so on (`[[feature]]`).
- **Inside a section or record:**
  - one field per line, in a fixed order;
  - a list of things (sketch entities, edges, parameters) puts one per line, each ending in a
    comma, so adding one never touches its neighbours;
  - a sheet's title block follows its sheet as a sub-table, `[sheet.title_block]`, laid out
    the same way;
  - anything else is written inline: `extent = { distance = 30.0 }`.
- **Values.**
  - Integers are written as integers.
  - Floats always have a point or an exponent (`30.0`, `1e-10`), in the shortest text that
    reads back as the same number.
  - `-0.0` is written `0.0`.
  - Strings are TOML basic strings, with `\"`, `\\`, `\n`, `\t` and `\uXXXX` escapes. Other
    characters are written as they are.
  - An empty optional value is left out, because TOML has no `null`.
- **Stable output.** Saving a document that did not change gives the same bytes on every OS.
  There are no timestamps, no "written by" field and no absolute paths. Computed values (solved
  sketch positions, solved component placements, reference fingerprints) are rounded to
  `tol::FILE_DECIMALS` (9) decimals of a millimetre. A recompute therefore never shows as a
  change. Values the user gave (dimensions, distances, equations) are written exactly.
- **Ids** are positive integers, never reused within their counter (`next_feature`,
  `next_entity`, ...), because references inside the file and from other files (drawings,
  assemblies) use them.
- **Merging.** Edits to different records or parameters merge cleanly. When two branches each
  add a feature, both new features take the same id. The merged file is then refused when
  opened ("two features have id 14: Fillet2 and Hole3"). Renumbering such a merge is not
  automatic yet.
- **Unknown top-level fields** of a part are kept when it is saved again; inside records they
  are not. Unknown fields of assemblies and drawings are not kept.
- **Limits.**
  - Files up to 64 MiB of text are read.
  - At most 10 000 features, and 20 000 entities and 20 000 constraints per sketch.
  - Assemblies: at most 5 000 components and 20 000 relationships.
  - Drawings: at most 500 sheets, 5 000 views and 50 000 annotations.
  - A file over a limit is refused with an error, never a crash.
- **Errors.**
  - Text that is not valid TOML is reported with its line and column.
  - A record that does not read gives the record's first line, which record it is and why:
    "line 41: feature 3 (Fillet1): invalid type: string "six", expected f64".
  - A file of the wrong kind is named ("this is an assembly (.tenonasm), not a part").
  - A file from a newer Tenon is refused: "made by a newer Tenon (format version 3; this build
    reads up to 2)".

Units everywhere are millimetres and radians, except where a field says otherwise: drawing
positions are millimetres of paper, and parameter equations are written in the units shown
(below).

## Parts (`.tenon`)

The start of `examples/m2-enclosure/enclosure.tenon`, with records left out:

```toml
# Tenon part file, format version 2. The format: docs/file-format.md in the Tenon sources.
format = "tenon"
version = 2
name = "Enclosure"
next_feature = 13

[parameters]
next = 18
user = [
  { name = "L", equation = "80 mm", unit = "mm", comment = "outside length" },
  { name = "H", equation = "30 mm", unit = "mm", comment = "outside height" },
]
model = [
  { name = "d0", target = { kind = "dimension", sketch = 1, constraint = 7 }, equation = "L" },
  { name = "d2", target = { kind = "feature", feature = 2, field = "/extent/distance" }, equation = "H" },
  { name = "d3", target = { kind = "feature", feature = 3, field = "/radius" } },
]

[[feature]]
id = 1
name = "Sketch1"
suppressed = false
type = "sketch"
plane = { origin = "XY" }
next_entity = 9
next_constraint = 8
entities = [
  { id = 1, type = "point", pos = { x = 0.0, y = 0.0 }, construction = true },
  { id = 3, type = "point", pos = { x = 80.0, y = 0.0 }, construction = false },
  { id = 6, type = "line", start = 2, end = 3, construction = false },
]
constraints = [
  { id = 1, type = "fix", point = 1 },
  { id = 7, type = "length", line = 6, value = 80.0 },
]

[[feature]]
id = 2
name = "Extrusion1"
suppressed = false
type = "extrude"
sketch = 1
regions = "default"
extent = { distance = 30.0 }
reverse = false
operation = "join"
```

### Top-level fields

| Field | Content |
|---|---|
| `format` | `"tenon"` |
| `version` | `2` |
| `name` | The part's name. |
| `next_feature` | The id the next feature gets. |
| `end_before` | Optional: the End of Part marker sits just before this feature, so it and the features after it are not computed. Absent: after the last feature. |

### Parameters (`[parameters]`)

- **`next`:** the number the next automatic name (`d18`) gets.
- **`user`:** user parameters.
  - `name`;
  - `equation`, in the units shown: `mm cm m in ft deg rad ul`;
  - `unit`: `mm`, `deg` or `ul` (unitless);
  - optional `comment`.
- **`model`:** the names given to every driving sketch dimension and numeric feature value
  (DEC-023).
  - `name`;
  - `target`: `{ kind = "dimension", sketch, constraint }`, or
    `{ kind = "feature", feature, field }` where `field` is a JSON pointer into the feature's
    own fields (`/extent/distance`);
  - an optional `equation` that sets it;
  - an optional `comment`.

The values themselves stay where they are used (in the sketch constraints and features). After
every edit, equations write them.

### Features (`[[feature]]`)

Every feature has `id`, `name`, `suppressed` and `type`, then the fields of its type. Features
may refer only to features before them.

| `type` | Fields |
|---|---|
| `sketch` | `plane`, `next_entity`, `next_constraint`, `entities`, `constraints` (below) |
| `extrude` | `sketch`, `regions`, `extent`, `reverse`, `operation` |
| `revolve` | `sketch`, `regions`, `axis` (revolve axis), `angle`, `operation` |
| `fillet` | `edges` (edge references), `radius` |
| `chamfer` | `edges`, `size` |
| `shell` | `remove` (face references), `thickness`, `outside` |
| `hole` | `sketch`, `points` (sketch point ids), `diameter`, `kind`, `extent`, optional `tip_angle`, `reverse` |
| `pattern_rect` | `features`, `dir1`, `count1`, `spacing1`, `reverse1`, optional `dir2`, `count2`, `spacing2`, `reverse2` |
| `pattern_circular` | `features`, `axis`, `count`, `angle`, `reverse` |
| `mirror` | `features`, `plane` |
| `work_plane` | `by`: `offset` (`base` plane, `distance`), `angle` (`base`, `axis`, `angle`) or `midplane` (`a`, `b`) |
| `work_axis` | `by`: `along` (`axis`) or `planes` (`a`, `b`) |
| `work_point` | `by`: `center` (`edge`) or `intersection` (`axis`, `plane`) |
| `rib` | `sketch`, `lines` (sketch line ids), `thickness`, `extent`, `flip` |
| `sweep` | `sketch`, `regions`, `path` (`{ sketch = <id>, curves = [<line and arc ids>] }`: the path's sketch and its curves, joined end to end), `fixed` (the profile keeps its direction instead of turning with the path), `operation` |
| `coil` | `sketch`, `regions`, `axis` (revolve axis), `pitch` (mm per turn), `turns`, `left` (left-handed), `operation` |
| `loft` | `sections` (sketch ids, in order), `ruled` (flat sides between sections), `operation` |

Field values:

- **A plane:** `{ origin = "XY" | "YZ" | "XZ" }`, `{ face = <face reference> }` (a planar
  face), or `{ work = <work plane id> }`.
- **A revolve axis:** `{ sketch_line = <entity id> }`, `{ origin = "X" | "Y" | "Z" }` or
  `{ work = <id> }`.
- **An axis** (patterns, work features): `{ origin = "X" }`, `{ edge = <edge reference> }`,
  `{ face = <face reference> }` (a cylinder's axis) or `{ work = <id> }`.
- **A direction:** `{ origin = "X" }`, `{ edge = <edge reference> }` or `{ work = <id> }`.
- **`regions`:**
  - `"default"`: every region at even nesting depth; or
  - `{ keys = [[curve ids], ...] }`: each key is the sorted ids of a region's outer boundary.
- **Extrusion `extent`:**
  - `{ distance = d }`
  - `{ symmetric = d }`
  - `{ two_sided = { forward = a, backward = b } }`
  - `"through_all"`
- **`operation`:** `"join"`, `"cut"`, `"new_body"` or `"intersect"`.
- **Revolve `angle`:** `"full"`, `{ angle = a }` or `{ symmetric = a }`.
- **Chamfer `size`:**
  - `{ equal = d }`
  - `{ two_distances = { d1, d2, reference = <face reference> } }`
  - `{ distance_angle = { distance, angle, reference } }`
- **Hole `kind`:** `"simple"`, `{ counterbore = { diameter, depth } }` or
  `{ countersink = { diameter, angle } }`.
- **Hole `extent`:** `{ distance = d }` or `"through_all"`.
- **Rib `extent`:** `"to_next"` or `{ distance = d }`.

### Sketches

A sketch feature's entities and constraints are lists of records, each with its `id`:

```toml
entities = [
  { id = 2, type = "point", pos = { x = 0.0, y = 0.0 }, construction = false },
  { id = 6, type = "line", start = 2, end = 3, construction = false },
]
constraints = [
  { id = 7, type = "length", line = 6, value = 80.0 },
]
```

**Entity types:**

| `type` | Fields |
|---|---|
| `point` | `pos` (solved position) |
| `line` | `start`, `end` (point ids) |
| `circle` | `center` (point id), `radius` |
| `arc` | `center`, `start`, `end` (point ids; counter-clockwise; the radius is \|start − center\|) |
| `spline` | `poles` (point ids), `degree` (a clamped uniform B-spline) |

Every entity also has `construction`.

**Constraint types:**

- **Geometric:**
  - `coincident` (`a`, `b`)
  - `point_on_curve` (`point`, `curve`)
  - `horizontal`, `vertical` (`line`)
  - `parallel`, `perpendicular`, `collinear`, `tangent`, `concentric`, `equal` (`a`, `b`)
  - `symmetric` (`a`, `b`, `axis`)
  - `midpoint` (`point`, `line`)
  - `fix` (`point`)
- **Dimensional, each with a `value`:**
  - `distance`, `horizontal_distance`, `vertical_distance`, `angle` (`a`, `b`)
  - `length` (`line`)
  - `radius`, `diameter` (`curve`)

Point positions are where the solver left them; they are rounded on writing (see
[Text files](#text-files)).

### References to faces and edges

Features store persistent references, never kernel indices (docs/persistent-naming.md):

```toml
remove = [
  { origin = { type = "cap", feature = 2, end = "end" }, fingerprint = { surface = "plane", direction = { x = 0.0, y = 0.0, z = 1.0 }, centroid = { x = 40.0, y = 30.0, z = 30.0 }, area = 4769.097335529 } },
]
edges = [
  { faces = [{ type = "side", feature = 2, curve = 6 }, { type = "side", feature = 2, curve = 7 }], fingerprint = { mid = { x = 80.0, y = 0.0, z = 15.0 }, length = 30.0 } },
]
```

- **A face's `origin`:**
  - `{ type = "cap", feature, end = "start" | "end" }`;
  - `{ type = "side", feature, curve }`: the face swept from a sketch curve;
  - `{ type = "from", feature, source, ordinal }`: a face made by a feature from a named
    sub-shape, such as a fillet face from an edge, or a patterned copy. `source` is the
    sub-shape's 64-bit key. TOML integers are signed, so a key above 2^63 − 1 is written as
    the same 64 bits read as signed: it shows as negative.
- **An edge** is the two faces it lies between.
- **The `fingerprint`** only breaks ties between pieces of a split face or edge. It is a computed
  value, so it is rounded and `tenon-cli diff` ignores it.

### Validation

After reading, a part is checked:
- ids are unique and within their counters;
- references point to earlier features;
- sketch references point to entities of the right type;
- numbers are finite and in range.

A file that fails is reported as damaged, naming what is wrong.

## Assemblies (`.tenonasm`)

An assembly is its own file (DEC-024):

```toml
# Tenon assembly file, format version 2. The format: docs/file-format.md in the Tenon sources.
format = "tenon-assembly"
version = 2
name = "Pivot"
next_component = 6
next_relationship = 6

[[component]]
id = 2
name = "arm:1"
part = "arm.tenon"
placement = { origin = { x = 28.0, y = 12.0, z = 10.0 }, x = { x = 0.0, y = 1.0, z = 0.0 }, y = { x = -1.0, y = 0.0, z = 0.0 }, z = { x = 0.0, y = 0.0, z = 1.0 } }
grounded = false
visible = true

[[relationship]]
id = 1
name = "Rotational:1"
suppressed = false
type = "joint"
joint = "revolute"
a = { component = 1, geom = { type = "face", face = { ... } } }
b = { component = 2, geom = { ... } }

[[explode]]
components = [2, 3]
direction = { x = 0.0, y = 0.0, z = 1.0 }
distance = 30.0
```

Implemented in `crates/io/src/asm.rs` and `crates/assembly/src/model.rs`; tested in
`crates/io/tests/asm.rs`.

- **`part`** is the part file's path relative to the assembly file's folder, with `/`
  separators. A part on another drive is stored as a full path. Opening resolves it against
  the folder the assembly is opened from, so the folder can move. A part that cannot be read
  is reported and its component shown as missing.
- **`placement`** maps part coordinates to assembly coordinates: a right-handed orthonormal
  frame (`origin`, `x`, `y`, `z`).
  - It is solved, so it is rounded on writing.
  - A frame that is not orthonormal (to 1e-6) or has coordinates beyond 1 km is refused.
- **`name`** is the part file's name and an occurrence number.
- **Relationship `type`:**
  - `mate`, `flush` (`offset`);
  - `angle` (`angle` in radians, `reference`: a unit vector in A's part coordinates);
  - `insert` (`offset`, `aligned`);
  - `joint`, with `joint` being `rigid`, `revolute`, `slider`, `cylindrical`, `planar` or
    `ball`.
- **A target** (`a`, `b`) is `{ component, geom }`. Without `component` it is the assembly's
  own origin geometry (only `plane`, `axis` and `origin`). `geom` is one of:
  - `{ type = "face", face = <face reference> }`;
  - `{ type = "edge", edge = <edge reference> }` (persistent references into the part, as in
    part files);
  - `{ type = "plane", plane = "XY" | "YZ" | "XZ" }`;
  - `{ type = "axis", axis = "X" | "Y" | "Z" }`;
  - `{ type = "origin" }`;
  - `{ type = "work", feature }`.
- **Explode steps** move `components` by `distance` along a unit `direction`.
- **Validation:** relationships must name existing components, and never join a component to
  itself.

## Drawings (`.tenondrw`)

A drawing is its own file (DEC-026). It holds the sheets, the views and the annotations, never
geometry: views are computed again from the model files each time the drawing is opened or
updated, so they always show the models as they are.

```toml
# Tenon drawing file, format version 2. The format: docs/file-format.md in the Tenon sources.
format = "tenon-drawing"
version = 2
name = "Plate"
standard = "ansi"
next_sheet = 3
next_view = 8
next_annotation = 10

[props]
title = "MOUNTING PLATE"
number = "TN-0004"
revision = "A"
company = ""
drawn_by = "TENON"
date = "2026-10-08"

[[sheet]]
id = 1
name = "Sheet:1"
size = { name = "B", width = 431.8, height = 279.4 }
border = true

[sheet.title_block]
name = "ANSI"
lines = [
  { a = { x = -180.0, y = 10.0 }, b = { x = -10.0, y = 10.0 } },
]
fields = [
  { key = "title", label = "TITLE", at = { x = -98.0, y = 40.0 }, height = 5.0 },
]
projection_symbol = { x = -32.5, y = 18.0 }

[[view]]
id = 1
sheet = 1
name = "VIEW1"
model = "plate.tenon"
kind = { type = "base", orientation = "front" }
scale = 1.0
center = { x = 100.0, y = 70.0 }
hidden = true
tangent = false
centerlines = true
label = false

[[annotation]]
id = 1
type = "dimension"
view = 2
dim = "horizontal"
a = { edge = { faces = [...], fingerprint = { ... } }, point = "whole" }
offset = { x = 0.0, y = 52.0 }
precision = 2
```

Implemented in `crates/io/src/drw.rs` and `crates/drawing/src/model.rs`; tested in
`crates/io/tests/drw.rs`.

- **Units.**
  - Sheet positions are millimetres of paper from the sheet's bottom-left corner.
  - Title block geometry is measured from the bottom-right corner (negative `x`).
  - Section lines and detail circles are in their parent view's own coordinates, in model
    millimetres.
- **`model`** is the part or assembly file's path relative to the drawing's folder, with `/`
  separators, as in assemblies. A model that cannot be read is reported, and its views are empty
  with the reason.
- **`standard`:** `ansi` (third-angle projection) or `iso` (first-angle) (DEC-027). It decides
  where projected views look from and which projection symbol the title block shows.
- **View `kind`** (inline, because a detail's `center` would clash with the view's own):
  - `base`, with `orientation` one of `front`, `back`, `top`, `bottom`, `left`, `right`, `iso`;
  - `projected`, with `parent` and `side` (`right`, `left`, `above`, `below`, or a corner such
    as `above_right` for an isometric view);
  - `section`, with `parent`, the cutting line `a`-`b`, and `flip` (seen towards the left of
    `a`-`b` instead of the right);
  - `detail`, with `parent`, `center` and `radius`.

  Parents come before their children, so there are no cycles. A view shows the same model as
  its parent.
- **A geometry pick** (`a`, `b`) is `{ edge = <edge reference>, point, component }`:
  - `edge` is a persistent edge reference into the part, as in part files;
  - `point` says which point of the edge is meant: `"whole"`, `"start"`, `"end"`, `"mid"` or
    `"center"`;
  - `component` is given for assembly views.

  Dimensions keep picks, never values: a value is measured again from the model whenever the
  drawing is shown.
- **Annotation `type`:**
  - `dimension`:
    - `dim`: `horizontal`, `vertical`, `aligned`, `diameter`, `radius` or `angle`;
    - `a`, and an optional `b`;
    - `offset` from the view's centre;
    - an optional `text`, where `<>` stands for the value;
    - `precision`.
  - `hole_table` and `parts_list`.
  - `balloon`: `attach` in the component's part coordinates, `offset` from the view's centre.
  - `note`.
  - Centre marks and lines placed by hand (DEC-029). They keep picks only and are drawn from
    the model each time:
    - `center_mark` (`a`: a circle or arc);
    - `centerline` (through the points of `a` and `b`);
    - `centerline_bisector` (midway between the lines `a` and `b`).
- **Validation:**
  - ids are positive, unique and below their counter;
  - views sit on existing sheets and refer to earlier views;
  - annotations refer to existing views or sheets;
  - scales are between 1e-4 and 1e4;
  - sizes, positions and text lengths are bounded.

## Drawing templates

A drawing template is an ordinary `.tenondrw` file. To start a drawing from one, use `drw.new`
with `template`, or File > New Drawing from Template.

- **Taken from the template:** its standard, properties and sheets (sizes, borders, title
  blocks), with the notes on them.
- **Left out:** views, and the dimensions, tables and balloons that need a model. No model
  file is read.

`drw.save_template` writes such a file from the open drawing. Tenon ships an ANSI B and an
ISO A3 template in `assets/templates`.

## Title block templates (`.json`, version 1)

A title block template is a small JSON file, separate from the document formats.
`drw.template.save` writes one; Manage > Apply Template or `drw.template.apply` uses one on a
drawing's sheets.

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
size. A field's `key` says what fills it:
- a drawing property: `title`, `number`, `revision`, `company`, `drawn_by`, `date`;
- a computed value: `scale`, `sheet`, `size`, `units`;
- `text`, for the label alone.

Each sheet keeps its own copy, so a drawing does not depend on the template file.

## Comparing files

`tenon-cli diff A B` (and the `file.diff` command for scripts and MCP) says what changed from one
document to another of the same kind, at any format version:

```text
Parameters
  ~ H  30 mm -> 35 mm (outside height)
Features
  ~ Extrusion1 [2]  extent distance 30 -> 35 (from H)
  > Round edges [3] moved after Shell1 [4]
  - Hole2 [13]
```

- **The markers:** `+` added, `-` removed, `~` changed, `>` moved.
- **What is compared:**
  - parts: parameters and features;
  - assemblies: components (a changed placement shows as "moved"), relationships and explode
    steps;
  - drawings: the drawing's own fields, sheets, views and annotations.
- **"(from H)"** marks a value that follows from a changed parameter.
- **Ignored:** fingerprints. Numbers are compared to 9 decimals, so a version-1 file and its
  upgrade compare as the same.
- **Exit code:** 0 when the documents are the same, 1 when they differ, 2 when one cannot be
  read. `--json` gives the changes as data.

## Version 1 (read only)

Tenon wrote version 1 until 2026-10-09. A version-1 file is a zip (the file starts with
`PK\x03\x04`) holding `project.json`:

```json
{ "format": "tenon", "version": 1, "generator": "tenon 0.1.0", "document": { ... } }
```

It holds the same document in Tenon's in-memory shape, with these differences:
- the body sits under `document`, `assembly` or `drawing`;
- lists are plural (`features`, `components`, `sheets`);
- each feature, relationship and annotation keeps its type's fields under `kind`;
- `params` holds the parameters;
- sketch entities and constraints are `[id, value]` pairs, and an entity's geometry is under
  `geometry`;
- computed values are not rounded.

**Upgrading:**
- Opening a version-1 file reads it as version 2.
- Saving writes version 2. The first time, the version-1 original is kept once beside it as
  `name.v1.tenon` (or `.v1.tenonasm`, `.v1.tenondrw`), so an older Tenon can still open it. A
  copy already there is never overwritten.
- `tenon-cli upgrade FILE...` upgrades files without opening the app.

A version-1 zip holds `project.json` of at most 64 MiB uncompressed, in a file of at most
256 MiB. Every version-1 example and template is kept in `crates/io/tests/fixtures/v1`. Tests
check that each upgrades to the version-2 file checked in beside the original, means the same
document, and regenerates the same solids (`crates/io/tests/format_v2.rs`).

## Changing the format

- **A change to what a document holds** bumps `version` and adds a migration step in
  `project::read_head`, from the previous version. The fixture files of every older version stay
  in `crates/io/tests/fixtures` and are tested.
- **Until a version has shipped in a release,** new feature types and new optional fields join
  it without a bump (DEC-029, DEC-035): files without them are unchanged, and a build that does
  not know a type refuses the file naming the record. Version 2 has gained `sweep`, `coil` and
  `loft` this way.
- **The layout is part of the format.** Field order, record names and what is inline all
  change every file, so they are changed as a version.
- **The container** (one text file, TOML) is the owner's decision (DEC-031).
