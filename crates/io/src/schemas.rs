//! What the commands of this crate take (`tenon_model::schema`): the file, export and print
//! commands of parts, assemblies and drawings. And the one place to ask for any command's
//! schema, whichever registry it is in ([`schema`]).

use tenon_drawing::schemas::{AT, SIZES, VIEW_SCALE};
use tenon_model::schema::Ty::{Angle, Bool, Either, Face, Id, Int, Length, Null, Number, Path, Text, Vector, Word};
use tenon_model::schema::{CommandSchema as C, Param, opt, req};

const LINEAR: Param = opt("linear", Length, "how far the mesh may be from the surface (default 0.01)");
const ANGULAR: Param = opt("angular", Angle, "the most neighbouring facets may turn from one another (default 0.2)");
const EXPLODED: Param = opt("exploded", Bool, "as the exploded view has it (default false)");
const AT3: Param = opt("at", Vector(3), "where it goes, [x, y, z] (default: grounded at the origin for the first, beside the others after)");
const GROUNDED: Param = opt("grounded", Bool, "held where it is (default: the first component is)");
const SHEET: Param = opt("sheet", Id, "which sheet");
const BODY: Param = opt("body", Int, "which body (default 0)");
const FLAT_FACE: Param = opt("face", Face, "the face to lay flat from (default: the A side)");

/// The part's file commands, in the order of `cmd::commands()`.
static PART: &[C] = &[
    C::new("file.new", &[opt("name", Text, "the new part's name (default Part1)")]),
    C::new("file.save", &[req("path", Path, "the part file to write (.tenon)")]),
    C::new("file.open", &[req("path", Path, "the part file to read (.tenon)")]),
    C::new("library.list", &[opt("category", Text, "only the standard parts of this category")]),
    C::new(
        "library.new",
        &[
            req("part", Text, "a standard part's id (see library.list)"),
            opt("size", Text, "one of its sizes (default: the one it is drawn at, such as M8x30 for a screw)"),
        ],
    ),
    C::new("file.diff", &[req("a", Path, "a part, assembly or drawing file"), req("b", Path, "the file to compare it with")]),
    C::new("export.step", &[req("path", Path, "the file to write (.step)")]),
    C::new("export.stl", &[req("path", Path, "the file to write (.stl)"), LINEAR, ANGULAR, opt("ascii", Bool, "write text (default false: binary)")]),
    C::new("export.3mf", &[req("path", Path, "the file to write (.3mf)"), LINEAR, ANGULAR]),
    C::new(
        "print.check",
        &[
            opt("min_wall", Number, "the thinnest wall the printer makes, in mm (default 0.8)"),
            opt("overhang", Number, "how far a face may lean out from vertical, in degrees (default 45)"),
            opt("up", Word(&["x", "y", "z", "-x", "-y", "-z"]), "which way is up on the printer (default z)"),
            opt("bed", Either(&[Vector(3), Null]), "the printer's room, [x, y, height] in mm (default: not checked)"),
        ],
    ),
    C::new(
        "sheet.export_dxf",
        &[
            req("path", Path, "the file to write (.dxf)"),
            opt("bends", Bool, "write the bend centre lines (default true)"),
            opt("extents", Bool, "write where bends begin and end (default true)"),
            FLAT_FACE,
            BODY,
        ],
    ),
    C::new(
        "sheet.export_svg",
        &[
            req("path", Path, "the file to write (.svg)"),
            opt("bends", Bool, "draw the bend centre lines (default true)"),
            opt("extents", Bool, "draw where bends begin and end (default true)"),
            FLAT_FACE,
            BODY,
        ],
    ),
];

/// The assembly's file commands, in the order of `asm::commands()`.
static ASSEMBLY: &[C] = &[
    C::new("asm.new", &[opt("name", Text, "the new assembly's name (default Assembly1)")]),
    C::new("asm.open", &[req("path", Path, "the assembly file to read (.tenonasm)")]),
    C::new("asm.save", &[req("path", Path, "the assembly file to write (.tenonasm)")]),
    C::new(
        "asm.insert",
        &[
            req("path", Path, "the part file to place (.tenon)"),
            AT3,
            GROUNDED,
            opt("row", Text, "a row of the part's design table: the component is the part in that size"),
        ],
    ),
    C::new(
        "asm.insert_standard",
        &[
            req("part", Text, "a standard part's id (see library.list)"),
            opt("size", Text, "one of its sizes (default: the one it is drawn at)"),
            req("path", Path, "the folder its file is kept in, beside the assembly (written the first time the part is used)"),
            AT3,
            GROUNDED,
        ],
    ),
    C::new("asm.export_step", &[req("path", Path, "the file to write (.step)"), EXPLODED]),
    C::new(
        "asm.export_stl",
        &[req("path", Path, "the file to write (.stl)"), EXPLODED, LINEAR, ANGULAR, opt("ascii", Bool, "write text (default false: binary)")],
    ),
    C::new("asm.export_3mf", &[req("path", Path, "the file to write (.3mf)"), EXPLODED, LINEAR, ANGULAR]),
    C::new("asm.export_bom", &[req("path", Path, "the file to write (.csv)")]),
];

/// The drawing's file commands, in the order of `drw::commands()`.
static DRAWING: &[C] = &[
    C::new(
        "drw.new",
        &[
            opt("name", Text, "the new drawing's name"),
            opt("standard", Word(&["ansi", "iso"]), "ansi (third-angle, the default) or iso (first-angle)"),
            opt("size", SIZES, "the first sheet's size (default: the standard's)"),
            opt("template", Path, "or a drawing file (.tenondrw) whose standard, properties, sheets and notes it starts with"),
        ],
    ),
    C::new("drw.save_template", &[req("path", Path, "the template to write (.tenondrw)"), opt("name", Text, "the template's name")]),
    C::new("drw.open", &[req("path", Path, "the drawing file to read (.tenondrw)")]),
    C::new("drw.save", &[req("path", Path, "the drawing file to write (.tenondrw)")]),
    C::new(
        "drw.view.base",
        &[
            req("model", Path, "the part (.tenon) or assembly (.tenonasm) file the view shows"),
            opt("orientation", Word(&["front", "back", "top", "bottom", "left", "right", "iso"]), "where it is seen from (default front)"),
            VIEW_SCALE,
            AT,
            opt("hidden", Bool, "draw hidden lines (default true; false for iso)"),
            SHEET,
        ],
    ),
    C::new("drw.update", &[]),
    C::new("drw.export.pdf", &[req("path", Path, "the file to write (.pdf)"), SHEET]),
    C::new("drw.export.svg", &[req("path", Path, "the file to write (.svg)"), SHEET]),
    C::new("drw.export.dxf", &[req("path", Path, "the file to write (.dxf)"), SHEET]),
    C::new("drw.template.save", &[req("path", Path, "the title block template to write (.json)"), SHEET]),
    C::new("drw.template.apply", &[req("path", Path, "a title block template (.json)"), SHEET]),
];

/// The schema of any command there is, whichever registry it is in.
pub fn schema(id: &str) -> Option<&'static C> {
    [PART, ASSEMBLY, DRAWING]
        .into_iter()
        .flatten()
        .find(|s| s.id == id)
        .or_else(|| tenon_model::schemas::schema(id).or_else(|| tenon_assembly::schemas::schema(id)).or_else(|| tenon_drawing::schemas::schema(id)))
}

/// Checks `params` against the schema of the command `id` (`tenon_model::schema::check`): for
/// whoever takes commands from outside. A command there is no schema for is let through; that
/// it does not exist is for running it to say.
pub fn check(id: &str, params: &serde_json::Value) -> Result<(), String> {
    schema(id).map_or(Ok(()), |s| tenon_model::schema::check(s, params))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_command_has_a_schema_and_every_schema_a_command() {
        let ids = |schemas: &[C]| schemas.iter().map(|s| s.id).collect::<Vec<_>>();
        assert_eq!(ids(PART), crate::cmd::commands().iter().map(|c| c.id).collect::<Vec<_>>());
        assert_eq!(ids(ASSEMBLY), crate::asm::commands().iter().map(|c| c.id).collect::<Vec<_>>());
        assert_eq!(ids(DRAWING), crate::drw::commands().iter().map(|c| c.id).collect::<Vec<_>>());
        for table in [PART, ASSEMBLY, DRAWING] {
            assert_eq!(tenon_model::schema::sound(table), Ok(()));
        }
        // Every command of every registry is found from here, and nothing else is.
        for id in
            crate::cmd::all_commands().map(|c| c.id).chain(crate::asm::all_commands().map(|c| c.id)).chain(crate::drw::all_commands().map(|c| c.id))
        {
            assert_eq!(schema(id).map(|s| s.id), Some(id));
        }
        assert!(schema("file.nonsense").is_none());
        assert_eq!(check("file.nonsense", &serde_json::json!({ "anything": 1 })), Ok(()));
        assert_eq!(check("file.save", &serde_json::json!({ "path": "a.tenon" })), Ok(()));
        assert_eq!(
            check("file.save", &serde_json::json!({ "pth": "a.tenon" })).unwrap_err(),
            "file.save has no parameter `pth`: did you mean `path`?"
        );
        assert_eq!(check("sketch.point", &serde_json::json!({ "sketch": 1, "x": 0 })).unwrap_err(), "sketch.point needs `y`: sketch coordinate");
    }
}
