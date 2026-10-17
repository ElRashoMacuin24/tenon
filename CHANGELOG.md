# Changelog

What changes from one release of Tenon to the next. Every item that works has the test that
shows it in [ROADMAP.md](ROADMAP.md).

## Unreleased

### Sketches

- **Text.** Sketch > Create > Text: click where it starts, type it in the Format Text box
  (several lines, the height of its capitals, an angle), OK. Its letters are profiles: a sketch
  of text alone extrudes or cuts at once, and in a feature's panel a click in a text's box adds
  its letters. Double-click a text to change it; drag it by its box. One font, DejaVu Sans,
  built in so that a part is the same everywhere. New commands `sketch.text` and
  `sketch.edit_text`; `sketch.info` lists the texts. Files gain the entity `text` and the
  region key `{ text = id }` (an addition to format version 2).
- **Center Point.** Sketch > Format > Center Point: points on their own are centre points
  (drawn as a cross, and a hole goes at them) or sketch points (a dot; a hole takes them only
  when picked). On the selected points, or with none selected, on what Point makes.
  `sketch.center_point`, and `center` of `sketch.point`; files mark `sketch_point = true`.

### Parts and assemblies

- **Delete Features.** Deleting a feature from the browser now says what goes with it: the
  sketches it is made from, and the features that use it, ticked as Inventor ticks them, in
  one undo step. `feature.delete` takes `consumed` and `dependents`.
- **Extrude To.** Extents > To takes an extrusion as far as a flat face, a work plane or an
  origin plane, on whichever side of the sketch it is; at an angle, the end is cut along the
  plane. `model.extrude` takes `to`; files gain `extent = { to = <plane> }`.
- **Hole: Spotface and To.** A spotface seat (the hole measured from its floor), and holes
  that go as far as a flat face or plane. `model.hole` takes `type: "spotface"` and `to`.
- **Hole on a face**: with no sketch of centres, click a flat face where each hole goes; OK
  makes the sketch and the hole together.
- **Tapped holes**: Hole > Tapped drills a screw's tap drill and puts the thread on each wall,
  full depth or a depth; drawings note it ("M8x1.25"). `model.hole` takes `tapped`.
- **Extrude Between**: from one flat face or plane to another, wherever the sketch is.
  `model.extrude` takes `from` and `to`.
- **Revolve To** a face or plane through the axis, and **To Next**. `model.revolve` takes `to`
  and `to_next`.
- **Work plane through three points**: work points, sketch points, circles' centres or the
  origin. `work.plane` takes `by: "three_points"` and `points`.
- **Symmetry** joins Constrain: two faces, axes or points mirrored in a plane picked third.
  `asm.constrain` takes `type: "symmetry"` and `plane`.
- **Tangent** joins Constrain: a cylinder or sphere against a plane or another, outside or
  inside. `asm.constrain` takes `type: "tangent"` and `inside`.
- **Extrude To Next**: joined, as far as the part's next face, sloping or curved; cut, through
  the material it starts in. `model.extrude` takes `to_next`.
- **Revolve** turns Default, Flipped, Symmetric or Asymmetric (two angles), as Inventor's
  does. `model.revolve` takes `backward` and `reverse`.
- **Place** works as Inventor's: after the first part (grounded at the origin), each instance
  follows the pointer and a click puts it down, until Esc or a right-click.
- **Visibility.** A sketch's or work feature's menu in the browser shows or hides it, as
  Inventor's does; saved with the part. `feature.visibility`.
- **Constrain** offers Mate, Angle and Insert, with Flush as Mate's second solution, as
  Inventor has it; offsets and angles are typed.
- [docs/inventor-fidelity.md](docs/inventor-fidelity.md) compares every environment with
  Inventor's, command by command.

### Fixed

- Ctrl+Z with a feature's panel open undid the change before it, unseen. It now leaves the
  panel, as Cancel does.
- A value a field refused was still taken for OK with the value from before. OK is now refused
  and the status bar says what the field takes.
- The face a sketch was started on stayed selected, and the next command took it as its own.
- In the light theme, sketch dimensions and fully constrained curves were near-white on white.
- A feature being set up was named by its internal id in messages; Extrude started on the
  newest sketch even when it held no profile.
- The status bar kept a panel's instruction after OK; "nothing to undo" was shown as an error;
  start hints named a button that does not exist; Application Options opened over the ribbon.
- A shell thicker than a chamfer of the part fails in the kernel: the message now says to make
  the shell before the chamfer.

## 0.1.0 (2026-10-13)

The first release: parametric parts, sheet metal, assemblies, drawings, 3D printing checks and
quick stress checks, in one program that runs on Windows, macOS (Apple silicon) and Linux
without installing anything else.

### Parts

- Sketches on planes, faces and work planes: lines, arcs, circles, rectangles, slots,
  polygons, splines and points; trim, extend, split, offset, fillet, move, copy, rotate,
  scale, mirror and patterns; constraints found as you draw, dimensions with formulas, the
  degrees of freedom that are left; edges of the part projected into the sketch.
- Extrude, revolve, sweep, loft, coil and rib; holes, threads, fillets, chamfers, shells,
  drafts; split and combine solids; rectangular and circular patterns and mirrors; work
  planes, axes and points; box, cylinder, sphere and torus.
- Every feature changed later by double-clicking it, with what came after rebuilt and the
  faces and edges it used found again; End of Part, reordering, suppressing.
- Parameters and formulas; design tables (one part in several sizes); materials with their
  density, colour and strength; mass, volume, centre of gravity; measuring.
- Files: `.tenon` (plain text, kept in Git and compared with `tenon-cli diff`); STEP, STL and
  3MF out; STEP in (`tenon-cli info`, `convert`).

### Sheet metal

- Faces, flanges (mitred where they meet), contour flanges (also along edges), hems, folds,
  bends between faces and on sharp edges, corner seams and rips, cuts across bends and
  square to the sheet, corner rounds and chamfers; bend and corner reliefs; the sheet's own
  rule (thickness, radius, K factor).
- The flat pattern, with its bend lines, written as DXF for a cutter.

### Assemblies

- Components placed from part files, grounded, held by mate, flush, angle and insert
  constraints or rigid, rotational, slider, cylindrical, planar and ball joints; dragged
  with their relationships kept; degrees of freedom shown.
- Standard parts (screws, nuts, washers, pins, bearings, sections) in their sizes; parts
  edited in place; interference; the bill of materials (CSV); exploded views.

### Drawings

- Base, projected, isometric, section and detail views of parts and assemblies, with hidden
  lines removed; dimensions that follow the model; centrelines, hole tables, parts lists,
  balloons and text; ANSI and ISO sheets and title block templates; PDF, SVG and DXF.

### 3D printing

- The 3D Print Check: closed surfaces, thin walls, overhangs and whether the part fits the
  bed.

### Stress checks

- Studies set up on the part: fixed supports (on faces or edges), pins, sliding supports;
  forces (on faces or along edges), pressures, the part's weight, moments, bearing loads,
  remote forces and the part turning. Worked out by Gmsh and CalculiX, two free programs
  installed apart from Tenon; von Mises stress, displacement and safety factor in colour on
  the part. Checked against textbook answers on every change to Tenon.

### For scripts and agents

- Every change is a named command with a published schema; command scripts
  (`tenon-cli run`); an MCP server for AI agents, on its own or driving the running app.

### Around it

- The ribbon, browser, properties panel, orientation cube and radial menu of the familiar
  mechanical-CAD workflow; light and dark themes; search for any command.
- Unsaved work is kept against a crash and offered back; a fault in Tenon itself is written
  to a report that the next start points to.
- Application Options are kept between sessions, including where the stress programs are.
- Release archives for Windows, macOS and Linux that hold every library Tenon needs and the
  licence of each.
- A part's number and description (Inspect > Properties), in the bill of materials, its CSV
  and drawings' parts lists.
- Holes sized for M3 to M20 screws: clearance (close or normal fit) or tap drill, with a
  counterbore or countersink for the head. Polygons of 3 to 64 sides. Moments of inertia in
  the Properties window. Solids picked in the browser. A thread's size in its browser row.
  Coils by turns and height, or pitch and height. The 3D Print Check draws the printer's build
  space. A stress result's mark says when the peak is behind the part. A fillet too large
  says the largest radius that fits.
