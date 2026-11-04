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

- **Your files open as they were,** and one that uses nothing new is saved byte for byte, so it goes
  on opening in 0.1.0.
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
- **A flange on a sheet a Mirror feature made** came to nothing in 0.1.0, with no word. It is there now,
  so such a part gains it on opening.
- **Going back to 0.1.0.** It refuses a file that uses what is new, in the words "the project is
  damaged", though it is not: text, ellipses or Smooth; an extrusion To, To Next or Between; a
  revolution To, To Next, by two angles or Flipped; a spotface or To hole; a variable or face fillet;
  a work plane through three points; a Tangent or Symmetry relationship; a mirrored occurrence in an
  assembly; an auxiliary or overlay view; a baseline or ordinate set; an emboss, a derived part or a
  UCS; a 3D sketch or what is made from one; a rip, an unfold, a refold, a contour roll, a lofted
  flange or a punch. Other new things it opens, and on a save it loses them: a tapped hole's thread
  (the solid is the same), which points are sketch points, which sketches are shown, a study's
  Optimize Shape settings, an assembly's patterns (each element is then a free component where it
  stands, those of a suppressed pattern shown and counted), and a drawing view's crops, breaks and
  break outs (the view is drawn whole). And one it opens with no word and gets wrong: a sketch, work
  plane or mate made here on a flat face from a Mirror feature, a chamfer or a part-turn Revolve lands
  turned over there, since it still reads such a face as looking into the part.

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

- **3D sketches.** Start 3D Sketch now works: a sketch on no plane, a feature with its own tab. Lines through
  vertices, work points, existing points or typed X, Y, Z, staying on an axis when near one or typed along one; splines through
  points; bends that round a corner by a radius and keep it; points; and Include Geometry, which brings model
  edges in to follow the part. Coincident, fix, parallel to X, Y or Z, parallel, perpendicular, equal, tangent;
  dimensions of a length, a distance, a coordinate and a radius, which are parameters; the degrees of freedom
  shown, and over-constraining refused in words. A Sweep runs along its curves in any direction (a new
  `Curve3::BSpline` for splines); work points, axes and planes come from its points and lines, among them
  a plane square to a curve at its end; Measure takes its curves. A sweep goes on along a path that is bent, unbent
  or drawn on from its end after it was made. New commands `sketch.new_3d`, `sketch3d.*` and `work.point_at`; a new
  `sketch3d` feature type in part files.
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

- **Emboss:** a closed profile, text included, raised from or recessed into the faces it is seen
  on, following them where they curve, as far as they look toward the sketch and nothing lies over
  them; or from the sketch plane (added on one side of it and taken away on the other, refused in
  words when what is added would come loose or shut a hollow in), with a direction and a taper.
  **Wrap to Face** lays it round a cylinder or a cone with its lengths kept, stopping at the face's
  edge. New command `model.emboss`.
- **Derive:** the solid of another part file brought in, by a path kept relative to this file,
  scaled, mirrored, joined or a new solid. It follows that file on Rebuild All and on opening, not
  on a save; a file that is gone fails in words and the part still opens; a part cannot derive
  itself. New command `model.derive`.
- **UCS:** a coordinate system of your own, by offsets and turns from the origin or another UCS or
  through three points, shown as a triad that keeps a size you can see and click on a part of any
  size. Its planes, axes and origin serve wherever work features do: sketches, Extrude To, Mirror,
  patterns, Revolve, Measure. Corners of the part can now be clicked for three-point work planes
  too, and Measure measures to work features and says where one of them is. New command `work.ucs`;
  `ucs_plane` on the commands that name a plane by `plane`, `face` and `work_plane`.
- **Pattern**, **Mirror** and **Copy** of components. Pattern
  repeats chosen components in rows along one or two directions (a picked edge, flat face or axis, a
  count and a spacing) or round an axis (a count and an angle). It is one record, a row in the
  browser with its elements under it; a count or spacing changed moves, adds or takes away elements;
  they follow what they repeat and what gives the direction. Each element is a component: it is
  counted in the parts list, can be constrained to, and is drawn, checked and exported as any other.
  Mirror reflects chosen components in a plane or flat face: each is a mirrored occurrence of the
  same part file holding its reflected geometry, on a row of its own in the parts list as a mirrored
  part, or for a part that is the same as its mirror image, the same part placed again in the
  mirrored position. Copy makes independent copies beside what they are copied from, of the same
  parts or in new part files saved beside the assembly, with the constraints among them. New
  commands `asm.pattern`, `asm.mirror`, `asm.edit_pattern`, `asm.suppress_pattern`,
  `asm.delete_pattern`, `asm.copy`; `asm.rename` takes `pattern`; `asm.tree` and `asm.bom` say what is
  mirrored and what is an element. An assembly file gains `[[pattern]]` records, `next_pattern` and a
  component's `mirrored_placement`.
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

### Sheet metal

- **Contour Roll:** an open profile rolled round an axis as sheet, up to 359.9 degrees. Its flat
  pattern unrolls each line and arc at the neutral surface: exactly for one straight line (a ring,
  a cone, a flat ring), and row by row for anything else, with a warning that says how far out it
  is. A flat ring takes punches, cuts and flanges like any flat face; one rolled with other pieces, with a flange, has no flat pattern, and says so. Cut Across Bend and Cut Normal
  follow a cylinder, and refuse in words a profile that reaches a cone or a round part. New command
  `sheet.contour_roll`; feature type `contour_roll`.
- **Lofted Flange:** a sheet between two open profiles, as one smooth die-formed surface laid flat
  by triangles (and saying how close that is) or as press-brake facets with bends between them, to
  a chord tolerance, which unfold exactly. A round going to a square comes out as the textbook
  one. A die-formed sheet is held to its thickness: too thick for a curve, or twisted enough to thin it
  by more than 2 %, it is refused with the figures, and with anything joined to it or cut part way into
  it, it has no flat pattern. New command `sheet.lofted_flange`; feature type `lofted_flange`.
- **Punch Tool:** a library of shapes written for Tenon, stamped at a sketch's centre points or at
  clicks on a flat face, with a rotation: round, square, rectangle, obround slot and D-shape as
  cuts, a dimple and a louver as formed shapes. Refused in words across a bend, and a formed shape over a
  hole. In the flat
  pattern and its DXF and SVG a cut is its outline and a formed shape a centre mark and a bend line,
  each with its name (new layers `PUNCH` and `PUNCH_ID`; `punches` of `sheet.export_dxf` and
  `sheet.export_svg` leaves them out). New command `sheet.punch`; feature type `punch`.
- The flat pattern says what it is not exact about (`notes` of `sheet.flat`), and bends close
  together are named where their names do not lie on one another.
- **Rip, Unfold and Refold.** Rip opens a seam in a
  face, flat or round, so that a closed shape can be laid flat: a slit through a point, a slit
  between two points, or the whole face out. A tube that was ripped lays flat from its round side.
  Unfold lays chosen bends flat from a face that stays where it is, so that cuts can go across
  them; Refold folds them back, with what was cut going round the bend, and says in words what
  cannot be folded back. New commands `sheet.rip`, `sheet.unfold`, `sheet.refold`, new feature types
  `rip`, `unfold`, `refold` in the part file. A rip that would part the sheet is refused, as is an
  unfold of a bend with a cut through it that is not square to the sheet: Cut Normal and Cut Across
  Bend are, and a plain straight cut is not.

### Drawings

- **Auxiliary, Crop, Break, Break Out and Overlay.** Auxiliary is a
  view square to a straight edge of its parent, which turns with the edge. Crop draws only
  what lies inside a rectangle or a circle on a view. Break takes a band out of a base
  or projected view, one or more of them, up the sheet or across, with break lines drawn;
  dimensions across it keep the true length. Break Out cuts a rectangle or circle of a view
  down to a depth from the nearest of the model, or through, draws what is then seen, hatches
  what it cut, and says so when it takes nothing away. Overlay draws
  an assembly's view again in another position, in phantom lines, from values of its joints
  and offsets typed in a panel and solved by the assembly solver; what they come to shows
  on the sheet as they are typed. All of them follow the model, have a row in the browser
  whose double-click reopens them, come through undo, save and open, and go to PDF, SVG
  and DXF (a `PHANTOM` layer and line type). New commands `drw.auxiliary`, `drw.overlay`,
  `drw.overlay.values`, `drw.crop`, `drw.break`, `drw.break_out`, `drw.modifier.delete`;
  `flip` and `edge` of `drw.view.edit`. In the file, the view kinds `auxiliary` and `overlay`
  and a view's `mods`.
- **`drw.pick` calls a curved edge a curve.** It said `line`, with its chord for a length, and
  so an edge that curves could pass for a straight one. The length is still the chord.
- **An edge across a detail's circle** can be picked where it crosses, and not only when one of its
  ends is inside.
- **Baseline and Ordinate** (Annotate > Dimension) measure several places from one, as one
  record. Click a datum or origin in a view, then the edges or points to measure, then where the
  set goes; a panel says what has been picked and the set shows where it would go. A baseline
  set stacks a dimension to each place, evenly and never overlapping; an ordinate set gives each
  place its position on its own line with no arrows, moving values apart where they would touch.
  Values follow the model. Sets are saved in the drawing, can be added to, taken from, moved,
  changed and deleted, have a row in the browser, and go to PDF, SVG and DXF. New commands
  `drw.baseline` and `drw.ordinate`; `drw.annotation.edit` gains `remove`, `add`, `datum`, `axis`
  and `spacing`. Suggest Dimensions leaves out what a set already gives, and refuses a section or
  projection of an isometric view as it does the view itself. A set is refused on an auxiliary view of a
  slanted edge, which is drawn turned on the sheet: an Aligned dimension gives the true size there.

### Getting it

- The archive now holds the user's documents (`docs`: the guide, stress checks, Optimize Shape,
  scripts, commands, agents, the file format), and its README says where they are. The licences
  of the fonts built into the window are in it too, and NOTICE says what the bundled libraries'
  licences ask to have said.
- The first start on macOS 15 and later is by Open Anyway in System Settings: README and guide
  say so.

### Fixed

- Esc now leaves what the guide says it does: the windows with an X or a Cancel button, topmost first,
  the navigation bar's Pan, Zoom and Orbit, and Free Rotate, whose button now lights while it is on.
- `asm.undo` did not take back `asm.edit_part`: it took back the assembly's step before it, placed
  components included, and the part stayed as edited. It is now a step of the assembly's history. In the
  window's control port, it said there was nothing to undo.
- A part that does not build, as one whose Derive has lost its file, said nothing where it was used: the
  component was simply not there, with a mass of 0 and no interference. It is now red in the browser and
  on the status line, what cannot be worked out without it names it, and each drawing view of it says so.
  A drawing of an assembly with a part file gone left the components out with no word: the view and
  the status line now say so, and `missing` of `drw.open` lists it. `asm.open`, `drw.open`, `asm.tree` and
  `asm.bom` gain `failing`.
- Entries that did nothing and said nothing: Finish Sketch with no sketch open and Clear Explode with no
  exploded view now say so, and Clear Explode says when it has. The Flat Pattern's View tab offered Visual
  Style, the projections, the Orientation Cube and the Navigation Bar, none of which it has: it now has
  Browser and Zoom All, as a drawing's has.
- Command search ranks what works where you are first. Enter on a name that several commands have ran
  the first in the list, whichever environment it was for: Mirror and Copy in an assembly said "that
  works on a part", Text and Dimension in a drawing were refused, and Line in a 3D sketch closed it to
  ask for a plane. And a command that is refused no longer closes the 3D sketch it was asked for in.
- On the flat pattern, keys, command search and the title bar still reached the part behind it: Measure
  and Extrude's panels asked for clicks on faces not on the screen, and E then Enter made a feature
  unseen. What is on the folded part now says so.
- Start 2D Sketch (S) inside a sketch asked for a plane and said Esc cancels, but neither Esc nor a click
  did anything; it waited on past Finish Sketch, and the next click on the part started a sketch nobody
  asked for. It now finishes the sketch first, and then asks.
- In a 2D sketch, Measure, Fillet, Chamfer, Shell, Draft, Thread, Split, the work features, the patterns,
  the primitives and the sheet metal commands that pick edges opened their panels and said to click,
  and no click reached them. Measuring while sketching and Fillet's key F were the likeliest ways in.
  They now leave the sketch first, as they do a 3D sketch.
- A list under a button's arrow that the window's edge pushed to the left took no click on its words:
  the press closed it and nothing ran, though the rows lit up under the pointer. At the default window
  this was all of Analysis > Displacement Display; narrower, Auto Balloon and Face Fillet. Every list
  was also 600 wide, over what lay beside it: each is now as wide as its longest row.
- On the Sheet Metal tab, Hole, Corner Round and Corner Chamfer said a sheet made with Face had no
  solid, and would not open, unless it also had an extrusion.
- A sweep round a sharp corner used to stop at the first corner of its path, saying nothing, and made a
  third of the tube it should. It is now mitred between two straight lines, for any path, where it closes on
  itself too. A sharp corner at a curve (a spline, an arc, a spline closed on itself), a profile with holes, or
  one that keeps its direction is refused there in words, saying what to do.
- A profile from what is no sketch is refused when it is asked for. Extrude and the others took a work plane
  or a 3D sketch without a word, and the file saved could not be opened.
- Undo after deleting the last component of a part brought it back with nothing behind it, not drawn,
  and refused by mass and export until the file was opened again. It now comes back with its part.
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
