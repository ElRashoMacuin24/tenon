//! What every command of this crate's registry takes ([`crate::schema`]). One entry for each
//! command of `cmd::commands()`; a test holds the two lists together, and every run of a
//! command in a test holds its entry to what the command is given and what it reads.

use crate::schema::Ty::{Angle, Bool, Edge, Either, Face, Id, Int, Length, List, Null, Number, Object, Text, Vector, Word};
use crate::schema::{CommandSchema as C, Param, Ty, opt, req};

const IDS: Ty = List(&Id);
const FACES: Ty = List(&Face);
const EDGES: Ty = List(&Edge);
/// One of the three planes through the part's origin.
const ORIGIN_PLANE: Ty = Word(&["xy", "yz", "xz"]);
/// A plane: an origin plane, a flat face, or a work plane as `{"work": id}`.
const PLANE: Ty = Either(&[ORIGIN_PLANE, Face, Object]);
/// A direction or an axis to turn about: an origin axis, an edge, a round face, or a work axis
/// as `{"work": id}`.
const AXIS: Ty = Either(&[Word(&["x", "y", "z"]), Edge, Face, Object]);
/// A size of the sheet metal rule: millimetres, or an equation.
const SIZE: Ty = Either(&[Length, Text]);
/// A value or the equation that gives it.
const EQUATION: Ty = Either(&[Text, Number]);

const SKETCH: Param = req("sketch", Id, "the sketch: its feature id, as sketch.create returns it");
const ENTITIES: Param = req("entities", IDS, "ids of the sketch's points and curves");
const COPY: Param = opt("copy", Bool, "leave the geometry where it is and make a copy (default false)");
const FEATURE: Param = req("feature", Id, "the feature's id, as model.tree lists it");
const EQUATIONS: Param = opt(
    "equations",
    Object,
    "equations for values of the feature, by where the value is in its definition: {\"/distance\": \"width / 2\"} (lengths in mm, angles in degrees)",
);
const OPERATION: Param = opt("operation", Word(&["join", "cut", "new_body", "intersect"]), "what the feature does to the part (default join)");
const REGIONS: Param = opt(
    "regions",
    List(&Either(&[IDS, Object])),
    "the closed regions of the sketch to use, each as sketch.info lists it under `select`: [curve ids], or {left, right, nth} for one region of a divided sketch (default: every outer region)",
);
const FEATURE_EDGES: Param = req("edges", EDGES, "edge references, from model.edge_ref");
const RADIUS: Param = opt("radius", Length, "the inside bend radius (default: the rule's BendRadius)");
const WIDTH: Param = opt("width", Length, "how wide it is, centred on the edge (default: the whole edge)");
const OFFSETS: Param = opt("offsets", Vector(2), "[from the start of the edge, from its end] in place of `width`");
const SEAM: Param = opt("seam", Word(&["symmetric", "overlap", "reverse_overlap"]), "how the two walls meet (default symmetric)");
const OVERLAP: Param = opt("overlap", Number, "how much of the thickness the overlapping wall covers, 0 to 1 (default 1)");
const GAP: Param = opt("gap", Length, "the gap left between the walls (default: the rule's GapSize)");
const CORNER_RELIEF: Param = opt("relief", Word(&["round", "square", "trim"]), "the corner relief's shape (default: the rule's)");
const RELIEF_SIZE: Param = opt("relief_size", Length, "the corner relief's size (default: the rule's CornerReliefSize)");
const X: Param = req("x", Length, "sketch coordinate");
const Y: Param = req("y", Length, "sketch coordinate");
const NEUTRAL_PLANE: [Param; 3] =
    [opt("plane", ORIGIN_PLANE, "an origin plane"), opt("face", Face, "a flat face"), opt("work_plane", Id, "a work plane's feature id")];

/// A direction in words or numbers.
const WAY: Ty =
    Either(&[Word(&["+x", "-x", "+y", "-y", "+z", "-z", "x", "y", "z", "top", "bottom", "up", "down", "left", "right", "front", "back"]), Vector(3)]);
const STUDY: Param = opt("study", Id, "which study (default: the part's only one)");
const ITEM_NAME: Param = opt("name", Text, "its name (default Fixed1, Force1, Pressure1...)");
const ON_PLANE: Param = opt("plane", ORIGIN_PLANE, "the origin plane its sketch is on (default xy)");
const ON_FACE: Param = opt("face", Face, "or a flat face to sketch on");
const ON_WORK_PLANE: Param = opt("work_plane", Id, "or a work plane's feature id");
const AT_X: Param = opt("x", Length, "where it is in the sketch (default 0)");
const AT_Y: Param = opt("y", Length, "where it is in the sketch (default 0)");

static SCHEMAS: &[C] = &[
    C::new("document.rename", &[req("name", Text, "the part's new name (1 to 200 characters)")]),
    C::new(
        "document.material",
        &[
            opt("name", Either(&[Text, Null]), "a material of the library (see document.materials) or of one's own; null for none"),
            opt("density", Number, "g/cm^3: changes the library's, or describes a material of one's own"),
            opt("color", Either(&[Text, Null]), "\"#rrggbb\""),
            opt("youngs_modulus", Number, "GPa: how stiff it is (changes the library's, or with the two below describes a material of one's own)"),
            opt("poisson", Number, "Poisson's ratio, from 0 to less than 0.5"),
            opt("yield_strength", Number, "MPa: the stress at which it stays bent, or breaks"),
        ],
    ),
    C::new("document.appearance", &[req("color", Either(&[Text, Null]), "\"#rrggbb\", the part's own colour; null for its material's")]),
    C::new("document.materials", &[]),
    C::new("table.show", &[]),
    C::new(
        "table.create",
        &[
            req("columns", List(&Text), "parameter names (plain values, not ones an equation works out)"),
            opt("row", Text, "the name of the first row, which is the part as it stands (default \"Size 1\")"),
        ],
    ),
    C::new("table.delete", &[]),
    C::new(
        "table.add_row",
        &[req("name", Text, "the new row's name"), opt("values", Object, "{parameter: number}; the rest as the active row (mm, degrees)")],
    ),
    C::new("table.remove_row", &[req("name", Text, "the row (not the active one)")]),
    C::new("table.rename_row", &[req("name", Text, "the row"), req("to", Text, "its new name")]),
    C::new(
        "table.set",
        &[req("row", Text, "the row"), req("column", Text, "a parameter name"), req("value", Number, "its value in that row (mm, degrees)")],
    ),
    C::new("table.add_column", &[req("name", Text, "a parameter; every row gets its present value")]),
    C::new("table.remove_column", &[req("name", Text, "the column; the parameter keeps its value")]),
    C::new("table.activate", &[req("row", Text, "the row whose values the part takes")]),
    C::new(
        "sketch.create",
        &[
            opt("plane", ORIGIN_PLANE, "an origin plane (default xy)"),
            opt("face", Face, "a flat face to sketch on"),
            opt("work_plane", Id, "a work plane's feature id"),
            opt("project_origin", Bool, "put a fixed point at the part origin, returned as `origin` (default true)"),
            opt("project_edges", Bool, "on a face: show the face's edges in the sketch as reference geometry (default false)"),
            opt("edges", EDGES, "with `geometry`: the edges to show as reference geometry, when there is no kernel to look them up"),
            opt("geometry", List(&Object), "with `edges`: what each edge is on the sketch plane, one for one, as sketch.project takes it"),
        ],
    ),
    C::new("sketch.point", &[SKETCH, X, Y]),
    C::with(
        "sketch.line",
        &[
            SKETCH,
            opt("start", Id, "an existing point to start at"),
            opt("x1", Length, "where it starts"),
            opt("y1", Length, "where it starts"),
            opt("end", Id, "an existing point to end at"),
            opt("x2", Length, "where it ends"),
            opt("y2", Length, "where it ends"),
        ],
        &[&[&["start"], &["x1", "y1"]], &[&["end"], &["x2", "y2"]]],
    ),
    C::with(
        "sketch.circle",
        &[
            SKETCH,
            opt("center", Id, "an existing point as its centre"),
            opt("cx", Length, "its centre"),
            opt("cy", Length, "its centre"),
            req("r", Length, "its radius"),
        ],
        &[&[&["center"], &["cx", "cy"]]],
    ),
    C::with(
        "sketch.arc",
        &[
            SKETCH,
            opt("center", Id, "an existing point as its centre"),
            opt("cx", Length, "its centre"),
            opt("cy", Length, "its centre"),
            opt("start", Id, "an existing point it starts at"),
            opt("x1", Length, "where it starts"),
            opt("y1", Length, "where it starts"),
            opt("end", Id, "an existing point it ends at, counter-clockwise from the start"),
            opt("x2", Length, "where it ends"),
            opt("y2", Length, "where it ends"),
        ],
        &[&[&["center"], &["cx", "cy"]], &[&["start"], &["x1", "y1"]], &[&["end"], &["x2", "y2"]]],
    ),
    C::new(
        "sketch.arc3",
        &[
            SKETCH,
            req("x1", Length, "where it starts"),
            req("y1", Length, "where it starts"),
            req("x2", Length, "a point on the arc"),
            req("y2", Length, "a point on the arc"),
            req("x3", Length, "where it ends"),
            req("y3", Length, "where it ends"),
        ],
    ),
    C::new(
        "sketch.rectangle",
        &[
            SKETCH,
            req("x1", Length, "one corner"),
            req("y1", Length, "one corner"),
            req("x2", Length, "the opposite corner"),
            req("y2", Length, "the opposite corner"),
        ],
    ),
    C::new(
        "sketch.polygon",
        &[
            SKETCH,
            req("cx", Length, "its centre"),
            req("cy", Length, "its centre"),
            req("x", Length, "one corner"),
            req("y", Length, "one corner"),
            req("sides", Int, "how many sides (3 or more)"),
        ],
    ),
    C::new(
        "sketch.spline",
        &[SKETCH, req("points", List(&Vector(2)), "its control points, [[x, y], ...]"), opt("degree", Int, "its degree (default 3)")],
    ),
    C::new(
        "sketch.constrain",
        &[
            SKETCH,
            req(
                "constraint",
                Object,
                "the constraint: {\"type\": \"horizontal\", \"line\": 3}, {\"type\": \"distance\", \"a\": 1, \"b\": 2, \"value\": 20} and so on (docs/commands.md lists the types)",
            ),
            opt("equation", EQUATION, "for a dimension: the equation that drives it, e.g. \"width / 2\""),
            opt("at_x", Length, "where a dimension's value is shown (default: beside its geometry)"),
            opt("at_y", Length, "where a dimension's value is shown"),
            opt(
                "driven",
                Either(&[Bool, Word(&["auto"])]),
                "true: a dimension that follows the sketch instead of setting it; \"auto\": driven only if a driving one would be one too many",
            ),
        ],
    ),
    C::new(
        "sketch.place_dimension",
        &[
            SKETCH,
            req("constraint", Id, "a dimension's id"),
            req("x", Length, "where its value is shown"),
            req("y", Length, "where its value is shown"),
        ],
    ),
    C::with(
        "sketch.set_dimension",
        &[
            SKETCH,
            req("constraint", Id, "a dimension's id"),
            opt("value", Number, "its new value (mm, or radians for an angle)"),
            opt("equation", EQUATION, "or the equation that gives it (lengths in mm, angles in degrees), e.g. \"d0 / 2\""),
        ],
        &[&[&["value"], &["equation"]]],
    ),
    C::new(
        "sketch.set_driven",
        &[
            SKETCH,
            req("constraint", Id, "a dimension's id"),
            req("driven", Bool, "true: it follows the sketch; false: it holds the sketch at the value it shows"),
        ],
    ),
    C::new("sketch.remove_constraint", &[SKETCH, req("constraint", Id, "the constraint's id")]),
    C::new("sketch.drag", &[SKETCH, req("point", Id, "the point to move"), req("x", Length, "where to"), req("y", Length, "where to")]),
    C::new("sketch.delete", &[SKETCH, ENTITIES]),
    C::new(
        "sketch.construction",
        &[SKETCH, req("entity", Id, "a point's or curve's id"), opt("on", Bool, "make it construction geometry (default true)")],
    ),
    C::new("sketch.centerline", &[SKETCH, req("entity", Id, "a line's id"), opt("on", Bool, "make it a centre line (default true)")]),
    C::new(
        "sketch.project",
        &[
            SKETCH,
            opt("edge", Edge, "one edge to project"),
            opt("edges", EDGES, "the edges to project"),
            opt("face", Face, "a face whose edges are all projected"),
            opt("flat", Bool, "sheet metal: project the edge as it lies in the flat pattern (default false)"),
            opt("geometry", List(&Object), "what each of `edges` is on the sketch plane, one for one, when there is no kernel to work it out"),
        ],
        // (`edge`, `edges` or `face`: the command says so itself, with what each is for.)
    ),
    C::new("sketch.break_link", &[SKETCH, req("entities", IDS, "ids of projected curves or points")]),
    C::new("sketch.fillet", &[SKETCH, req("point", Id, "the corner: a point two lines share"), req("radius", Length, "the fillet's radius")]),
    C::new(
        "sketch.trim",
        &[
            SKETCH,
            req("curve", Id, "the curve to trim"),
            req("x", Length, "a point on the piece to remove"),
            req("y", Length, "a point on the piece to remove"),
        ],
    ),
    C::new(
        "sketch.offset",
        &[SKETCH, req("curves", IDS, "the curves to offset"), req("distance", Length, "how far (positive: outward, or to the left)")],
    ),
    C::new("sketch.mirror", &[SKETCH, ENTITIES, req("axis", Id, "the line to mirror across")]),
    C::new("sketch.move", &[SKETCH, ENTITIES, req("dx", Length, "how far along x"), req("dy", Length, "how far along y"), COPY]),
    C::new(
        "sketch.rotate",
        &[
            SKETCH,
            ENTITIES,
            req("cx", Length, "the point to turn about"),
            req("cy", Length, "the point to turn about"),
            req("angle", Angle, "counter-clockwise"),
            COPY,
        ],
    ),
    C::new(
        "sketch.scale",
        &[
            SKETCH,
            ENTITIES,
            req("cx", Length, "the point that stays"),
            req("cy", Length, "the point that stays"),
            req("factor", Number, "how many times larger"),
            COPY,
        ],
    ),
    C::new(
        "sketch.extend",
        &[SKETCH, req("curve", Id, "a line or an arc"), req("x", Length, "near the end to extend"), req("y", Length, "near the end to extend")],
    ),
    C::new(
        "sketch.split",
        &[
            SKETCH,
            req("curve", Id, "a line, an arc or a circle"),
            req("x", Length, "near the crossing to split at"),
            req("y", Length, "near the crossing to split at"),
        ],
    ),
    C::with(
        "sketch.pattern.rect",
        &[
            SKETCH,
            ENTITIES,
            opt("along", Id, "a line whose direction the copies follow"),
            opt("dx", Length, "or the direction as a vector"),
            opt("dy", Length, "or the direction as a vector"),
            req("count", Int, "how many, the original counted"),
            req("spacing", Length, "the distance from one to the next"),
            opt("count2", Int, "how many in a second direction (default 1)"),
            opt("spacing2", Length, "the spacing in the second direction"),
            opt("along2", Id, "a line for the second direction (default: square to the first)"),
            opt("dx2", Length, "or the second direction as a vector"),
            opt("dy2", Length, "or the second direction as a vector"),
        ],
        // (A direction, by `along` or by `dx` and `dy`: the command asks for it in its own words.)
        &[],
    ),
    C::with(
        "sketch.pattern.circular",
        &[
            SKETCH,
            ENTITIES,
            opt("center", Id, "an existing point to turn about"),
            opt("cx", Length, "the point to turn about"),
            opt("cy", Length, "the point to turn about"),
            req("count", Int, "how many, the original counted"),
            opt("angle", Angle, "from the first to the last (default: a whole turn, divided evenly)"),
        ],
        &[&[&["center"], &["cx", "cy"]]],
    ),
    C::new(
        "sketch.slot",
        &[
            SKETCH,
            req("x1", Length, "the first point"),
            req("y1", Length, "the first point"),
            req("x2", Length, "the second point"),
            req("y2", Length, "the second point"),
            req("width", Length, "how wide the slot is"),
            opt(
                "by",
                Word(&["centers", "overall", "center"]),
                "what the two points are: the centres of the round ends (default), the ends of the slot, or its middle and one centre",
            ),
        ],
    ),
    C::new(
        "sketch.rectangle_center",
        &[SKETCH, req("cx", Length, "its centre"), req("cy", Length, "its centre"), req("x", Length, "a corner"), req("y", Length, "a corner")],
    ),
    C::new(
        "sketch.rectangle3",
        &[
            SKETCH,
            req("x1", Length, "one end of a side"),
            req("y1", Length, "one end of a side"),
            req("x2", Length, "the other end of that side"),
            req("y2", Length, "the other end of that side"),
            req("x3", Length, "a point on the opposite side"),
            req("y3", Length, "a point on the opposite side"),
        ],
    ),
    C::new(
        "sketch.arc_tangent",
        &[
            SKETCH,
            req("from", Id, "the free end point of a line or an arc"),
            req("x", Length, "where the arc ends"),
            req("y", Length, "where the arc ends"),
        ],
    ),
    C::new("sketch.info", &[SKETCH]),
    C::new(
        "model.extrude",
        &[
            SKETCH,
            opt("distance", Length, "how far"),
            opt("backward", Length, "with `distance`: a second distance the other way"),
            opt("symmetric", Length, "the total distance, half each way"),
            opt("through_all", Bool, "through the whole part"),
            opt("reverse", Bool, "the other way from the sketch (default false)"),
            OPERATION,
            REGIONS,
            opt("taper", Angle, "how far the sides lean in as they leave the sketch (negative: out); with one distance or through_all only"),
            EQUATIONS,
        ],
        // (`distance`, `symmetric` or `through_all`: one of them, which the command asks for.)
    ),
    C::new(
        "model.revolve",
        &[
            SKETCH,
            opt(
                "axis",
                Either(&[Id, Word(&["x", "y", "z"]), Object]),
                "a line of the sketch (its id), an origin axis, or a work axis as {\"work\": id} (default: the sketch's centre line)",
            ),
            opt("angle", Angle, "how far round (default: all the way)"),
            opt("symmetric", Bool, "half the angle each way from the sketch (default false)"),
            OPERATION,
            REGIONS,
            EQUATIONS,
        ],
    ),
    C::new("model.fillet", &[FEATURE_EDGES, req("radius", Length, "the fillet's radius"), EQUATIONS]),
    C::new(
        "model.chamfer",
        &[
            FEATURE_EDGES,
            req("distance", Length, "how far back from the edge"),
            opt("distance2", Length, "a second distance, on the other face"),
            opt("angle", Angle, "or the angle the chamfer makes with the face of `reference`"),
            opt("reference", Face, "with `distance2` or `angle`: the face the first distance is measured on"),
            EQUATIONS,
        ],
    ),
    C::new(
        "model.shell",
        &[
            req("thickness", Length, "the wall thickness"),
            opt("remove", FACES, "faces to open (default none)"),
            opt("outside", Bool, "walls grow outwards (default false: inwards)"),
            EQUATIONS,
        ],
    ),
    C::new(
        "model.hole",
        &[
            SKETCH,
            opt("points", IDS, "the points to drill at (default: the sketch's points that lie on no curve)"),
            req("diameter", Length, "the hole's diameter"),
            opt("depth", Length, "how deep"),
            opt("through_all", Bool, "through the whole part"),
            opt("type", Word(&["simple", "counterbore", "countersink"]), "default simple"),
            opt("counterbore_diameter", Length, "for a counterbore"),
            opt("counterbore_depth", Length, "for a counterbore"),
            opt("countersink_diameter", Length, "for a countersink"),
            opt("countersink_angle", Angle, "for a countersink (default 90 degrees)"),
            opt("tip_angle", Angle, "the drill point's angle (default 118 degrees)"),
            opt("flat_bottom", Bool, "no drill point (default false)"),
            opt("reverse", Bool, "drill along the sketch normal instead of against it (default false)"),
            EQUATIONS,
        ],
    ),
    C::new(
        "model.rib",
        &[
            SKETCH,
            opt("lines", IDS, "the lines of the sketch the rib follows (default: its open lines)"),
            req("thickness", Length, "the rib's thickness, half on each side of the sketch plane"),
            opt("distance", Length, "how far it reaches (default: until it meets the part)"),
            opt("flip", Bool, "reach the other way (default false)"),
            EQUATIONS,
        ],
    ),
    C::new(
        "model.sweep",
        &[
            req("sketch", Id, "the sketch of the profile"),
            REGIONS,
            req("path_sketch", Id, "the sketch of the path"),
            opt("path", IDS, "the path's lines and arcs, joined end to end (default: every line and arc of path_sketch that is not construction)"),
            opt("fixed", Bool, "keep the profile's orientation (default false: it turns with the path)"),
            OPERATION,
            EQUATIONS,
        ],
    ),
    C::new(
        "model.coil",
        &[
            req("sketch", Id, "the sketch of the profile, which lies off the axis"),
            REGIONS,
            req(
                "axis",
                Either(&[Id, Word(&["x", "y", "z"]), Object]),
                "an origin axis, a line of the sketch (its id), or a work axis as {\"work\": id}",
            ),
            req("pitch", Length, "the rise in one turn"),
            req("turns", Number, "how many turns"),
            opt("left", Bool, "left-handed (default false)"),
            OPERATION,
            EQUATIONS,
        ],
    ),
    C::new(
        "model.loft",
        &[
            req("sections", IDS, "two or more sketches in order, each with one closed profile"),
            opt("ruled", Bool, "flat sides between the sections (default false: smooth)"),
            OPERATION,
            EQUATIONS,
        ],
    ),
    C::with(
        "model.draft",
        &[
            req("faces", FACES, "the faces to tilt, on one body"),
            NEUTRAL_PLANE[0],
            NEUTRAL_PLANE[1],
            NEUTRAL_PLANE[2],
            req("angle", Angle, "from the plane's normal, which is the pull direction"),
            opt("reverse", Bool, "tilt the other way (default false)"),
            EQUATIONS,
        ],
        // (Which plane: the command asks for it in its own words.)
        &[],
    ),
    C::with(
        "model.split",
        &[
            NEUTRAL_PLANE[0],
            NEUTRAL_PLANE[1],
            NEUTRAL_PLANE[2],
            opt(
                "keep",
                Word(&["both", "front", "back"]),
                "both pieces as two bodies (default), or the one the plane's normal points to, or the other",
            ),
            opt("body", Face, "a face of the one body to split (default: every body the plane passes through)"),
            EQUATIONS,
        ],
        // (Which plane: the command asks for it in its own words.)
        &[],
    ),
    C::new(
        "model.combine",
        &[
            req("base", Face, "a face of the body that stays"),
            req("tools", FACES, "a face of each other body"),
            opt("operation", Word(&["join", "cut", "intersect"]), "default join"),
            opt("keep_tools", Bool, "keep the other bodies (default false: they are used up)"),
            EQUATIONS,
        ],
    ),
    C::new(
        "model.thread",
        &[
            req("face", Face, "a round shaft or hole"),
            opt("pitch", Length, "default: the ISO coarse pitch for the face's diameter, following it"),
            opt("designation", Text, "what drawings call it (default \"M<diameter>x<pitch>\")"),
            opt("length", Length, "from the start end (default: the whole face)"),
            opt("reverse", Bool, "start from the other end (default false)"),
            opt("left", Bool, "left-handed (default false)"),
            opt("modelled", Bool, "cut the groove into the part (default false: a cosmetic thread)"),
            EQUATIONS,
        ],
    ),
    C::new(
        "model.pattern.rect",
        &[
            req("features", IDS, "the features to copy"),
            req("direction", AXIS, "an origin axis, a straight edge, or a work axis as {\"work\": id}"),
            req("count", Int, "how many, the original counted"),
            req("spacing", Length, "the distance from one to the next"),
            opt("reverse", Bool, "the other way (default false)"),
            opt("direction2", AXIS, "a second direction"),
            opt("count2", Int, "how many in the second direction (default 1)"),
            opt("spacing2", Length, "the spacing in the second direction"),
            opt("reverse2", Bool, "the second direction the other way (default false)"),
            EQUATIONS,
        ],
    ),
    C::new(
        "model.pattern.circular",
        &[
            req("features", IDS, "the features to copy"),
            req("axis", AXIS, "an origin axis, a straight or circular edge, a cylinder or cone, or a work axis as {\"work\": id}"),
            req("count", Int, "how many, the original counted"),
            opt("angle", Angle, "over how much of a turn the copies spread evenly (default: all of it)"),
            opt("reverse", Bool, "the other way round (default false)"),
            EQUATIONS,
        ],
    ),
    C::new(
        "model.mirror",
        &[
            req("features", IDS, "the features to mirror"),
            opt("plane", ORIGIN_PLANE, "an origin plane (default xy)"),
            opt("face", Face, "a flat face"),
            opt("work_plane", Id, "a work plane's feature id"),
            EQUATIONS,
        ],
    ),
    C::new(
        "work.plane",
        &[
            opt("by", Word(&["offset", "angle", "midplane"]), "how it is placed (default offset)"),
            opt("base", PLANE, "offset, angle: the plane it starts from"),
            opt("distance", Length, "offset: how far from `base`"),
            opt("axis", AXIS, "angle: the axis in `base` it turns about"),
            opt("angle", Angle, "angle: how far it turns"),
            opt("a", PLANE, "midplane: one of two parallel planes"),
            opt("b", PLANE, "midplane: the other"),
            EQUATIONS,
        ],
    ),
    C::with(
        "work.axis",
        &[
            opt("axis", AXIS, "an origin axis, an edge, a round face, or a work axis as {\"work\": id}"),
            opt("a", PLANE, "or one of two planes it lies on"),
            opt("b", PLANE, "and the other"),
            EQUATIONS,
        ],
        &[&[&["axis"], &["a", "b"]]],
    ),
    C::with(
        "work.point",
        &[
            opt("edge", Edge, "a circular edge: the point is its centre"),
            opt("axis", AXIS, "or an axis"),
            opt("plane", PLANE, "and the plane it meets"),
            EQUATIONS,
        ],
        &[&[&["edge"], &["axis", "plane"]]],
    ),
    C::new("feature.add", &[req("kind", Object, "a feature's definition, as model.tree lists it and the file format has it"), EQUATIONS]),
    C::new("param.list", &[]),
    C::new(
        "param.add",
        &[
            req("name", Text, "the new parameter's name"),
            req("equation", EQUATION, "a value or an equation, e.g. \"12 mm\" or \"width / 2\""),
            opt("unit", Word(&["mm", "deg", "ul"]), "what it measures (default mm; ul: no unit)"),
            opt("comment", Text, "what it is for"),
        ],
    ),
    C::new(
        "param.set",
        &[
            req("name", Text, "the parameter: one of the user's, or a model value such as d0"),
            opt("equation", Either(&[Text, Number, Null]), "its value or equation; null or \"\" removes the equation"),
            opt("comment", Text, "what it is for"),
        ],
    ),
    C::new("param.rename", &[req("name", Text, "the parameter"), req("to", Text, "its new name; equations that use it follow")]),
    C::new("param.delete", &[req("name", Text, "a user parameter that no equation uses")]),
    C::new("feature.update", &[FEATURE, req("kind", Object, "the feature's whole definition, as model.tree lists it"), EQUATIONS]),
    C::new("feature.rename", &[FEATURE, req("name", Text, "its new name")]),
    C::new("feature.suppress", &[FEATURE, opt("suppressed", Bool, "true to suppress (default), false to bring it back")]),
    C::new("feature.delete", &[FEATURE]),
    C::new("feature.end_of_part", &[opt("before", Id, "the feature the marker goes above (default: the end, with every feature in)")]),
    C::new("feature.move", &[FEATURE, opt("before", Id, "the feature it goes above (default: to the end)")]),
    C::new("edit.undo", &[]),
    C::new("edit.redo", &[]),
    C::new("model.tree", &[]),
    C::new("model.regenerate", &[]),
    C::new("model.mass", &[opt("density", Number, "g/cm^3 (default: the part's material's, or 1 without one)")]),
    C::new("model.threads", &[]),
    C::new("model.topology", &[]),
    C::new("model.faces", &[opt("body", Int, "which body (default 0)")]),
    C::new("model.edges", &[opt("body", Int, "which body (default 0)")]),
    C::new("model.work", &[]),
    C::new(
        "model.measure",
        &[
            req(
                "a",
                Object,
                "what to measure: {\"face\": n} or {\"edge\": n} (numbered as model.faces and model.edges list them, with \"body\": b for a body other than 0), or a face or edge reference under the same key",
            ),
            opt("b", Object, "a second face or edge: the distance and angle between the two"),
        ],
    ),
    C::new("model.broken", &[]),
    C::new(
        "model.repair",
        &[
            FEATURE,
            req("path", Text, "which of the feature's references, as model.broken lists it"),
            req(
                "with",
                Either(&[Face, Edge]),
                "the face or edge it should refer to (from model.face_ref or model.edge_ref, or a candidate of model.broken)",
            ),
        ],
    ),
    C::new(
        "sheet.defaults",
        &[
            opt("thickness", SIZE, "the sheet's thickness"),
            opt("bend_radius", SIZE, "the inside radius of bends"),
            opt("relief", Word(&["straight", "round"]), "the shape of the cut beside a bend narrower than its edge"),
            opt("relief_width", SIZE, "that cut's width"),
            opt("relief_depth", SIZE, "how far it goes past the bend"),
            opt("min_remnant", SIZE, "less sheet than this left beside a relief is cut away too"),
            opt("corner", Word(&["round", "square", "trim"]), "the shape of the cut where two bends meet"),
            opt("corner_size", SIZE, "that cut's size"),
            opt("gap", SIZE, "the gap left where walls meet (Miter Gap)"),
            opt("k_factor", Either(&[Number, Text]), "where in the thickness a bend keeps its length, 0 to 1"),
        ],
    ),
    C::new("sheet.info", &[]),
    C::new(
        "sheet.face",
        &[
            SKETCH,
            REGIONS,
            opt("flip", Bool, "give the sheet its thickness on the other side of the sketch (default false)"),
            opt("operation", Word(&["join", "new_body"]), "default join"),
            EQUATIONS,
        ],
    ),
    C::new(
        "sheet.flange",
        &[
            req("edges", EDGES, "the edges of the sheet to bend walls up from"),
            req("height", Length, "the wall's height, measured from `datum`"),
            opt("angle", Angle, "how far the wall is bent up (default a right angle)"),
            opt("datum", Word(&["outer", "inner", "tangent"]), "what the height is measured from (default outer: over all)"),
            opt("position", Word(&["inside", "adjacent", "outside", "tangent"]), "where the bend lies relative to the edge (default inside)"),
            opt("flip", Bool, "bend to the other side of the sheet (default false)"),
            RADIUS,
            opt("miter", Bool, "mitre walls of this feature where they meet (default true)"),
            WIDTH,
            OFFSETS,
            SEAM,
            OVERLAP,
            GAP,
            CORNER_RELIEF,
            RELIEF_SIZE,
            EQUATIONS,
        ],
    ),
    C::new(
        "sheet.hem",
        &[
            req("edges", EDGES, "the edges to fold back"),
            opt("type", Word(&["single", "double", "rolled", "teardrop"]), "default single"),
            opt("gap", Length, "single, double: the gap inside the fold (default half the thickness)"),
            opt("length", Length, "single, double: how long the hem is, over all (default four times the thickness)"),
            opt("radius", Length, "rolled, teardrop: the inside radius (default: the rule's BendRadius)"),
            opt("angle", Angle, "rolled, teardrop: how far round (default 270 and 190 degrees)"),
            opt("flip", Bool, "fold to the other side (default false)"),
            WIDTH,
            OFFSETS,
            EQUATIONS,
        ],
    ),
    C::new(
        "sheet.contour_flange",
        &[
            req("sketch", Id, "the sketch of the open profile: lines and arcs end to end"),
            opt("distance", Length, "how wide the strip is, when it is not along `edges`"),
            opt("direction", Word(&["default", "flipped", "symmetric"]), "which way from the sketch the strip is pushed"),
            opt("position", Word(&["default", "flipped", "symmetric"]), "which side of the profile the sheet's thickness is on"),
            opt("operation", Word(&["join", "new_body"]), "default join"),
            opt("edges", EDGES, "edges of the sheet to run the profile along, the first of them where the profile is drawn"),
            RADIUS,
            WIDTH,
            OFFSETS,
            EQUATIONS,
        ],
    ),
    C::new(
        "sheet.corner_seam",
        &[req("edges", EDGES, "an edge of each of the two walls that meet"), SEAM, OVERLAP, GAP, CORNER_RELIEF, RELIEF_SIZE, EQUATIONS],
    ),
    C::new(
        "sheet.corner_rip",
        &[
            req("edges", EDGES, "the corner's edges: outside or inside edges where two walls meet square"),
            SEAM,
            OVERLAP,
            GAP,
            CORNER_RELIEF,
            RELIEF_SIZE,
            EQUATIONS,
        ],
    ),
    C::new("sheet.cut_across_bend", &[SKETCH, REGIONS, opt("distance", Length, "how deep (default: through the sheet)"), EQUATIONS]),
    C::new("sheet.cut_normal", &[SKETCH, REGIONS, opt("distance", Length, "how deep (default: through the sheet)"), EQUATIONS]),
    C::new(
        "sheet.bend",
        &[
            req("edges", EDGES, "two edges: one of each of the two faces to join"),
            RADIUS,
            opt(
                "double",
                Word(&["fix_edges", "45_degree", "full_radius", "90_degree"]),
                "for faces in parallel planes: how the two bends between them are made (default fix_edges)",
            ),
            opt("flip_fixed", Bool, "which of the two faces is trimmed or extended (default false: the second)"),
            EQUATIONS,
        ],
    ),
    C::new("sheet.sharp_bend", &[req("edges", EDGES, "sharp edges where two faces of the sheet meet"), RADIUS, EQUATIONS]),
    C::new(
        "sheet.fold",
        &[
            req("sketch", Id, "the sketch with the line to fold along, drawn across a face from edge to edge"),
            opt("line", Id, "which line (default: the sketch's only one)"),
            opt("angle", Angle, "how far to fold (default a right angle)"),
            RADIUS,
            opt("location", Word(&["centerline", "start", "end"]), "where the bend lies relative to the line (default centerline)"),
            opt("flip_side", Bool, "fold the other side of the line (default false)"),
            opt("flip_direction", Bool, "fold the other way (default false)"),
            EQUATIONS,
        ],
    ),
    C::new("sheet.a_side", &[opt("face", Either(&[Face, Null]), "the face that lies up in the flat pattern; null for the part's own choice")]),
    C::new("sheet.flat_pattern", &[opt("on", Bool, "keep the flat pattern with the part (default true)")]),
    C::new(
        "sheet.flat",
        &[
            opt("body", Int, "which body (default 0)"),
            opt("face", Face, "the face to lay flat from (default: the A side)"),
            opt("curves", Bool, "also give the outline's curves (default false)"),
        ],
    ),
    C::with(
        "model.edge_ref",
        &[
            opt("faces", List(&Either(&[Face, Object])), "the two faces the edge lies between: face references, or their names"),
            opt("at", Vector(3), "or a point [x, y, z] on the edge"),
            opt("body", Int, "with `edge`: which body (default 0)"),
            opt("edge", Int, "or the edge's number, as model.edges lists it"),
        ],
        &[&[&["faces"], &["at"], &["edge"]]],
    ),
    C::with(
        "model.face_ref",
        &[
            opt(
                "origin",
                Object,
                "the face's name: {\"type\": \"cap\", \"feature\": id, \"end\": \"start\" | \"end\"}, {\"type\": \"side\", \"feature\": id, \"curve\": sketch curve id} and so on, as model.faces lists them",
            ),
            opt("at", Vector(3), "or a point [x, y, z] on the face"),
            opt("body", Int, "with `face`: which body (default 0)"),
            opt("face", Int, "or the face's number, as model.faces lists it"),
        ],
        &[&[&["origin"], &["at"], &["face"]]],
    ),
    C::new(
        "model.find",
        &[
            req("what", Word(&["faces", "edges"]), "which to look for"),
            opt("body", Int, "only on this body (default: every body)"),
            opt(
                "kind",
                Word(&["plane", "cylinder", "cone", "sphere", "torus", "line", "circle", "arc", "other"]),
                "what it is: plane, cylinder, cone, sphere, torus or other for faces; line, circle (a whole one), arc or other for edges",
            ),
            opt("facing", WAY, "flat faces that look this way: top, bottom, left, right, front, back, +x ... -z, or [x, y, z]"),
            opt("along", WAY, "straight edges that run this way, and round faces and edges whose axis does (either way along it)"),
            opt("feature", Id, "made by this feature"),
            opt("hole", Bool, "round faces only: true for the walls of holes, false for shafts"),
            opt("radius", Length, "round faces and edges of this radius"),
            opt(
                "most",
                Either(&[Word(&["largest", "smallest"]), WAY]),
                "of what is found, only the furthest this way (top, -x, [x, y, z] ...), or the largest or smallest; those level with it count too",
            ),
        ],
    ),
    C::new("study.new", &[opt("name", Text, "its name (default Study1, Study2...)")]),
    C::new("study.rename", &[STUDY, req("name", Text, "its new name (1 to 64 characters)")]),
    C::new("study.delete", &[STUDY]),
    C::new("study.fix", &[STUDY, req("faces", List(&Face), "the faces that are held still"), ITEM_NAME]),
    C::new(
        "study.force",
        &[
            STUDY,
            req("faces", List(&Face), "the faces the force is spread over"),
            req("newtons", Number, "the whole force, N (negative: the other way)"),
            opt(
                "direction",
                WAY,
                "the way it pushes: +x ... -z, top, bottom, left, right, front, back or [x, y, z]; default square to each face, into it",
            ),
            ITEM_NAME,
        ],
    ),
    C::new(
        "study.pressure",
        &[
            STUDY,
            req("faces", List(&Face), "the faces it is on"),
            req("megapascals", Number, "N/mm^2, into the faces (negative: pulling)"),
            ITEM_NAME,
        ],
    ),
    C::new("study.remove", &[STUDY, req("item", Id, "the id of a support or a load of the study")]),
    C::new("study.mesh_size", &[STUDY, opt("size", Either(&[Length, Null]), "the size of an element, mm; null: chosen from the part's size")]),
    C::new("study.list", &[]),
    C::new(
        "model.box",
        &[
            ON_PLANE,
            ON_FACE,
            ON_WORK_PLANE,
            AT_X,
            AT_Y,
            opt("centered", Bool, "x, y is the middle of the box (default false: a corner)"),
            req("width", Length, "along the sketch's x"),
            req("depth", Length, "along the sketch's y"),
            req("height", Length, "out of the sketch"),
            OPERATION,
            opt("reverse", Bool, "the other way from the sketch (default false)"),
        ],
    ),
    C::new(
        "model.cylinder",
        &[
            ON_PLANE,
            ON_FACE,
            ON_WORK_PLANE,
            AT_X,
            AT_Y,
            req("diameter", Length, "its diameter"),
            req("height", Length, "out of the sketch"),
            OPERATION,
            opt("reverse", Bool, "the other way from the sketch (default false)"),
        ],
    ),
    C::new("model.sphere", &[ON_PLANE, ON_FACE, ON_WORK_PLANE, AT_X, AT_Y, req("diameter", Length, "its diameter"), OPERATION]),
    C::new(
        "model.torus",
        &[
            ON_PLANE,
            ON_FACE,
            ON_WORK_PLANE,
            AT_X,
            AT_Y,
            req("diameter", Length, "across the ring, from the middle of its section to the middle opposite"),
            req("section", Length, "how thick the ring is"),
            OPERATION,
        ],
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
        assert_eq!(crate::schema::sound(SCHEMAS), Ok(()));
    }
}
