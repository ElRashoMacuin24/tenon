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
- **Symmetry**: up to three planes at right angles (the origin planes, or flat faces) the shape
  is to be the same on both sides of. The part must itself be the same on both sides of each;
  the supports and loads should be too.
- **Optimize Shape**: how much to keep (a share of the mass taken away, 5 to 95%, or a mass in
  grams), the smallest member (Auto: three elements), the resolution (Coarse, Medium, Fine: about
  12,000, 30,000 or 80,000 nodes), and Sharpen (on: the design is pushed toward solid and empty,
  in more rounds). OK keeps the settings with the study and works the shape out. The view says
  which round it is on; **Esc** stops it and keeps the shape found so far.

The shape is drawn over the part, which is drawn as its edges. The status bar says its mass, how
it ended, and how much of the part's stiffness it keeps. A slider at the foot of the view shows
the surface for another share of the part: that shows where material ranks, not what an
optimization for that share would make. The browser's study has Keep Faces, Symmetry and Shape
rows; the Shape row shows or hides the shape, and its menu works it out again with the same
settings. **Export Shape** writes the shape shown as STL or 3MF.

Settings, kept faces and planes are kept with the study in the part file. The shape itself is
not: it goes when you leave the environment or change the part. Export it to keep it.

For agents and scripts: `study.keep`, `study.shape` and `study.optimize` ([commands.md](commands.md)).

## What it can and cannot do

- Stiffness only, for one set of loads. There is no stress limit: a shape can yield. Check the
  part you make from it with Simulate.
- Gravity and a body's turning are refused (their loads depend on the material kept): suppress
  them to optimize.
- One solid of one material, linear and static, as for a stress check.
- The smallest member is approximate: it is what the filter allows, not a hard limit.
- Coarse resolutions give blocky shapes; the surface is faceted at the element size.
- A run takes minutes: about a minute for 7,000 elements and 90 rounds on the machine the tests
  run on (2 to 3 times longer on a laptop). The geometry is busy while it runs.

## The method

Each element e of the part's mesh (second-order tetrahedra, as for a stress check) has a density
x_e between X_MIN = 0.001 and 1, and a Young's modulus

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
  h = 0.5, with b raised through 1, 2, 4, 8 and 16, at most 20 rounds at each. Without
  sharpening, x- = x~.
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
- **Same answer twice.** Sums run in a fixed order and the mesher runs on one thread: the same
  study gives the same shape bit for bit.

## The shape

The densities are averaged onto the corner nodes (by volume; 1 where an element must stay). The
level t is found whose inside volume, worked out exactly for a field linear on each tetrahedron,
is the share asked; the surface where the density is t is cut out of each tetrahedron (marching
tetrahedra, the crossings on shared sides worked out once so the surface is closed), the part's
own boundary capped where it is inside, pieces cut off from all that must stay dropped (and
said), and a load that the kept material does not join to a support warned of. The surface is
smoothed by Taubin's two steps (10 pairs), which keeps its volume where plain smoothing shrinks
it; vertices on the part's boundary stay.

Then the shape is solved once: the densest elements, what must stay first, up to the share kept,
solid, and the rest empty. The ratio of the solid part's work to the shape's is the stiffness it
keeps.

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
