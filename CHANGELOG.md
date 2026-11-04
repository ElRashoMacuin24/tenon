# Changelog

What changes from one release of Tenon to the next. Every item that works has the test that
shows it in [ROADMAP.md](https://github.com/ElRashoMacuin24/tenon/blob/main/ROADMAP.md).

## Unreleased

**New:** Optimize Shape in Stress Analysis works out where material is needed to carry a study's
loads, for a share of the mass kept, as a shape to model over (it needs Gmsh and CalculiX, as
Simulate does). Sketches gain text, ellipses, arc slots, Stretch and Smooth (G2). Parts gain Extrude
To, To Next and Between, Revolve To and To Next, spotface and tapped holes, holes placed by clicking
a face, and variable and face fillets. Assemblies gain Tangent, Symmetry and placing by the pointer.

**Everything on the ribbon works now,** with nothing greyed: 3D sketches; Emboss, Derive and user
coordinate systems; Contour Roll, Lofted Flange, Punch Tool, Rip, Unfold and Refold in sheet metal;
Pattern, Mirror and Copy of components; and in drawings Auxiliary, Overlay, Crop, Break and Break Out
views and Baseline and Ordinate dimensions.

### Coming from 0.1.0

- **Your files open, and what you made comes out as it did,** apart from what is listed here. One that
  uses nothing new is saved byte for byte, so it goes on opening in 0.1.0.
- **A sweep round a sharp corner** used to stop at the first corner, saying nothing, and made a third of
  the tube. It is now whole, mitred between two straight lines; at a curve, with a profile with holes,
  or keeping its direction, it is refused in words. So a part with one changes.
- **Refused now, where it opened:** a spline that crosses its own profile, which made a solid of negative
  volume; a name, or what fills a title block, over 1000 bytes; an equation or comment over 64,000.
- **A flange on a sheet a Mirror feature made** came to nothing, with no word. It is there now.
- **Parts list CSV:** its Material column says Generic for a part with none, where it was empty.
- **STEP of an assembly, or of a part of several solids,** is now one assembly in the file, a product for each
  solid, where each was a product on its own: another program shows one assembly with its components. The solids
  and where they stand are the same.
- **Solve again** any study with a bearing load on a wall not symmetric about the load: results move
  by tens of percent (below). A part in pieces with a piece not held fast is now refused, where it
  gave a meaningless answer.
- **Hole:** Tap Drill is now Tapped, which puts the thread on the wall. A hole from 0.1.0 opens as it
  is: Simple, whatever its size, until you choose Tapped; its own Thread feature stays.
- **Constrain:** Flush is now Mate's second solution.
- **Flat faces made by a Mirror feature, a chamfer or a part-turn Revolve** now have their normal out of
  the material, as every other face has: a sketch made on one from now on faces outward, an extrude from
  it goes outward, and a mate or a work plane made on one is on the side it should be. What 0.1.0 made
  on such a face stays as it was made, for the file says which way the face looked: sketches, work
  planes, splits, mates and moments alike. What only asks which way a face looks hears the new answer:
  `facing` of `model.find` and `model.faces` in a script, and Measure's angle.
- **Going back to 0.1.0.** It refuses a file that uses what is new, in the words "the project is
  damaged", though it is not: text, ellipses or Smooth; an extrusion To, To Next or Between; a
  revolution To, To Next, by two angles or Flipped; a spotface or To hole; a variable or face fillet;
  a work plane through three points; a Tangent or Symmetry relationship; a mirrored occurrence; an
  auxiliary or overlay view; a baseline or ordinate set; an emboss, a derived part or a UCS; a 3D sketch
  or what is made from one; a rip, an unfold, a refold, a contour roll, a lofted flange or a punch. Other
  new things it opens, and on a save loses: a tapped hole's thread, which points are sketch points, which
  sketches are shown, a study's Optimize Shape settings, an assembly's patterns (each element then a free
  component where it stands) and a drawing view's crops, breaks and break outs. And one it opens with
  no word and gets wrong: a sketch, work plane or mate made here on a flat face from a Mirror feature, a
  chamfer or a part-turn Revolve lands turned over there, since it still reads such a face as looking
  into the part.

### Stress checks

- **Optimize Shape:** where material is needed to carry a study's loads to its supports, for a share of the
  mass kept or a mass in grams, with faces to keep and planes of symmetry; how much stiffness the shape
  keeps; STL or 3MF. `study.keep`, `study.shape`, `study.optimize`; `[study.optimize]` in the part file;
  [docs/topology.md](https://github.com/ElRashoMacuin24/tenon/blob/main/docs/topology.md).
- **A part in pieces** held by one piece used to solve, with a displacement of billions of millimetres and
  "it holds". A piece not held fast is now refused, in words.
- **Bearing loads** go the way asked, in full. On a whole round with flat ends that changes little (at most
  0.08 % in the 38 studies compared); on a wall not symmetric about the load, tens of percent, up or down.

### Sketches

- **3D sketches:** a sketch on no plane, with lines, splines, bends and points through vertices, work
  points or typed coordinates, staying on an axis when near one or typed along one; Include Geometry;
  constraints and dimensions; a Sweep runs along it, and work features come from its points and lines.
  `sketch.new_3d`, `sketch3d.*`, `work.point_at`.
- **Text:** its letters are profiles, in one built-in font, so that a part is the same everywhere.
  `sketch.text`, `sketch.edit_text`.
- **Ellipse**, **Stretch**, **Smooth (G2)**, **arc slots** and **Center Point** (points on their own are
  centre points, which a hole goes at, or sketch points). `sketch.ellipse`, `sketch.stretch`,
  `sketch.arc_slot`, `sketch.center_point`; the constraint type `smooth`.

### Parts and assemblies

- **Emboss:** a closed profile or text raised from or recessed into the faces it is seen on, or from the
  sketch plane; **Wrap to Face** lays it round a cylinder or a cone. `model.emboss`.
- **Derive:** the solid of another part file brought in, scaled and mirrored, by a path kept relative to this
  file. It follows that file on Rebuild All and on opening, and a drawing follows it. `model.derive`.
- **UCS:** a coordinate system of your own, whose planes, axes and origin serve as work features. `work.ucs`.
- **Pattern**, **Mirror** and **Copy** of components, in the parts list, drawings and exports as any other.
  `asm.pattern`, `asm.mirror`, `asm.copy`; an assembly file gains `[[pattern]]` and `mirrored_placement`.
- **Extrude** To, To Next and Between; **Revolve** To, To Next, Flipped or by two angles; **Hole** Spotface, To,
  Tapped, and on a face with no sketch; **Variable and Face Fillet**; a **work plane through three points**.
- **Constrain** gains Tangent and Symmetry, with offsets and angles typed. **Place** follows the pointer until
  Esc or a right-click. **Delete Features** says what goes with a feature. **Visibility** shows or hides a
  sketch or work feature.

### Sheet metal

- **Contour Roll:** an open profile rolled round an axis, up to 359.9 degrees. One straight line unrolls
  exactly, anything else row by row with a note of how far out that is. A flat ring takes punches, cuts and
  flanges; one rolled with other pieces, with a flange, has no flat pattern, and says so. `sheet.contour_roll`.
- **Lofted Flange:** a sheet between two open profiles, smooth or as press-brake facets. `sheet.lofted_flange`.
- **Punch Tool:** cuts, a dimple and a louver, at a sketch's centre points or at clicks on a flat face, with
  layers `PUNCH` and `PUNCH_ID` in the flat pattern's files. `sheet.punch`.
- **Rip, Unfold and Refold** open a closed shape, lay bends flat to cut across them, and fold them back.
  `sheet.rip`, `sheet.unfold`, `sheet.refold`.

### Drawings

- **Auxiliary**, **Crop**, **Break** (dimensions across it keep the true length), **Break Out** and **Overlay**
  (an assembly's view again in another position, in phantom lines). `drw.auxiliary`, `drw.crop`, `drw.break`,
  `drw.break_out`, `drw.overlay`.
- **Baseline and Ordinate** measure several places from one, as one record that follows the model. A set is
  refused on an isometric view, and on an auxiliary view of a slanted edge, which is drawn turned on the
  sheet. `drw.baseline`, `drw.ordinate`.

### Getting it

- The archive now holds the user's documents (`docs`) and the licences of the fonts in the window, and its
  NOTICE says what the bundled libraries' licences ask to have said.
- The first start on macOS 15 and later is by Open Anyway in System Settings: README and guide say so.

### Fixed

- **A list under an arrow** that the window's edge pushed left took no click on its words, though the rows lit
  up: at the default window, all of Analysis > Displacement Display. Each list is now as wide as its rows.
- **In a 2D sketch,** Measure, Fillet, Chamfer, the work features, the patterns and the primitives opened
  panels no click could reach. They now leave the sketch first, and so does Start 2D Sketch, which asked for a
  plane that neither Esc nor a click answered, and then took the next click on the part for one.
- **On the flat pattern,** keys, search and the title bar reached the part behind it, and E then Enter made a
  feature unseen. What is on the folded part now says so, and the View tab has what a flat pattern has.
- **Command search** puts what works where you are first, says when nothing matches, and its Esc is for the
  box and not for a panel that is open. Search finds what a button reads.
- **Keys:** L, C, A, D, X and O outside a sketch ask for a plane, as their buttons do. **Esc** leaves windows,
  topmost first, the navigation bar's tools and Free Rotate, whose button now lights. The File menu closes on a
  press elsewhere, and a second click closes what it opened.
- **A part that does not build** said nothing where it was used: the component was not there, with a mass of 0
  and no interference. It is now red in the browser and on the status line, what needs it names it, and each
  drawing view says so, as a drawing of an assembly with a part file gone now does too. `asm.open`, `drw.open`,
  `asm.tree` and `asm.bom` gain `failing`.
- **Undo:** `asm.undo` takes back `asm.edit_part`, where it took back the step before it. Undo after deleting
  the last component of a part brings its part back too. Ctrl+Z with a panel open leaves the panel.
- **Saving** over a file another window or program has saved since it was opened here now asks. A save that
  fails no longer leaves `name.tmp`. A part saved with a size its feature cannot have opens again, with that
  feature in error. A work plane given as a profile is refused, where the file saved could not be opened.
- **Safety:** what a file says is held to limits. A part or model on another machine is not gone to because a
  file names it, unless the file is on that machine itself. A stress study no longer runs what it finds in a
  `.pixi` folder of the working folder. The control port limits a connection before it gives its token.
- **Panels:** OK with nothing touched changes nothing. OK on a preview that failed says why and stays. A value a
  field refused is not taken for OK. A Cut on a sketch on a face starts Flipped, where it removed nothing.
- **Stress checks:** a blind hole with a drilled tip often could not be solved: the mesher now tries again.
  Every way out of Stress Analysis stops a run, and so does a change. A study that does not read gives its
  line.
- **Sheet metal and shells:** on the Sheet Metal tab, Hole and Corner Round said a sheet made with Face had no
  solid. A shell thicker than a chamfer says to make the shell first.
- **Speed and hanging:** STEP of a large assembly took as long as the square of its components, 79 s for 902;
  2 s now, as one assembly. The close button did nothing during one long operation on the part: it closes
  within a few seconds.
- **The window:** a ribbon wider than the window has an arrow and scrolls. A long status message shows its first
  line, with all of it under the pointer. Sketch dimensions were near-white in the light theme. Finish Sketch
  and Clear Explode with nothing to do say so, as Auto Explode of nothing and Update with no views now read
  right. The radial menu's labels are on one line, and Repeat offers what can be done there.
- **Command line:** `tenon --help` and `tenon-cli` with nothing say how. `tenon-cli info` and `convert` say a
  damaged STEP file could not be read, and see each component of an assembly's STEP file on its own, another
  program's too, where that read as one compound. `--size` holds against the saved size. `study.solve`'s `keep`
  goes under `--out`. A face's name in JSON is a string past what a double holds, for JavaScript clients.

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
