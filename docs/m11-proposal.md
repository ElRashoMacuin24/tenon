# M11 stress checks: the mesher and the solver

For the owner to decide (docs/plan.md: third-party solvers are asked about first, and M11's
licence analysis comes before anything is bundled or recommended). Written 2026-10-12. Nothing
below has been installed, bundled or recommended yet.

## What is needed

A stress check divides the part into small tetrahedra (a **mesher**), works out how each
moves under the loads (a **solver**), and shows the result on the part. Tenon has neither a
mesher nor a solver. The plan names Gmsh or Netgen for the first and CalculiX for the second,
run as separate programs.

## The candidates

| | What | Licence | Where it comes from | Fits? |
|---|---|---|---|---|
| **Gmsh** 4.15 | Mesher. Reads STEP, writes tetrahedra, from the command line | GPL 2 or later, with an exception that lets it be combined with Netgen, METIS, OpenCASCADE and ParaView | conda-forge, for Windows, Linux, macOS (Intel and Apple); also its authors' own downloads | Yes |
| **Netgen** 6 | Mesher | LGPL 2.1 | Not on conda-forge (other channels only); no documented command that turns a STEP file into tetrahedra without its Python | Hard to ship and to drive |
| **CalculiX** ccx 2.23 | Solver for linear statics and much more; in use since 1998 | GPL 2 (its site's notice says 2 or later) | conda-forge, for Windows, Linux, macOS (Intel and Apple) | Yes |
| TetGen | Mesher | AGPL 3, or paid | | No: AGPL |
| fTetWild | Mesher, very forgiving of bad input | MPL 2 | No packages | Later, perhaps: it does not keep sharp edges exactly |
| Code_Aster, Elmer | Solvers | GPL | Large installs | No: far more than is needed |
| A solver of our own, in Rust | Solver | Tenon's | To be written | See option B |

## What "a separate program" means for licences

Tenon is MIT or Apache-2.0. Gmsh and CalculiX are GPL. The GPL's conditions fall on works that
contain GPL code or are combined with it into one program.

- **Running them is not combining with them.** Tenon would start `gmsh` and `ccx` as programs
  of their own, hand each a file and read the file it writes. The Free Software Foundation's
  own FAQ on the GPL says that programs which talk through command lines, files, pipes or
  sockets are normally separate programs, and that being shipped side by side is "mere
  aggregation", which leaves the other program's licence alone. It also says the line is in
  the end for courts, and that it has not been tested there. This is the common reading and
  the one the plan was written on; it is not legal advice.
- **What they write is not theirs.** A mesh or a result file is the output of a program, and
  the GPL does not reach output (unless it copies the program into it, which these do not).
- **Nothing of theirs enters Tenon.** No source is read, no library is linked, no header is
  included. The file formats (Gmsh's `.msh`, CalculiX's `.inp` and `.frd`) are documented and
  are written and read by code of our own.
- **Duties begin with handing them out.** Whoever *distributes* Gmsh or CalculiX binaries
  must pass on their licence and offer their source. Telling people where to get them
  ("recommending") carries no duty. Putting them in an installer of Tenon would: their
  licences and a source offer beside them, in `NOTICE`. Tenon has no installer yet.

## One thing found on the way

conda-forge's Gmsh is built against OpenCASCADE 7.9.3 and cannot be in the same environment
as Tenon's OpenCASCADE 8.0. So it cannot simply be added to `pixi.toml` next to what is
there. It can be a second environment in the same file (pixi has these), used by developers
and CI; or the user's own install, which Tenon finds. Either way Tenon and Gmsh would each
use their own OpenCASCADE and meet only through a STEP file, which is what STEP is for.

## The options

**A. Gmsh and CalculiX as separate programs (the plan).**

- Tenon looks for `gmsh` and `ccx`: where its settings say, then on the path. Without them
  the Stress Analysis environment says what is missing and where to get it, and everything
  else works as before.
- Developers and CI get them from conda-forge through a second pixi environment. The
  textbook checks in CI run through the real programs on the three platforms.
- Nothing is bundled now. When Tenon gets installers, bundling is decided then, with the
  licences and source offers it needs.
- For: both are mature and widely used; the numbers are CalculiX's, which people trust;
  least code of our own to get wrong.
- Against: two programs to find on the user's machine; a first-time step for the user until
  they are bundled; two more things that can change under us.

**B. Gmsh as a separate program, and a solver of our own.**

- A linear static solver for ten-node tetrahedra is a few hundred lines of well-known
  mathematics, plus solving a large sparse system, which is the hard part to do fast.
- For: one outside program instead of two; no Fortran runtime; results without files.
- Against: weeks of work that buys nothing a user sees; slower on large meshes; and the
  numbers would be ours to stand behind, where Tenon's whole case is that its numbers can be
  trusted. A quick stress check that is wrong is worse than none.

**C. Netgen in place of Gmsh.** LGPL in place of GPL, which changes nothing while it is a
separate program, and it is harder to get onto three platforms and to drive without its
Python.

**D. Leave M11.** The studies described below stay in the part and do no harm.

## Recommendation

**A**, with the solver behind a narrow interface of our own (a mesh and loads in, movements
and stresses out), so that B can replace CalculiX later without touching the rest.

The first thing built would be a trial on the three platforms: a bar meshed by Gmsh, solved
by CalculiX, compared with beam theory. If either program cannot be had or driven on one of
them, that comes back to the owner before anything else is built on it.

## What is asked

1. May Tenon use **Gmsh and CalculiX as separate programs** for stress checks (option A)?
2. May they be added to `pixi.toml` as a **second environment**, so that developers and CI
   download them from conda-forge? (This is the "recommended" of the plan: the repository
   would name them.)
3. May the documentation tell users to install them to use stress checks?
4. Bundling them in installers: asked again when there are installers.

## What is done already, whichever way this goes

- **Studies in the part** (DEC-093): where a part is held (fixed supports on faces) and what
  pushes on it (forces and pressures on faces), kept with the part, edited by commands with
  undo (`study.new`, `study.fix`, `study.force`, `study.pressure` and the rest), written in
  the part's file and reported by `tenon-cli diff`.
- **Materials say how they answer to a load**: Young's modulus, Poisson's ratio and yield
  strength for the library's metals and plastics (typical values; woods have none, their
  strength follows the grain), and for a material of one's own.
- **The Stress Analysis environment, as far as setting a study up** (DEC-095, added
  2026-10-13): Environments > Stress Analysis; Fixed, Force and Pressure by clicks on the
  part's faces; the study in the browser; supports and loads drawn on the part. Simulate is
  on its ribbon as a command that has not arrived.

## What follows the answer

- The pipeline: the part as STEP, to the mesher; its mesh read back and its boundary matched
  to the part's faces by where it lies (not by the order of faces, which a STEP file does not
  keep); the solver's input written; its results read.
- In the Stress Analysis environment: the mesh shown, Simulate, and von Mises stress,
  displacement and safety factor shown in colour on the part with a scale.
- Checks against cases with known answers, in CI: a cantilever's deflection, the stress
  beside a hole in a plate, a thick tube under pressure.
- Every result labelled as an engineering aid, not a certified analysis.

## Sources

- Gmsh licence: <https://gmsh.info/LICENSE.txt>
- CalculiX: <http://www.dhondt.de/>, <https://calculix.de>
- conda-forge packages: <https://anaconda.org/conda-forge/gmsh>, <https://anaconda.org/conda-forge/calculix>
- Netgen's licence as packaged: <https://blueprints.launchpad.net/ubuntu/jammy/+source/netgen/+copyright>
- The GPL FAQ on aggregation and on output: <https://www.gnu.org/licenses/gpl-faq.html>
  (the page could not be reached from this machine on the day; it was read through
  quotations of those two answers elsewhere, and should be read again before anything is
  bundled)
