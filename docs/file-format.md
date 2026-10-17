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
| `part_number` | Optional: the part's number, for parts lists (one line, at most 200 characters). Absent: its file's name stands in. |
| `description` | Optional: what the part is, for parts lists (the same). Absent: its name stands in. |
| `next_feature` | The id the next feature gets. |
| `end_before` | Optional: the End of Part marker sits just before this feature, so it and the features after it are not computed. Absent: after the last feature. |
| `appearance` | Optional: the part's own colour, `"#rrggbb"`, shown in place of its material's. |

### Material (`[material]`) and appearance

Optional. A part without `[material]` weighs 1 g/cm^3 and is shown in the standard colour.

```toml
appearance = "#d04030"

[material]
name = "Brass"
density = 8.5
color = "#c9a64a"
strength = { youngs_modulus = 110.0, poisson = 0.34, yield_strength = 125.0 }
```

- **`name`:** 1 to 64 characters. It need not be a material of Tenon's library: the file
  carries the density and the colour itself, so it reads the same wherever it is opened.
- **`density`:** grams per cubic centimetre, more than 0 and at most 30.
- **`color`:** `"#rrggbb"`, lower case.
- **`strength`:** optional: how the material answers to a load, for stress studies.
  `youngs_modulus` in GPa (more than 0, at most 2000), `poisson` (from 0 to less than 0.5)
  and `yield_strength` in MPa (the stress at which it stays bent, or for a brittle material
  breaks). A material without it that has the name of a library material is taken to have
  the library's values; any other cannot be checked for stress.

The part is shown in `appearance` when it has one, else in its material's `color`.

### Design table (`[table]`)

Optional: the sizes one part comes in (DEC-044).

```toml
[table]
active = "Small"
columns = ["d0", "d1"]
rows = [
  { name = "Small", values = [40.0, 10.0] },
  { name = "Large", values = [60.0, 25.5] },
]
```

- **`columns`:** the parameters the table sets, by name (1 to 100). Each is a model parameter
  without an equation or a user parameter whose equation is a plain value.
- **`rows`:** 1 to 1000, each with a `name` of its own and one number per column, as the
  parameters show them: millimetres, degrees or plain numbers.
- **`active`:** the row the part is at. The file's parameters and features hold that row's
  values, so a reader that ignores the table still gets the right part. Should a file edited by
  hand disagree with itself, the part's own values stand and the active row takes them.

### Sheet metal (`[sheet_metal]`)

A sheet metal part (DEC-057) has this table; a part that is not one has none.

```toml
[sheet_metal]
relief = "straight"
corner = "round"
flat = true
```

| Field | |
|---|---|
| `relief` | the cut beside a bend that is narrower than its edge: `"straight"` or `"round"` |
| `corner` | the cut where two bends meet at a corner, centred where the centre lines of the two bends cross in the flat (DEC-063): `"round"`, `"square"` or `"trim"` (none) |
| `a_side` | optional: a [face reference](#references-to-faces-and-edges), the side that is up when the flat pattern lies on the table. Without it, the largest flat face |
| `flat` | optional, written when true: the part has a flat pattern (it is shown in the browser). The pattern itself is not in the file: it is worked out from the solid |

The rule's sizes are user parameters with fixed names, each with its equation like any other:
`Thickness`, `BendRadius`, `BendReliefWidth`, `BendReliefDepth`, `MinimumRemnant`,
`CornerReliefSize`, `GapSize` (millimetres) and `KFactor` (a plain number from 0 to 1). A sheet
metal part without one of them, or with one out of range, opens, and its sheet metal features
say what is missing when the part is rebuilt; `GapSize` alone may be missing (parts saved
before there was one), and is then one `Thickness`.

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
may refer only to features before them. A sketch or work feature the user showed or hid from
the browser has `visible = true` or `false` after `suppressed` (DEC-106); without it, a sketch
shows until a feature uses it and a work feature shows.

| `type` | Fields |
|---|---|
| `sketch` | `plane`, `next_entity`, `next_constraint`, `entities`, `constraints`; optional `projections` (all below) |
| `extrude` | `sketch`, `regions`, `extent`, `reverse`, `operation`; optional `taper` (radians the sides lean in as they leave the sketch, negative to lean out; one distance or through-all only) |
| `revolve` | `sketch`, `regions`, `axis` (revolve axis), `angle`, `operation` |
| `fillet` | `edges` (edge references), `radius`; optional `radii` (a variable radius: `[{ at, radius }, ...]`, `at` from 0 at each edge's start to 1 at its end, `radius` being the start's) and `faces` (a face fillet: two lists of face references, `edges` then empty; DEC-122) |
| `chamfer` | `edges`, `size` |
| `shell` | `remove` (face references), `thickness`, `outside` |
| `hole` | `sketch`, `points` (sketch point ids), `diameter`, `kind`, `extent`, optional `tip_angle`, `reverse` |
| `pattern_rect` | `features`, `dir1`, `count1`, `spacing1`, `reverse1`, optional `dir2`, `count2`, `spacing2`, `reverse2` |
| `pattern_circular` | `features`, `axis`, `count`, `angle`, `reverse` |
| `mirror` | `features`, `plane` |
| `work_plane` | `by`: `offset` (`base` plane, `distance`), `angle` (`base`, `axis`, `angle`), `midplane` (`a`, `b`) or `three_points` (`points`: three of `"origin"`, `{ work = <point id> }`, `{ center = <edge reference> }` or `{ sketch_point = { sketch, point } }`; DEC-116) |
| `work_axis` | `by`: `along` (`axis`) or `planes` (`a`, `b`) |
| `work_point` | `by`: `center` (`edge`) or `intersection` (`axis`, `plane`) |
| `rib` | `sketch`, `lines` (sketch line ids), `thickness`, `extent`, `flip` |
| `sweep` | `sketch`, `regions`, `path` (`{ sketch = <id>, curves = [<line and arc ids>] }`: the path's sketch and its curves, joined end to end), `fixed` (the profile keeps its direction instead of turning with the path), `operation` |
| `coil` | `sketch`, `regions`, `axis` (revolve axis), `pitch` (mm per turn), `turns`, `left` (left-handed), `operation` |
| `loft` | `sections` (sketch ids, in order), `ruled` (flat sides between sections), `operation` |
| `draft` | `faces` (face references, on one body), `plane` (the neutral plane: faces stay put where it crosses them, and the part is pulled along its normal), `angle` (radians), `reverse` |
| `split` | `plane` (the cutting plane), `keep`: `"both"` (two bodies), `"front"` (the side the plane's normal points to) or `"back"`; optional `body` (a face reference on the one body to split; without it, every body the plane passes through) |
| `combine` | `base` (a face reference on the body that stays), `tools` (a face reference on each other body), `operation` (`"join"`, `"cut"` or `"intersect"`), `keep_tools` |
| `thread` | `face` (a face reference on a round shaft or hole); optional `pitch` (mm; without it, the ISO coarse pitch for the face's diameter); optional `designation` (text for drawings; without it, `M<diameter>x<pitch>`); `length`: `"full"` or `{ distance = mm }` from the end it starts at; `reverse` (start from the other end); `left` (left-handed); `modelled` (the groove is cut into the part; otherwise the thread is cosmetic) |
| `sheet_face` | `sketch`, `regions`, `flip` (the thickness goes behind the sketch plane), `operation` (`"join"` or `"new_body"`): a profile made one `Thickness` thick |
| `flange` | `edges` (edge references: straight edges of the sheet's rim), `angle` (radians the sheet turns, 1 to 179 degrees), `height`, `datum` (`"outer"`, `"inner"` or `"tangent"`: what the height is measured from), `position` (`"inside"`, `"adjacent"`, `"outside"` or `"tangent"`: where the bend lies relative to the edge), `flip` (bend to the other side); optional `radius` (the bend's inside radius; without it the part's `BendRadius`); `width` (`"edge"`, `{ centered = { width = w } }` or `{ offset = { start = a, end = b } }`); optional `miter`, written when false: where the flange meets another of the same feature at a corner, each wall ends where its bend does instead of being brought up to the part's `GapSize` from the other; and what its corners have of their own in place of the rule's, each optional: `seam` (`"symmetric"`, the usual, `"overlap"` or `"reverse_overlap"`: how the two walls are brought together, the first being the one whose edge comes first), `overlap` (0 to 1, the part of the other wall's thickness covered; 1 when not written), `gap`, `relief` (`"round"`, `"square"` or `"trim"`) and `relief_size` |
| `hem` | `edges` (as a flange's), `shape` (`{ single = { gap = g, length = l } }`, `{ double = { gap = g, length = l } }`: the gap between the layers and the length over all; `{ rolled = { radius = r, angle = a } }`, `{ teardrop = { radius = r, angle = a } }`: the inside radius and the angle in radians, 1 to 350 degrees rolled and 181 to 270 for a teardrop), `flip` (fold to the other side), `width` (as a flange's) |
| `contour_flange` | `sketch` (an open profile: its lines and arcs end to end), `distance` (how far it is pushed out of the sketch plane), `direction` and `position` (each `"default"`, `"flipped"` or `"symmetric"`: which way it is pushed, and which side of the profile the sheet's thickness goes), optional `radius` (inside, of the bends made at corners; without it the part's `BendRadius`), `operation` (`"join"` or `"new_body"`). Or, along edges of the sheet (DEC-072): optional `edges` (edge references: straight edges of the sheet's rim; the sketch is square to the first and the profile starts at it) and `width` (as a flange's: how much of each edge), with no `distance` written: the side of the sheet the edge is on runs on into the profile |
| `corner_seam` | `edges` (two edge references: the edge at the end of each of two faces bent up from one sheet, where they come together at a square corner; the first face's, then the second's); optional `seam`, `overlap`, `gap`, `relief` and `relief_size`, as a flange's. The bends are ended where their bend lines cross, the faces brought to the seam, and the corner relieved (DEC-066) |
| `corner_rip` | `edges` (edge references: each the outside or the inside edge of a square corner where two walls of the sheet meet with no seam between them); optional `seam`, `overlap` and `gap`, as a flange's (no relief). The corner is opened along each edge, as far as the edge goes: both walls cut back the gap apart, or one kept and the other cut the gap short of it; the first wall is the one whose face the edge reference names first (DEC-074) |
| `cut_across_bend` | `sketch` (on a side of the sheet: a face of it, or a plane that lies on one), `regions` (its profile: lines, arcs and circles, drawn as on the sheet laid flat from that face), optional `distance` (how deep from the side the sketch is on; without it, or at the thickness or more, through). The profile is cut out of every face and bend it lies on, so the cut goes round the bends and is the profile in the flat pattern (DEC-068) |
| `cut_normal` | `sketch` (on any plane that looks at the sheet), `regions` (its profile: lines, arcs and circles), optional `distance` (how deep from the side of the sheet that looks at the sketch; without it, or at the thickness or more, through). The profile is cut out of every flat face and bend of that side it is seen on, looking along the sketch's normal, each cut square to itself (DEC-073) |
| `bend` | `edges` (two edge references: a straight edge of the rim of each of two faces of the sheet that do not touch, running alongside one another); optional `radius` (the bend's inside radius; without it the part's `BendRadius`), `double` (how faces in parallel planes are joined: `"fix_edges"` when it is not written, `"45_degree"`, `"full_radius"` or `"90_degree"`) and `flip_fixed` (the second edge stays and the first face is trimmed or extended). The faces are trimmed or extended to the bend and joined by it (DEC-070) |
| `sharp_bend` | `edges` (edge references: each the outside or the inside edge of a sharp corner where two faces of the sheet meet with no bend between them); optional `radius` (the bends' inside radius; without it the part's `BendRadius`). Each corner is cleared back to where a bend begins on either face, for the length of its edge, and the bend put in, tangent to both, as far along the edge as both faces still reach it: edges that meet at a corner of the sheet, given together, have their bends end where their bend lines cross (DEC-075) |
| `fold` | `sketch` (on a side of the sheet, with a line from edge to edge of a face), optional `line` (the entity to fold along; without it, the sketch's one line), `angle` (radians the sheet turns, 1 to 179 degrees), optional `radius` (the bend's inside radius; without it the part's `BendRadius`), `location` (`"centerline"` when it is not written, `"start"` or `"end"`: where the line is in the bend's zone), `flip_side` (the sheet on the other side of the line folds) and `flip_direction` (it folds away from the side the sketch is on). The sheet beyond the line turns about a bend in the zone; nothing is added (DEC-071) |

Field values:

- **A plane:** `{ origin = "XY" | "YZ" | "XZ" }`, `{ face = <face reference> }` (a planar
  face), or `{ work = <work plane id> }`.
- **A revolve axis:** `{ sketch_line = <entity id> }`, `{ origin = "X" | "Y" | "Z" }` or
  `{ work = <id> }`.
- **An axis** (patterns, work features): `{ origin = "X" }`, `{ edge = <edge reference> }`,
  `{ face = <face reference> }` (a cylinder's axis) or `{ work = <id> }`.
- **A direction:** `{ origin = "X" }`, `{ edge = <edge reference> }` or `{ work = <id> }`.
- **`regions`:** the closed regions of the sketch the feature is made from. Lines, arcs and
  circles that cross or touch divide one another, so a line through a circle gives two regions.
  Regions side by side are used as one: the curve between them leaves no face.
  - `"default"`: every region at even nesting depth (a plate with its holes, but not the discs
    in the holes); or
  - `{ keys = [<key>, ...] }`, each key one of:
    - `[<curve ids>]`: what those curves enclose on their own, whole, however other curves
      divide it. This is the only form written for a region no other curve divides, and the
      only form before DEC-046;
    - `{ left = [<curve ids>], right = [<curve ids>], nth = <n> }`: one region of a divided
      sketch, by the side of each curve of its outer boundary it lies on. A curve runs from its
      start to its end; a circle or arc runs counter-clockwise, so its left is its inside.
      `nth` (left out when 0) tells apart regions with the same curves on the same sides,
      counted along the lowest-numbered curve. An empty list is left out.
    - `{ text = <text id> }`: the letters of a text, each with its holes (DEC-102). A
      sketch with no closed region of curves takes its texts by default.
- **Extrusion `extent`:**
  - `{ distance = d }`
  - `{ symmetric = d }`
  - `{ two_sided = { forward = a, backward = b } }`
  - `"through_all"`
  - `"to_next"`: as far as the part's next faces (DEC-109)
  - `{ between = { from = <plane>, to = <plane> } }`: from one plane to the other (DEC-111)
  - `{ to = <plane> }`: as far as a plane, on whichever side of the sketch it is (DEC-104); the
    plane written as a sketch's is: `{ origin = "XY" }`, `{ face = <face reference> }` or
    `{ work = <work plane id> }`
- **`operation`:** `"join"`, `"cut"`, `"new_body"` or `"intersect"`.
- **Revolve `angle`:** `"full"`, `{ angle = a }`, `{ symmetric = a }`,
  `{ two_sided = { forward = a, backward = b } }` (DEC-108), `{ to = <plane> }` or `"to_next"`
  (DEC-112); `reverse = true` after it turns it
  the other way round the axis (written only when true).
- **Chamfer `size`:**
  - `{ equal = d }`
  - `{ two_distances = { d1, d2, reference = <face reference> } }`
  - `{ distance_angle = { distance, angle, reference } }`
- **Hole `kind`:** `"simple"`, `{ counterbore = { diameter, depth } }`,
  `{ countersink = { diameter, angle } }` or `{ spotface = { diameter, depth } }` (DEC-105: the
  hole's distance is measured from the spotface's floor; a depth of 0 is allowed).
- **Hole `extent`:** `{ distance = d }`, `"through_all"` or `{ to = <plane> }` (DEC-105: each
  hole to where its axis meets the plane; the plane written as a sketch's is).
- **Hole `tapped`** (DEC-114, written only for a tapped hole): `{ length = "full" | { distance = d } }`,
  with `pitch`, `designation` and `left = true` when they are given.
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
| `ellipse` | `center`, `major`, `minor` (point ids: its centre and the ends of its two axes, which the solver keeps square; either axis may be the longer; DEC-120) |
| `spline` | `poles` (point ids), `degree` (a clamped uniform B-spline) |
| `text` | `at` (point id: where the first line's baseline starts), `text` (the words; a new line starts a line below), `height` (of capital letters, mm), `angle` (radians counter-clockwise, left out when 0). The letters are DejaVu Sans 2.37's, worked out when the file is read, never written (DEC-102) |

Every entity also has `construction`, and optional marks written only when true:

- `centerline = true` on a line: a centre line (DEC-056). It bounds profiles like any line,
  is the axis a revolution takes when none is named, and is what a `linear_diameter` is
  measured to.
- `reference = true`: reference geometry, shown in the sketch from an edge of the part
  (DEC-055). The solver never moves it; the sketch's `projections` (below) say which edge each
  piece shows. A point, line, circle or arc can be reference geometry; a spline cannot.
- `sketch_point = true` on a point on its own: a sketch point, which a hole takes only when
  it is picked; points on their own are centre points otherwise (DEC-117).

**Constraint types:**

- **Geometric:**
  - `coincident` (`a`, `b`)
  - `point_on_curve` (`point`, `curve`)
  - `horizontal`, `vertical` (`line`)
  - `parallel`, `perpendicular`, `collinear`, `tangent`, `concentric`, `equal` (`a`, `b`)
  - `smooth` (`a`, `b`: a spline and the line, arc or spline it joins end to end; DEC-121)
  - `symmetric` (`a`, `b`, `axis`)
  - `midpoint` (`point`, `line`)
  - `fix` (`point`)
- **Dimensional, each with a `value`:**
  - `distance`, `horizontal_distance`, `vertical_distance`, `angle` (`a`, `b`)
  - `length` (`line`)
  - `radius`, `diameter` (`curve`)
  - `linear_diameter` (`point`, `axis`): a diameter measured across a line. `axis` is a line
    (the app offers this dimension for a centre line); the `value` is twice the distance from
    the point to it.

  A dimension placed by hand also has `at = { x, y }`: where its value is shown in the sketch
  (its dimension line runs through there). Without it, the dimension is drawn beside its
  geometry. Moving a dimension changes only this field.

  A driven dimension has `driven = true` (DEC-047). It follows the sketch instead of setting
  it: the solver leaves it out, and its `value` is what the sketch measures, written like any
  computed number. It has a parameter name, which equations may read but nothing may set.

**Projected edges.** A sketch that shows edges of the part lists them, after its constraints:

```toml
projections = [
  { entity = 4, edge = { faces = [{ type = "cap", feature = 2, end = "end" }, { type = "side", feature = 2, curve = 6 }], fingerprint = { mid = { x = 30.0, y = 0.0, z = 12.0 }, length = 60.0 } } },
]
```

`entity` is a line, circle, arc or point of this sketch with `reference = true`, and `edge` an
[edge reference](#references-to-faces-and-edges) to an edge made by features before the sketch.
Each entity is listed once at most. At every regeneration the edge is found again and the
entity, with its points, is put where the edge then is on the sketch's plane; the positions in
the file are those of the last regeneration. An edge that cannot be found leaves its entity
where it is. A sketch with no projected edges has no `projections` field.

A projection with `flat = true` (Project Flat Pattern, DEC-068) is of a sheet metal part: its
entity is put where the edge lies when the sheet is laid flat from the face the sketch is on,
a bend's allowance and the faces beyond it unfolded into the sketch's plane, not where the
edge is seen square on.

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

### Stress studies (`[[study]]`)

Optional, after the features: where the part is held and what pushes on it (DEC-093). What a
study comes to is worked out when it is asked for and is not kept in the file.

```toml
[[study]]
id = 1
name = "End load"
supports = [
  { id = 1, name = "Fixed1", kind = "fixed", faces = [{ origin = { type = "side", feature = 2, curve = 9 }, fingerprint = { ... } }] },
]
loads = [
  { id = 2, name = "Force1", faces = [{ origin = { type = "side", feature = 2, curve = 7 }, fingerprint = { ... } }], type = "force", newtons = 150.0, direction = { x = 0.0, y = 0.0, z = -1.0 } },
  { id = 3, name = "Snow", faces = [{ origin = { type = "cap", feature = 2, end = "end" }, fingerprint = { ... } }], type = "pressure", megapascals = 0.25 },
  { id = 6, name = "Gravity1", type = "gravity", direction = { x = 0.0, y = 0.0, z = -1.0 }, acceleration = 9810.0 },
]
mesh_size = 2.5
```

Other supports, as they are written in `supports`:

```toml
  { id = 4, name = "Pin1", kind = "pin", radial = true, axial = true, tangential = false, faces = [...] },
  { id = 5, name = "Frictionless1", kind = "frictionless", faces = [...], suppressed = true },
  { id = 7, name = "Fixed2", kind = "fixed", edges = [{ faces = [{ type = "side", feature = 2, curve = 9 }, { type = "cap", feature = 2, end = "start" }], fingerprint = { ... } }] },
```

Other loads, as they are written in `loads`:

```toml
  { id = 8, name = "Force2", edges = [...], type = "force", newtons = 60.0, direction = { x = 0.0, y = 1.0, z = 0.0 } },
  { id = 9, name = "Moment1", faces = [...], type = "moment", newton_millimetres = 2000.0 },
  { id = 10, name = "Moment2", faces = [...], type = "moment", newton_millimetres = 2000.0, axis = { x = 0.0, y = 0.0, z = 1.0 } },
  { id = 11, name = "Bearing1", faces = [...], type = "bearing", newtons = 500.0, direction = { x = 1.0, y = 0.0, z = 0.0 } },
  { id = 12, name = "RemoteForce1", faces = [...], type = "remote_force", newtons = 80.0, direction = { x = 0.0, y = 0.0, z = -1.0 }, point = { x = 130.0, y = 10.0, z = 5.0 } },
  { id = 13, name = "Body1", type = "body", axis = { x = 0.0, y = 0.0, z = 1.0 }, point = { x = 50.0, y = 10.0, z = 0.0 }, rpm = 3000.0 },
```

- **`id`, `name`:** the study's id within the part, and its name (1 to 64 characters).
- **`supports`:** faces that are held. `kind = "fixed"`: held still in every direction.
  `kind = "pin"`: round (cylindrical) faces on a pin, with what the pin holds: `radial`
  (across it), `axial` (along it), `tangential` (from turning), at least one of them true.
  `kind = "frictionless"`: flat or round faces that may slide along themselves but not leave
  or enter what they rest on.
- **`loads`:** `type = "force"` is a force of `newtons` in all, spread evenly over its faces,
  along `direction` (in the part's coordinates; its length does not matter) or, without one,
  square to each face and into it. `type = "pressure"` is `megapascals` (N/mm²) on its faces,
  square to them and into them; a negative value pulls. `type = "gravity"` is the part's own
  weight: all of it pulled along `direction` at `acceleration` (mm/s²; the Earth's is 9810).
  It has no `faces`, and a study has it once. `type = "moment"` is `newton_millimetres` in
  all on its faces, about `axis` (a direction; seen from its tip the moment turns
  anticlockwise) or, without one, about the faces' own axis: the way a flat face looks, the
  axis of a round one. `type = "bearing"` is the load of a shaft, `newtons` along
  `direction`, on round (cylindrical) faces. `type = "remote_force"` is `newtons` along
  `direction` acting at `point` (in the part's coordinates) and carried by its faces.
  `type = "body"` is the part turning at `rpm` (turns a minute) about `axis` through
  `point`: it has no `faces`, and a study has it once.
- **`faces`:** references to faces, as features have them (below). At least one, but for
  gravity and a body's turning, which have none, and for what is on `edges`.
- **`edges`:** optional, in place of `faces`, on a `fixed` support and a `force`: references
  to edges, as features have them. Written only when there are any. A force along edges is
  spread evenly along their length and has a `direction`. A support or a load has faces or
  edges, never both.
- **`suppressed`:** optional, on a support or a load: `true` leaves it out of the study for
  now, and keeps it. Written only when true.
- **`mesh_size`:** optional: the size of the elements the part is divided into, mm. Without it
  the size is chosen from the part's.
- Supports and loads of one study share one run of ids. A study with no supports, no loads
  and no size is its `id` and its `name`.

### Validation

After reading, a part is checked:
- ids are unique and within their counters;
- references point to earlier features;
- sketch references point to entities of the right type;
- numbers are finite and in range;
- a material has a name, a density in range and a colour, and if it says how it answers to a
  load, a stiffness, a Poisson's ratio and a yield strength in range; an appearance is a
  colour;
- a study has a name; its supports and loads have names, ids of their own and at least one
  face each, or edges where they may be on edges, and not both; every size is a number other
  than 0 within range, and every direction and axis is one; gravity and a body's turning
  are there at most once;
- a design table names parameters of the part, each once; its rows have names of their own and
  one number for each parameter; its active row is one of them.

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
- **`row`** (optional, DEC-048) names a row of the part's design table: the component is the
  part in that size, whatever row the part file itself is at. Components of one part file may
  name different rows. Nothing is written for a size: it is worked out from the part file each
  time. A row the part no longer has is reported and the component shown as missing.
  A standard part placed from the library (DEC-049) is this and nothing more: an ordinary part
  file, written by Tenon into a folder beside the assembly (`standard/hex-screw-iso-4017.tenon`),
  and a component that names it and a row. The folder travels with the assembly.
- **Relationship `type`:**
  - `mate`, `flush` (`offset`);
  - `angle` (`angle` in radians, `reference`: a unit vector in A's part coordinates);
  - `insert` (`offset`, `aligned`);
  - `tangent` (`offset`, `inside` written only when true; DEC-110);
  - `symmetry` (`plane`: a third target, the plane the two are mirrored in; DEC-115);
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
  - parts: the name, material (with its strength) and appearance, parameters, features, the
    design table (its active row, its parameters, and each row that was added, removed or
    changed), and stress studies (each support and load that was added, removed or changed);
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
  not know a type refuses the file naming the record. Version 2 has gained the M6 feature types
  this way (`sweep`, `coil`, `loft`, `draft`, `split`, `combine`, `thread`), the optional
  `taper` of an extrusion, the optional `at` and `driven` of a sketch dimension, region keys by
  side (`{ left, right, nth }`) beside the lists of curves they were, the optional `row` of an
  assembly component, a part's optional `appearance`, `[material]` and `[table]`, and for
  sketches the optional `centerline` and `reference` of an entity, the `linear_diameter`
  dimension and a sketch feature's optional `projections` (DEC-055, DEC-056), and for sheet
  metal the `[sheet_metal]` table and the feature types `sheet_face` and `flange` (DEC-057),
  then `hem`, `contour_flange`, `corner_seam`, a flange's `miter` and corner options and the
  parameter `GapSize` (DEC-063, DEC-064, DEC-066), then `cut_across_bend` and a projection's
  `flat` (DEC-068), then `bend` (DEC-070), then `fold`, a contour flange's `edges` and
  `width`, `cut_normal` and `corner_rip` (DEC-071 to DEC-074), then `sharp_bend` (DEC-075), then a
  material's optional `strength` and the `[[study]]` records (DEC-093); in them, the
  `pin` and `frictionless` supports, the `gravity` load and `suppressed` (DEC-097), then
  `edges` and the `moment`, `bearing`, `remote_force` and `body` loads (DEC-100), then a
  part's `part_number` and `description` (DEC-101).
- **The layout is part of the format.** Field order, record names and what is inline all
  change every file, so they are changed as a version.
- **The container** (one text file, TOML) is the owner's decision (DEC-031).
