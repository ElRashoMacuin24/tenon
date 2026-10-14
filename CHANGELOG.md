# Changelog

What changes from one release of Tenon to the next. Every item that works has the test that
shows it in [ROADMAP.md](ROADMAP.md).

## 0.1.0 (not yet released)

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
  space. A stress result's mark says when the peak is behind the part.
