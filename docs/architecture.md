# Architecture

Tenon is a Cargo workspace with strict layering. Lower layers never know about higher ones; the
rules are enforced by `cargo xtask layers` (table in `xtask/src/layers.rs`) and
`cargo xtask unsafe-audit`.

## Crates and layers

| Layer | Crate | Responsibility | Status |
|---|---|---|---|
| L0 | `geom` | f64 2D/3D math, central tolerances (`tol`), robust predicates, frames/axes/boxes | M0 |
| L0 | `dxf` | standalone DXF tag reader/writer (no workspace deps) | inherited |
| L1 | `kernel` | backend-neutral `Kernel` trait, handles, operation history, query types, validation | M0 |
| L2 (backend) | `kernel-occt` | OpenCASCADE 8 backend: cxx bridge + C++ shim; the only crate with `unsafe` | M0 |
| L2 | `sketch` | 2D sketch entities, constraint solver behind `SketchSolver` | placeholder (M1) |
| L3 | `model` | feature tree, parameters/expressions, regeneration, persistent naming, commands | placeholder (M1/M2) |
| L4 | `assembly` | components, joints, DOF, BOM | placeholder (M3) |
| L5 | `drawing` | views, dimensions, title blocks, PDF/SVG/DXF | placeholder (M4) |
| L6 | `io` | STL (done), 3MF, DXF mapping, native project format; STEP via the kernel | M0: STL |
| L7 | `render` | wgpu viewport, picking, edges, orientation cube (no UI toolkit) | placeholder (M1) |
| L8 | `ui` | egui front end: ribbon, browser, viewport chrome, dialogs | M0: layout shell |
| exempt | `apps/tenon` | desktop binary (eframe + wgpu) | M0 |
| exempt | `apps/tenon-cli` | headless CLI (MCP server from M1) | M0 |
| exempt | `xtask` | workspace tooling and the CI gate | M0 |

Rules:

- A crate may depend only on strictly lower layers (dev-dependencies included).
- Kernel backends (`kernel-occt`) may be used by library crates only as **dev-dependencies**;
  library code talks to `dyn Kernel`. Apps choose the backend. A Rust-native kernel (M6) slots in
  as another backend.
- Native-binding crates (`cxx`, `cxx-build`, `cc`, `bindgen`, `opencascade*`, `occt*`) only in a
  backend.
- `wgpu` from `render` (L7) up; egui, eframe, winit, rfd only in `ui` (L8) and apps.
- Every crate root except `kernel-occt` has `#![forbid(unsafe_code)]`; workspace lints forbid
  `unsafe` and deny `unwrap`/`expect`/`panic!` in non-test code.
- The wasm check (`cargo xtask wasm`) covers L0-L6. Apps and the OCCT backend are native only
  until a wasm-capable kernel exists.

## The kernel boundary

```
model / io / cli  ──>  dyn Kernel  ──>  OcctKernel (Rust arena of shapes)
                                           │  cxx bridge (src/ffi.rs)
                                           ▼
                                      C++ shim (shim/tenon_occt.*)  ──>  OpenCASCADE 8 (shared libs)
```

- **Handles.** Shapes live in the backend's generational arena. `ShapeHandle`, `FaceId`,
  `EdgeId` and `VertexId` are valid only inside one kernel and are never persisted.
- **Coarse FFI.** One shim function per kernel operation. Each runs inside `guarded()`, which turns
  every OCCT `Standard_Failure` and C++ exception into `std::runtime_error`; cxx returns it as
  `Err`, the backend maps it to `KernelError`. Inputs are validated in Rust (`kernel::check`)
  before native code runs.
- **History.** Every modelling op returns `Op { shape, history }`: the image of every input face
  and edge, sub-shapes generated from inputs or tagged profile curves, and primitive face roles.
  This is what persistent naming builds on (docs/persistent-naming.md).
- **Threading.** `Kernel: Send`. A kernel is owned by one thread at a time (the regeneration
  worker from M1); OCCT's STEP translator, which has global state, is serialised by a lock.
- **Quiet.** OCCT's default message printers are removed so nothing writes to stdout.

## Commands

Every user action is a named command (`area.verb`, e.g. `model.extrude`). In M0 the ribbon's
command table lives in `ui::commands` with the milestone that implements each command. From M1 the
command registry moves into `model`, following CADCraft's `CommandSpec` pattern (id, label,
parameters, `enabled`, JSON `run`, optional interactive prompts), and the same registry serves the
UI, the CLI, scripts and the MCP server.

## Tolerances

All tolerances come from `tenon_geom::tol`: `LINEAR` (1e-7 mm) and `ANGULAR` (1e-12 rad) match
OCCT's confusion and angular precision; `MIN_SIZE`/`MAX_SIZE` bound modelling input;
`MEASURE_REL` (1e-6) compares measurements across routes (STEP round trip, backends);
`MESH_LINEAR`/`MESH_ANGULAR` are the display tessellation defaults. Orientation and in-circle
decisions use the exact predicates in `tenon_geom::predicates`. Some inherited 2D code in `geom`
still has local literal epsilons; they move to `tol` as that code is touched.

## Testing

- Unit tests next to the code; integration tests per crate in `tests/`.
- Kernel tests compare against analytic values (volumes, areas, inertia), check invariants
  (inclusion-exclusion on random booleans, closed outward meshes, STEP round trip) and feed
  hostile input (NaN, infinities, zero sizes, stale handles, garbage files).
- From M6, a differential harness runs randomised operations through two backends and compares
  volume, area, topology counts and validity.
- `cargo xtask ci` is the gate locally and in GitHub Actions.

## Inheritance from CADCraft

Tenon forked CADCraft at `14e143b`. Kept: `geom` (2D core), `dxf`, `xtask` (ci, layers, assets,
wasm, stats). Planned ports from that commit: the constraint solver (M1), the command registry and
undo model (M1), the MCP server and control channel (M1), and the drafting stack (fonts,
dimensions, PDF plot) for drawings (M4). Not taken: DWG, AutoCAD command set, hatch/blocks/layers.
