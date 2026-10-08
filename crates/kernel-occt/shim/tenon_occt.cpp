// Tenon OCCT shim implementation. See tenon_occt.h.
//
// Original Tenon code (MIT OR Apache-2.0), written against OCCT's public headers.

#include "tenon-kernel-occt/shim/tenon_occt.h"
#include "tenon-kernel-occt/src/ffi.rs.h"

#include <algorithm>
#include <cmath>
#include <mutex>
#include <sstream>
#include <stdexcept>
#include <string>
#include <utility>

#include <BRepAdaptor_Curve.hxx>
#include <BRepAdaptor_Surface.hxx>
#include <BRepAlgoAPI_BooleanOperation.hxx>
#include <BRepAlgoAPI_Common.hxx>
#include <BRepAlgoAPI_Cut.hxx>
#include <BRepAlgoAPI_Fuse.hxx>
#include <BRepBndLib.hxx>
#include <BRepBuilderAPI_MakeShape.hxx>
#include <BRepCheck_Analyzer.hxx>
#include <BRepGProp.hxx>
#include <BRepLib_ToolTriangulatedShape.hxx>
#include <BRepMesh_IncrementalMesh.hxx>
#include <BRepPrimAPI_MakeBox.hxx>
#include <BRepPrimAPI_MakeCone.hxx>
#include <BRepPrimAPI_MakeCylinder.hxx>
#include <BRepPrimAPI_MakeSphere.hxx>
#include <BRepPrimAPI_MakeTorus.hxx>
#include <BRepPrim_Cone.hxx>
#include <BRepPrim_Cylinder.hxx>
#include <BRep_Tool.hxx>
#include <Bnd_Box.hxx>
#include <GCPnts_TangentialDeflection.hxx>
#include <GProp_GProps.hxx>
#include <IFSelect_ReturnStatus.hxx>
#include <Message.hxx>
#include <Message_Messenger.hxx>
#include <NCollection_IndexedDataMap.hxx>
#include <NCollection_List.hxx>
#include <Poly_Triangulation.hxx>
#include <STEPControl_Reader.hxx>
#include <STEPControl_Writer.hxx>
#include <Standard_Failure.hxx>
#include <Standard_Version.hxx>
#include <TopExp.hxx>
#include <TopLoc_Location.hxx>
#include <TopoDS.hxx>
#include <TopoDS_Edge.hxx>
#include <TopoDS_Face.hxx>
#include <TopoDS_Iterator.hxx>
#include <TopoDS_Vertex.hxx>
#include <gp_Ax2.hxx>
#include <gp_Circ.hxx>
#include <gp_Cone.hxx>
#include <gp_Cylinder.hxx>
#include <gp_Dir.hxx>
#include <gp_Lin.hxx>
#include <gp_Mat.hxx>
#include <gp_Pln.hxx>
#include <gp_Pnt.hxx>
#include <gp_Sphere.hxx>
#include <gp_Torus.hxx>
#include <gp_Trsf.hxx>

namespace tenon_occt {

namespace {

// Role codes, in the order of tenon_kernel::PrimitiveRole.
enum : std::uint8_t {
  ROLE_BOX_XMIN = 0,
  ROLE_BOX_XMAX = 1,
  ROLE_BOX_YMIN = 2,
  ROLE_BOX_YMAX = 3,
  ROLE_BOX_ZMIN = 4,
  ROLE_BOX_ZMAX = 5,
  ROLE_LATERAL = 6,
  ROLE_BOTTOM = 7,
  ROLE_TOP = 8,
};

// Sub-shape kind codes, in the order of tenon_kernel::TopoKind.
enum : std::uint8_t { KIND_VERTEX = 0, KIND_EDGE = 1, KIND_FACE = 2 };

std::string describe(const Standard_Failure& e) {
  const char* type_name = e.ExceptionType();
  const std::string type = type_name != nullptr ? type_name : "Standard_Failure";
  const char* msg = e.what();
  if (msg != nullptr && *msg != '\0') {
    return type + ": " + msg;
  }
  return type;
}

// OCCT's default messenger prints progress and statistics (for example STEP transfer reports) to
// stdout, which would corrupt machine-readable CLI output. Tenon reports through its own errors.
void silence_occt_messages() {
  static std::once_flag once;
  std::call_once(once, [] { Message::DefaultMessenger()->ChangePrinters().Clear(); });
}

// Runs `f`; converts every exception into std::runtime_error prefixed with `op`, so cxx
// reports it as Err(cxx::Exception) and nothing else can unwind into Rust.
template <class F>
auto guarded(const char* op, F&& f) -> decltype(f()) {
  try {
    silence_occt_messages();
    return f();
  } catch (const Standard_Failure& e) {
    throw std::runtime_error(std::string(op) + ": " + describe(e));
  } catch (const std::exception& e) {
    throw std::runtime_error(std::string(op) + ": " + e.what());
  } catch (...) {
    throw std::runtime_error(std::string(op) + ": unknown C++ exception");
  }
}

gp_Pnt pnt(const V3& v) { return gp_Pnt(v.x, v.y, v.z); }
gp_Dir dir(const V3& v) { return gp_Dir(v.x, v.y, v.z); }
gp_Ax2 ax2(const Frame3& f) { return gp_Ax2(pnt(f.origin), dir(f.z_dir), dir(f.x_dir)); }
V3 v3(const gp_XYZ& p) { return V3{p.X(), p.Y(), p.Z()}; }
V3 v3(const gp_Pnt& p) { return v3(p.XYZ()); }
V3 v3(const gp_Dir& d) { return v3(d.XYZ()); }

// Booleans return a compound even when the result is one solid; downstream features want the
// solid itself. Sub-shape enumerations are identical either way.
TopoDS_Shape single_solid(const TopoDS_Shape& s) {
  if (s.IsNull() || s.ShapeType() != TopAbs_COMPOUND) {
    return s;
  }
  TopoDS_Iterator it(s);
  if (!it.More()) {
    return s;
  }
  const TopoDS_Shape first = it.Value();
  it.Next();
  return (!it.More() && first.ShapeType() == TopAbs_SOLID) ? first : s;
}

std::unique_ptr<Shape> wrap(const TopoDS_Shape& s) {
  if (s.IsNull()) {
    throw std::runtime_error("the operation produced an empty shape");
  }
  return std::make_unique<Shape>(s);
}

void push_index(rust::Vec<std::uint32_t>& out, const ShapeMap& map, const TopoDS_Shape& s) {
  const int k = map.FindIndex(s);
  if (k > 0) {
    out.push_back(static_cast<std::uint32_t>(k - 1));
  }
}

void add_role(HistoryOut& hist, std::uint8_t role, const Shape& result, const TopoDS_Shape& face) {
  const int k = result.faces.FindIndex(face);
  if (k > 0) {
    hist.roles.push_back(RoleFace{role, static_cast<std::uint32_t>(k - 1)});
  }
}

// Locates a result sub-shape: (kind, 0-based index), or false if it is not in the result.
bool locate(const Shape& result, const TopoDS_Shape& s, std::uint8_t& kind, std::uint32_t& index) {
  const std::pair<std::uint8_t, const ShapeMap*> maps[] = {
      {KIND_FACE, &result.faces}, {KIND_EDGE, &result.edges}, {KIND_VERTEX, &result.vertices}};
  for (const auto& m : maps) {
    const int k = m.second->FindIndex(s);
    if (k > 0) {
      kind = m.first;
      index = static_cast<std::uint32_t>(k - 1);
      return true;
    }
  }
  return false;
}

// Records, for every face and edge of one input, where it went in `result`.
void record_history(BRepBuilderAPI_MakeShape& algo, std::uint32_t input, const TopoDS_Shape& input_shape,
                    const Shape& result, HistoryOut& hist) {
  const std::pair<std::uint8_t, TopAbs_ShapeEnum> kinds[] = {{KIND_FACE, TopAbs_FACE}, {KIND_EDGE, TopAbs_EDGE}};
  for (const auto& kind : kinds) {
    ShapeMap map;
    TopExp::MapShapes(input_shape, kind.second, map);
    const ShapeMap& result_map = kind.first == KIND_FACE ? result.faces : result.edges;
    for (int i = 1; i <= map.Extent(); ++i) {
      const TopoDS_Shape& s = map(i);
      ImageEntry entry;
      entry.input = input;
      entry.kind = kind.first;
      entry.index = static_cast<std::uint32_t>(i - 1);
      if (!algo.IsDeleted(s)) {
        const NCollection_List<TopoDS_Shape>& modified = algo.Modified(s);
        if (modified.IsEmpty()) {
          push_index(entry.images, result_map, s);
        } else {
          for (const TopoDS_Shape& m : modified) {
            push_index(entry.images, result_map, m);
          }
        }
      }
      hist.images.push_back(std::move(entry));
      for (const TopoDS_Shape& g : algo.Generated(s)) {
        GenEntry gen{input, kind.first, static_cast<std::uint32_t>(i - 1), 0, 0};
        if (locate(result, g, gen.gen_kind, gen.gen_index)) {
          hist.generated.push_back(gen);
        }
      }
    }
  }
}

// CSR row helper: appends a row and closes it in `offsets`.
template <class Row>
void push_row(rust::Vec<std::uint32_t>& offsets, rust::Vec<std::uint32_t>& values, const Row& row) {
  for (std::uint32_t v : row) {
    values.push_back(v);
  }
  offsets.push_back(static_cast<std::uint32_t>(values.size()));
}

void push3(rust::Vec<float>& out, double x, double y, double z) {
  out.push_back(static_cast<float>(x));
  out.push_back(static_cast<float>(y));
  out.push_back(static_cast<float>(z));
}

} // namespace

Shape::Shape(const TopoDS_Shape& s) : shape(s) {
  TopExp::MapShapes(shape, TopAbs_FACE, faces);
  TopExp::MapShapes(shape, TopAbs_EDGE, edges);
  TopExp::MapShapes(shape, TopAbs_VERTEX, vertices);
}

rust::String occt_version() {
  return guarded("occt_version", [] { return rust::String(OCC_VERSION_COMPLETE); });
}

std::unique_ptr<ShapeList> new_shape_list() {
  return guarded("new_shape_list", [] { return std::make_unique<ShapeList>(); });
}

void shape_list_push(ShapeList& list, const Shape& shape) {
  guarded("shape_list_push", [&] { list.items.push_back(shape.shape); });
}

std::size_t shape_list_len(const ShapeList& list) noexcept { return list.items.size(); }

std::unique_ptr<Shape> shape_list_get(const ShapeList& list, std::size_t index) {
  return guarded("shape_list_get", [&] {
    if (index >= list.items.size()) {
      throw std::out_of_range("shape list index out of range");
    }
    return wrap(list.items[index]);
  });
}

std::uint32_t face_count(const Shape& shape) noexcept { return static_cast<std::uint32_t>(shape.faces.Extent()); }
std::uint32_t edge_count(const Shape& shape) noexcept { return static_cast<std::uint32_t>(shape.edges.Extent()); }

std::unique_ptr<Shape> make_box(const Frame3& frame, double dx, double dy, double dz, HistoryOut& hist) {
  return guarded("make_box", [&] {
    BRepPrimAPI_MakeBox mk(ax2(frame), dx, dy, dz);
    mk.Build();
    if (!mk.IsDone()) {
      throw std::runtime_error("box construction did not complete");
    }
    auto out = wrap(mk.Shape());
    add_role(hist, ROLE_BOX_XMIN, *out, mk.BackFace());
    add_role(hist, ROLE_BOX_XMAX, *out, mk.FrontFace());
    add_role(hist, ROLE_BOX_YMIN, *out, mk.LeftFace());
    add_role(hist, ROLE_BOX_YMAX, *out, mk.RightFace());
    add_role(hist, ROLE_BOX_ZMIN, *out, mk.BottomFace());
    add_role(hist, ROLE_BOX_ZMAX, *out, mk.TopFace());
    return out;
  });
}

std::unique_ptr<Shape> make_cylinder(const Frame3& frame, double radius, double height, HistoryOut& hist) {
  return guarded("make_cylinder", [&] {
    BRepPrimAPI_MakeCylinder mk(ax2(frame), radius, height);
    mk.Build();
    if (!mk.IsDone()) {
      throw std::runtime_error("cylinder construction did not complete");
    }
    auto out = wrap(mk.Shape());
    BRepPrim_Cylinder& prim = mk.Cylinder();
    add_role(hist, ROLE_LATERAL, *out, prim.LateralFace());
    add_role(hist, ROLE_BOTTOM, *out, prim.BottomFace());
    add_role(hist, ROLE_TOP, *out, prim.TopFace());
    return out;
  });
}

std::unique_ptr<Shape> make_cone(const Frame3& frame, double r1, double r2, double height, HistoryOut& hist) {
  return guarded("make_cone", [&] {
    BRepPrimAPI_MakeCone mk(ax2(frame), r1, r2, height);
    mk.Build();
    if (!mk.IsDone()) {
      throw std::runtime_error("cone construction did not complete");
    }
    auto out = wrap(mk.Shape());
    BRepPrim_Cone& prim = mk.Cone();
    add_role(hist, ROLE_LATERAL, *out, prim.LateralFace());
    if (prim.HasBottom()) {
      add_role(hist, ROLE_BOTTOM, *out, prim.BottomFace());
    }
    if (prim.HasTop()) {
      add_role(hist, ROLE_TOP, *out, prim.TopFace());
    }
    return out;
  });
}

std::unique_ptr<Shape> make_sphere(const V3& center, double radius) {
  return guarded("make_sphere", [&] {
    BRepPrimAPI_MakeSphere mk(pnt(center), radius);
    mk.Build();
    if (!mk.IsDone()) {
      throw std::runtime_error("sphere construction did not complete");
    }
    return wrap(mk.Shape());
  });
}

std::unique_ptr<Shape> make_torus(const Frame3& frame, double major_radius, double minor_radius) {
  return guarded("make_torus", [&] {
    BRepPrimAPI_MakeTorus mk(ax2(frame), major_radius, minor_radius);
    mk.Build();
    if (!mk.IsDone()) {
      throw std::runtime_error("torus construction did not complete");
    }
    return wrap(mk.Shape());
  });
}

std::unique_ptr<Shape> boolean_op(std::uint8_t op, const Shape& target, const ShapeList& tools, HistoryOut& hist) {
  return guarded("boolean", [&] {
    if (tools.items.empty()) {
      throw std::invalid_argument("a boolean needs at least one tool");
    }
    std::unique_ptr<BRepAlgoAPI_BooleanOperation> algo;
    switch (op) {
    case 0:
      algo = std::make_unique<BRepAlgoAPI_Fuse>();
      break;
    case 1:
      algo = std::make_unique<BRepAlgoAPI_Cut>();
      break;
    case 2:
      algo = std::make_unique<BRepAlgoAPI_Common>();
      break;
    default:
      throw std::invalid_argument("unknown boolean operation code");
    }
    NCollection_List<TopoDS_Shape> args;
    args.Append(target.shape);
    NCollection_List<TopoDS_Shape> tool_list;
    for (const TopoDS_Shape& t : tools.items) {
      tool_list.Append(t);
    }
    algo->SetArguments(args);
    algo->SetTools(tool_list);
    algo->SetRunParallel(false);
    algo->Build();
    if (algo->HasErrors()) {
      std::ostringstream report;
      algo->DumpErrors(report);
      throw std::runtime_error("boolean failed: " + report.str());
    }
    if (!algo->IsDone()) {
      throw std::runtime_error("boolean did not complete");
    }
    auto out = wrap(single_solid(algo->Shape()));
    record_history(*algo, 0, target.shape, *out, hist);
    for (std::size_t i = 0; i < tools.items.size(); ++i) {
      record_history(*algo, static_cast<std::uint32_t>(i + 1), tools.items[i], *out, hist);
    }
    return out;
  });
}

void topology(const Shape& s, TopoOut& out) {
  guarded("topology", [&] {
    out.kind = static_cast<std::uint8_t>(s.shape.ShapeType());
    ShapeMap solids;
    ShapeMap shells;
    TopExp::MapShapes(s.shape, TopAbs_SOLID, solids);
    TopExp::MapShapes(s.shape, TopAbs_SHELL, shells);
    out.solids = static_cast<std::uint32_t>(solids.Extent());
    out.shells = static_cast<std::uint32_t>(shells.Extent());
    out.faces = static_cast<std::uint32_t>(s.faces.Extent());
    out.edges = static_cast<std::uint32_t>(s.edges.Extent());
    out.vertices = static_cast<std::uint32_t>(s.vertices.Extent());

    out.face_edge_offsets.push_back(0);
    for (int f = 1; f <= s.faces.Extent(); ++f) {
      ShapeMap fe;
      TopExp::MapShapes(s.faces(f), TopAbs_EDGE, fe);
      std::vector<std::uint32_t> row;
      for (int j = 1; j <= fe.Extent(); ++j) {
        const int k = s.edges.FindIndex(fe(j));
        if (k > 0) {
          row.push_back(static_cast<std::uint32_t>(k - 1));
        }
      }
      push_row(out.face_edge_offsets, out.face_edges, row);
    }

    NCollection_IndexedDataMap<TopoDS_Shape, NCollection_List<TopoDS_Shape>, TopTools_ShapeMapHasher> ancestors;
    TopExp::MapShapesAndAncestors(s.shape, TopAbs_EDGE, TopAbs_FACE, ancestors);
    out.edge_face_offsets.push_back(0);
    out.edge_vertex_offsets.push_back(0);
    for (int e = 1; e <= s.edges.Extent(); ++e) {
      const TopoDS_Shape& edge = s.edges(e);
      std::vector<std::uint32_t> faces;
      if (ancestors.Contains(edge)) {
        for (const TopoDS_Shape& f : ancestors.FindFromKey(edge)) {
          const int k = s.faces.FindIndex(f);
          if (k > 0 && std::find(faces.begin(), faces.end(), static_cast<std::uint32_t>(k - 1)) == faces.end()) {
            faces.push_back(static_cast<std::uint32_t>(k - 1));
          }
        }
      }
      push_row(out.edge_face_offsets, out.edge_faces, faces);

      TopoDS_Vertex v1;
      TopoDS_Vertex v2;
      TopExp::Vertices(TopoDS::Edge(edge), v1, v2);
      std::vector<std::uint32_t> verts;
      for (const TopoDS_Vertex* v : {&v1, &v2}) {
        if (!v->IsNull()) {
          const int k = s.vertices.FindIndex(*v);
          if (k > 0 && std::find(verts.begin(), verts.end(), static_cast<std::uint32_t>(k - 1)) == verts.end()) {
            verts.push_back(static_cast<std::uint32_t>(k - 1));
          }
        }
      }
      push_row(out.edge_vertex_offsets, out.edge_vertices, verts);
    }
  });
}

void face_info(const Shape& s, std::uint32_t index, FaceOut& out) {
  guarded("face_info", [&] {
    if (index >= static_cast<std::uint32_t>(s.faces.Extent())) {
      throw std::out_of_range("face index out of range");
    }
    const TopoDS_Face& face = TopoDS::Face(s.faces(static_cast<int>(index) + 1));
    out.reversed = face.Orientation() == TopAbs_REVERSED;
    GProp_GProps props;
    BRepGProp::SurfaceProperties(face, props);
    out.area = props.Mass();
    out.centroid = v3(props.CentreOfMass());

    BRepAdaptor_Surface surf(face, true);
    switch (surf.GetType()) {
    case GeomAbs_Plane: {
      const gp_Pln pl = surf.Plane();
      gp_Dir n = pl.Axis().Direction();
      if (out.reversed) {
        n.Reverse();
      }
      out.kind = 0;
      out.origin = v3(pl.Location());
      out.dir = v3(n);
      break;
    }
    case GeomAbs_Cylinder: {
      const gp_Cylinder c = surf.Cylinder();
      out.kind = 1;
      out.origin = v3(c.Location());
      out.dir = v3(c.Axis().Direction());
      out.radius = c.Radius();
      break;
    }
    case GeomAbs_Cone: {
      const gp_Cone c = surf.Cone();
      out.kind = 2;
      out.origin = v3(c.Location());
      out.dir = v3(c.Axis().Direction());
      out.radius = c.RefRadius();
      out.angle = c.SemiAngle();
      break;
    }
    case GeomAbs_Sphere: {
      const gp_Sphere sp = surf.Sphere();
      out.kind = 3;
      out.origin = v3(sp.Location());
      out.radius = sp.Radius();
      break;
    }
    case GeomAbs_Torus: {
      const gp_Torus t = surf.Torus();
      out.kind = 4;
      out.origin = v3(t.Location());
      out.dir = v3(t.Axis().Direction());
      out.radius = t.MajorRadius();
      out.radius2 = t.MinorRadius();
      break;
    }
    case GeomAbs_BSplineSurface:
      out.kind = 5;
      break;
    case GeomAbs_BezierSurface:
      out.kind = 6;
      break;
    case GeomAbs_SurfaceOfRevolution:
      out.kind = 7;
      break;
    case GeomAbs_SurfaceOfExtrusion:
      out.kind = 8;
      break;
    case GeomAbs_OffsetSurface:
      out.kind = 9;
      break;
    default:
      out.kind = 10;
      break;
    }
  });
}

void edge_info(const Shape& s, std::uint32_t index, EdgeOut& out) {
  guarded("edge_info", [&] {
    if (index >= static_cast<std::uint32_t>(s.edges.Extent())) {
      throw std::out_of_range("edge index out of range");
    }
    const TopoDS_Edge& edge = TopoDS::Edge(s.edges(static_cast<int>(index) + 1));
    TopoDS_Vertex v1;
    TopoDS_Vertex v2;
    TopExp::Vertices(edge, v1, v2, true);
    if (!v1.IsNull()) {
      out.start = v3(BRep_Tool::Pnt(v1));
    }
    if (!v2.IsNull()) {
      out.end = v3(BRep_Tool::Pnt(v2));
    }
    out.degenerate = BRep_Tool::Degenerated(edge);
    if (out.degenerate) {
      out.kind = 8;
      out.length = 0.0;
      return;
    }
    GProp_GProps props;
    BRepGProp::LinearProperties(edge, props);
    out.length = props.Mass();
    BRepAdaptor_Curve curve(edge);
    switch (curve.GetType()) {
    case GeomAbs_Line: {
      const gp_Lin l = curve.Line();
      out.kind = 0;
      out.origin = v3(l.Location());
      out.dir = v3(l.Direction());
      break;
    }
    case GeomAbs_Circle: {
      const gp_Circ c = curve.Circle();
      out.kind = 1;
      out.origin = v3(c.Location());
      out.dir = v3(c.Axis().Direction());
      out.radius = c.Radius();
      break;
    }
    case GeomAbs_Ellipse:
      out.kind = 2;
      break;
    case GeomAbs_Hyperbola:
      out.kind = 3;
      break;
    case GeomAbs_Parabola:
      out.kind = 4;
      break;
    case GeomAbs_BSplineCurve:
      out.kind = 5;
      break;
    case GeomAbs_BezierCurve:
      out.kind = 6;
      break;
    case GeomAbs_OffsetCurve:
      out.kind = 7;
      break;
    default:
      out.kind = 8;
      break;
    }
  });
}

void tessellate(const Shape& s, double linear, double angular, MeshOut& out) {
  guarded("tessellate", [&] {
    BRepMesh_IncrementalMesh mesher(s.shape, linear, false, angular, false);
    if (!mesher.IsDone()) {
      throw std::runtime_error("meshing did not complete");
    }
    for (int fi = 1; fi <= s.faces.Extent(); ++fi) {
      const TopoDS_Face& face = TopoDS::Face(s.faces(fi));
      const std::uint32_t first = static_cast<std::uint32_t>(out.indices.size());
      TopLoc_Location loc;
      const occ::handle<Poly_Triangulation>& tri = BRep_Tool::Triangulation(face, loc);
      if (!tri.IsNull()) {
        if (!tri->HasNormals()) {
          BRepLib_ToolTriangulatedShape::ComputeNormals(face, tri);
        }
        const gp_Trsf trsf = loc.Transformation();
        const bool reversed = face.Orientation() == TopAbs_REVERSED;
        const std::uint32_t base = static_cast<std::uint32_t>(out.positions.size() / 3);
        for (int n = 1; n <= tri->NbNodes(); ++n) {
          const gp_Pnt p = tri->Node(n).Transformed(trsf);
          push3(out.positions, p.X(), p.Y(), p.Z());
          gp_Dir d = tri->Normal(n).Transformed(trsf);
          if (reversed) {
            d.Reverse();
          }
          push3(out.normals, d.X(), d.Y(), d.Z());
        }
        for (int t = 1; t <= tri->NbTriangles(); ++t) {
          int a = 0;
          int b = 0;
          int c = 0;
          tri->Triangle(t).Get(a, b, c);
          if (reversed) {
            std::swap(b, c);
          }
          out.indices.push_back(base + static_cast<std::uint32_t>(a - 1));
          out.indices.push_back(base + static_cast<std::uint32_t>(b - 1));
          out.indices.push_back(base + static_cast<std::uint32_t>(c - 1));
        }
      }
      out.face_ranges.push_back(static_cast<std::uint32_t>(fi - 1));
      out.face_ranges.push_back(first);
      out.face_ranges.push_back(static_cast<std::uint32_t>(out.indices.size()) - first);
    }
    for (int ei = 1; ei <= s.edges.Extent(); ++ei) {
      const TopoDS_Edge& edge = TopoDS::Edge(s.edges(ei));
      const std::uint32_t first = static_cast<std::uint32_t>(out.edge_points.size() / 3);
      if (!BRep_Tool::Degenerated(edge)) {
        BRepAdaptor_Curve curve(edge);
        GCPnts_TangentialDeflection points(curve, angular, linear);
        for (int i = 1; i <= points.NbPoints(); ++i) {
          const gp_Pnt p = points.Value(i);
          push3(out.edge_points, p.X(), p.Y(), p.Z());
        }
      }
      out.edge_ranges.push_back(static_cast<std::uint32_t>(ei - 1));
      out.edge_ranges.push_back(first);
      out.edge_ranges.push_back(static_cast<std::uint32_t>(out.edge_points.size() / 3) - first);
    }
  });
}

void mass_properties(const Shape& s, MassOut& out) {
  guarded("mass_properties", [&] {
    GProp_GProps vol;
    BRepGProp::VolumeProperties(s.shape, vol);
    GProp_GProps surf;
    BRepGProp::SurfaceProperties(s.shape, surf);
    out.volume = vol.Mass();
    out.area = surf.Mass();
    const GProp_GProps& use = std::abs(out.volume) > 0.0 ? vol : surf;
    out.center = v3(use.CentreOfMass());
    const gp_Mat m = use.MatrixOfInertia();
    for (int r = 1; r <= 3; ++r) {
      for (int c = 1; c <= 3; ++c) {
        out.inertia.push_back(m.Value(r, c));
      }
    }
  });
}

void bounding_box(const Shape& s, BoxOut& out) {
  guarded("bounding_box", [&] {
    Bnd_Box box;
    BRepBndLib::AddOptimal(s.shape, box, false, false);
    out.is_void = box.IsVoid();
    if (!out.is_void) {
      double x0 = 0, y0 = 0, z0 = 0, x1 = 0, y1 = 0, z1 = 0;
      box.Get(x0, y0, z0, x1, y1, z1);
      out.min = V3{x0, y0, z0};
      out.max = V3{x1, y1, z1};
    }
  });
}

bool is_valid(const Shape& s) {
  return guarded("is_valid", [&] {
    BRepCheck_Analyzer analyzer(s.shape);
    return analyzer.IsValid();
  });
}

rust::Vec<std::uint8_t> export_step(const ShapeList& shapes) {
  return guarded("export_step", [&] {
    STEPControl_Writer writer;
    for (const TopoDS_Shape& s : shapes.items) {
      if (writer.Transfer(s, STEPControl_AsIs) != IFSelect_RetDone) {
        throw std::runtime_error("could not translate a shape to STEP");
      }
    }
    std::ostringstream stream;
    if (writer.WriteStream(stream) != IFSelect_RetDone) {
      throw std::runtime_error("could not write the STEP stream");
    }
    const std::string text = stream.str();
    rust::Vec<std::uint8_t> bytes;
    bytes.reserve(text.size());
    for (char c : text) {
      bytes.push_back(static_cast<std::uint8_t>(c));
    }
    return bytes;
  });
}

std::unique_ptr<ShapeList> import_step(rust::Slice<const std::uint8_t> data) {
  return guarded("import_step", [&] {
    std::istringstream stream(std::string(reinterpret_cast<const char*>(data.data()), data.size()));
    STEPControl_Reader reader;
    if (reader.ReadStream("tenon-import.step", stream) != IFSelect_RetDone) {
      throw std::runtime_error("the data is not readable STEP");
    }
    reader.TransferRoots();
    auto list = std::make_unique<ShapeList>();
    for (int i = 1; i <= reader.NbShapes(); ++i) {
      const TopoDS_Shape s = reader.Shape(i);
      if (!s.IsNull()) {
        list->items.push_back(s);
      }
    }
    if (list->items.empty()) {
      throw std::runtime_error("the STEP data contains no shapes");
    }
    return list;
  });
}

} // namespace tenon_occt
