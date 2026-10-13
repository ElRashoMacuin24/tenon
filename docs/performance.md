# Performance

What Tenon takes to rebuild, mesh, open and draw the example models; what is done to keep
that small; and where the time that is left goes. Milestone M9 (decisions DEC-078 to
DEC-082), and what was filled in after it: one mesh for all the components of a part
(DEC-089) and a tree over the bodies for picking (DEC-090).

In short: Tenon's own work is about a hundredth of a rebuild or less, and the rest is the
kernel (OpenCASCADE, OCCT). So the work of M9 was to ask the kernel for less: not to sum, mesh
or box again what a feature left alone, not to rebuild what an edit did not reach, and not to
draw or search what is not on the screen or not under the pointer. What is left is listed
under [Where OCCT is the limit](#where-occt-is-the-limit) and
[What is not done](#what-is-not-done).

## How it is measured

`tenon-cli bench` times, on the example models and three made for the purpose (a plate with
40 holes as 42 features, a tray with four mitred lips, a grid of 100 bolts and nozzles):

- a part rebuilt from nothing on a new kernel; meshed and measured for the screen; rebuilt
  after an edit of its last feature and of one half way up its history, as the app rebuilds
  it (with what it kept from the time before); meshed again after an edit;
- a sheet metal part laid flat;
- an assembly opened, every part rebuilt and meshed;
- a large assembly in the view: the far meshes made, the triangles drawn, the face and the
  edge under the pointer found;
- a drawing's views worked out;
- the memory the run holds at its end.

Each time is the median of three or five. The edits are a feature renamed: nothing changes
but everything after it must be followed through.

`cargo xtask ci` runs it as a step of its own (`bench`, with `--check`), on Linux, macOS and
Windows, and fails when a row is over its budget. **A budget is four times what the slowest
of the three platforms took** when the budgets were set, and no less than 100 ms (DEC-082).
That is loose on purpose: shared runners have slow minutes. It catches a change that makes
something several times slower, not one that makes it half as slow again; that shows only in
the table, which every run prints. The counts of triangles do not vary and are held closer.

`tenon-cli bench profile` says where a rebuild's time goes, kernel call by kernel call
(`tenon_kernel::timed::Timed`).

All numbers below are from one desktop (32 logical processors, a discrete graphics card,
Windows 11), in the profile CI builds with (Tenon's crates at `opt-level = 1`; OCCT is a
release build either way). They move by a tenth or two from run to run. CI's runners take
two to three times as long: the bolt rebuilt in 550 ms on Linux, 716 on Windows and 751 on
macOS, against 272 here.

## Parts

Milliseconds.

| Part | Rebuild from nothing | Mesh and measure | Rebuild after an edit of the last feature | Mesh again after it | Rebuild after an edit half way up |
|---|---|---|---|---|---|
| m1-bracket (6 features) | 7.1 | 2.9 | 2.9 | 2.0 | 6.0 |
| m2-enclosure (shell, fillets) | 40 | 8.9 | 4.1 | 3.9 | 20 |
| m2-mount | 31 | 7.6 | 4.5 | 5.7 | 23 |
| m6 nozzle (a loft) | 28 | 5.5 | 8.0 | 5.7 | 27 |
| m6 bolt (a modelled thread) | 272 | 34 | 268 | 33 | 263 |
| m6 spring (a coil) | 7.1 | 123 | 7.2 | 126 | 7.1 |
| m7-end-cap | 11 | 3.1 | 3.8 | 2.3 | 6.6 |
| m8-tray (sheet metal) | 228 | 13 | 12 | 5.4 | 40 |
| A plate with 40 holes (42 features) | 167 | 78 | 7.7 | 76 | 106 |
| A tray with four mitred lips | 444 | 20 | 350 | 12 | 413 |

A flat pattern: 1.3 ms for the tray, 2.1 ms for the tray with lips.

Three of these rows are not what the columns suggest, and are the kernel's doing:

- **The bolt's last feature is its thread**, a helical groove cut out of the shank, so every
  edit pays for it: 139 ms of boolean and 81 ms of summing the volume of the helical faces.
- **The spring is one feature**, so nothing of it is kept across an edit, and its 123 ms of
  meshing is a coil's: one long swept face.
- **The plate's top and bottom are changed by every hole**, and a face that changes is
  meshed whole again: after an edit of one hole the mesh costs 76 ms of the first time's 78,
  nearly all of it those two faces with 40 holes each. Meshed again with nothing changed,
  the whole plate costs 9 ms.
## Where the time goes

`tenon-cli bench profile`, milliseconds:

| Part | Rebuild | Booleans | Summing volumes | Other kernel calls | Tenon's own |
|---|---|---|---|---|---|
| m1-bracket | 6.9 | 5.0 | 1.0 | 0.8 | 0.1 |
| m2-enclosure | 37 | 16 | 3.5 | 17 (fillet 7.5, shell 7.4) | 0.2 |
| m6 bolt | 255 | 139 | 81 | 35 (sweep 33) | 0.1 |
| m8-tray | 199 | 143 | 39 | 17 | 0.7 |
| A plate with 40 holes | 145 | 106 | 20 | 19 (revolve 15) | 0.5 |
| A tray with four mitred lips | 448 | 218 | 196 | 33 (roll 23) | 0.8 |

Tenon's own work (the sketch solver's results looked up, profiles built, names carried from
feature to feature, checkpoints kept) is about a part in a hundred of a small part's rebuild
and under one in a thousand of the bolt's. Booleans are half to three quarters of every
rebuild. The volume is summed after each cutting or joining feature to see that the feature
did something (DEC-032).

## What is kept from one feature to the next

- **Each face's share of the volume, its area and its box.** A solid's volume, centre and
  inertia are sums over its faces; the backend keeps each face's share by the face, and its
  exact box, so after a feature only the faces it made or changed are worked out
  (DEC-078). Before this, summing volumes was up to 60 % of a rebuild. A plate with ten
  holes, one more cut: 3 faces summed and 14 kept
  (`after_a_feature_only_what_it_changed_is_summed_and_meshed_again`).
- **Each face's triangles.** OCCT keeps them on the face; the same test holds that 3 faces
  are meshed and 14 kept. Meshing a part again when nothing changed costs a fraction of the
  first time (the tray 4.9 ms for 12, the plate 8.9 for 72, the spring 3.4 for 120), which
  is the walk over the faces and the copy out.
- **The part as it stood before each feature** (`RegenCache`: second handles to shapes the
  kernel holds anyway). An edit starts from the feature it changes: the tray after an edit
  half way up its history in 40 ms, not 228; the plate after an edit of its last hole in
  7.7 ms, not 167. A history longer than 128 features has a checkpoint every few.

## What runs on every core

Both are OCCT's own switches; the results are the same with them on (the whole test suite,
and builds of three examples compared file by file). Measured by turning each off in a
local build (meshing: the part meshed and measured for the screen; booleans: the part
rebuilt from nothing), milliseconds:

| | one core | every core |
|---|---|---|
| Meshing: m6 spring | 387 | 104 |
| Meshing: m6 bolt | 76 | 29 |
| Meshing: a plate with 40 holes | 142 | 65 |
| Meshing: m2-enclosure | 19 | 8.0 |
| Booleans: m6 bolt, rebuild | 653 | 244 |
| Booleans: m8-tray, rebuild | 433 | 199 |
| Booleans: a tray with four mitred lips, rebuild | 661 | 413 |
| Booleans: a plate with 40 holes, rebuild | 138 | 141 |
| Booleans: m2-enclosure, rebuild | 33 | 35 |

Meshing gains two to four times on 32 processors, booleans up to 2.7 times where the
solids are large and nothing where they are small. Neither comes near the number of
processors: faces are meshed side by side but each face on one, and a boolean's
intersections run side by side but the building of its result does not.

Parts of an assembly are rebuilt one after another on the worker thread, not side by side
(the kernel is one object on one thread).

## Assemblies

| Assembly | Open, rebuild and mesh every part |
|---|---|
| m3-pivot (5 components, 4 parts) | 14 ms |
| m6-fittings (bolt, nozzle, spring) | 499 ms |
| A grid of 100 bolts and nozzles (2 parts) | 348 ms |

An assembly opens by rebuilding each of its part files from its features, once for each
part and each size of it, however many components there are of it. There is no geometry
kept on disk to open from.

### In the view

`cargo test -p tenon-ui large_assembly -- --ignored --nocapture`, grids of bolts and
nozzles in a window 1440 by 900:

| | 100 components | 400 | 1024 |
|---|---|---|---|
| Triangles in all bodies | 535,400 | 2,141,600 | 5,482,496 |
| Drawn with the whole assembly in view | 182,100 | 586,400 | 1,501,184 |
| Drawn close in (a sixth of the distance) | 344,368 in 74 bodies | 663,232 in 238 | 1,042,216 in 523 |
| Find the face under the pointer | 0.01 ms | 0.014 ms | 0.025 ms |
| Find the edge under the pointer | 0.65 ms | 1.0 ms | 1.7 ms |
| An idle frame, the UI's own work | 0.2 ms | 0.7 ms | 1.4 ms |
| Meshes held in memory, and far meshes | 0.42 + 0.12 MB | 0.42 + 0.12 MB | 0.42 + 0.12 MB |
| What is said of each component's faces and edges | 3.1 MB | 12 MB | 32 MB |

**One mesh for all the components of a part** (DEC-089). A component is its part's own mesh
and a record of where it stands: nothing is copied for it, in memory or on the graphics
card, and what picking keeps about a mesh (its tree of boxes) is kept once. The grids above
are two parts, so two meshes and their two far meshes however many components there are.
Before, each component had a copy of its part's mesh moved to where it stood: 21 + 6 MB for
100 components, 84 + 24 for 400, 216 + 62 for 1024. What is still kept for each component
is what is said of its faces and edges (their names, surfaces, curves and ends, where the
component stands): the last row, now the larger part of what an assembly holds.

**Level of detail** (DEC-080). A component drawn smaller than 90 pixels across is drawn
from a far mesh, made by the kernel to a hundredth of the part's size, which is under a
pixel there: the bolt 2,700 triangles for 9,056, the nozzle 232 for 1,652. A component off
the screen is not drawn. Making the far meshes of the bolt and the nozzle takes 21 ms.

**Finding what is under the pointer** (DEC-079, DEC-090) goes through a tree of boxes over
the bodies, and then through the tree of boxes kept with the mesh of each body the ray comes
near, and gives exactly what trying every triangle of every body gave. The ray is brought
into the mesh's own coordinates, so the components of a part share one tree. Without any of
it: 20 ms for a face and 130 ms for an edge with 100 components, 84 and 695 ms with 400,
every frame the pointer moved. With the trees over the meshes alone (M9 as first reported):
0.02, 0.05 and 0.17 ms for a face and 0.7, 1.4 and 2.8 ms for an edge.

**A frame** (DEC-081): the browser lays out only the rows in sight, and nothing is looked
for by going through every component. An idle frame with 400 components was 1.4 ms.

On the graphics card (one discrete card, 1440 by 900, four samples a pixel;
`large_assembly_on_the_graphics_card`):

| | on the card | sent and first drawn | a frame |
|---|---|---|---|
| 400 components in view, every body in full | 0.57 MB | 1.7 ms | 0.39 ms |
| the same, with level of detail | 0.18 MB | 1.2 ms | 0.20 ms |
| 1024 components in view, every body in full | 0.63 MB | 1.9 ms | 0.66 ms |
| the same, with level of detail | 0.24 MB | 1.0 ms | 0.31 ms |
| 1024 components, close in, with level of detail | 0.73 MB | 1.3 ms | 0.33 ms |

Each mesh is on the card once (its corners, its triangles as numbers of corners, its edge
lines), with a record of 96 bytes for every body drawn from it, and all the bodies of a mesh
are drawn in one call. With a copy for every component the same rows were 277, 74, 710, 188
and 162 MB, 127, 20, 315, 67 and 64 ms to send, and 0.75, 0.53, 1.43, 0.70 and 0.69 ms a
frame. Level of detail no longer saves memory worth the name; it halves the frame, which is
what it is for. Nothing here has been measured on integrated graphics, where the frame
should gain most as the triangles fall.

## Drawings

A sheet of the plate with its views: 66 ms to open and work out every view. The hidden
lines are removed by the kernel; this has not been taken apart further.

## Memory

The whole benchmark run (every example rebuilt several times, three assemblies, the grid
placed in memory) ends holding 140 to 180 MB, differing from run to run. An assembly holds each part's mesh
once, with what is said of each component's faces and edges moved to where it stands (the
table above), and the same on the graphics card.

## Where OCCT is the limit

These are the kernel's own costs, measured above, that Tenon can only avoid asking for:

- **Booleans**: half to three quarters of every rebuild. 5 ms for a hole in a bracket; 139 ms
  to cut a thread; 143 ms for the 23 of a sheet metal tray.
- **The volume of faces on free surfaces**: a helical thread (81 ms in 5 sums), rolled faces
  with a mitre or a profile cut across them (196 ms in 68 for the tray with lips). On a
  wall pushed out of a free curve the kernel's quick sum settles some parts in a hundred
  thousand out, so the slower one is used there (DEC-078).
- **A face that changes is meshed whole again.** A plate's top and bottom with 40 holes:
  about 70 ms each time a hole is added or changed.
- **A coil or any long sweep** is one face: 104 ms to mesh on 32 processors, 387 on one, and
  nothing to keep across an edit of it.
- **One kernel, one thread.** A boolean uses the processors inside itself; two features, or
  two parts of an assembly, are never worked on side by side.
- **The exact box of a free surface**: 4 to 11 ms for the bolt, the spring and the tray with
  lips, now paid once for a face and not each time the part is shown.

## What is not done

- **What is said of a component's faces and edges is still copied for it** (their names,
  surfaces, curves and ends, moved to where it stands): 31 kB for a bolt with its thread,
  32 MB for the 1024 components. It is what the commands that pick faces and edges read,
  all over the app; it would have to be moved when asked for instead.
- **No geometry kept on disk.** A part is rebuilt from its features when it is opened; an
  assembly rebuilds every part it uses.
- **A change in sizes on screen is not smoothed.** A body changes between its far mesh and
  its full one at once, with a gap between the two limits so that it does not flicker.
- **Without a graphics card** the software drawing takes 0.4 s for a frame of 400
  components. It is the fallback, and is drawn only when something changes.
- **Finding an edge under the pointer** goes through every point of a long edge whose box
  the pointer is over (a thread's helix, in each bolt near the pointer): most of the 1.7 ms
  with 1024 components. A tree over the points of long edges would end that.
- **An idle frame's own work grows with the components** (1.4 microseconds each): how each
  is drawn, its colours and what it is are worked out afresh every frame.
- **Budgets are loose** (four times the slowest platform), for want of a machine of our own
  to compare runs on.
- **Nothing here was measured on integrated graphics, or beyond a thousand components.**

## Tests

| What | Test |
|---|---|
| The rows above stay within budget | `tenon-cli bench --check` (the `bench` step of `cargo xtask ci`) |
| After a feature, only what it changed is summed and meshed again | `crates/kernel-occt/tests/m9.rs` `after_a_feature_only_what_it_changed_is_summed_and_meshed_again` |
| An edit resumes from the feature it changes, with the same result as from nothing | `crates/model/tests/m2.rs` `regeneration_resumes_from_the_feature_being_edited` |
| A far mesh is coarse and leaves the display's triangles alone | `crates/kernel-occt/tests/m6.rs` `a_far_mesh_is_coarse_and_leaves_the_displays_triangles_alone` |
| Far meshes are made where they pay, and for an assembly's parts only | `crates/model/tests/model.rs` `far_meshes_are_made_where_they_pay`, `worker_regenerates_off_thread_and_reports_the_latest` |
| Which level a body is drawn at | `crates/render/src/tests.rs` `a_body_is_drawn_in_full_from_its_far_mesh_or_not_at_all` |
| Meshes left out are kept aside on the card; highlights only where there are any | `crates/render/src/tests.rs` `bodies_left_out_are_kept_aside_and_highlights_are_sent_only_where_there_are_any` |
| Picking through the index and the tree over bodies gives what trying everything gives, for one mesh standing in many places | `crates/render/src/tests.rs` `picking_with_the_index_gives_what_trying_everything_gives`, `a_mesh_standing_somewhere_is_picked_as_a_copy_moved_there_is`; `crates/kernel/src/index.rs` |
| One mesh on the card for every body drawn from it, each where it stands | `crates/render/src/tests.rs` `gpu_viewport_renders_when_an_adapter_exists` |
| The components of a part share its mesh | `crates/ui/src/asm_tests.rs` `a_large_assembly_is_drawn_by_level_of_detail` (a hundred components, two meshes) |
| A large assembly in the app: levels, picking and an idle frame within budget | `crates/ui/src/asm_tests.rs` `a_large_assembly_is_drawn_by_level_of_detail` |
| The browser lays out only the rows in sight | `crates/ui/src/asm_tests.rs` `the_browser_of_a_large_assembly_lays_out_only_the_rows_in_sight` |
| UI work per frame on a 40-feature part, and while dragging a component, under 16 ms | `crates/ui/src/tests.rs` `frame_time_stays_within_budget`; `crates/ui/src/asm_tests.rs` `assembly_drag_frame_time_stays_within_budget` |
| The M1 bracket rebuilds and meshes within 500 ms each (kept from M1; the bench holds each to 100) | `apps/tenon-cli/tests/m1.rs` `m1_demo_part_regenerates_and_tessellates_within_budget` |
| Timing only: 42 features from nothing against resumed at the edited feature | `crates/model/tests/m2.rs` `regeneration_speed` (ignored) |

## UI thread

The interactive app never regenerates on the UI thread: the workbench sends the document to a
worker thread (`tenon_model::worker`), keeps drawing the last result, and swaps in the new scene
when it arrives. A newer document cancels the regeneration in progress (between kernel
operations), and STEP export also runs on the worker. `crates/model/tests/model.rs`
(`worker_regenerates_off_thread_and_reports_the_latest`) checks that only the latest revision is
reported; cancellation itself has no dedicated test yet. The UI thread does not clone or compare
the document every frame: it keys the shown geometry on the document revision and the open panel.
