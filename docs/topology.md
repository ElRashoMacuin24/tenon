# Optimize Shape (topology optimization)

Optimize Shape works out where in a part material is needed to carry a stress study's loads to
its supports, for a share of the part's mass kept (DEC-123). What comes back is a shape to look
at and to model over, not a finished part: remodel the part from it with ordinary features, then
check what you made with Simulate.

It uses the two programs a stress check uses, Gmsh and CalculiX, started as programs of their
own ([stress.md](stress.md)); nothing of them is in Tenon. The method is the published one,
written here from the papers listed at the end.

## Using it

In the Stress Analysis environment (Environments > Stress Analysis), a study with its supports,
loads and material, as for Simulate. Then, on the Analysis tab's Optimize panel:

- **Keep Faces**: faces the shape must keep (bolt holes, mating faces), with material kept as
  deep under them as asked (or one layer of elements). Faces that are held or loaded always keep
  a layer.
- **Symmetry**: up to three planes at right angles the shape is to be the same on both sides of:
  the origin planes and the part's work planes, each ticked. The part must itself be the same on
  both sides of each, or it is refused: before anything is divided, its middle must be within a
  tenth of its own width of the plane (a boss of more than a quarter of a thin plate's thickness
  is off by that), and once it is divided, a point of it must be within a tenth of an element of
  its mirror image. The supports and loads should be the same on both sides too. A face of the part
  cannot be one: a solid has material on one side of its own face and none on the other. A part not
  centred on an origin plane takes a work plane through its middle (made outside the environment:
  Finish Analysis first).
- **Optimize Shape**: how much to keep (a share of the mass taken away, 5 to 95%, or a mass in
  grams), the smallest member (Auto: three elements), the resolution (Coarse, Medium, Fine: about
  12,000, 30,000 or 80,000 nodes on a plain solid, see [the mesh](#the-mesh)), and Sharpen (on:
  the design is pushed toward solid and empty, in more rounds). An element size or a limit of
  rounds that the study holds, from a command or a file, is shown too, with a button to clear it;
  picking a resolution clears the size. OK keeps the settings with the study and works the shape
  out. The view says which round it is on; **Esc** stops it and keeps the shape found so far
  (stopped before the first round is done, there is none yet, and it says so). Esc meant for
  something else goes to that: with a menu, a panel, a text field, a prompt, a sketch tool, a
  repair or a pick of a plane or a face going on, it is that one's, and the next press is the run's.
  A window closed by its own button (About, the part's properties, the options, the tables, the
  print check) has no use for Esc: with one open, Esc stops the run and the window stays. Esc held
  down is one press.

The shape is drawn over the part, which is drawn as its edges. The status bar says its mass, how
much was removed (and how much was asked, when it did not come to that: stopped early, or pieces
left out), how it ended and how much of the part's stiffness it keeps; a long status is cut off at
the counts on the right, and its whole text comes up under the pointer. Whatever else there is to
know of the shape (the warnings of `study.optimize`) is written over the view while it is shown,
and in the tip of its Shape row. A slider at the foot of the view shows the surface for another
share of the part: that shows where material ranks, not what an optimization for that share would
make. What is written over the view, the Shape row and its tip go by the share shown: whether its
surface is closed, the pieces it leaves out, whether it joins the loads to the supports. A run
stopped early may have no material ranked above a share: the surface then comes to less than the
slider says, and the note under it says so. The browser's study has Keep Faces and Shape rows, and a
Symmetry row once there are planes. A click on a kept set picks its faces out. A click on the Shape row shows or hides the shape, a
double-click opens Optimize Shape's settings, and its menu works it out again with the settings
the study has. **Export Shape** writes the shape shown as STL or 3MF, a share still in the slider's
field being the one meant: its save dialog offers both, and a name given with no ending is STL,
unless the system's dialog adds the ending of the kind picked. A surface that is not closed, or
that does not join a load to a support at that share, is written, and said to be so. Esc in the
slider's field gives up what was typed.

The shape and the stress in colour are not drawn together. When Simulate's results or Mesh View's
lines come, or a Results row is clicked, the shape is hidden, so that what was asked for is drawn;
a click on its row brings it back.

A kept set with a face the part no longer has is written in red with `(face lost)`, as a support
is. A plane of symmetry that cannot be one (a work plane that is gone or is not a plane, a face
held in a file) turns the Symmetry row red, saying how many are lost, and shows in the panel as a
line with `(lost)` to untick. These stop Optimize Shape before anything starts, and do not stop
Simulate.

Settings, kept faces and planes are kept with the study in the part file. The shape itself is
not: it goes when you leave the environment or change the part or the study. Export it to keep
it. What a shape does not hang on is no change to it: the part's or the study's name, or another
study, made or changed.

One run goes on at a time. While a shape is being worked out, another run, Simulate and Mesh View
are refused (Esc first). Leaving the environment stops the run, whichever way it is left: Finish
Analysis, File New, opening or starting another document, closing Tenon. What it came to is then
dropped and nothing is said of it; until it has stopped, a new Optimize Shape, Simulate or Mesh
View is refused: `The last run is still stopping: try again in a moment.` A part or study changed
while a run goes on stops the run there and then, as nothing it could come to would be kept: the
status bar says it is out of date, and that it was not kept. (For Optimize Shape, what is no change
to a shape, as above, is no change to its run. Simulate and Mesh View stop for any change, as what
they find goes out of date with any.) A shape that comes back while another study is being worked
on is kept for its own, and the status bar says to work on that one to see it and export it.

`study.solve` and `study.optimize` sent over the control port (`tenon --control`, `mcp --attach`)
are runs like these: one sent while a run goes on in the window is refused, and the window refuses
one while they go. The status bar and the view say one is going on, and Esc, and whatever else stops
a run, stops them, and the port is told. A step of `batch.run` stopped so ends the batch: its answer
has the steps done and, under `not_run`, that the rest were not run.

For agents and scripts: `study.keep`, `study.shape` and `study.optimize` ([commands.md](commands.md)).
`study.shape` refuses a face as a plane of symmetry. `study.check` says what a shape needs beyond
a stress check under `shape_needs`, in words: Gravity or a body's turning suppressed, kept faces the
part no longer has, a work plane of symmetry that is gone or is not a plane, a face used as a plane
of symmetry (a file that holds one still opens), planes not at right angles. `ready`, `needs` and
`lost` leave these out, so they do not stop `study.solve`; `study.optimize` refuses them before
anything starts. Not there, and said only when `study.optimize` is asked: at once, with no program
started, a mass the part cannot come to, a part off a plane of symmetry by more than a tenth of its
own width, and a closed edge far shorter than the elements; once the part is divided, what cannot
be told before: too many nodes, a piece not held fast, a part not the same on both sides of a plane
to within a tenth of an element, too much kept. `study.optimize` also tries its `stl` and `threemf`
before anything starts (the ending, a name the system does not take, the folder, a file that cannot
be written). A file that still cannot be written at the end does not lose the run: `not_written`
says why, and so do the `warnings`.

## What it can and cannot do

- Stiffness only, for one set of loads. There is no stress limit: a shape can yield. Check the
  part you make from it with Simulate.
- Gravity and a body's turning are refused (their loads depend on the material kept): suppress
  them to optimize.
- One solid of one material, linear and static, as for a stress check. A part that its own features
  cut into pieces (a bore wider than a plate, say) is still one solid to the kernel: every piece
  must be held fast, and one that is not is refused once the part is divided, as it has no one
  answer. So is it in a stress check. That goes too for a loose piece that nothing pushes on, though
  the solver would leave that one still and the rest would come out sound: join it up or hold it.
- The smallest member is approximate: it is what the filter allows, not a hard limit.
- Coarse resolutions give blocky shapes; the surface is faceted at the element size.
- A part with very many small details can be refused even at Coarse: suppress them to optimize
  (see [the mesh](#the-mesh)).
- A run takes minutes: about a minute and a half for 7,000 elements and 90 rounds on the machine
  the tests run on (2 to 3 times longer on a laptop). The geometry is busy while it runs.

## The mesh

A shape's mesh is not a stress check's. Both are second-order tetrahedra from Gmsh, on one thread;
a shape's differs in two ways (`mesh_for_shape` in `crates/fea/src/lib.rs`):

- **Coarser round small details.** Round holes and rounds get elements no smaller than a third of
  the element size (a stress check's go to a tenth), and the size at the part's boundary is not
  carried in through its inside. The filter works over at least one and a half elements, so a
  shape cannot use finer detail than that, and a stress check's grading would only cost nodes: on
  it, a plate with a few small holes came to three times the nodes its size was picked for, and
  one with 32 small holes to seventy times. The one thing that takes it lower is a closed edge,
  such as a small round hole's: elements are no smaller than a third of the shortest one's length
  when that is less, since a hole's wall with elements much wider than that folds flat, and the
  mesher then fails, or gives a mesh with no hole there at all. That floor is the mesher's for the
  whole part, and at twelve elements to a full turn every other round face would go down to it (one
  more small bore took a plate from 16,000 nodes to 34,000), so with it they go by four to a full
  turn, at the first try. A closed edge more than a hundred times shorter than the elements is refused before
  anything starts: the mesher must go down to it along all of the wall under it, which took it
  nearly ten minutes for a bore of 0.005 on a 10 mm plate, only for it to be refused all the same.
- **Straight mid-side nodes.** Curved ones can turn an element inside out on a small hole at these
  sizes, and the solver refuses such an element; straight ones cannot. A shape is a guide, and does
  not need a bore's roundness: a bore can still be kept, and a plate on a pin with a bearing load,
  both on bores, comes out within 5% as stiff as on a stress check's mesh. So that nothing else
  hangs on the mid-side nodes, a mesh's surfaces are told from the part's faces by their corners,
  which lie on the part's own faces (the middle of a flat strip on a thin round rim lies inside the
  material, nearer the flat faces), and a bearing load pushes square to the round face at each
  spot, not square to the flat side it is on, so all of it goes the way asked. (The corners tell a
  face to within how far the kernel's drawing of a round stands off it, 0.01 mm: on a plate of 5
  micrometres or thinner, a bore's wall can be taken for the flat face beside it.)

The mesh a shape is worked out on is not the one Mesh View shows, which is a stress check's. Should
the mesher fail on it, or a hole close up all the same (a side of the boundary with more than two
triangles on it), the part is divided once more with elements down to a tenth of the size, as a
stress check's go, or half the first floor if that is less. (Not when the mesher ran past its ten
minutes: the second try is the slower.) If that will not do either it is refused, in words that say
what the mesher said and what helps: at Coarse or Medium, the next resolution up; at any, to
suppress the small holes and rounds; for an element size asked, smaller elements. Where it is the
closed edge and not the size that the floors go by, no size helps, and only the suppressing is said.

A part can really have such a side: a line of no thickness, where it touches itself (two bores just
touching, a bore just touching a side). The kernel draws that line too, so it is told from a hole
closed up, and the part is worked out. The surface of a shape through that line is not closed, and
is said to be so.

**How large the elements are.** From the part's volume V and the resolution's nodes N,
h = (9 V / N)^(1/3): about nine nodes to each cube of h. With a smallest member m, h is no more
than m / 3. So the figures of 12,000, 30,000 and 80,000 nodes are for a plain solid. Where thin
walls or small details take more, and the mesh comes to more than one and a half times N or past
the 100,000 nodes a shape is worked out on, h is made larger by (nodes / N)^(1/3) and the part
divided again, at most three more times and never past m / 3. The last mesh a shape can be worked
out on is kept: within the 100,000, with the study lying on it, with room for the share kept (the
layers under what is held and loaded are as thick as the elements), and not too much for the
filter. The element size the run says, and the filter's radius, are of the size it came to, and a warning says
the elements were made larger. An element size asked for is used as it is.

Larger elements are not always worth having. Where small details hold the nodes up, a larger size
brings them down by little, and all that grows is the filter's radius, so a mesh that has not fallen
by a fifth, and is still over one and a half times N, is not used: the one before stands, and a
warning says how many nodes it comes to, that each round takes that much longer, and what would
bring it down (a larger smallest member, when that is what holds the size down). When one size is
too many nodes and the next will not do, up to two sizes between them are tried.

Nodes as measured on plates held at one end (where the size was made larger, what it came to):

| Plate | Coarse | Medium | Fine |
|---|---|---|---|
| 60 x 30 x 10, plain | 11,480 | 26,407 | 64,280 |
| 80 x 40 x 10, four holes of 3 and 6 | 15,962 | 31,620 | 69,479 |
| 120 x 80 x 6, eight holes of 4 | 13,422 (4.16 mm) | 37,382 | 82,846 |
| 200 x 100 x 5, 32 holes of 3 | 16,103 (6.62 mm) | 36,635 (3.72 mm) | 92,162 (2.51 mm) |
| 200 x 100 x 5, one hole of 1 | 14,079 (4.84 mm) | 31,230 | 82,955 |
| 110 x 110 x 3, 100 holes of 1 | 32,275 (4.59 mm) | 42,713 (3.01 mm) | 89,168 (1.94 mm) |

**Past 100,000 nodes** a mesh is refused, in words that say what helps: for an element size asked,
to make the elements larger; for a smallest member that holds the size down, to ask for a larger
one; at Fine or Medium, to choose the next resolution down; at Coarse, to suppress the part's small
holes and rounds.

**Too much for the filter** (more than 512 MB, for many elements within the radius of each other) is
refused too, and that too says what helps: a smaller smallest member while that sets the radius, the
small holes and rounds suppressed, fewer planes of symmetry. While a smallest member sets the radius
it stays put as the elements grow, so a mesh over one and a half times N is divided again on larger
ones before that is said. While one and a half elements set it, the size of the elements does not
say which way it goes: larger ones are fewer but the radius grows with them, and where small details
crowd them together it takes in more of them, and smaller ones can come out the same; so neither is
offered.

**Too much kept** (the layers under what is held and loaded, and the kept faces, come to more than
the share kept) is refused as well, with what would leave room: a smaller share taken away or a
larger mass, when one can be asked that does; less kept, when faces are; and smaller elements, as the
layers go with them. Where no share will do, as when a thin plate is held on one side and loaded on
the other, it says to hold or load smaller faces.

## The method

Each element e of the part's mesh ([second-order tetrahedra with straight sides](#the-mesh)) has a
density x_e between X_MIN = 0.001 and 1, and a Young's modulus

    E(x) = E0 (FLOOR + x^3 (1 - FLOOR)),   FLOOR = 1e-9

(SIMP with a penalty of 3: a part-full element is stiff for its mass only when it is full, so
the design is driven toward solid and empty).

What is minimised is the compliance c, the work of the loads (twice the energy the part holds
under them), with the volume kept fixed at the share asked: the stiffest structure of that mass.

- **Filter.** Each element's density is seen as the weighted mean of the densities within a
  radius r of it: x~_e = sum_i a_ei x_i, a_ei = w_ei V_i / sum_j w_ej V_j,
  w_ei = max(0, r - |c_e - c_i|) (c the elements' middles, V their volumes). r is half the
  smallest member, and at least one and a half elements. With planes of symmetry, the weight
  sums over each element's mirror images as well, so the filtered field is the same on both
  sides of each plane.
- **Sharpen** (projection). x- = (tanh(b h) + tanh(b (x~ - h))) / (tanh(b h) + tanh(b (1 - h))),
  h = 0.5, with b raised through 1, 2, 4, 8 and 16: at most 20 rounds at each up to 8; at 16 until
  a stop rule ends it. Without sharpening, x- = x~.
- **What must stay.** Elements with a node held or loaded, and those of kept faces as deep as
  asked, are at 1 throughout.
- **Each round.** The solver is given each element's level, the nearest thousandth of x-, and
  says the energy U_e each holds. Then dc/dx-_e = -3 x-_e^2 (1 - FLOOR) 2 U_e / E(level_e)/E0 is
  carried back through the projection and the filter (dc/dx = A^T (dx-/dx~ . dc/dx-)), and so is
  the volume's slope.
- **Update** (optimality criteria): x'_j = x_j (-dc_j / (lambda dV_j))^0.5, held within a move of
  0.2 (0.2 / sqrt(b) when sharpening) and within X_MIN and 1; lambda is found by bisection so
  that the volume is the share asked, to 1e-10 of the whole.
- **Stopping.** When no density moves by more than 0.01, or the work changes by less than
  0.1% five rounds running (at full sharpness), or at the most rounds (100 sharpened, 60
  without), or after an hour, or when stopped.
- **The same answer twice, nearly.** Sums here run in a fixed order and the mesher runs on one
  thread, so the same study gives the same mesh, to the byte. The solver runs on all the machine's
  threads and adds up in the order they finish, so what it says need not be the same to the last
  figure: a solve repeated on the same densities agrees to about fourteen figures, and over a whole
  run a round's work can part by a few in a million as the densities it goes on part. The surface's
  points come out within thousandths of a millimetre on most parts, and within a hundredth or two on
  a thin plate; the work on the finished shape within a few parts in a hundred thousand; an STL's
  bytes need not be the same. On one thread they would be, but a round takes one and a half times as
  long at Coarse and two and a half times at Medium, and more above that: not worth an hour's run
  for the last figures.

## The shape

The densities are averaged onto the corner nodes (by volume; 1 where an element must stay). The
level t is found whose inside volume, worked out exactly for a field linear on each tetrahedron,
is the share asked; the surface where the density is t is cut out of each tetrahedron (marching
tetrahedra, the crossings on shared sides worked out once so the surface is closed), the part's
own boundary capped where it is inside, pieces cut off from all that must stay dropped (and
said), and a load that the kept material does not join to a support warned of. The surface is
smoothed by Taubin's two steps (10 pairs), which keeps its volume where plain smoothing shrinks
it; vertices on the part's boundary stay. A surface that does not come out closed all the same is
said to be so: among the warnings, over the view, and on Export Shape, which still writes it.

Then the shape is solved once: the densest elements, what must stay first, up to the share kept,
solid, and the rest empty. The ratio of the solid part's work to the shape's is the stiffness it
keeps.

Only that figure hangs on this solve, so the shape stands without it. If the solve fails, or the
run is stopped during it, the shape is kept, `stiffness_kept` and `compliance.shape` are null, and
a warning says why (`The shape could not be solved once, so how much stiffness it keeps was not
worked out: ...`, or `It was stopped while the shape was being solved once, ...`). A run stopped
before it, whose warning already says so, is not solved once at all.

## The solver, each round

CalculiX is given the mesh with an element set, a material and a section for each stiffness
level in use (L0 the floor to L1000 the material), the supports and loads as for a stress check
(no weight, no turning), and asked for each element's internal energy (`*EL PRINT`, `ELSE`). It
writes them to `shape.dat`, read back by element. The files are written each round in place of
the last; with `keep` they are left in that folder.

## Sources

The method is the published one; the code is Tenon's own, written from these. No topology
optimization program's code (beso, ToOptix, the top88/top3d codes, others) was read into Tenon.

- M. P. Bendsoe, O. Sigmund, Material interpolation schemes in topology optimization, Archive of
  Applied Mechanics 69 (1999); Topology Optimization: Theory, Methods and Applications, Springer
  (2003).
- O. Sigmund, A 99 line topology optimization code written in Matlab, Structural and
  Multidisciplinary Optimization 21 (2001): the optimality criteria update.
- T. E. Bruns, D. A. Tortorelli, Topology optimization of non-linear elastic structures and
  compliant mechanisms, CMAME 190 (2001); B. Bourdin, Filters in topology optimization, IJNME 50
  (2001): the density filter.
- F. Wang, B. S. Lazarov, O. Sigmund, On projection methods, convergence and robust formulations
  in topology optimization, SMO 43 (2011): the projection.
- A. Doi, A. Koide, An efficient method of triangulating equi-valued surfaces by using
  tetrahedral cells, IEICE Transactions 74 (1991): marching tetrahedra.
- G. Taubin, A signal processing approach to fair surface design, SIGGRAPH (1995): smoothing.
