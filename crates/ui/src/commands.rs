//! The ribbon's command table. Every button maps to a named command id (`area.verb`). Buttons that
//! edit the model end in the shared command registry (`tenon_model::cmd`, `tenon_io::cmd`); the
//! rest are UI actions (tools, views).
//!
//! The tabs, panels and button sizes follow the established mechanical-CAD ribbon so the workflow
//! feels familiar (DEC-019); labels are plain functional words, icons are Tenon's own.

use crate::icons::Icon;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Size {
    /// Icon above a label (one or two lines, split at `\n`).
    Large,
    /// Small icon then label, three to a column.
    Small,
    /// Icon only, three to a column (constraint grids).
    Icon,
}

#[derive(Clone, Copy, Debug)]
pub struct UiCommand {
    pub id: &'static str,
    pub label: &'static str,
    pub tip: &'static str,
    pub icon: Icon,
    pub size: Size,
    /// Single-key shortcut, shown in the tooltip.
    pub key: Option<&'static str>,
    /// Commands under the button's drop-down arrow.
    pub more: &'static [&'static str],
}

impl UiCommand {
    /// The label on one line (for menus, search and tooltips).
    pub fn name(&self) -> String {
        self.label.replace('\n', " ")
    }
    const fn key(mut self, k: &'static str) -> Self {
        self.key = Some(k);
        self
    }
    const fn more(mut self, ids: &'static [&'static str]) -> Self {
        self.more = ids;
        self
    }
}

#[derive(Clone, Copy, Debug)]
pub struct RibbonPanel {
    pub title: &'static str,
    pub commands: &'static [UiCommand],
}

#[derive(Clone, Copy, Debug)]
pub struct RibbonTab {
    pub name: &'static str,
    pub panels: &'static [RibbonPanel],
}

const fn cmd(id: &'static str, label: &'static str, tip: &'static str, icon: Icon, size: Size) -> UiCommand {
    UiCommand { id, label, tip, icon, size, key: None, more: &[] }
}
const fn large(id: &'static str, label: &'static str, tip: &'static str, icon: Icon) -> UiCommand {
    cmd(id, label, tip, icon, Size::Large)
}
const fn small(id: &'static str, label: &'static str, tip: &'static str, icon: Icon) -> UiCommand {
    cmd(id, label, tip, icon, Size::Small)
}
const fn glyph(id: &'static str, label: &'static str, tip: &'static str, icon: Icon) -> UiCommand {
    cmd(id, label, tip, icon, Size::Icon)
}

pub const MODEL_TAB: usize = 0;
pub const SKETCH_TAB: usize = 1;
/// The home tab of a sheet metal part; shown only for one.
pub const SHEET_TAB: usize = 7;
/// The tab of the flat pattern; shown only while the pattern is.
pub const FLAT_TAB: usize = 8;
/// The tab of the Stress Analysis environment; shown only in it.
pub const ANALYSIS_TAB: usize = 9;
/// The tab of a 3D sketch; shown only in one, in the Sketch tab's place.
pub const SKETCH3D_TAB: usize = 10;

pub const RIBBON: &[RibbonTab] = &[
    RibbonTab {
        name: "3D Model",
        panels: &[
            RibbonPanel {
                title: "Sketch",
                commands: &[large("sketch.new", "Start\n2D Sketch", "Pick an origin plane or a planar face to sketch on", Icon::NewSketch)
                    .key("S")
                    .more(&["sketch.new_3d"])],
            },
            RibbonPanel {
                title: "Primitives",
                commands: &[large(
                    "model.box",
                    "Box",
                    "A box in a few clicks: a plane or flat face, its centre, a corner, its height",
                    Icon::PrimBox)
                .more(&["model.box", "model.cylinder", "model.sphere", "model.torus"])],
            },
            RibbonPanel {
                title: "Create",
                commands: &[
                    large("model.extrude", "Extrude", "Add or cut material by sweeping a profile straight", Icon::Extrude).key("E"),
                    large("model.revolve", "Revolve", "Add or cut material by rotating a profile about an axis", Icon::Revolve).key("R"),
                    small("model.sweep", "Sweep", "Sweep a profile along a path", Icon::Sweep),
                    small("model.loft", "Loft", "Blend between two or more profiles", Icon::Loft),
                    small("model.coil", "Coil", "Helical sweep", Icon::Coil),
                    small("model.rib", "Rib", "A thin wall from an open profile", Icon::Rib),
                    small(
                        "model.emboss",
                        "Emboss",
                        "A profile or text raised from or recessed into the part's faces, following them where they curve, or laid round a cylinder or cone",
                        Icon::Text),
                    small(
                        "model.derive",
                        "Derive",
                        "Bring in the solid of another part file, scaled and mirrored as you say: it follows that file when the part is rebuilt",
                        Icon::Copy),
                ],
            },
            RibbonPanel {
                title: "Modify",
                commands: &[
                    large("model.hole", "Hole", "Simple, clearance or tapped; with a counterbore, a spotface or a countersink", Icon::Hole).key("H"),
                    large("model.fillet", "Fillet", "Round edges", Icon::Fillet).key("F").more(&["model.fillet", "model.face_fillet"]),
                    small("model.chamfer", "Chamfer", "Bevel edges", Icon::Chamfer),
                    small("model.shell", "Shell", "Hollow a solid, removing faces", Icon::Shell),
                    small("model.draft", "Draft", "Taper faces for moulding", Icon::Draft),
                    small("model.thread", "Thread", "A screw thread on a round shaft or hole: cosmetic, or cut into the part", Icon::Thread),
                    small("model.combine", "Combine", "Join, cut or intersect bodies", Icon::Combine),
                    small("model.split", "Split", "Cut solids in two along a plane, or keep one side", Icon::Split),
                ],
            },
            RibbonPanel {
                title: "Work Features",
                commands: &[
                    large("work.plane", "Plane", "Construction plane: offset, angled, mid-plane or through three points", Icon::Plane),
                    small("work.axis", "Axis", "Construction axis: on an edge or cylinder, or where two planes meet", Icon::Axis),
                    small("work.point", "Point", "Construction point: a circle centre, or where an axis meets a plane", Icon::Point),
                    small(
                        "work.ucs",
                        "UCS",
                        "A coordinate system of your own, by offsets and turns or three points: its planes, axes and origin serve as work features",
                        Icon::Ucs),
                ],
            },
            RibbonPanel {
                title: "Pattern",
                commands: &[
                    small("model.pattern.rect", "Rectangular", "Repeat features in rows and columns", Icon::PatternRect),
                    small("model.pattern.circular", "Circular", "Repeat features around an axis", Icon::PatternCircular),
                    small("model.mirror", "Mirror", "Mirror features across a plane", Icon::Mirror),
                ],
            },
            RibbonPanel {
                title: "Simulation",
                commands: &[large(
                    "analysis.begin",
                    "Stress\nAnalysis",
                    "Say where the part is held and what pushes on it, to see whether it holds",
                    Icon::Stress)],
            },
            RibbonPanel {
                title: "Convert",
                commands: &[large(
                    "sheet.convert",
                    "Convert to\nSheet Metal",
                    "Make this part a sheet metal part: say how thick its sheet is, and it can be laid flat",
                    Icon::ConvertSheet)],
            },
        ],
    },
    RibbonTab {
        name: "Sketch",
        panels: &[
            RibbonPanel {
                title: "Create",
                commands: &[
                    large("sketch.line", "Line", "Click points; each line starts where the last ended. Esc ends the chain.", Icon::Line)
                        .key("L")
                        .more(&["sketch.line", "sketch.spline"]),
                    small("sketch.circle", "Circle", "Click the centre, then a point on the circle", Icon::Circle)
                        .key("C")
                        .more(&["sketch.circle", "sketch.ellipse"]),
                    small("sketch.arc", "Arc", "Three points: click the start, the end, then a point on the arc", Icon::Arc).key("A").more(&[
                        "sketch.arc",
                        "sketch.arc_tangent",
                        "sketch.arc_center",
                    ]),
                    small("sketch.rectangle", "Rectangle", "Click two opposite corners", Icon::Rectangle).more(&[
                        "sketch.rectangle",
                        "sketch.rectangle3",
                        "sketch.rectangle_center",
                        "sketch.slot",
                        "sketch.slot_overall",
                        "sketch.slot_center",
                        "sketch.slot_arc_three",
                        "sketch.slot_arc_center",
                        "sketch.polygon",
                    ]),
                    small("sketch.fillet", "Fillet", "Click a corner where two lines meet", Icon::Fillet),
                    small("sketch.text", "Text", "Click where the text starts, then type it: its letters are profiles to extrude or cut", Icon::Text),
                    small("sketch.point", "Point", "Click to place a point (e.g. a hole centre)", Icon::Point),
                    large(
                        "sketch.project",
                        "Project\nGeometry",
                        "Show edges of the part in the sketch as reference geometry: it follows the part, and the sketch is dimensioned and constrained to it",
                        Icon::Project)
                    .more(&["sketch.project", "sketch.project_flat"]),
                ],
            },
            RibbonPanel {
                title: "Modify",
                commands: &[
                    small("sketch.move", "Move", "Move the selected geometry: click where from and where to, or type the distance", Icon::Move),
                    small("sketch.copy", "Copy", "Copy the selected geometry: click where from, then where each copy goes", Icon::Copy),
                    small(
                        "sketch.rotate",
                        "Rotate",
                        "Turn the selected geometry about a point: click it, then from and to, or type the angle",
                        Icon::Rotate),
                    small("sketch.trim", "Trim", "Click the piece of a curve to remove", Icon::Trim).key("X"),
                    small("sketch.extend", "Extend", "Click a line or an arc near an end: it goes on to the next curve in its way", Icon::Extend),
                    small("sketch.split", "Split", "Click a curve near where another crosses it: it becomes two", Icon::Split),
                    small("sketch.scale", "Scale", "Make the selected geometry larger or smaller about a point", Icon::Scale),
                    small(
                        "sketch.stretch",
                        "Stretch",
                        "Stretch the selected geometry: click where from and where to; what it is joined to stretches, and the dimensions across it follow",
                        Icon::Stretch),
                    small("sketch.offset", "Offset", "Select curves, then choose a distance", Icon::Offset).key("O"),
                ],
            },
            RibbonPanel {
                title: "Pattern",
                commands: &[
                    small(
                        "sketch.pattern.rect",
                        "Rectangular",
                        "Repeat the selected geometry in rows: click a line for the direction",
                        Icon::PatternRect),
                    small("sketch.pattern.circular", "Circular", "Repeat the selected geometry round a point", Icon::PatternCircular),
                    small("sketch.mirror", "Mirror", "Select geometry, then click the mirror line", Icon::Mirror),
                ],
            },
            RibbonPanel {
                title: "Constrain",
                commands: &[
                    large("sketch.dimension", "Dimension", "Click a line, circle, arc, two points or two lines", Icon::Dimension).key("D"),
                    // Column by column, so the grid reads in rows: coincident, collinear,
                    // concentric, fix / parallel, perpendicular, horizontal, vertical /
                    // tangent, smooth, symmetric, equal.
                    glyph("sketch.coincident", "Coincident", "Two points, or a point and a curve", Icon::Coincident),
                    glyph("sketch.parallel", "Parallel", "Two lines", Icon::Parallel),
                    glyph("sketch.tangent", "Tangent", "A line and a circle/arc, or two circles/arcs", Icon::Tangent),
                    glyph("sketch.collinear", "Collinear", "Two lines on one line", Icon::Collinear),
                    glyph("sketch.perpendicular", "Perpendicular", "Two lines", Icon::Perpendicular),
                    glyph("sketch.smooth", "Smooth (G2)", "A spline and the line, arc or spline it ends on: tangent and as sharply bent on both sides", Icon::Smooth),
                    glyph("sketch.concentric", "Concentric", "Two circles/arcs", Icon::Concentric),
                    glyph("sketch.horizontal", "Horizontal", "A line", Icon::Horizontal),
                    glyph("sketch.symmetric", "Symmetric", "Two points, then the line of symmetry", Icon::Symmetric),
                    glyph("sketch.fix", "Fix", "A point that must not move", Icon::Fix),
                    glyph("sketch.vertical", "Vertical", "A line", Icon::Vertical),
                    glyph("sketch.equal", "Equal", "Two lines or two circles/arcs", Icon::Equal),
                ],
            },
            RibbonPanel {
                title: "Format",
                commands: &[
                    small("sketch.construction", "Construction", "Make the selected geometry construction (or normal again)", Icon::Construction),
                    small(
                        "sketch.centerline",
                        "Centerline",
                        "Make the selected line a centre line (or an ordinary line again): a revolution turns about it, and a dimension to it is a diameter",
                        Icon::Centerline),
                    small(
                        "sketch.center_point",
                        "Center Point",
                        "Make the selected points centre points (a hole goes at them) or sketch points (a hole takes them only when picked); with none selected, what the Point tool makes",
                        Icon::CenterPoint),
                    small(
                        "sketch.driven",
                        "Driven Dimension",
                        "Click a dimension to make it driven (it follows the sketch, in parentheses) or driving again",
                        Icon::Driven),
                ],
            },
            RibbonPanel { title: "Exit", commands: &[large("sketch.finish", "Finish\nSketch", "Leave the sketch", Icon::FinishSketch)] },
        ],
    },
    RibbonTab {
        name: "Inspect",
        panels: &[RibbonPanel {
            title: "Measure",
            commands: &[
                large("inspect.measure", "Measure", "Distances, angles, lengths and areas", Icon::Measure),
                large("inspect.mass", "Properties", "Material, appearance, mass, volume and centre of gravity", Icon::MassProps),
            ],
        }],
    },
    RibbonTab {
        name: "Tools",
        panels: &[RibbonPanel {
            title: "Options",
            commands: &[
                large("tools.options", "Application\nOptions", "Colour scheme and other settings", Icon::Settings),
                large("app.about", "About", "Version, licences and kernel information", Icon::Info),
            ],
        }],
    },
    RibbonTab {
        name: "Manage",
        panels: &[
            RibbonPanel {
                title: "Parameters",
                commands: &[
                    large("tools.parameters", "Parameters", "Named parameters and expressions", Icon::Parameters),
                    large(
                        "tools.table",
                        "Design\nTable",
                        "The sizes this part comes in: a row for each, a column for each parameter that differs",
                        Icon::Bom),
                ],
            },
            RibbonPanel {
                title: "Library",
                commands: &[large(
                    "tools.library",
                    "Standard\nParts",
                    "Start a part from a standard one: a screw, nut, washer, bearing or section in its sizes",
                    Icon::StandardPart)],
            },
            RibbonPanel {
                title: "Update",
                commands: &[large("model.rebuild", "Rebuild\nAll", "Regenerate every feature from scratch", Icon::Update)],
            },
        ],
    },
    RibbonTab {
        name: "View",
        panels: &[
            RibbonPanel {
                title: "Appearance",
                commands: &[
                    large("view.style", "Visual\nStyle", "Shaded with edges, shaded, or wireframe", Icon::VisualStyle).more(&[
                        "view.style.shaded_edges",
                        "view.style.shaded",
                        "view.style.wireframe",
                    ]),
                    small("view.orthographic", "Orthographic", "Parallel projection", Icon::Projection),
                    small("view.perspective", "Perspective", "Perspective projection", Icon::Projection),
                ],
            },
            RibbonPanel {
                title: "Windows",
                commands: &[
                    small("view.browser", "Browser", "Show or hide the model browser", Icon::Browser),
                    small("view.cube", "Orientation Cube", "Show or hide the orientation cube", Icon::Cube),
                    small("view.navbar", "Navigation Bar", "Show or hide the navigation bar", Icon::Window),
                ],
            },
            RibbonPanel {
                title: "Navigate",
                commands: &[
                    small("view.home", "Home View", "Three-quarter view of the whole part (F6)", Icon::Home),
                    small("view.fit", "Zoom All", "Fit the model in the window", Icon::ZoomFit),
                    small("view.look_at", "Look At", "Look straight at the selected planar face or the active sketch", Icon::LookAt),
                    small("view.previous", "Previous View", "Go back to the last view (F5)", Icon::PreviousView),
                ],
            },
        ],
    },
    RibbonTab {
        name: "Environments",
        panels: &[
            RibbonPanel {
                title: "Begin",
                commands: &[large(
                    "analysis.begin",
                    "Stress\nAnalysis",
                    "Say where the part is held and what pushes on it, to see whether it holds",
                    Icon::Stress)],
            },
            RibbonPanel {
                title: "3D Print",
                commands: &[large(
                    "inspect.print",
                    "3D Print\nCheck",
                    "How the part would print: a closed surface, overhangs, thin walls, and whether it fits the bed",
                    Icon::Print3d)],
            },
        ],
    },
    RibbonTab {
        name: "Sheet Metal",
        panels: &[
            RibbonPanel {
                title: "Sketch",
                commands: &[large("sketch.new", "Start\n2D Sketch", "Pick an origin plane or a planar face to sketch on", Icon::NewSketch)
                    .key("S")
                    .more(&["sketch.new_3d"])],
            },
            RibbonPanel {
                title: "Create",
                commands: &[
                    large("sheet.face", "Face", "A flat piece of sheet from a closed profile, one thickness thick", Icon::SheetFace),
                    large(
                        "sheet.flange",
                        "Flange",
                        "A wall bent up from an edge of the sheet: click edges, set the height and the angle",
                        Icon::Flange),
                    large(
                        "sheet.contour_flange",
                        "Contour\nFlange",
                        "A strip of sheet along an open profile: lines and arcs drawn end to end, with a bend at every corner",
                        Icon::ContourFlange),
                    small("sheet.lofted_flange", "Lofted Flange", "A sheet between two open profiles: one smooth surface, or flat facets with bends between them", Icon::LoftedFlange),
                    small("sheet.contour_roll", "Contour Roll", "An open profile rolled round an axis as sheet", Icon::ContourRoll),
                    small("sheet.hem", "Hem", "An edge folded back on itself: single, double, rolled or teardrop", Icon::Hem),
                    small(
                        "sheet.bend",
                        "Bend",
                        "Join two faces of the sheet that do not touch with a bend: click an edge of each",
                        Icon::Bend),
                    small("sheet.fold", "Fold", "Fold a face along a line drawn across it, from edge to edge", Icon::Bend),
                ],
            },
            RibbonPanel {
                title: "Modify",
                commands: &[
                    large("sheet.cut", "Cut", "Cut a profile through the sheet", Icon::SheetCut),
                    small("model.hole", "Hole", "Simple, clearance or tapped; with a counterbore, a spotface or a countersink", Icon::Hole).key("H"),
                    small("sheet.corner_round", "Corner Round", "Round the corners of the sheet's outline", Icon::CornerRound),
                    small("sheet.corner_chamfer", "Corner Chamfer", "Cut the corners of the sheet's outline off", Icon::CornerChamfer),
                    small(
                        "sheet.corner_seam",
                        "Corner Seam",
                        "Close the corner between two flanges made apart: their bends ended where they cross, the walls brought to a seam, the corner relieved",
                        Icon::CornerSeam),
                    small("sheet.punch", "Punch Tool", "Stamp a shape from the punch library into a flat face: a cut, a dimple or a louver", Icon::Punch),
                    small(
                        "sheet.rip",
                        "Rip",
                        "Open a seam in a face, so that a closed shape can be laid flat: a slit through a point, a slit between two points, or the whole face out",
                        Icon::Split),
                    small("sheet.unfold", "Unfold", "Lay chosen bends flat from a face that stays, to work across them", Icon::FlatPattern),
                    small("sheet.refold", "Refold", "Fold back the bends an Unfold laid flat, with what was cut across them", Icon::FoldedPart),
                ],
            },
            RibbonPanel {
                title: "Work Features",
                commands: &[
                    large("work.plane", "Plane", "Construction plane: offset, angled, mid-plane or through three points", Icon::Plane),
                    small("work.axis", "Axis", "Construction axis: on an edge or cylinder, or where two planes meet", Icon::Axis),
                    small("work.point", "Point", "Construction point: a circle centre, or where an axis meets a plane", Icon::Point),
                ],
            },
            RibbonPanel {
                title: "Pattern",
                commands: &[
                    small("model.pattern.rect", "Rectangular", "Repeat features in rows and columns", Icon::PatternRect),
                    small("model.pattern.circular", "Circular", "Repeat features around an axis", Icon::PatternCircular),
                    small("model.mirror", "Mirror", "Mirror features across a plane", Icon::Mirror),
                ],
            },
            RibbonPanel {
                title: "Setup",
                commands: &[large(
                    "sheet.defaults",
                    "Sheet Metal\nDefaults",
                    "The sheet's thickness, its bend radius and reliefs, and the K factor its flat pattern is worked out with",
                    Icon::SheetDefaults)],
            },
            RibbonPanel {
                title: "Flat Pattern",
                commands: &[
                    large(
                        "sheet.flat_view",
                        "Create\nFlat Pattern",
                        "Lay the part flat: its outline for cutting, with its bend lines. Made the first time, shown after that",
                        Icon::FlatPattern),
                    small(
                        "sheet.a_side",
                        "Define A-Side",
                        "Click the face that is up when the pattern lies on the table (the selected face, when one is)",
                        Icon::ASide),
                ],
            },
        ],
    },
    RibbonTab {
        name: "Flat Pattern",
        panels: &[
            RibbonPanel {
                title: "Export",
                commands: &[
                    large(
                        "sheet.export_dxf",
                        "Export\nDXF",
                        "The flat pattern for a laser, plasma or waterjet cutter: outline, holes and bend lines on their own layers",
                        Icon::Save),
                    small("sheet.export_svg", "Export SVG", "The flat pattern as a picture at full size", Icon::Save),
                ],
            },
            RibbonPanel {
                title: "Show",
                commands: &[
                    small("sheet.show_bends", "Bend Lines", "Show or hide the centre line of each bend", Icon::Centerline),
                    small("sheet.show_extents", "Bend Extents", "Show or hide where each bend begins and ends", Icon::Parallel),
                    small("view.fit", "Zoom All", "Fit the model in the window", Icon::ZoomFit),
                ],
            },
            RibbonPanel {
                title: "Folded Part",
                commands: &[large("sheet.folded", "Go to\nFolded Part", "Back to the part as it is folded", Icon::FoldedPart)],
            },
        ],
    },
    RibbonTab {
        name: "Analysis",
        panels: &[
            RibbonPanel {
                title: "Manage",
                commands: &[large(
                    "study.new",
                    "Create\nStudy",
                    "A new study of the part: where it is held and what pushes on it",
                    Icon::StudyNew)],
            },
            RibbonPanel {
                title: "Material",
                commands: &[large(
                    "analysis.material",
                    "Assign",
                    "The part's material: what it is made of says how it answers to a load",
                    Icon::MassProps)],
            },
            RibbonPanel {
                title: "Constraints",
                commands: &[
                    large("study.fix", "Fixed", "Faces that are held still", Icon::Fixed),
                    small("study.pin", "Pin", "Round faces held on a pin: free to turn, not to move off", Icon::Pin),
                    small("study.frictionless", "Frictionless", "Faces that may slide along themselves but not leave", Icon::Frictionless),
                ],
            },
            RibbonPanel {
                title: "Loads",
                commands: &[
                    large("study.force", "Force", "A force spread over faces, square to them or along a direction", Icon::Force),
                    large("study.pressure", "Pressure", "A pressure on faces, square to them", Icon::Pressure),
                    small("study.gravity", "Gravity", "The part's own weight", Icon::Gravity),
                    small("study.bearing", "Bearing", "The load of a shaft in its bore", Icon::BearingLoad),
                    small("study.moment", "Moment", "A turning load on faces", Icon::Moment),
                    small("study.remote_force", "Remote Force", "A force that acts on faces from a point away from them", Icon::RemoteForce),
                    small("study.body", "Body", "The part turning: the load of its own mass, flung outward", Icon::BodyLoad),
                ],
            },
            RibbonPanel {
                title: "Mesh",
                commands: &[
                    large("analysis.mesh_view", "Mesh\nView", "The part divided into the elements the study is worked out on", Icon::MeshGrid),
                    large("study.mesh_size", "Mesh\nSettings", "How large the elements are", Icon::MeshSettings),
                ],
            },
            RibbonPanel {
                title: "Solve",
                commands: &[large(
                    "analysis.simulate",
                    "Simulate",
                    "Work the study out: how hard the material is worked, how far the part moves, and whether it holds",
                    Icon::Simulate)],
            },
            RibbonPanel {
                title: "Optimize",
                commands: &[
                    large(
                        "analysis.optimize",
                        "Optimize\nShape",
                        "Where material is needed to carry the study's loads, for a share of the mass kept: a shape to model over",
                        Icon::OptimizeShape),
                    small("analysis.keep_faces", "Keep Faces", "Faces the shape must keep (bolt holes, mating faces), as deep as asked", Icon::KeepFaces),
                    small("analysis.symmetry", "Symmetry", "Planes the shape is the same on both sides of", Icon::SymmetryPlane),
                    small("analysis.export_shape", "Export Shape", "Write the shape shown as STL or 3MF, to model over or print", Icon::Save),
                ],
            },
            RibbonPanel {
                title: "Display",
                commands: &[
                    large("analysis.marks", "Boundary\nConditions", "Show or hide the supports and loads drawn on the part", Icon::BoundaryConditions),
                    large(
                        "analysis.deform",
                        "Displacement\nDisplay",
                        "How the part's bending is drawn with its results: large enough to see, as far as it really moves, or not at all",
                        Icon::Displacement)
                    .more(&["analysis.deform.adjusted", "analysis.deform.actual", "analysis.deform.undeformed"]),
                ],
            },
            RibbonPanel {
                title: "Exit",
                commands: &[large("analysis.finish", "Finish\nAnalysis", "Leave the study and go back to the part", Icon::FinishSketch)],
            },
        ],
    },
    RibbonTab {
        name: "3D Sketch",
        panels: &[
            RibbonPanel {
                title: "Draw",
                commands: &[
                    large("sketch3d.line", "Line", "Lines through points: click a vertex, a work point or a point of the sketch, or type X, Y, Z", Icon::Line).key("L"),
                    small("sketch3d.spline", "Spline", "A smooth curve through the points you click or type; Enter makes it", Icon::Spline),
                    small("sketch3d.bend", "Bend", "Round a corner between two lines: click the corner, then say the radius", Icon::Bend),
                    small("sketch3d.point", "Point", "A point: on a vertex or a work point it follows the part, or type X, Y, Z", Icon::Point),
                    large("sketch3d.include", "Include\nGeometry", "Bring edges of the part in as reference geometry that follows the part", Icon::Project),
                ],
            },
            RibbonPanel {
                title: "Constrain",
                commands: &[
                    large("sketch3d.dimension", "Dimension", "Click a line for its length, a bend for its radius, or two points for how far apart they are", Icon::Dimension).key("D"),
                    glyph("sketch3d.coincident", "Coincident", "Two points", Icon::Coincident),
                    glyph("sketch3d.parallel", "Parallel", "Two lines", Icon::Parallel),
                    glyph("sketch3d.tangent", "Tangent", "Two curves that meet at an end", Icon::Tangent),
                    glyph("sketch3d.fix", "Fix", "A point that must not move", Icon::Fix),
                    glyph("sketch3d.perpendicular", "Perpendicular", "Two lines", Icon::Perpendicular),
                    glyph("sketch3d.equal", "Equal", "Two lines, or two bends", Icon::Equal),
                    glyph("sketch3d.along_x", "Parallel to X", "A line, held along the X axis", Icon::AlongX),
                    glyph("sketch3d.along_y", "Parallel to Y", "A line, held along the Y axis", Icon::AlongY),
                    glyph("sketch3d.along_z", "Parallel to Z", "A line, held along the Z axis", Icon::AlongZ),
                    small("sketch3d.coord_x", "X Coordinate", "Click a point, then say where it is along X", Icon::Dimension),
                    small("sketch3d.coord_y", "Y Coordinate", "Click a point, then say where it is along Y", Icon::Dimension),
                    small("sketch3d.coord_z", "Z Coordinate", "Click a point, then say where it is along Z", Icon::Dimension),
                ],
            },
            RibbonPanel {
                title: "Format",
                commands: &[
                    small("sketch3d.construction", "Construction", "Make the selected geometry construction (or normal again)", Icon::Construction),
                    small(
                        "sketch3d.break_link",
                        "Break Link",
                        "Make the selected reference geometry the sketch's own: it stays where it is and no longer follows the part",
                        Icon::Project),
                ],
            },
            RibbonPanel { title: "Exit", commands: &[large("sketch3d.finish", "Finish\n3D Sketch", "Leave the 3D sketch", Icon::FinishSketch)] },
        ],
    },
];

/// The ribbon of the assembly environment.
pub const ASM_RIBBON: &[RibbonTab] = &[
    RibbonTab {
        name: "Assemble",
        panels: &[
            RibbonPanel {
                title: "Component",
                commands: &[
                    large("asm.place", "Place", "Place a part file in the assembly", Icon::Place).key("P"),
                    large("asm.create", "Create", "A new part file, placed and edited in place", Icon::CreateComponent),
                    large(
                        "tools.library",
                        "Standard\nParts",
                        "Place a screw, nut, washer, bearing or section from the library, in the size you pick",
                        Icon::StandardPart,
                    ),
                ],
            },
            RibbonPanel {
                title: "Position",
                commands: &[
                    small(
                        "asm.free_rotate",
                        "Free Rotate",
                        "Dragging a component turns it instead of moving it; its relationships still hold",
                        Icon::Rotate,
                    ),
                    small("asm.ground", "Grounded", "Fix the selected components where they are, or free them", Icon::Ground),
                    small("asm.update", "Update", "Solve every relationship again", Icon::Update),
                ],
            },
            RibbonPanel {
                title: "Relationships",
                commands: &[
                    large("asm.joint", "Joint", "Rigid, rotational, slider, cylindrical, planar or ball joint between two origins", Icon::Joint)
                        .key("J"),
                    large("asm.constrain", "Constrain", "Mate (or flush), angle, insert, tangent or symmetry", Icon::Constrain).key("C"),
                ],
            },
            RibbonPanel {
                title: "Explode",
                commands: &[
                    large("asm.explode.toggle", "Exploded\nView", "Show the assembly exploded, or assembled again", Icon::Explode),
                    small("asm.explode.auto", "Auto Explode", "An exploded view made from the relationships", Icon::Explode),
                    small("asm.explode.tweak", "Tweak", "Move components along a direction in the exploded view", Icon::Move),
                    small("asm.explode.clear", "Clear Explode", "Remove every exploded-view step", Icon::Delete),
                ],
            },
            RibbonPanel {
                title: "Pattern",
                commands: &[
                    small("asm.pattern", "Pattern", "Repeat components in rows along one or two directions, or round an axis", Icon::PatternRect),
                    small(
                        "asm.mirror",
                        "Mirror",
                        "Reflect components in a plane or flat face: mirrored parts, or symmetric ones placed again",
                        Icon::Mirror,
                    ),
                    small("asm.copy", "Copy", "Independent copies of components, of the same parts or in new part files", Icon::Copy),
                ],
            },
        ],
    },
    RibbonTab {
        name: "Inspect",
        panels: &[
            RibbonPanel {
                title: "Interference",
                commands: &[large("asm.interference", "Analyze\nInterference", "Find the components whose solids overlap", Icon::Interference)],
            },
            RibbonPanel {
                title: "Measure",
                commands: &[large(
                    "inspect.mass",
                    "Properties",
                    "Mass, volume and centre of gravity, each component at its part's material",
                    Icon::MassProps,
                )],
            },
        ],
    },
    RibbonTab {
        name: "Tools",
        panels: &[RibbonPanel {
            title: "Options",
            commands: &[
                large("tools.options", "Application\nOptions", "Colour scheme and other settings", Icon::Settings),
                large("app.about", "About", "Version, licences and kernel information", Icon::Info),
            ],
        }],
    },
    RibbonTab {
        name: "Manage",
        panels: &[
            RibbonPanel {
                title: "Bill of Materials",
                commands: &[large(
                    "asm.bom",
                    "Bill of\nMaterials",
                    "The parts list with quantities, materials and masses; export it as CSV",
                    Icon::Bom,
                )],
            },
            RibbonPanel { title: "Update", commands: &[large("asm.update", "Update", "Solve every relationship again", Icon::Update)] },
        ],
    },
    RibbonTab {
        name: "View",
        panels: &[
            RibbonPanel {
                title: "Appearance",
                commands: &[
                    large("view.style", "Visual\nStyle", "Shaded with edges, shaded, or wireframe", Icon::VisualStyle).more(&[
                        "view.style.shaded_edges",
                        "view.style.shaded",
                        "view.style.wireframe",
                    ]),
                    small("view.orthographic", "Orthographic", "Parallel projection", Icon::Projection),
                    small("view.perspective", "Perspective", "Perspective projection", Icon::Projection),
                ],
            },
            RibbonPanel {
                title: "Visibility",
                commands: &[small("asm.dof", "Degrees of Freedom", "Show the motions each component has left", Icon::Dof)],
            },
            RibbonPanel {
                title: "Windows",
                commands: &[
                    small("view.browser", "Browser", "Show or hide the model browser", Icon::Browser),
                    small("view.cube", "Orientation Cube", "Show or hide the orientation cube", Icon::Cube),
                    small("view.navbar", "Navigation Bar", "Show or hide the navigation bar", Icon::Window),
                ],
            },
            RibbonPanel {
                title: "Navigate",
                commands: &[
                    small("view.home", "Home View", "Three-quarter view of the whole part (F6)", Icon::Home),
                    small("view.fit", "Zoom All", "Fit the model in the window", Icon::ZoomFit),
                    small("view.look_at", "Look At", "Look straight at the selected planar face or the active sketch", Icon::LookAt),
                    small("view.previous", "Previous View", "Go back to the last view (F5)", Icon::PreviousView),
                ],
            },
        ],
    },
];

/// The ribbon of the drawing environment.
pub const DRW_RIBBON: &[RibbonTab] = &[
    RibbonTab {
        name: "Place Views",
        panels: &[
            RibbonPanel {
                title: "Create",
                commands: &[
                    large("drw.base", "Base", "A view of a part or assembly file: choose the file, orientation and scale", Icon::BaseView),
                    large("drw.projected", "Projected", "Click a view, then click where each view projected from it goes", Icon::ProjectedView),
                    large(
                        "drw.section",
                        "Section",
                        "Click a view, click the two ends of the section line, then where the section goes",
                        Icon::SectionView,
                    ),
                    large(
                        "drw.detail",
                        "Detail",
                        "Click a view, click the centre of the detail, then its radius, then where it goes",
                        Icon::DetailView,
                    ),
                    small(
                        "drw.auxiliary",
                        "Auxiliary",
                        "Click a view, a straight edge of it, then where the view square to that edge goes",
                        Icon::ProjectedView,
                    ),
                    small(
                        "drw.overlay",
                        "Overlay",
                        "Click a view of an assembly and type a position: it is drawn over the view in phantom lines",
                        Icon::Explode,
                    ),
                ],
            },
            RibbonPanel {
                title: "Modify",
                commands: &[
                    small("drw.break", "Break", "Click a view, then where two break lines go: the band between them is taken out", Icon::Split),
                    small(
                        "drw.break_out",
                        "Break Out",
                        "Click a view, a rectangle or circle on it, and give a depth: inside it the part is cut away",
                        Icon::Trim,
                    ),
                    small("drw.crop", "Crop", "Click a view, then a rectangle or circle on it: only what lies inside is drawn", Icon::Rectangle),
                ],
            },
            RibbonPanel { title: "Sheets", commands: &[large("drw.sheet.new", "New\nSheet", "Add a sheet to the drawing", Icon::NewSheet)] },
        ],
    },
    RibbonTab {
        name: "Annotate",
        panels: &[
            RibbonPanel {
                title: "Dimension",
                commands: &[
                    large("drw.dimension", "Dimension", "Click an edge or two, then where the dimension goes", Icon::Dimension).key("D"),
                    small("drw.dimension.auto", "Auto Dimension", "Click a view: the dimensions it needs first, to review and add", Icon::Dimension),
                    small("drw.baseline", "Baseline", "Click a datum, then what to measure from it, then where the dimensions go", Icon::Baseline),
                    small("drw.ordinate", "Ordinate", "Click an origin, then what to measure from it, then where the values go", Icon::Ordinate),
                ],
            },
            RibbonPanel { title: "Text", commands: &[large("drw.text", "Text", "Click where the text goes, then type it", Icon::Text).key("T")] },
            RibbonPanel {
                title: "Symbols",
                commands: &[
                    large("drw.center_mark", "Centre\nMark", "Click circles and arcs: a centre mark on each", Icon::CenterMark),
                    small(
                        "drw.centerline",
                        "Centreline",
                        "Click two places: circles (their centres), lines (their middles) or points",
                        Icon::Centerline,
                    ),
                    small(
                        "drw.centerline.bisector",
                        "Centreline Bisector",
                        "Click two lines: the centreline midway between them",
                        Icon::CenterlineBisector,
                    ),
                ],
            },
            RibbonPanel {
                title: "Table",
                commands: &[
                    large("drw.parts_list", "Parts\nList", "Click an assembly view, then where the list goes", Icon::Bom),
                    large("drw.balloon", "Balloon", "Click a part in an assembly view, then where the balloon goes", Icon::Balloon)
                        .key("B")
                        .more(&["drw.balloon", "drw.balloon.auto"]),
                    large("drw.hole_table", "Hole", "Click a part view, then where the hole table goes", Icon::HoleTable),
                ],
            },
        ],
    },
    RibbonTab {
        name: "Tools",
        panels: &[RibbonPanel {
            title: "Options",
            commands: &[
                large("tools.options", "Application\nOptions", "Colour scheme and other settings", Icon::Settings),
                large("app.about", "About", "Version, licences and kernel information", Icon::Info),
            ],
        }],
    },
    RibbonTab {
        name: "Manage",
        panels: &[
            RibbonPanel {
                title: "Update",
                commands: &[large("drw.update", "Update", "Read the model files again; the views and dimensions follow", Icon::Update)],
            },
            RibbonPanel {
                title: "Title Block",
                commands: &[
                    large("drw.template.apply", "Apply\nTemplate", "Use a title block template file (.json) on every sheet", Icon::Open),
                    small("drw.template.save", "Save Template", "Save this sheet's title block as a template file (.json)", Icon::Save),
                ],
            },
        ],
    },
    RibbonTab {
        name: "View",
        panels: &[
            RibbonPanel { title: "Windows", commands: &[small("view.browser", "Browser", "Show or hide the model browser", Icon::Browser)] },
            RibbonPanel { title: "Navigate", commands: &[small("view.fit", "Zoom All", "Fit the sheet in the window", Icon::ZoomFit)] },
        ],
    },
];

/// The panel shown at the end of the ribbon while a part is edited in place.
pub const RETURN: UiCommand = large("asm.return", "Return", "Finish editing the part and go back to the assembly", Icon::Return);

/// The same, while a model is edited from a drawing.
pub const DRW_RETURN: UiCommand = large("drw.return", "Return", "Finish editing the model and go back to the drawing", Icon::Return);

/// Commands reachable from drop-down arrows, menus and keys but not shown on the ribbon itself.
pub const EXTRA: &[UiCommand] = &[
    small("analysis.deform.adjusted", "Adjusted", "The part's bending drawn large enough to see", Icon::Displacement),
    small("analysis.deform.actual", "Actual", "The part drawn bent as far as it really moves", Icon::Displacement),
    small("analysis.deform.undeformed", "Undeformed", "Results on the part as it is, not bent", Icon::Displacement),
    small("model.cylinder", "Cylinder", "A cylinder in a few clicks: a plane or flat face, its centre, its size, its height", Icon::PrimCylinder),
    small("model.sphere", "Sphere", "A sphere in a few clicks: a plane or flat face, its centre, its size", Icon::PrimSphere),
    small(
        "model.torus",
        "Torus",
        "A ring in a few clicks: a plane or flat face, its centre, the middle of the ring, how thick it is",
        Icon::PrimTorus,
    ),
    small("sketch.spline", "Spline", "Click control points; Enter finishes", Icon::Spline),
    small(
        "sketch.project_flat",
        "Project Flat Pattern",
        "In a sketch on a sheet metal face: show a face beyond a bend where it lies with the sheet laid flat, to draw a cut across the bend on",
        Icon::Project,
    ),
    small("sketch.polygon", "Polygon", "Click the centre, then a corner (6 sides)", Icon::Polygon),
    small(
        "sketch.break_link",
        "Break Link",
        "Make the selected projected geometry the sketch's own: it stays where it is and no longer follows the part",
        Icon::Project,
    ),
    small(
        "sketch.arc_tangent",
        "Arc Tangent",
        "An arc that carries on from the end of a line or an arc: click that end, then where the arc ends",
        Icon::Arc,
    ),
    small("sketch.arc_center", "Arc Center Point", "Click the centre, the start, then the end of the arc", Icon::Arc),
    small("model.face_fillet", "Face Fillet", "Round the edges between two sets of faces", Icon::Fillet),
    small("sketch.ellipse", "Ellipse", "Click the centre, the end of one axis, then a point on the ellipse", Icon::Ellipse),
    small("sketch.rectangle3", "Rectangle Three Point", "A rectangle at any angle: click two corners of one side, then how deep", Icon::Rectangle),
    small("sketch.rectangle_center", "Rectangle Two Point Center", "Click the centre, then a corner", Icon::Rectangle),
    small("sketch.slot", "Slot Center to Center", "Click the centre of each round end, then how wide", Icon::Slot),
    small("sketch.slot_overall", "Slot Overall", "Click each end of the slot, then how wide", Icon::Slot),
    small("sketch.slot_center", "Slot Center Point", "Click the middle of the slot, the centre of one end, then how wide", Icon::Slot),
    small("sketch.slot_arc_three", "Three Point Arc Slot", "Click the start of the arc, its end, a point on it, then how wide", Icon::SlotArc),
    small("sketch.slot_arc_center", "Center Point Arc Slot", "Click the centre of the arc, its start, its end, then how wide", Icon::SlotArc),
    small(
        "sketch.new_3d",
        "Start 3D Sketch",
        "A sketch on no plane: lines, bends and splines through vertices, work points or typed coordinates, to sweep along",
        Icon::Sketch3d,
    ),
    small("view.style.shaded_edges", "Shaded with Edges", "Shaded faces with their edges", Icon::VisualStyle),
    small("view.style.shaded", "Shaded", "Shaded faces only", Icon::VisualStyle),
    small("view.style.wireframe", "Wireframe", "Edges only", Icon::VisualStyle),
    small("file.new_sheet_metal", "New Sheet Metal Part", "A new part that is cut from sheet and folded", Icon::SheetFace),
    small("file.new_assembly", "New Assembly", "A new assembly of part files", Icon::Assembly),
    small("asm.edit", "Edit", "Edit the selected component's part in place", Icon::Part),
    RETURN,
    small("file.new_drawing", "New Drawing", "A new drawing of parts and assemblies", Icon::Drawing),
    small(
        "file.new_drawing_template",
        "New Drawing from Template",
        "A new drawing starting with a template's sheets, standard, properties and notes",
        Icon::Drawing,
    ),
    small(
        "drw.save_template",
        "Save as Template",
        "This drawing's sheets, standard, properties and notes, without views, for new drawings to start from",
        Icon::Save,
    ),
    small("drw.balloon.auto", "Auto Balloon", "Click an assembly view: a balloon for each part", Icon::Balloon),
    small("drw.edit_view", "Edit View", "Scale, hidden lines, centrelines and label of the selected view", Icon::BaseView),
    small("drw.edit_model", "Open Model", "Edit the selected view's part or assembly; Return comes back to the drawing", Icon::Part),
    small("drw.delete", "Delete", "Delete the selected view (with the views made from it) or annotation", Icon::Delete),
    small("export.pdf", "Export PDF", "Every sheet as a page of a PDF", Icon::Save),
    small("export.svg", "Export SVG", "The sheet as SVG", Icon::Save),
    small("export.dxf", "Export DXF", "The sheet as DXF, a layer per line type", Icon::Save),
    DRW_RETURN,
];

/// Navigation bar (right edge of the viewport). Pan/Zoom/Orbit make the left button do that.
pub const NAV_BAR: &[UiCommand] = &[
    small("view.pan", "Pan", "Left drag pans (also: middle drag, F2)", Icon::Pan),
    small("view.zoom", "Zoom", "Left drag zooms (also: the wheel, F3)", Icon::Zoom).more(&["view.zoom", "view.fit"]),
    small("view.orbit", "Orbit", "Left drag orbits (also: Shift + middle drag, F4)", Icon::Orbit),
    small("view.look_at", "Look At", "Look straight at the selected planar face or the active sketch", Icon::LookAt),
];

/// Quick-access toolbar (title bar), in groups.
pub const QUICK_ACCESS: &[&[UiCommand]] = &[
    &[
        small("file.new", "New", "New part (Ctrl+N)", Icon::New),
        small("file.open", "Open", "Open a project (Ctrl+O)", Icon::Open),
        small("file.save", "Save", "Save the project (Ctrl+S)", Icon::Save),
    ],
    &[small("edit.undo", "Undo", "Undo (Ctrl+Z)", Icon::Undo), small("edit.redo", "Redo", "Redo (Ctrl+Y)", Icon::Redo)],
    &[
        small("view.home", "Home View", "Three-quarter view of the whole part (F6)", Icon::Home),
        small("model.rebuild", "Rebuild All", "Regenerate every feature from scratch", Icon::Update),
    ],
    &[
        small("tools.parameters", "Parameters", "Named parameters and expressions", Icon::Parameters),
        small("inspect.measure", "Measure", "Distances, angles, lengths and areas", Icon::Measure),
    ],
];

/// File menu entries (label, command id).
pub const FILE_MENU: &[(&str, &str)] = &[
    ("New Part", "file.new"),
    ("New Sheet Metal Part", "file.new_sheet_metal"),
    ("New Assembly", "file.new_assembly"),
    ("New Drawing", "file.new_drawing"),
    ("New Drawing from Template...", "file.new_drawing_template"),
    ("Open...", "file.open"),
    ("Save", "file.save"),
    ("Save As...", "file.save_as"),
    ("Export STEP...", "export.step"),
    ("Export STL...", "export.stl"),
    ("Export 3MF...", "export.3mf"),
    ("Export Flat Pattern DXF...", "sheet.export_dxf"),
];

/// File menu entries in a drawing.
pub const DRW_FILE_MENU: &[(&str, &str)] = &[
    ("New Part", "file.new"),
    ("New Assembly", "file.new_assembly"),
    ("New Drawing", "file.new_drawing"),
    ("New Drawing from Template...", "file.new_drawing_template"),
    ("Open...", "file.open"),
    ("Save", "file.save"),
    ("Save As...", "file.save_as"),
    ("Save as Template...", "drw.save_template"),
    ("Export PDF...", "export.pdf"),
    ("Export SVG...", "export.svg"),
    ("Export DXF...", "export.dxf"),
];

/// Every command reachable from the ribbon, menus and toolbars.
pub fn all() -> impl Iterator<Item = &'static UiCommand> {
    RIBBON
        .iter()
        .chain(ASM_RIBBON)
        .chain(DRW_RIBBON)
        .flat_map(|t| t.panels.iter())
        .flat_map(|p| p.commands.iter())
        .chain(EXTRA)
        .chain(NAV_BAR)
        .chain(QUICK_ACCESS.iter().flat_map(|g| g.iter()))
}

/// Looks up a command by id.
pub fn find(id: &str) -> Option<&'static UiCommand> {
    all().find(|c| c.id == id)
}

/// Where a command lives, e.g. "3D Model > Create" (for search results).
pub fn location(id: &str) -> Option<String> {
    RIBBON
        .iter()
        .chain(ASM_RIBBON)
        .chain(DRW_RIBBON)
        .find_map(|t| t.panels.iter().find(|p| p.commands.iter().any(|c| c.id == id)).map(|p| format!("{} > {}", t.name, p.title)))
}

/// Which ribbon is showing.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Env {
    Part {
        sketching: bool,
    },
    /// In a 3D sketch.
    Sketch3d,
    Assembly,
    Drawing,
}

/// Whether `c` works where `env` says the window is: among commands of one name, the one that does
/// is the one meant. (Mirror is a part's, a sketch's and an assembly's.)
fn is_here(c: &UiCommand, env: Env) -> bool {
    let on = |ribbon: &[RibbonTab]| {
        ribbon.iter().flat_map(|t| t.panels.iter()).flat_map(|p| p.commands.iter()).any(|x| x.id == c.id || x.more.contains(&c.id))
    };
    match env {
        Env::Drawing => on(DRW_RIBBON) || c.id.starts_with("drw.") || DRW_FILE_MENU.iter().any(|(_, id)| *id == c.id),
        Env::Assembly => on(ASM_RIBBON) || c.id.starts_with("asm."),
        Env::Sketch3d => c.id.starts_with("sketch3d."),
        Env::Part { sketching: true } => c.id.starts_with("sketch."),
        Env::Part { sketching: false } => {
            !["sketch3d.", "asm.", "drw."].iter().any(|p| c.id.starts_with(p)) && !["export.pdf", "export.svg", "export.dxf"].contains(&c.id)
        }
    }
}

/// Commands matching a search, best first: label prefix, then word prefix, then substring; and
/// within each, what works in `env` before what does not.
pub fn search(query: &str, env: Env) -> Vec<&'static UiCommand> {
    let q = query.trim().to_lowercase();
    if q.is_empty() {
        return Vec::new();
    }
    let mut seen = std::collections::BTreeSet::new();
    let mut scored: Vec<(u8, &'static UiCommand)> = all()
        .filter(|c| seen.insert(c.id))
        .filter_map(|c| {
            let name = c.name().to_lowercase();
            // (What the button may read instead, as the part changes: see `ribbon_label`.)
            let also = if c.id == "sheet.flat_view" { "go to flat pattern" } else { "" };
            let rank = if name.starts_with(&q) || also.starts_with(&q) {
                0
            } else if name.split(' ').any(|w| w.starts_with(&q)) {
                1
            } else if name.contains(&q) || also.contains(&q) || c.id.contains(&q) || c.tip.to_lowercase().contains(&q) {
                2
            } else {
                return None;
            };
            Some((rank, c))
        })
        .collect();
    scored.sort_by_key(|(r, c)| (*r, !is_here(c, env), c.name()));
    scored.into_iter().map(|(_, c)| c).collect()
}

/// The command for a single-key shortcut. While sketching, sketch keys come first and the rest
/// still work (E extrudes, finishing the sketch). In an assembly, the Assemble tab's keys; in a
/// drawing, the Place Views and Annotate tabs'.
pub fn for_key(key: &str, env: Env) -> Option<&'static UiCommand> {
    let (ribbon, tabs): (&[RibbonTab], &[usize]) = match env {
        Env::Part { sketching: true } => (RIBBON, &[SKETCH_TAB, MODEL_TAB]),
        Env::Part { sketching: false } => (RIBBON, &[MODEL_TAB]),
        Env::Sketch3d => (RIBBON, &[SKETCH3D_TAB]),
        Env::Assembly => (ASM_RIBBON, &[0]),
        Env::Drawing => (DRW_RIBBON, &[0, 1]),
    };
    tabs.iter().filter_map(|t| ribbon.get(*t)).flat_map(|t| t.panels.iter()).flat_map(|p| p.commands.iter()).find(|c| c.key == Some(key))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ids_are_well_formed_and_consistent() {
        for c in all() {
            assert!(
                c.id.contains('.') && c.id.chars().all(|ch| ch.is_ascii_lowercase() || ch == '.' || ch == '_' || ch.is_ascii_digit()),
                "{}",
                c.id
            );
            assert!(!c.label.is_empty() && !c.tip.is_empty(), "{}", c.id);
            let first = find(c.id).unwrap();
            assert_eq!(first.name(), c.name(), "{}", c.id);
            for m in c.more {
                assert!(find(m).is_some(), "{}: drop-down entry {m} is not a command", c.id);
            }
        }
    }

    #[test]
    fn ribbon_has_the_planned_tabs() {
        let names: Vec<_> = RIBBON.iter().map(|t| t.name).collect();
        assert_eq!(
            names,
            ["3D Model", "Sketch", "Inspect", "Tools", "Manage", "View", "Environments", "Sheet Metal", "Flat Pattern", "Analysis", "3D Sketch"]
        );
        assert_eq!((RIBBON[SHEET_TAB].name, RIBBON[FLAT_TAB].name, RIBBON[ANALYSIS_TAB].name), ("Sheet Metal", "Flat Pattern", "Analysis"));
        assert_eq!(RIBBON[SKETCH3D_TAB].name, "3D Sketch");
        assert_eq!(RIBBON[MODEL_TAB].name, "3D Model");
        assert_eq!(RIBBON[SKETCH_TAB].name, "Sketch");
        assert!(RIBBON.iter().all(|t| !t.panels.is_empty() && t.panels.iter().all(|p| !p.commands.is_empty())));
        let names: Vec<_> = DRW_RIBBON.iter().map(|t| t.name).collect();
        assert_eq!(names, ["Place Views", "Annotate", "Tools", "Manage", "View"]);
    }

    /// A name that several commands have is the one that works here, with the others below it.
    #[test]
    fn a_name_searched_for_is_the_command_of_where_you_are() {
        let sketch = Env::Part { sketching: true };
        let part = Env::Part { sketching: false };
        for (env, name, id) in [
            (Env::Assembly, "Mirror", "asm.mirror"),
            (Env::Assembly, "Copy", "asm.copy"),
            (Env::Assembly, "Pattern", "asm.pattern"),
            (Env::Assembly, "Update", "asm.update"),
            (Env::Drawing, "Text", "drw.text"),
            (Env::Drawing, "Dimension", "drw.dimension"),
            (Env::Drawing, "Hole", "drw.hole_table"),
            (Env::Drawing, "Update", "drw.update"),
            (Env::Drawing, "Export DXF", "export.dxf"),
            (Env::Drawing, "Export SVG", "export.svg"),
            (sketch, "Spline", "sketch.spline"),
            (sketch, "Break Link", "sketch.break_link"),
            (sketch, "Point", "sketch.point"),
            (sketch, "Mirror", "sketch.mirror"),
            (Env::Sketch3d, "Line", "sketch3d.line"),
            (Env::Sketch3d, "Dimension", "sketch3d.dimension"),
            (Env::Sketch3d, "Point", "sketch3d.point"),
            (Env::Sketch3d, "Bend", "sketch3d.bend"),
            (Env::Sketch3d, "Spline", "sketch3d.spline"),
            (part, "Mirror", "model.mirror"),
            (part, "Bend", "sheet.bend"),
            (part, "Line", "sketch.line"),
            (part, "Export DXF", "sheet.export_dxf"),
        ] {
            let found = search(name, env);
            assert_eq!(found.first().map(|c| c.id), Some(id), "{name} in {env:?}");
        }
        // The others stay below it.
        assert!(search("Mirror", Env::Assembly).iter().any(|c| c.id == "model.mirror"));
    }

    #[test]
    fn search_and_shortcuts() {
        let part = Env::Part { sketching: false };
        assert_eq!(search("extr", part).first().map(|c| c.id), Some("model.extrude"));
        assert_eq!(search("start", part).first().map(|c| c.id), Some("sketch.new"));
        assert!(search("sketch", part).iter().take(2).any(|c| c.id == "sketch.finish"), "a word prefix ranks above a substring");
        assert!(search("zzzz", part).is_empty());
        assert_eq!(for_key("E", part).map(|c| c.id), Some("model.extrude"));
        assert_eq!(for_key("L", Env::Part { sketching: true }).map(|c| c.id), Some("sketch.line"));
        assert!(for_key("L", part).is_none(), "sketch keys only while sketching");
        assert_eq!(for_key("C", Env::Assembly).map(|c| c.id), Some("asm.constrain"));
        assert!(for_key("E", Env::Assembly).is_none(), "no part keys in an assembly");
        assert_eq!(for_key("D", Env::Drawing).map(|c| c.id), Some("drw.dimension"));
        assert_eq!(for_key("B", Env::Drawing).map(|c| c.id), Some("drw.balloon"));
        assert!(for_key("E", Env::Drawing).is_none(), "no part keys in a drawing");
        assert_eq!(location("asm.joint").as_deref(), Some("Assemble > Relationships"));
        assert_eq!(location("model.extrude").as_deref(), Some("3D Model > Create"));
        assert_eq!(location("drw.section").as_deref(), Some("Place Views > Create"));
        assert_eq!(location("drw.hole_table").as_deref(), Some("Annotate > Table"));
    }
}
