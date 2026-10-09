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
| L2 | `sketch` | 2D sketch entities, constraints, solver behind `SketchSolver`, sketch tools, profile regions | M1 |
| L3 | `model` | feature tree, regeneration (incremental), face and edge naming, parameters and equations, measuring, command registry and undo, regeneration worker | M2 |
| L4 | `assembly` | assemblies: components, constraints and joints, the rigid-body solver, degrees of freedom, parts list, interference, exploded view, `asm.*` commands | M3 |
| L5 | `drawing` | views, dimensions, title blocks, PDF/SVG/DXF | placeholder (M4) |
| L6 | `io` | `.tenon` part and `.tenonasm` assembly files, file and export commands, STL; STEP via the kernel | M3 (3MF, DXF: later) |
| L7 | `render` | camera, picking, software rasteriser, wgpu viewport renderer (no UI toolkit) | M1 |
| L8 | `ui` | egui workbench: ribbon, browser, sketcher, feature panels, properties panel, viewport, dialogs; the assembly environment and editing parts in place | M3 |
| exempt | `apps/tenon` | desktop binary (eframe + wgpu, OCCT, native file dialogs) | M1 |
| exempt | `apps/tenon-cli` | headless CLI: command scripts, MCP server, PNG render, STEP tools | M1 |
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
- **Threading.** `Kernel: Send`. A kernel is owned by one thread at a time (in the app, the
  regeneration worker); OCCT's STEP translator, which has global state, is serialised by a lock.
- **Quiet.** OCCT's default message printers are removed so nothing writes to stdout.

## Commands

Every change to a part is a named command (`area.verb`, e.g. `model.extrude`) with JSON
parameters and a JSON result. The list is generated in [commands.md](commands.md).

- **Registry.** `tenon_model::cmd` holds the document and geometry commands (`CommandSpec`: id,
  label, parameter help, whether it adds an undo step, and a `Doc` or `Geo` function). The file and
  export commands live in `tenon_io::cmd`, and `tenon_io::cmd::run` dispatches over both.
- **Session.** `Session` owns the document with snapshot undo (200 steps). A command either
  succeeds as one undo step or leaves the document unchanged. Regeneration is cached per revision.
- **Front ends.** The ribbon (`ui::commands`) maps buttons to registry commands or interactive
  tools; commands whose milestone has not arrived say so. Scripts (`tenon-cli run`,
  [scripts.md](scripts.md)) and the MCP server ([mcp.md](mcp.md)) call the registry directly. The
  headless host adds `render.png`.

## Regeneration (M2)

`tenon_model::regen` rebuilds the part feature by feature through `dyn Kernel`:

- **Equations first.** `Session::edit` runs `Document::sync_parameters` after every change, in the
  same undo step: values are named (`d0`, `d1`, ...), equations evaluated in dependency order and
  written into the sketch constraints and feature definitions. Regeneration only ever sees plain
  numbers (DEC-023).
- **Tool solids.** Features that a pattern or mirror copies keep their solid before it was
  combined with the part (`Regen::tools`); a pattern transforms copies of it and combines each
  copy the same way (DEC-022).
- **Work geometry.** Work planes, axes and points are computed in history order into
  `Regen::work`; sketches, mirrors, patterns and revolves look them up there.
- **End of Part.** Features at and below the marker get `RolledBack` and are not computed.
- **Checkpoints.** `RegenCache` keeps the state just before the first feature the last edit
  changed: second handles to its shapes (`Kernel::duplicate`, no geometry copied). The next run
  starts there if the prefix hash of the history before it is unchanged. A prefix hash covers each
  feature's definition, whether it is rolled back and whether a pattern copies it. Editing the
  last of 42 features takes 9.3 ms instead of 166 ms (release build, `regeneration_speed`). Rebuild All starts from scratch.
- **Measuring** needs the exact B-rep, so it runs where the kernel is: the worker answers
  `Measure` requests on the part it last regenerated (`tenon_model::measure`).

## Assemblies (M3)

`tenon_assembly` works on documents in memory; `tenon_io::asm` reads and writes the files (DEC-024).

- **Parts.** An `AsmSession` holds the assembly (with undo) and one part `Session` per part file.
  Each part session has its own undo, so a part edited in place keeps its history.
- **Geometry.** Each part comes with its regenerated `Scene`. Relationships find their faces and
  edges in it by persistent name (`tenon_assembly::geometry`), so solving and counting degrees
  of freedom need no kernel and run on the UI thread.
- **Solver.** `tenon_assembly::solve` (DEC-025) gives six unknowns per free component and uses
  the sketch solver's damped minimum-norm Gauss-Newton.
- **Kernel work.** Interference and assembly STEP use the kernel: placed copies of each
  component's solids are intersected or exported.

## The desktop app's data flow

```
 egui frame (UI thread)                          worker thread (owns the kernel)
 ──────────────────────                          ───────────────────────────────
 input → Workbench → registry command → Session (document, undo)
                │  document changed (or panel preview)
                └──── Document snapshot ───────────▶  regenerate → tessellate, measure
                                                                │
 viewport ◀─ Scene (meshes, face names, mass, status) ◀─────────┘
```

The UI thread never calls the kernel in the app. It sends a snapshot whenever the shown document
changes, including live Extrude/Revolve previews, and keeps drawing the last `Scene` until the
new one arrives.

The worker keeps one slot per document: slot 0 is the part being edited, and each part of an open
assembly has its own slot. A new request cancels only its own slot's regeneration. Jobs (interference,
assembly STEP) run on the worker against the slots' latest solids.

In an assembly, the viewport shows one `Scene` holding every visible component's bodies moved to
where the component is, so picking and highlighting work unchanged. The GPU uploads bodies by key,
so dragging one component re-sends only its triangles. A newer snapshot cancels the one in progress (`tenon_model::worker`). STEP
export and measuring also run on the worker. Tests and tools can run the same workbench synchronously with a
kernel on the calling thread (`Workbench::headless`).

The viewport renders through wgpu into an offscreen MSAA texture that egui shows as an image. It
falls back to the software rasteriser when the app has no wgpu render state. Picking casts rays
against the same meshes the viewport shows. Face names come with the `Scene`, so selecting a
face yields a persistent `FaceRef` without asking the kernel.

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
wasm, stats).

- **Ported in M1:** the Gauss-Newton constraint solver (`sketch/src/lm.rs`, attributed in
  NOTICE).
- **Written for Tenon instead of ported (DEC-014):** the command registry and the MCP server.
- **Still to come:** the drafting stack (fonts, dimensions, PDF plot) for drawings (M4).
- **Not taken:** DWG, the AutoCAD command set, hatch/blocks/layers.
