//! The ribbon's command table. Every button maps to a named command id (`area.verb`). Buttons that
//! edit the model end in the shared command registry (`tenon_model::cmd`, `tenon_io::cmd`); the
//! rest are UI actions (tools, views). `milestone` says when a command starts working: 0 means it
//! works today.

use crate::icons::Icon;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Size {
    Large,
    Small,
}

#[derive(Clone, Copy, Debug)]
pub struct UiCommand {
    pub id: &'static str,
    pub label: &'static str,
    pub tip: &'static str,
    pub icon: Icon,
    pub size: Size,
    pub milestone: u8,
}

impl UiCommand {
    pub fn available(&self) -> bool {
        self.milestone == 0
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

const fn large(id: &'static str, label: &'static str, tip: &'static str, icon: Icon, milestone: u8) -> UiCommand {
    UiCommand { id, label, tip, icon, size: Size::Large, milestone }
}
const fn small(id: &'static str, label: &'static str, tip: &'static str, icon: Icon, milestone: u8) -> UiCommand {
    UiCommand { id, label, tip, icon, size: Size::Small, milestone }
}

pub const RIBBON: &[RibbonTab] = &[
    RibbonTab {
        name: "Model",
        panels: &[
            RibbonPanel {
                title: "Sketch",
                commands: &[large("sketch.new", "New Sketch", "Start a 2D sketch on an origin plane or a selected planar face", Icon::NewSketch, 0)],
            },
            RibbonPanel {
                title: "Create",
                commands: &[
                    large("model.extrude", "Extrude", "Add or cut material by sweeping a profile straight", Icon::Extrude, 0),
                    large("model.revolve", "Revolve", "Add or cut material by rotating a profile about an axis", Icon::Revolve, 0),
                    small("model.sweep", "Sweep", "Sweep a profile along a path", Icon::Sweep, 5),
                    small("model.loft", "Loft", "Blend between two or more profiles", Icon::Loft, 5),
                    small("model.coil", "Coil", "Helical sweep", Icon::Coil, 5),
                ],
            },
            RibbonPanel {
                title: "Modify",
                commands: &[
                    large("model.hole", "Hole", "Simple, counterbore or countersink holes", Icon::Hole, 2),
                    large("model.fillet", "Fillet", "Round edges", Icon::Fillet, 2),
                    small("model.chamfer", "Chamfer", "Bevel edges", Icon::Chamfer, 2),
                    small("model.shell", "Shell", "Hollow a solid, removing faces", Icon::Shell, 2),
                ],
            },
            RibbonPanel {
                title: "Work Features",
                commands: &[
                    large("work.plane", "Plane", "Construction plane", Icon::Plane, 2),
                    small("work.axis", "Axis", "Construction axis", Icon::Axis, 2),
                    small("work.point", "Point", "Construction point", Icon::Point, 2),
                ],
            },
            RibbonPanel {
                title: "Pattern",
                commands: &[
                    small("model.pattern.rect", "Rectangular", "Repeat features in rows and columns", Icon::PatternRect, 2),
                    small("model.pattern.circular", "Circular", "Repeat features around an axis", Icon::PatternCircular, 2),
                    small("model.mirror", "Mirror", "Mirror features across a plane", Icon::Mirror, 2),
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
                    large("sketch.line", "Line", "Click points; each line starts where the last ended. Esc ends the chain.", Icon::Line, 0),
                    small("sketch.circle", "Circle", "Click the centre, then a point on the circle", Icon::Circle, 0),
                    small("sketch.arc", "Arc", "Click the start, the end, then a point on the arc", Icon::Arc, 0),
                    small("sketch.rectangle", "Rectangle", "Click two opposite corners", Icon::Rectangle, 0),
                    small("sketch.polygon", "Polygon", "Click the centre, then a corner (6 sides)", Icon::Polygon, 0),
                    small("sketch.spline", "Spline", "Click control points; Enter finishes", Icon::Spline, 0),
                    small("sketch.point", "Point", "Click to place a point (e.g. a hole centre)", Icon::Point, 0),
                ],
            },
            RibbonPanel {
                title: "Modify",
                commands: &[
                    small("sketch.trim", "Trim", "Click the piece of a curve to remove", Icon::Trim, 0),
                    small("sketch.offset", "Offset", "Select curves, then choose a distance", Icon::Offset, 0),
                    small("sketch.mirror", "Mirror", "Select geometry, then click the mirror line", Icon::Mirror, 0),
                    small("sketch.fillet", "Fillet", "Click a corner where two lines meet", Icon::Fillet, 0),
                ],
            },
            RibbonPanel {
                title: "Constrain",
                commands: &[
                    large("sketch.dimension", "Dimension", "Click a line, circle, arc, two points or two lines", Icon::Dimension, 0),
                    small("sketch.coincident", "Coincident", "Two points, or a point and a curve", Icon::Coincident, 0),
                    small("sketch.horizontal", "Horizontal", "A line", Icon::Horizontal, 0),
                    small("sketch.vertical", "Vertical", "A line", Icon::Vertical, 0),
                    small("sketch.parallel", "Parallel", "Two lines", Icon::Parallel, 0),
                    small("sketch.perpendicular", "Perpendicular", "Two lines", Icon::Perpendicular, 0),
                    small("sketch.tangent", "Tangent", "A line and a circle/arc, or two circles/arcs", Icon::Tangent, 0),
                    small("sketch.equal", "Equal", "Two lines or two circles/arcs", Icon::Parallel, 0),
                    small("sketch.concentric", "Concentric", "Two circles/arcs", Icon::Circle, 0),
                    small("sketch.fix", "Fix", "A point that must not move", Icon::Point, 0),
                ],
            },
            RibbonPanel { title: "Exit", commands: &[large("sketch.finish", "Finish Sketch", "Leave the sketch", Icon::FinishSketch, 0)] },
        ],
    },
    RibbonTab {
        name: "Inspect",
        panels: &[RibbonPanel {
            title: "Measure",
            commands: &[
                large("inspect.measure", "Measure", "Distances, angles, lengths and areas", Icon::Measure, 2),
                large("inspect.mass", "Mass", "Volume, area, centre of mass, inertia", Icon::MassProps, 0),
            ],
        }],
    },
    RibbonTab {
        name: "Tools",
        panels: &[RibbonPanel {
            title: "Options",
            commands: &[
                large("tools.parameters", "Parameters", "Named parameters and expressions", Icon::Parameters, 2),
                large("tools.options", "Options", "Application settings", Icon::Settings, 5),
                large("app.about", "About", "Version, licences and kernel information", Icon::Info, 0),
            ],
        }],
    },
    RibbonTab {
        name: "View",
        panels: &[
            RibbonPanel {
                title: "Windows",
                commands: &[
                    large("view.browser", "Browser", "Show or hide the model browser", Icon::Browser, 0),
                    large("view.cube", "Orientation Cube", "Show or hide the orientation cube", Icon::Cube, 0),
                ],
            },
            RibbonPanel {
                title: "Navigate",
                commands: &[
                    small("view.home", "Home", "Three-quarter view of the whole part", Icon::Home, 0),
                    small("view.fit", "Zoom All", "Fit the model in the window", Icon::ZoomFit, 0),
                    small("view.look_at", "Look At", "Look straight at the selected planar face or the active sketch", Icon::LookAt, 0),
                ],
            },
        ],
    },
];

/// Navigation bar (right edge of the viewport). Orbit/Pan/Zoom make the left button do that.
pub const NAV_BAR: &[UiCommand] = &[
    small("view.orbit", "Orbit", "Left-drag orbits (also: right-drag)", Icon::Orbit, 0),
    small("view.pan", "Pan", "Left-drag pans (also: middle-drag)", Icon::Pan, 0),
    small("view.zoom", "Zoom", "Left-drag zooms (also: the wheel)", Icon::Zoom, 0),
    small("view.fit", "Zoom All", "Fit the model in the window", Icon::ZoomFit, 0),
    small("view.look_at", "Look At", "Look straight at the selected planar face or the active sketch", Icon::LookAt, 0),
];

/// Quick-access toolbar (title bar).
pub const QUICK_ACCESS: &[UiCommand] = &[
    small("file.new", "New", "New part (Ctrl+N)", Icon::New, 0),
    small("file.open", "Open", "Open a project (Ctrl+O)", Icon::Open, 0),
    small("file.save", "Save", "Save the project (Ctrl+S)", Icon::Save, 0),
    small("edit.undo", "Undo", "Undo (Ctrl+Z)", Icon::Undo, 0),
    small("edit.redo", "Redo", "Redo (Ctrl+Y)", Icon::Redo, 0),
];

/// File menu entries (label, command id).
pub const FILE_MENU: &[(&str, &str)] = &[
    ("New Part", "file.new"),
    ("Open...", "file.open"),
    ("Save", "file.save"),
    ("Save As...", "file.save_as"),
    ("Export STEP...", "export.step"),
    ("Export STL...", "export.stl"),
];

/// Every command reachable from the ribbon and toolbars.
pub fn all() -> impl Iterator<Item = &'static UiCommand> {
    RIBBON.iter().flat_map(|t| t.panels.iter()).flat_map(|p| p.commands.iter()).chain(NAV_BAR).chain(QUICK_ACCESS)
}

/// Looks up a command by id.
pub fn find(id: &str) -> Option<&'static UiCommand> {
    all().find(|c| c.id == id)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ids_are_well_formed_and_consistent() {
        for c in all() {
            assert!(c.id.contains('.') && c.id.chars().all(|ch| ch.is_ascii_lowercase() || ch == '.' || ch == '_'), "{}", c.id);
            assert!(!c.label.is_empty() && !c.tip.is_empty(), "{}", c.id);
            assert!(c.milestone <= 6, "{}", c.id);
            let first = find(c.id).unwrap();
            assert_eq!((first.label, first.milestone), (c.label, c.milestone), "{}", c.id);
        }
    }

    #[test]
    fn ribbon_has_the_planned_tabs() {
        let names: Vec<_> = RIBBON.iter().map(|t| t.name).collect();
        assert_eq!(names, ["Model", "Sketch", "Inspect", "Tools", "View"]);
        assert!(RIBBON.iter().all(|t| !t.panels.is_empty() && t.panels.iter().all(|p| !p.commands.is_empty())));
    }
}
