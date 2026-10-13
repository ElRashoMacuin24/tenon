//! What every assembly command of this crate takes (`tenon_model::schema`). One entry for each
//! command of `cmd::commands()`; a test holds the two lists together, and every run of a
//! command in a test holds its entry to what the command is given and what it reads.

use tenon_model::schema::Ty::{Angle, Bool, Either, Id, Length, List, Null, Object, Text, Vector, Word};
use tenon_model::schema::{CommandSchema as C, Param, Ty, opt, req};

const COMPONENT: Param = req("component", Id, "the component's id, as asm.tree lists it");
const RELATIONSHIP: Param = req("relationship", Id, "the relationship's id, as asm.tree lists it");
/// What a relationship holds on: made by asm.geom.
const TARGET: Ty = Object;
const JOINTS: Ty = Word(&["rigid", "revolute", "slider", "cylindrical", "planar", "ball"]);
/// A direction: three numbers, or an axis by name.
const DIRECTION: Ty = Either(&[Vector(3), Word(&["x", "y", "z", "-x", "-y", "-z", "+x", "+y", "+z"])]);

static SCHEMAS: &[C] = &[
    C::new("asm.tree", &[]),
    C::new(
        "asm.geom",
        &[
            opt("component", Id, "the component the geometry is on (none: the assembly's own origin planes, axes and point)"),
            opt("face", Object, "a face: its name (a face origin, as model.faces lists them) or {\"body\": b, \"index\": n}"),
            opt("edge", Either(&[List(&Object), Object]), "an edge: the names of the two faces it lies between, or {\"body\": b, \"index\": n}"),
            opt("plane", Word(&["xy", "yz", "xz"]), "an origin plane of the component's part, or of the assembly"),
            opt("axis", Word(&["x", "y", "z"]), "an origin axis"),
            opt("origin", Bool, "true: the origin point"),
            opt("work", Id, "a work plane, axis or point of the part: its feature id"),
        ],
    ),
    C::new(
        "asm.constrain",
        &[
            req("type", Word(&["mate", "flush", "angle", "insert"]), "what kind of constraint"),
            req("a", TARGET, "a target from asm.geom"),
            req("b", TARGET, "a target from asm.geom, on another component"),
            opt("offset", Length, "the distance held between the two (default 0)"),
            opt("angle", Angle, "for angle: the angle held between the two"),
            opt("aligned", Bool, "for insert: the two axes point the same way (default false: opposed)"),
        ],
    ),
    C::new(
        "asm.joint",
        &[
            req("type", JOINTS, "what kind of joint"),
            req("a", TARGET, "a target from asm.geom: the joint's origin on one component"),
            req("b", TARGET, "a target from asm.geom: its origin on the other"),
            opt("flip", Bool, "the two Z axes point the same way (default false: opposed)"),
            opt("offset", Length, "along A's Z (default 0)"),
            opt("angle", Angle, "rigid and slider: the turn about Z (default 0)"),
        ],
    ),
    C::new(
        "asm.edit_relationship",
        &[
            RELATIONSHIP,
            opt("offset", Length, "its new offset"),
            opt("angle", Angle, "its new angle"),
            opt("flip", Bool, "a joint's Z axes the same way"),
            opt("aligned", Bool, "an insert's axes the same way"),
            opt("type", JOINTS, "a joint's new kind"),
        ],
    ),
    C::new("asm.suppress", &[RELATIONSHIP, opt("suppressed", Bool, "true to suppress (default), false to bring it back")]),
    C::with(
        "asm.delete",
        &[opt("component", Id, "a component to delete, with its relationships"), opt("relationship", Id, "or a relationship")],
        &[&[&["component"], &["relationship"]]],
    ),
    C::new(
        "asm.rename",
        &[
            req("name", Text, "the new name (1 to 256 characters)"),
            opt("component", Id, "the component to rename"),
            opt("relationship", Id, "or the relationship (neither: the assembly)"),
        ],
    ),
    C::new(
        "asm.set_row",
        &[
            COMPONENT,
            opt(
                "row",
                Either(&[Text, Null]),
                "a row of its part's design table: the component is the part in that size; null for the part as its file has it",
            ),
        ],
    ),
    C::new("asm.ground", &[COMPONENT, opt("grounded", Bool, "true to hold it where it is (default), false to free it")]),
    C::new("asm.visible", &[COMPONENT, opt("visible", Bool, "true to show it (default), false to hide it")]),
    C::new(
        "asm.move",
        &[
            COMPONENT,
            opt("by", Vector(3), "how far to move it, [x, y, z]"),
            opt("turn", Object, "{\"axis\": [x, y, z] or \"x\", \"angle\": rad, \"through\": [x, y, z] (default: its centre)}"),
        ],
    ),
    C::new(
        "asm.place",
        &[
            COMPONENT,
            req("origin", Vector(3), "where its part's origin goes, [x, y, z]"),
            opt("z", DIRECTION, "its part's Z direction (default unturned)"),
            opt("x", DIRECTION, "its part's X direction"),
        ],
    ),
    C::new("asm.update", &[]),
    C::new("asm.dof", &[]),
    C::new("asm.bom", &[]),
    C::new("asm.mass", &[]),
    C::new(
        "asm.explode.add",
        &[
            req("components", List(&Id), "the components to move apart"),
            req("direction", DIRECTION, "which way: [x, y, z], or \"x\", \"-z\" and so on"),
            req("distance", Length, "how far"),
        ],
    ),
    C::new("asm.explode.auto", &[opt("spacing", Length, "the room left between components (default a quarter of the assembly's size)")]),
    C::new("asm.explode.clear", &[opt("step", Id, "which step of the exploded view to take out (default: every step)")]),
    C::new("asm.explode.positions", &[]),
    C::new("asm.undo", &[]),
    C::new("asm.redo", &[]),
    C::new("asm.interference", &[opt("components", List(&Id), "the components to check against one another (default all)")]),
    C::new(
        "asm.edit_part",
        &[COMPONENT, req("run", Text, "a part command's id, e.g. sketch.set_dimension"), opt("with", Object, "that command's parameters")],
    ),
];

/// The schema of the command `id`, if it is one of this registry's.
pub fn schema(id: &str) -> Option<&'static C> {
    SCHEMAS.iter().find(|s| s.id == id)
}

/// Every schema of this registry.
pub fn schemas() -> &'static [C] {
    SCHEMAS
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_command_has_a_schema_and_every_schema_a_command() {
        let commands: Vec<&str> = crate::cmd::commands().iter().map(|c| c.id).collect();
        let declared: Vec<&str> = SCHEMAS.iter().map(|s| s.id).collect();
        assert_eq!(declared, commands, "one schema for each command, in the registry's order");
        assert_eq!(tenon_model::schema::sound(SCHEMAS), Ok(()));
    }
}
