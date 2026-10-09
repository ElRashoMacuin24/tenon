# Roadmap and feature parity

Status values: **done** (a named test proves it), **partial** (the gap is stated), **missing**.
Nothing is marked done without a test. Milestone scope: `docs/plan.md` (amended 2026-10-09,
DEC-028). Decisions: `docs/decisions.md`.

**Positioning.** Tenon is for makers, students and small shops who cannot afford the commercial
tools and find the free ones rough. It wins on reliability, coherence and speed, not on feature
count: when two tasks compete, the one that makes existing features more dependable and pleasant
goes first.

Current milestone: **M5 reliability and project format** (started 2026-10-09). M4 drawings was
confirmed by the owner on 2026-10-09. Done so far: format version 2, plain text (DEC-031), with
`tenon-cli diff` and `upgrade`; kernel failures explained in plain words (DEC-032); undo and
redo verified through failing rebuilds; autosave and crash recovery (DEC-033).

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
| 3D viewport (wgpu, MSAA, shaded + edges; software fallback) | done | `gpu_viewport_renders_when_an_adapter_exists` (reads the GPU image back and compares it with the software renderer; skips where no adapter exists, which includes the CI runners, so the GPU path is checked on development machines only), `software_render_draws_the_cube_and_encodes_png`, `the_three_faces_of_an_iso_view_shade_differently` |
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
| Performance budget in CI | partial | `m1_demo_part_regenerates_and_tessellates_within_budget` (500 ms each; measured 6 ms); docs/performance.md. Gap: the budget is 80 times the measured time, so it catches only gross regressions (real benchmarks with tight budgets are M9) |
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
| Hole (simple, counterbore, countersink; blind with drill point or flat, through all) | done | `holes_simple_counterbore_and_countersink`, `a_hole_follows_its_point_and_keeps_its_edges`, `bad_holes_are_refused_or_reported`, `hole_takes_sketch_points_and_toggles_them_in_the_viewport`. Gap: no tapped holes (M6) or clearance holes (M7), no hole placed by clicking a face |
| Rib (to next or finite, flip) | done | `a_rib_fills_the_corner_of_an_l_bracket`, `rib_from_the_sketch_being_drawn`, `a_cut_in_two_gives_its_solids_with_their_faces`. Gap: lines only, in the sketch plane (no ribs normal to the sketch); the profile lines are taken from the sketch, not picked one by one |
| Rectangular and circular patterns, mirror (of features, and of patterns) | done | `rectangular_pattern_of_a_hole_follows_the_hole`, `circular_pattern_full_and_partial`, `mirror_a_cut_and_a_boss_across_an_origin_plane`, `patterns_refuse_what_they_cannot_copy`, `pattern_and_mirror_pick_features_in_the_viewport` (DEC-022). Gap: "through all" extents are sized for the original only |
| Work planes (offset, angle, midplane), axes (edge or cylinder, two planes), points (circle centre, axis through plane) | done | `a_work_plane_offset_from_a_face_carries_a_sketch_and_follows_it`, `angled_and_mid_planes_mirror_like_origin_planes`, `work_axes_and_points`, `work_plane_from_a_face_carries_a_sketch_and_origin_axes_come_from_the_browser`. Gap: drawn over the part rather than depth-tested; no three-point plane |
| Parameters and expressions (named dimensions and values, user parameters, equations in every value field, Parameters dialog) | done | `expr::tests` (2), `dimensions_and_feature_values_are_named_and_driven_by_equations`, `equations_on_new_dimensions_angles_and_counts`, `files_without_parameters_get_names_when_opened`, `equations_typed_into_fields_and_the_parameters_dialog` (DEC-023). Gap: units are converted, not checked; no parameter export or linking to a spreadsheet |
| End of Part marker, suppress, reorder | done | `end_of_part_rolls_back_and_new_features_go_above_it`, `features_reorder_with_their_sketches_but_not_before_what_they_use`, `end_of_part_and_features_are_dragged_in_the_browser` (real drags); suppress: `patterns_refuse_what_they_cannot_copy` |
| Persistent naming resolver with edit-upstream tests (faces and edges, through fillets, holes and patterns) | done | `fillets_follow_upstream_edits`, `edge_references_survive_shortening_and_added_sketch_geometry`, `a_split_face_keeps_each_piece_by_geometry`, `references_survive_reordering_independent_features`, `every_face_of_m2_features_is_named`, `a_hole_follows_its_point_and_keeps_its_edges`, `rectangular_pattern_of_a_hole_follows_the_hole` (a chamfer on a copied hole's edge), `a_work_plane_offset_from_a_face_carries_a_sketch_and_follows_it`. Gaps: vertices are not referenced; splits are told apart by geometry, not ordinals; a sign-flipped dimension is not tested (docs/persistent-naming.md) |
| Broken-feature reporting | partial | the model side: `a_lost_edge_breaks_the_fillet_with_a_clear_message`, `bad_holes_are_refused_or_reported`, `patterns_refuse_what_they_cannot_copy` (a suppressed source), `work_axes_and_points` (parallel planes). Gaps: the browser's red row and message are drawn but no test asserts them; kernel errors reach the user in OCCT's words; no repair (all three are M5) |
| Measure (area, length, diameter, distance, angle) | done | `minimum_distances_between_faces_edges_and_shapes`, `measure_areas_lengths_distances_and_angles`, `measure_faces_and_edges_by_clicking_them`, the worker test |
| Incremental regeneration (resume from the edited feature) | done | `regeneration_resumes_from_the_feature_being_edited` (same result as from scratch, no leaks); timing `regeneration_speed` (ignored test): editing the last of 42 features 166 ms -> 9.3 ms, release build |
| UI frame budget | done | `frame_time_stays_within_budget` (16 ms mean frame; measured 0.08 ms idle, 0.85 ms with the pointer moving, release, 40 features). Limits: CPU side only (no GPU time); the budget is far above the measurement, so only gross regressions fail it |
| Inventor-familiar UI (layout and workflow; DEC-019 to DEC-021) | partial | ribbon, browser, properties panel, mini toolbar, orientation cube (faces, edges, corners), radial menu, navigation, sketch workflow, all exercised by the UI tests. Gaps: no in-canvas drag handles for fillet radius or hole depth; no hover highlight preview of a feature's result before clicking; work planes not depth-tested |
| Demo: parametric enclosure that regenerates when dimensions change | done | `examples/m2-enclosure` (parameters L, W, H, t drive the box, shell, boss pattern and cable hole); `the_m2_enclosure_script_builds_a_verified_parametric_enclosure` (analytic volume after every feature, after changing L and H, and after reopening and changing W). Second example: `examples/m2-mount`, `the_m2_demo_script_builds_a_verified_parametric_mount` |

## M3 assemblies

Demo: [examples/m3-pivot](examples/m3-pivot) (four part files and an assembly, built and checked by
a command script).

Complete 2026-10-08. Solver tests: `crates/assembly/src/solve.rs` (6) and `math.rs` (2). Files:
`crates/io/tests/asm.rs` (2). Commands and demo: `apps/tenon-cli/tests/m3.rs` (2). UI (real pointer
and key input): `crates/ui/src/asm_tests.rs` (5). Worker slots: `worker_regenerates_off_thread_and_reports_the_latest`.

| Feature | Status | Proof / gap |
|---|---|---|
| Assembly documents: `.tenonasm` linking `.tenon` part files by relative path (DEC-024) | done | `assemblies_round_trip_with_part_paths_relative_to_the_file`, `damaged_and_mistaken_assembly_files_are_refused`; a moved folder and a missing part file: `the_m3_demo_script_builds_a_verified_pivot_assembly`. Gap: no sub-assemblies; parts cannot be embedded in the assembly file |
| Insert (place) components; the first is grounded at the origin | done | the demo script (`asm.insert`), `constrain_and_joint_by_clicking_faces_in_the_view` (placing through the workbench) |
| Grounded components | done | `an_assembly_opens_and_components_drag_along_their_joints` (a grounded component does not move), `assembly_edits_undo_and_conflicts_are_refused` |
| Constraints: mate, flush, angle, insert (DEC-025) | done | `mates_and_flushes_place_a_block_on_a_block_and_count_what_is_left`, `angle_and_insert`, the demo script (angle and inserts, analytic placements), `constrain_and_joint_by_clicking_faces_in_the_view` (flush by clicking two faces, with its preview). Gap: no tangent constraint; no limits |
| Joints: rigid, rotational (revolute), slider, plus cylindrical, planar, ball | done | `joints_leave_their_motions` (each joint's free motions and the total), the demo script (rotational and slider), `constrain_and_joint_by_clicking_faces_in_the_view` (rotational by clicking). Gap: joint origins come from faces, circular edges and axes; no vertex or mid-edge snap points; no joint limits or motion studies |
| Conflicts refused with the relationships they conflict with | done | `a_conflict_does_not_converge`, `assembly_edits_undo_and_conflicts_are_refused`, the refused flush in `constrain_and_joint_by_clicking_faces_in_the_view` |
| Dragging components under their relationships; Free Rotate | done | `dragging_keeps_the_dragged_body_near_the_pointer`, `an_assembly_opens_and_components_drag_along_their_joints` (slider drag, undo, Free Rotate about an insert), `assembly_drag_frame_time_stays_within_budget` (median frame under 16 ms; 2.8 ms release, 5.5 ms in the test profile) |
| Degrees-of-freedom display | done | `joints_leave_their_motions`, `dof_interference_parts_list_and_explode_from_the_ribbon` (per component and total); the symbols are drawn by `asm_overlays` (not asserted pixel by pixel) |
| Interference check | done | `dof_interference_parts_list_and_explode_from_the_ribbon` (108π mm³ between a pin head and the block), the demo script |
| Bill of materials (with CSV export) | done | the demo script (`asm.bom`, `asm.export_bom`), `the_m3_demo_script_builds_a_verified_pivot_assembly` (the CSV), `dof_interference_parts_list_and_explode_from_the_ribbon`. Gap: no part numbers, descriptions or materials (materials: M6) |
| Exploded view (steps, auto explode, trails) | done | the demo script (`asm.explode.positions`), `dof_interference_parts_list_and_explode_from_the_ribbon`. Gap: no animation; no rotation steps |
| In-context editing of parts (with the rest of the assembly shown) | done | `editing_a_part_in_place_and_returning_updates_the_assembly` (double-click, edit, Return, the assembly follows, Save saves the part), the demo's `asm.edit_part`. Gap: no references from one part to another's geometry (adaptive parts) |
| Assembly STEP export | partial | `the_m3_demo_script_builds_a_verified_pivot_assembly` (volume read back). Gap: placed solids only, no product structure |

## M4 drawings (confirmed by the owner 2026-10-09)

Demo: [examples/m4-plate](examples/m4-plate) (a plate, a pin, their assembly and a two-sheet drawing,
built and checked by a command script).

The first pass was reported 2026-10-08. The plan amendment of 2026-10-09 raised the bar
(auto-dimension suggestions, sheet and title-block templates, clean PDF/SVG/DXF, views that follow
model changes, all with tests); the rows below are rated against it, and all are met on
2026-10-09 (centre marks and centrelines placed by hand joined drawing format v1 by the owner's
choice, DEC-029).
Hidden-line removal: `crates/kernel-occt/tests/m4.rs` (2). Drawing crate: `crates/drawing/src` (8).
Sheet raster: `lines_dashes_and_text_land_on_white_paper`. Files: `crates/io/tests/drw.rs` (5).
Commands and demo: `apps/tenon-cli/tests/m4.rs` (6). UI (real pointer and key input):
`crates/ui/src/drw_tests.rs` (10).

| Feature | Status | Proof / gap |
|---|---|---|
| Drawing documents: `.tenondrw` linking part and assembly files by relative path (DEC-026) | done | `drawings_round_trip_with_model_paths_relative_to_the_file`, `damaged_and_mistaken_drawing_files_are_refused`; a moved folder and missing model files: `the_m4_demo_script_draws_a_plate_and_its_assembly` |
| Sheets: ANSI A to D, ISO A4 to A1, several per drawing | done | the demo script (two B sheets), `drawing_edits_undo_and_bad_input_is_refused` (a sheet resized to A refits the scale), `views_and_dimensions_are_placed_by_clicking_on_the_sheet`, `models_edited_from_the_drawing_update_it` (the browser switches sheets) |
| Standards: ANSI third-angle by default, ISO first-angle per drawing (DEC-027) | done | `frames_follow_the_projection_angle` (both angles, section frames unfolded from their parent). Gap: no per-company drafting standard settings (text heights, arrow styles) |
| Base, projected and isometric views with hidden-line removal | done | `a_box_seen_from_the_front_is_its_front_rectangle`, `a_cylinder_shows_its_silhouettes_and_a_hole_its_hidden_lines`, `hatching_clipping_and_covered_hidden_lines`, the demo script (every view's direction and size), `views_and_dimensions_are_placed_by_clicking_on_the_sheet` (placed by clicking, lined up with their parent), `assembly_views_hide_one_part_behind_another_and_sections_cut_every_part` (a part behind another is drawn hidden, nothing of it visible). Gaps: no shaded views; tangent edges off by default with no per-view style beyond hidden lines |
| Section views (full, hatched) | done | `hatching_clipping_and_covered_hidden_lines`, the demo script, `views_and_dimensions_are_placed_by_clicking_on_the_sheet` (line clicked, seen from the side it is placed), `assembly_views_hide_one_part_behind_another_and_sections_cut_every_part` (every part of an assembly cut, each hatched its own way: 45 and 135 degrees, then wider). Gap: straight cutting lines only (no offset, aligned or half sections) |
| Detail views | done | `hatching_clipping_and_covered_hidden_lines` (clipped to the circle), the demo script (2:1), `dialogs_menus_navigation_and_details_by_real_input` (view, centre, radius and place clicked). Gap: circular boundary only |
| Associative dimensions: horizontal, vertical, aligned, diameter, radius, angle | done | the demo script (values picked from the views; the plate made thicker and the drawing updated: 12 becomes 16), `models_edited_from_the_drawing_update_it` (16 becomes 20 after editing from the drawing), `views_and_dimensions_are_placed_by_clicking_on_the_sheet` (the type chosen from what is clicked), `aligned_radius_and_angle_dimensions_measure_the_model_and_follow_it` (a fillet's radius, a chamfer's true length and its angles in each sector, all following parameter changes), `an_arc_dimensions_as_a_radius_and_meeting_lines_as_an_angle` (chosen from clicks). Gaps: no ordinate, baseline or chain dimensions; no tolerances; model dimensions cannot be retrieved into the drawing |
| Auto-dimension suggestions | done | for a base, projected or section view: overall width and height from the edges that span it, each size of hole or boss (with its count, "4X Ø8") and of round, placed outside the view, minus what its dimensions already give; added as ordinary associative dimensions in one undo step: `suggested_dimensions_cover_the_part_once_and_follow_it` (the plate's views; nothing suggested twice; the thickness follows the model), `auto_dimension_suggests_reviews_and_adds_in_one_step` (the tool, the review dialog with its preview on the sheet, one undo). Gaps: no hole positions (the hole table gives them), no chained or baseline sets, isometric and detail views are left to the user |
| Centre marks and centrelines | done | drawn automatically for holes and cylinders seen end-on (centre marks) or side-on (centrelines), and in sections only for what is left: `the_m4_demo_script_draws_a_plate_and_its_assembly` (a centre mark through each of the plate's five holes in the top view). Placed by hand (DEC-029): a centre mark on any circle or arc, a centreline through two picked places (circle centres, line middles, points) and a centreline bisector midway between two lines (parallel or meeting), kept as edge picks so they follow the model: `centre_marks_and_centrelines_placed_by_hand_follow_the_model` (an arc's mark moves when its radius changes, after saving and reopening; bad picks refused; on the CENTER layer in DXF), `centre_marks_and_centrelines_by_clicking` (the three tools, real clicks, one undo each). Gap: no centred pattern (a bolt circle's centreline through a pattern of holes) |
| Hole tables | done | the demo script (positions and descriptions from the hole features), `tables_balloons_and_text_through_the_tools`. Gap: holes made by Hole features only, not by patterns of them or by cut extrusions |
| Balloons (placed and automatic) and parts lists from the bill of materials | done | the demo script, `tables_balloons_and_text_through_the_tools` (a balloon attached where the pin's edge was clicked; auto balloon; a parts list placed with the tool), `models_edited_from_the_drawing_update_it` (the list follows the assembly), `drawing_edits_undo_and_bad_input_is_refused` (the parts list equals the assembly's bill of materials row for row, also after parts are added; both come from `bom_with`). Gap: no custom columns or part numbers beyond the file name |
| Title block templates | done | a Tenon title block per standard with fields from `drw.props` and the projection symbol (`svg_pdf_and_dxf_hold_the_sheet`, the demo's PDF); templates saved, edited and applied as JSON files: `title_block_templates_round_trip_and_bad_ones_are_refused`, `drawing_edits_undo_and_bad_input_is_refused`, `models_edited_from_the_drawing_update_it` (from the Manage tab). Gap: templates are edited as text, not drawn in the app; the border is fixed |
| Sheet (drawing) templates: a new drawing starting from saved sheets, standard and properties | done | any drawing saved as a template (sheets, borders, title blocks, standard, properties, notes; no views) starts new drawings: `drawings_start_from_templates_and_save_as_templates`, `new_drawings_start_from_templates_and_save_as_templates` (File menu commands; the ISO template's first-angle projection checked); ANSI B and ISO A3 templates shipped in `assets/templates` (made by `templates.json`). Gap: no list of templates to choose from in the app; the user picks a file |
| Export to PDF, SVG and DXF | done | strokes written as true circles and arcs, polylines without needless points, nothing twice; text in the drafting font everywhere, with the words kept as invisible, searchable text in PDF and SVG (`circles_arcs_lines_and_duplicates`, `svg_pdf_and_dxf_hold_the_sheet`). Read back as other programs read them: the PDF's cross-references, stream lengths and page tree (`check_pdf`), the SVG by an XML parser, the DXF's sections, layers, line types, polylines and extents (`check_dxf`); broken files are caught (`broken_files_are_caught_by_the_checks`). On the demo: `the_m4_demo_script_draws_a_plate_and_its_assembly` (2 PDF pages; the holes as SVG and DXF circles; 195 DXF entities where there were 1317 segments); `models_edited_from_the_drawing_update_it` (from the File menu). Checked by eye in pdf.js and a browser (2026-10-09). Gap: DXF is R12 (no line weights) |
| Views update when the model changes | done | the demo script (`drw.update` after the model file changed), `models_edited_from_the_drawing_update_it` (Open Model, edit, Return; saving the drawing saves the model), `views_are_computed_on_the_geometry_thread`; files saved by another program or window: `model_files_changed_on_disk_are_read_again_unless_changed_in_the_drawing` (read again, unsaved changes made from the drawing kept), `a_model_saved_elsewhere_updates_the_open_drawing` (within a second, without input) |
| Drawing environment in the UI (Place Views and Annotate ribbons, sheet that pans and zooms, browser, dragging and deleting, dialogs) | done | `crates/ui/src/drw_tests.rs` with real pointer and key input; dialogs, the context menu, panning and zooming: `dialogs_menus_navigation_and_details_by_real_input` |
| Demo: a drawing of a part and its assembly that follows a model change | done | `examples/m4-plate`, `the_m4_demo_script_draws_a_plate_and_its_assembly`, `drawing_edits_undo_and_bad_input_is_refused` |

## M5 reliability and project format

Scope: `docs/plan.md` section 10. Started 2026-10-09, after the owner confirmed M4. The
plain-text format was proposed in [docs/format-v2-proposal.md](docs/format-v2-proposal.md),
approved by the owner (DEC-031) and is specified in [docs/file-format.md](docs/file-format.md).

| Feature | Status | Proof / gap |
|---|---|---|
| Broken-reference repair: show what broke, highlight candidates, re-pick in one step | missing | |
| Per-feature errors in plain language, in the browser | done | kernel failures are explained for the feature that hit them (DEC-032): what could not be made, the likely reason and what to try, then the kernel's own words in brackets: `each_feature_says_what_it_could_not_do_and_what_to_try` (every feature type), `kernel_failures_are_explained_in_plain_words_and_never_pass_silently` (a fillet too large, a cut that removes everything, and a shell the kernel "built" without hollowing anything, which used to pass as a success); shown in the viewport and on the failing browser row by real pointer input: `a_failing_feature_says_why_in_the_viewport_and_its_browser_row`. References that break say which feature and face are gone (`a_lost_edge_breaks_the_fillet_with_a_clear_message`). Gap: the reason is the likely one for the feature type, not a diagnosis of the geometry (no "largest radius that fits") |
| Autosave and crash recovery | done | DEC-033: every 30 s, unsaved work is copied to a recovery folder of the app's own (locked while that Tenon runs); after a crash the next start asks "Recover unsaved work?" (Recover, Discard, Not Now). Recovered work opens as unsaved edits over the files, so Save writes it back and Undo returns to the saved file. Storage: `a_running_tenons_copies_are_not_offered_and_a_crashed_ones_are`, `saved_work_clears_the_copies_and_damaged_folders_are_dropped`. Real input, `crates/ui/src/recovery_tests.rs`: `unsaved_part_work_survives_a_crash_and_comes_back_on_request` (Enter recovers; the file untouched until Save; Ctrl+Z, Ctrl+Y, Ctrl+S; a proper close leaves nothing), `not_now_keeps_the_work_for_next_time_and_discard_drops_it` (a part never saved), `a_drawing_comes_back_with_the_model_edited_from_it` (each copy back where it was changed, though the drawing also shows the part inside an assembly), `a_running_tenons_work_is_never_offered_to_another`. Gap: a crash is simulated by dropping the workbench without closing; the app's own exit hook (`on_exit`) is not driven by a test |
| Unsaved changes are never dropped silently: "Save changes?" (Save, Don't Save, Cancel) before New, Open and Exit and on the window's close button, covering a drawing with the models changed from it and an assembly with the part edited in place (DEC-030) | done | real pointer and key input in `crates/ui/src/save_tests.rs`: `a_changed_part_asks_before_new_and_open_and_each_answer_does_what_it_says` (Cancel, Esc, Enter, a closed save dialog, a part saved before; keys do not reach the part behind), `an_assembly_and_the_part_edited_in_place_are_asked_about_together`, `a_drawing_and_the_model_edited_from_it_are_asked_about_together` (Delete pressed under the prompt deletes nothing), `a_drawing_its_assembly_and_a_part_in_place_in_it_are_saved_whole`, `exit_and_the_window_close_button_ask_first`. Gap: the app's part of the close button (answering the window system's close request with `CancelClose`, `apps/tenon/src/main.rs`) is not driven by a test; the workbench call it makes is |
| Undo/redo verified across regeneration failures | done | `undo_and_redo_through_failing_rebuilds_match_a_rebuild_from_scratch`: a fixed walk of 160 edits, undos and redos on a filleted, shelled and cut block, failures included (a fillet too large, walls too thick); after every step the rebuild the app makes (resuming from its cache) equals one from scratch, feature by feature and body by body, and each failing edit undoes to exactly the document and part before it and redoes to the same failure. In the app, by real keys: `a_failing_feature_says_why_in_the_viewport_and_its_browser_row` (Ctrl+Z after the failure rebuilds the part whole and clears the message, which used to stay in the status bar; Ctrl+Y brings it back). Gap: assemblies and drawings are covered by their own undo tests, not by a walk like this |
| Text-based, diffable, versioned `.tenon` with migrations | done | format version 2 (DEC-031, `docs/file-format.md`): parts, assemblies and drawings as plain TOML text with a fixed layout, one field per line and one list item per line; version-1 zips read and upgraded on save, the original kept once as `name.v1.ext`; `tenon-cli upgrade`. Tests: the writer and reader (`layout_is_fixed_and_reads_back`, `numbers_keep_their_kind_and_value`, `crlf_a_byte_order_mark_and_odd_keys_read`, `bad_text_says_where`, `objects_named_as_sub_tables_follow_their_record`, `a_part_lays_out_and_comes_back`, `clashes_and_strays_are_caught_when_writing`); `crates/io/tests/format_v2.rs`: `saving_over_a_version_1_file_keeps_it_once`, `damaged_text_is_refused_with_its_line_and_record` (wrong types and unknown feature types name the record and its line; two features with one id name both), `names_with_quotes_backslashes_newlines_and_other_scripts_round_trip`, `a_parameter_change_and_a_new_feature_merge_cleanly` (with `git merge-file`; the merge opens and regenerates); `upgrade_rewrites_version_1_files_and_keeps_them`. Gap: two branches that each add a feature still give both the same id (refused clearly, not renumbered) |
| `tenon diff` at feature and parameter level | done | `tenon-cli diff A B` and the `file.diff` command: parameters, features (added, removed, moved, renamed, suppressed, each changed field, sketch dimensions by name), values that follow from a changed parameter marked "(from H)", assemblies' components, relationships and explode steps, drawings' sheets, views and annotations; any format version; exit code 0, 1 or 2. Tests: `diff_names_parameters_and_features_and_what_follows_from_what`, `diff_reports_assemblies_and_drawings_and_refuses_mixed_kinds`, `diff_says_what_changed_and_exits_like_diff`. Gap: no Git merge driver yet |
| Round-trip stability, migrations from every older version, persistent-naming corpus | partial | every version-1 example and template is kept in `crates/io/tests/fixtures/v1` and upgrades to the text checked in beside it, byte for byte on every OS, and saves the same again (`every_version_1_file_upgrades_to_the_text_beside_it_and_saves_the_same_again`); upgraded parts regenerate the same solids (`upgraded_parts_regenerate_the_same_solids`) and upgraded assemblies solve with every relationship holding (`upgraded_assemblies_solve_with_every_relationship_holding`). Gap: the persistent-naming corpus of edit-then-regenerate cases |

## M6 part feature breadth

| Feature | Status | Proof / gap |
|---|---|---|
| Sweep, loft, coil (helix), threads (cosmetic and modelled), draft, split, combine | missing | the ribbon buttons exist and say which milestone brings them |
| Mass properties with materials and appearance, in a Properties panel | partial | volume, area, centre of mass and inertia at unit density (`box_inertia_about_centre_of_mass`, the Mass Properties window); no materials, density or appearance |
| Design tables (configurations from a table of parameters) | missing | |

## M7 standard parts and maker workflow

| Feature | Status | Proof / gap |
|---|---|---|
| Standard parts library (fasteners, bearings, extrusion profiles), parametric or open-licensed data | missing | |
| 3D-print checks (wall thickness, overhangs, watertightness) | missing | |
| 3MF export; STL export hardened | partial | STL binary and ASCII (`crates/io/src/stl.rs` tests); no 3MF |

## M8 sheet metal basics

| Feature | Status | Proof / gap |
|---|---|---|
| Base and edge flanges, bends with K-factor, corner relief | missing | |
| Flat pattern with DXF export, checked against hand-computed bend allowances | missing | |

## M9 performance

| Feature | Status | Proof / gap |
|---|---|---|
| Benchmarks in CI with budgets that fail on regression (regeneration, tessellation, assembly load, memory) | partial | loose budgets only (see M1, M2 and M3 rows) |
| Per-feature regeneration caching | done | `regeneration_resumes_from_the_feature_being_edited` (M2) |
| Per-face tessellation caching, parallel tessellation, level of detail for large assemblies | missing | |
| Published numbers in docs/performance.md, including where OCCT is the limit | partial | M1 and M2 numbers only |

## M10 agent-native workflow

| Feature | Status | Proof / gap |
|---|---|---|
| Versioned JSON schemas for every command | missing | commands document their parameters in prose (docs/commands.md) |
| MCP tools for build, inspect, render | partial | `mcp_server_builds_measures_and_renders_a_part`; no repair-reference tool |
| Natural-language-to-feature helpers and AI-assisted reference repair (as normal undoable commands) | missing | waits for M5's reliability gates |

## M11 quick stress checks (linear static)

| Feature | Status | Proof / gap |
|---|---|---|
| Materials, supports, loads; meshing and solving as external processes; results on the model | missing | the licence analysis of the external mesher and solver goes to the owner first |
| Verified against textbook benchmarks in CI | missing | |

## Experimental: native kernel

Not started (plan section 10): only after M5 and M6, unless the owner says otherwise. An
operation's default moves to the native kernel only after it matches OCCT on the differential
corpus.

## Not planned now

CAM, generative design, cable and harness, tube and pipe, dynamic simulation.
