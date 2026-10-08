# Decision log

Newest last. Decisions that are expensive to reverse are asked first (AGENTS.md); the rest are
made and recorded here.

## DEC-001 OpenCASCADE binding: own cxx shim over OCCT 8, dynamically linked (2026-10-07, approved: plan D1)

Tenon binds OCCT through its own small C++ shim and a cxx bridge in `kernel-occt`
(MIT OR Apache-2.0), against OCCT 8.0.x shared libraries from conda-forge via pixi.

Not opencascade-rs as the base:

- it is LGPL-2.1, so extending or vendoring it would make our FFI crate LGPL;
- its build script requires OCCT 7.x, and its built-in mode compiles OCCT 7.8 statically;
- it binds OCCT class by class, and we need per-operation exception boundaries and history maps.

Confirmed in M0: OCCT 8's `Standard_Failure` derives from `std::exception`, and stream-based STEP
I/O exists.

## DEC-002 Selective fork of CADCraft (2026-10-07, approved: plan D2)

Fork with full history, then keep `geom`, `dxf` and `xtask`. Port the solver, command registry and
MCP server when their milestone starts; take the drafting crates for M4. Remove the AutoCAD-parity
surface and the ArtCraft brand.

CADCraft's own policy forbids LGPL code, so kernel work is never upstreamable. Upstream stays
reachable as the `upstream` git remote.

## DEC-003 Name: Tenon (2026-10-07, approved: plan D6)

"Forgeline" collided with a wheel brand. Tenon, the joint that locks parts together, is short and
has no CAD product with that name (search on 2026-10-07). Crates are `tenon-*`.

## DEC-004 Native file format container: zip + versioned JSON (2026-10-07, approved: plan D5)

A `.tenon` zip with `project.json` (versioned schema), an optional cached B-rep, and a thumbnail.
The detailed schema is written in M1 (docs/file-format.md). Changing the container later is
expensive, so any change goes back to the project owner.

## DEC-005 Sketch solver: port CADCraft's solver behind `SketchSolver` (2026-10-07)

CADCraft's damped Gauss-Newton solver is MIT/Apache, pure Rust and wasm-safe. It already does
conflict probing and redundancy detection by Jacobian rank. The alternatives have licence
problems:

- SolveSpace is GPL-3, which is incompatible with distributing Tenon under MIT/Apache.
- FreeCAD's planegcs is LGPL C++ and would need a second FFI crate.

The port happens in M1, adding DOF reporting, drag and midpoint.

## DEC-006 pixi for native dependencies (2026-10-07)

`pixi.toml` and `pixi.lock` pin OCCT 8.0.1 (the conda-forge `novtk` build, without the VTK
visualisation toolkit) for win-64, linux-64, osx-64 and osx-arm64. This gives one recipe for
every OS and for CI, with no admin rights and no OCCT compile.

## DEC-007 `Kernel::tessellate` takes `&mut self` (2026-10-07)

The plan draft had `&self`. Backends may cache triangulations (OCCT stores them on the shape), so
the trait is honest about mutation.

## DEC-008 Single-solid boolean results are returned as the solid (2026-10-07)

OCCT booleans return a compound even for one solid. The shim unwraps a compound holding exactly one
solid. Sub-shape enumerations, and therefore history, are unchanged.

## DEC-009 Exchange calls are serialised; OCCT messages are silenced (2026-10-07)

- STEP import/export run under a process-wide mutex, because the OCCT translator has global state.
- OCCT's default message printers are cleared, because they wrote STEP statistics to stdout and
  corrupted `tenon-cli --json`. There is a regression test for this.

## DEC-010 Smart App Control workarounds (2026-10-07)

On the Windows development machine, Smart App Control (Enforce mode) blocked:

- the empty unit-test executable of `kernel-occt`, which is now disabled because all its tests
  are integration tests;
- the `zerocopy-derive` proc-macro DLL, which is now re-hashed by a per-package profile override.

These are workarounds. The durable fix, turning Smart App Control off on development machines,
is the owner's decision (docs/setup.md).

Update (2026-10-07): the owner turned Smart App Control off, and the `zerocopy-derive` override was
removed. `kernel-occt` keeps `[lib] test = false` because it has no unit tests.

## DEC-011 wasm check scope: L0-L6 (2026-10-07)

`kernel-occt` (C++) cannot target `wasm32-unknown-unknown`. The backend-neutral crates are checked
for wasm now. The renderer, UI and apps are checked once a wasm-capable kernel exists (M6).

## DEC-012 Vendor-neutral vocabulary (2026-10-07)

Generic CAD verbs are used as they are (Extrude, Revolve, Fillet). Vendor-specific names are not:
the product says "orientation cube" (not ViewCube), "End of history" (not End of Part), "radial
menu" (not marking menu), "New Sketch", "Mass" and "Parameters".

## DEC-013 Integration tests may `unwrap` (2026-10-07)

`clippy.toml`'s test exemptions do not cover helper functions in `tests/*.rs`. Those files carry
`#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]` at the top; production code
stays under the deny lints.
