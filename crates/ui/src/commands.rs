//! The ribbon's command table. Every button maps to a named command id (`area.verb`); the same ids
//! will be served by the command registry to the CLI, scripts and MCP from M1. `milestone` says
//! when the command starts working: 0 means it works today.

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
                commands: &[large("sketch.new", "New Sketch", "Start a 2D sketch on a plane or planar face", Icon::NewSketch, 1)],
            },
            RibbonPanel {
                title: "Create",
                commands: &[
                    large("model.extrude", "Extrude", "Add or cut material by sweeping a profile straight", Icon::Extrude, 1),
                    large("model.revolve", "Revolve", "Add or cut material by rotating a profile about an axis", Icon::Revolve, 1),
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
                    large("sketch.line", "Line", "Lines and tangent arcs", Icon::Line, 1),
                    small("sketch.circle", "Circle", "Circle by centre and radius", Icon::Circle, 1),
                    small("sketch.arc", "Arc", "Three-point or centre arc", Icon::Arc, 1),
                    small("sketch.rectangle", "Rectangle", "Two-point rectangle", Icon::Rectangle, 1),
                    small("sketch.polygon", "Polygon", "Regular polygon", Icon::Polygon, 1),
                    small("sketch.spline", "Spline", "Interpolated spline", Icon::Spline, 1),
                    small("sketch.point", "Point", "Sketch point or hole centre", Icon::Point, 1),
                ],
            },
            RibbonPanel {
                title: "Modify",
                commands: &[
                    small("sketch.trim", "Trim", "Trim curves to the nearest intersection", Icon::Trim, 1),
                    small("sketch.offset", "Offset", "Offset a chain of curves", Icon::Offset, 1),
                    small("sketch.mirror", "Mirror", "Mirror sketch geometry", Icon::Mirror, 1),
                    small("sketch.fillet", "Fillet", "Round a sketch corner", Icon::Fillet, 1),
                ],
            },
            RibbonPanel {
                title: "Constrain",
                commands: &[
                    large("sketch.dimension", "Dimension", "Driving dimension (distance, angle, radius, diameter)", Icon::Dimension, 1),
                    small("sketch.coincident", "Coincident", "Make points coincide", Icon::Coincident, 1),
                    small("sketch.horizontal", "Horizontal", "Make a line horizontal", Icon::Horizontal, 1),
                    small("sketch.vertical", "Vertical", "Make a line vertical", Icon::Vertical, 1),
                    small("sketch.parallel", "Parallel", "Make lines parallel", Icon::Parallel, 1),
                    small("sketch.perpendicular", "Perpendicular", "Make lines perpendicular", Icon::Perpendicular, 1),
                    small("sketch.tangent", "Tangent", "Make curves tangent", Icon::Tangent, 1),
                ],
            },
            RibbonPanel { title: "Exit", commands: &[large("sketch.finish", "Finish Sketch", "Leave the sketch", Icon::FinishSketch, 1)] },
        ],
    },
    RibbonTab {
        name: "Inspect",
        panels: &[RibbonPanel {
            title: "Measure",
            commands: &[
                large("inspect.measure", "Measure", "Distances, angles, lengths and areas", Icon::Measure, 1),
                large("inspect.mass", "Mass", "Volume, mass, centre of mass, inertia", Icon::MassProps, 1),
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
                    small("view.home", "Home", "Home view", Icon::Home, 1),
                    small("view.fit", "Zoom All", "Fit the model in the window", Icon::ZoomFit, 1),
                    small("view.look_at", "Look At", "Look straight at a face or plane", Icon::LookAt, 1),
                ],
            },
        ],
    },
];

/// Navigation bar (right edge of the viewport).
pub const NAV_BAR: &[UiCommand] = &[
    small("view.orbit", "Orbit", "Orbit the view", Icon::Orbit, 1),
    small("view.pan", "Pan", "Pan the view", Icon::Pan, 1),
    small("view.zoom", "Zoom", "Zoom the view", Icon::Zoom, 1),
    small("view.fit", "Zoom All", "Fit the model in the window", Icon::ZoomFit, 1),
    small("view.look_at", "Look At", "Look straight at a face or plane", Icon::LookAt, 1),
];

/// Quick-access toolbar (title bar).
pub const QUICK_ACCESS: &[UiCommand] = &[
    small("file.new", "New", "New part", Icon::New, 1),
    small("file.open", "Open", "Open a project", Icon::Open, 1),
    small("file.save", "Save", "Save the project", Icon::Save, 1),
    small("edit.undo", "Undo", "Undo", Icon::Undo, 1),
    small("edit.redo", "Redo", "Redo", Icon::Redo, 1),
];

/// Every command reachable from the shell.
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
            // A command that appears twice must be described identically.
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

    #[test]
    fn only_shell_commands_claim_to_work_in_m0() {
        let available: Vec<_> = all().filter(|c| c.available()).map(|c| c.id).collect();
        assert_eq!(available, ["app.about", "view.browser", "view.cube"]);
    }
}
