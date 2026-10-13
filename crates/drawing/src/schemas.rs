//! What every drawing command of this crate takes (`tenon_model::schema`). One entry for each
//! command of `cmd::commands()`; a test holds the two lists together, and every run of a
//! command in a test holds its entry to what the command is given and what it reads.

use tenon_model::schema::Ty::{Bool, Either, Id, Int, Length, List, Null, Number, Object, Text, Vector, Word};
use tenon_model::schema::{CommandSchema as C, Param, Ty, opt, req};

/// A point: `[x, y]`, or `{"x": .., "y": ..}`.
pub const POINT: Ty = Either(&[Vector(2), Object]);
/// A view's scale: a number, or text such as "1:2".
pub const SCALE: Ty = Either(&[Number, Text]);
/// What a dimension or a mark holds on: made by drw.pick.
const PICK: Ty = Object;
pub const SIZES: Ty = Word(&["A", "B", "C", "D", "A4", "A3", "A2", "A1"]);

const VIEW: Param = req("view", Id, "the view's id, as drw.info lists it");
const PARENT: Param = req("parent", Id, "the view it is made from");
const ANNOTATION: Param = req("annotation", Id, "the annotation's id, as drw.info lists it");
pub const AT: Param = opt("at", POINT, "where it goes on the sheet, [x, y] in mm from the sheet's lower left");
pub const VIEW_SCALE: Param = opt("scale", SCALE, "a number or \"1:2\" (default: its parent's, or what fits the sheet)");

static SCHEMAS: &[C] = &[
    C::new("drw.info", &[]),
    C::new(
        "drw.pick",
        &[
            VIEW,
            opt("at", POINT, "a sheet point: the edge drawn nearest it is picked, as a click picks it"),
            opt("view_at", POINT, "or the same in the view's own coordinates (model mm)"),
            opt(
                "edge",
                Either(&[List(&Object), Object]),
                "or the edge itself: the names of the two faces it lies between, or {\"body\": b, \"index\": n}",
            ),
            opt("component", Id, "with `edge`, in a view of an assembly: the component the edge is on"),
            opt("point", Word(&["start", "end", "mid", "center", "whole"]), "which point of the edge (default whole: the edge itself)"),
        ],
    ),
    C::new(
        "drw.view.projected",
        &[
            PARENT,
            req(
                "side",
                Word(&["right", "left", "above", "below", "above_right", "above_left", "below_right", "below_left"]),
                "which side of its parent it is seen from (the diagonals are isometric)",
            ),
            AT,
            VIEW_SCALE,
            opt("hidden", Bool, "draw hidden lines (default: as its parent; not for isometric)"),
        ],
    ),
    C::new(
        "drw.view.section",
        &[
            PARENT,
            req("a", POINT, "one end of the section line, in the parent view (model mm; see drw.to_view)"),
            req("b", POINT, "its other end"),
            opt("flip", Bool, "look the other way (default false)"),
            AT,
            VIEW_SCALE,
        ],
    ),
    C::new(
        "drw.view.detail",
        &[
            PARENT,
            req("center", POINT, "the middle of the detail, in the parent view (model mm)"),
            req("radius", Length, "how much of the parent it shows (model mm)"),
            VIEW_SCALE,
            AT,
        ],
    ),
    C::new(
        "drw.view.edit",
        &[
            VIEW,
            opt("at", POINT, "where it moves to on the sheet"),
            opt("by", POINT, "or how far it moves, [dx, dy]"),
            VIEW_SCALE,
            opt("hidden", Bool, "draw hidden lines"),
            opt("tangent", Bool, "draw the edges where faces meet smoothly"),
            opt("centerlines", Bool, "draw centre lines and marks"),
            opt("label", Bool, "show the view's label"),
        ],
    ),
    C::new("drw.view.delete", &[VIEW]),
    C::new(
        "drw.dimension",
        &[
            VIEW,
            req("type", Word(&["horizontal", "vertical", "aligned", "diameter", "radius", "angle"]), "what it measures"),
            req("a", PICK, "a pick from drw.pick: a line alone for its length, a circle for its size"),
            opt("b", PICK, "a second pick, to measure between the two"),
            opt("at", POINT, "where the dimension line or its text goes, on the sheet"),
            opt("by", POINT, "or how far from the middle of what is measured, [dx, dy]"),
            opt("text", Text, "what it says: \"<>\" stands for the value"),
            opt("precision", Int, "decimals (default 2)"),
        ],
    ),
    C::new("drw.center_mark", &[VIEW, req("a", PICK, "a pick of a circle or an arc")]),
    C::new("drw.centerline", &[VIEW, req("a", PICK, "a pick: the line starts at its point"), req("b", PICK, "a pick: and runs through this one's")]),
    C::new("drw.centerline.bisector", &[VIEW, req("a", PICK, "a pick of a straight edge"), req("b", PICK, "a pick of another")]),
    C::new("drw.dimension.suggest", &[VIEW]),
    C::new("drw.dimension.auto", &[VIEW, opt("accept", List(&Int), "which of drw.dimension.suggest's, by number (default all)")]),
    C::new(
        "drw.hole_table",
        &[
            VIEW,
            opt("origin", PICK, "a pick for the datum the holes are measured from (default: the view's lower left)"),
            opt("at", POINT, "the table's top left on the sheet (default: the top right of the sheet)"),
        ],
    ),
    C::new("drw.parts_list", &[VIEW, opt("at", POINT, "the table's top left on the sheet (default: above the title block)")]),
    C::new(
        "drw.balloon",
        &[
            VIEW,
            opt("component", Id, "the component it names"),
            opt("attach_at", POINT, "or a sheet point on an edge of the component: the leader ends there"),
            opt("at", POINT, "where the balloon goes (default: out from the view)"),
        ],
    ),
    C::new("drw.balloon.auto", &[VIEW]),
    C::new(
        "drw.note",
        &[
            req("text", Text, "what it says"),
            opt("at", POINT, "where it goes on the sheet"),
            opt("sheet", Id, "which sheet (default the first)"),
            opt("height", Length, "the height of its letters (default 3.5)"),
        ],
    ),
    C::new(
        "drw.annotation.edit",
        &[
            ANNOTATION,
            opt("by", POINT, "how far to move it, [dx, dy]"),
            opt("text", Either(&[Text, Null]), "a dimension's text (\"<>\" is the value; null for the value alone), or a note's"),
            opt("precision", Int, "a dimension's decimals"),
        ],
    ),
    C::new("drw.delete", &[ANNOTATION]),
    C::new(
        "drw.props",
        &[
            opt("title", Text, "the title block's title"),
            opt("number", Text, "the drawing number"),
            opt("revision", Text, "the revision"),
            opt("company", Text, "the company"),
            opt("drawn_by", Text, "who drew it"),
            opt("date", Text, "the date"),
        ],
    ),
    C::new("drw.sheet.add", &[opt("size", SIZES, "the sheet's size (default: the standard's)")]),
    C::new("drw.sheet.size", &[req("sheet", Id, "the sheet"), req("size", SIZES, "its new size")]),
    C::new("drw.to_view", &[VIEW, req("at", POINT, "a point on the sheet")]),
    C::new("drw.to_sheet", &[VIEW, req("at", POINT, "a point in the view (model mm)")]),
    C::new("drw.undo", &[]),
    C::new("drw.redo", &[]),
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
