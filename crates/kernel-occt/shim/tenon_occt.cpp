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
#include <BRepExtrema_DistShapeShape.hxx>
#include <BRepFilletAPI_MakeChamfer.hxx>
#include <BRepFilletAPI_MakeFillet.hxx>
#include <BRepOffsetAPI_MakeThickSolid.hxx>
#include <TopTools_ListOfShape.hxx>
#include <BRepBuilderAPI_MakeEdge.hxx>
#include <BRepBuilderAPI_MakeFace.hxx>
#include <BRepBuilderAPI_MakeWire.hxx>
#include <BRepBuilderAPI_Transform.hxx>
#include <BRepPrimAPI_MakePrism.hxx>
#include <BRepPrimAPI_MakeRevol.hxx>
#include <BRep_Builder.hxx>
#include <GeomAPI_ProjectPointOnCurve.hxx>
#include <Geom_BSplineCurve.hxx>
#include <Geom_Circle.hxx>
#include <Geom_Line.hxx>
#include <NCollection_Array1.hxx>
#include <Precision.hxx>
#include <ShapeFix_Face.hxx>
#include <TopExp_Explorer.hxx>
#include <TopoDS_Compound.hxx>
#include <TopoDS_Wire.hxx>
#include <gp_Ax1.hxx>
#include <gp_Ax3.hxx>
#include <gp_Vec.hxx>
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

// Records, for every face and edge of one input, where it went in `result`, and what every face,
// edge and vertex generated (a fillet's corner blends come from vertices).
void record_history(BRepBuilderAPI_MakeShape& algo, std::uint32_t input, const TopoDS_Shape& input_shape,
                    const Shape& result, HistoryOut& hist) {
  const std::pair<std::uint8_t, TopAbs_ShapeEnum> kinds[] = {
      {KIND_FACE, TopAbs_FACE}, {KIND_EDGE, TopAbs_EDGE}, {KIND_VERTEX, TopAbs_VERTEX}};
  for (const auto& kind : kinds) {
    ShapeMap map;
    TopExp::MapShapes(input_shape, kind.second, map);
    const ShapeMap& result_map = kind.first == KIND_FACE ? result.faces : result.edges;
    for (int i = 1; i <= map.Extent(); ++i) {
      const TopoDS_Shape& s = map(i);
      if (kind.first != KIND_VERTEX) {
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
      }
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

namespace {

constexpr std::uint8_t ROLE_START_CAP = 9;
constexpr std::uint8_t ROLE_END_CAP = 10;
constexpr double kTwoPi = 6.283185307179586;
/// Distance within which an edge lies on the profile curve it was built from (mm).
constexpr double kTagTolerance = 1e-5;

struct PlaneFrame {
  gp_Pnt origin;
  gp_Dir x;
  gp_Dir y;
  gp_Dir z;
  gp_Pnt at(double u, double v) const { return gp_Pnt(origin.XYZ() + x.XYZ() * u + y.XYZ() * v); }
};

PlaneFrame plane_frame(const Frame3& f, double offset) {
  const gp_Dir z = dir(f.z_dir);
  const gp_Dir x = dir(f.x_dir);
  return PlaneFrame{gp_Pnt(pnt(f.origin).XYZ() + z.XYZ() * offset), x, z.Crossed(x), z};
}

struct TaggedCurve {
  occ::handle<Geom_Curve> curve;
  std::uint64_t tag;
};

using TaggedEdges = std::vector<std::pair<TopoDS_Edge, std::uint64_t>>;

TopoDS_Edge profile_edge(const CurveIn& c, const ProfileIn& p, const PlaneFrame& f, std::vector<TaggedCurve>& curves) {
  switch (c.kind) {
  case 0: {
    const gp_Pnt a = f.at(c.x0, c.y0);
    const gp_Pnt b = f.at(c.x1, c.y1);
    if (a.Distance(b) <= Precision::Confusion()) {
      throw std::invalid_argument("a profile line has zero length");
    }
    curves.push_back({new Geom_Line(a, gp_Dir(gp_Vec(a, b))), c.tag});
    BRepBuilderAPI_MakeEdge mk(a, b);
    if (!mk.IsDone()) {
      throw std::runtime_error("could not build a profile line");
    }
    return mk.Edge();
  }
  case 1:
  case 2: {
    const gp_Circ circ(gp_Ax2(f.at(c.cx, c.cy), f.z, f.x), c.r);
    curves.push_back({new Geom_Circle(circ), c.tag});
    if (c.kind == 2) {
      BRepBuilderAPI_MakeEdge mk(circ);
      if (!mk.IsDone()) {
        throw std::runtime_error("could not build a profile circle");
      }
      return mk.Edge();
    }
    double a0 = c.a0;
    double a1 = c.a1;
    while (a1 <= a0) {
      a1 += kTwoPi;
    }
    BRepBuilderAPI_MakeEdge mk(circ, a0, a1);
    if (!mk.IsDone()) {
      throw std::runtime_error("could not build a profile arc");
    }
    return mk.Edge();
  }
  case 3: {
    const int n = static_cast<int>(c.pole_count);
    const int deg = static_cast<int>(c.degree);
    if (deg < 1 || n <= deg || static_cast<std::size_t>(c.pole_start + c.pole_count) * 2 > p.poles.size()) {
      throw std::invalid_argument("invalid B-spline in profile");
    }
    NCollection_Array1<gp_Pnt> poles(1, n);
    for (int i = 0; i < n; ++i) {
      const std::size_t k = (static_cast<std::size_t>(c.pole_start) + static_cast<std::size_t>(i)) * 2;
      poles(i + 1) = f.at(p.poles[k], p.poles[k + 1]);
    }
    const int nk = n - deg + 1;
    NCollection_Array1<double> knots(1, nk);
    NCollection_Array1<int> mults(1, nk);
    for (int i = 0; i < nk; ++i) {
      knots(i + 1) = static_cast<double>(i);
      mults(i + 1) = (i == 0 || i == nk - 1) ? deg + 1 : 1;
    }
    occ::handle<Geom_BSplineCurve> bs = new Geom_BSplineCurve(poles, knots, mults, deg);
    curves.push_back({bs, c.tag});
    BRepBuilderAPI_MakeEdge mk(bs);
    if (!mk.IsDone()) {
      throw std::runtime_error("could not build a profile spline");
    }
    return mk.Edge();
  }
  default:
    throw std::invalid_argument("unknown profile curve kind");
  }
}

TopoDS_Wire profile_wire(const std::vector<TopoDS_Edge>& edges) {
  BRepBuilderAPI_MakeWire mw;
  for (const TopoDS_Edge& e : edges) {
    mw.Add(e);
    if (!mw.IsDone()) {
      throw std::runtime_error("profile curves do not join end to end");
    }
  }
  const TopoDS_Wire w = mw.Wire();
  if (!BRep_Tool::IsClosed(w)) {
    throw std::runtime_error("a profile loop is not closed");
  }
  return w;
}

/// Faces of every region of the profile (a compound when there are several), placed `offset`
/// along the frame normal. Fills `curves` with the source geometry of each tag.
TopoDS_Shape profile_faces(const ProfileIn& p, double offset, std::vector<TaggedCurve>& curves) {
  const PlaneFrame f = plane_frame(p.frame, offset);
  const gp_Pln plane(gp_Ax3(f.origin, f.z, f.x));
  std::vector<TopoDS_Face> faces;
  const std::size_t n = p.curves.size();
  std::size_t i = 0;
  while (i < n) {
    const std::uint32_t region = p.curves[i].region;
    std::vector<TopoDS_Wire> wires;
    while (i < n && p.curves[i].region == region) {
      const std::uint32_t loop = p.curves[i].loop_index;
      std::vector<TopoDS_Edge> edges;
      while (i < n && p.curves[i].region == region && p.curves[i].loop_index == loop) {
        edges.push_back(profile_edge(p.curves[i], p, f, curves));
        ++i;
      }
      wires.push_back(profile_wire(edges));
    }
    BRepBuilderAPI_MakeFace mf(plane, wires.front(), true);
    if (!mf.IsDone()) {
      throw std::runtime_error("could not build a face from a profile region");
    }
    for (std::size_t w = 1; w < wires.size(); ++w) {
      mf.Add(TopoDS::Wire(wires[w].Reversed()));
    }
    ShapeFix_Face fix(mf.Face());
    fix.Perform();
    faces.push_back(fix.Face());
  }
  if (faces.empty()) {
    throw std::invalid_argument("the profile has no regions");
  }
  if (faces.size() == 1) {
    return faces.front();
  }
  TopoDS_Compound compound;
  BRep_Builder builder;
  builder.MakeCompound(compound);
  for (const TopoDS_Face& face : faces) {
    builder.Add(compound, face);
  }
  return compound;
}

/// Matches every edge of `faces` to the profile curve it lies on.
TaggedEdges tag_edges(const TopoDS_Shape& faces, const std::vector<TaggedCurve>& curves) {
  TaggedEdges out;
  ShapeMap edges;
  TopExp::MapShapes(faces, TopAbs_EDGE, edges);
  for (int i = 1; i <= edges.Extent(); ++i) {
    const TopoDS_Edge& e = TopoDS::Edge(edges(i));
    if (BRep_Tool::Degenerated(e)) {
      continue;
    }
    BRepAdaptor_Curve ac(e);
    const gp_Pnt mid = ac.Value(0.5 * (ac.FirstParameter() + ac.LastParameter()));
    double best = kTagTolerance;
    const TaggedCurve* found = nullptr;
    for (const TaggedCurve& c : curves) {
      GeomAPI_ProjectPointOnCurve proj(mid, c.curve);
      if (proj.NbPoints() > 0 && proj.LowerDistance() <= best) {
        best = proj.LowerDistance();
        found = &c;
      }
    }
    if (found != nullptr) {
      out.emplace_back(e, found->tag);
    }
  }
  return out;
}

void record_tagged(BRepBuilderAPI_MakeShape& algo, const TaggedEdges& edges, const Shape& result, HistoryOut& hist) {
  for (const auto& entry : edges) {
    for (const TopoDS_Shape& g : algo.Generated(entry.first)) {
      TagGen gen{entry.second, 0, 0};
      if (locate(result, g, gen.gen_kind, gen.gen_index)) {
        hist.tagged.push_back(gen);
      }
    }
  }
}

void add_roles(HistoryOut& hist, std::uint8_t role, const Shape& result, const TopoDS_Shape& caps) {
  for (TopExp_Explorer ex(caps, TopAbs_FACE); ex.More(); ex.Next()) {
    add_role(hist, role, result, ex.Current());
  }
}

} // namespace

std::unique_ptr<Shape> make_face(const ProfileIn& profile, double offset, HistoryOut& hist) {
  return guarded("make_face", [&] {
    std::vector<TaggedCurve> curves;
    const TopoDS_Shape faces = profile_faces(profile, offset, curves);
    auto out = wrap(faces);
    for (const auto& entry : tag_edges(faces, curves)) {
      TagGen gen{entry.second, 0, 0};
      if (locate(*out, entry.first, gen.gen_kind, gen.gen_index)) {
        hist.tagged.push_back(gen);
      }
    }
    return out;
  });
}

std::unique_ptr<Shape> extrude(const ProfileIn& profile, double start, double length, HistoryOut& hist) {
  return guarded("extrude", [&] {
    std::vector<TaggedCurve> curves;
    const TopoDS_Shape base = profile_faces(profile, start, curves);
    const TaggedEdges tags = tag_edges(base, curves);
    BRepPrimAPI_MakePrism mk(base, gp_Vec(dir(profile.frame.z_dir)) * length, false, true);
    mk.Build();
    if (!mk.IsDone()) {
      throw std::runtime_error("the extrusion did not complete");
    }
    auto out = wrap(single_solid(mk.Shape()));
    add_roles(hist, ROLE_START_CAP, *out, mk.FirstShape());
    add_roles(hist, ROLE_END_CAP, *out, mk.LastShape());
    record_tagged(mk, tags, *out, hist);
    return out;
  });
}

std::unique_ptr<Shape> revolve(const ProfileIn& profile, const V3& origin, const V3& axis_dir, double start, double sweep, bool full,
                               HistoryOut& hist) {
  return guarded("revolve", [&] {
    std::vector<TaggedCurve> curves;
    const TopoDS_Shape base = profile_faces(profile, 0.0, curves);
    const TaggedEdges tags = tag_edges(base, curves);
    const gp_Ax1 axis(pnt(origin), dir(axis_dir));
    std::unique_ptr<BRepPrimAPI_MakeRevol> mk =
        full ? std::make_unique<BRepPrimAPI_MakeRevol>(base, axis, false) : std::make_unique<BRepPrimAPI_MakeRevol>(base, axis, sweep, false);
    mk->Build();
    if (!mk->IsDone()) {
      throw std::runtime_error("the revolution did not complete");
    }
    TopoDS_Shape result = single_solid(mk->Shape());
    // History is recorded against the unrotated result; rotating by a location keeps every
    // sub-shape enumeration in the same order, so the indices stay valid.
    const Shape unrotated(result);
    if (!full) {
      add_roles(hist, ROLE_START_CAP, unrotated, mk->FirstShape());
      add_roles(hist, ROLE_END_CAP, unrotated, mk->LastShape());
    }
    record_tagged(*mk, tags, unrotated, hist);
    if (start != 0.0) {
      gp_Trsf rot;
      rot.SetRotation(axis, start);
      result = result.Moved(TopLoc_Location(rot));
    }
    BRepCheck_Analyzer check(result);
    if (!check.IsValid()) {
      throw std::runtime_error("the revolution is not a valid solid (does the profile cross the axis?)");
    }
    return wrap(result);
  });
}

std::unique_ptr<Shape> transform(const Shape& shape, std::uint8_t kind, const V3& a, const V3& b, double value, HistoryOut& hist) {
  return guarded("transform", [&] {
    gp_Trsf t;
    switch (kind) {
    case 0:
      t.SetTranslation(gp_Vec(a.x, a.y, a.z));
      break;
    case 1:
      t.SetRotation(gp_Ax1(pnt(a), dir(b)), value);
      break;
    case 2:
      t.SetMirror(gp_Ax2(pnt(a), dir(b)));
      break;
    case 3:
      t.SetScale(pnt(a), value);
      break;
    default:
      throw std::invalid_argument("unknown transform kind");
    }
    BRepBuilderAPI_Transform mk(shape.shape, t, true);
    if (!mk.IsDone()) {
      throw std::runtime_error("the transform did not complete");
    }
    auto out = wrap(mk.Shape());
    record_history(mk, 0, shape.shape, *out, hist);
    return out;
  });
}

namespace {

const TopoDS_Edge& edge_at(const Shape& body, std::uint32_t index) {
  if (index >= static_cast<std::uint32_t>(body.edges.Extent())) {
    throw std::invalid_argument("edge index out of range");
  }
  return TopoDS::Edge(body.edges(static_cast<int>(index) + 1));
}

const TopoDS_Face& face_at(const Shape& body, std::uint32_t index) {
  if (index >= static_cast<std::uint32_t>(body.faces.Extent())) {
    throw std::invalid_argument("face index out of range");
  }
  return TopoDS::Face(body.faces(static_cast<int>(index) + 1));
}

// Fillets, chamfers and shells can "succeed" with a broken solid; refuse those.
void require_valid(const TopoDS_Shape& s, const char* message) {
  BRepCheck_Analyzer check(s);
  if (!check.IsValid()) {
    throw std::runtime_error(message);
  }
}

} // namespace

std::unique_ptr<Shape> fillet(const Shape& body, rust::Slice<const std::uint32_t> edges, double radius, HistoryOut& hist) {
  return guarded("fillet", [&] {
    BRepFilletAPI_MakeFillet mk(body.shape);
    for (std::uint32_t e : edges) {
      mk.Add(radius, edge_at(body, e));
    }
    mk.Build();
    if (!mk.IsDone()) {
      throw std::runtime_error("the fillet could not be built (is the radius too large for the edges?)");
    }
    require_valid(mk.Shape(), "the fillet gave an invalid solid (is the radius too large for the edges?)");
    auto out = wrap(single_solid(mk.Shape()));
    record_history(mk, 0, body.shape, *out, hist);
    return out;
  });
}

std::unique_ptr<Shape> chamfer(const Shape& body, rust::Slice<const std::uint32_t> edges, std::uint8_t kind, double a, double b,
                               std::uint32_t reference, HistoryOut& hist) {
  return guarded("chamfer", [&] {
    BRepFilletAPI_MakeChamfer mk(body.shape);
    for (std::uint32_t e : edges) {
      switch (kind) {
      case 0:
        mk.Add(a, edge_at(body, e));
        break;
      case 1:
        mk.Add(a, b, edge_at(body, e), face_at(body, reference));
        break;
      case 2:
        mk.AddDA(a, b, edge_at(body, e), face_at(body, reference));
        break;
      default:
        throw std::invalid_argument("unknown chamfer kind");
      }
    }
    mk.Build();
    if (!mk.IsDone()) {
      throw std::runtime_error("the chamfer could not be built (is the distance too large for the edges?)");
    }
    require_valid(mk.Shape(), "the chamfer gave an invalid solid (is the distance too large for the edges?)");
    auto out = wrap(single_solid(mk.Shape()));
    record_history(mk, 0, body.shape, *out, hist);
    return out;
  });
}

std::unique_ptr<Shape> shell(const Shape& body, rust::Slice<const std::uint32_t> faces, double offset, HistoryOut& hist) {
  return guarded("shell", [&] {
    TopTools_ListOfShape open;
    for (std::uint32_t f : faces) {
      open.Append(face_at(body, f));
    }
    BRepOffsetAPI_MakeThickSolid mk;
    mk.MakeThickSolidByJoin(body.shape, open, offset, 1.0e-6);
    mk.Build();
    if (!mk.IsDone()) {
      throw std::runtime_error("the shell could not be built (is the wall thicker than the part allows?)");
    }
    require_valid(mk.Shape(), "the shell gave an invalid solid (is the wall thicker than the part allows?)");
    auto out = wrap(single_solid(mk.Shape()));
    record_history(mk, 0, body.shape, *out, hist);
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
    // Faces are meshed in parallel (OCCT's own thread pool).
    BRepMesh_IncrementalMesh mesher(s.shape, linear, false, angular, true);
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

namespace {
const TopoDS_Shape& sub_shape(const Shape& s, std::uint8_t kind, std::uint32_t index) {
  const ShapeMap* map = nullptr;
  switch (kind) {
  case 0:
    return s.shape;
  case 1:
    map = &s.faces;
    break;
  case 2:
    map = &s.edges;
    break;
  case 3:
    map = &s.vertices;
    break;
  default:
    throw std::invalid_argument("unknown sub-shape kind");
  }
  if (index >= static_cast<std::uint32_t>(map->Extent())) {
    throw std::out_of_range("sub-shape index out of range");
  }
  return (*map)(static_cast<int>(index) + 1);
}
} // namespace

void min_distance(const Shape& a, std::uint8_t kind_a, std::uint32_t index_a, const Shape& b, std::uint8_t kind_b, std::uint32_t index_b,
                  DistOut& out) {
  guarded("min_distance", [&] {
    BRepExtrema_DistShapeShape d(sub_shape(a, kind_a, index_a), sub_shape(b, kind_b, index_b));
    if (!d.IsDone() || d.NbSolution() < 1) {
      throw std::runtime_error("no distance found");
    }
    out.distance = d.Value();
    out.on_a = v3(d.PointOnShape1(1));
    out.on_b = v3(d.PointOnShape2(1));
  });
}

std::unique_ptr<Shape> solid_at(const Shape& shape, std::uint32_t index, HistoryOut& hist) {
  return guarded("solid_at", [&] {
    ShapeMap solids;
    TopExp::MapShapes(shape.shape, TopAbs_SOLID, solids);
    if (index >= static_cast<std::uint32_t>(solids.Extent())) {
      throw std::out_of_range("solid index out of range");
    }
    auto result = std::make_unique<Shape>(solids(static_cast<int>(index) + 1));
    // Faces and edges of the input that are in this solid map to themselves; the others are gone.
    const std::pair<std::uint8_t, const ShapeMap*> kinds[] = {{KIND_FACE, &shape.faces}, {KIND_EDGE, &shape.edges}};
    for (const auto& kind : kinds) {
      const ShapeMap& result_map = kind.first == KIND_FACE ? result->faces : result->edges;
      for (int i = 1; i <= kind.second->Extent(); ++i) {
        ImageEntry entry;
        entry.input = 0;
        entry.kind = kind.first;
        entry.index = static_cast<std::uint32_t>(i - 1);
        push_index(entry.images, result_map, (*kind.second)(i));
        hist.images.push_back(std::move(entry));
      }
    }
    return result;
  });
}

} // namespace tenon_occt
