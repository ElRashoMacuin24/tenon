# Changelog

What changes from one release of Tenon to the next. Every item that works has the test that
shows it in [ROADMAP.md](https://github.com/ElRashoMacuin24/tenon/blob/main/ROADMAP.md).

## Unreleased

**New:** Optimize Shape in Stress Analysis works out where material is needed to carry a study's
loads, for a share of the mass kept, as a shape to model over (it needs Gmsh and CalculiX, as
Simulate does). Sketches gain text, ellipses, arc slots, Stretch and Smooth (G2). Parts gain Extrude
To, To Next and Between, Revolve To and To Next, spotface and tapped holes, holes placed by clicking
a face, and variable and face fillets. Assemblies gain Tangent, Symmetry and placing by the pointer.

### Coming from 0.1.0

- **Your files open as they were,** and one that uses nothing new is saved byte for byte, so it goes
  on opening in 0.1.0.
- **Solve again** any study with a bearing load on a wall not symmetric about the load: results move
  by tens of percent (below). A part in pieces with a piece not held fast is now refused, where it
  gave a meaningless answer.
- **Hole:** Tap Drill is now Tapped, which puts the thread on the wall. A hole from 0.1.0 opens as it
  is: Simple, whatever its size, until you choose Tapped; its own Thread feature stays.
- **Constrain:** Flush is now Mate's second solution.
- **Going back to 0.1.0.** It refuses a file that uses what is new, in the words "the project is
  damaged", though it is not: text, ellipses or Smooth; an extrusion To, To Next or Between; a
  revolution To, To Next, by two angles or Flipped; a spotface or To hole; a variable or face fillet;
  a work plane through three points; a Tangent or Symmetry relationship. Other new things it opens, and on a save it loses them: a
  tapped hole's thread (the solid is the same), which points are sketch points, which sketches are
  shown, and a study's Optimize Shape settings.

### Stress checks

- **Optimize Shape** (topology optimization): in Stress Analysis, the Optimize panel works out
  where material is needed to carry a study's loads to its supports, for a share of the mass
  kept or a mass in grams. Keep Faces keeps bolt holes and mating faces (as deep as asked),
  Symmetry makes the shape the same on both sides of up to three origin planes or work planes.
  It runs with Gmsh and CalculiX as Simulate does, on a mesh of its own, coarser round small
  details; when thin walls or small details take far more nodes than the resolution's, the
  elements are made larger, and it says so. It shows which round it is on (Esc stops it and
  keeps the shape so far, unless a menu, a panel or a prompt takes the Esc; leaving the
  environment stops it and drops what it came to, and so does changing the part), draws the shape
  over the part with what there is to know of it written beside it and a slider for the share
  shown, says how much of the part's stiffness it keeps, and exports it as STL or 3MF from one
  save dialog. A guide to model over: check what you make with
  Simulate. Settings are kept with the study (`[study.optimize]` in the part file; `remove` or
  `grams`, not both), and `tenon-cli diff` compares them. New commands `study.keep`,
  `study.shape`, `study.optimize`; `study.check` gains `shape_needs`, what only a shape needs,
  which does not stop Simulate; `study.solve` and `study.optimize` over the control port are runs
  the window can stop; [docs/topology.md](https://github.com/ElRashoMacuin24/tenon/blob/main/docs/topology.md).
- **A part in pieces.** A bore wider than a plate leaves a part in two pieces that is one solid to
  the kernel. A stress check of it held by one piece used to solve, with a displacement in the tens
  of billions of millimetres and "it holds". A piece not held fast is now refused, in words, also
  when nothing pushes on it, where the rest of the old answer was sound.
- **Bearing loads** go the way asked, in full. They push square to the round face at each spot, not
  to the side of the element it is on. On a whole round with flat ends that changes little (at most
  0.08% in the 38 studies compared). On a wall not symmetric about the load (a rounded corner, a
  slot's end, a bore with slanted ends) it changes results by tens of percent, up or down: the old
  load came out off the way asked and too large (1.2 times on a square corner's fillet loaded along
  a side, up to about twice on a shallower round), and the new one goes the way asked, with its pushes then no longer square to the face, since pushes square to such a
  wall cannot add up to that direction. Studies with a bearing load on such a wall should be solved
  again.

### Sketches

- **Text.** Sketch > Create > Text: click where it starts, type it in the Format Text box
  (several lines, the height of its capitals, an angle), OK. Its letters are profiles: a sketch
  of text alone extrudes or cuts at once, and in a feature's panel a click in a text's box adds
  its letters. Double-click a text to change it; drag it by its box; Scale stops at what a text can
  be, 0.01 to 10000 mm tall. One font, DejaVu Sans,
  built in so that a part is the same everywhere. New commands `sketch.text` and
  `sketch.edit_text`; `sketch.info` lists the texts. Files gain the entity `text` and the
  region key `{ text = id }` (an addition to format version 2).
- **Variable fillets.** The Fillet panel has Constant and Variable: a start and an end radius,
  marked at each end of the edges in the view. `model.fillet` takes `end_radius`, or `radii`
  for more places along the edges.
- **Face Fillet** under Fillet: two sets of faces, and every edge between them is rounded,
  found again whenever the part is rebuilt. `model.fillet` takes `faces` in place of `edges`.
- **Smooth (G2).** Constrain > Smooth: a spline and the line, arc or spline it ends on are
  tangent there and bend as sharply on both sides (one it does not end on is refused in words). A
  spline drawn from an existing point now
  starts on that point (`sketch.spline` takes point ids among its `points`); the constraint
  type is `smooth`.
- **Ellipse.** Under Circle: click the centre, the end of one axis, then a point on the
  ellipse. Dimension it by clicking it (half its first axis, then the other). Lines, arcs and
  circles cut it into regions, and it extrudes, revolves and cuts exactly. `sketch.ellipse`;
  files gain the entity `ellipse`.
- **Stretch.** Sketch > Modify > Stretch: select geometry, click where from and where to (or
  type X and Y). It moves, what is joined to it stretches, and the dimensions across the
  stretch take the new sizes. `sketch.stretch`.
- **Arc slots.** Center Point Arc Slot and Three Point Arc Slot join the slots under
  Rectangle: an arc's centre, start and end (or its start, end and a point on it), then how
  wide. Its sides share their centre and its ends stay tangent to both. `sketch.arc_slot`.
- **Center Point.** Sketch > Format > Center Point: points on their own are centre points
  (drawn as a cross, and a hole goes at them) or sketch points (a dot; a hole takes them only
  when picked). On the selected points, or with none selected, on what Point makes.
  `sketch.center_point`, and `center` of `sketch.point`; files mark `sketch_point = true`.

### Parts and assemblies

- **Delete Features.** Deleting a feature from the browser now says what goes with it: the
  sketches it is made from, and the features that use it, each with a tick, in one undo step.
  `feature.delete` takes `consumed` and `dependents`.
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
- **Revolve** turns Default, Flipped, Symmetric or Asymmetric (two angles). `model.revolve` takes
  `backward` and `reverse`.
- **Place**: after the first part (grounded at the origin), each instance follows the pointer and
  a click puts it down, until Esc or a right-click.
- **Visibility.** A sketch's or work feature's menu in the browser shows or hides it; saved with
  the part. `feature.visibility`.
- **Constrain** offers Mate, Angle and Insert, with Flush as Mate's second solution; offsets and
  angles are typed.

### Getting it

- The archive now holds the user's documents (`docs`: the guide, stress checks, Optimize Shape,
  scripts, commands, agents, the file format), and its README says where they are. The licences
  of the fonts built into the window are in it too, and NOTICE says what the bundled libraries'
  licences ask to have said.
- The first start on macOS 15 and later is by Open Anyway in System Settings: README and guide
  say so.

### Fixed

- OK with nothing touched changes nothing now, on any feature: a through cut stays through, and an
  angle comes back to the bit. A window's panel asks for no more once it has closed, in Stress
  Analysis and assemblies as in a part. Keep Faces and Symmetry no longer put a stress result out of
  date. The Format Text box refuses what a field refused, closes on Esc, and Finish keeps what was
  typed; characters the font lacks are named. A conflict in a sketch says what it is with, in words.
  Projected no longer stacks a view on one already there. Command search finds what a button reads,
  and its Enter stays its own.
- From the command line: `tenon --help` says how and ends, where it opened a window. `--size` holds
  against the saved window size. `tenon-cli bench --out` takes a relative folder and finds the examples
  in a package; `tenon-cli upgrade` no longer calls what is no document current; `tenon-cli diff`
  sees a sketch shown or hidden, and does not call a marked point moved. Fields a later build put at
  the top of a file are kept by the window and by scripts, as the format's page said they were.
- The control port limits what a connection may do before it gives its token: 64 KiB in its first
  request and 10 seconds to send it. After that it may be quiet as long as it likes. See
  [docs/mcp.md](https://github.com/ElRashoMacuin24/tenon/blob/main/docs/mcp.md).
- A change while Simulate runs now stops it, as it does Optimize Shape. Define A-Side gives up on
  Esc, as it says. Finish Analysis closes Mesh Settings. A long status is cut at the counts, with all
  of it under the pointer.
- A run ended by a crash or from outside left its working folder in the system's temporary folder
  for good. The first run of a later start now takes those away.
- After Undo or Redo the status bar went on saying what had been undone, and in a new sketch the
  tool's instruction after the sketch had gone. "Nothing to undo" was still an error in an assembly
  and a drawing, as it no longer is in a part.
- `tenon-cli info` and `convert` died with no word on some damaged STEP files, a changed byte being
  enough. They now say the file could not be read.
- The close button did nothing while the geometry thread was inside one long operation of the
  kernel: the window hung and memory went on growing. It now closes within a few seconds.
- A part saved with a size its feature cannot have, as a parameter set to 0 leaves a hole's depth,
  could not be opened again: "the project is damaged". It opens now, also if 0.1.0 saved it, with
  that feature in error to be put right.
- What a file says is now held to limits, as the commands' words are: a text to its 2000 characters
  (a small file could hang the rebuild); a name, or what fills a title block, to 1000 bytes, and a
  parameter's equation or comment to 64,000 (a very long one crashed the window, or took minutes to
  draw); and a part or model on another machine is not gone to because a file names it (Windows would
  connect there, offering the user's credentials) unless the file is on that machine itself.
- A stress study no longer runs what it finds in a `.pixi` folder of the working folder: only a
  program built in a checkout looks in that checkout's.
- Saving replaced a file another window or program had saved since it was opened here, with no
  word, and that work was gone. It now asks: Overwrite, Save As or Cancel. A save that fails no
  longer leaves `name.tmp` beside the file, and says which file it was.
- In a window narrower than a tab's ribbon, what did not fit could not be reached, with nothing to
  show it was there (Finish Analysis under about 1130 wide, Go to Flat Pattern under about 1215).
  The ribbon now has an arrow at the end that goes on, and scrolls with the wheel.
- A message of several lines in the status bar, as every failure of the mesher or the solver is,
  showed as clipped pieces of its middle lines, with nothing under the pointer. The bar shows the
  first line, and the whole of it comes up under the pointer.
- A spline that crosses its own profile made a solid of negative volume and exported it, with no
  word: it is now refused, by name. So are letters picked with a region they lie over, whose
  solids overlapped and whose volume was counted twice.
- A part with an elliptical wall took about two seconds longer to rebuild, at every change above
  it and every preview. It is now as quick as a round one.
- OK with To or Between chosen and nothing picked made a feature of another kind, and said so. It
  now asks for the face or plane. OK on a panel whose preview has failed closed it and said "Made":
  it now says why again, and the panel stays. A Cut on a sketch on a face went out of the part by
  default and removed nothing: it now starts Flipped, as a hole does. With a sketch on a work
  plane, Extents To could not pick a face seen through that plane.
- A stress study of a part with a blind hole with a drilled tip often could not be solved: the
  solver refused the mesh, in its own words. Tenon now has the mesher try again where that
  happens, and says what helps if that fails too.
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
- Leaving Stress Analysis while Simulate ran (Finish Analysis, File New, opening another document,
  closing Tenon) left it running: the new part's geometry waited behind it, and closing the window
  did too. Every way out now stops it, and what it came to is dropped.
- In a script, `study.solve`'s `keep` folder went to the working folder, not under `--out` as
  every other file and folder does.
- A study that does not read now gives its line and its name, as a feature does.

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
