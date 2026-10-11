# Matching Inventor's workflow

Tenon's goal is that someone who knows Autodesk Inventor can sit down and work: the same
commands in the same places, started and finished the same way. This page tracks that goal for
the sketch and part-feature workflow: what Inventor does (from Autodesk's published help, linked
at the end), what Tenon does, and what is still missing. It is a working list, kept in the order
things should be done.

Tenon copies behaviour, never artwork: no icons, images, names or text of the product are used
(DEC-019), and no screenshots of it are kept in this repository.

## Sketch dimensions

| Inventor | Tenon | Proof / gap |
|---|---|---|
| One Dimension command (D). What it makes follows what is picked: a line gives its length, a circle its diameter, an arc its radius, two points or parallel lines the distance between them, two crossing lines their angle | done | `the_pointer_chooses_between_level_upright_and_aligned`, `a_dimension_follows_the_pointer_is_put_down_by_a_click_and_can_be_dragged` |
| Pick the geometry, then click where the dimension goes; its value box opens on it | done | the same test: nothing is made until the placing click; the box opens there; Enter accepts |
| Between two points or on a sloping line, where the pointer is decides between a horizontal, a vertical and an aligned dimension | done | the same test: under the line reads its width, beside it its height, off its end its length |
| Drawn as a dimension: extension lines from the geometry, a dimension line with arrowheads, the value on the line; a leader through the centre for a diameter, from the centre for a radius; an arc for an angle | done | `a_length_has_extension_lines_a_dimension_line_and_two_arrowheads`, `a_short_dimension_puts_its_arrowheads_outside_and_reaches_a_value_placed_beyond`, `a_diameter_runs_through_the_centre_and_a_radius_from_it`, `an_angle_is_an_arc_between_its_lines_with_an_arrowhead_on_each` |
| A dimension is dragged to where it reads best, and stays there | done | the placement is saved with the dimension (`at` on its record, DEC-041): `a_placed_dimension_keeps_its_place_on_its_own_record`, `a_dimension_keeps_the_place_it_was_put_until_it_is_gone`; dragged and undone by real input in the placement test |
| Double-click a dimension to edit it | done (M1) | `dimensions_are_typed_into_a_box_on_the_dimension` |
| Dimension display, from the status bar: Value, Name, Expression, Tolerance, Precise Value | partial | Value, Name ("d0") and Expression ("d0 = 40", "d1 = d0 / 2"): `the_status_bar_changes_how_dimensions_read_and_hides_constraint_symbols`. Gap: Tolerance and Precise Value (Tenon has no dimension tolerances yet) |
| A dimension driven by an equation is marked "fx:" | done (M2) | the same test |
| A dimension that would over-constrain the sketch becomes a driven dimension, shown in parentheses | done | DEC-047: the Dimension tool puts it down as driven at once and says so in the status bar (as with Inventor's option to apply driven dimensions without asking). `a_dimension_too_many_is_put_down_driven_and_a_click_changes_driven_and_driving`, `a_driven_dimension_follows_the_sketch_and_holds_nothing` |
| Driven Dimension on the Format panel changes a dimension from driving to driven and back | done | a tool: click the dimension. The same UI test (refused, with the reason, when the sketch is held without it). Gap: Inventor's button also works on dimensions selected beforehand, and as a mode while dimensioning |
| A driven dimension has a parameter name that equations can read (a reference parameter) | done | `a_driven_dimension_is_taken_where_one_more_would_be_too_many_and_equations_read_it` (a block's height follows its base's diagonal; a dimension of the same sketch may not, and the message says why). It cannot be set, by value, equation or design table; the Parameters dialog shows it as "driven" |
| A dimension from a centre line reads as a diameter | done | DEC-056: the Dimension tool, given a centre line and a point or a line parallel to it, makes a diameter drawn across the axis and read with Ø. `a_dimension_to_a_centre_line_is_a_diameter`, `a_diameter_to_a_centre_line_spans_the_axis_from_the_point_to_its_mirror_image`; by real input `a_centre_line_gives_diameters_and_is_the_axis_revolve_takes` |
| Automatic Dimensions and Constraints finishes a sketch | missing | |
| The value's text turns with the dimension line | done | it lies along the line and reads from the left or from below: `a_value_lies_along_its_dimension_line_and_reads_from_the_left_or_from_below` |

## Constraints in sketches

| Inventor | Tenon | Proof / gap |
|---|---|---|
| The status bar says how many dimensions the sketch still needs, or that it is fully constrained | done (M1) | `sketch_extrude_and_sketch_on_the_top_face_through_the_ui` ("4 dimensions needed"), `typed_values_and_inference_constrain_while_drawing` ("Fully Constrained") |
| Geometry changes colour once it is fully constrained | done (M1) | |
| Show All Constraints (F8) and Hide All Constraints (F9), also on the status bar | done | `the_status_bar_changes_how_dimensions_read_and_hides_constraint_symbols`. They show by default in Tenon (Inventor hides them until asked) |
| Constraint symbols are small pictures in a row beside the geometry; one can be selected and deleted | done | DEC-056: a picture per constraint, at each of the two things it holds together; pointing at one lights it and its geometry, a click selects it, Delete removes the constraint. `a_constraint_symbol_is_pointed_at_selected_and_deleted`. Gap: a symbol cannot be dragged aside; no right-click menu on it |
| Degrees-of-freedom symbols | missing | |
| Constraints inferred while drawing, shown as they are about to apply | partial (M1) | horizontal and vertical only |

## Features made from a sketch

| Inventor | Tenon | Proof / gap |
|---|---|---|
| With one closed profile it is chosen for you; with several, click the regions to use | done | `profiles_are_chosen_by_clicking_the_regions_of_the_sketch` (Extrude; the same picking serves Revolve, Sweep and Coil). The chosen regions are outlined boldly over the sketch |
| Curves that cross divide the sketch into regions that can each be picked (a line through a circle gives two halves) | done | DEC-046: lines, arcs and circles divide one another where they cross, touch, or end on one another. `a_line_through_a_circle_gives_two_halves_and_both_together_the_circle`, `overlapping_circles_give_a_lens_and_two_crescents`, `touching_curves_meet_at_one_place_and_leave_no_sliver`; by real clicks `a_line_through_a_circle_divides_it_into_halves_that_are_clicked_apart`. Gap: a spline divides nothing where it crosses another curve midway (it joins at its ends only) |
| Regions picked side by side make one profile: the curve between them leaves no face | done | `a_circle_divided_by_a_line_is_used_whole_or_by_halves` (both halves are one cylinder with one round wall), `a_plate_divided_by_a_line_keeps_its_plain_keys_and_its_holes` |
| The region under the pointer is highlighted before it is clicked | done | its outline in the hover colour: the click test above. Gap: Inventor shades the region; Tenon outlines it |
| Revolve takes a centre line as its axis without being asked; otherwise the axis is clicked in the graphics window | done | DEC-039, DEC-056: the one centre line of the sketch, else a construction line, else the one line that bounds no profile, else an origin axis in the sketch's plane; a sketch line, work axis or origin axis can be clicked. `revolve_turns_a_circle_into_a_sphere_and_takes_a_clicked_line_for_its_axis` |
| Centerline is a line format of its own (Format panel), drawn as a chain line | done | Sketch > Format > Centerline, on what is selected (DEC-056). It still bounds profiles, so a half section closes along its own axis. `a_centre_line_is_the_axis_and_dimensions_to_it_are_diameters`. Gap: no Center Point format |
| A sketch stays visible until a feature uses it; a used sketch can be shared and shown again | partial | unused sketches show (DEC-037); a used sketch cannot be shared with a second feature from the browser yet |
| A feature previews as it is set up, with a mini-toolbar at the pointer and a Properties panel | done (M2) | |
| Extrude tapers from its panel | done | DEC-040, `an_extrusion_is_tapered_from_its_advanced_properties` |

## What to do next, in order

1. ~~Driven dimensions: accept an over-constraining dimension as a reference, in parentheses.~~ Done (DEC-047).
2. ~~Regions from crossing curves, so half of a divided profile can be picked.~~ Done (DEC-046).
3. ~~A Centerline format, with diameter dimensions measured from it.~~ Done (DEC-056).
4. ~~Constraint symbols as pictures that can be selected and deleted.~~ Done (DEC-056).
5. ~~Dimension text turned along its line.~~ Done (DEC-056). Tolerances on dimensions are still to do.
6. For the part as a whole (the first table below): an appearance for one body or face; design table
   rows that suppress features or set the material; choosing the size while placing a part.
7. Standard parts and printing (the second table below): a length asked for when a section is
   placed; a fastener that takes its size from the hole it is dropped on; the printer's build
   space drawn round the part.
8. The Sketch tab's remaining buttons (the tables below): Stretch, Text, ellipses, the Center
   Point format, sketch patterns that stay tied to their original, automatic dimensions.
9. Sheet metal (M8): the Sheet Metal tab, its defaults, Face, Flange, Bend and the flat pattern.

## Geometry of the part in a sketch

| Inventor | Tenon | Proof / gap |
|---|---|---|
| Project Geometry (Sketch > Create) shows edges of the part in the sketch as reference geometry; the sketch's own geometry is constrained and dimensioned to it | done | DEC-055: the tool takes clicks on edges, or on a face for all its edges. `project_geometry_shows_a_clicked_edge_or_face_in_the_sketch`, `a_hole_dimensioned_from_a_projected_edge_follows_the_part`. Gap: vertices, work features and other sketches' curves cannot be projected; edges that would be ellipses or splines on the plane are refused, saying so |
| Projected geometry is associative: it moves when the part does | done | found again at every regeneration by the names of the edge's two faces, and the document's sketch takes up where it is: `projected_edges_follow_the_part_on_the_geometry_thread`, `a_profile_is_bounded_by_projected_edges_and_the_sketchs_own_line`, `an_equation_reads_a_dimension_to_a_projected_edge` |
| It is drawn in a colour of its own and cannot be dragged or trimmed | done | amber; `reference_geometry_stays_where_it_is_put_and_the_rest_follows_it` |
| Break Link makes projected geometry the sketch's own | done | the right-click menu: `projected_geometry_is_deleted_unlinked_and_says_when_its_edge_is_gone` |
| Geometry whose edge is gone is shown in magenta and can be pointed at another edge | partial | magenta, and `sketch.info` names it; the part still builds. Gap: no dialog to choose another edge (delete it and project again) |
| A new sketch on a face starts with the face's edges projected (Application Options > Sketch > "Autoproject edges for sketch creation and edit") | done | on at first, as there; the option is in Application Options. `a_sketch_on_a_face_shows_its_edges_and_a_hole_is_dimensioned_from_one` |
| "Autoproject edges during curve creation": pointing at an edge while drawing projects it | missing | |
| "Autoproject part origin on sketch create" | done (M1) | the origin point of every sketch |
| Project Cut Edges, Project Flat Pattern, Project to 3D Sketch | missing | |
| With several closed loops a feature asks for its profile | done differently | the sketch's own closed geometry is taken before the projected outline, so a circle drawn on a face extrudes as the circle without a click; any region can still be clicked (`what_the_sketch_draws_itself_is_the_default_profile_before_what_it_shows_of_the_part`) |

## The Sketch tab, button by button

Create panel:

| Inventor | Tenon | Proof / gap |
|---|---|---|
| Line, with an arc swept out of its end by dragging | partial | Line (L). Gap: no arc from a line by dragging; use Arc > Tangent |
| Circle: Center Point; Tangent (to three lines); Ellipse | partial | by centre. Gap: tangent circle, ellipse |
| Arc: Three Point; Tangent; Center Point | done | the drop-down under Arc: `slots_rectangles_and_arcs_are_drawn_by_their_own_clicks`, `a_tangent_arc_carries_on_from_the_end_of_a_line_or_an_arc` |
| Rectangle: Two Point; Three Point; Two Point Center; Three Point Center | partial | two point, three point, two point centre. Gap: three point centre |
| Slot: Center to Center; Overall; Center Point; Three Point Arc; Center Point Arc | partial | the three straight slots, under Rectangle as in Inventor: `slots_and_rectangles_about_a_centre_or_at_an_angle_hold_their_shape` (a slot is two lines tangent to two equal arcs, five degrees of freedom). Gap: the two arc slots |
| Polygon, with a number of sides and inscribed or circumscribed | partial | a hexagon. Gap: the dialog |
| Spline: Control Vertex; Interpolation; Equation Curve; Bridge Curve | partial | control vertex. Gap: the rest |
| Fillet and Chamfer | partial | Fillet. Gap: sketch chamfer |
| Text, Geometry Text | missing | |
| Point | done (M1) | |

Modify panel:

| Inventor | Tenon | Proof / gap |
|---|---|---|
| Move, with Copy as a tick box; a base point and an end point, or Precise Input | done | Move and Copy as two buttons; clicks, or values typed beside the pointer (DEC-056). `geometry_is_moved_copied_turned_and_scaled_with_the_modify_tools` |
| Rotate about a centre point by an angle typed or clicked | done | the same test |
| Scale about a base point by a factor | done | the same test; dimensions within what is scaled are scaled |
| A dialog asks whether constraints and dimensions in the way may be relaxed | done differently | level and upright constraints in what is turned are dropped (swapped by quarter turns); a fixed point or a dimension to the rest of the sketch refuses the motion and says which. `geometry_is_moved_turned_and_scaled_and_what_is_joined_to_it_follows` |
| Trim (X); Extend; Split | done | `curves_are_extended_and_split_by_a_click`, `a_curve_is_extended_to_the_next_one_in_its_way`, `curves_are_split_where_another_crosses_them`. Gap: Inventor previews what a click would do; Shift does not yet switch between Trim and Extend |
| Offset | done (M1) | |
| Stretch | missing | on the ribbon, greyed |

Pattern panel:

| Inventor | Tenon | Proof / gap |
|---|---|---|
| Rectangular: geometry, two directions, each with a count and a spacing | done | a direction is a click on a line; counts and spacings typed in a window, the copies shown before OK. `sketch_patterns_are_set_up_in_a_window_after_a_direction_or_a_centre_is_clicked` |
| Circular: geometry, an axis point, a count and an angle | done | the same test |
| Mirror | done (M1) | |
| Associative: the copies follow the original, and the count can be edited afterwards | missing | DEC-056: the copies are geometry of their own, with the constraints that keep their shape. Needs a pattern constraint in the solver |
| Suppress single occurrences; Fitted | missing | |

Constrain and Format panels: the constraints are all there (M1, M2); Automatic Dimensions and
Constraints, Show Constraints for a selection, and Constraint Settings are missing. Format has
Construction, Centerline and Driven Dimension; Center Point and Show Format are missing.

## The part as a whole: physical properties and sizes

| Inventor | Tenon | Proof / gap |
|---|---|---|
| The Physical tab of a part's properties: its material and density, then mass, area, volume and centre of gravity | done | Inspect > Properties (DEC-043): `a_part_is_given_a_material_a_density_and_a_colour_in_its_properties`. Gap: the window does not list moments of inertia (the `model.mass` command returns them), and a mass or volume cannot be overridden by hand |
| The values are recalculated on Update; out-of-date values read N/A | done differently | Tenon's values follow every change, so there is no Update button and nothing goes stale |
| The material comes from a library and brings its density; a part with the default material has a density of 1 | done | a library of 22 materials; without one a part is at 1 g/cm³. Gap: no library of one's own kept between parts (a material typed in stays with its part); no properties beyond density and colour |
| An appearance is shown in place of the material's own, and can be set for one body or one face | partial | one colour for the whole part (twelve swatches or a typed `#rrggbb`). Gap: bodies and faces cannot have their own; no textures or finishes |
| An assembly's physical properties add up its components, each with its own material | done | `an_assembly_weighs_and_shows_each_component_in_its_parts_material`; components are also drawn in their part's colour |
| The bill of materials can list each part's material and mass | done | the Material and Mass columns of the parts list and its CSV: the same test, `the_m6_demo_script_builds_four_verified_fittings_and_weighs_their_assembly` |
| A part family is a table: each row is a member, each column a value that differs between members; one row is the default | done | Manage > Design Table (DEC-044): `a_design_table_is_made_from_ticked_parameters_and_its_rows_resize_the_part`. The ticked row is the size the part is at |
| Columns can also suppress features, set the material and appearance, or hold other properties | missing | columns are parameters only |
| The member is chosen for each occurrence of the part in an assembly | done | DEC-048: a component names a row of its part's design table, and components of one file may be different sizes. `a_component_is_shown_in_its_size_and_changes_size_from_the_browser` (right-click, Size), `components_of_one_part_file_come_in_the_sizes_of_its_design_table`. Gap: the size is not asked for while placing; it is changed afterwards |
| Each member is generated as a file of its own | not planned | Tenon works a size out from the one part file each time, so there is nothing to keep in step (DEC-048). A copy of one size can be had by saving the part at that row |
| Key columns order how members are listed; the table can be edited as a spreadsheet | missing | |

## Standard parts and 3D printing

| Inventor | Tenon | Proof / gap |
|---|---|---|
| Place from Content Center is on the Assemble tab's Component panel | done | Assemble > Component > Standard Parts: `standard_parts_are_placed_from_the_library_in_the_size_picked` |
| The dialog lists categories (fasteners, shaft parts, structural shapes) that open to families; a family is picked, then its member in the Family dialog's Select or Table View tab | done, in one window | DEC-049: the families by kind on the left, the picked family's sizes on the right with what the size comes to. Gap: no search, history or favourites (there are ten families); sizes are one list, not a thread and a length picked apart |
| As Standard keeps the member's file in the project's Content Center Files folder; As Custom makes a file of one's own that can be edited | done differently | a placed standard part's file goes into a folder named `standard` beside the assembly, one file for all its sizes (DEC-048, DEC-049). The custom kind is Manage > Standard Parts in a part: a new part started from the family, to edit and save as one's own (`a_part_is_started_from_a_standard_one_in_the_library`) |
| The placed part is positioned with constraints like any other | done | `standard_parts_are_placed_in_sizes_held_together_and_changed` (a screw inserted in a washer, both changed size, still held) |
| Change Size on a placed part's right-click menu | done | the Size menu of DEC-048: `a_component_is_shown_in_its_size_and_changes_size_from_the_browser` |
| AutoDrop: a bolt, nut, washer or bearing dragged to a hole or shaft takes its size from it and is constrained there | missing | the size is picked from the list and the part constrained by hand |
| Structural shapes are placed at a length typed in | partial | a section's length is its parameter `L`: set in a part started from the library. A section placed straight into an assembly is at the length its file has. Gap: no length asked for on placing; two lengths need two part files |
| Libraries hold many standards (ISO, DIN, ANSI, JIS) and can be added to and published | missing | ISO metric fasteners M3 to M20, seventeen bearings, five kinds of section. No way to add a family without writing code |
| The 3D Print environment (Environments tab, parts only) shows the part in the printer's build space, Z up; the part is turned and moved in it, a printer is chosen, an oversized part is partitioned | partial | Environments > 3D Print Check (DEC-051): which way is up is one of six directions, the bed is three sizes typed in, and the part is said to fit or not. `the_print_check_window_reads_the_part_and_marks_what_it_finds`. Gap: the build space is not drawn; no list of printers; no free turning; no partitioning |
| (Inventor's environment does not check the part) | Tenon adds | whether the surface is closed, what overhangs past a limit and how thin the thinnest wall is, with the faces tinted in the model. A maker's first three questions, answered before the slicer is opened |
| The part is sent on as STL; resolution is set in the export's options | done | File > Export STL and Export 3MF, for parts and assemblies (DEC-052): `files_for_the_slicer_come_from_the_file_menu_for_parts_and_assemblies`. Gap: no options dialog in the app (the tolerance is the usual one; the commands take `linear` and `angular`); nothing is sent to a printer or a slicer directly |

## Sources

Behaviour described above is from Autodesk's help for Inventor:

- [About Sketch Constraints](https://help.autodesk.com/cloudhelp/2026/ENU/Inventor-Help/files/GUID-FAF69614-E8F2-4763-975C-552E0BEA1DD1.htm) (normal and driven dimensions, dimension display, degrees of freedom, the status bar's count)
- [To work with dimensions in sketches](https://help.autodesk.com/cloudhelp/2023/ENU/Inventor-Help/files/GUID-026FE8D3-AFBB-4834-BC18-73D9CFF78185.htm) (placing, editing, driven dimensions)
- [Dimension commands](https://help.autodesk.com/cloudhelp/2014/ENU/Inventor/files/GUID-05281AB1-87BD-4C9E-95CE-177BB702F774.htm) (one command; the type follows the selection)
- [Change display style of dimension value](https://help.autodesk.com/cloudhelp/2014/ENU/Inventor/files/GUID-1144C51A-18CB-4461-BF4B-71BE054EBD98.htm) and the [sketch status bar commands](https://help.autodesk.com/cloudhelp/2014/ENU/Inventor/files/GUID-938F7A77-3FDA-417E-8FB0-4241FCEBE313.htm)
- [To create revolved features](https://help.autodesk.com/cloudhelp/2020/ENU/Inventor-Help/files/GUID-401BABD2-8D5B-4300-BA8D-70037445044F.htm) (a centre line is taken as the axis)
- [iProperties, Physical tab](https://help.autodesk.com/cloudhelp/2022/ENU/Inventor-Help/files/GUID-877564AC-78CE-4216-9913-003A2DE8D698.htm) and [About Mass Properties](https://help.autodesk.com/cloudhelp/2022/ENU/Inventor-Help/files/GUID-2D7AC6B0-7B1E-4F1A-900F-C98A0705D47A.htm) (material and density, mass, area, volume, centre of gravity, Update)
- [To Set Physical Properties and Appearances of Part Bodies and Faces](https://help.autodesk.com/cloudhelp/2026/ENU/Inventor-Help/files/GUID-549F82A0-E151-4995-8C60-1E602E5128CA.htm)
- [iParts](https://help.autodesk.com/cloudhelp/2022/ENU/Inventor-Help/files/GUID-60919937-2247-4C32-B9C9-6045D751FFF9.htm) and the [iPart Author dialog box](https://help.autodesk.com/cloudhelp/2015/ENU/Inventor-Help/files/GUID-B7820955-D0F8-478A-8965-64ACD5CA0B54.htm) (rows are members, columns the values that differ, keys, the default row)
- [To Insert a Content Center Part Manually](https://help.autodesk.com/cloudhelp/2026/ENU/Inventor-Help/files/GUID-97683B08-1AEC-4371-9199-F14C9A932A6C.htm), [About Inserting Content Center Components in Assemblies](https://help.autodesk.com/cloudhelp/2019/ENU/Inventor-Help/files/GUID-0BBBD2A4-9628-4B14-854C-4C95155E9FF0.htm) and [To Insert Parts Using AutoDrop](https://help.autodesk.com/cloudhelp/2022/ENU/Inventor-Help/files/GUID-0018159B-2D2D-4816-9BC0-3973DE755AE1.htm) (the Component panel, categories and families, the Family dialog, As Standard and As Custom, AutoDrop)
- [To Access the 3D Print Environment](https://help.autodesk.com/cloudhelp/2017/ENU/Inventor-Help/files/GUID-72511A09-F17C-4A93-8274-B953FA0139B6.htm) (parts only, Z up, orientation, position, partitioning, printers)
- The Sketch tab's panels (Create, Modify, Pattern, Constrain, Format), Project Geometry and the
  Sketch page of Application Options are from the same help's pages on sketch commands and
  options; they are not linked one by one here.
