# Stress checks

A stress check answers one question about a part: held like this and loaded like that, does it
hold, and by how much does it move? Tenon sets the question up on the part (the Stress Analysis
environment, or the `study.*` commands) and has it worked out by two programs made for exactly
that.

**A quick check is an engineering aid, not a certified analysis.** It is linear and static, of
one solid of one material that is the same in every direction. It does not know about
buckling, fatigue, impact, heat, or what happens after the material yields. Use it to see
where a part is weak and whether a change helps; do not use it as the proof that something
is safe.

## The two programs

| | What it does | Licence | From |
|---|---|---|---|
| **Gmsh** 4 | divides the part into small tetrahedra | GPL 2 or later | <https://gmsh.info> |
| **CalculiX** (`ccx`) 2 | solves them | GPL 2 | <http://www.dhondt.de> |

They are not part of Tenon and are not shipped with it. Tenon starts them as programs, hands
each a file and reads the file it writes; nothing of theirs is in Tenon, and Tenon's own
licence (MIT or Apache-2.0) is as it was. Their authors' licences are theirs: if you pass
either program on to someone else, their terms apply to that.

### Getting them

Working from a checkout of Tenon:

```sh
pixi install -e fea        # once: downloads both into .pixi/envs/fea
```

Otherwise install them any way you like (your system's packages, conda-forge, or the
authors' own downloads) so that `gmsh` and `ccx` are on your search path.

Tenon looks, in this order:

1. where the environment variables `TENON_GMSH` and `TENON_CCX` say (the full path of each
   program);
2. in a checkout's `.pixi/envs/fea`;
3. on the search path.

When one is missing, the study says which, and everything else in Tenon works as before.

## What is worked out

1. The part is rebuilt and written as a STEP file.
2. Gmsh divides it into ten-node tetrahedra. Their size is the study's (Mesh Settings,
   `study.mesh_size`), or a twentieth of the part's size; round holes and fillets get
   smaller ones. It runs on one thread, so that the same part gives the same mesh, and the
   same answer, every time.
3. Tenon reads the mesh and tells which face of the part each of its surfaces is, and which
   edge each of its curves lies along, by where they are (a STEP file keeps neither the
   order of faces and edges nor their names).
4. The study is put onto the mesh: every node of a held face or edge gets the directions it
   may not move along (all three for Fixed, square to the face for Frictionless, across,
   along or round the axis for a Pin); every load becomes forces on nodes, worked out over
   each curved triangle, or along each curved piece of an edge. The part's weight and its
   turning are given to the solver as they are, with the material's density.
5. If, held as it is, the part could still slide or turn as a whole, that is said before the
   solver is asked for an answer that does not exist.
6. CalculiX solves it. Tenon reads how far each node moved and the stress there.

What comes back (`study.solve`): the largest von Mises stress and where it is, the largest
displacement and where, the material's yield strength, and the **safety factor**: yield
strength over the largest stress. Below 1 the part yields somewhere.

## Supports and loads

| | On | What it is |
|---|---|---|
| **Fixed** | faces, or edges | held still in every direction |
| **Pin** | round faces | on a pin: held across it, along it, from turning on it, as ticked |
| **Frictionless** | flat or round faces | free to slide along itself; cannot leave or enter what it rests on |
| **Force** | faces, or edges | newtons in all, spread evenly over the faces (or along the edges). Along a direction; on faces it may be square to each face instead |
| **Pressure** | faces | megapascals, square to the faces and into them; negative pulls |
| **Gravity** | the whole part | its own weight, from the material's density: a direction and an acceleration (9810 mm/s² on the Earth). Once in a study |
| **Moment** | faces | newton millimetres in all, about an axis: one you give, or the faces' own (the way a flat face looks, the axis of a round one). Seen from the axis's tip it turns anticlockwise. Put on as forces round the axis that grow with the distance from it and add up to no force at all |
| **Bearing Load** | round faces | the load of a shaft in its bore, in newtons: pressed on the half of the bore the shaft pushes against, most where it pushes squarely and nothing at the sides. Its direction is across the bore |
| **Remote Force** | faces | a force that acts at a point away from the part and is carried by these faces: they get the force, and the turning it has about them |
| **Body** | the whole part | the part turning at so many turns a minute about an axis through a point: every bit of it is flung outward. Once in a study |

A support or a load is on faces or on edges, never on both. Points (vertices) are not
offered: a force on a point is an endless stress there.

A moment and a remote force are spread over their faces; they do not make the faces rigid.
Away from those faces the part answers as it would to any load with that force and that
turning.

Reading a result:

- A stress that is far higher at one sharp inside corner, or where a face is held fast, than
  anywhere else, and that grows as the elements get smaller, is the mathematics of a perfectly
  sharp corner, not the part. Look at the stress a little away from it, or give the corner
  the fillet the real part has.
- Halve the element size and solve again. If the answer barely changes, the mesh was fine
  enough.
- A support is rigid. A real fixture gives a little, which lowers the stress beside it.
- A force along an edge, and a support on one, are on a line with no width: the stress right
  at the line grows without end as the elements get smaller. Use them for how the part
  moves and for the stress away from the edge, not for the stress at it.

## In the app

Environments > Stress Analysis (or 3D Model > Simulation). Create Study; say where the part
is held (Fixed, Pin, Frictionless) and what pushes on it (Force, Pressure, Gravity, Bearing,
Moment, Remote Force, Body) by clicking its faces; Assign gives it a material that says how
strong it is. Then **Simulate**.

Fixed and Force take edges too: whichever is clicked first, a face or an edge, says which it
is on, and Clear (the cross beside the count) lets you choose again. A direction, or the axis
a load turns about, is clicked in the part (the selector, then a straight edge, a face or a
work axis) or chosen from its list. For Body, a round face or an edge clicked as the axis
also gives the point the axis goes through. Where a remote force acts is typed, in the
part's coordinates.

The status bar says what the study comes to. The browser's Results have a row for von Mises
stress, displacement and safety factor: click one to see it in colour on the part, with a
scale and a mark where the most is. The part is drawn bent, large enough to see
(Displacement Display changes that, and the scale says how many times over it is drawn).
Mesh View shows the elements.

Change a load, a face or the part, and the results are out of date: Simulate again.

## How it is checked

`cargo xtask ci` (and so CI, on Windows, Linux and macOS) works these studies out through
the real programs and holds them against answers from the textbooks
(`crates/io/tests/m11_solve.rs`):

| Case | Checked against | Within |
|---|---|---|
| A bar pulled along its length | Hooke's law: stress, stretch and thinning | 0.1 % |
| A cantilever with a load on its end | beam theory with shear (Timoshenko): the drop of its end | 1.5 % |
| A thick tube under pressure inside | Lamé: stress at the bore, growth of the bore | 2 % |
| A plate with a hole, pulled | Heywood's formula for the stress beside the hole | 5 % |
| A round bar twisted by a moment | torsion: the stress at its rim, how far its end turns | 2 % |
| A bar bent by a moment on its end | beam theory: how far its end moves | 1.5 % |
| The same bar, a force acting 50 mm beyond its end (a remote force) | beam theory: the force and its turning together | 1.5 % |
| A shaft pressing in a bore along a bar (a bearing load) | beam theory for a force there | 3 % |
| A rod turning about one end (Body) | the pull at the axis, how far its end moves out | 1.5 % and 1 % |
| A bar standing under its own weight (Gravity) | the stress at its foot, how much shorter it is | 1 % and 0.5 % |
| A cantilever with its load along an edge, and held by the edges of its end | beam theory, a little softer: within 4 % and 8 % above it |  |

Those are tolerances the cases are held to, not a promise about your part.

## The files

`study.solve` with `keep` set to a folder leaves everything there: the part as the mesher got
it (`part.step`), the mesh (`part.msh`), the solver's input (`study.inp`) and results
(`study.frd`), and what each program said (`mesher.log`, `solver.log`). The input can be run
again, or changed, with CalculiX on its own.
