//! The cxx bridge to the C++ shim in `shim/tenon_occt.{h,cpp}`.
//!
//! This is the entire FFI surface of Tenon. Every C++ function that can fail returns `Result`:
//! the shim converts every C++ and OCCT exception into `std::runtime_error`, which cxx turns into
//! `Err(cxx::Exception)`. Nothing unwinds into Rust.
//!
//! Enumerations travel as `u8` codes; `lib.rs` maps them. Sub-shape indices are 0-based.

#[cxx::bridge(namespace = "tenon_occt")]
pub(crate) mod bridge {
    #[derive(Clone, Copy, Debug, Default)]
    struct V3 {
        x: f64,
        y: f64,
        z: f64,
    }

    /// Right-handed frame (`gp_Ax2`): origin, X direction, Z (main) direction.
    #[derive(Clone, Copy, Debug, Default)]
    struct Frame3 {
        origin: V3,
        x_dir: V3,
        z_dir: V3,
    }

    /// Primitive face role (codes as `PrimitiveRole` order) and the face index in the result.
    #[derive(Clone, Copy, Debug)]
    struct RoleFace {
        role: u8,
        face: u32,
    }

    /// Input sub-shape (`kind`: 0 vertex, 1 edge, 2 face) and its images in the result.
    #[derive(Debug)]
    struct ImageEntry {
        input: u32,
        kind: u8,
        index: u32,
        images: Vec<u32>,
    }

    /// One result sub-shape generated from an input sub-shape.
    #[derive(Clone, Copy, Debug)]
    struct GenEntry {
        input: u32,
        kind: u8,
        index: u32,
        gen_kind: u8,
        gen_index: u32,
    }

    #[derive(Debug, Default)]
    struct HistoryOut {
        roles: Vec<RoleFace>,
        images: Vec<ImageEntry>,
        generated: Vec<GenEntry>,
    }

    /// Topology with adjacency in compressed rows: entries of row `i` are
    /// `values[offsets[i]..offsets[i + 1]]`.
    #[derive(Debug, Default)]
    struct TopoOut {
        kind: u8,
        solids: u32,
        shells: u32,
        faces: u32,
        edges: u32,
        vertices: u32,
        face_edge_offsets: Vec<u32>,
        face_edges: Vec<u32>,
        edge_face_offsets: Vec<u32>,
        edge_faces: Vec<u32>,
        edge_vertex_offsets: Vec<u32>,
        edge_vertices: Vec<u32>,
    }

    #[derive(Debug, Default)]
    struct FaceOut {
        kind: u8,
        area: f64,
        centroid: V3,
        origin: V3,
        dir: V3,
        radius: f64,
        radius2: f64,
        angle: f64,
        reversed: bool,
    }

    #[derive(Debug, Default)]
    struct EdgeOut {
        kind: u8,
        length: f64,
        start: V3,
        end: V3,
        origin: V3,
        dir: V3,
        radius: f64,
        degenerate: bool,
    }

    /// Flat mesh. `face_ranges` and `edge_ranges` hold triples (id, first, count); for faces
    /// `first`/`count` index `indices`, for edges they index points in `edge_points`.
    #[derive(Debug, Default)]
    struct MeshOut {
        positions: Vec<f32>,
        normals: Vec<f32>,
        indices: Vec<u32>,
        face_ranges: Vec<u32>,
        edge_points: Vec<f32>,
        edge_ranges: Vec<u32>,
    }

    #[derive(Debug, Default)]
    struct MassOut {
        volume: f64,
        area: f64,
        center: V3,
        /// Row-major 3x3 inertia about the centre of mass, unit density.
        inertia: Vec<f64>,
    }

    #[derive(Debug, Default)]
    struct BoxOut {
        is_void: bool,
        min: V3,
        max: V3,
    }

    unsafe extern "C++" {
        include!("tenon-kernel-occt/shim/tenon_occt.h");

        /// An OCCT shape plus its face/edge/vertex enumerations.
        type Shape;
        /// A list of OCCT shapes (boolean tools, STEP roots).
        type ShapeList;

        fn occt_version() -> Result<String>;

        fn new_shape_list() -> Result<UniquePtr<ShapeList>>;
        fn shape_list_push(list: Pin<&mut ShapeList>, shape: &Shape) -> Result<()>;
        fn shape_list_len(list: &ShapeList) -> usize;
        fn shape_list_get(list: &ShapeList, index: usize) -> Result<UniquePtr<Shape>>;

        fn face_count(shape: &Shape) -> u32;
        fn edge_count(shape: &Shape) -> u32;

        fn make_box(frame: &Frame3, dx: f64, dy: f64, dz: f64, hist: &mut HistoryOut) -> Result<UniquePtr<Shape>>;
        fn make_cylinder(frame: &Frame3, radius: f64, height: f64, hist: &mut HistoryOut) -> Result<UniquePtr<Shape>>;
        fn make_cone(frame: &Frame3, r1: f64, r2: f64, height: f64, hist: &mut HistoryOut) -> Result<UniquePtr<Shape>>;
        fn make_sphere(center: &V3, radius: f64) -> Result<UniquePtr<Shape>>;
        fn make_torus(frame: &Frame3, major_radius: f64, minor_radius: f64) -> Result<UniquePtr<Shape>>;

        /// `op`: 0 union, 1 cut, 2 intersect.
        fn boolean_op(op: u8, target: &Shape, tools: &ShapeList, hist: &mut HistoryOut) -> Result<UniquePtr<Shape>>;

        fn topology(shape: &Shape, out: &mut TopoOut) -> Result<()>;
        fn face_info(shape: &Shape, index: u32, out: &mut FaceOut) -> Result<()>;
        fn edge_info(shape: &Shape, index: u32, out: &mut EdgeOut) -> Result<()>;
        fn tessellate(shape: &Shape, linear: f64, angular: f64, out: &mut MeshOut) -> Result<()>;
        fn mass_properties(shape: &Shape, out: &mut MassOut) -> Result<()>;
        fn bounding_box(shape: &Shape, out: &mut BoxOut) -> Result<()>;
        fn is_valid(shape: &Shape) -> Result<bool>;

        fn export_step(shapes: &ShapeList) -> Result<Vec<u8>>;
        fn import_step(data: &[u8]) -> Result<UniquePtr<ShapeList>>;
    }
}

// SAFETY: a `Shape` exclusively owns its `TopoDS_Shape` (OCCT handles with atomic reference counts)
// and its sub-shape maps; it has no thread affinity. The kernel may move to another thread, but a
// `Shape` is never used from two threads at once: `OcctKernel` is `Send` but not `Sync`, and all
// access goes through `&self`/`&mut self` of the owning kernel.
unsafe impl Send for bridge::Shape {}
