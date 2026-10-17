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

## DEC-043 Materials, appearance and physical properties (2026-10-10)

M6 asks for mass properties, materials and appearance in a Properties panel.

- **A part has one material:** a name, a density and a colour. The part file holds all three
  (`[material]`), not a reference into a library, so a file reads the same on every machine and
  a later change to the library does not change parts already made.
- **The library** is 22 common workshop materials held in code
  (`crates/model/src/materials.rs`): metals, printing and moulding plastics, woods. Densities
  are typical handbook figures typed in; names are generic; colours were chosen for Tenon
  (ATTRIBUTION.md). A library material's density can be typed over, and a material that is not
  in the library is a name, a density and a colour.
- **Appearance** is one colour of the part's own (`appearance = "#rrggbb"`), shown in place of
  its material's. Without either the part is the standard grey.
- **Units.** Parts are in millimetres, so densities are in g/cm^3 and masses in grams
  (kilograms from 1000 g in the window). A part without a material is at 1 g/cm^3, so its mass
  in grams is its volume in cm^3.
- **`model.mass` changed meaning:** its `density` was a mass per mm^3 with a default of 1; it
  is now g/cm^3, defaulting to the part's material. `mass` in its result is therefore 1000
  times smaller than before for the same number. Volumes, areas and centres are unchanged.
  Nothing released used the old meaning (the examples check volumes).
- **The Properties window** (Inspect > Properties) replaces Mass Properties: material, density,
  appearance, then mass, volume, area and centre of gravity, per solid. Every change is a
  command and one undo step. Values follow the part, so there is nothing to update by hand.
- **In an assembly** the same button gives a summary: the whole assembly's mass, volume, area
  and centre of gravity with each component at its own part's density, and a row per
  component. Components are drawn in their part's colour. The parts list gains Material and
  Mass; in the CSV they are the last two columns, so a script that reads the earlier columns by
  position still works.
- **Renders** (`render.png`, `tenon-cli render`) show parts in their colours.

Rejected: a material library as a file beside the parts (a second file to ship, and parts that
break when it is missing); appearances for single bodies or faces now (they need face
references in the appearance and a way to repair them: later, see inventor-fidelity.md);
strength and thermal properties (M11 adds what its solver needs).

## DEC-044 Design tables (2026-10-10)

M6 asks for design tables: one part in several sizes.

- **One table per part.** Its columns are parameters, by name; its rows are named sizes; one
  row is active (`crates/model/src/table.rs`).
- **The active row is the part.** Making a row active writes its values into the parameters, as
  one undo step. From then on that row follows the part: a dimension edited in the sketch, in a
  panel or in the Parameters dialog is that size's value. So the parameters and features in the
  file always hold the active size, and a reader that knows nothing of tables still gets the
  right part.
- **Only plain values.** A column is a model parameter without an equation or a user parameter
  whose equation is a number. A parameter that begins to follow an equation, or whose feature
  is deleted, leaves the table (the table would overwrite the equation, or set nothing); one
  that is renamed keeps its column; a table with no column left is removed. Undo brings each
  back.
- **Stored** as `[table]` in the part file, an optional part of format version 2 (DEC-035).
  When a file edited by hand disagrees with itself, the part's own values stand and the active
  row takes them.
- **The dialog** (Manage > Design Table): tick the parameters, Create Table; then a grid with a
  row per size, the active one ticked.

Not done, and listed in inventor-fidelity.md: columns that suppress features or set the
material; a file per row; an assembly component that names the row it uses. The last needs a
field on the component and a rule for when the row is renamed or removed, so it goes to the
owner with the M6 report.

Rejected: a copy of the feature list per size (the copies drift apart); a spreadsheet file
beside the part (a second file, and not compared or merged with the part).

## DEC-045 Smaller things settled in M6 (2026-10-10)

- **Scripts: `within`.** A step may give the relative tolerance of its numeric expectations
  (`"within": 0.0001`, at most 0.1) in place of the usual 1e-6. It is for values the kernel
  approximates: a coil's volume is right to about 1e-6 and a cut thread's to a part of a turn,
  and the M6 example checks both against their analytic values rather than against numbers
  copied from a run (docs/scripts.md).
- **A newly opened assembly is fitted as its parts arrive.** Parts regenerate one after
  another; the view was fitted to whichever came first and left the rest off screen. It is now
  fitted again with each arrival and for the last time when all are in
  (`a_newly_opened_assembly_is_fitted_again_as_its_parts_arrive`).
- **The M6 example** is four small parts rather than one: no single part uses a coil, a thread,
  a sweep and a drafted, lofted, split body sensibly, and four parts with four materials also
  show the assembly's parts list (examples/m6-fittings).

## DEC-046 The owner confirms M6 and hands over the open decisions (2026-10-10)

The M6 report ended with four decisions for the owner. The answer: "Do what you feel
necessary, let's keep moving forward." M6 is confirmed, and the four are settled here:

1. **Format version 2 grows without a version bump** (DEC-035 stands). Every addition so far
   is optional: a file without it is byte for byte what it was.
2. **`model.mass` in grams from g/cm^3** (DEC-043 stands).
3. **An assembly component may name a design table row.** Done: DEC-048.
4. **Curves that cross divide a sketch into regions.** Done: the rest of this decision.

Driven dimensions, next on the list in [inventor-fidelity.md](inventor-fidelity.md), followed
with DEC-047.

**Regions from crossing curves.** A sketch's closed regions were traced along whole curves
joined at their end points, so a line drawn through a circle divided nothing. Now lines, arcs
and circles divide one another where they cross, where they touch, and where one ends on
another; each bounded face of that arrangement is a region that can be clicked.

- **Side by side is one profile.** A feature made from several regions uses their union: a
  piece of curve with a chosen region on both sides is inside the profile and left out, and
  pieces of one curve that follow one another are one curve again. Both halves of a divided
  circle extrude to one cylinder with one round wall, exactly as the undivided circle does. So
  "nothing picked" (every outer region) gives what it always gave, and existing parts
  regenerate as they did.
- **How a region is named in a feature.** A list of curves still means what those curves
  enclose on their own, whole, however other curves divide it; it is the only form written for
  a region nothing else divides, so files that had it are unchanged. One region of a divided
  sketch is named by the side of each curve of its outer boundary it lies on: `{ left, right }`
  (a curve runs from its start to its end; a circle or arc counter-clockwise, so left is
  inside). Regions alike in curves and sides (two regions between the same two lines and the
  same two circles) are counted along the lowest-numbered curve: `nth`. All three survive
  dimension edits; a region that is gone (its dividing line deleted) makes the feature say so
  rather than take something else.
- **Touching is one point.** Curves closer than a micrometre meet at one place, so a circle
  resting on a line makes no sliver, and faces under 1e-9 mm^2 are not regions.
- **Splines** join at their ends only. Dividing a spline needs trimmed splines in the kernel's
  profile type, which it does not have; listed as a gap.

Rejected: naming a region by a point inside it (it does not survive a dimension edit); always
naming regions by side (every existing file's keys would change).

## DEC-047 Driven dimensions (2026-10-10)

A dimension that would hold a sketch where it is already held was refused as redundant. It can
now be kept as a driven dimension: it follows the sketch instead of setting it, and reads in
parentheses, as in Inventor.

- **The solver leaves it out.** Its value is measured again after every change. It is saved as
  `driven = true` on the dimension's own record.
- **The Dimension tool decides.** Where a driving dimension would be one too many, the
  dimension is put down as driven at once and the status bar says why (Inventor can ask first;
  Tenon follows its option not to). **Driven Dimension** on the Format panel is a tool: click a
  dimension to change it over. Making a dimension drive again is refused, with the reason,
  when the sketch is held without it.
- **It is a parameter that is read, never set.** It has a name like any dimension; equations
  may use it (a block as high as half its base's diagonal), and it cannot be given a value, an
  equation or a design table column. Equations are evaluated in dependency order as before; a
  driven dimension becomes known once nothing more is to be written into its sketch. A
  dimension of the same sketch that tried to follow it would chase itself, and is refused in
  those words.

Rejected: asking before making the dimension driven (a dialog for something that is undone with
one key); leaving driven dimensions out of the parameters (they are what makes one sketch's
result usable in the next feature).

## DEC-048 An assembly component may name the size of its part (2026-10-10)

With design tables (DEC-044) a part file holds several sizes, but an assembly could use it
only at the row the file was saved at. A component may now name a row: `row = "M8x50"` on its
record in the assembly file.

- **Nothing new is written.** A size is worked out from the part file's document whenever that
  changes (in memory it is a part of its own, keyed by file and row), so there is one file to
  edit and the sizes cannot drift from it. Editing the part in place changes every size.
- **It is a part like any other everywhere else:** geometry, relationships (faces are found by
  the same names in every size), interference, mass, STEP export, drawing views. The parts
  list has an item per size, named with it ("Pin, 8x50").
- **Changing size** is a command (`asm.set_row`), on the component's right-click menu in the
  browser (Size); `asm.insert` takes a `row`. The relationships are solved again.
- **A row the part no longer has** (renamed or removed) shows its components as missing and
  says which row; the rest of the assembly opens.

Not done: choosing the size while placing a part from the Place dialog (it is placed as its
file has it, then changed); a file per size, for sending one size to someone.

Rejected: a copy of the part file per size (Inventor's members: the copies must be kept in
step, and a maker's folder fills with files); storing the row's values in the assembly (they
would go stale when the table changes).

## DEC-049 Standard parts are ordinary parts, built by commands from sizes typed in as facts (2026-10-11)

M7 asks for a standard parts library "generated parametrically or from open-licensed data
only", with no scraped catalogues. Tenon's library is ten families built into the program
(`tenon_model::library`): hex head screws (ISO 4017), socket head cap screws (ISO 4762), hex
nuts (ISO 4032), plain washers (ISO 7089), deep groove ball bearings by their ISO 15 boundary
dimensions, square and rectangular tube, round tube, angle, flat bar, and a T-slot profile.

- **A standard part is a part.** Each family is built by the ordinary commands (parameters, a
  fully dimensioned sketch, extrusions, a cosmetic thread) and carries a design table (DEC-044)
  with a row for each size. Nothing about it is special afterwards: it opens, edits, saves,
  diffs and goes into drawings like any part, and every size is checked against its analytic
  volume (`every_size_of_every_standard_part_is_one_solid_of_the_right_volume`).
- **The sizes are facts, typed in.** A standard's dimensions (an M8 hexagon is 13 across its
  flats; a 608 bearing is 8 x 22 x 7) are facts, widely published and not anyone's property;
  the standards' text, drawings and tables are, and none of them is in the repository. No
  dataset was downloaded or copied, so there is no third-party licence to carry; ATTRIBUTION.md
  says where the numbers stand. A few are spot-checked in `the_sizes_are_the_standards`.
- **What is simplified says so.** Threads are cosmetic; heads have no chamfer or washer face;
  a bearing is one solid by its boundary dimensions; sections have square corners. Each
  family's description in the dialog says what was left out.
- **The T-slot profile is Tenon's own.** Makers' profiles are their makers' designs. Tenon's is
  a generic section with one slot a side and a centre bore, in three sizes, described as such:
  good for laying out a frame, not for relying on a fit.
- **In an assembly** a standard part is placed in a size (`asm.insert_standard`, Assemble >
  Standard Parts). Its file is written once into a folder named `standard` beside the assembly
  and every size comes from that file by its row (DEC-048), so a project carries its standard
  parts with it and opens anywhere. **In a part**, Manage > Standard Parts starts a new part
  from a family (`library.new`), to change and save as one's own.

Not done: other standards (DIN, ANSI inch sizes), fine threads, more families; a way to add a
family without writing code; a fastener that sizes itself from the hole it is dropped on; a
length asked for when a section is placed (it is the parameter `L` of the part).

Rejected: bundling an open parts dataset (none found whose licence and provenance were clear
enough to carry, and the families above need a few hundred numbers, not a database); writing a
file per size (DEC-048); a library on a server.

## DEC-050 Dimensions of one sketch that change together are set together (2026-10-11)

Found while building the library. A design table row, or one parameter that several
dimensions follow, changes several dimensions of a sketch at once. They were written one at a
time, each followed by a solve: after the first, the sketch held one new size and the rest old,
which is often a sketch that cannot be drawn (a slot wider than the bar it is in), and the
change was refused as a conflict though the row as a whole was fine.

Dimensions of one sketch that change in one step are now written together and the sketch is
solved once (`Sketch::set_dimensions`, `Document::set_values_at`). Equations are still
evaluated in dependency order; a sketch's values are held back until no equation still to be
evaluated writes into that sketch. `one_parameter_scales_three_dimensions_of_a_sketch_at_once`,
`a_design_table_row_sets_the_dimensions_of_a_sketch_at_once`.

## DEC-051 The 3D print check reads the part as triangles (2026-10-11)

`print.check` and Environments > 3D Print Check (where Inventor has its 3D Print environment) answer a maker's first questions before a slicer is
opened: is the surface closed, what overhangs, how thin is the thinnest wall, does it fit the
printer.

- **It reads the mesh, as a slicer will.** The part is tessellated and its triangles welded
  (`tenon_io::meshcheck`). Watertight means every edge joins exactly two triangles that face
  the same way; open, crowded and flipped edges are counted apart. Each finding is given by
  kernel face, so the app tints the faces and a command can act on them.
- **Overhangs** are faces leaning out past a limit measured from vertical (45 degrees unless
  said), given a direction that is up (one of the six axis directions). What lies on the bed,
  at the part's lowest point, is not an overhang.
- **Wall thickness** is measured through the material: from the middle of each triangle
  straight in to the far side. The least found is reported, and the faces under the minimum
  asked for (0.8 mm unless said, two passes of the commonest nozzle).
- **The bed** is three sizes; the part fits if its footprint does either way round and it is
  not too high. It is only checked when given.
- **It is an aid, not a verdict.** It says what it found ("wants support") and does not know
  the printer, the material or the slicer's settings.

In the app the check runs again whenever the part or its settings change, on the geometry
thread, and never while a sketch is open. Not done: support volume or print time; the build
space drawn round the part; turning the part freely; a list of printers; small features
(holes and gaps that close up), which need the nozzle's width; checking an assembly.

Rejected: checking the exact solid (a wall's thickness between curved faces is a hard exact
problem and the answer wanted is what the printer will get, which is the mesh); calling a
slicer (a dependency and a licence for a first answer).

## DEC-052 3MF beside STL, and files that are written keep their tolerance everywhere (2026-10-11)

- **3MF** (`export.3mf`, `asm.export_3mf`, File > Export 3MF) is the core specification only:
  millimetres, one object per solid with its triangles, a base material carrying the part's
  material name and colour, and for an assembly an item per component with its placement, the
  components of one part in one size sharing an object. It is a zip package (the `zip` crate
  Tenon already used) written with fixed dates, so the same model is the same file. No
  extensions (no slicer settings, no beam lattices, no textures).
- **STL** (`export.stl`, new `asm.export_stl`) leaves out triangles with no area or a corner
  that is not a number, refuses more triangles than the format can count, and says whether what
  it wrote is closed. An assembly's STL holds every solid where it is.
- **Watertight is judged solid by solid.** Two parts that touch share corners, and a welded
  mesh of both would call their common edges crowded; each is a closed solid and prints.
- **A written mesh keeps its tolerance inside freeform faces.** DEC-036 lets the inside of a
  swept, lofted or blended face deviate five times as far as its edges, which is right for the
  screen (shading uses exact normals) and wrong for a file that is the result: a 2 mm spring
  wire came out 1.7 % under volume. `MeshTol` now says which is wanted (`relaxed`, true for the
  display); exports mesh exactly (`MeshTol::exact()`), from scratch, and leave no triangles
  behind for the display to pick up. The spring is then 0.7 % under, which is what 0.2 rad
  chords round a wire lose, and `linear` and `angular` on the export commands mean what they
  say (`an_exact_mesh_keeps_the_tolerance_inside_freeform_faces`,
  `stl_and_3mf_files_hold_closed_solids_of_the_right_volume`).
- **In the app** both exports run on the geometry thread from the saved part, not from the
  triangles on screen (which may be showing a preview or an open sketch).

Not done: an options dialog for the tolerance in the app; units other than millimetres;
colour per face; reading 3MF or STL back in as a body.

## DEC-053 Smaller things settled in M7 (2026-10-11)

- **A screenshot run starts clean and leaves nothing.** `tenon --screenshot` was reading and
  writing where dialogs were last left (egui's memory, kept beside the user's settings), so a
  picture depended on who took it, and one bad layout was remembered. Such a run now starts
  with no remembered dialog positions and saves none.
- **A parameter cannot be named after a unit.** The nut's height is `m` in its standard and
  `height` in Tenon: `m` is metres in an equation.
- **The print check's result rows are laid out by hand,** not in a grid: a wrapped label in a
  grid column is measured a letter wide on a window's first frame.
- **The M7 example** is a printed end cap on a T-slot bar (examples/m7-end-cap): one part worth
  checking for printing both ways up, one section from the library with its length set, one
  screw placed from the library and changed for a longer one, and an interference that is
  wanted (the screw's thread in the bar's bore), checked against its analytic volume.

## DEC-054 The owner confirms M7; the sketch environment comes before M8 (2026-10-11)

Shown the M7 report, the owner answered "Look great let's keep going keep trying to make the UI
more and more like inventors". M7 is confirmed, with the three points put to them standing as
decided (sizes typed in as facts, DEC-049; a section's length is its part's parameter; what was
left out). M8 (sheet metal) is next in the plan.

The second half of the answer is the standing direction of DEC-042 again, so before M8 the place
an Inventor user spends most time in, the sketch, was gone through against Inventor's Sketch tab
button by button. A dozen buttons were on the ribbon and did nothing; the tool used most after
Line and Dimension, Project Geometry, was not there at all, and a sketch on a face could not be
dimensioned to the face's edges. DEC-055 and DEC-056 are what was done about it;
[inventor-fidelity.md](inventor-fidelity.md) has the tables.

## DEC-055 Edges of the part shown in a sketch: projected geometry (2026-10-11)

A sketch on a face had only the part's origin to hold on to. Now edges of the part are shown in
a sketch as **reference geometry**, as Inventor's Project Geometry does and as its sketches on
faces start out.

- **In the sketch** it is ordinary lines, circles and arcs marked `reference`: the solver never
  moves them (their points and radii are not unknowns), and the sketch's own geometry is
  constrained and dimensioned to them. They bound profiles like any curve. The sketch's own
  tools leave them alone (no trim, fillet, move); deleting one, or **Break Link**, ends it.
- **In the document** a sketch feature lists its projections: the entity and the edge, by the
  names of the edge's two faces (the same reference a fillet uses). The sketch depends on the
  features that made those faces.
- **At every regeneration** each edge is found again, what it is on the sketch plane is worked
  out (a line; a circle or an arc seen square on; a line for one seen edge on; a point for a
  straight edge seen end on), the reference geometry is put there and the sketch solved; the
  features after it use that. An edge that is gone leaves its geometry where it was, drawn in
  magenta, and the sketch says which (`sketch.info`); nothing fails for it.
- **The document follows.** The sketches of the document take up what regeneration found
  (`Session::follow_projections`), so the file, the sketch on screen and the next edit all have
  the edges where they are. It is not an edit: there is no undo step, and undoing whatever
  moved the part puts the sketch back with it. When following changes more than the sketch (an
  equation reading a driven dimension to a projected edge), the part is rebuilt once more.
- **A kernel is not needed to add one.** The app's kernel is on another thread, and it has the
  part's edges on screen: `sketch.project` and `sketch.create` take what each edge is on the
  plane from the caller (`geometry`), or work it out with the kernel when there is one. Either
  way regeneration is what decides where it is from then on. To tell which way round an arc
  goes, the kernel reports a point halfway along each edge.
- **A new sketch on a face shows the face's edges** when Application Options has "Autoproject
  edges for sketch creation and edit" ticked, which it is at first, as in Inventor. Scripts say
  so (`project_edges`); without it `sketch.create` is as it was.
- **What a feature uses when no profile is picked** is the sketch's own closed geometry before
  what it only shows of the part: a circle drawn on a face is the circle, not the face with a
  hole in it. (Inventor would have the user click; this is one click fewer, and the click is
  still there to take.) A sketch with nothing closed of its own gives the outline.
- **In the file** the additions are `reference = true` on an entity and `projections` on the
  sketch feature, written only where there are any. Format version 2 stands (DEC-035).

Not done: edges that would be ellipses or splines on the plane; vertices, work features and
other sketches' curves; edges projected while a curve is drawn over them; cut edges of a
section; the broken ones offered for repair by the dialog of DEC-034.

Rejected: copying the edge once and warning when the part moves (it is the part following that
makes it worth having); keeping the true positions only in the regeneration result (every edit
of the sketch would solve against stale ones).

## DEC-056 The rest of the sketch environment's second round (2026-10-11)

- **Centre lines.** A line can have the Centerline format (`centerline = true`): Revolve takes
  it for its axis without being told, and a dimension from it to a point or a parallel line is
  a diameter (the new `linear_diameter` dimension, drawn across the axis and read with Ø). It
  still bounds profiles, as Inventor's does, so a half section is closed along its own axis.
- **Constraint symbols are pictures** beside what they hold, one at each of two lines made
  parallel. Pointing at one lights it and its geometry; a click selects it; Delete removes the
  constraint. The part origin shown in a sketch is held without a symbol.
- **A dimension's value lies along its line** and reads from the left or from below.
- **Move, Copy, Rotate, Scale** work on what is selected: clicks for from and to, or values
  typed beside the pointer. What is joined to the selection follows through the solver; a
  fixed point or a dimension to the rest of the sketch refuses the motion and says so. Within
  what turns, level and upright constraints are dropped (by quarter turns, swapped); within
  what is scaled, dimensions scale. A copy keeps its shape (geometric constraints within it)
  and not its sizes. Inventor asks whether to relax constraints; Tenon does what that dialog's
  usual answer does and refuses the rest.
- **Extend and Split** go to the nearest curve in the way and the nearest crossing. A split
  line's length becomes the distance between its ends, and its pieces stay in line.
- **Sketch patterns are copies.** Rectangular and Circular make geometry of its own that does
  not follow the original afterwards. Inventor's are associative; that needs a pattern
  constraint the solver does not have, and is noted as a gap rather than faked.
- **Slots** (centre to centre, overall, centre point), **rectangles** about their centre and at
  an angle, **arcs** by centre and tangent to the end of a curve, in the drop-downs where
  Inventor has them. A slot is two lines tangent to two equal arcs, so it stays a slot.
- **Tangency at a shared end point** has an equation of its own in the solver. The general one
  (distance from centre equals radius) is at its turning point when the curves already meet,
  so the degrees of freedom of a fillet or a slot were counted two too many.
- **The Environments tab** holds 3D Print Check, where Inventor has its 3D Print environment.
- **The ribbon lists a panel's buttons in order,** so a large button can end a panel (Project
  Geometry ends Create, as in Inventor).
- **A sketch entered before its plane is known** (on a face, before the part is rebuilt) turns
  the view to it when the plane arrives; it used to stay as it was.
- **The app** takes `--edit-sketch NAME` and `--tab NAME`, for pictures of the sketch
  environment and of any ribbon tab.
- **`sketch.circle` returns its centre point** beside the circle, so a script can dimension a
  hole from a projected edge without asking the sketch what it holds.

Not done: Stretch; sketch text; ellipses; the Center Point format (every point of the sketch's
own that is on no curve is a hole centre, as before); automatic dimensions; polygons with other
than six sides from the ribbon.

## DEC-057 Sheet metal: a part with a rule, built from faces and flanges (2026-10-11)

M8 is sheet metal basics (docs/plan.md): base flange, edge flange, bends with K-factor, corner
relief, a flat pattern with DXF export. It is built round Inventor's Sheet Metal tab (DEC-054).

- **A sheet metal part is a part with a rule.** The rule says how reliefs are shaped; its sizes
  are **parameters of the part with fixed names**: `Thickness`, `BendRadius`, `BendReliefWidth`,
  `BendReliefDepth`, `MinimumRemnant`, `CornerReliefSize`, `KFactor`, as Inventor names its
  sheet metal parameters. Being parameters they take equations (`BendRadius = Thickness`),
  design table columns, and can be used anywhere a value is typed (a cut `Thickness` deep). They
  cannot be renamed or deleted while the part is sheet metal.
- **Features read the rule when the part is rebuilt**, so one thickness holds for the whole
  part and a change of it is a change of every face, bend and relief. The regeneration cache
  counts the rule as part of each sheet metal feature (`a_change_of_the_rule_rebuilds_the_part_from_its_first_sheet_metal_feature`).
- **Face** (Inventor's Face): a sketch profile one thickness thick.
- **Flange**: walls bent up from edges of the sheet's outline (DEC-059).
- **Cuts, holes and rounded corners are the ordinary features.** The flat pattern is worked out
  from the solid (DEC-058), so nothing sheet-metal-specific is needed of them. Cut on the Sheet
  Metal tab is an extrusion that removes its profile one `Thickness` each way from its sketch.
- **Any part can be a sheet metal part** (Convert to Sheet Metal is Sheet Metal Defaults): a
  solid of one thickness made by other means is laid flat like any other.
- **In the file** a sheet metal part has a `[sheet_metal]` table and the two feature types
  `sheet_face` and `flange`; a part that is not sheet metal is written as before. Format
  version 2 stands (DEC-035).

Not done in M8, and on the ribbon as buttons that say so: Contour Flange, Lofted Flange,
Contour Roll, Hem, Bend (between two faces), Fold, Corner Seam, Punch Tool, Rip, Unfold and
Refold. A face is not yet joined to an edge of the sheet by a bend of its own.

Rejected: a thickness on every feature (one part, one sheet); a separate kind of document for
sheet metal (Inventor has a template, not a file type, and so does Tenon: File > New Sheet Metal
Part is a part with the rule already given).

## DEC-058 The flat pattern is unfolded from the solid (2026-10-11)

There were two ways to a flat pattern: keep a tree of plates and bends beside the solid and
lay that out, or read the solid. Tenon reads the solid, as Inventor does.

- **How.** One side of the sheet (the A side) is followed from a flat face across every edge
  where the next face runs on smoothly: a flat face in the same plane, or one side of a bend
  (a round face with a partner about the same axis one thickness away, so the rounded rim of a
  sheet and the wall of a hole are not bends). Each face is given its place in the plane: flat
  faces by a rigid move, bends unrolled to the length of their **neutral surface**, angle x
  (inside radius + K x thickness). The edges left on the rim are the outline.
- **Why.** Whatever made the solid, what is cut is what the model shows: holes, cuts, rounded
  corners and parts converted from ordinary solids need nothing of their own. A tree of plates
  would have needed every later feature taught to it.
- **It is checked, never trusted.** The faces of the side, each one thickness deep, must account
  for the solid's volume to within 2 %; a solid that is something else is refused in plain
  words. An outline that does not close, a sheet that closes on itself (a box section) and a
  pattern that overlaps itself are each said as what they are
  (`what_cannot_be_laid_flat_is_refused_in_plain_words`).
- **Exact where the part is.** Lines stay lines and circles stay circles and arcs; pieces of
  one line or one circle that the kernel split are one curve again. Only a cut that runs across
  a bend is a run of points (the kernel's new `edge_points`).
- **Outlines that touch.** A relief that reaches the corner between two bends touches the
  outline at a point. It is a cut-out of its own, whichever way the curves are followed: at
  such a point the way on is across a gap in the sheet, never through the point.
- **Which side is up** is the part's choice (Define A-Side), kept as a reference to a face;
  without one it is the largest flat face, and of two equal ones (the two sides of a plate) the
  one that looks up. Bends towards whoever looks at the pattern are **up**, the others **down**.
- **What it says:** the outline and cut-outs, each bend's centre line, extent lines, angle,
  inside radius and allowance, the size of the sheet it is cut from, its area, and how far a
  cutter travels.

Not done: bend tables (K factor only, one for the part); a flat pattern that can be turned on
the sheet or edited as a solid (cosmetic centre lines, bend order); more than one solid in a
part.

## DEC-059 Flanges: where the bend goes, and what is cut for it (2026-10-11)

A flange is a bend and a wall along straight edges of the sheet's rim, built as one extruded
cross-section per edge and joined to the sheet.

- **It bends towards the side its edge is on** (the top edge of the rim: up), or the other way
  when flipped, as in Inventor.
- **Height** is measured from where the outer faces of the sheet and the flange would meet
  (the overall height; the default), from where their inner faces would, or from the plane
  that touches the outside of the bend.
- **Bend position**: *inside* (the flange's outer face goes through the edge, so the part gets
  no bigger and the sheet is cut back for the bend; the default), *from the edge* (the bend
  starts at the edge and nothing is cut), *outside* (the inner face goes through the edge) or
  *tangent* to the plane of the edge.
- **Width**: the whole edge, a width in the middle of it, or offsets from its ends. An edge is
  taken clockwise round the sheet, seen from the side the flange bends to.
- **Bend relief**, where the flange is narrower than its edge and the bend is cut into the
  sheet: a slot `BendReliefWidth` wide beside the bend, `BendReliefDepth` deeper than it,
  straight or round ended. A strip left between a relief and the end of the edge narrower than
  `MinimumRemnant` goes with it.
- **Corners.** Edges of one flange that meet at a corner of the sheet each start where the
  other's bend line crosses its own, so the bends never share space, and the corner gets the
  rule's relief. What the relief is, and how the two walls close the corner, is DEC-063
  (which replaced what stood here: reliefs cut in the flat of the sheet only, touching the
  bend lines, and walls that ended where their bends did).
- **Names.** The faces a flange makes are named after the edge it is on and the part they are
  (wall, tip, bend, ends), so a flange on a flange, or a sketch on a wall, is found again when
  the sheet changes. A script names such an edge or face by a point on it
  (`model.edge_ref` and `model.face_ref` take `at`).
- **What cannot be** is said: an edge that is not on the rim of the sheet, a curved edge, a
  flange lower than its own bend, one wider than its edge, two that meet in an inside corner,
  one that runs into the part.

Not done: a tear relief (no slot at all); an angle or a height of its own for each edge of
one flange; height to a point; flanges on curved edges.

## DEC-060 The flat pattern as DXF and SVG (2026-10-11)

- **DXF** is R12, the version every cutting program reads, in millimetres, with the pattern's
  lower left corner at the origin (some machines want everything in the positive quadrant).
  Lines, arcs, circles; a polyline only for a cut across a bend.
- **Layers of Tenon's own naming**: `OUTER`, `INTERIOR`, `BEND_UP`, `BEND_DOWN`,
  `BEND_EXTENT`, each with a colour, so that a cutter's software can treat them apart and bend
  lines can be turned off or etched. The vendor's layer names are the vendor's.
- **Numbers to a millionth**: more than a cutter can use, and the same file from every
  machine.
- **SVG** at full size (a millimetre is a millimetre), for a look or a paper template.

Not done: an options dialog (other DXF versions, layer names, leaving layers out beyond the two
switches); bend notes as text; nesting several parts on a sheet.

## DEC-061 Sheet metal in the app (2026-10-11)

- **The Sheet Metal tab is a sheet metal part's own.** It comes first and is the tab a finished
  sketch returns to; an ordinary part does not show it, and has Convert to Sheet Metal on its
  3D Model tab instead. Its panels are Inventor's: Sketch, Create, Modify, Work Features,
  Pattern, Setup, Flat Pattern.
- **Sheet Metal Defaults** is a window of the rule: each size a box that takes a number or an
  equation and shows what it comes to. OK is one undo step.
- **Face and Flange** are panels like Extrude's: sections, values that take equations, a live
  preview, OK, Cancel and Apply. A flange's edges are clicked in the part.
- **Create Flat Pattern** gives the part a flat pattern and shows it in place of the folded
  part, with a tab of its own (export, what to show, Go to Folded Part); from then on the
  button reads Go to Flat Pattern, and the browser has Folded Model and Flat Pattern at its
  top, as Inventor's does. The pattern is worked out on the geometry thread and again whenever
  the part changes.
- **The flat pattern is drawn, not modelled**: the sheet filled in the part's colour, its
  outline, bend lines (up green, down red, each with its angle and radius), and the size of
  sheet it is cut from. Inventor shows a flat solid that can be worked on; Tenon's cannot be,
  and what is cut is cut in the folded part.

## DEC-062 Smaller things settled in M8 (2026-10-11)

- **The kernel gives points along an edge** (`edge_points`), from its start to its end within
  a deflection: for curves that are neither lines nor circles.
- **An edge or a face is named by a point on it** (`at: [x, y, z]`), for scripts and agents:
  faces made by fillets, flanges and the like have names a script cannot spell. A point where
  several meet is refused, saying to give another.
- **Filling a shape with holes** for the painter is done in level strips, each a row of
  trapezia: holes that touch the outline or each other are no special case.
- **The M8 example** is a tray for a circuit board (examples/m8-tray): a face, four walls in
  one flange with round corner reliefs, a tab that is a flange on a flange with bend reliefs,
  holes, a window, and its flat pattern as DXF, every number worked out by hand in its README.

## DEC-063 Corners between flanges: mitred walls, and reliefs laid out in the flat (2026-10-11)

The owner asked for the gap with Inventor's sheet metal to be closed before M9, the corners of
a tray first. This replaces what DEC-059 says of corners and of reliefs being cut in the flat
of the sheet only.

Where two edges of one flange meet at a corner of the sheet:

- **The bends** end where their two bend lines cross, as before: square to their own edge, so
  they never share space, at any corner angle.
- **The walls are mitred** (Auto-Mitering in the Flange panel, on unless unticked;
  `miter = false` in the file). Each reaches on towards the corner, or draws back from it,
  until the part's `GapSize` (a new parameter of the rule: Miter Gap in Sheet Metal Defaults,
  one `Thickness` at first) is left between the two walls' inner faces, all the way up: the
  two stop either side of the plane that halves the corner, half the gap from it. Each end is
  cut square through the sheet, as a cutter cuts. Upright walls at a square corner end
  `Thickness + GapSize / √2` from the corner; walls that lean in draw back as they rise, walls
  that lean out reach on past the corner. Not mitred, each wall ends where its bend does, as in
  M8.
- **The relief** is laid out in the flat sheet and centred where the centre lines of the two
  bends cross, as Inventor's default is: *round*, a disc `CornerReliefSize` across; *square*,
  a square that size with sides along the two bend lines; *trim to bend*, none. It takes the
  corner of the floor, the ends of the two bends and the feet of the two walls, as much of
  each as it reaches. A relief too small to reach any of them changes nothing.

How it is built. The pieces of a flange that meets another are no longer one extruded
cross-section. Its bend and its wall are each an outline in the flat (a rectangle; a
trapezium for a mitred wall) with the relief taken out of it by plain 2D clipping
(`sheet::outline`); the wall is that outline made one thickness thick, the floor is cut by the
relief's own outline, and the bend is its outline **rolled** round the bend's axis.

- **`Kernel::roll`** is the new kernel operation: a flat outline rolled round an axis, each
  point `x / rho` radians round it, between an inner and an outer radius. Its sides are square
  to the sheet everywhere, so what was a circle in the flat is that circle again when the bend
  is unrolled. Lines along the axis and round it stay planes; anything else (the arc of a round
  relief) is a ruled surface between the two cylinders, which OpenCASCADE keeps as a spline.
  The backend builds the faces itself and sews them (docs/architecture.md).
- **The flat pattern gives arcs back as arcs.** An edge cut across a bend comes back from the
  kernel as points. Where they lie on the circle of an arc beside them they are more of that
  arc; on their own, on any one circle or line, they are that arc or line (to 0.01 µm;
  `sheet::flat`). So a tray's corner relief is one arc in the DXF, from wall to wall.
- **The folded part now depends on the K factor**, slightly: the relief is placed by the bend
  allowance. That is the price of laying it out in the flat, and is what Inventor does.
- A flange on one edge, and the ends of a flange that meet no other, are built as before.

Not done: a relief of its own for one corner, the seams (overlap, reverse overlap), tear and
weld reliefs, three bends meeting at a point. Edges that meet in an inside corner are still
refused.

## DEC-064 Hems and contour flanges are strips (2026-10-11)

Both are sheet that follows a path seen end on: straight runs joined by bends. `sheet::strip`
makes the cross-section of such a strip (the path offset to both faces of the sheet, each bend
two arcs about one centre a thickness apart), refuses a bend tighter than the sheet can follow
and a strip that comes back through itself, and the kernel pushes it out as one solid. So the
bends are true cylinders and the flat pattern needs nothing new.

- **Hem** (`hem`; Sheet Metal > Create > Hem). The path starts at an edge of the sheet's rim,
  along the face the hem folds towards. The types are Inventor's: *single* (folded flat back,
  `gap` between the faces, `length` over all), *double* (the hem's own end folded in under it:
  the outer fold goes round the inner layer and a gap either side), *rolled* (a bend of
  `radius` through `angle` and nothing after it) and *teardrop* (rolled past half a turn, then
  straight back down to the sheet). Its width is the whole edge, a width in the middle, or
  offsets, as a flange's. Nothing is cut from the sheet: the fold starts at the edge, so the
  part grows by it.
  - Inventor's help says what the fields are but not how the length is measured or where the
    fold lies; Tenon's choices are its own: the length is over all, from the outside of the
    fold, and the fold starts at the edge.
  - A new hem's sizes are equations of the rule (`Thickness * 0.5`, `Thickness * 4`,
    `BendRadius`), so it follows the sheet.
  - A teardrop stops 0.01 mm short of the sheet: touching it, the part would be a solid that
    meets itself along a line, which neither the kernel nor the flat pattern takes well.
- **Contour Flange** (`contour_flange`; Sheet Metal > Create > Contour Flange). The path is an
  open profile drawn in a sketch: its lines and arcs, end to end (construction and projected
  geometry aside). A corner between two lines becomes a bend with the part's `BendRadius`
  inside (or the feature's own); an arc is a bend as drawn and must run on smoothly from its
  neighbours. The profile is pushed `distance` out of its plane: in front, behind, or half each
  way. The sheet goes on the inside of the profile's first corner (the profile is then the
  outside of the part, which is how a channel is usually dimensioned), on the outside, or half
  each side. It joins the sheet it touches or is a solid of its own.
  - Not done: a contour flange along edges of an existing face with a bend made to join it
    (Inventor's edge sets), and its widths between two points.

## DEC-065 Smaller things settled with the corners (2026-10-11)

- **The properties panel takes the height it needs.** It had 58 % of the left column and
  scrolled beyond that, which hid the last sections of the Flange panel on a laptop screen. It
  now grows to what its content needs while the browser keeps a few rows.
- **`sketch.arc` and `sketch.arc3` return the arc's points** (`center`, `start`, `end`), as
  `sketch.line` does, so a script can start the next curve where an arc ended.
- **A part saved before there was a `GapSize`** opens: the gap is one thickness until Sheet
  Metal Defaults is confirmed, which adds the parameter.
- **A test of the background worker** no longer depends on how fast the machine is
  (`Worker::regenerating` says which slot is being worked on), and the M8 example's test
  compares files without regard to line ends: both had failed on one CI platform only.
- **The M8 example** gets the mitred corners and centred reliefs by being regenerated, and a
  hem along the top of its back wall; its README works the new numbers out by hand.

## DEC-066 Corner Seam, and corners with a seam of their own (2026-10-11)

Asked for by the owner after the second round: Corner Seam next.

**Seams.** Two walls at a corner are brought together in one of three ways (`sheet::Seam`):
*symmetric* (both stop the same way short of one another, the gap between their inner faces:
what DEC-063 called mitred), *overlap* (the first runs on across the end of the second, as far
into its thickness as `overlap` says, 1 by default; the second stops the gap short of the
first's inner face) and *reverse overlap* (the second runs across the first). These are
Inventor's Symmetric Gap, Overlap and Reverse Overlap, with its Percent Overlap. One function
(`flange::seam_end`) places a wall's end for any pair of walls: unlike radii, heights and
angles included, since each wall's end follows the other wall's faces.

- The gap is measured between the walls themselves (Inventor's Maximum Gap Distance, its
  default); the older Face/Edge Distance is not offered.
- Ends are cut square through the sheet. Where two walls are not square to one another a wall
  stops where the first of its two faces reaches the other, so nothing ever passes into the
  other wall.

**A flange's own corners** (`flange` gains `seam`, `overlap`, `gap`, `relief`, `relief_size`;
the Corner section of its panel). They apply to every corner the feature makes; the first of
two walls is the one whose edge comes first in the feature. Without them a corner has the
rule's gap and relief and a symmetric seam, as before; Auto-Mitering off still leaves each wall
ending where its bend does. Inventor has these per corner in a table; a corner of its own in
Tenon is a Corner Seam on it.

**Corner Seam** (`corner_seam`; Sheet Metal > Modify > Corner Seam) closes the corner between
two flanges that were made apart. One after the other, the first flange runs the whole length
of its edge, bend and all, and the second starts where that bend left its own edge: the corner
is open by a bend's radius, with the end of the first bend through it. Given the edge at the
end of each of the two walls, the feature

1. reads from the solid what each wall is: the bend along its foot (a round face with a
   partner one thickness away), the flat that bend comes up from, the bend's radius and
   angle, the wall's height;
2. finds where the two bend lines cross, and cuts away what of either flange runs on past it;
3. brings the walls to the seam, adding to a wall or cutting it back;
4. cuts the relief, laid out in the flat as a flange's is, out of the flat, both bends (as a
   rolled outline: `Kernel::roll` makes the cutting tool here) and both walls.

So it works on what is there, whatever made it, and the corner it leaves is the corner one
flange makes of the same two edges: the same solid and the same flat pattern (the test
compares them, for every seam and relief).

- **The piece put on a wall is the wall.** `Kernel::unify` (new) makes faces that lie in one
  plane one face, across the edges of the faces a feature added and nowhere else; such a face
  keeps the name the part had for it. A sketch or a flange on the wall still finds it.
- **Square corners only**, between two faces bent up from one flat, at an outside corner. At
  any other angle the bends of two flanges made apart already share space; edges of one
  flange close their own corners at any angle (DEC-063).
- Not done: Corner Rip (opening the corner of a solid made some other way), the mitre
  between two faces in one plane, Extend Corner (aligned or perpendicular), the Face/Edge
  gap, No Seam as a choice in the feature (a corner without a seam is one without the
  feature).

## DEC-067 Sheet cuts are made one tool at a time, and checked (2026-10-12)

The first push of Corner Seam (DEC-066) was green on Linux and Windows and red on macOS, in
one case of its test only: a round relief 12 across with an overlapped seam came out 25 mm³
short of the same corner made by one flange. Read step by step on the macOS runner:

- each of the five relief tools (the flat's prism, the two rolled bend tools, the two wall
  prisms) taken out alone, or one after the other, leaves the right solid on every platform;
- all five in one boolean, or made one tool first and then cut, leaves a *sound* solid of the
  wrong volume there, and not the same wrong volume from one run to the next (44 mm³ too much
  in the second run).

The tools of a relief lie against one another along the bend lines, face on face, and their
curved faces run on into one another there. A boolean with several such tools is where the
kernel's answer depends on the platform; nothing in the result says so (it is valid, one
solid).

**Decision.** Everything the sheet metal features take out of the part (`sheet_cut`: a
flange's and a corner seam's trims, a corner seam's relief) goes one tool at a time, and each
time what went is compared with what the tool shared with the part (their common volume, by a
boolean of its own): a cut that took anything else fails the feature with a message, instead
of leaving a part that is silently wrong. Where the common volume cannot be had, the cut must
at least take nothing negative and no more than the tool. `Kernel::unify` after a corner seam
is taken only when it leaves the same solid (same volume, one solid).

- This costs two booleans a tool where there was one for all. The parts are small; M9 will
  measure it.
- Joining pieces on was already checked (nothing shared, one solid) and stays one boolean.
- Other features' booleans with many tools (a pattern's cuts) are tools that stand apart;
  they are left as they are.
- A cause that shows on one platform only is found on that platform: a draft pull request
  from a throwaway branch ran the one test there with its steps printed, and was closed.

## DEC-068 Cut Across Bend, and Project Flat Pattern (2026-10-12)

Asked for by the owner on Corner Seam's report: Cut Across Bend next.

**What it is.** In Inventor a Cut with Cut Across Bend takes a profile drawn on a face of the
sheet and wraps it across the faces and bends it lies over, Thickness deep unless told
otherwise; in the flat pattern the cut is the profile as it was drawn. Tenon's is the same
(`cut_across_bend`; Sheet Metal > Modify > Cut with Cut Across Bend ticked):

1. the sheet is laid flat from the face the sketch is on, *in the sketch's own coordinates*:
   that face stays where it is, a bend beside it is its allowance wide, the face beyond lies
   past that. This is the unfolding the flat pattern already does (`sheet::unfold::follow`),
   started from the sketch's frame;
2. every flat face and every bend of that side of the sheet takes the part of the profile
   that lies on it (`sheet::region::common`: what two regions bounded by lines and arcs have
   in common, with holes, written for this);
3. a face's share is cut as a prism in the face's own plane, a bend's share as the outline
   rolled round the bend's axis (`Kernel::roll`, as a relieved corner is), each one at a time
   and checked (DEC-067).

So a round hole drawn across a bend line is a circle in the flat pattern, where the same
circle extruded through the part is an arc and a run of points (the M8 test still shows that).

- **The face the sketch is on** is the face it was put on. A sketch on an origin or work plane
  that lies on a side of the sheet works as well: of the sides in its plane, the one with
  most of the profile on it. The rim of the sheet is no side (its other side is not one
  thickness behind it), so a sketch there is refused in words.
- **Depth** (`distance`): from the side the sketch is on. Without one, or at the thickness or
  more, the cut goes through, whatever the thickness becomes. Less is a pocket: on a bend it is
  that much of the thickness from the side the sketch is on, inside or outside. The panel's
  field starts as the equation `Thickness`, as Inventor's does.
- **A face's own outline** bounds what is cut in it (not its holes: a hole is no sheet, and the
  floor of a pocket is sheet like any other). A bend is taken as far round as it turns and as
  far along as it reaches. What of the profile lies on no face and no bend cuts nothing; none
  of it on the sheet is an error.
- **Profiles** are lines, arcs and circles. A profile with an island keeps the island where it
  is on a flat face or across a bend line; an island that lies wholly on a bend is refused
  (the rolled outline has no holes yet), as is a spline.
- **Names.** A wall of the cut is named after the curve of the profile that cuts it and the
  face of the sheet it is cut in. A circle on a flat face is one wall, as an extruded one is.
- **The Cut panel.** Sheet Metal > Modify > Cut now has a panel of its own: the profile, Cut
  Across Bend, the distance. Unticked it makes what it made before, an extrusion that cuts one
  distance each way from its sketch (an ordinary Extrusion in the browser, edited as one);
  ticked it makes a Cut. A Cut stays a cut across bends when edited.

**Project Flat Pattern** (Sketch > Create, under Project Geometry; `sketch.project` with
`flat`). Inventor's workflow starts there: the faces beyond a bend are shown in the sketch
where they lie unfolded, and the profile is drawn on them. A projection (DEC-055) gains
`flat`: its edge is shown where the sheet has it when laid flat from the sketch's face, by the
same unfolding the cut uses, and follows the part like any projected edge (a higher wall's top
edge moves out). Straight edges, edges round a bend (straight in the flat) and circles on flat
faces; an edge of the sheet's other side has no place on this one. In the app the kernel is on
the geometry thread, so a click on a face asks it for the edges laid flat and the answer goes
into the sketch as the command's `geometry`.

In files: the feature type `cut_across_bend` (`sketch`, `regions`, `distance` when it has one)
and `flat = true` on a projection; additions to version 2 (DEC-046).

Not done: Cut Normal as a choice (the cut that is not across bends is the extrusion it was);
the projected flat pattern of a whole face in one go with its bend centre lines (the bend's
extent lines are edges and are shown; its centre line is not an edge); a pocket in the flat
pattern drawing (the pattern follows one side of the sheet: a pocket on that side shows as a
hole, one on the other side not at all, as for any pocket).

## DEC-069 The volume of a solid is asked for to a precision (2026-10-12)

Found by the first round hole cut across a bend: the cut and the volume the tool shared with
the part, both from the kernel, differed by 0.03 mm³, and both were 0.1 mm³ from the hand
formula. The solid was right (its mesh agreed with the formula); its volume was not.

OpenCASCADE sums a solid's volume face by face. The plain call does that with a fixed number
of points, and is off where a face of a round surface has a rim that curves back on itself:

- a rolled half disc (an arc of half a turn on a bend): 0.2 % of its volume, an arc of 5.5
  rad 0.9 %; an arc under about 2 rad, as a corner relief's is, under a millionth;
- a hole of radius 3 drilled across a shaft of radius 10: 0.009 mm³ of its 559, which is
  0.7 millionths of the part. So the mass of quite ordinary parts was a little out.

**Decision.** `Kernel::mass_properties` asks for the volume to a relative precision of 1e-9 a
face (`BRepGProp::VolumeProperties` with `Eps`, the adaptive sum). The same cases are then
right to 1e-9 against the formula (`the_volume_is_right_where_the_rim_of_a_round_face_curves_back_on_itself`,
`half_a_disc_and_most_of_one_roll_into_the_volume_their_area_has`). Every volume the model
tests check by hand kept its value; no test or example changed.

- Cost: on a sheet metal part of 25 faces a call went from 3.4 ms to 7.9 ms (5.2 ms at 1e-6).
  Regeneration is far longer; M9 will measure it in a whole session.
- Face areas are left as they are: they are not what was wrong, and a face's area is part of
  how a face is found again (its fingerprint).
- Two things tried first did nothing: shorter spans in the rolled arc's curve, and lower
  degrees in the curves of the rolled side face.

## DEC-070 Bend between two faces (2026-10-12)

Asked for by the owner on Cut Across Bend's report: Bend between two faces next.

**What it is.** In Inventor a Bend joins two sheet metal faces that do not touch: a model edge
is picked on each, and the faces are trimmed or extended as needed to make the bend. Faces
that are parallel but not in one plane take a Double Bend, of one of four kinds. Tenon's is
the same (`bend`; Sheet Metal > Create > Bend; `sheet.bend`).

**Pieces of sheet.** A Face made apart from the rest (Join, touching nothing) is a second
piece of the same solid body: that was so already, and is what a Bend is for. Sheet features
no longer ask that the body be one piece, only that what they add is joined to it, so a
flange or a hem goes on one piece while others are loose. A Bend makes two pieces one, and
says so when it does not. The two faces may also be on two solids (a Face made as a New
solid): the second solid is then joined into the first.

**The strip between the faces** (`sheet::join`). Seen end on, in the cross-section of the
first face at its edge, the bend is a strip of sheet that sets off out of the first face along
its side and arrives going into the second face along the side of it that is the same side of
the sheet (whichever edge of the second face's end was picked). It is built as a hem is: its
cross-section pushed along the edges, as wide as the two edges lie side by side.

- **Planes that cross: one bend**, tangent to both faces, turning from the way out of the
  first to the way into the second about where their sides would cross. Each face is extended
  to where the bend begins or cut back to it, over the width of the bend.
- **Parallel planes, side by side** (edge towards edge): an S of two bends, one the inside
  of its bend and one the outside. *Fix Edges* (the default, as Inventor's): both edges stay,
  and the two equal bends turn as far as it takes for them and the face between them to reach
  from one to the other. *45 Degree* and *90 Degree*: bends of that angle with a face between
  them (at 45, bends that turn less and are tangent when the faces are too close for a face
  between); the first edge stays and the second face is trimmed or extended to the bends, or
  the other way round with Flip Fixed Edge.
- **Parallel planes, one over the other** (edges the same way): a U. *Full Radius*: half a
  circle whose inside is as wide as the faces are apart (its radius is not the rule's). *90
  Degree*: two quarter turns with a face between them. One face is trimmed or extended to
  where the U is, by Flip Fixed Edge the first.
- What does not fit is said with the numbers: faces too close for two bends of this radius,
  a kind of double bend that is for the other arrangement, faces in one plane, edges that are
  not parallel or not beside one another.

**The piece put on a face is the face**: what the bend adds in the plane of a face is made one
with it (`Kernel::unify`, as a corner seam's is), and the face keeps its name. The flat
pattern needs nothing new: it is unfolded from the solid.

In files: the feature type `bend` (`edges`, optional `radius`, `double` when it is not
`fix_edges`, `flip_fixed`); an addition to version 2 (DEC-046).

Not done: Bend Extension aligned to the side faces (the bend's ends are square to it); a
relief where a face is wider than the bend and is cut back (it is cut square, as a tear
would be); the Face command's own bend to an edge it is drawn against; Inventor's Bend Edit
of a bend's width (the bend is as wide as the edges overlap); what Inventor does at Fix
Edges when the edges are too close for the rule's radius (Tenon refuses, and says which
kinds move an edge).

## DEC-071 Fold (2026-10-12)

Asked for by the owner on Bend's report, with Corner Rip, Contour Flange along edges and Cut
Normal: all four at once.

**What it is.** In Inventor a Fold bends a sheet metal face along a straight sketched line
that ends at the face's edges, and adds no material; the line is the centre, the start or the
end of the bend, and Flip Side and Flip Direction say which side folds and which way.
Tenon's is the same (`fold`; Sheet Metal > Create > Fold; `sheet.fold`).

**How.** The bend's allowance of sheet (angle x (radius + K x thickness), as the flat pattern
has it) is cut out of the face at the line, as long as the line. That leaves the sheet in two.
The piece on the side that folds (told from the other by a point just past the cut) is brought
up to the cut and turned about the bend's axis; the bend, a strip built as a hem's is, joins
the two again. So everything on the sheet beyond the line goes with it, flanges and all, and
laid flat the part is exactly what it was: the test compares the flat pattern before and after.

- **The side that folds** is the one to the left of the line as it was drawn (from its start
  to its end), seen from the side of the sheet the sketch is on; Flip Side takes the other.
  Inventor shows the side with an arrow and does not say how it chooses; by the line's own
  direction it does not change when a dimension makes one side the larger.
- **Up** is towards the side of the sheet the sketch is on; Flip Direction folds down.
- **Location**: the line is the middle of the bend (the default, as there), its start (the
  bend lies on the side that folds) or its end (on the side that stays).
- A line that does not reach from edge to edge leaves the face in one piece and is refused,
  as is a fold that would run what folds into the rest (the volume says so).
- The sketch is on a side of the sheet: a face of it, or a plane that lies on one, as for a
  cut across bends (DEC-068).

In files: the feature type `fold` (`sketch`, optional `line`, `angle`, optional `radius`,
`location`, `flip_side`, `flip_direction`); an addition to version 2 (DEC-046).

Not done: a line that is not straight; several lines in one feature; picking the line in the
viewport (the panel takes the sketch, and the feature its one line, or the one named).

## DEC-072 Contour Flange along edges of the sheet (2026-10-12)

The second of the four asked for on Bend's report.

**What it is.** In Inventor a Contour Flange either pushes its open profile out of the sketch
by a distance, or runs it along edges picked on an existing face, with width extents. Tenon
had the first (DEC-064); a contour flange now takes `edges` as well (Sheet Metal > Create >
Contour Flange: click an edge of the sheet; `sheet.contour_flange` with `edges`).

- **The profile's sketch is square to the first edge** (the edge runs straight through its
  plane, and shows in it as a point) and **the profile starts at that edge**. Either end of
  the profile may be the one there. Anything else is refused, saying how far off it is.
- **The side of the sheet the edge is on runs on into the profile**: seen end on, the profile
  is that side of the sheet continued. Where it leaves the edge at an angle there is a bend
  of the rule's radius (or the flange's own), for which the sheet is cut back to where the
  bend begins, exactly as a Bend trims a face; where it leaves straight on, nothing is cut and
  what is added is made one face with the sheet. Every further corner is a bend, as in a
  contour flange pushed out of its sketch. (`distance`, `direction` and `position` say
  nothing here: the thickness is where the sheet's is.)
- **Along each edge given** the same profile is used, seen from that edge: on the opposite
  edge of a plate it is the mirror image. How much of an edge it takes is a flange's `width`
  (the whole edge, a width in its middle, offsets from its ends).

In files: a `contour_flange` gains optional `edges` and `width`, and its `distance` is not
written when it has edges; additions to version 2 (DEC-046).

Not done: mitres where the flanges of adjacent edges meet (they must stop short of the
corner, by a width or offsets, or the flange is refused for running into itself); loop
select; a relief where the sheet is cut back beside a narrower flange (it is cut square);
From To and Distance extents.

## DEC-073 Cut Normal (2026-10-12)

The third of the four asked for on Bend's report.

**What it is.** In Inventor's sheet metal Cut, Cut Normal projects the profile onto the faces
of the sheet and cuts square to each face it lands on, so a sketch that is not parallel to
the sheet still leaves walls that are square to it. A plain Cut of the same sketch goes along
the sketch's normal and leaves slanted walls that a laser cannot cut from the flat sheet.
Tenon has it as a feature of its own, `cut_normal` (Sheet Metal > Modify > Cut, with Cut
Normal ticked; `sheet.cut_normal`).

- **Where the profile is seen.** The profile is looked at along its sketch's normal. Of the
  flat sides of the sheet that look towards the sketch, the one with most of the profile over
  it says which side of the sheet is cut (the nearest to the sketch, of equals; the nearest
  to the profile when it is over none of them, as a hole in a bend alone). Every flat face
  and every bend of that side that looks at the sketch takes what is seen on it; a face seen
  edge on, or from behind, takes nothing. `distance` is how deep from that side (default and
  at most the Thickness: through).
- **Each face is cut square to itself**: a flat face by a prism along its own normal, a bend
  by an outline rolled round its axis, as a cut across bends is (DEC-068). The two differ in
  where the profile lies: across bends it is drawn on the sheet laid flat, here it is where
  it is seen on the folded part. On a bend seen more and more edge on, the hole is longer in
  the flat pattern than the profile that was drawn.
- **Curves.** On a face parallel to the sketch the profile is itself, lines and arcs. On a
  face at an angle its lines are lines, and an arc is an oval; on a bend everything is a
  curve of its own. Those are worked out point by point (the profile is first divided where
  it crosses the straight edges of the sheet as the sketch sees them, so two faces that
  share an edge have the very same point of the profile on it) and given to the kernel as
  cubic splines through the points, evenly spaced along the curve: within a millionth of the
  curve's size. A run of points along one straight line is a line.
- **Cut Across Bend implies it.** In the panel the Cut Normal box is ticked and not for
  changing while Cut Across Bend is, as in Inventor: a cut across bends is always square to
  the sheet.

**The kernel's volumes.** OCCT sums the volume of a solid with walls of free curves far less
closely than one of flats and rounds: off by up to about five parts in ten thousand of such
a cut, where the others are right to a billionth. Its Gauss-Kronrod routine is exact but
took minutes on these parts, so it is not used. Two things follow: the check that a sheet
cut took what its tool shares with the part (DEC-067) allows a hundredth of the tool, which
still finds a tool that was not taken or took something else; and the mass of a part with
such a cut is right to about a hundred thousandth, not a billionth. The areas of its faces
are right to a millionth, and the tests measure the cut by those.

In files: `type = "cut_normal"` with `sketch`, `regions` and an optional `distance`; an
addition to version 2 (DEC-046).

Not done: an island of the profile on a bend (refused, as across bends); splines in the
profile; the flat pattern shows these curves as chains of short lines (within 0.005 mm), not
as splines; From To and To Next extents.

## DEC-074 Corner Rip (2026-10-12)

The fourth of the four asked for on Bend's report.

**What it is.** Inventor's Corner Seam has a second shape, Corner Rip: it opens a square
corner where two faces meet with no seam between them, "typically on a converted part
model": a box that was shelled and then made sheet metal has such corners. Tenon has it as a
feature of its own, `corner_rip` (Sheet Metal > Modify > Corner Seam, with Corner Rip chosen
under Shape; `sheet.corner_rip`). In the browser it is a Corner Rip, with Corner Seam's icon.

- **The edge.** Each edge given is the edge of a corner between two walls: flat faces that
  are both sides of the sheet (each with the sheet's other side one thickness behind it) and
  stand square to one another. The outside edge of the corner or the inside one will do;
  which it is is found by looking for sheet a little way to either side of it. The rip is
  as long as the edge: on a shelled box the outside edge runs down through the floor, and
  the floor loses its corner with the walls; the inside edge starts at the floor, and the
  floor stays whole. An edge of the rim, a curved edge and a corner that is not square are
  refused, each saying why. One rip may open several corners.
- **The seam** is a corner seam's (DEC-066), with the same gap (the part's `GapSize` unless
  the rip has one) measured between the walls. *Symmetric*: both walls stop where their
  inner faces are the gap apart (the thickness and gap / sqrt 2 from the outside of the
  corner), and the corner between them goes. *Overlap*: the first wall keeps the corner, as
  far into the second's thickness as `overlap` says, and the second stops the gap short of
  its inner face; *reverse overlap* is the other way round. The first wall is the one
  whose face the edge reference names first, which is settled by the faces' names, not by
  the click: the two overlaps are there to be tried.
- **No gap is no rip**: with a gap of nothing an overlap would leave the corner as it is,
  so it is refused. There is no relief, since there is no bend at the corner.

**What it does not do.** As in Inventor, a rip only opens the corner. The edges where the
walls meet the floor are still sharp, and the flat pattern follows bends, so a shelled box
with its corners ripped is not yet laid flat: Inventor's next step is Bend on those edges,
which Tenon's Bend does not do yet (it joins faces that do not touch, DEC-070). That is the
next thing to offer.

In files: `type = "corner_rip"` with `edges` and, where they are not the defaults, `seam`,
`overlap` and `gap`; an addition to version 2 (DEC-046).

Not done: bends on the sharp edges that are left; the Face/Edge gap; corners that are not
square; Inventor's separate Rip command (a gap cut across a face, for lofted flanges and
contour rolls).

## DEC-075 Bend on a sharp edge (2026-10-12)

Asked for by the owner on the report of the four: "do Bend on a sharp edge next". It is what
lets a box that was shelled, and had its corners ripped (DEC-074), be laid flat.

**What it is.** Inventor's Bend also goes on the edge between two faces that touch: on a
part converted from a solid, bends are put "on the inside edges" once the corners are
ripped, and the part can then be flattened. In Tenon that is a feature of its own,
`sharp_bend` (Sheet Metal > Create > Bend, by a click on a sharp edge; `sheet.sharp_bend`),
named Bend in the browser as the bend between two faces is (DEC-070).

- **The edge** is the outside or the inside edge of a corner where two flat sides of the
  sheet meet (each with the sheet's other side one thickness behind it), at any angle a
  bend can turn: 1 to 179 degrees. Which side of the corner an edge is on is found as a
  corner rip finds it.
- **The corner is cleared, then bent.** Seen along the edge, everything from the outside of
  the corner back to where the bend begins on either face goes (the radius and the
  thickness, times the tangent of half the turn, from the outside of the corner), for the
  length of the edge that was picked. The bend is then put in, tangent to both faces. The
  outer faces stay where they are, so the part keeps its outside size, and the flat pattern
  is what a flange of the same size would give.
- **As far as both faces reach.** A bend is as long as the stretch of the edge where both of
  its faces still come up to it once every corner of the feature has been cleared. So edges
  that meet at a corner of the sheet, given in one feature, have their bends end where their
  bend lines cross, the floor loses a square at the corner, and the walls hang on past the
  bends to where the rip left them: the tray a flange makes on four edges, with its corners
  trimmed to the bends and mitred to the rip's gap. The test compares the two, solid and
  flat pattern.
- **What is in the way is said.** A bend whose end would lie against sheet that runs on
  beside it (a corner that was not ripped, or the next edge's corner when the edges are bent
  one at a time) would be joined to that sheet sideways: it is refused, by looking at the
  ends of the bend in the result, and the message says to open the corner or to bend the
  edges together. Bends that would leave a piece of the sheet on its own (the floor's
  corners, when the corners were ripped by their inside edges) are refused too.

**In the panel** the first edge clicked says which bend it is, by what it is
(`sheet::sharp_edge` reads it from the part as shown): a sharp edge between two faces makes
it a bend on sharp edges, and more of them can be added; an edge of the rim makes it the
bend between two faces. An edge of the other kind is refused in the status bar. Double Bend
and Flip Fixed Edge have nothing to say on sharp edges and are not shown.

In files: `type = "sharp_bend"` with `edges` and an optional `radius`; an addition to
version 2 (DEC-046).

Not done: a relief of the rule's shape where bends meet (the corner is trimmed to the bends,
square); a bend relief cut beside a bend that is shorter than the sheet next to it (it is
refused instead); Inventor's Bend Extension choice; curved edges.

## DEC-076 Flanges on the rims of two walls are mitred (2026-10-12)

The owner's answer to Bend on a sharp edge was to keep going until there is enough to start
M9. The first thing tried was an everyday enclosure, a tray with a lip turned in from the
top of each wall, and it could not be made: the four lips, one flange on four edges, ran
into each other at the corners. Inventor mitres such flanges (Auto-Mitering applies between
the flanges of one feature wherever they come together), so this is a gap in what was there
already, not a new command.

**When.** Two edges of one flange are on different faces of the sheet whose planes cross
along a line both edges are square to, and both flanges set off along that line from the
same level: the tops of two walls of a box. (Edges on one face that meet at a corner of the
sheet are DEC-063's, and stay as they were.) Each wall must be all on the side of the other
face that face looks to; lips turned outwards part from one another and are left whole.

**What.** Each flange is mitred back from the plane that halves the angle between the two
faces, so that half the gap is left on its own side of it: where the flange stands `d`
above the face its edge is on, it stops `d tan(a/2) + gap / (2 cos(a/2))` short of where
the other face's plane crosses its edge, `a` being the angle between the two faces' normals
(a right angle on a box: `d + gap / sqrt 2`). The gap is the flange's own or the rule's
`GapSize`, as for corners on one face. Where the walls themselves were brought together to
that gap, the mitre begins exactly where the wall ends.

- It is laid out in the flat and cut square to the sheet, like every other end
  (`flange::pieces_at_rims`): up the flat of the flange it is a straight line, and round the
  bend it is the curve `3 (1 - cos)` makes, which goes to the kernel as a smooth curve
  through points of it (DEC-073's `region::to_loop_smoothing`). The height `d` is that of
  the inner side of the sheet, or of the outer side once the flange is turned past upright,
  so whichever side is nearer keeps the gap.
- It takes only what it must: an end stays square as far up the flange as that keeps it
  clear, and a flange that never comes near the other is not touched at all (it is built as
  it always was, with the same face names).
- Auto-Mitering off leaves the flanges as long as their edges, and they are refused if they
  then run into each other, as before. Only the symmetric seam is made here: an overlap
  seam between two such flanges is not.

In files nothing changes: it is what a `flange` with several `edges` does. New face names:
the mitred end of a bend and of a wall, at the start and at the end of the edge.

**On the way:** a screenshot run (`--screenshot`) waited a number of frames for the part
and its flat pattern, which go by in no time when nothing holds the frames back; it now
waits until the flat pattern that is shown has been worked out, and for thirty seconds
before giving up.

Not done: the same mitre between flanges made as two features (Corner Seam between faces in
one plane); overlap seams between flanges on two faces; edges that are not square to the
line where the two faces' planes meet.

## DEC-077 Sheet metal is enough for now; M9 starts (2026-10-12)

The owner's word on the report of Bend on a sharp edge was to keep going until there is
enough to start M9. After the mitre between lips (DEC-076) the everyday parts tried all
build and lay flat: a bracket with a tab, rounded corners and patterned holes; a channel
with end walls, hems and a notch round a bend; a tray with lips; a shelled box ripped and
bent. What is left on the Sheet Metal tab is specialist (Lofted Flange, Contour Roll, Rip,
Punch Tool, Unfold and Refold, bend tables, tear and weld reliefs, a Corner Seam between
faces in one plane) and waits until after M9. One thing found on the way is left as it is
and said here: walls on the straight edges of a plate whose corners are rounded with a
radius smaller than the bends reach are refused for running into each other, with advice
that fits (make the flange narrower), not mitred across the rounded corner.

M9 is the plan's performance milestone: benchmarks in CI with budgets, regeneration and
tessellation caching, parallel tessellation, level of detail for large assemblies, and the
numbers published.

## DEC-078 Where the time went, and the first three changes (2026-10-12)

**Measured first.** `tenon-cli bench` times, on the example models and two made for the
purpose (a plate with 40 holes as 42 features, a tray with four mitred lips): a part
rebuilt from nothing, meshed and measured for the screen, rebuilt after an edit of its last
feature and of one half way up its history, meshed again after an edit; a flat pattern; an
assembly opened; a drawing's views. Each row has a budget, and `cargo xtask ci` runs it as
a step of its own (`bench`), so CI fails when a row goes over. `tenon-cli bench profile`
says where the time of a rebuild goes, kernel call by kernel call, by wrapping the kernel
(`tenon_kernel::timed::Timed`).

The profile said three things. Up to 60 % of a rebuild was the kernel summing volumes:
after every cutting or joining feature the part's volume is taken, to see that the feature
did something (DEC-032), and each time every face of the part was integrated again to a
part in a billion. Most of the rest was booleans. And meshing for the screen was mostly
not meshing: on curved parts, measuring the faces took several times longer.

**1. The kernel keeps what each face adds** (the OCCT shim; no change to the `Kernel`
trait). A solid's volume, centre and inertia are sums over its faces, reckoned from one
point; the shim keeps each face's share, and its area and centroid, by the face itself (the
same underlying face, in the same place, the same way round). After a feature only the
faces it made or changed are summed. The point they are reckoned from is near the part's
middle on a coarse grid that grows with the part, so that it stays put while the part
changes a little; when it moves, everything is summed again. Shares of faces nothing else
holds any more are dropped each time the table has doubled.

- A flat face's share is summed with the plain integration, which is exact for it (seen
  from any point it is a cone), not the adaptive one.
- A wall pushed out of a free curve (an extrusion surface: the wall of a hole cut from a
  profile seen at an angle) is summed span by span with the Gauss-Kronrod integration. The
  adaptive sum settled too soon on these, up to five parts in ten thousand of the cut out:
  this was the error DEC-073 put down to the kernel in general. With it gone, the check on
  sheet cuts (DEC-067) is back to a millionth of the part, and the test of an angled cut
  holds its volume to the tolerance of its areas. On rolled faces the Gauss-Kronrod sum is
  still far too slow, and the adaptive one is right on them to a few parts in a billion.

**2. A checkpoint before every feature** (`RegenCache`), where there was one, before the
feature last changed. An edit anywhere now starts from the feature it changes; the first
edit of an earlier feature no longer costs a rebuild from nothing. A checkpoint is second
handles to shapes the kernel holds anyway (they share their unchanged faces), and a copy of
the face names; a history longer than 128 features has one every few.

**3. Booleans run on all cores** (`SetRunParallel(true)`). The results are the same: the
whole test suite, and three builds of three examples compared file by file.

Meshing was already parallel and already kept: OCCT keeps a face's triangles on the face,
so a part meshed again after an edit meshes only what changed, and the bench now shows it
(`the same again` in the profile).

On the development machine, in the profile CI builds with (milliseconds, before and after):

| | before | after |
|---|---|---|
| m8-tray: rebuild from nothing | 646 | 237 |
| m8-tray: rebuild after an edit half way up | 656 | 45 |
| m8-tray: mesh and measure | 54 | 18 |
| m8-tray: lay flat | 37 | 1.6 |
| A tray with four mitred lips: rebuild | 1161 | 484 |
| A tray with four mitred lips: mesh and measure | 203 | 23 |
| A plate with 40 holes: rebuild | 411 | 189 |
| A plate with 40 holes: edit of the last feature | 32 | 9 |
| A plate with 40 holes: edit half way up | 447 | 118 |
| m6 bolt (a modelled thread): rebuild | 857 | 256 |
| m6 bolt: mesh and measure | 194 | 30 |
| m2-enclosure: rebuild | 79 | 37 |
| m6 fittings assembly: open | 1160 | 553 |

What is left is the kernel's own work: of the tray's 237 ms, booleans are about 140.

## DEC-079 What is under the pointer is found through an index kept with the mesh (2026-10-12)

With an assembly of 400 components (two million triangles) on screen, finding the face under
the pointer took 84 ms and the edge 695 ms, every frame the pointer moved: every triangle
was tried against the ray, every edge segment was put on the screen, and each edge that
came near was then tried against every triangle again to see whether it was hidden. A part
of half a million triangles on its own cost the same as an assembly of that many.

A mesh now keeps an index of itself (`tenon_kernel::index`, `Mesh::index`): a box around
it, a tree of boxes over its triangles (halved at the median, eight to a leaf), and a box
around each edge. It is made when first asked for (the tree and the edge boxes only for a
mesh the ray or the pointer comes over) and stays with the mesh, so it moves with a body
from one scene to the next. Faces: a mesh whose box the ray misses is passed over, and in
the others the tree is walked nearest first. Edges: a mesh, and in it an edge, is passed
over when the pointer is clear of where the corners of its box are on the screen.

- **The answers are those of trying everything**, to the bit: the nearest hit, and of two at
  the same distance the earlier body and the earlier triangle; the same edges in the same
  order. Tested against the old code kept as the reference, over ten thousand rays and
  pointer positions on a field of bodies, in perspective and not, with bodies behind the eye
  (`picking_with_the_index_gives_what_trying_everything_gives`,
  `the_index_finds_what_trying_every_triangle_finds`).
- **A mesh is plain data that anyone may change**, so the index carries a stamp of the mesh
  (its sizes, where its buffers are, its first and last point, corner and edge point) and
  is made again when the mesh no longer matches; a copy of a mesh starts without one. Two
  meshes are equal whether or not they have one, and it is not written out. The stamp was
  first a sample of eighty values spread through the mesh; taking it was then most of the
  cost of a pick in a large assembly, each value a part of memory not otherwise touched,
  and it is now the few that a mesh moved or added to cannot keep.
- Not done: one tree over all the bodies. Each body's box is still tried in turn (a fifth
  of a microsecond a body for a face; for an edge the corners of each box are put on the
  screen, 2.7 microseconds a body).

On the development machine, pointer over the middle of each component in turn (ms):

| | face, before | face, after | edge, before | edge, after |
|---|---|---|---|---|
| 100 components, 0.5 million triangles | 20 | 0.02 | 130 | 0.7 |
| 400 components, 2.1 million | 84 | 0.05 | 695 | 1.4 |
| 1024 components, 5.5 million | | 0.17 | | 2.8 |

## DEC-080 Level of detail: far meshes, and nothing drawn that is off the screen (2026-10-12)

**What was there.** Every component of an assembly was sent to the graphics card in full
and drawn in full every frame, wherever it was: 400 bolts and nozzles were 277 MB on the
card and 2.1 million triangles a frame, though each was drawn 40 pixels across.

**The far mesh.** The kernel makes a second, coarse mesh of a part: off the surface by at
most a hundredth of the part's size (the diagonal of its box) and with facets up to 0.8 rad
apart (`MeshTol::far`), so a round hole has eight sides. It is made *apart*: on a copy of
the shape's faces and edges that shares their surfaces and has no triangles yet. Meshed on
the shape itself it would have been the finer triangles handed back (OCCT keeps a face's
triangles when they already meet the tolerance), and put in their place it would have had
the display mesh everything again. The bolt with a modelled thread is 2,700 triangles for
9,056, the nozzle 232 for 1,652. A body gets one only where it at least halves what is
drawn and the body has 200 triangles or more (`tenon_model::far_meshes`). They are made for
the parts of an assembly that is shown (the worker's slots other than the part being worked
on; `AsmSession::refresh_shown`), not for a part on its own, which fills the view, and not
for assemblies the command line opens to export. Making them for the bolt and the nozzle
takes 21 ms, where making the two parts ready takes 350.

Considered and not taken: thinning the display mesh in Rust (no kernel, but a worse mesh for
the same triangles, and silhouettes that wander); and turning off the mesher's check of the
inside of faces, which gave a coil 450 triangles for 16,618 and lost 61 % of its volume.

**Which is drawn** is chosen for every body every frame, from where the ball round its box
is on the screen (`tenon_render::detail`):

- off the screen (or behind the eye): not drawn, and not sent to the card;
- drawn with a radius under 45 pixels: from its far mesh, which is then within a pixel of
  the fine one. It changes back above 55, so a body near the limit does not flicker as the
  view moves;
- otherwise, and always when it has no far mesh, in full.

What is picked is always the full mesh, and a highlight is drawn from the mesh its body is
drawn from, so the two lie at the same depth.

**On the card.** A body is uploaded when it is first drawn at a level. Bodies drawn before
and left out now are kept aside, two million vertices of them at most (72 MB), those left
out longest going first, so that panning and zooming do not send the same bodies again and
again. Highlights are sent only for the bodies that have any: before, every hover made two
buffers for every body on the screen.

**Not done, and said in docs/performance.md:** one copy of a part's mesh on the card for all
its components (each component still has its own, placed, in memory and on the card); and
drawing many small bodies in one call (each body is two draw calls).

Measured on a discrete graphics card, 1440 by 900, four samples a pixel
(`large_assembly_on_the_graphics_card`):

| | on the card | sent and first drawn | a frame |
|---|---|---|---|
| 400 components in view, before | 277 MB | 127 ms | 0.75 ms |
| the same, with level of detail | 74 MB | 20 ms | 0.53 ms |
| 1024 components in view, before | 710 MB | 315 ms | 1.43 ms |
| the same, with level of detail | 188 MB | 67 ms | 0.70 ms |
| 1024 components, close in, with level of detail (507 on screen) | 162 MB | 64 ms | 0.69 ms |

On this card the frame was never the limit; what it buys here is memory and the time to
send. On a card that draws a tenth as fast it is the frame too.

## DEC-081 A frame of a large assembly: only what is in sight, nothing looked for twice (2026-10-12)

An idle frame of 400 components took 1.4 ms of the UI's own work, growing with the assembly
and in places with its square. Three things, found by timing a frame piece by piece:

- **The browser laid out every row every frame**, and for each component looked through
  every relationship for those that hold it. It now lists its rows first and lays out only
  those in sight (`the_browser_of_a_large_assembly_lays_out_only_the_rows_in_sight`); the
  relationships of each component are found in one pass.
- **Every body asked for its component every frame**, and `Assembly::component` looked
  through all of them: a thousand components, a million comparisons a frame. Components are
  kept in the order of their ids, so it is now found by halving (and looked for the old way
  if that misses).
- **Every body looked through every highlight** for its own. Each highlight now goes
  straight to its body.

An idle frame is now 0.2 ms with 100 components, 0.55 with 400 and 1.3 with 1024: about a
microsecond a component, against a 16 ms frame.

## DEC-082 Budgets from CI's own numbers, and counters that prove what is kept (2026-10-12)

**Budgets.** The first run of the benchmarks on CI's three platforms gave their numbers
(they are two to three times slower than the development machine). Each budget is now four
times what the slowest platform took, and no less than 100 ms: a shared runner has slow
minutes, and a budget that fails for those teaches people to run CI again. So a change that
makes something several times slower fails CI; one that makes it half as slow again does
not, and is seen only by someone reading the table every run prints. The counts (triangles
drawn of the grid assembly) do not vary and are held more closely. Tighter than this needs
a machine of our own to compare runs on, which there is not.

**Counters.** That only the faces a feature changed are summed and meshed again was shown
by the clock alone. The backend now counts faces summed, sums kept, faces meshed and meshes
kept (`OcctKernel::counters`), and a test in a program of its own holds the counts: a plate
with ten holes, one more hole cut: 3 faces summed and 14 kept, 3 meshed and 14 kept
(`after_a_feature_only_what_it_changed_is_summed_and_meshed_again`).

**Boxes.** The profile of meshing showed one more thing done over each time: the exact box
of a part with faces on free surfaces (4 to 11 ms for the bolt, the spring and the tray
with lips, changed or not). A shape's box is the box round its faces' boxes, which is how
the kernel reckons it, so each face's is now kept by the face with its sums. The same boxes
to the last bit: checked once against the kernel's own over the kernel, model, assembly and
command line tests, with both worked out and compared in every call.

## DEC-083 Command schemas: declared beside the commands, held true by the tests, one version (2026-10-12)

M10 asks for versioned JSON schemas for every command. Until now a command said what it took
in a sentence of help, and read its parameters out of the JSON one `get` at a time.

**Declared by hand.** Each command has a `CommandSchema` in a `schemas.rs` beside its registry:
its parameters by name, with a type (`Length`, `Angle`, `Id`, `Word` of a set, `Face`, `Path`,
...), whether each must be given, a line of what it is, and where one of several must be given,
which. The other way was to make every command's parameters a struct and derive the schema from
it. That is the tidier end state, and it means rewriting how 204 commands read their input, with
their error messages (which say what was wrong in the command's own terms, and are tested)
replaced by a deserialiser's. Not at this milestone.

**Kept true by the tests, both ways.** A schema written by hand goes stale the first time a
parameter is added. So in builds with debug assertions, which is every test, each command that
runs is held to its schema: what it is *given* must be declared, and what it *reads* must be
declared (commands read parameters through `schema::arg`, which notes each name; reading one
the schema does not have fails the run). A parameter cannot be added to a command and tested
without its schema. A registry test in each crate holds that every command has a schema and
every schema a command. When version 1 was written, twelve commands that no test ran were
found by logging what the suite ran, and given tests.

**Outside and inside.** What comes from outside is checked before it runs, in every build:
scripts, the MCP server, `batch.run`, the app's control port. A wrong name is refused with the
one that was probably meant, and nothing is changed. The app's own calls are not checked in a
release build: they are covered by the tests, and a check there could only turn a working
button into an error.

**Where the schema says less than the command.** Where a command's own message for a missing
parameter is better than the checker's ("a draft needs a pull direction or a parting plane"),
the schema leaves the choice to the command. Parameters that are whole structures (a sketch
constraint, a feature's definition, a relationship's target) are declared as objects, with
their shape in words. What commands *return* is not in the schemas.

**One version.** `version` is one number for the whole set. Within a version the schemas only
grow (commands, optional parameters, words, wider types); anything that could break a caller
raises it. `schema::breaking(old, new)` lists what would break, and `tenon-cli commands --schema
--out` refuses to write over a published file of the same version with one that breaks it. The
published file is `docs/schemas/commands.json`, and a test fails when it is not the
registry's, so a change to any command's parameters is a change to that file in review.

## DEC-084 Words to features: helpers on the registry, and no language model inside Tenon (2026-10-12)

The plan asks for "natural-language-to-feature helpers ... on the command registry".

**What was built.** Commands that take what a person says and make ordinary features of it:

- `model.box`, `model.cylinder`, `model.sphere`, `model.torus`: a dimensioned sketch and its
  extrusion or revolution, as one undo step. The tree afterwards is what drawing it by hand
  leaves, so the sizes are parameters and the sketch can be edited.
- `model.find`: faces and edges by what they are and where ("the top face", "the upright
  edges", "the wall of the 6 mm hole", "the largest flat face", "what Extrusion2 made"), each
  with a reference that other commands take. Seam edges are left out. On a part that fails
  part way it searches what was rebuilt and says so.

With these, "a 60 by 40 plate 8 thick with a 10 mm hole in the middle and its corners rounded"
is four commands, with no coordinates of faces worked out by the caller.

**What was not, and why.**

- *No parser of sentences in Tenon.* Rules that turn English into commands would be brittle,
  English only, and a poor copy of what the model on the other end of the MCP connection
  already does well. Tenon's part is to make the translation short and hard to get wrong, and
  to check it (DEC-083).
- *No client of a hosted language model in the app* (a prompt box that builds the part). That
  needs a third party's service: an account and a key, a cost per use, and the user's model
  and words sent to somebody's servers. That is the owner's decision, and no code was written
  towards it. With `tenon-cli mcp --attach` (DEC-087) an agent the user already has works in
  the app's window, which gives the same thing to anyone with an MCP client.

## DEC-085 Several commands as one undo step (2026-10-12)

An agent that builds a plate in four commands should not leave half a plate when the third
fails, nor four entries to undo. Each session (part, assembly, drawing) now counts its undo
steps and has `mark`, `squash_since(mark)` (the steps since are one) and `roll_back_to(mark)`
(they are taken back, and are not left to redo).

`batch.run` runs the steps of a script on that: as one (the default), or with `as_one: false`
letting the steps before a failure stand. Run as one, the steps may not open or start a
document, nor undo or redo, since those could not be taken back with the rest; a file a step
wrote stays written, and the refusal says so. The primitives (DEC-084) use the same marks, so
a box whose extrusion fails leaves no sketch behind.

## DEC-086 The MCP server's tools: finding, repairing, looking, and a log (2026-10-12)

**Tools.** To the tools of M1 were added: `describe_command` (one command's schema),
`run_commands` (`batch.run`), `find_geometry` (`model.find`), `broken_references` and
`repair_reference` (`model.broken`, `model.repair` of DEC-034), and `session_log`. The schemas
are also a resource. Each tool that changes anything runs a registry command, so there is
nothing an agent can do that is not in the undo history.

**Repair.** `model.broken` now says of each candidate what it is (body, number, surface or
curve, size, where its middle is), and of each broken reference whether the nearest candidate
is clearly the one meant: within 5 mm of where the old one was, and the next nearest more
than four times as far. `repair_reference` without a choice takes the nearest only then. The
alternative, always taking the nearest, would make a part that rebuilds with a fillet on the
wrong edge, which is worse than one that says it is broken. Otherwise the agent is told to
look: `render_png` picks faces and edges out in colour by their numbers.

**The log.** `session_log` is what was done, as a script that `tenon-cli run` replays. The
person whose part it is can read what an agent did, keep it, and run it again. Steps run as
one are one `batch.run` entry; steps not run as one are logged one by one, so that a batch
that failed half way is on record as far as it went.

## DEC-087 The control port: an agent works in the running app (2026-10-12)

"Every AI-made edit is a normal, undoable command the user can inspect" is only half met by a
server with a document of its own: the user sees the result when the file is opened. So the app
can be driven.

**How.** Started with `--control`, the app listens on the loopback address. A request is a
line of JSON with a command id, its parameters and a token; the answer is a line of JSON.
Requests are taken on the UI thread once a frame, one at a time, and run as the registry's
commands on the open document. One that needs the kernel runs as a worker job on a copy of the
part, and the changed part is put in as one edit when it comes back, so the UI thread still
never waits for geometry. `tenon-cli mcp --attach` is the MCP server with the app in place of
its own engine (`Host`), so every tool works the same in both places.

**Safety.**

- Off unless asked for, at each start. Never in a screenshot run.
- Loopback only, and every request carries a token of 128 bits made at start-up. It is written
  to a file in the user's settings folder (mode 0600 on Unix) for `--attach` to find, taken
  away on exit, and not printed (unless that file cannot be written).
- The token comes from the standard library's hasher keys, which it takes from the system's
  random numbers on the three platforms. That is the standard library's practice and not its
  promise; were it to change, the `getrandom` crate is the replacement. It was not added now
  because the app does not otherwise link it.
- Over the port the app refuses what would take something from the person at it: a change
  while a command's panel is open; New or Open with unsaved changes; and it does nothing with
  a command whose result came back after the person changed the part.
- Whoever has the token can do what commands can, including writing files as the user. The
  port is a way for the user's own tools to reach their own app. It is not a sandbox, and the
  documentation says so.

**Other ways considered.** A Unix socket or a named pipe would take the file system's
permissions in place of a token, and needs code per platform that the standard library does
not have for Windows. An HTTP MCP server inside the app would save the second program and add
an HTTP server to the app, open to whatever a browser on the machine can be made to send.
Lines over loopback with a token needed neither.

**Assemblies and drawings.** Their parts are rebuilt and their views drawn on the geometry
thread after each change. A command sent straight after another would have met an assembly
whose new part had no geometry yet, or a drawing whose views were not drawn (a count of
nothing, an empty export). So each step on an assembly or a drawing waits until the app has
caught up with the one before. A base view with no scale has its model built there first, to
pick the scale that fits, as the app's own Base View does.

**Not done.** Four assembly commands need the kernel where they run and have no job of their
own for the port yet: `asm.interference` and the three exports (`asm.export_step`,
`asm.export_stl`, `asm.export_3mf`). Over the port they say so, and that they run in the
app or on the saved file. There is no indicator in the window that the port is open beyond
the status bar naming each command as it runs.

## DEC-088 A path is what the schema says is a path (2026-10-12)

The headless engine made relative paths absolute for any parameter *named* `path`.
`model.repair`'s `path` is a place in a feature's definition (`/edges/0`), which on Windows
reads as a relative path, so a repair from a script failed there with a folder joined onto it.
The engine now resolves the parameters whose schema type is `Path`, whatever they are called,
and no others.

## DEC-089 One mesh for all the components of a part (2026-10-12)

M9's report listed this as not done, and the owner said to start filling in what it listed.

**What was there.** An assembly's scene was a list of bodies each with its own mesh: every
component a copy of its part's mesh, moved to where the component stood, in single
precision. The copy was what was drawn, what was sent to the graphics card (a buffer of
corners with their colours for every body, and two draw calls each), and what picking made
its tree of boxes over. A thousand bolts were a thousand meshes, a thousand trees and two
thousand buffers: 278 MB in memory and 710 MB on the card (DEC-080).

**Now.** A body's mesh stays in the coordinates it was made in, behind a shared pointer, and
the body says where it stands (`BodyView::at`, a frame; `tenon_kernel::Placed` is the two
together). A component's body is its part's mesh and a frame. Whoever needs the mesh where
it stands takes both:

- *Picking* brings the ray to the mesh (`Placed::ray_in`): distances along the ray are the
  same, since a body is moved and not stretched. So the components of a part share one index
  of their mesh, and a hit is worked out in double precision from the mesh as the kernel
  made it, where the copies had been rounded a second time.
- *The graphics card* holds each mesh once: its corners (position and normal), its
  triangles as numbers of corners, its edge lines. A body is a record of 96 bytes: where it
  stands (a matrix) and its two colours. All the bodies of a mesh are drawn in one call. A
  highlighted face is a stretch of its mesh's triangles drawn again with a record of its
  own, so a hover sends 96 bytes and nothing else, and comes to the same depth as the face
  under it because it is the same corners through the same shader.
- *The software renderer* moves each corner as it draws it.

**What a mesh is known by.** The card keeps a mesh after the scene that held it has gone
(for when it comes back into view), so it cannot be known by where it is in memory: a part
rebuilt while a dimension is dragged gets its buffers where the last ones were. Each mesh's
index now carries a number that is never given out twice (`MeshIndex::id`), new when the
mesh is first asked for it and again if the mesh has changed since. Two meshes with the same
triangles are two meshes; sharing comes from holding the same one, not from comparing.

**Measured** (docs/performance.md): 1024 components of two parts are 0.5 MB of meshes in
memory for 278, and 0.63 MB on the card for 710, sent in 1.9 ms for 315; a frame of them in
full 0.66 ms for 1.43.

**Not done.** What is said of each component's faces and edges (names, surfaces, curves,
ends) is still a copy for each, moved to where it stands: 32 MB for those 1024 components,
now the larger part of what the assembly holds. The commands that pick read it all over the
app; moving it only when asked for is a change to each of them.

## DEC-090 A tree over the bodies, for what is under the pointer (2026-10-12)

Also listed in M9's report: finding the edge under the pointer still tried every body's box
against the pointer, 2.7 microseconds a body.

A tree of boxes over the bodies, each box round a body where it stands
(`tenon_render::pick::Bodies`), made when the pointer first asks after the scene has changed
and kept by the workbench until it changes again. A ray, or the pointer with its reach of a
few pixels, passes over whole groups of bodies. Scenes of fewer than sixteen bodies are gone
through one by one as before. The answers are the same as trying every triangle of every
body, held by a test that does both over a field of bodies, some turned and some on their
sides (`picking_with_the_index_gives_what_trying_everything_gives`). Of two faces hit at the
same distance, the one of the earlier body is still the one found.

With 1024 components: a face in 0.025 ms for 0.17, an edge in 1.7 ms for 2.8. What is left
of the edge's time is not the bodies. Measured: where each component is a few dozen pixels
across, some 1,800 stretches of edges lie within reach of the pointer, and putting each on
the screen is 1.2 ms of it; the 34 of them tried for being hidden take 0.3.

(This paragraph first said the time went on long edges, a thread's helix gone through point
by point. That was a guess, and wrong: the longest edge of the bolt has 46 points. Boxes over
runs of a long edge's points were tried on the strength of it, changed nothing that could
be measured, and were taken out again.)

## DEC-091 What a part looked like is kept on disk, and shown at once when it is opened again (2026-10-12)

The third thing M9's report listed as not done: a part is rebuilt from its features when it
is opened, an assembly rebuilds every part it uses, and until then there is nothing to see.
The fittings example shows nothing for half a second; an assembly of thirty ordinary parts
would for several.

**What is kept.** When the app has rebuilt a part that is as it is on disk (just opened, or
a part of an assembly that has not been changed), it writes what the part came to into the
user's cache folder: the scene, which is everything the window shows and picks of the part
(meshes, the names and geometry of faces and edges, mass, the features' status). When the
part is opened again the scene is read and shown at once: 1 to 3 ms for the parts of the
example assemblies, where rebuilding them takes 14 to 500.

**It is never trusted.** The part is still rebuilt, behind what is shown, exactly as before.
When the rebuild is done it is compared with what was shown: the same (as it has been in
every case tried: a rebuild comes to the same scene to the last bit of every number), and
nothing changes on the screen; different, and the right scene takes its place and is what
is kept from then on. So a wrong or stale file can show a wrong picture only for as long as
the rebuild takes, which is how long there was nothing to see before. Until the rebuild is
done the app is busy as it was: commands that need the solid wait for it, and a picture
taken of the window waits for it.

**Found by what the document says.** A file's name is a sum over the part as its file
would say it, how finely it is meshed, whether it carries meshes for drawing from far away,
and which build of the program wrote it (its version, and the size and date of the program
file). A part that changed, or another build of Tenon, finds nothing and writes its own.
The file ends with a sum over its bytes; one that is cut short or damaged is no scene and is
deleted. Numbers are written so that they read back to the last bit (the meshes as plain
numbers, the rest as JSON with exact reading of numbers, which the workspace already had
on).

**Not a file format.** It lives in the cache folder (`%LOCALAPPDATA%\Tenon\cache\scenes`,
`~/Library/Caches/Tenon/scenes`, `$XDG_CACHE_HOME/tenon/scenes`; `TENON_SCENE_CACHE` names
another folder or, with `off`, none), holds 256 MB at most (the files unused longest go
first), and can be deleted at any time. Nothing reads it but the app that wrote it. Part,
assembly and drawing files are unchanged. It is off in tests unless a test turns it on, and
in screenshot runs, which must give the same picture whatever was done on the machine
before.

**Considered.** Keeping the solids themselves (the kernel can write a shape with its
triangles) would spare the rebuild and not only the wait for a picture: an assembly would
open without any part being rebuilt until it is edited. That needs the kernel's own format
read back with the names of every face, a way to tell that what was read is still right
without rebuilding, and a decision on whether it belongs in the part file (as other CAD
systems have it), which is a change of file format and the owner's to make. What is done
here needs none of that and cannot show a wrong part for longer than a rebuild takes.

**Not done.** Drawings: their views are worked out afresh each time. The command line:
it rebuilds, as it must to do anything with the solids. A part being worked on: only parts
as they are on disk are kept, not every state a part passes through while it is edited.

## DEC-092 The Primitives panel: a box, a cylinder, a sphere or a torus in a few clicks (2026-10-12)

Asked for by the owner on M10's report ("do the Primitives panel then start M11"). The
commands were there since M10 (`model.box`, `model.cylinder`, `model.sphere`, `model.torus`,
DEC-084); this is their place on the ribbon and the clicks that fill them in.

**What Inventor does** (from Autodesk's help for the torus and a reseller's description of the
panel; nothing was copied): a Primitives panel on the 3D Model tab with Box, Cylinder, Sphere
and Torus, hidden until it is ticked in the ribbon's list of panels. A shape is chosen, then a
plane or flat face; the matching sketch tool starts by itself (a centre and a corner; a centre
and a size; for the torus its centre, the centre of its section and the section's size); when
the shape is drawn the matching feature command starts by itself, with Join, Cut, Intersect
and New solid, and OK makes it. What is left is an ordinary sketch and an ordinary extrusion
or revolution.

**What Tenon does.** The same clicks in the same order, on a panel that is shown (Tenon has no
list of panels to tick): a large button, Box, with the other three under its arrow, between
Sketch and Create.

1. The shape: its properties panel opens, the origin planes are shown, and a click takes a
   plane or flat face (in the part, or an origin plane or work plane in the browser). A flat
   face selected beforehand is taken at once.
2. A click where its centre is. A click near the plane's origin, which is marked, is on it.
3. The pointer sizes it and a click sets the size: a corner of the box, the rim of the
   cylinder or the sphere; for the torus the middle of its ring and then how thick it is. The
   solid is shown as it will be all the while.
4. Height, direction and Join, Cut, Intersect or New solid in the panel; every size can be
   typed instead of clicked. OK, or Enter.

**Where it differs, and why.**

- *One panel, not a sketch tool followed by a feature command.* The part is not turned to face
  the sketch plane and back, and the solid is seen growing under the pointer, not a rectangle
  that becomes a solid two steps later. The clicks are the same.
- *The sketch is dimensioned.* What is made is the command's result: a rectangle or circle
  held by its dimensions and by where its centre was clicked, so the sizes are parameters at
  once. A shape drawn by clicks in a sketch has none until someone adds them.
- *One undo step* for the sketch and its feature, as the command gives.
- *Sizes set by the pointer are rounded to 0.1 mm*, so the dimensions read as typed.
- *The torus stands as the command makes it:* its ring square to the plane it was clicked on,
  about the plane's second direction. The second click gives how wide the ring is, not which
  way it lies.

**Small things done on the way.** The view no longer turns to fit a first solid while it is
only a preview being sized (the clicks would land elsewhere). The floating toolbar beside a
solid being set up stays away until its size is clicked. The hint in an empty part is not
drawn over the planes while one is being picked. A large ribbon button with a list and a
label of one line had two arrows; it has one.

## DEC-093 Stress studies are kept with the part, and materials say how they answer to a load (2026-10-12)

The part of M11 that does not depend on which mesher and solver are used (DEC-094), done
while that is asked.

**A study** is the question "does this part hold?": faces that are held still (fixed
supports), and forces and pressures on other faces. A part has any number of them. They are
in the part document, so they are undone and redone with everything else, saved in the
part's file (`[[study]]`, an addition to version 2 that files without studies do not see)
and reported by `tenon-cli diff`. They refer to faces by their persistent names, as
features do, so a study follows the part as it changes. What a study comes to is not kept:
it is worked out when asked for.

- A **force** is so many newtons in all, spread evenly over its faces, along a direction
  said as for `model.find` ("down", "-x", three numbers), or square to each face and into it.
- A **pressure** is so many megapascals, square to its faces and into them.
- Units are the part's: millimetres and newtons, so megapascals for pressure and stress.

Nine commands (`study.new`, `study.rename`, `study.delete`, `study.fix`, `study.force`,
`study.pressure`, `study.remove`, `study.mesh_size`, `study.list`), each with its schema.
A command says which study it means, or means the part's only one. `study.list` says what
each study still needs before it can be worked out.

**Materials.** A material may now say how it answers to a load: Young's modulus (GPa),
Poisson's ratio and yield strength (MPa). The library has them for its metals and plastics,
as typical values typed in from general knowledge, like its densities: mild steel as rolled,
304 stainless annealed, 6061-T6, soft brass, bronze and copper, Ti-6Al-4V, plastics as
moulded. For cast iron, acrylic and resin, which break before they bend, the figure is the
stress at which they break. Woods and boards have none (their strength follows the grain),
nor has TPU (it stretches far beyond what a linear check can speak of). A part saved before
this, whose material has a library name, is taken to have the library's values. A material
of one's own has all three or none.

Not decided here: pins, sliding supports, moments, gravity, bearing loads. The plan asks for
fixed supports, forces and pressures.

## DEC-094 M11's mesher and solver: analysed, and asked of the owner (2026-10-12)

The plan has meshing by Gmsh or Netgen and solving by CalculiX, as separate programs, with a
licence analysis to the owner before any of them is bundled or recommended. The analysis is
[docs/m11-proposal.md](m11-proposal.md). In short:

- Gmsh is GPL 2 or later, CalculiX GPL 2, Netgen LGPL 2.1. Run as separate programs that
  are handed files and write files, they are not combined with Tenon, and Tenon's licence is
  as it was. Duties (their licences passed on, their source offered) begin only if Tenon
  hands their binaries out, which it does not yet do.
- Gmsh and CalculiX are on conda-forge for the three platforms; Netgen is not, and has no
  documented way to be driven without its Python.
- conda-forge's Gmsh is built against OpenCASCADE 7.9 and cannot share an environment with
  Tenon's 8.0: it needs an environment of its own, or the user's own install.
- Writing a solver of our own was weighed and is not recommended: weeks of work the user
  does not see, and numbers that would be ours to stand behind.

**Recommended:** Gmsh and CalculiX as separate programs, found on the machine, got by
developers and CI through a second pixi environment, not bundled; the solver behind a narrow
interface so that it can be replaced. **Nothing has been installed, added to `pixi.toml` or
recommended.** That waits for the owner's answer.

## DEC-095 The Stress Analysis environment: a study is set up on the part itself (2026-10-13)

The app's side of DEC-093: what does not depend on the mesher and solver (DEC-094). Laid out
as the established workflow has it, from Autodesk's help (docs/inventor-fidelity.md).

- **Entered** from Environments > Begin > Stress Analysis, or 3D Model > Simulation. The
  ribbon becomes the Analysis tab (Manage, Material, Constraints, Loads, Mesh, Solve, Exit)
  with Inspect, Tools and View; the browser shows the part's studies and, under the one
  worked on, its material, its constraints, its loads and its mesh. Finish Analysis goes
  back. The environment is the window's state: nothing of it is in the part's file but the
  studies themselves.
- **The part is not changed from inside it.** Sketch, feature, work feature and sheet metal
  commands are refused with "Finish Analysis first", so a study's faces cannot be modelled
  away under an open panel.
- **Fixed, Force, Pressure** open a panel that takes clicks on faces (a second click takes a
  face out). A force has its size and its direction: Normal to Face, or along X, Y or Z, and
  the other way. OK is one undo step. With no study yet, the first of them makes one.
- **Changing one** is a double-click on its row (or Edit in its menu): the same panel, and
  OK is `study.set`, a tenth study command added for this: what is given takes the place of
  what the support or load had, and what is not its to have (newtons of a pressure) is
  refused. Delete is in the row's menu.
- **Shown on the part:** a ground mark on each face that is held, an arrow on each face that
  is loaded (three side by side for a pressure) with how large the load is. The faces
  themselves are picked out only for the row clicked in the browser. The first version
  coloured every held and loaded face in; a picture of it showed a part painted yellow with
  its arrows lost on it, and that was changed.
- **What is not there says so.** Simulate and Mesh View are drawn as the ribbon draws every
  command whose milestone has not come, and say "arrives in milestone M11" when pressed.
  Pin, Frictionless, Bearing, Moment, Gravity, Remote Force and Body are there as later
  commands. Constraints(0) and Loads(0) are written in red while the study lacks them, and
  say why when pointed at; so does a material that does not say how strong it is.

Not done: results (they wait on DEC-094); a direction picked from an edge or a face (any
direction can be given by command, and is shown as "As Set"); supports on edges or points;
contacts (studies are of parts); turning the marks off.

**Found on the way.** In the small value window (Sketch Fillet's radius, Offset's distance,
and now Mesh Settings) Enter did nothing, and what was typed went in beside the value the
window opened with (3.5 typed over a 2 made 23.5). Enter: the box was given the keyboard
again each frame before it was asked whether it had lost it. Typing: a box that is given the
keyboard after it is drawn does not select what it shows. It is now the value box of the
panels, given the keyboard before it is drawn: what is typed takes the place of what was
there, and Enter is OK. `a_fillets_radius_and_an_offsets_distance_are_typed_and_entered`;
no test went through that window before.

## DEC-096 Properties shows how strong the material is, and takes it (2026-10-13)

Assign in the Stress Analysis environment opens the part's Properties (DEC-095). Under the
material and its density it now has Young's modulus (GPa), Poisson's ratio and yield strength
(MPa): the library's for a material of the library, changed there like the density; for a
material of one's own that says nothing of them, three empty boxes, which become the
material's once all three are filled (a material has all three or none, DEC-093). What cannot
be (a Poisson's ratio of 0.7) is refused in the command's words.

**Found on the way.** A density typed in Properties described the material again by its name
and density alone, so a yield strength of one's own on a library material went back to the
library's without a word, and a material of one's own lost its colour. Properties now sends
what the material already says with what was changed; a test changes the density after the
strength and looks.

## DEC-097 More of a study: pins, sliding supports, the part's weight, directions that are picked (2026-10-13)

At the owner's word on M11's first report ("keep closing the gaps"), the gaps that report
listed and that do not wait on the mesher and solver (DEC-094, still unanswered: nothing of
them is installed or named in the repository).

**In the model** (additions to DEC-093; files without them are as they were):

- **Pin**: round faces on a pin. Each of the three ways such a face could move is held or
  left free: across the pin (radial), along it (axial), round it (tangential). Held across
  and along and free to turn unless said otherwise; a pin that holds nothing is refused.
- **Frictionless**: flat or round faces that may slide along themselves but not leave or
  enter what they rest on.
- A support says what faces it can hold from what each face is (a pin: cylinders; a
  frictionless one: planes and cylinders), read off the reference to the face, so a command
  is refused without the kernel being asked.
- **Gravity**: the part's own weight, from its material's density: a direction (down, -Z,
  unless said) and an acceleration (9810 mm/s²). On no faces, and once in a study.
- **Suppressed**: a support or load left out of the study for now, and kept. A study whose
  only support is suppressed lacks a support again.
- **Copy Study**: the same study under another name, to change and compare.
- Four commands (`study.pin`, `study.frictionless`, `study.gravity`, `study.copy`) and
  more that `study.set` changes; 218 commands.

**In the app:**

- A force's (and the weight's) **direction is picked**: the Direction selector, then a
  straight edge (along it), a flat face (square to it, into it) or a work axis; or an axis
  of the part from its list. What is kept is the direction, not what gave it: it does not
  follow the edge if the part changes. (Recorded as a gap.)
- **Use Vector Components**: a force by its parts along X, Y and Z, in place of a size and
  a direction. The same force either way: ticking and unticking turns one into the other.
- Pin, Frictionless and Gravity on the ribbon, with panels, marks on the part (a pin seen
  end on, a bar on rollers, an arrow from the centre of gravity) and rows in the browser.
  A pin's panel has Fix Radial, Axial and Tangential Direction. The pointer offers a
  support only the faces its kind can hold.
- A row's menu has Edit, Rename, Suppress and Delete; a study's has Rename, Copy Study and
  Delete Study. **Boundary Conditions** (Display panel) hides and shows the marks.

Still greyed: Bearing, Moment, Remote Force, Body. Each needs deciding how it is spread
over the elements, which is better done with the solver in hand.

**A study is held against the part as it is.** A study names faces, and the part goes on
changing under it. `study.check` (a fifteenth command; 219 in all) rebuilds the part and
says whether the study can be worked out: `ready`, and what it lacks in words: a support, a
load, a part that builds, one solid, a material that says how strong it is, and each support
or load with faces the part no longer has or that have become faces its kind cannot hold. A
suppressed one is not in the study and is not held against it. In the app the same is seen
at once: a row whose faces are gone is written in red with how many ("1 of 2 faces lost");
opened, it is without them and says so, and OK puts it right. The study keeps a lost face
until then: a feature that was only suppressed gives it back, by its name.

**Found on the way.** Enter did nothing in the Rename window of a feature either (the same
fault as DEC-095's value window: the box took the keyboard back before it was asked whether
it had lost it), and what was typed went in beside the old name. The Rename window is now
one for features, studies, supports and loads, with the fault mended:
`a_feature_is_renamed_by_typing_its_name_and_enter` (checked to fail with the fault put
back).

## DEC-098 Gmsh and CalculiX, as separate programs: the owner's answer, and the pipeline (2026-10-13)

**The answer.** Asked the questions of DEC-094 again on the second report, the owner chose
"Yes, all three": Tenon may use Gmsh and CalculiX as separate programs; they may be in
`pixi.toml` as a second environment, for developers and CI to download; the documentation
may tell users to install them. They are not bundled: that is asked again when there are
installers.

**What that made.**

- `pixi.toml` has a `fea` environment (Gmsh 4.15, CalculiX 2.23, conda-forge, locked for
  Windows, Linux, macOS on Intel and Apple). It shares nothing with the default one, whose
  locked packages are as they were (compared before and after). `pixi install -e fea`.
- **The trial** the proposal promised came first: a bar meshed, solved and held against beam
  theory by hand, here on Windows; then the same as tests on the three platforms in CI.
- A crate, `tenon-fea` (layer 4), with everything between the two programs, all of it our
  own: the mesh read from Gmsh's file; its surfaces told to be faces of the part; supports
  and loads put on nodes; CalculiX's input written and its results read. `tenon_io::stress`
  rebuilds the part, exports it and runs the whole; `study.solve` is the command (220).
- Tenon finds the programs where `TENON_GMSH` and `TENON_CCX` say, in a checkout's
  `.pixi/envs/fea`, or on the search path, and starts them with their output in a log.
  Started from outside its environment on Windows, a program is given the environment's
  library folders (CalculiX would not start without them).

**Decisions inside it.**

- **Faces are told apart by where they lie.** A STEP file keeps neither the order of a
  part's faces nor their names, and the mesher numbers surfaces its own way. Each surface of
  the mesh is the face of the part that its triangles lie on, judged against the triangles
  the kernel draws the part with (a dozen triangles of each surface vote).
- **Supports are directions a node may not move along**, not a solver's kinds of support.
  Fixed is all three; Frictionless is square to the face (the radius, on a round one); a Pin
  is across, along or round its axis. A node on two supports is held by both. They are
  written as boundary conditions where they are along an axis of the part and as equations
  between the node's own movements where they are not, each equation leading with a freedom
  no other uses. No solver's "transform" is used, which a node could have only one of.
- **Loads are forces on nodes**, worked out by integrating over each six-node triangle as
  its mid-side nodes curve it. A flat one under an even push puts nothing on its corners
  and a third on the middle of each side. Which way is out of the solid is found from the
  element behind the triangle, not from how the mesher wound it.
- **A part that can still move is told so first.** Held as it is, the ways a part can slide
  or turn as a whole are counted (six less the independent conditions on rigid motion). More
  than none, and the study is refused in words with how many, before the solver is asked
  for an answer that does not exist.
- **Numbers are written short.** CalculiX reads a number from twenty characters; a full
  double with sign and exponent is longer and would be read wrong without a word. Twelve
  digits after the point are written.
- **One solid, one material**, linear and static. The part's weight is the solver's own
  gravity load, from the material's density. At most 600,000 nodes.
- **The solver is behind one module** (`tenon_fea::ccx`): a problem in, movements and
  stresses out. Nothing else knows it is CalculiX.

**How it is checked.** Four cases with answers in the textbooks, worked out through the real
programs by `cargo xtask ci`: a bar in tension (within 0.1 % of Hooke's law), a cantilever
(1.5 % of Timoshenko's beam), a thick tube under pressure (2 % of Lame), a plate with a hole
(5 % of Heywood's formula). The gate sets `TENON_FEA_REQUIRED`, which makes the programs'
absence a failure; without it (a plain `cargo test` on a machine that has not installed
them) those tests say they did not run.

Every result carries: "An engineering aid, not a certified analysis".

## DEC-099 Simulate in the app: a study's results in colour on the part (2026-10-13)

The app's side of DEC-098.

- **Simulate** works the study out where the kernel is (its own thread in the app, so the
  window stays alive; Esc stops the program that is running). What a study lacks is said in
  the study's own words, before anything is started. When it is done the status bar says
  what it comes to in one sentence: the most the material is worked against what it yields
  at, how many times over it holds (or, in the colour of a fault, that it yields), how far it
  moves, and that this is an engineering aid.
- **The results are drawn over the part**: the boundary of the mesh, each six-node triangle
  as four, coloured by the result that is chosen (von Mises stress, displacement, safety
  factor: a row each in the browser), with a scale of ten bands to read the colours by and a
  mark where the most is. They are drawn by the window's own painter, furthest triangle
  first, not by the 3D renderer: that needed no new kind of vertex in two renderers, and it
  is fast enough for the meshes a quick check has. Where two parts of a part overlap on the
  screen at nearly one depth the order can be wrong for a triangle or two. (A gap.)
- **The part bends on the screen.** Displacement Display goes round three ways of drawing
  it: large enough to see (so that the most it moves is a twentieth of the part's size; the
  scale says how many times over), as far as it really moves, and not at all. Under results
  the part itself is drawn as edges only, so the bent shape is seen against the straight one.
- A safety factor's scale stops at 15 ("or more"), and its colours run the other way: red is
  the bad end of every scale.
- **Mesh View** draws the elements' sides over the results. With no mesh yet it divides the
  part without solving, so the mesh can be looked at before a study is ready.
- **Results are of the part and the study as they were.** Any change to either (a load, a
  face, a feature, the material) puts them out of date: they are no longer drawn, and the
  browser says so and that Simulate works them out again. They are not kept in the file.

The mark for the most is drawn where that node is, also when it is on the far side of the
part: it is then seen through it.

**Found on the way: the same part gave a different answer each time.** Two pictures of one
study showed 3,129 nodes in one and 3,123 in the other. The mesher was given every thread of
the machine, and with more than one it does not make the same mesh twice. It now runs on
one: the same part gives the same mesh and the same answer to the last figure, which
`a_cantilever_bends_as_beam_theory_says` holds it to. A quick check's mesh takes a fraction
of a second either way.

## DEC-100 The rest of the Loads panel, and supports and loads on edges (2026-10-13)

The owner confirmed M11 and asked for what was greyed on the Loads panel and for loads on
edges ("M11 confirmed, do the greyed loads and loads on edges next").

**On edges.** A support and a load have `edges` beside `faces`. Only a Fixed support and a
Force may be on edges: a pin and a sliding support are defined by a surface (its axis, the
way it looks), a pressure by an area, and the others below by the faces they are spread
over. An item is on faces or on edges, never both; `study.set` with the one takes the other
away. A force along edges is spread evenly along their length and must have a direction (an
edge has no face to be square to). In the app whichever is clicked first decides, and the
other kind is not offered until the selection is cleared: with both on offer, a click meant
for a face beside an edge would take the edge.

Vertices are not offered. A force on a point is an endless stress there, and a vertex would
be a third kind of reference in the format, which has none. (A gap against Inventor.)

**Moment.** Newton millimetres in all on its faces, about an axis that is given or the
faces' own (a flat face's normal, a round face's axis: they must share it, and when they do
not the study says so and asks for an axis). It is put on the mesh as pushes `F/A + w x r`
over the faces, with `w` found so that the pushes add up to exactly the moment and to no
force: about the faces' centre, growing with the distance from it. The faces are not made
rigid.

**Remote Force.** A force at a point. Its faces get the force and the moment it has about
their centre, in the same form. Again a spread load, not a rigid link to the point: near the
faces a rigid link would give other stresses, away from them the same.

**Bearing Load.** The load of a shaft in a bore, on round faces only: a pressure on the half
of the face that the shaft pushes against, in proportion to how squarely the face meets the
load, scaled so that it adds up to the load. A direction along the bore's axis presses on
nothing and is refused when the study is put to the part (a study does not know which way
its faces lie until then). The app turns a new bearing load across the bore it is given.

**Body.** The part turning, in turns a minute about an axis through a point: the solver's
own centrifugal load on every element, from the material's density. Like the part's weight
it is on all of the part and is there once. Inventor's Body load also takes a linear
acceleration: that is Gravity here, which takes any direction and size. An angular
acceleration is not offered. (A gap.)

**In the file**, as additions to version 2: `edges`, and the load types `moment`, `bearing`,
`remote_force` and `body`. Four commands (`study.moment`, `study.bearing`,
`study.remote_force`, `study.body`: 224 in all); `study.fix` and `study.force` take `edges`
in place of `faces`.

**On the mesh**, the mesher's curve elements (three-node lines) are read as well, and each
curve is told to be an edge of the part by where it lies, as surfaces are told to be faces.
A force along edges is summed along each curved line; held edges hold their nodes.

**Checked** against torsion of a round bar, a bar bent by a moment and by a force from
beyond its end, a shaft's load in a bore, a rod flung round, a bar under its own weight, and
a cantilever loaded along an edge and held by its edges (docs/stress.md has the tolerances).

**Found on the way.**

- A force that a command had given as a negative size along a direction was shown in its
  panel the other way round, and OK would have turned it. The panel now shows the way it
  pushes, with a size that is never negative: `a_force_given_backwards_is_shown_the_way_it_pushes`.
- The textbook cases took 56 seconds side by side and 19 one after another: each run of the
  solver takes every thread of the machine. One external program runs at a time in a
  process.

Gaps: no vertices; a moment and a remote force do not make their faces rigid; no angular
acceleration; where a remote force acts is typed, not picked; a direction, an axis or a
point that was picked is kept as numbers and does not follow what it was picked from.

## DEC-101 The first release: what it is, how it is made, and what it carries (2026-10-13)

The owner asked to close the gaps that can be closed and get ready for the first release
("CLose all the gaps you can let's get ready for the first release version").

**Version 0.1.0.** The workspace already says 0.1.0; a first release of a young program is a
0.x. Publishing it (a tag, and the draft release it makes) is the owner's step.

**A folder that runs on its own** (`cargo xtask dist`, `xtask/src/dist.rs`). Until now Tenon
ran only inside the pixi environment, which puts OpenCASCADE's libraries on the search path.
The release build of `tenon` and `tenon-cli` is put beside the shared libraries it needs,
found by reading each binary's own list of imports (PE, ELF and Mach-O, read by xtask itself:
no tool and no new dependency), and the libraries those need in turn, from the environment.
What is not in the environment is the system's (the C runtime's own DLLs on Windows 10 and
later are counted as the system's though conda has copies). Linux and macOS programs are
linked to look beside themselves (`$ORIGIN/../lib`, `@executable_path/../Frameworks`), so
nothing is rewritten after linking and every signature stays good; the macOS app is given
one ad-hoc signature over the whole bundle. Layouts: a flat folder on Windows; `bin` and
`lib` on Linux; `Tenon.app` on macOS (Apple silicon: Intel Macs build from source).

**Every licence goes with it.** For each bundled library, the conda package that installed
it and that package's licence texts (from the package cache; a package split off another,
as freetype's runtime library is, takes the texts of the one it was split from). For each
Rust crate built into the programs (normal dependencies of `tenon` and `tenon-cli` for that
platform; not build tools, not procedural macros), its published licence files, or, where it
publishes none, its licence and its authors. All listed in `licenses/THIRD-PARTY.txt`. The
libraries are under permissive licences, the LGPL (OpenCASCADE, LibRaw: separate files, so
they can be replaced) or a choice that includes a permissive one (FreeImage, FreeType).

**Tried as a user would have it.** `dist` runs the packaged `tenon-cli version`, `tenon-cli
demo m1` (which builds a part through OpenCASCADE and checks its volumes) and `tenon
--version` with nothing of the build environment on their search paths. The archive is a zip
on Windows and a gzipped tar elsewhere.

**Release workflow** (`.github/workflows/release.yml`): by hand, or on a tag `v*`, the three
platforms are packaged and tried (Linux on the older runner, ubuntu-22.04, so that glibc 2.35
is enough); a tag then makes a *draft* release with the three archives and the changelog's
section for that version. Nothing is published without a person.

**Not signed.** Windows SmartScreen and macOS Gatekeeper warn on first start; the README in
the archive and the user guide say what to do. Signing needs certificates the project does
not have: a question for the owner when it matters.

**Gmsh and CalculiX are not in the archive** (DEC-098: asked again when there are
installers). The user installs them; where they are can now be chosen in Tools > Application
Options as well as on the search path or by `TENON_GMSH` / `TENON_CCX`.

**Also for the release:**

- *Application Options are kept* between sessions (they were not, but for the theme):
  `tenon_ui::Settings`, kept by eframe as one JSON string; a screenshot run neither reads nor
  writes them.
- *A fault of Tenon's own is reported.* A panic writes `crash-<time>.txt` (what, where in
  the code, the thread, a backtrace, the version and system; no part of the user's work) in
  the data folder; the next start says where it is and how to report it. At most twenty are
  kept.
- *The geometry thread survives a fault.* A panic while rebuilding or doing a job used to
  end that thread, leaving the window waiting for ever. It is now caught: the rebuild fails
  with words that say it is a fault in Tenon, a job answers with them, what the thread kept
  is let go, and the next request is served.
- *For users:* `docs/user-guide.md` (installing, the window, keys, a first part, each
  environment, files, faults, what Tenon does not do yet), `CHANGELOG.md`, a README in the
  archive, and the README's "Getting Tenon".
- *A part's number and description*, for parts lists (the roadmap's gap since M3): two
  optional fields of the part, an addition to format version 2 (`part_number`,
  `description`, after its name); `document.properties`; Inspect > Properties. The bill of
  materials and drawings' parts lists show them, and the file's name and the part's name where
  a part has none, as they always did. The CSV gains two columns at its end, so that scripts
  that read it by position read what they read before. A part in several sizes shows its
  number with the size. (A number for each size, in its design table row, is a gap.)
- *Holes for screws.* The Hole panel's Clearance and Tapped choices were greyed. They now size
  the hole for an M3 to M20 screw, from a table in the library beside the screws' own: the
  close and normal clearances of ISO 273, the tap drill, and typical seats for a socket head
  (a counterbore, after DIN 974-1) and a countersunk head (ISO 10642). The hole keeps only its
  sizes; opened again, a size that is a screw's says so. Tapping is still a Thread put on the
  hole; the choice is called Tap Drill, for that is what it gives.

## DEC-102 Text in sketches: one font built in, its letters as profiles (2026-10-16)

The owner asked to "keep going with the milestones for text and stuff in the sketches". Text
was the greyed button on the Sketch tab most worth having: lettering cut into or raised from a
part.

**What a text is.** A sketch entity `text` with a point `at` (where the first line's
baseline starts), the words, the `height` of capital letters and an `angle`. Its point is
all that holds it: a text has no curves of its own for constraints, and its letters are
worked out from the font whenever they are needed, never stored. So the file holds the words
and the sizes, and a text is changed by changing them (`sketch.edit_text`, or a double-click
in the sketch). Moved, turned, scaled or copied with the sketch's Modify tools it goes with
its point, turning and growing; Mirror leaves it, for mirrored letters read backwards.

**One font, built in: DejaVu Sans 2.37**, read by skrifa (pure Rust, from the fontations
project; no system font lookup, no new native library). A part must be the same solid on every
computer and in every version of Tenon: a font found on the system differs from one machine to
the next, and a part whose font is missing would change when opened. DejaVu Sans covers Latin,
Greek and Cyrillic, and its licence (Bitstream Vera and Arev, DejaVu's changes public domain)
allows it to be built into a program and passed on unmodified under its own name; the licence
ships in every release package. A choice of fonts would be an addition: a `font` field whose
absence means this one. This is a third-party data choice made without asking the owner: it
costs nothing to undo until a release has carried it (no file with a text exists before
this), and it is reported.

**Sizes.** `height` is how tall a capital letter is, measured from the font's own H (the
height the font states is about 1% off). Lines are 1.6 heights apart. Characters the font
lacks show its missing-letter box.

**Its letters as profiles.** A text's letters are not put into the planar graph the sketch's
curves divide into regions: a hundred outlines of curves crossing the sketch's lines would
make regions nobody wants. They are a profile of their own: `RegionKey::Text { text }`, all
its letters with their holes. A feature takes the sketch's regions of curves by default and,
in a sketch without any, its texts: a sketch with only a text extrudes or cuts at once. In
Extrude, Revolve, Sweep, Coil and sheet metal Face and Cut, a click anywhere in a text's box
adds its letters or takes them away. Letters go to the kernel as quadratic B-splines; their
holes run counter-clockwise, as circles' do (the kernel turns holes round).

**Not done (said in the fidelity notes):** a choice of font, bold or italic; justification
(the text starts at its point, on its baseline); text along a curve; text that stretches to
fit a box; letters that overlap a region of curves are not merged with it (cut text into a
plate from a sketch of its own, as above); text is not constrained to the sketch beyond its
point. Emboss (raising or recessing a profile wrapped onto a face) is still greyed.

**File format:** additive in version 2: the entity `text` (`at`, `text`, `height`,
`angle`, the angle left out when 0) and the region key `{ text = <id> }`. A file with a
text is refused by an older Tenon, as any addition is.

**Commands:** `sketch.text`, `sketch.edit_text`; `sketch.info` lists `texts`, each with
what a feature's `regions` takes (`select`), how many letters it has and its box.

## DEC-103 Delete Features: what goes with a feature (2026-10-16)

From the comparison with Inventor the owner asked for ("compare all of our features with how
they work on inventor and make ours feel 1-1"). Inventor's Delete on a feature opens Delete
Features, with consumed sketches ticked to go with it and dependent sketches and features
listed to go too. Tenon deleted the feature alone, leaving its sketch, and refused while
anything used it.

**Now:** Delete in the browser's menu opens the Delete Features box when there is more to say:
"Consumed sketches and features" (the sketches the feature and its dependents are made from
that nothing else uses), ticked; "Dependent sketches and features" (what uses it, and what
uses that), ticked; OK deletes them in one undo step and says how many went. With nothing to
say the feature goes at once, as before. `Document::deletion` works out the lists;
`Document::remove_all` deletes later features first and puts everything back if one is
refused. `feature.delete` takes `consumed` and `dependents` (both false unless given, so
that what scripts did before they still do) and returns the features `deleted`.

**Done differently:** Inventor keeps dependents that are unticked, broken ("sick") until they
are given new parents. Tenon keeps no feature whose parent is gone: unticked, the box says
the feature cannot go while they use it, and nothing is deleted. Work features are dependents
like any other feature, not a third tick box.

## DEC-104 Extrude To a face or a plane (2026-10-16)

From the same comparison. Inventor's Extrude goes To a face or plane besides a distance or
Through All; Tenon had the choice greyed.

**What it does.** `ExtrudeExtent::To(PlaneRef)`: a flat face of the part, a work plane or an
origin plane. The extrusion goes to it on whichever side of the sketch it is, so it has no
direction to choose (Inventor's Flip settles which of several faces when a curved face is
ambiguous, which does not arise here). Square to the sketch, it is the distance to the
plane, so its faces are named as any extrusion's. At an angle, the profile is extruded a
little past the plane's farthest reach and cut back along it, the way Split cuts a body; the
face on the plane is named as the extrusion's end cap, so features after it can hold it. A
plane that crosses the profile, lies in the sketch or runs along the extrusion is refused in
words. It follows the face: the block below made taller, a post extruded down to it is
shorter. A taper works with it.

**In the panel:** Extents offers To; a row says which face or plane, or "Click a face or
plane"; a click on a flat face, on a work plane in the view, or on a plane in the browser
picks it. Until one is picked the preview shows the distance.

**File format:** additive in version 2, `extent = { to = <plane> }`, the plane written as a
sketch's is. **Command:** `model.extrude` takes `to`.

**Not done:** To Next and Between, To a vertex, To a curved face (Inventor's "extend to
face"), and the same extents for Revolve.

**Also from the comparison:** the assembly Constrain panel offers Mate, Angle and Insert, with
Flush as Mate's second solution as Inventor has it (Flush had been a type of its own; the
relationship is stored as before), and its offset and angle are typed in value boxes like
every other value in Tenon (they were drag fields).

## DEC-105 The Hole panel's Spotface and To (2026-10-16)

The next differences the comparison with Inventor listed for Hole.

**Spotface.** Inventor's seats are None, Counterbore, Spotface and Countersink; Tenon had no
Spotface. A spotface is a shallow seat whose floor the hole is measured from: a hole 5 deep
under a spotface 1 deep reaches 6 below where it starts. Its depth may be 0, which leaves only
the hole (Inventor allows it, for a spotface on the face where the hole ends). Its faces are
named as a counterbore's are, by the hole's point, so features after it hold them the same
way. A screw's size gives it the width of the head's counterbore; its depth stays the user's.
The drawing note reads "SF Ø12 DEEP 1".

**To.** Inventor's terminations are Distance, Through All and To. Each hole goes along its own
axis to where that meets the plane (a flat face, a work plane or an origin plane): square to
the hole the plane is its floor; at an angle each hole stops where its axis meets it, with its
drill point or flat bottom there. A plane behind where the holes start is refused in words.
Picked as Extrude's To is: a click on a flat face or a work plane, or a plane in the browser
(the two panels share the picking now). Its depth is the plane's, so it has no Depth field;
its drill point stays.

**File format:** additive in version 2, `kind = { spotface = { diameter, depth } }` and
`extent = { to = <plane> }`. **Command:** `model.hole` takes `type: "spotface"`,
`spotface_diameter`, `spotface_depth` and `to`.

**Not done:** a Tapped hole that puts its thread on (Thread does it afterwards), taper tapped,
and placing a hole on a face without a sketch.

## DEC-106 Visibility of sketches and work features (2026-10-16)

The next difference the comparison with Inventor listed for the browser. Inventor's
right-click menu on a sketch or a work feature has Visibility, a tick that shows or hides it in
the view; a sketch a feature uses is hidden until shown that way. Tenon showed the sketches no
feature used, and every work feature, with no say.

**Now:** a sketch's or work feature's menu in the browser has Visibility, ticked when it shows;
`feature.visibility` sets it. A feature remembers only what the user said
(`Feature::visible`, None when they said nothing), so the rule goes on applying to the rest.
A hidden work feature is not drawn and cannot be clicked in the view, unless the open panel
uses it; it is still picked from the browser. The panel being set up always shows its own
sketch, as before.

**It is no change to the part:** the history's hashes leave it out, so showing a sketch
rebuilds nothing (`regeneration_resumes_from_the_feature_being_edited`). It is an edit all the
same, saved with the part and undone like one, as Inventor saves it with the part.

**File format:** additive in version 2, `visible = true | false` on the feature's record,
written only when the user set it. Other features are refused: their visibility is the part's.

## DEC-107 Place: instances follow the pointer (2026-10-16)

The next difference the comparison with Inventor listed for assemblies. Inventor's Place puts
the first component of an assembly at the origin, grounded; then an instance follows the
pointer, a click puts it down and another follows, until Esc or OK from the right-click menu.
Tenon put each part beside the others, one per command.

**Now:** Place (P) on an empty assembly puts the part at the origin, grounded, as before; then,
and at once in an assembly with components, an instance follows the pointer: its middle is
under the pointer, in the view's plane through the middle of the rest of the assembly (so it
stays at the assembly's depth however the view is turned). A click puts it down and the next
instance follows; Esc or a right-click ends, taking back the one following. Each instance put
down is one step to undo (the instance and where it went: `AsmSession::squash_since`); the
one taken back leaves no step (`roll_back_to`). Moving round the view keeps placing on; any
other command ends it first.

**Done differently:** Inventor's right-click opens a menu (OK, Place Grounded at Origin,
rotate); Tenon's right-click ends placing at once. The instance does not settle onto faces
under the pointer, as Inventor's does not.

`Workbench::place_file` (scripts and tests) still places beside the others in one go.

## DEC-108 Revolve's four directions (2026-10-16)

From the comparison with Inventor: a revolution by an angle goes Default, Flipped, Symmetric or
Asymmetric (two angles, one each way). Tenon had Default and Symmetric.

**Now:** `RevolveAngle::TwoSided { forward, backward }` for Asymmetric, and
`Revolve::reverse` for Flipped (turned the other way round the axis; with two angles they
change places). The panel's Direction row has the four, as Extrude's has; Asymmetric shows
Angle A and Angle B, each taking equations. Both go to the kernel as the angles it already
took (`AngleExtent::Angle` of the other sign, `AngleExtent::TwoSided`), so faces are named
as before. `model.revolve` takes `backward` and `reverse`; files gain
`angle = { two_sided = {...} }` and `reverse = true`, additive in version 2.

**Not done:** To, To Next and Between for Revolve.

## DEC-109 Extrude To Next (2026-10-16)

From the comparison with Inventor, whose help calls To Next the termination to prefer over To,
since it names no face that may later be gone.

**What it does.** `ExtrudeExtent::ToNext`, Default or Flipped. The profile is extruded past
the part, then brought back to the part's next faces, as Rib's "to next" is built:

- *Joined, or a new solid:* the part is taken away from it, and the pieces that start at the
  sketch are kept: each runs from the sketch to where it meets the part, however that face
  slopes or curves. A piece that reaches the far end met nothing: refused, "part of the profile
  meets no face of the part ahead" (Inventor fails the same way when the profile overhangs).
- *Cut:* what it shares with the part is taken, and of that the pieces that start at the
  sketch: the cut goes through the material it starts in, to where that ends, and not on into
  material beyond a gap. (Through All goes on.)
- *Intersect* is refused in words.

A piece starts at the sketch when it touches the profile's face there, and runs to the end
when it touches the profile's face at the far end (`Kernel::min_distance`). The pieces keep
the extrusion's names for their sides and the cap at the sketch; their far faces are the part's
faces. Every body of the part is taken into account. No taper: the panel says so.

**Commands and files:** `model.extrude` takes `to_next: true`; files write
`extent = "to_next"`, additive in version 2. **Not done:** Between; To Next for Revolve.

## DEC-110 The assembly Tangent constraint (2026-10-16)

From the comparison with Inventor, whose Constrain types are Mate, Angle, Tangent, Insert and
Symmetry; Tenon had no Tangent.

**What it holds.** A cylindrical or spherical face against a plane, or against another such
face: `RelKind::Tangent { a, b, offset, inside }`. Against a plane, the round face's axis lies
parallel to the plane (a cylinder's) and its axis or centre is its radius in front of the
plane, or behind it when `inside`; between two round faces, their axes parallel (cylinders)
and their axes or centres as far apart as their radii added, or one less the other when
`inside`. `offset` is added to the gap. Each resolved target now carries the radius of the
face it is (`End::radius`, from the face's surface), and `solve::check_tangent` refuses
two planes, or a face that is not round, in words. A cylinder lying on a plane keeps four
motions (it rolls, slides either way and turns about the plane's normal), as it should.

**In the panel:** Constrain's types are Mate, Angle, Tangent and Insert in Inventor's order;
Tangent's Solution is Outside or Inside. `asm.constrain` takes `type: "tangent"` and
`inside`; `asm.edit_relationship` takes `inside`. Files gain the relationship type
`tangent`, additive.

**Not done:** cones (their radius changes along them), and a tangent to a curved face that is
neither a cylinder nor a sphere; Symmetry.

## DEC-111 Extrude Between (2026-10-17)

The owner asked to keep going with the comparison's list ("keep going with the rest of the
list"). Inventor's Extrude goes Between two faces or planes, its help warning that both must
stay; Tenon had it greyed.

**What it does.** `ExtrudeExtent::Between { from, to }`, each a flat face, a work plane or an
origin plane, wherever the sketch is (the sketch need not lie between them). The profile is run
across both and cut along each, the way To is cut at an angle (`trim_to_plane`, which now keeps
a given side and names either cap): the face on `from` is the start cap, the face on `to` the
end cap. Both square to the sketch, it is the profile moved to `from` and extruded as far as
`to`, exactly. One plane twice, or a plane along the extrusion, is refused in words; no taper.

**In the panel:** Extents > Between shows From and To; a pick fills From, then To, and clicking
either row makes it the one the next pick fills. `model.extrude` takes `from` with `to`;
files write `extent = { between = { from, to } }`, additive in version 2.

## DEC-112 Revolve To and To Next (2026-10-17)

From the same list. `RevolveAngle::To(PlaneRef)` and `RevolveAngle::ToNext`, Default or
Flipped.

- *To* turns round to a plane that holds the axis (any other is refused in words): to a flat
  face, as far round as the half of the plane the face is on (from its centroid); to a work or
  origin plane, to where the profile first meets it. Measured from the profile's point farthest
  from the axis. Its own plane is half a turn round.
- *To Next* turns a little short of a whole turn (`NEXT_GAP`, so that where it would end is
  apart from where it starts) and keeps the pieces that start at the sketch, as Extrude's To
  Next does: the selection is now `next_pieces`, given where the sweep starts and where it would
  end, for both.

In the panel, Extents has Angle, Full, To and To Next; To shows the plane or "Click a face or
plane through the axis", picked in the view or the browser (a pick of an axis in the browser
still sets the axis). `model.revolve` takes `to` and `to_next`; files write
`angle = { to = ... }` and `angle = "to_next"`.

**Not done:** Revolve Between, and To a plane that does not hold the axis.

## DEC-113 A hole placed by clicking a face (2026-10-17)

Inventor's Hole is placed on a face without a sketch drawn first: a click where the hole goes,
then dimensions from edges. Tenon's needed a sketch of points.

**Now:** Hole with no sketch of centres (none being edited, and none with free points) opens
asking for a flat face: each click on one puts a centre there (to a hundredth of a millimetre),
a click on a centre takes it away, and the preview drills them. Centres go on the face clicked
first; another face is refused in words (OK, then start another Hole). OK makes a sketch on the
face with the points, and the hole on it, as one undo step (the session's mark, squashed; rolled
back if the hole is refused). The sketch is the hole's, nested under it in the browser; editing
it dimensions the holes from the face's edges, which is how Inventor's placement is
dimensioned too. Termination To, while its plane is still to be picked, takes the click first.

**Not done:** dimensioning from edges while placing (Inventor's linear and concentric
placements); the sketch is dimensioned afterwards.

## DEC-114 Tapped holes (2026-10-17)

Inventor's Hole types are Simple, Clearance, Tapped and Taper Tapped. Tenon's Tap Drill choice
sized the drill and left the thread to a Thread feature (DEC-101).

**Now:** `Hole::tapped: Option<Tapping>` (pitch, designation, length, left; each optional): after
the holes are cut, each hole's wall gets the thread the Thread feature would put on it, cosmetic,
from the hole's mouth, so drawings, the Threads list and the thread checks treat it the same.
The panel's choice is called Tapped: the screw's tap drill, its thread named (M8x1.25, ISO
coarse), Full Depth ticked, or a Thread Depth taking equations. The part's solid is as an
untapped hole's. The drawing note adds the thread ("Ø6.8 DEEP 12, M8x1.25 DEEP 8"). Opened
again, a Tap Drill hole made before this is Tapped, and OK puts its thread on.

`model.hole` takes `tapped`, `thread_length`, `pitch`, `designation` and `left`; files gain
`tapped = {...}`, additive in version 2. **Not done:** taper tapped; a modelled thread from the
Hole (Thread still models one).

## DEC-115 The assembly Symmetry constraint (2026-10-17)

Inventor's last Constrain type: two planes, axes or points are held as mirror images of each
other in a third pick, a plane.

**What it holds.** `RelKind::Symmetry { a, b, plane }`: B is A mirrored in the plane: for planes,
B's normal is A's mirrored and B lies on A's plane mirrored; for axes, the same with directions
and lines; for points, B is A's mirror image. A face and an axis, or a third pick that is no
plane, are refused in words. The plane may be on any component or the assembly's own.

The solver's relationships have two ends, so the plane rides in the law
(`Law::Symmetry { plane: End }`, resolved by `session::law_for`), and the bodies a
relationship holds now include it (`Rel::bodies`), for what moves and for the freedoms
counted. `RelKind::all_targets` lists all three, for components, checks and replacing a
component.

**In the panel:** Symmetry is the fifth type, in Inventor's order; it has a third selection,
Plane, which the second pick hands on to (a click in the view, or a plane in the browser's
Origin folder). `asm.constrain` takes `type: "symmetry"` and `plane`; files write the
relationship type `symmetry` with its `plane`, additive.

## DEC-116 A work plane through three points (2026-10-17)

Inventor's work plane methods include Three Points (endpoints, midpoints, intersections or work
points; the first toward the second is X, the third on +Y). Tenon had offset, angle and
midplane.

**Now:** `WorkPlane::ThreePoints { points: [PointSel; 3] }`, a `PointSel` being the origin, a
work point, a circular edge's centre, or a point of a sketch (`SketchPoint { sketch, point }`).
These are what Tenon can name and follow; vertices are not named yet (persistent naming has no
vertices), so they cannot be picked. The frame: origin at the first point, X toward the second,
the third on the positive Y side. Three in a line, or two the same, are refused in words; the
plane follows its points (a sketch point dragged turns it).

**In the panel:** Plane > Three Points, with Point 1, 2 and 3: a work point or a sketch's point
clicked in the view (drawn over the part, picked first), a circular edge (its centre), a work
point in the browser, or the Origin folder's Center Point. `work.plane` takes
`by: "three_points"` and `points`; files gain the kind, additive.

## DEC-117 Center Point: centre points and sketch points (2026-10-17)

Inventor's sketch points come in two formats: centre points, drawn as a cross, which a hole
takes on its own, and sketch points, drawn as a dot, which it takes only when picked. Format >
Center Point switches the selected points, and what the Point tool makes. Tenon had one kind:
every point on its own was a hole centre.

**Now:** an entity mark `sketch_point` (written only when true), on a point on its own. Points
are centre points unless marked, so files and sketches made before keep their holes.
`Sketch::set_center_point` refuses anything else in words (a curve, or the end of one: Tenon's
holes go only at points on their own, so the format would mean nothing there).
`hole_centres` passes sketch points over; the Hole panel still offers every point to pick.
A copied sketch point stays one.

**In the sketch:** Format > Center Point, between Centerline and Driven Dimension. With points
selected it toggles them; with none, what the Point tool makes, and it is lit while that is
centre points (as a sketch opens). Centre points are drawn as a cross, sketch points as a dot.
`sketch.center_point` (`entity`, `on`), and `center` on `sketch.point`; `sketch.info` lists
`sketch_point`. Files gain the mark, additive.
