# Roadmap and feature parity

Status values: **done** (a named test proves it), **partial** (the gap is stated), **missing**.
Nothing is marked done without a test. Milestone scope: `docs/plan.md`. Decisions:
`docs/decisions.md`.

Current milestone: **M2 parametric modelling, complete** (2026-10-08; CI on Linux, macOS and Windows).
Next: M3 assemblies.

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
| Desktop app: layout shell (ribbon, browser, orientation cube, nav bar, status) | done | superseded by the M1 workbench below; `renders_every_tab_and_size_without_a_kernel` |

Kernel operations that return `Unsupported` in the OCCT backend today: `sweep`, `loft`,
`fillet`, `chamfer`, `shell`, `hole`, `pattern`, tapered extrude, extrude to a face.

## M1 sketch to solid

Demo: [examples/m1-bracket](examples/m1-bracket) (a parametric bracket with holes, built by a command
script that checks its own volumes). UI tests drive the workbench through raw egui pointer and key
events, the way the windowing layer does.

| Feature | Status | Proof / gap |
|---|---|---|
| Kernel: planar faces from profiles (lines, arcs, circles, B-splines, holes), extrude, revolve, transform | done | `crates/kernel-occt/tests/m1.rs` (12 tests, e.g. `region_with_a_circular_hole`, `revolve_full_partial_symmetric`, `extrude_then_cut_keeps_tags_through_the_boolean`) |
| Sketch geometry: point, line, arc (centre / three-point), circle, rectangle, polygon, spline | done | `crates/sketch/src/tests.rs` (`polygon_and_three_point_arc`, `regions_with_holes_splits_and_nesting`, `serde_round_trip_and_validation`) |
| Sketch edit: fillet, trim, offset, mirror, delete | done | `fillet_rounds_a_corner_tangentially`, `trim_lines_and_circles`, `offset_rectangle_and_circle`, `mirror_across_a_line`, `delete_cleans_up_points_and_constraints` |
| Constraints (13 geometric, 7 dimensional), live solve, DOF, conflict and redundancy refusal, drag | done | `rectangle_dof_counts_down_to_zero`, `every_geometric_constraint_solves`, `dimensions_drive_geometry`, `conflicting_and_redundant_constraints_are_refused`, `drag_keeps_constraints` |
| Profile regions (nesting, holes, default selection) | done | `regions_with_holes_splits_and_nesting` |
| Sketcher UI: tools, snapping, constraint glyphs, dimensions, DOF in the status bar | partial | real clicks: `sketching_with_real_clicks_and_drags` (line chain closes on its first point, Escape, fast point drag), `dimension_tool_creates_a_driving_dimension`, DOF text in `sketch_extrude_and_sketch_on_the_top_face_through_the_ui`. Arc, polygon, spline, trim, mirror and the constraint tools are wired (`every_available_command_has_a_handler`), but no click-level test covers them yet |
| Sketch on an origin plane or a planar face | done | `sketch_extrude_and_sketch_on_the_top_face_through_the_ui`, `bracket_with_holes_regenerates_after_an_upstream_edit` |
| Extrude (distance, symmetric, through all, reverse; join, cut, intersect, new body) | done | `extents_place_the_solid_correctly`, `two_regions_extrude_together`, the M1 demo script |
| Revolve (full, angle, symmetric; about an origin axis or a sketch line) | done | `revolve_full_partial_symmetric`, `revolve_about_an_origin_axis`, `revolve_through_the_axis_is_rejected` |
| Extrude/Revolve panels with live preview | done | `sketch_extrude_and_sketch_on_the_top_face_through_the_ui` (preview volume before OK) |
| Feature tree, regeneration, per-feature status, broken references reported | done | `crates/model/tests/model.rs` (8 tests), `a_lost_face_reference_breaks_the_feature_clearly` |
| Persistent face names through booleans and upstream edits | done | `sketch_dimension_edit_moves_the_hole`, `bracket_with_holes_regenerates_after_an_upstream_edit`, the demo's thickness edit. Scope: faces of extrusions and revolutions; edges and vertices are M2 (docs/persistent-naming.md) |
| Model browser driven by the feature tree | done | rows with status, double-click to edit, context menu; real drags of the End of Part row and feature rows in `end_of_part_and_features_are_dragged_in_the_browser` |
| Undo / redo | done | `bad_commands_change_nothing`, the UI test (`edit.undo`), the demo script (undo after the thickness edit) |
| Regeneration off the UI thread | done | `worker_regenerates_off_thread_and_reports_the_latest`; cancellation stops between kernel operations only (see M0) |
| 3D viewport (wgpu, MSAA, shaded + edges; software fallback) | done | `gpu_viewport_renders_when_an_adapter_exists` (reads the GPU image back and compares it with the software renderer; skips where no adapter exists), `software_render_draws_the_cube_and_encodes_png`, `the_three_faces_of_an_iso_view_shade_differently` |
| Orbit, pan, zoom (mouse and nav bar) | done | `viewport_responds_to_real_pointer_input` (left/right drag, middle drag, wheel), `fit_zoom_pan_orbit` |
| Orientation cube | done | faces, edges and corners glide the view there and do not reach the model, the arrows turn and roll it (`viewport_responds_to_real_pointer_input`, `cube::tests::faces_edges_and_corners_from_the_home_view`); drag orbits; home and context menu |
| Face and edge picking, selection | done | `viewport_responds_to_real_pointer_input` (click selects the top face and names it; background clears), `ray_through_the_centre_hits_the_facing_side`, `edges_are_picked_near_the_pointer_and_not_through_faces`, `every_visible_edge_is_picked_along_its_length_in_perspective`; window and crossing box selection in `viewport_responds_to_real_pointer_input` |
| Native project files (.tenon) | done | `round_trip_keeps_the_document_and_unknown_fields`, `bad_files_are_rejected_with_reasons`, `reopening_projects_does_not_leak_kernel_shapes`; spec in docs/file-format.md |
| Export STEP / STL | done | `save_open_and_export_commands`, `the_m1_demo_script_builds_a_verified_bracket_and_writes_its_files` (reads the STEP back). The app's File menu goes through native dialogs (rfd), which no automated test drives |
| Mass properties (UI window, commands) | done | `model.mass` in the demo script (analytic volumes); the window renders in `sketch_extrude_and_sketch_on_the_top_face_through_the_ui` |
| Command registry shared by UI, scripts and MCP | done | `every_available_command_has_a_handler`, `command_docs_are_generated_from_the_registry` (docs/commands.md) |
| Command scripts (`tenon-cli run`) | done | `a_failing_script_names_the_step_and_keeps_going_no_further`, `references_resolve_into_nested_values`, `expectations_compare_numbers_relatively` |
| MCP server (stdio): run command, tree, topology, measure, render PNG, export | done | `mcp_server_builds_measures_and_renders_a_part`, `mcp_transport_survives_garbage`; docs/mcp.md |
| MCP control of a running desktop app | missing | the server drives its own headless session (DEC-014) |
| PNG rendering without a GPU (CLI, MCP) | done | `tenon-cli render`; `mcp_server_builds_measures_and_renders_a_part` |
| Performance budget in CI | done | `m1_demo_part_regenerates_and_tessellates_within_budget` (500 ms each; measured 6 ms); docs/performance.md |
| Demo: bracket with holes | done | `examples/m1-bracket`; `the_m1_demo_script_builds_a_verified_bracket_and_writes_its_files` |

## M2 parametric modelling

Complete 2026-10-08. Model tests: `crates/model/tests/m2.rs` (20, plus one ignored timing test), `crates/model/tests/params.rs` (3); kernel:
`crates/kernel-occt/tests/m2.rs` (9); UI (real pointer and key input): `crates/ui/src/tests.rs`; demo:
`apps/tenon-cli/tests/m2.rs`.

| Feature | Status | Proof / gap |
|---|---|---|
| Fillet (constant radius, several edges) | done | `fillets_follow_upstream_edits`, `fillet_one_edge_and_its_history`, `fillet_chamfer_and_shell_pick_edges_and_faces_in_the_viewport` (clicks add and remove edges; editing shows the part rolled back) |
| Chamfer (equal, two distances, distance and angle) | done | `chamfers_meet_at_the_corners`, `chamfers_equal_and_unequal`, `bad_fillets_chamfers_and_shells_are_errors`, the UI test above |
| Shell (inside or outside, open faces) | done | `shell_follows_its_open_face`, the UI test above |
| Hole (simple, counterbore, countersink; blind with drill point or flat, through all) | done | `holes_simple_counterbore_and_countersink`, `a_hole_follows_its_point_and_keeps_its_edges`, `bad_holes_are_refused_or_reported`, `hole_takes_sketch_points_and_toggles_them_in_the_viewport`. Gap: no tapped or clearance holes (M5), no hole placed by clicking a face |
| Rib (to next or finite, flip) | done | `a_rib_fills_the_corner_of_an_l_bracket`, `rib_from_the_sketch_being_drawn`, `a_cut_in_two_gives_its_solids_with_their_faces`. Gap: lines only, in the sketch plane (no ribs normal to the sketch); the profile lines are taken from the sketch, not picked one by one |
| Rectangular and circular patterns, mirror (of features, and of patterns) | done | `rectangular_pattern_of_a_hole_follows_the_hole`, `circular_pattern_full_and_partial`, `mirror_a_cut_and_a_boss_across_an_origin_plane`, `patterns_refuse_what_they_cannot_copy`, `pattern_and_mirror_pick_features_in_the_viewport` (DEC-022). Gap: "through all" extents are sized for the original only |
| Work planes (offset, angle, midplane), axes (edge or cylinder, two planes), points (circle centre, axis through plane) | done | `a_work_plane_offset_from_a_face_carries_a_sketch_and_follows_it`, `angled_and_mid_planes_mirror_like_origin_planes`, `work_axes_and_points`, `work_plane_from_a_face_carries_a_sketch_and_origin_axes_come_from_the_browser`. Gap: drawn over the part rather than depth-tested; no three-point plane |
| Parameters and expressions (named dimensions and values, user parameters, equations in every value field, Parameters dialog) | done | `expr::tests` (2), `dimensions_and_feature_values_are_named_and_driven_by_equations`, `equations_on_new_dimensions_angles_and_counts`, `files_without_parameters_get_names_when_opened`, `equations_typed_into_fields_and_the_parameters_dialog` (DEC-023). Gap: units are converted, not checked; no parameter export or linking to a spreadsheet |
| End of Part marker, suppress, reorder | done | `end_of_part_rolls_back_and_new_features_go_above_it`, `features_reorder_with_their_sketches_but_not_before_what_they_use`, `end_of_part_and_features_are_dragged_in_the_browser` (real drags); suppress: `patterns_refuse_what_they_cannot_copy` |
| Persistent naming resolver with edit-upstream tests (faces and edges, through fillets, holes and patterns) | done | `fillets_follow_upstream_edits`, `edge_references_survive_shortening_and_added_sketch_geometry`, `a_split_face_keeps_each_piece_by_geometry`, `references_survive_reordering_independent_features`, `every_face_of_m2_features_is_named`, `a_hole_follows_its_point_and_keeps_its_edges`, `rectangular_pattern_of_a_hole_follows_the_hole` (a chamfer on a copied hole's edge), `a_work_plane_offset_from_a_face_carries_a_sketch_and_follows_it`. Gaps: vertices are not referenced; splits are told apart by geometry, not ordinals; a sign-flipped dimension is not tested (docs/persistent-naming.md) |
| Broken-feature reporting | done | `a_lost_edge_breaks_the_fillet_with_a_clear_message`, `bad_holes_are_refused_or_reported`, `patterns_refuse_what_they_cannot_copy` (a suppressed source), `work_axes_and_points` (parallel planes); the browser shows the failing feature in red with the message (drawn in every UI test frame, not asserted) |
| Measure (area, length, diameter, distance, angle) | done | `minimum_distances_between_faces_edges_and_shapes`, `measure_areas_lengths_distances_and_angles`, `measure_faces_and_edges_by_clicking_them`, the worker test |
| Incremental regeneration (resume from the edited feature) | done | `regeneration_resumes_from_the_feature_being_edited` (same result as from scratch, no leaks); timing `regeneration_speed` (ignored test): editing the last of 42 features 166 ms -> 9.3 ms, release build |
| UI frame budget | done | `frame_time_stays_within_budget` (16 ms; measured 0.08 ms idle, 0.85 ms with the pointer moving, release, 40 features; CPU side only) |
| Inventor-familiar UI (layout and workflow; DEC-019 to DEC-021) | partial | ribbon, browser, properties panel, mini toolbar, orientation cube (faces, edges, corners), radial menu, navigation, sketch workflow, all exercised by the UI tests. Gaps: no in-canvas drag handles for fillet radius or hole depth; no hover highlight preview of a feature's result before clicking; work planes not depth-tested |
| Demo: parametric enclosure that regenerates when dimensions change | done | `examples/m2-enclosure` (parameters L, W, H, t drive the box, shell, boss pattern and cable hole); `the_m2_enclosure_script_builds_a_verified_parametric_enclosure` (analytic volume after every feature, after changing L and H, and after reopening and changing W). Second example: `examples/m2-mount`, `the_m2_demo_script_builds_a_verified_parametric_mount` |

## M3 assemblies, M4 drawings, M5 breadth and polish, M6 native kernel

All missing. Scope per milestone: `docs/plan.md`.
