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

## DEC-014 Command registry and MCP server written for the feature tree, not ported (2026-10-07)

DEC-002 planned to port CADCraft's command registry and MCP server. CADCraft's commands are 2D
drafting verbs over a flat entity list, typed with command-line syntax. Tenon's commands edit a
feature tree, with undo per command, and some need the kernel. So the registry
(`tenon_model::cmd`, with file commands in `tenon_io::cmd`) was written for that model.

The MCP server (`apps/tenon-cli/src/mcp.rs`) has the same shape as CADCraft's, as checked
afterwards: newline-delimited JSON-RPC, protocol-version negotiation, and tool failures returned
as `isError` results. It differs in two ways:

- It drives a headless session only. CADCraft can also drive a running app through a loopback
  control port; for Tenon that is listed as missing in the ROADMAP.
- It offers tools only, no resources.

It is hand-written over `serde_json`, with no MCP SDK, to avoid an async runtime in the CLI.

## DEC-015 Command scripts are a tooling format (2026-10-07)

`tenon-cli run` takes a JSON list of commands, with `$name.path` references and `expect` checks
(docs/scripts.md). Demos and end-to-end tests are written in it. It is not a document format:
projects are `.tenon` files (DEC-004). So it can change between milestones; changes are logged
here.

## DEC-016 Scripts name faces by origin (2026-10-07)

`model.face_ref` accepts a face origin (`{type: side, feature, curve}` or `{type: cap, feature,
end}`) as well as a face index. Face indices depend on the kernel's enumeration order, which
would make scripts kernel-specific; origins are Tenon's own persistent names
(docs/persistent-naming.md). An origin that matches no face, or several faces, is an error.

## DEC-017 Viewport colour pipeline and lighting (2026-10-07)

- **Colour:** egui-wgpu 0.36 samples native textures as gamma-encoded `Rgba8Unorm`. The viewport
  renders into `Rgba8Unorm`, lights in linear space, and encodes to sRGB in the shader. An
  `Rgba8UnormSrgb` target made the model look about twice as dark.
- **Lighting:** both renderers use a key light above and slightly left of the eye
  (`Camera::key_light`) and the same shading formula. A headlight along the view direction lit
  the three faces of an iso view identically.
- **Test:** `gpu_viewport_renders_when_an_adapter_exists` reads the GPU image back and compares a
  lit face with the software renderer.

## DEC-018 Native file dialogs: rfd (2026-10-07)

The desktop app opens and saves files through `rfd` (MIT), which uses the platform dialogs: Win32,
macOS panels, and the XDG portal on Linux (no GTK dependency). The workbench takes dialogs as
injected `Services`, so the UI crate and its tests do not depend on rfd.

## DEC-019 UI fidelity: match the workflow one to one, keep brands out (2026-10-08, owner request)

The owner asked for the UI to match Inventor's as closely as possible ("the workflow should feel
exactly the same"). From M2 the UI follows Inventor's:

- layout (panel order, button sizes and positions, browser structure, a docked properties panel);
- mouse and keyboard behaviour;
- interaction flow (pick a plane in the viewport, value boxes at the cursor, inline dimension
  editing).

Generic functional labels are used where they are the natural words, even if identical
("Finish Sketch", "End of Part", "Extrusion1"). This supersedes the stricter vocabulary rule
of DEC-012.

The original hard rule still holds:

- no Autodesk icons, artwork or screenshots in the repository;
- no trademarked or branded names (ViewCube, SteeringWheels, iProperties, iLogic, Content Center,
  Shape Generator). We say "orientation cube" and "radial menu";
- colours are Tenon's own tokens.

Reference screenshots stay out of the repository.

## DEC-020 Navigation follows Inventor's mapping; views glide (2026-10-08)

| Input | M1 | From M2 |
|---|---|---|
| Left drag (select mode) | orbit | selection box: window to the right, crossing to the left |
| Middle drag | pan | pan |
| Shift + middle drag | — | orbit |
| Right click / drag | orbit | radial menu (a flick picks a slot) |
| Wheel | zoom about the pointer | zoom about the pointer |
| F2 / F3 / F4 + left drag | — | pan / zoom / orbit |
| F5 / F6 | — | previous view / home view |

The camera gained a roll angle, so the orientation cube's quarter-turn arrows can roll the view.
View changes from the cube, Home, Zoom All, Look At and sketch entry animate over 0.3 s; any
navigation input stops the animation where it is. The cube has 26 targets (6 faces, 12 edges,
8 corners), drag-to-orbit and a context menu (home, perspective/orthographic, set home).

## DEC-021 Sketch workflow details (2026-10-08)

- **Projected origin:** `sketch.create` puts the part origin into every new sketch as a fixed
  construction point (`project_origin`, default true; its id is returned as `origin`). Drawing from
  it, or attaching a rectangle corner to it, fixes the sketch's position, as the familiar
  workflow expects. Scripts that count entities see one more.
- **Plane picking:** Start 2D Sketch shows the origin planes in the viewport; a click on a plane
  or a planar face (the nearer one along the pointer ray) starts the sketch. The dialog is gone.
- **While drawing:** lines within 3° of horizontal or vertical snap and get that constraint.
  Typed values go into boxes beside the cursor (line length and angle, circle diameter,
  rectangle width and height) and become dimensions. Dimensions are edited in a box placed on
  the dimension.
- **Views:** entering a sketch glides square to it, X to the right, framed; finishing glides back
  to the view from before.
- **Keyboard focus:** Tenon's own drawn controls are not keyboard-focusable, so Tab belongs to the
  value boxes and Enter never presses a toolbar button.

## DEC-022 Patterns and mirrors copy feature solids (2026-10-08)

A pattern or mirror copies features, as the familiar workflow does, not the whole body.
Regeneration keeps the tool solid of every feature that some pattern copies (the extruded,
revolved or drilled solid before it was combined with the part), transforms copies of it, and
combines each copy the way its feature did (join, cut, ...). Fillets, chamfers and shells have
no tool solid and are refused with a message. Copying a pattern copies all of its occurrences,
the first one too. Copied faces are named `From { feature: pattern, source: key of the copied
face's name, ordinal: copy number }`, so edges of copies can be referenced. "Through All"
extents are sized once, for the original; a copy reaching past the part is the known limit.

## DEC-023 Parameters and equations (2026-10-08)

- Every driving sketch dimension and every numeric feature value gets a name when it is
  created: `d0`, `d1`, ... in creation order (files from before parameters get theirs when
  opened). A name may be given an equation; user parameters add values of their own.
- The document stores the names, equations and comments (`params`, additive: older files
  load). Values stay where they always were (in the sketch constraints and feature
  definitions); equations write them.
- After every edit, in the same undo step, equations are evaluated in dependency order and their
  values written into the document. An unknown name, a cycle, or a value the document refuses
  (a non-whole count, an unsolvable dimension) refuses the whole edit with a message.
- Setting a value directly (typing a number, dragging the extrude arrow) drops its equation.
- Equations are in the units shown: millimetres and degrees, with `mm cm m in ft deg rad ul`
  suffixes, `+ - * / ^`, parentheses, and `sin cos tan` (degrees) `asin acos atan sqrt abs round
  floor ceil ln log exp min max`. Units are converted, not checked.
- Every value field takes an equation; it shows `fx: <equation>` until a plain number replaces
  it, and driven dimensions read `fx: 20`.