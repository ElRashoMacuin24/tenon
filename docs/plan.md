# Tenon: plan

Status: **approved 2026-10-07** ("go with your recommendations"). Written under the working name
Forgeline; renamed to **Tenon** at approval (D6). Outcomes of D1–D6 are logged in
`docs/decisions.md`. This file is kept as the original plan; `ROADMAP.md` tracks progress.

## 1. Decisions (resolved)

| # | Question | Recommendation (approved) |
|---|---|---|
| D1 | OCCT binding: our own thin C++ shim over OCCT 8 vs. building on opencascade-rs | Own shim (section 5). opencascade-rs is LGPL and locked to OCCT 7.x. |
| D2 | How much of CADCraft to keep | Selective fork (section 2): keep history and attribution, harvest about 6 crates, drop the AutoCAD-clone surface. |
| D3 | Install a toolchain on this PC | Yes. This PC has no Rust, MSVC, CMake or git. About 10 GB, and the Build Tools need an admin (UAC) prompt. |
| D4 | Where CI runs | A GitHub repo under your account with GitHub Actions. Until then, `cargo xtask ci` runs the same gate locally. |
| D5 | Native file format container | Zip (`.fgl`) holding `project.json` (versioned schema), with optional cached B-rep and a thumbnail. Needed by M1, not M0. |
| D6 | Product name | "Forgeline" is also a US wheel brand. Renamed to **Tenon** (the joint that locks parts together). |

## 2. CADCraft: what we reuse vs. replace

I read `ROADMAP.md`, `AGENTS.md`, the workspace `Cargo.toml` and the crate sources at `storytold/cadcraft@main`.

- CADCraft is an AutoCAD-parity 2D drafter: Rust, egui 0.36, edition 2024, MSRV 1.90, about 290 commands. The engine, prompts and snaps all model DXF entities, so its command ids and its README describe it as an AutoCAD reimplementation.
- Its own 3D work (M11) is at 0%.
- **Policy conflict:** its `AGENTS.md` forbids linking or reading LGPL code, OpenCASCADE included. Our fork deliberately departs from that policy, so kernel work can never go back upstream. That's fine, but it's a permanent divergence.

| CADCraft crate | Fate | Becomes |
|---|---|---|
| `geom` (2D arcs, bulges, splines, offsets, intersections) | **Reuse** | `geom::d2`. We add 3D types, a central `tol` module and `robust` predicates. |
| `constraints` (clean-room damped Gauss-Newton solver, conflict probing, redundancy by Jacobian rank, expression parser) | **Reuse the core, rewrite the model** | `sketch` and `model::expr`. Its `model.rs` is coupled to the DXF `Drawing`, so I replace it with a sketch entity model. I add DOF reporting (the free count from rank exists internally), drag-to-solve (its `keep`/`prefer_move` weights already do most of this) and `Midpoint`. Kinds already present: coincident, horizontal, vertical, parallel, perpendicular, collinear, concentric, equal, tangent, smooth, symmetric, fix, distance (aligned, horizontal, vertical), angle, radius, diameter. |
| `engine` command registry pattern (`CommandSpec`: id, label, menu path, shortcut, params, `enabled`, JSON `run`, prompt machine; `execute` under `catch_unwind`) | **Reuse the pattern, not the 290 commands** | `model::cmd` |
| `doc` copy-on-write entity store (cheap undo) | **Reuse the idea** | Document snapshots in `model` |
| `dxf` (standalone tag reader and writer) | **Reuse** | `io::dxf` (sketch import, drawing export) |
| `mcp`, `apps/cadcraft-cli`, control channel (JSON lines over TCP, screenshot) | **Reuse** | `cli` (headless run, MCP server, `--control` bridge) |
| `xtask` (fmt, clippy, tests, `layers`, `assets`, `wasm`) | **Reuse and extend** | Adds `unsafe-audit` and `bench-budget` |
| `ui-egui` (theme tokens, icons drawn in code, command line widget) | **Reuse pieces** | `ui`. The ribbon, browser and property panel are new. |
| `fonts`, `render`, `io` (stroke font, dimensions, linetypes, PDF plot), layouts | **Park until M4** | `drawing`. Left in git history and pulled in when drawings start. |
| `dwg`, `color`, hatch, blocks, layers, sysvars, AutoLISP work | **Drop** | Out of scope |
| ArtCraft brand (`docs/brand/`), CADCraft names | **Remove** (required by its brand licence) | — |

Method: clone with full history, add `upstream` as a remote, then one large but mechanical "strip to harvest set" commit, then renames. That keeps provenance auditable and lets us cherry-pick upstream fixes to harvested crates.

The solver choice is recorded as a decision rather than asked about. The other options:
- **SolveSpace** is GPL-3, which is incompatible with MIT/Apache distribution.
- **planegcs** is LGPL C++, so it would need a second `unsafe` FFI crate.

The CADCraft core is MIT/Apache, pure Rust and wasm-safe. It sits behind a `SketchSolver` trait, so it can still be swapped.

## 3. Workspace and layering

```
L0 geom                 L4 assembly, drawing
L1 kernel (trait)       L5 io
L2 kernel-occt (FFI)    L6 render (wgpu, no egui)
L3 sketch, model        L7 ui (egui) -> app, cli (+ mcp)
```

`kernel-occt` is depended on only by `app`, `cli` and tests. Everything else sees `dyn Kernel`.

`cargo xtask layers` checks `cargo metadata` against an allow-table. It also fails if any crate below `ui` depends on egui, eframe or winit, or if any crate other than `kernel-occt` depends on `cxx` or OCCT.

`unsafe-audit` checks two things:
- Every crate root except `kernel-occt` has `#![forbid(unsafe_code)]`.
- No `unsafe` token appears outside that crate.

The lints from CADCraft are kept: deny `unwrap`, `expect` and `panic!` in non-test code.

## 4. Kernel trait (draft)

```rust
// crates/kernel/src/lib.rs
#![forbid(unsafe_code)]
pub type KResult<T> = Result<T, KernelError>;

/// Opaque, backend-owned, NOT persistent (generational arena index).
#[derive(Copy, Clone, Eq, PartialEq, Hash, Debug)] pub struct ShapeHandle(u64);
/// Sub-shape ids are valid only for their own shape. Features never store them.
#[derive(Copy, Clone, Eq, PartialEq, Hash, Debug)] pub struct FaceId  { pub shape: ShapeHandle, pub index: u32 }
// EdgeId, VertexId likewise. BodyId = a ShapeHandle that is a solid.

/// Planar profile in a 3D frame. Each curve carries a caller tag (sketch entity id),
/// so the kernel can report which output faces each curve generated.
pub struct Profile { pub frame: Frame, pub loops: Vec<Vec<TaggedCurve2>> }

pub trait Kernel: Send {                     // object-safe: used as Box<dyn Kernel>
    fn make_box(&mut self, frame: &Frame, size: DVec3) -> KResult<Op>;
    fn make_cylinder(&mut self, axis: &Axis, radius: f64, height: f64) -> KResult<Op>;
    fn make_cone(&mut self, axis: &Axis, r1: f64, r2: f64, h: f64) -> KResult<Op>;
    fn make_sphere(&mut self, center: DVec3, r: f64) -> KResult<Op>;
    fn make_torus(&mut self, axis: &Axis, r_major: f64, r_minor: f64) -> KResult<Op>;
    fn make_face(&mut self, profile: &Profile) -> KResult<Op>;

    fn extrude(&mut self, p: &Profile, extent: &Extent, taper: Option<f64>) -> KResult<Op>; // Distance | Symmetric | TwoSided | ToFace(FaceId) | ThroughAll
    fn revolve(&mut self, p: &Profile, axis: &Axis, angle: &AngleExtent) -> KResult<Op>;
    fn sweep(&mut self, p: &Profile, path: &Path3, o: &SweepOpts) -> KResult<Op>;
    fn loft(&mut self, sections: &[Profile], o: &LoftOpts) -> KResult<Op>;
    fn fillet(&mut self, body: ShapeHandle, edges: &[EdgeId], r: f64) -> KResult<Op>;
    fn chamfer(&mut self, body: ShapeHandle, edges: &[EdgeId], c: &ChamferSpec) -> KResult<Op>;
    fn shell(&mut self, body: ShapeHandle, open: &[FaceId], thickness: f64) -> KResult<Op>;
    fn hole(&mut self, body: ShapeHandle, h: &HoleSpec) -> KResult<Op>;        // simple | cbore | csink, blind | through
    fn boolean(&mut self, op: BoolOp, target: ShapeHandle, tools: &[ShapeHandle]) -> KResult<Op>;
    fn transform(&mut self, s: ShapeHandle, t: &Transform) -> KResult<Op>;
    fn pattern(&mut self, s: ShapeHandle, p: &Pattern) -> KResult<Op>;        // Rect | Circular | Mirror, fused

    fn topology(&self, s: ShapeHandle) -> KResult<Topology>;   // faces, edges, vertices + adjacency tables
    fn face_info(&self, f: FaceId) -> KResult<FaceInfo>;       // SurfaceKind{Plane,Cylinder,Cone,Sphere,Torus,BSpline,Other} + area, centroid, normal/axis
    fn edge_info(&self, e: EdgeId) -> KResult<EdgeInfo>;
    fn tessellate(&self, s: ShapeHandle, tol: &MeshTol) -> KResult<Mesh>; // triangles grouped per FaceId, polylines per EdgeId (for picking)
    fn mass_properties(&self, s: ShapeHandle, density: f64) -> KResult<MassProps>;
    fn bounding_box(&self, s: ShapeHandle) -> KResult<Aabb>;
    fn check(&self, s: ShapeHandle) -> KResult<Validity>;

    fn import_step(&mut self, bytes: &[u8]) -> KResult<Vec<ShapeHandle>>;
    fn export_step(&self, shapes: &[ShapeHandle]) -> KResult<Vec<u8>>;
    fn set_cancel(&mut self, token: CancelToken);              // polled where the backend allows it
    fn release(&mut self, s: ShapeHandle);
}

/// Every modeling op returns lineage. This is the input to persistent naming.
pub struct Op { pub shape: ShapeHandle, pub history: History }
pub struct History {
    pub generated: Vec<(Origin, Vec<TopoId>)>,  // Origin = profile tag | input sub-shape | cap role
    pub modified:  Vec<(TopoId, Vec<TopoId>)>,
    pub deleted:   Vec<TopoId>,
}

#[derive(thiserror::Error, Debug)]
pub enum KernelError {
    #[error("invalid handle")] InvalidHandle,
    #[error("invalid input: {0}")] InvalidInput(String),
    #[error("{op} failed: {reason}")] OperationFailed { op: &'static str, reason: String },
    #[error("{0} not supported by this backend")] Unsupported(&'static str),
    #[error("exchange error: {0}")] Exchange(String),
    #[error("cancelled")] Cancelled,
    #[error("backend exception: {0}")] Backend(String),
}
```

Notes:
- STL, 3MF and native files live in `io`, built on `tessellate()`, so the trait stays small.
- The kernel instance lives on the regeneration worker thread; the UI only sees meshes and snapshots.
- M0 implements: box, cylinder, boolean, topology, tessellate, mass properties, bounding box, check, and STEP import/export. Everything else returns `Unsupported` until its milestone.

**Persistent naming (summary; full design goes in `docs/persistent-naming.md` during M0):**
- A feature stores a `TopoRef`, not an index. A `TopoRef` has three parts:
  - **origin:** creating feature + role, e.g. "side face of extrude E3 from sketch line L7", or "end cap of E3".
  - **fingerprint:** surface kind, normal or axis, area, centroid, bounding box.
  - **neighbours:** the origins of adjacent faces.
- Resolution is three steps: filter by origin through `History` lineage, then narrow by adjacency, then pick the nearest fingerprint within `tol`.
- If zero candidates or an ambiguous match remain, the feature is marked broken with a clear error and the user re-picks.
- This is why `History` and tagged profiles are part of the trait from day one.

## 5. OCCT binding approach (D1)

**Recommendation: our own `kernel-occt` crate.** It is the only crate allowed `unsafe`, licensed MIT OR Apache-2.0, and has three pieces:
- a small C++ shim (`shim/*.cpp`, about 40 coarse functions, one per kernel op);
- a `cxx` bridge;
- a Rust arena of `TopoDS_Shape`.

It links **OCCT 8.0.x dynamically**, from conda-forge (prebuilt LGPL-2.1 binaries for win-64, osx-arm64, osx-64, linux-64 and linux-aarch64), managed through a checked-in `pixi.toml`. That gives the same OCCT version on every platform and CI, with no admin rights needed.

OCCT 8 made `Standard_Failure` derive from `std::exception`, so `cxx` turns kernel exceptions into `Result::Err`. The shim also wraps every entry in `catch (...)`, so nothing unwinds into Rust. History comes from OCCT's `BRepBuilderAPI_MakeShape::Generated`, `Modified` and `IsDeleted`.

**Why not start from opencascade-rs, as the brief suggests:**
1. **Licence.** It is LGPL-2.1 (`opencascade-sys` 0.3.0, 2026-08-24). Vendoring or extending it makes our FFI crate LGPL, which conflicts with hard rule 2.
2. **OCCT version.** Its `build.rs` (main branch) rejects any OCCT whose major version isn't 7, and needs 7.8 or later. Its `builtin` mode statically compiles OCCT 7.8.1, so it can't link the current OCCT 8.0.1 (the version conda-forge ships).
3. **Shape of the binding.** It binds OCCT class by class (41 bridge modules). We need per-op exception boundaries and history maps, so we'd rewrite most of what we touch anyway.

I'd use it as a reference for build-script tricks only, copying no code.

**Fallback if you prefer the brief literally:** depend on `opencascade-sys` 0.3 from crates.io, unmodified, with `builtin`.
- OCCT 7.8.1 is linked statically.
- `NOTICE` explains how to relink: rebuild from source with a modified OCCT.
- CI builds OCCT (about 20 to 40 minutes, cacheable).
- We'd still need our own bindings for history.

## 6. Toolchain and CI

- **All platforms:** Rust stable 1.90 or later via rustup, and `pixi` (it provides OCCT, CMake and Ninja).
- **Platform compilers:**
  - Windows: VS 2022 Build Tools with the C++ workload.
  - macOS: Xcode Command Line Tools.
  - Linux: gcc or clang.
- `docs/setup.md` will have copy-paste steps per OS.
- **This PC (D3):**

  `winget install` for `Rustlang.Rustup`, `Git.Git`, `prefix-dev.pixi` and `Microsoft.VisualStudio.2022.BuildTools`; the Build Tools also need the VCTools workload. That's about 10 GB and shows one UAC prompt. I won't run it without your OK.
- **CI:** GitHub Actions on ubuntu-24.04, macos-14 and windows-2022.
  - Steps: fmt, clippy `-D warnings`, tests, `xtask layers`, `xtask unsafe-audit`, `xtask assets`.
  - wasm check: `cargo check --target wasm32-unknown-unknown` for `geom kernel sketch model assembly drawing`. A full wasm app is not feasible while the kernel is OCCT; revisit at M6.
  - Performance budgets (regeneration and tessellation times) are tracked from M1.

## 7. UI layout from the reference screenshots (layout only)

What we take from the three screenshots:
- **Layout:**
  - A quick-access strip and title bar at the top.
  - A ribbon below it: tabs, with labelled panels underneath.
  - A dockable model browser on the left, with an optional property panel above it.
  - The viewport in the centre: a gradient background, an orientation cube top-right, a vertical navigation bar (orbit, pan, zoom, look-at) on the right edge, and an axis triad bottom-left.
  - Document tabs and a status bar (prompt text, counters) at the bottom.
- **Browser content:**
  - The browser shows the part, its bodies, origin planes and axes, then features in history order.
  - It ends with a red end-of-history marker that can be dragged. That marker is our rollback bar.
- **Ribbon tabs (ours):** File · Sketch · Model · Inspect · Tools · View, with an Assemble tab in M3.
- **Panels:** Sketch · Create · Modify · Work Features · Pattern · Measure.

Kept different on purpose (rule 1):
- All icons are drawn in code.
- We use our own colour tokens.
- Standard CAD verbs (Extrude, Revolve, Fillet) are fine, but Inventor-specific names are not used. In the product we say "orientation cube" (not ViewCube), "radial menu" (not marking menu), "End of history", "New Sketch", and avoid "Shape Generator", "Model States" and "Content Center".
- The screenshots are never committed (same rule as CADCraft).

## 8. Risks

1. **Persistent naming** is the hardest problem.
   - Mitigation: lineage-first resolution, tests that edit an early sketch and verify downstream fillets still resolve, and the broken-feature UX from M2.
2. **OCCT robustness.** Fillets, shells and booleans fail on edge cases, and exceptions or signals can surface.
   - Mitigation: catch everything in the shim, run `check()` after every op, keep the last good result visible, and stress tests.
3. **Windows build friction.** MSVC, the C++17 shim and DLL discovery at runtime.
   - Mitigation: pixi environment activation; packaging bundles the OCCT DLLs from M5.
4. **LGPL compliance in binaries.** Dynamic OCCT; `NOTICE` lists the licence, where to get the source, and how to swap the DLLs.
5. **Fork divergence.** Upstream commits daily. We cherry-pick into harvested crates only, and never merge wholesale.
6. **Solver scale.** CADCraft caps a system at 4000 params. That's fine for sketches; interactive drag needs a benchmark.
7. **Scope.** CADCraft budgets about 200 hours for AutoCAD-level 3D alone; Inventor parity is far larger.
   - `ROADMAP.md` will track parity honestly per feature. Each milestone ships a vertical slice.
8. **Thread model.** OCCT is not safe to share across threads freely. Each kernel is owned by one worker thread, and handles never cross threads.

## 9. M0 steps after approval (each step one or more small commits)

1. Install the toolchain (D3).
2. Fork and clone CADCraft, strip it to the harvest set, remove branding, rename crates, and add `LICENSE-*`, `NOTICE`, `ATTRIBUTION.md`, the README trademark disclaimer and `ROADMAP.md` (parity table, all "missing").
3. Set up the workspace skeleton with every crate, lints, `#![forbid(unsafe_code)]` per crate, and xtask (`ci`, `layers`, `unsafe-audit`, `assets`, `wasm`).
4. `geom`: the `tol` module, robust predicates and 3D types.
5. `kernel`: trait, types and errors, plus a `NullKernel` for wasm and UI tests.
6. `kernel-occt`: `build.rs` (finds OCCT via pixi or `OCCT_ROOT`) and a shim for box, cylinder, boolean, topology, tessellate, mass properties, bounding box, check and STEP.
7. Tests:
   - box ∪ cylinder and box − cylinder volumes vs. analytic values;
   - STEP export then re-import, with volume equal within `tol`;
   - topology counts;
   - a hostile-input test (zero or negative sizes return `Err`, never panic).
8. Write `docs/architecture.md`, `docs/decisions.md`, `docs/setup.md` and the GitHub Actions workflow.
9. Demo: `examples/m0-box-minus-cylinder/`, run via `forgeline-cli run`, writes a STEP file and prints volume. `app` opens an empty layout shell (ribbon, browser, status bar). The 3D viewport is M1.
