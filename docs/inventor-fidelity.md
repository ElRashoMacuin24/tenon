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
| A dimension that would over-constrain the sketch is offered as a driven dimension, shown in parentheses | missing | Tenon refuses the dimension as redundant. Next on this list |
| A dimension from a centre line reads as a diameter | missing | needs the Centerline format below |
| Automatic Dimensions and Constraints finishes a sketch | missing | |
| The value's text turns with the dimension line | missing | Tenon keeps the text level; the line is broken round it |

## Constraints in sketches

| Inventor | Tenon | Proof / gap |
|---|---|---|
| The status bar says how many dimensions the sketch still needs, or that it is fully constrained | done (M1) | `sketch_extrude_and_sketch_on_the_top_face_through_the_ui` ("4 dimensions needed"), `typed_values_and_inference_constrain_while_drawing` ("Fully Constrained") |
| Geometry changes colour once it is fully constrained | done (M1) | |
| Show All Constraints (F8) and Hide All Constraints (F9), also on the status bar | done | `the_status_bar_changes_how_dimensions_read_and_hides_constraint_symbols`. They show by default in Tenon (Inventor hides them until asked) |
| Constraint symbols are small pictures in a row beside the geometry; one can be selected and deleted | partial | Tenon shows a letter per constraint ("H", "V", "//"), not a picture, and they cannot be clicked. Gap |
| Degrees-of-freedom symbols | missing | |
| Constraints inferred while drawing, shown as they are about to apply | partial (M1) | horizontal and vertical only |

## Features made from a sketch

| Inventor | Tenon | Proof / gap |
|---|---|---|
| With one closed profile it is chosen for you; with several, click the regions to use | done | `profiles_are_chosen_by_clicking_the_regions_of_the_sketch` (Extrude; the same picking serves Revolve, Sweep and Coil). The chosen regions are outlined boldly over the sketch |
| Curves that cross divide the sketch into regions that can each be picked (a line through a circle gives two halves) | missing | Tenon finds only regions bounded by curves that meet at their ends (`crates/sketch/src/profile.rs`). The largest gap here; it needs regions keyed by more than their boundary curves, which is a file-format question |
| Revolve takes a centre line as its axis without being asked; otherwise the axis is clicked in the graphics window | done | DEC-039: a construction line, else the one line that bounds no profile, else an origin axis in the sketch's plane; a sketch line, work axis or origin axis can be clicked. `revolve_turns_a_circle_into_a_sphere_and_takes_a_clicked_line_for_its_axis` |
| Centerline is a line format of its own (Format panel), drawn as a chain line | missing | Tenon uses Construction for the same purpose. Needed for diameter dimensions |
| A sketch stays visible until a feature uses it; a used sketch can be shared and shown again | partial | unused sketches show (DEC-037); a used sketch cannot be shared with a second feature from the browser yet |
| A feature previews as it is set up, with a mini-toolbar at the pointer and a Properties panel | done (M2) | |
| Extrude tapers from its panel | done | DEC-040, `an_extrusion_is_tapered_from_its_advanced_properties` |

## What to do next, in order

1. Driven dimensions: accept an over-constraining dimension as a reference, in parentheses.
2. Regions from crossing curves, so half of a divided profile can be picked.
3. A Centerline format, with diameter dimensions measured from it.
4. Constraint symbols as pictures that can be selected and deleted.
5. Dimension text turned along its line; tolerances on dimensions.

## Sources

Behaviour described above is from Autodesk's help for Inventor:

- [About Sketch Constraints](https://help.autodesk.com/cloudhelp/2026/ENU/Inventor-Help/files/GUID-FAF69614-E8F2-4763-975C-552E0BEA1DD1.htm) (normal and driven dimensions, dimension display, degrees of freedom, the status bar's count)
- [To work with dimensions in sketches](https://help.autodesk.com/cloudhelp/2023/ENU/Inventor-Help/files/GUID-026FE8D3-AFBB-4834-BC18-73D9CFF78185.htm) (placing, editing, driven dimensions)
- [Dimension commands](https://help.autodesk.com/cloudhelp/2014/ENU/Inventor/files/GUID-05281AB1-87BD-4C9E-95CE-177BB702F774.htm) (one command; the type follows the selection)
- [Change display style of dimension value](https://help.autodesk.com/cloudhelp/2014/ENU/Inventor/files/GUID-1144C51A-18CB-4461-BF4B-71BE054EBD98.htm) and the [sketch status bar commands](https://help.autodesk.com/cloudhelp/2014/ENU/Inventor/files/GUID-938F7A77-3FDA-417E-8FB0-4241FCEBE313.htm)
- [To create revolved features](https://help.autodesk.com/cloudhelp/2020/ENU/Inventor-Help/files/GUID-401BABD2-8D5B-4300-BA8D-70037445044F.htm) (a centre line is taken as the axis)
