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

## DEC-024 Assemblies are their own files and link part files (2026-10-08, approved by the owner)

The owner chose this over keeping parts inside the assembly file.

- An assembly is a `.tenonasm` file: the same zip container as a part (DEC-004), with
  `project.json` holding `"format": "tenon-assembly"` and its own schema version
  (docs/file-format.md).
- Components point at `.tenon` part files by path, stored relative to the assembly file's folder
  with `/` separators.
  - A part may be used by many assemblies, and several times in one.
  - Editing a part (also in context) changes it for every assembly that uses it.
- Opening an assembly whose part is missing reports the path; the component stays in the
  assembly and is shown as missing.
- Embedding parts inside an assembly file was considered and left out.
- Sub-assemblies (a `.tenonasm` placed in another) are not in M3.

## DEC-025 Assembly relationships and the solver (2026-10-08)

**Relationships.** Both kinds of the familiar workflow are kept: constraints (mate, flush,
angle, insert) and joints (rigid, rotational, slider, cylindrical, planar, ball). Each refers to
geometry by the part's persistent names (`Target`), so it survives edits to the part.

**What each geometry means:**

- A planar face gives its plane, with the normal out of the material.
- A cylinder, cone or straight edge gives its axis; a sphere gives its centre.
- A circular edge gives its centre and axis. The axis points out of the material of the planar
  face the edge bounds.
- A joint takes a frame from its geometry: the face centroid, circle centre or point on the
  axis, with Z along the normal or axis and X chosen from Z alone.

**What each relationship requires:**

- **Mate:** planes face each other `offset` apart; axes are in line; points meet; a point or
  line lies on a plane.
- **Flush:** planes face the same way, `offset` apart.
- **Angle:** the turn from A's direction to B's about a reference axis is `angle`. The
  reference is fixed in A when the angle is made: the axis the two directions turn about then.
- **Insert:** circle centres on one axis, the circles' planes facing each other `offset` apart.
- **Joints:** the two frames' Z axes face each other (flip: the same way). Origins meet:
  - rigid and rotational: at a point;
  - slider and cylindrical: anywhere on the axis;
  - planar: anywhere on the plane;
  - ball: at a point.
  Rigid and slider joints also fix the turn about Z (`angle`).

**The solver.**

- Every free component has six unknowns: a translation, and a rotation vector about its
  bounding-box centre.
- The residuals are solved by the sketch solver's damped minimum-norm Gauss-Newton (the sketch
  crate's `lm`, now public). Rotations are weighted by the component's size squared, so "move
  as little as possible" means the component's points move least.
- A new relationship first snaps the free side into place: turned onto the other geometry,
  then slid. The whole assembly is then solved. A start exactly half a turn from the answer is
  retried from a slight turn.
- A relationship that cannot hold with the others is refused, naming the ones it conflicts
  with.
- Dragging puts a component where the pointer says and solves with that component weighted
  10 000 times, so the relationships pull it back only along what they forbid.

**Degrees of freedom.**

- A component's remaining motions are counted with the other components held still, as its
  symbol shows them. They come from the null space of its 6-column Jacobian:
  - turning axes from the rotation parts;
  - sliding directions from the pure translations.
- The assembly's total is 6 x free components minus the rank of the whole Jacobian. Small
  singular values count as free below `tol::DOF_REL` of the largest.

**Names.** A component is named after its part file and an occurrence number (`pin:2`). The
parts list shows the part document's name.

**Exploded view.** The view is stored in the assembly as steps, each moving some components
along a direction. Auto Explode starts at the grounded components:

- every other component moves `spacing` away from the one that holds it, carrying everything
  attached beyond it;
- the direction is the holding face's normal, which points out of the holder;
- an axis or edge has no side, so for those the component moves away from the holder's centre.

**Interference.** Interference intersects the solids of every pair of visible components whose
boxes overlap. Overlaps under `tol::CLASH_VOLUME` count as touching.

**Not in M3:** limits on joints, contact and motion studies, tangent constraints, and assembly
STEP with a product structure. Export writes placed solids.

## DEC-026 Drawings are their own files and link their model (2026-10-08, approved by the owner)

The owner chose this over storing sheets inside the model file.

- A drawing is a `.tenondrw` file: the same zip container, with `project.json` holding
  `"format": "tenon-drawing"` and its own schema version.
- It holds sheets, views, dimensions, notes and tables. Views point at the part (`.tenon`) or
  assembly (`.tenonasm`) they show, by a path relative to the drawing's folder, as assemblies
  point at parts (DEC-024).
- Several drawings can show one model. A view shows whatever its model file holds when the
  drawing is opened or updated.
- Dimensions refer to the model's geometry by persistent name, so they follow model changes.

## DEC-027 New drawings follow ANSI, third-angle projection (2026-10-08, chosen by the owner)

- New drawings start on ANSI sheets (A, B, C; landscape) with third-angle projection: a view
  projected to the right of its parent shows the model from its right, one above shows it from
  above.
- Dimensions stay in millimetres.
- ISO (first-angle projection, A4 to A2 sheets) is a per-drawing setting, so either standard can
  be used in any drawing.

## DEC-028 Plan amendment: reliability first, new milestones, native kernel experimental (2026-10-09, owner)

The owner amended the plan (docs/plan.md section 10):

- **Positioning:** makers, students and small shops; Tenon wins on reliability, coherence and
  speed, not feature count. When tasks compete, dependability of existing features goes first.
- **M4 continues** with a higher bar: auto-dimension suggestions, balloons and a parts list from
  the assembly's bill of materials, sheet and title-block templates, clean PDF/SVG/DXF, and views
  that follow model changes, with tests. The first M4 report (2026-10-08) does not meet it, so
  M4 is open again in `ROADMAP.md`.
- **New milestones** replace the old M5 (breadth) and M6 (native kernel): M5 reliability and
  project format, M6 part feature breadth, M7 standard parts and maker workflow, M8 sheet metal
  basics, M9 performance, M10 agent-native workflow, M11 linear static stress checks.
- **Native kernel:** demoted to an experimental track behind the `Kernel` trait. OCCT stays the
  default and the product does not wait for it. It does not start before M5 and M6 are done.
- **Not now:** CAM, generative design, cable and harness, tube and pipe, dynamic simulation.

Consequences:

- The ROADMAP was re-rated against the new bar. Rows whose named tests did not prove the claim
  went from done to partial, with the gap stated: M1 performance budget; M2 broken-feature
  reporting; in M4, views with hidden-line removal, detail views, associative dimensions, the
  parts list, export, views that follow the model, and the drawing environment.
- M5's text-based `.tenon` changes DEC-004 (zip container), so its design goes to the owner
  before any code.
- M6's "iProperties-style panel" is called Properties in the product (DEC-019 keeps trademarked
  names out).
- DEC-011's "revisit the wasm scope at M6" now waits on the experimental native kernel.

## DEC-029 Centre marks and centrelines placed by hand join drawing format v1 (2026-10-09, chosen by the owner)

Automatic centre marks and centrelines cover holes and cylinders only. To place them by hand, and
on other symmetric features, the drawing format gained three annotation kinds: `center_mark` (on
a circle or arc), `centerline` (through two picked points) and `centerline_bisector` (midway
between two lines: parallel, along both; meeting, the bisector of their angle). Like dimensions,
they keep persistent edge picks, never coordinates, so they follow the model.

The owner chose to add them to format version 1 rather than start version 2: an older build of
Tenon cannot open a drawing that uses them, and no build had been released, so nothing breaks.
Drawings without them are unchanged.

## DEC-030 "Save changes?" before New, Open and Exit (2026-10-09)

No document type asked before discarding unsaved work (found in the M4 review). Now File > New
Part, New Assembly, New Drawing, New Drawing from Template, Open and Exit, and the window's close
button, show one modal prompt, "Save changes to <file>?", with Save, Don't Save and Cancel, when
anything open has unsaved changes.

- **One prompt for everything that would be lost.** A drawing is asked about with the models
  changed from it, an assembly with its changed parts, including a model being edited from the
  drawing and a part edited in place (both are on the workbench at the time). The prompt lists
  those files under the top document's name.
- **Save works from the top document**, as its File > Save does: saving a drawing saves the
  models changed from it, saving an assembly its parts. There is no per-file choice; saving the
  top document without the files it uses changed would leave it inconsistent with them.
- **A document never saved asks where.** If that dialog is closed, or saving fails, nothing else
  happens: the pending command does not run and nothing is lost.
- **Asked first, then the file dialog** for Open and New from Template, as single-document
  programs do. Cancelling the file dialog after Don't Save keeps the document, still changed.
- **Keys:** Save is the default button (Enter) and Esc cancels. While the prompt is open no key
  reaches the document behind it.
- **Not asked:** a new document never edited; a file opened from the command line (nothing is
  open yet); scripts, the CLI and MCP (their `file.new` and `file.open` are explicit); and
  `--screenshot` runs, which always close.

## DEC-031 Format version 2: plain-text TOML documents (2026-10-09, chosen by the owner)

Parts, assemblies and drawings move from zips holding JSON (DEC-004, DEC-024, DEC-026) to
version 2: one plain UTF-8 text file each, in TOML with a fixed layout. The owner approved all
four recommendations of `docs/format-v2-proposal.md`.

- **The container is one text file per document.** Git diffs and merges it.
- **The syntax is TOML with a layout Tenon writes itself:**
  - one field per line;
  - lists of things one per line, each ending in a comma, so appends never conflict;
  - records as `[[feature]]` tables with their `kind` flattened in;
  - sketch entities and constraints as records with their ids.

  It is read with `toml_edit` (MIT/Apache, already in the dependency tree). Canonical JSON was
  the cheaper alternative; it was not chosen because of its long lines and its appends that
  conflict.
- **All three kinds of document** follow the same rules (`.tenonasm` and `.tenondrw` as well as
  `.tenon`).
- **Version-1 files** are read forever and upgraded when saved. The first save keeps the
  original once as `name.v1.ext`; a copy already there is never overwritten.
- **Stable output:**
  - no generator or timestamps;
  - fields in the model's declaration order (serde_json's `preserve_order` is now on across the
    workspace);
  - computed values (solved sketch positions and component placements, fingerprints) rounded to
    `tol::FILE_DECIMALS` = 9 decimals of a millimetre;
  - values the user gave written exactly.
- **What it adds:**
  - `tenon-cli diff` and `file.diff` report changes by parameter and feature;
  - `tenon-cli upgrade` rewrites old files;
  - errors give a line number and the record.
- **Merging:** two branches that each add a feature give both the same id. That is refused on
  open with both features named, not repaired; an automatic renumbering is left for later.
- Title block templates stay small JSON files: they are not documents.

## DEC-032 Kernel failures in plain words; no silent successes (2026-10-09)

A feature that fails in the kernel shows three things: what could not be made, the likely
reason for that feature type, and what to try. The kernel's own words follow in brackets
("The 25 mm fillet could not be made on this edge. The radius is probably too large ...: try a
smaller radius, or fewer edges. (Kernel: fillet failed: ...)").

- **The plain part comes first.** It is chosen by the feature type and the kind of kernel
  failure (`crates/model/src/explain.rs`). The kernel's words stay for diagnosis and bug
  reports.
- **Already plain messages are left as they are:** messages Tenon writes itself (a lost
  reference, an empty result) and inputs the kernel refused.
- **A result that cannot be right is a failure even when the kernel reports success.** A shell
  that leaves the volume unchanged hollowed nothing (OCCT does this when the walls are thicker
  than the part allows). Such checks belong in the model, so every kernel backend gets them.

## DEC-033 Autosave and crash recovery (2026-10-09)

While anything has unsaved changes, Tenon keeps copies of it in a folder of its own, never beside
the user's files. After a crash, the next start offers them back.

- **Where:** `<the app's settings folder>/recovery/<one folder per running Tenon>`, beside
  eframe's settings (`eframe::storage_dir("Tenon")`). The user's folders and Git repositories
  never get stray files.
- **When:** every 30 seconds, if something changed since the last copy. Copies are written whole
  and renamed into place; the manifest naming them is written last.
- **What:**
  - the open part, assembly or drawing;
  - the parts and models it uses that were changed with it (parts edited in place, a model edited
    from the drawing, a part inside an assembly the drawing shows, each kept where it was
    changed).

  The copies are in the current file format.
- **Running or crashed:** each running Tenon holds an exclusive lock on its folder
  (`File::try_lock`). A folder whose lock is free belongs to a Tenon that did not close properly.
  A second window never sees another's work as lost.
- **Recover, Discard or Not Now** (Esc). Recover opens the document from its file (or as new, if
  it was never saved) and applies each kept copy as one edit. So the work shows as unsaved, Save
  writes it where it came from, and Undo goes back to the saved file. Not Now keeps the copies for
  the next start.
- **Closing properly** (after Save or Don't Save) removes the folder. Screenshot runs neither
  keep nor offer anything.

## DEC-034 Repairing broken references (2026-10-09)

When a feature fails because a face or edge it uses is gone, Tenon names the reference, offers
the nearest replacements, and puts one in with one click.

- **Which reference broke** is found after the fact, not threaded through each feature: every
  edge and face reference in the failing feature's definition is checked against the part as it
  stood just before that feature (the scene a failed rebuild shows). This works for every feature
  type, including ones added later.
- **Candidates**, nearest first (at most three):
  - edges: one sharing a face with the lost edge first (a redrawn side keeps the top), then by
    how far the middle moved plus the change in length;
  - faces of the same surface sort: by how far the centroid moved, how much the direction
    turned (a right angle counts as 10 mm) and the relative change in area.
- **Commands:** `model.broken` (the failing feature, its message, each lost reference's path in
  the feature definition and its candidates) and `model.repair` (feature, path, with). The
  repair is one undoable edit; a reference of the wrong sort is refused.
- **In the app:** the failure banner's Repair button (or Repair Reference in the browser's
  context menu) highlights the candidates in green. A click on any face or edge of the right
  sort, highlighted or not, puts it in. Esc stops.

## DEC-035 M6 feature types join format version 2 (2026-10-09)

Sweep, coil and loft (and the M6 features after them) are new feature types in part files.
They join format version 2 instead of starting version 3, following DEC-029: version 2 was
introduced the same day and no build has been released, so no file in anyone's hands changes
meaning. Files without the new types are byte for byte what they were; a build that does not
know a type refuses the file and names the record ("line 66: feature 4 (Sweep1): unknown
variant `sweep`"). The rule is now written into `docs/file-format.md` ("Changing the format"):
once a version has shipped in a release, any change to what a document holds bumps it.

This is the cheap choice to reverse: starting version 3 instead means changing one number and
adding an empty migration step. It is listed in the M6 report for the owner.

## DEC-036 Freeform faces are meshed looser inside than along their edges (2026-10-09)

A swept, lofted or blended face is a B-spline surface. OCCT refined the inside of such a face
until every triangle was within the edge tolerance (0.01 mm) of it: a five-turn coil of radius
40 came out at 549 000 triangles and took 7.6 s to show, against 26 000 for a torus of the same
size. The inside of a face may now deviate five times the edge tolerance (0.05 mm) and its
facets turn up to 0.5 rad; edges keep the full tolerance, so faces still meet exactly and
silhouettes of analytic faces (planes, cylinders, cones, spheres, tori) do not change at all.
The same coil is 108 000 triangles in 1.3 s and looks the same, because shading uses the exact
surface normal at each vertex.

- **Rejected: no refinement inside at all.** Sixteen times faster still, but the error then has
  no bound on a large, gently curved face, and the app's STL export uses the same mesh.
- **Rejected: other ways of building the helix** (an interpolated spline, lower degree, tighter
  approximation): the triangle count did not move; the cost is in the mesher.
- `a_long_coil_meshes_without_excess_triangles` holds the count down.

## DEC-037 Sketches show in the part until a feature uses them (2026-10-09)

Outside sketch mode, a sketch that no feature uses yet is drawn over the part in a dim line,
as Inventor shows unconsumed sketches. A sweep needs two sketches and a loft
several, and they could not be seen while being chosen. The sketches an open Sweep, Coil or
Loft panel takes its shape from are drawn in the accent colour. A sketch disappears once a
feature uses it, and sketches past End of Part or suppressed do not show.

A new Sweep offers the last sketch with a closed profile whose plane is not parallel to the
path's (a profile in the path's own plane cannot be swept along it, and the part says so in
plain words if asked to); the path is the last other sketch of lines and arcs, preferring one
with no closed profile of its own. A new Coil takes a construction line of the profile's sketch
for its axis, else the origin axis lying in the sketch plane. A new Loft takes every sketch
with a closed profile that nothing uses yet, in order.

## DEC-038 Threads: cosmetic by default, sized from the face, cut only when asked (2026-10-10)

A Thread feature goes on a round shaft or hole. By default it is **cosmetic**: the face is on
record as threaded (`model.threads`), drawn in its own colour, and the part stays a plain
cylinder. That is instant, and enough for drawings and for parts that are tapped or bought.
Ticking **Modelled** cuts the groove as real geometry, for threads that are printed.

- **Size follows the face.** Without a pitch, the thread takes the ISO coarse pitch for the
  face's diameter and is named from it ("M8x1.25"), and both follow when the diameter changes.
  A shaft is its thread's major diameter; a hole is taken as drilled for tapping (between the
  minor diameter and the usual tap drill), so a 6.8 mm hole is M8. A pitch or a name of one's
  own can be given.
- **The sizes** are the 21 first-choice coarse sizes of ISO 261, M1 to M64, and the basic 60
  degree profile's proportions: facts of the standard, typed into `crates/model/src/threads.rs`.
  No dataset is copied (ATTRIBUTION.md). Fine pitches, inch and pipe threads are entered by
  hand for now; a library belongs to M7.
- **Where it starts.** At its face's open end (a bolt's tip, a hole's mouth), found by testing
  for material beyond each end. With both ends open, or neither, at the upper one. This does
  not depend on how the kernel happens to hold the cylinder's axis.
- **The groove** is the basic profile's (7/8 of the pitch wide at a shaft's surface, 1/4 at its
  root, 0.5413 of the pitch deep; in a hole 3/4 and 1/8), wound as a helical solid and cut away.
  It runs out past an open end and stops just short of a closed one (a shoulder, a blind
  hole's bottom).
- **Every cut is checked** by the volume it removed, against the turns asked for. OCCT returned
  nothing, or the part unchanged, for some thread lengths without reporting an error; the cause
  was a helix built as one long edge (see below), but a wrong solid must never pass silently
  (DEC-032), so the check stays, with two retries at other run-out lengths.

Two kernel fixes came out of this, and both also fix Coil:

- **A helix is built one turn per edge.** As a single edge, a 60-turn coil came out at half its
  volume, and booleans with the swept faces (each winding several times round the axis) cut
  nothing or everything depending on the length.
- **A round face says which side its material is on** (`FaceInfo::reversed` now accounts for
  the surface's handedness), so a shaft is told from a hole however the solid was made.

## DEC-039 Revolve: a profile across its axis, and an axis worth offering (2026-10-10)

The owner could not turn a circle into a sphere, or revolve at all on some planes. Four faults:

- **A profile lying across the axis was refused.** A whole turn of it has a clear meaning: the
  turn of what is on one side together with the turn of what is on the other. The kernel now
  divides the profile along the axis, turns each side and joins them, so a circle about a line
  through its centre is a sphere and a rectangle about its middle is a cylinder. Faces keep the
  names of the sketch curves they come from. Part of a turn of such a profile is still refused,
  in plain words: each side would sweep a different sector.
- **The axis offered was always the origin Y axis**, even when it was square to the sketch (an
  XZ sketch) or a line had been drawn to turn about. A new revolution now takes: a centre line
  of the sketch (construction); else the one line in the sketch that bounds no profile; else an
  origin axis lying in the sketch's plane, the upright one first, and one the profile is beside
  before one that runs through it.
- **An axis square to the sketch made an empty part without a word.** It is refused, saying
  what to choose; and any revolution with no volume is an error.
- **The axis could only be chosen from a list.** With the Revolve (or Coil) panel open, the
  sketch's lines show over the part and a click on one makes it the axis, as does a click on a
  work axis or on an origin axis in the browser. The axis is drawn as a centre line.

Still missing: a line drawn across a profile does not divide it into regions that can be picked
separately (Inventor does this); it is listed in the roadmap.

## DEC-040 A tapered extrusion is an extrusion and a draft (2026-10-10)

Extrude's Advanced Properties had promised Taper for M6. The kernel has no tapered prism, but
it has Draft: a tapered extrusion is built straight and its sides are then tilted about the
sketch plane, where they stay put. The faces keep the names a straight extrusion gives them.
The taper is optional in the file (`taper`, absent for straight extrusions, so existing files
are unchanged) and is a parameter in degrees. It works for one distance and Through All; a
symmetric or two-distance extrusion would need a different angle each side of the sketch and is
refused, saying so.

## DEC-041 Sketch dimensions are drawn and placed as dimensions (2026-10-10, asked for by the owner)

A sketch dimension was a number in a box beside its geometry. The owner asked for it to be drawn
as Inventor draws it. It now is:

- **Drawn** with extension lines, a dimension line with arrowheads and the value on the line; a
  leader through the centre for a diameter and from it for a radius; an arc for an angle.
  Arrowheads go outside when there is no room between the extension lines, and the line runs out
  to a value placed beyond them (`crates/ui/src/dims.rs`). Positions are worked out in the
  sketch and sizes on screen, so arrowheads do not grow with the zoom.
- **Placed by a click.** With the Dimension tool, picking geometry starts a dimension that
  follows the pointer; the next click puts it down and opens its value box there. Between two
  points or on a sloping line, the pointer chooses: over or under gives a horizontal dimension,
  beside gives a vertical one, anywhere else the aligned one.
- **Moved by dragging** its value, as one undoable step (`sketch.place_dimension`).
- **Saved.** Where a dimension was placed is kept with the sketch and written on the
  dimension's own record (`at = { x, y }`), so moving one changes one line of the file. It is an
  optional field of format version 2 (DEC-035); files without it are unchanged, and a dimension
  without it is drawn beside its geometry, away from the middle of the sketch. `tenon-cli diff`
  reports "1 dimension moved".

Rejected: text turned along the dimension line (harder to read at small sizes; it can follow
when dimension styles do), and a separate list of placements in the file (a moved dimension
would then change a line far from the dimension).

## DEC-042 Closer to Inventor in the sketch and feature workflow (2026-10-10, asked for by the owner)

The owner asked for research into Inventor's interface and for Tenon to look and feel more like
it. The findings, what Tenon now does and what is still missing are kept in
[inventor-fidelity.md](inventor-fidelity.md), in the order the gaps should be closed. Behaviour
is taken from Autodesk's published help; no artwork, text or screenshot of the product is used
(DEC-019). Done with this decision:

- **Profiles are chosen by clicking.** With an Extrude, Revolve, Sweep or Coil panel open, the
  profile's sketch shows over the preview and a click inside a closed region adds it to the
  feature or takes it out (the last one stays). The chosen regions are outlined boldly. Until a
  region is clicked the feature uses every outer region, as before. (Extrude and Revolve had
  silently dropped a chosen set of regions when the feature was made; they now keep it.)
- **The sketch status bar** has Inventor's two display controls: how dimensions read (Value,
  Name, Expression) and whether constraint symbols show (also F8 and F9).

The largest gap left is that crossing curves do not divide a sketch into regions: a line drawn
through a circle does not give two halves to pick from.
