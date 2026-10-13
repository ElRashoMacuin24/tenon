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
   smaller ones.
3. Tenon reads the mesh and tells which face of the part each of its surfaces is, by where
   it lies (a STEP file keeps neither the order of faces nor their names).
4. The study is put onto the mesh: every node of a held face gets the directions it may not
   move along (all three for Fixed, square to the face for Frictionless, across, along or
   round the axis for a Pin); every load becomes forces on nodes, worked out over each
   curved triangle.
5. If, held as it is, the part could still slide or turn as a whole, that is said before the
   solver is asked for an answer that does not exist.
6. CalculiX solves it. Tenon reads how far each node moved and the stress there.

What comes back (`study.solve`): the largest von Mises stress and where it is, the largest
displacement and where, the material's yield strength, and the **safety factor**: yield
strength over the largest stress. Below 1 the part yields somewhere.

Reading a result:

- A stress that is far higher at one sharp inside corner, or where a face is held fast, than
  anywhere else, and that grows as the elements get smaller, is the mathematics of a perfectly
  sharp corner, not the part. Look at the stress a little away from it, or give the corner
  the fillet the real part has.
- Halve the element size and solve again. If the answer barely changes, the mesh was fine
  enough.
- A support is rigid. A real fixture gives a little, which lowers the stress beside it.

## How it is checked

`cargo xtask ci` (and so CI, on Windows, Linux and macOS) works four studies out through the
real programs and holds them against answers from the textbooks
(`crates/io/tests/m11_solve.rs`):

| Case | Checked against | Within |
|---|---|---|
| A bar pulled along its length | Hooke's law: stress, stretch and thinning | 0.1 % |
| A cantilever with a load on its end | beam theory with shear (Timoshenko): the drop of its end | 1.5 % |
| A thick tube under pressure inside | Lamé: stress at the bore, growth of the bore | 2 % |
| A plate with a hole, pulled | Heywood's formula for the stress beside the hole | 5 % |

Those are tolerances the cases are held to, not a promise about your part.

## The files

`study.solve` with `keep` set to a folder leaves everything there: the part as the mesher got
it (`part.step`), the mesh (`part.msh`), the solver's input (`study.inp`) and results
(`study.frd`), and what each program said (`mesher.log`, `solver.log`). The input can be run
again, or changed, with CalculiX on its own.
