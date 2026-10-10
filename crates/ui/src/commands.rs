//! The ribbon's command table. Every button maps to a named command id (`area.verb`). Buttons that
//! edit the model end in the shared command registry (`tenon_model::cmd`, `tenon_io::cmd`); the
//! rest are UI actions (tools, views). `milestone` says when a command starts working: 0 means it
//! works today.
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
    pub milestone: u8,
    /// Single-key shortcut, shown in the tooltip.
    pub key: Option<&'static str>,
    /// Commands under the button's drop-down arrow.
    pub more: &'static [&'static str],
}

/// The `milestone` of a command no milestone of the current plan brings (docs/plan.md section 10).
pub const LATER: u8 = 99;

impl UiCommand {
    pub fn available(&self) -> bool {
        self.milestone == 0
    }
    /// When it arrives, for tooltips: "milestone M6", or "a later milestone".
    pub fn arrives(&self) -> String {
        if self.milestone == LATER { "a later milestone (not in the current plan)".into() } else { format!("milestone M{}", self.milestone) }
    }
    /// The message for using it before it works.
    pub fn not_yet(&self) -> String {
        format!("{} is not available yet: it arrives in {}.", self.name(), self.arrives())
    }
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

const fn cmd(id: &'static str, label: &'static str, tip: &'static str, icon: Icon, size: Size, milestone: u8) -> UiCommand {
    UiCommand { id, label, tip, icon, size, milestone, key: None, more: &[] }
}
const fn large(id: &'static str, label: &'static str, tip: &'static str, icon: Icon, milestone: u8) -> UiCommand {
    cmd(id, label, tip, icon, Size::Large, milestone)
}
const fn small(id: &'static str, label: &'static str, tip: &'static str, icon: Icon, milestone: u8) -> UiCommand {
    cmd(id, label, tip, icon, Size::Small, milestone)
}
const fn glyph(id: &'static str, label: &'static str, tip: &'static str, icon: Icon, milestone: u8) -> UiCommand {
    cmd(id, label, tip, icon, Size::Icon, milestone)
}

pub const MODEL_TAB: usize = 0;
pub const SKETCH_TAB: usize = 1;

pub const RIBBON: &[RibbonTab] = &[
    RibbonTab {
        name: "3D Model",
        panels: &[
            RibbonPanel {
                title: "Sketch",
                commands: &[large("sketch.new", "Start\n2D Sketch", "Pick an origin plane or a planar face to sketch on", Icon::NewSketch, 0)
                    .key("S")
                    .more(&["sketch.new_3d"])],
            },
            RibbonPanel {
                title: "Create",
                commands: &[
                    large("model.extrude", "Extrude", "Add or cut material by sweeping a profile straight", Icon::Extrude, 0).key("E"),
                    large("model.revolve", "Revolve", "Add or cut material by rotating a profile about an axis", Icon::Revolve, 0).key("R"),
                    small("model.sweep", "Sweep", "Sweep a profile along a path", Icon::Sweep, 0),
                    small("model.loft", "Loft", "Blend between two or more profiles", Icon::Loft, 0),
                    small("model.coil", "Coil", "Helical sweep", Icon::Coil, 0),
                    small("model.rib", "Rib", "A thin wall from an open profile", Icon::Rib, 0),
                    small("model.emboss", "Emboss", "Raise or recess a profile on a face", Icon::Text, LATER),
                    small("model.derive", "Derive", "Bring in another part", Icon::Copy, LATER),
                ],
            },
            RibbonPanel {
                title: "Modify",
                commands: &[
                    large("model.hole", "Hole", "Simple, counterbore or countersink holes", Icon::Hole, 0).key("H"),
                    large("model.fillet", "Fillet", "Round edges", Icon::Fillet, 0).key("F"),
                    small("model.chamfer", "Chamfer", "Bevel edges", Icon::Chamfer, 0),
                    small("model.shell", "Shell", "Hollow a solid, removing faces", Icon::Shell, 0),
                    small("model.draft", "Draft", "Taper faces for moulding", Icon::Draft, 0),
                    small("model.thread", "Thread", "A screw thread on a round shaft or hole: cosmetic, or cut into the part", Icon::Thread, 0),
                    small("model.combine", "Combine", "Join, cut or intersect bodies", Icon::Combine, 0),
                    small("model.split", "Split", "Cut solids in two along a plane, or keep one side", Icon::Split, 0),
                ],
            },
            RibbonPanel {
                title: "Work Features",
                commands: &[
                    large("work.plane", "Plane", "Construction plane: offset, angled or mid-plane", Icon::Plane, 0),
                    small("work.axis", "Axis", "Construction axis: on an edge or cylinder, or where two planes meet", Icon::Axis, 0),
                    small("work.point", "Point", "Construction point: a circle centre, or where an axis meets a plane", Icon::Point, 0),
                    small("work.ucs", "UCS", "User coordinate system", Icon::Ucs, LATER),
                ],
            },
            RibbonPanel {
                title: "Pattern",
                commands: &[
                    small("model.pattern.rect", "Rectangular", "Repeat features in rows and columns", Icon::PatternRect, 0),
                    small("model.pattern.circular", "Circular", "Repeat features around an axis", Icon::PatternCircular, 0),
                    small("model.mirror", "Mirror", "Mirror features across a plane", Icon::Mirror, 0),
                ],
            },
        ],
    },
    RibbonTab {
        name: "Sketch",
        panels: &[
            RibbonPanel {
                title: "Create",
                commands: &[
                    large("sketch.line", "Line", "Click points; each line starts where the last ended. Esc ends the chain.", Icon::Line, 0)
                        .key("L")
                        .more(&["sketch.line", "sketch.spline"]),
                    small("sketch.circle", "Circle", "Click the centre, then a point on the circle", Icon::Circle, 0).key("C"),
                    small("sketch.arc", "Arc", "Click the start, the end, then a point on the arc", Icon::Arc, 0).key("A"),
                    small("sketch.rectangle", "Rectangle", "Click two opposite corners", Icon::Rectangle, 0)
                        .more(&["sketch.rectangle", "sketch.polygon"]),
                    small("sketch.fillet", "Fillet", "Click a corner where two lines meet", Icon::Fillet, 0),
                    small("sketch.text", "Text", "Text in a sketch", Icon::Text, LATER),
                    small("sketch.point", "Point", "Click to place a point (e.g. a hole centre)", Icon::Point, 0),
                ],
            },
            RibbonPanel {
                title: "Modify",
                commands: &[
                    small("sketch.move", "Move", "Move sketch geometry", Icon::Move, LATER),
                    small("sketch.copy", "Copy", "Copy sketch geometry", Icon::Copy, LATER),
                    small("sketch.rotate", "Rotate", "Rotate sketch geometry", Icon::Rotate, LATER),
                    small("sketch.trim", "Trim", "Click the piece of a curve to remove", Icon::Trim, 0).key("X"),
                    small("sketch.extend", "Extend", "Extend a curve to the next one", Icon::Extend, LATER),
                    small("sketch.split", "Split", "Split a curve where it meets another", Icon::Split, LATER),
                    small("sketch.scale", "Scale", "Scale sketch geometry", Icon::Scale, LATER),
                    small("sketch.stretch", "Stretch", "Stretch sketch geometry", Icon::Stretch, LATER),
                    small("sketch.offset", "Offset", "Select curves, then choose a distance", Icon::Offset, 0).key("O"),
                ],
            },
            RibbonPanel {
                title: "Pattern",
                commands: &[
                    small("sketch.pattern.rect", "Rectangular", "Repeat sketch geometry in rows and columns", Icon::PatternRect, LATER),
                    small("sketch.pattern.circular", "Circular", "Repeat sketch geometry around a point", Icon::PatternCircular, LATER),
                    small("sketch.mirror", "Mirror", "Select geometry, then click the mirror line", Icon::Mirror, 0),
                ],
            },
            RibbonPanel {
                title: "Constrain",
                commands: &[
                    large("sketch.dimension", "Dimension", "Click a line, circle, arc, two points or two lines", Icon::Dimension, 0).key("D"),
                    // Column by column, so the grid reads in rows: coincident, collinear,
                    // concentric, fix / parallel, perpendicular, horizontal, vertical /
                    // tangent, smooth, symmetric, equal.
                    glyph("sketch.coincident", "Coincident", "Two points, or a point and a curve", Icon::Coincident, 0),
                    glyph("sketch.parallel", "Parallel", "Two lines", Icon::Parallel, 0),
                    glyph("sketch.tangent", "Tangent", "A line and a circle/arc, or two circles/arcs", Icon::Tangent, 0),
                    glyph("sketch.collinear", "Collinear", "Two lines on one line", Icon::Collinear, 0),
                    glyph("sketch.perpendicular", "Perpendicular", "Two lines", Icon::Perpendicular, 0),
                    glyph("sketch.smooth", "Smooth", "Curvature-continuous join", Icon::Spline, LATER),
                    glyph("sketch.concentric", "Concentric", "Two circles/arcs", Icon::Concentric, 0),
                    glyph("sketch.horizontal", "Horizontal", "A line", Icon::Horizontal, 0),
                    glyph("sketch.symmetric", "Symmetric", "Two points, then the line of symmetry", Icon::Symmetric, 0),
                    glyph("sketch.fix", "Fix", "A point that must not move", Icon::Fix, 0),
                    glyph("sketch.vertical", "Vertical", "A line", Icon::Vertical, 0),
                    glyph("sketch.equal", "Equal", "Two lines or two circles/arcs", Icon::Equal, 0),
                ],
            },
            RibbonPanel {
                title: "Format",
                commands: &[small(
                    "sketch.construction",
                    "Construction",
                    "Make the selected geometry construction (or normal again)",
                    Icon::Construction,
                    0,
                )],
            },
            RibbonPanel { title: "Exit", commands: &[large("sketch.finish", "Finish\nSketch", "Leave the sketch", Icon::FinishSketch, 0)] },
        ],
    },
    RibbonTab {
        name: "Inspect",
        panels: &[RibbonPanel {
            title: "Measure",
            commands: &[
                large("inspect.measure", "Measure", "Distances, angles, lengths and areas", Icon::Measure, 0),
                large("inspect.mass", "Mass\nProperties", "Volume, area, centre of mass, inertia", Icon::MassProps, 0),
            ],
        }],
    },
    RibbonTab {
        name: "Tools",
        panels: &[RibbonPanel {
            title: "Options",
            commands: &[
                large("tools.options", "Application\nOptions", "Colour scheme and other settings", Icon::Settings, 0),
                large("app.about", "About", "Version, licences and kernel information", Icon::Info, 0),
            ],
        }],
    },
    RibbonTab {
        name: "Manage",
        panels: &[
            RibbonPanel {
                title: "Parameters",
                commands: &[large("tools.parameters", "Parameters", "Named parameters and expressions", Icon::Parameters, 0)],
            },
            RibbonPanel {
                title: "Update",
                commands: &[large("model.rebuild", "Rebuild\nAll", "Regenerate every feature from scratch", Icon::Update, 0)],
            },
        ],
    },
    RibbonTab {
        name: "View",
        panels: &[
            RibbonPanel {
                title: "Appearance",
                commands: &[
                    large("view.style", "Visual\nStyle", "Shaded with edges, shaded, or wireframe", Icon::VisualStyle, 0).more(&[
                        "view.style.shaded_edges",
                        "view.style.shaded",
                        "view.style.wireframe",
                    ]),
                    small("view.orthographic", "Orthographic", "Parallel projection", Icon::Projection, 0),
                    small("view.perspective", "Perspective", "Perspective projection", Icon::Projection, 0),
                ],
            },
            RibbonPanel {
                title: "Windows",
                commands: &[
                    small("view.browser", "Browser", "Show or hide the model browser", Icon::Browser, 0),
                    small("view.cube", "Orientation Cube", "Show or hide the orientation cube", Icon::Cube, 0),
                    small("view.navbar", "Navigation Bar", "Show or hide the navigation bar", Icon::Window, 0),
                ],
            },
            RibbonPanel {
                title: "Navigate",
                commands: &[
                    small("view.home", "Home View", "Three-quarter view of the whole part (F6)", Icon::Home, 0),
                    small("view.fit", "Zoom All", "Fit the model in the window", Icon::ZoomFit, 0),
                    small("view.look_at", "Look At", "Look straight at the selected planar face or the active sketch", Icon::LookAt, 0),
                    small("view.previous", "Previous View", "Go back to the last view (F5)", Icon::PreviousView, 0),
                ],
            },
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
                    large("asm.place", "Place", "Place a part file in the assembly", Icon::Place, 0).key("P"),
                    large("asm.create", "Create", "A new part file, placed and edited in place", Icon::CreateComponent, 0),
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
                        0,
                    ),
                    small("asm.ground", "Grounded", "Fix the selected components where they are, or free them", Icon::Ground, 0),
                    small("asm.update", "Update", "Solve every relationship again", Icon::Update, 0),
                ],
            },
            RibbonPanel {
                title: "Relationships",
                commands: &[
                    large("asm.joint", "Joint", "Rigid, rotational, slider, cylindrical, planar or ball joint between two origins", Icon::Joint, 0)
                        .key("J"),
                    large("asm.constrain", "Constrain", "Mate, flush, angle or insert", Icon::Constrain, 0).key("C"),
                ],
            },
            RibbonPanel {
                title: "Explode",
                commands: &[
                    large("asm.explode.toggle", "Exploded\nView", "Show the assembly exploded, or assembled again", Icon::Explode, 0),
                    small("asm.explode.auto", "Auto Explode", "An exploded view made from the relationships", Icon::Explode, 0),
                    small("asm.explode.tweak", "Tweak", "Move components along a direction in the exploded view", Icon::Move, 0),
                    small("asm.explode.clear", "Clear Explode", "Remove every exploded-view step", Icon::Delete, 0),
                ],
            },
            RibbonPanel {
                title: "Pattern",
                commands: &[
                    small("asm.pattern", "Pattern", "Repeat components", Icon::PatternRect, LATER),
                    small("asm.mirror", "Mirror", "Mirror components", Icon::Mirror, LATER),
                    small("asm.copy", "Copy", "Copy components", Icon::Copy, LATER),
                ],
            },
        ],
    },
    RibbonTab {
        name: "Inspect",
        panels: &[
            RibbonPanel {
                title: "Interference",
                commands: &[large("asm.interference", "Analyze\nInterference", "Find the components whose solids overlap", Icon::Interference, 0)],
            },
            RibbonPanel {
                title: "Measure",
                commands: &[large("inspect.mass", "Mass\nProperties", "Volume, area, centre of mass, inertia", Icon::MassProps, 0)],
            },
        ],
    },
    RibbonTab {
        name: "Tools",
        panels: &[RibbonPanel {
            title: "Options",
            commands: &[
                large("tools.options", "Application\nOptions", "Colour scheme and other settings", Icon::Settings, 0),
                large("app.about", "About", "Version, licences and kernel information", Icon::Info, 0),
            ],
        }],
    },
    RibbonTab {
        name: "Manage",
        panels: &[
            RibbonPanel {
                title: "Bill of Materials",
                commands: &[large("asm.bom", "Bill of\nMaterials", "The parts list with quantities and volumes; export it as CSV", Icon::Bom, 0)],
            },
            RibbonPanel { title: "Update", commands: &[large("asm.update", "Update", "Solve every relationship again", Icon::Update, 0)] },
        ],
    },
    RibbonTab {
        name: "View",
        panels: &[
            RibbonPanel {
                title: "Appearance",
                commands: &[
                    large("view.style", "Visual\nStyle", "Shaded with edges, shaded, or wireframe", Icon::VisualStyle, 0).more(&[
                        "view.style.shaded_edges",
                        "view.style.shaded",
                        "view.style.wireframe",
                    ]),
                    small("view.orthographic", "Orthographic", "Parallel projection", Icon::Projection, 0),
                    small("view.perspective", "Perspective", "Perspective projection", Icon::Projection, 0),
                ],
            },
            RibbonPanel {
                title: "Visibility",
                commands: &[small("asm.dof", "Degrees of Freedom", "Show the motions each component has left", Icon::Dof, 0)],
            },
            RibbonPanel {
                title: "Windows",
                commands: &[
                    small("view.browser", "Browser", "Show or hide the model browser", Icon::Browser, 0),
                    small("view.cube", "Orientation Cube", "Show or hide the orientation cube", Icon::Cube, 0),
                    small("view.navbar", "Navigation Bar", "Show or hide the navigation bar", Icon::Window, 0),
                ],
            },
            RibbonPanel {
                title: "Navigate",
                commands: &[
                    small("view.home", "Home View", "Three-quarter view of the whole part (F6)", Icon::Home, 0),
                    small("view.fit", "Zoom All", "Fit the model in the window", Icon::ZoomFit, 0),
                    small("view.look_at", "Look At", "Look straight at the selected planar face or the active sketch", Icon::LookAt, 0),
                    small("view.previous", "Previous View", "Go back to the last view (F5)", Icon::PreviousView, 0),
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
                    large("drw.base", "Base", "A view of a part or assembly file: choose the file, orientation and scale", Icon::BaseView, 0),
                    large("drw.projected", "Projected", "Click a view, then click where each view projected from it goes", Icon::ProjectedView, 0),
                    large(
                        "drw.section",
                        "Section",
                        "Click a view, click the two ends of the section line, then where the section goes",
                        Icon::SectionView,
                        0,
                    ),
                    large(
                        "drw.detail",
                        "Detail",
                        "Click a view, click the centre of the detail, then its radius, then where it goes",
                        Icon::DetailView,
                        0,
                    ),
                    small("drw.auxiliary", "Auxiliary", "A view square to an inclined edge", Icon::ProjectedView, LATER),
                    small("drw.overlay", "Overlay", "Positions of an assembly drawn over a view", Icon::Explode, LATER),
                ],
            },
            RibbonPanel {
                title: "Modify",
                commands: &[
                    small("drw.break", "Break", "Shorten a long view", Icon::Split, LATER),
                    small("drw.break_out", "Break Out", "Cut away part of a view to show what is inside", Icon::Trim, LATER),
                    small("drw.crop", "Crop", "Show only part of a view", Icon::Rectangle, LATER),
                ],
            },
            RibbonPanel { title: "Sheets", commands: &[large("drw.sheet.new", "New\nSheet", "Add a sheet to the drawing", Icon::NewSheet, 0)] },
        ],
    },
    RibbonTab {
        name: "Annotate",
        panels: &[
            RibbonPanel {
                title: "Dimension",
                commands: &[
                    large("drw.dimension", "Dimension", "Click an edge or two, then where the dimension goes", Icon::Dimension, 0).key("D"),
                    small(
                        "drw.dimension.auto",
                        "Auto Dimension",
                        "Click a view: the dimensions it needs first, to review and add",
                        Icon::Dimension,
                        0,
                    ),
                    small("drw.baseline", "Baseline", "Dimensions from one datum", Icon::Dimension, LATER),
                    small("drw.ordinate", "Ordinate", "Ordinate dimensions", Icon::Dimension, LATER),
                ],
            },
            RibbonPanel { title: "Text", commands: &[large("drw.text", "Text", "Click where the text goes, then type it", Icon::Text, 0).key("T")] },
            RibbonPanel {
                title: "Symbols",
                commands: &[
                    large("drw.center_mark", "Centre\nMark", "Click circles and arcs: a centre mark on each", Icon::CenterMark, 0),
                    small(
                        "drw.centerline",
                        "Centreline",
                        "Click two places: circles (their centres), lines (their middles) or points",
                        Icon::Centerline,
                        0,
                    ),
                    small(
                        "drw.centerline.bisector",
                        "Centreline Bisector",
                        "Click two lines: the centreline midway between them",
                        Icon::CenterlineBisector,
                        0,
                    ),
                ],
            },
            RibbonPanel {
                title: "Table",
                commands: &[
                    large("drw.parts_list", "Parts\nList", "Click an assembly view, then where the list goes", Icon::Bom, 0),
                    large("drw.balloon", "Balloon", "Click a part in an assembly view, then where the balloon goes", Icon::Balloon, 0)
                        .key("B")
                        .more(&["drw.balloon", "drw.balloon.auto"]),
                    large("drw.hole_table", "Hole", "Click a part view, then where the hole table goes", Icon::HoleTable, 0),
                ],
            },
        ],
    },
    RibbonTab {
        name: "Tools",
        panels: &[RibbonPanel {
            title: "Options",
            commands: &[
                large("tools.options", "Application\nOptions", "Colour scheme and other settings", Icon::Settings, 0),
                large("app.about", "About", "Version, licences and kernel information", Icon::Info, 0),
            ],
        }],
    },
    RibbonTab {
        name: "Manage",
        panels: &[
            RibbonPanel {
                title: "Update",
                commands: &[large("drw.update", "Update", "Read the model files again; the views and dimensions follow", Icon::Update, 0)],
            },
            RibbonPanel {
                title: "Title Block",
                commands: &[
                    large("drw.template.apply", "Apply\nTemplate", "Use a title block template file (.json) on every sheet", Icon::Open, 0),
                    small("drw.template.save", "Save Template", "Save this sheet's title block as a template file (.json)", Icon::Save, 0),
                ],
            },
        ],
    },
    RibbonTab {
        name: "View",
        panels: &[
            RibbonPanel { title: "Windows", commands: &[small("view.browser", "Browser", "Show or hide the model browser", Icon::Browser, 0)] },
            RibbonPanel { title: "Navigate", commands: &[small("view.fit", "Zoom All", "Fit the sheet in the window", Icon::ZoomFit, 0)] },
        ],
    },
];

/// The panel shown at the end of the ribbon while a part is edited in place.
pub const RETURN: UiCommand = large("asm.return", "Return", "Finish editing the part and go back to the assembly", Icon::Return, 0);

/// The same, while a model is edited from a drawing.
pub const DRW_RETURN: UiCommand = large("drw.return", "Return", "Finish editing the model and go back to the drawing", Icon::Return, 0);

/// Commands reachable from drop-down arrows, menus and keys but not shown on the ribbon itself.
pub const EXTRA: &[UiCommand] = &[
    small("sketch.spline", "Spline", "Click control points; Enter finishes", Icon::Spline, 0),
    small("sketch.polygon", "Polygon", "Click the centre, then a corner (6 sides)", Icon::Polygon, 0),
    small("sketch.new_3d", "Start 3D Sketch", "A sketch of 3D curves", Icon::NewSketch, LATER),
    small("view.style.shaded_edges", "Shaded with Edges", "Shaded faces with their edges", Icon::VisualStyle, 0),
    small("view.style.shaded", "Shaded", "Shaded faces only", Icon::VisualStyle, 0),
    small("view.style.wireframe", "Wireframe", "Edges only", Icon::VisualStyle, 0),
    small("file.new_assembly", "New Assembly", "A new assembly of part files", Icon::Assembly, 0),
    small("asm.edit", "Edit", "Edit the selected component's part in place", Icon::Part, 0),
    RETURN,
    small("file.new_drawing", "New Drawing", "A new drawing of parts and assemblies", Icon::Drawing, 0),
    small(
        "file.new_drawing_template",
        "New Drawing from Template",
        "A new drawing starting with a template's sheets, standard, properties and notes",
        Icon::Drawing,
        0,
    ),
    small(
        "drw.save_template",
        "Save as Template",
        "This drawing's sheets, standard, properties and notes, without views, for new drawings to start from",
        Icon::Save,
        0,
    ),
    small("drw.balloon.auto", "Auto Balloon", "Click an assembly view: a balloon for each part", Icon::Balloon, 0),
    small("drw.edit_view", "Edit View", "Scale, hidden lines, centrelines and label of the selected view", Icon::BaseView, 0),
    small("drw.edit_model", "Open Model", "Edit the selected view's part or assembly; Return comes back to the drawing", Icon::Part, 0),
    small("drw.delete", "Delete", "Delete the selected view (with the views made from it) or annotation", Icon::Delete, 0),
    small("export.pdf", "Export PDF", "Every sheet as a page of a PDF", Icon::Save, 0),
    small("export.svg", "Export SVG", "The sheet as SVG", Icon::Save, 0),
    small("export.dxf", "Export DXF", "The sheet as DXF, a layer per line type", Icon::Save, 0),
    DRW_RETURN,
];

/// Navigation bar (right edge of the viewport). Pan/Zoom/Orbit make the left button do that.
pub const NAV_BAR: &[UiCommand] = &[
    small("view.pan", "Pan", "Left drag pans (also: middle drag, F2)", Icon::Pan, 0),
    small("view.zoom", "Zoom", "Left drag zooms (also: the wheel, F3)", Icon::Zoom, 0).more(&["view.zoom", "view.fit"]),
    small("view.orbit", "Orbit", "Left drag orbits (also: Shift + middle drag, F4)", Icon::Orbit, 0),
    small("view.look_at", "Look At", "Look straight at the selected planar face or the active sketch", Icon::LookAt, 0),
];

/// Quick-access toolbar (title bar), in groups.
pub const QUICK_ACCESS: &[&[UiCommand]] = &[
    &[
        small("file.new", "New", "New part (Ctrl+N)", Icon::New, 0),
        small("file.open", "Open", "Open a project (Ctrl+O)", Icon::Open, 0),
        small("file.save", "Save", "Save the project (Ctrl+S)", Icon::Save, 0),
    ],
    &[small("edit.undo", "Undo", "Undo (Ctrl+Z)", Icon::Undo, 0), small("edit.redo", "Redo", "Redo (Ctrl+Y)", Icon::Redo, 0)],
    &[
        small("view.home", "Home View", "Three-quarter view of the whole part (F6)", Icon::Home, 0),
        small("model.rebuild", "Rebuild All", "Regenerate every feature from scratch", Icon::Update, 0),
    ],
    &[
        small("tools.parameters", "Parameters", "Named parameters and expressions", Icon::Parameters, 0),
        small("inspect.measure", "Measure", "Distances, angles, lengths and areas", Icon::Measure, 0),
    ],
];

/// File menu entries (label, command id).
pub const FILE_MENU: &[(&str, &str)] = &[
    ("New Part", "file.new"),
    ("New Assembly", "file.new_assembly"),
    ("New Drawing", "file.new_drawing"),
    ("New Drawing from Template...", "file.new_drawing_template"),
    ("Open...", "file.open"),
    ("Save", "file.save"),
    ("Save As...", "file.save_as"),
    ("Export STEP...", "export.step"),
    ("Export STL...", "export.stl"),
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
    Part { sketching: bool },
    Assembly,
    Drawing,
}

/// Commands matching a search, best first: label prefix, then word prefix, then substring.
pub fn search(query: &str) -> Vec<&'static UiCommand> {
    let q = query.trim().to_lowercase();
    if q.is_empty() {
        return Vec::new();
    }
    let mut seen = std::collections::BTreeSet::new();
    let mut scored: Vec<(u8, &'static UiCommand)> = all()
        .filter(|c| seen.insert(c.id))
        .filter_map(|c| {
            let name = c.name().to_lowercase();
            let rank = if name.starts_with(&q) {
                0
            } else if name.split(' ').any(|w| w.starts_with(&q)) {
                1
            } else if name.contains(&q) || c.id.contains(&q) || c.tip.to_lowercase().contains(&q) {
                2
            } else {
                return None;
            };
            Some((rank + if c.available() { 0 } else { 3 }, c))
        })
        .collect();
    scored.sort_by_key(|(r, c)| (*r, c.name()));
    scored.into_iter().map(|(_, c)| c).collect()
}

/// The command for a single-key shortcut. While sketching, sketch keys come first and the rest
/// still work (E extrudes, finishing the sketch). In an assembly, the Assemble tab's keys; in a
/// drawing, the Place Views and Annotate tabs'.
pub fn for_key(key: &str, env: Env) -> Option<&'static UiCommand> {
    let (ribbon, tabs): (&[RibbonTab], &[usize]) = match env {
        Env::Part { sketching: true } => (RIBBON, &[SKETCH_TAB, MODEL_TAB]),
        Env::Part { sketching: false } => (RIBBON, &[MODEL_TAB]),
        Env::Assembly => (ASM_RIBBON, &[0]),
        Env::Drawing => (DRW_RIBBON, &[0, 1]),
    };
    tabs.iter()
        .filter_map(|t| ribbon.get(*t))
        .flat_map(|t| t.panels.iter())
        .flat_map(|p| p.commands.iter())
        .find(|c| c.key == Some(key) && c.available())
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
            assert!(c.milestone <= 11 || c.milestone == LATER, "{}", c.id);
            let first = find(c.id).unwrap();
            assert_eq!((first.name(), first.milestone), (c.name(), c.milestone), "{}", c.id);
            for m in c.more {
                assert!(find(m).is_some(), "{}: drop-down entry {m} is not a command", c.id);
            }
        }
    }

    #[test]
    fn ribbon_has_the_planned_tabs() {
        let names: Vec<_> = RIBBON.iter().map(|t| t.name).collect();
        assert_eq!(names, ["3D Model", "Sketch", "Inspect", "Tools", "Manage", "View"]);
        assert_eq!(RIBBON[MODEL_TAB].name, "3D Model");
        assert_eq!(RIBBON[SKETCH_TAB].name, "Sketch");
        assert!(RIBBON.iter().all(|t| !t.panels.is_empty() && t.panels.iter().all(|p| !p.commands.is_empty())));
        let names: Vec<_> = DRW_RIBBON.iter().map(|t| t.name).collect();
        assert_eq!(names, ["Place Views", "Annotate", "Tools", "Manage", "View"]);
    }

    #[test]
    fn search_and_shortcuts() {
        assert_eq!(search("extr").first().map(|c| c.id), Some("model.extrude"));
        assert_eq!(search("start").first().map(|c| c.id), Some("sketch.new"), "working commands before later milestones");
        assert!(search("sketch").iter().take(2).any(|c| c.id == "sketch.finish"), "a word prefix ranks above a substring");
        assert!(search("zzzz").is_empty());
        let part = Env::Part { sketching: false };
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
