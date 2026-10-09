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
| L5 | `drawing` | drawings: sheets, views through hidden-line removal (projected, section, detail, isometric), associative dimensions, centrelines, hole tables, balloons, parts lists, title blocks, PDF/SVG/DXF, `drw.*` commands | M4 |
| L6 | `io` | `.tenon` part, `.tenonasm` assembly and `.tenondrw` drawing files, file and export commands, STL; STEP via the kernel | M4 (3MF: later) |
| L7 | `render` | camera, picking, software rasteriser, wgpu viewport renderer, drawing sheets to images (no UI toolkit) | M4 |
| L8 | `ui` | egui workbench: ribbon, browser, sketcher, feature panels, properties panel, viewport, dialogs; the assembly environment and editing parts in place; the drawing environment | M4 |
| exempt | `apps/tenon` | desktop binary (eframe + wgpu, OCCT, native file dialogs) | M1 |
| exempt | `apps/tenon-cli` | headless CLI: command scripts, MCP server, PNG render, STEP tools | M1 |
| exempt | `xtask` | workspace tooling and the CI gate | M0 |

Rules:

- A crate may depend only on strictly lower layers (dev-dependencies included).
- Kernel backends (`kernel-occt`) may be used by library crates only as **dev-dependencies**;
  library code talks to `dyn Kernel`. Apps choose the backend. A Rust-native kernel (an experimental track, DEC-028) slots in
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

## Drawings (M4)

`tenon_drawing` works on drawings in memory; `tenon_io::drw` reads and writes the files (DEC-026).

- **Models.** A `DrwSession` holds the drawing (with undo) and one part `Session` or assembly
  `AsmSession` per model file its views show, so a model can be edited from the drawing and saved
  with it.
- **Views.** `views::evaluate` builds every model through the kernel and projects it into each
  view's frame with hidden-line removal (`Kernel::project_edges`, OCCT's `HLRBRep_Algo`).
  Projected views take their frame from their parent (third-angle for ANSI, first-angle for ISO,
  DEC-027). A section cuts the model with a half-space box first and hatches the cut faces; a
  detail clips its parent's curves to a circle. The result (`Evaluation`) is keyed by everything
  the views depend on (`DrwSession::eval_key`): moving a view or adding a dimension does not
  recompute anything.
- **Sheets.** `annotate::build` turns a sheet into `Graphics`: polylines with a pen (visible,
  hidden, thin, centre, cutting, border, hatch), filled arrowheads and text, in millimetres of
  paper. Dimensions keep persistent edge references and are measured again every time, which is
  what makes them follow model changes. The same `Graphics` feeds the screen, SVG, PDF, DXF (a
  layer per pen) and PNG (`tenon_render::sheet`).
- **Picking.** A click on the sheet finds the model edge drawn nearest it (`annotate::edge_at`,
  the edge nearest the eye where several are drawn on top of each other), so dimensions and
  balloons are placed by clicking, in the UI and in scripts (`drw.pick` with `at`).
- **Suggestions.** `suggest` reads a view's model edges (lines spanning the view, circles and
  arcs seen end-on) and proposes ordinary dimensions, minus those the view has; the user keeps
  the ones wanted.
- **Output.** `clean` turns strokes into true arcs and circles and drops repeats before PDF, SVG
  and DXF are written; `export::check_pdf` and `check_dxf` read the files back for the tests.
- **Models changing on disk.** Each model's files are fingerprinted when read or saved;
  `tenon_io::drw::reload_changed` reads again those saved elsewhere (the app checks about once a
  second), keeping models with unsaved changes made from the drawing.

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
assembly has its own slot. An edit cancels only its own slot's regeneration. A preview (a value
being dragged or typed into a feature panel) never cancels: it waits, and only the latest waiting
one runs, so a drag shows results all along instead of none until it stops
(`a_stream_of_previews_shows_results_while_it_lasts`). Jobs (interference, assembly STEP, a
drawing's views) run on the worker against the slots' latest solids.

In an assembly, the viewport shows one `Scene` holding every visible component's bodies moved to
where the component is, so picking and highlighting work unchanged. The GPU uploads bodies by key,
so dragging one component re-sends only its triangles (`tenon_model::worker`). STEP export and
measuring also run on the worker. In a drawing, the central panel is the sheet; its views are
computed by one worker job at a time, and the sheet keeps showing the last ones meanwhile. Tests
and tools can run the same workbench synchronously with a kernel on the calling thread
(`Workbench::headless`).

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
- With the experimental native kernel (DEC-028), a differential harness runs randomised operations through two backends and compares
  volume, area, topology counts and validity.
- `cargo xtask ci` is the gate locally and in GitHub Actions.

## Inheritance from CADCraft

Tenon forked CADCraft at `14e143b`. Kept: `geom` (2D core), `dxf`, `xtask` (ci, layers, assets,
wasm, stats).

- **Ported in M1:** the Gauss-Newton constraint solver (`sketch/src/lm.rs`, attributed in
  NOTICE).
- **Written for Tenon instead of ported (DEC-014):** the command registry and the MCP server.
- **Taken in M4:** the single-stroke drafting font (`drawing/src/stroke.rs`, attributed in
  NOTICE). The rest of the drawing stack (views, dimensions, PDF, SVG and DXF output) was written
  for Tenon; the DXF output uses the inherited `dxf` writer.
- **Not taken:** DWG, the AutoCAD command set, CADCraft's drafting entities (blocks, hatch
  objects).
