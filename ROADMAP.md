# Roadmap and feature parity

Status values: **done** (a named test proves it), **partial** (the gap is stated), **missing**.
Nothing is marked done without a test. Milestone scope: `docs/plan.md`. Decisions:
`docs/decisions.md`.

Current milestone: **M0 foundations, complete on Windows** (2026-10-07). Linux and macOS are
configured but not yet verified (no CI run yet).

## M0 foundations

| Feature | Status | Proof / gap |
|---|---|---|
| Layered workspace, layering check | done | `cargo xtask layers`; rules tested in `xtask/src/layers.rs` (14 tests) |
| `unsafe` confined to the FFI crate | done | `cargo xtask unsafe-audit`; lexer tested in `xtask/src/unsafe_audit.rs` |
| Asset attribution check | done | `cargo xtask assets` (`xtask/src/assets.rs`) |
| wasm check of backend-neutral crates (L0-L6) | done | `cargo xtask wasm` |
| Local CI gate (fmt, clippy -D warnings, tests, assets, layers, unsafe audit, wasm) | done | `pixi run cargo xtask ci`: 7/7 steps pass on Windows 11 |
| GitHub Actions on Linux, macOS, Windows | done | `.github/workflows/ci.yml`; first run [37715695877](https://github.com/ElRashoMacuin24/tenon/actions/runs/37715695877) green on all three |
| `Kernel` trait (backend-neutral, object safe, `Send`) | done | `crates/kernel/src/lib.rs` tests; `kernel_moves_to_a_worker_thread` |
| Central tolerances, robust predicates, frames | done | `crates/geom/src/{tol,predicates,space}.rs` tests |
| OCCT 8 backend builds and links (Windows) | done | `crates/kernel-occt/tests/m0.rs` `reports_occt_8` |
| OCCT 8 backend builds and links (Linux, macOS) | done | all kernel tests pass on ubuntu-24.04 and macos-14 in CI |
| Primitives: box, cylinder, cone, sphere, torus | done | `box_measures_and_topology`, `cylinder_measures_roles_and_geometry`, `other_primitives` |
| Booleans: union, cut, intersect | done | `union_with_partly_overlapping_cylinder`, `cut_through_hole_with_history`, `intersect`, `inclusion_exclusion_on_random_boxes_and_cylinders` |
| Operation history (images, generated, primitive roles) | done | `cut_through_hole_with_history`, `box_face_roles_match_geometry` |
| Topology queries with adjacency; face/edge geometry | done | `box_measures_and_topology`, `cylinder_measures_roles_and_geometry` |
| Tessellation grouped per face, outward, with edge polylines | done | `tessellation_is_closed_outward_and_grouped_by_face` |
| Mass properties incl. inertia; bounding box; validity | done | `box_inertia_about_centre_of_mass`, `box_measures_and_topology` |
| STEP export and re-import | done | `step_round_trip_preserves_volume_and_topology` |
| Hostile input never panics; stale handles rejected | done | `hostile_input_is_rejected_not_panicking`, `released_and_foreign_handles_are_invalid` |
| Cancellation (between operations) | partial | `cancellation_stops_new_operations`; cannot interrupt an operation already inside OCCT |
| STL export (binary, ASCII) | done | `crates/io/src/stl.rs` tests |
| CLI: version, demo, info, convert, `--json` | done | `apps/tenon-cli/tests/cli.rs` (5 tests) |
| Demo file | done | `examples/m0-bracket` (STEP + STL), checked by `demo_writes_step_and_stl_that_read_back` |
| Desktop app: layout shell (ribbon, browser, orientation cube, nav bar, status) | partial | renders (`renders_every_tab_and_toggle_without_panicking`, `docs/images/m0-shell.png`); no modelling yet: the UI does not call the kernel |

Kernel trait operations that exist but return `Unsupported` in the OCCT backend today:
`make_face`, `extrude`, `revolve`, `sweep`, `loft`, `fillet`, `chamfer`, `shell`, `hole`,
`transform`, `pattern`.

## M1 sketch to solid

| Feature | Status | Proof / gap |
|---|---|---|
| 3D viewport (wgpu: orbit/pan/zoom, shaded + edges) | missing | |
| Orientation cube driving the view | missing | drawn in M0, not wired |
| Face/edge selection (picking) | missing | meshes already group triangles per face |
| Sketch on plane or face: line, arc, circle, rectangle, polygon, spline | missing | |
| Sketch edit: trim, offset, mirror, fillet | missing | |
| Constraints and dimensions, live solve, DOF | missing | solver to port from CADCraft (DEC-005) |
| Extrude (add/cut), revolve | missing | kernel types defined |
| Model browser driven by the feature tree | missing | static in M0 |
| Native project save/load | missing | format direction approved (DEC-004) |
| Export STEP/STL from the app | missing | works from the CLI |
| Command registry shared by UI/CLI/MCP; MCP server | missing | |
| Demo: bracket with holes | missing | M0 bracket is built from primitives |

## M2 parametric modelling

| Feature | Status |
|---|---|
| Fillet, chamfer, hole (simple/counterbore/countersink), shell, rib | missing |
| Rectangular/circular patterns, mirror | missing |
| Work planes, axes, points | missing |
| Parameters and expressions | missing |
| Rollback marker, suppress, reorder | missing |
| Persistent naming resolver with edit-upstream tests | missing (design: docs/persistent-naming.md) |
| Broken-feature reporting | missing |

## M3 assemblies, M4 drawings, M5 breadth and polish, M6 native kernel

All missing. Scope per milestone: `docs/plan.md`.
