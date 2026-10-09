// Tenon OCCT shim: a small, coarse-grained C++ API over OpenCASCADE 8 for the cxx bridge in
// src/ffi.rs. One function per kernel operation; every function that can fail converts all C++
// and OCCT exceptions to std::runtime_error, which cxx returns to Rust as Err.
//
// Original Tenon code (MIT OR Apache-2.0), written against OCCT's public headers.
#pragma once

#include <cstddef>
#include <cstdint>
#include <memory>
#include <vector>

#include "rust/cxx.h"

#include <NCollection_IndexedMap.hxx>
#include <TopTools_ShapeMapHasher.hxx>
#include <TopoDS_Shape.hxx>

namespace tenon_occt {

// Shared structs, defined by the cxx-generated header (src/ffi.rs.h).
struct V3;
struct Frame3;
struct HistoryOut;
struct TopoOut;
struct FaceOut;
struct EdgeOut;
struct MeshOut;
struct MassOut;
struct BoxOut;
struct ProfileIn;
struct DistOut;
struct HlrOut;

using ShapeMap = NCollection_IndexedMap<TopoDS_Shape, TopTools_ShapeMapHasher>;

// An OCCT shape plus its deterministic face/edge/vertex enumerations (TopExp::MapShapes order).
// Sub-shape index i in Rust is map index i + 1 here.
class Shape {
public:
  explicit Shape(const TopoDS_Shape& s);
  TopoDS_Shape shape;
  ShapeMap faces;
  ShapeMap edges;
  ShapeMap vertices;
};

class ShapeList {
public:
  std::vector<TopoDS_Shape> items;
};

rust::String occt_version();

std::unique_ptr<ShapeList> new_shape_list();
void shape_list_push(ShapeList& list, const Shape& shape);
std::size_t shape_list_len(const ShapeList& list) noexcept;
std::unique_ptr<Shape> shape_list_get(const ShapeList& list, std::size_t index);

std::uint32_t face_count(const Shape& shape) noexcept;
std::uint32_t edge_count(const Shape& shape) noexcept;

std::unique_ptr<Shape> make_box(const Frame3& frame, double dx, double dy, double dz, HistoryOut& hist);
std::unique_ptr<Shape> make_cylinder(const Frame3& frame, double radius, double height, HistoryOut& hist);
std::unique_ptr<Shape> make_cone(const Frame3& frame, double r1, double r2, double height, HistoryOut& hist);
std::unique_ptr<Shape> make_sphere(const V3& center, double radius);
std::unique_ptr<Shape> make_torus(const Frame3& frame, double major_radius, double minor_radius);

std::unique_ptr<Shape> boolean_op(std::uint8_t op, const Shape& target, const ShapeList& tools, HistoryOut& hist);

std::unique_ptr<Shape> make_face(const ProfileIn& profile, double offset, HistoryOut& hist);
std::unique_ptr<Shape> extrude(const ProfileIn& profile, double start, double length, HistoryOut& hist);
std::unique_ptr<Shape> revolve(const ProfileIn& profile, const V3& origin, const V3& dir, double start, double sweep, bool full,
                               HistoryOut& hist);
std::unique_ptr<Shape> transform(const Shape& shape, std::uint8_t kind, const V3& a, const V3& b, double value, HistoryOut& hist);
std::unique_ptr<Shape> fillet(const Shape& body, rust::Slice<const std::uint32_t> edges, double radius, HistoryOut& hist);
std::unique_ptr<Shape> chamfer(const Shape& body, rust::Slice<const std::uint32_t> edges, std::uint8_t kind, double a, double b,
                               std::uint32_t reference, HistoryOut& hist);
std::unique_ptr<Shape> shell(const Shape& body, rust::Slice<const std::uint32_t> faces, double offset, HistoryOut& hist);

void topology(const Shape& shape, TopoOut& out);
void face_info(const Shape& shape, std::uint32_t index, FaceOut& out);
void edge_info(const Shape& shape, std::uint32_t index, EdgeOut& out);
void tessellate(const Shape& shape, double linear, double angular, MeshOut& out);
void mass_properties(const Shape& shape, MassOut& out);
void bounding_box(const Shape& shape, BoxOut& out);
bool is_valid(const Shape& shape);
// Solid `index` of a shape (TopExp order), with the images of the shape's faces and edges.
std::unique_ptr<Shape> solid_at(const Shape& shape, std::uint32_t index, HistoryOut& hist);
// Minimum distance between sub-shapes: kind 0 the whole shape, 1 a face, 2 an edge, 3 a vertex.
void min_distance(const Shape& a, std::uint8_t kind_a, std::uint32_t index_a, const Shape& b, std::uint8_t kind_b, std::uint32_t index_b,
                  DistOut& out);

// Hidden-line removal: the edges of shapes seen along -Z of iew (orthographic), as polylines
// in the view's XY coordinates, each marked sharp / smooth / outline and visible or hidden.
void hlr(const ShapeList& shapes, const Frame3& view, double deflection, bool hidden, HlrOut& out);

rust::Vec<std::uint8_t> export_step(const ShapeList& shapes);
std::unique_ptr<ShapeList> import_step(rust::Slice<const std::uint8_t> data);

} // namespace tenon_occt
