//! Which face of the part each surface of the mesh is.
//!
//! The mesher reads the part from a STEP file and numbers its surfaces its own way: the file
//! does not keep the order of the part's faces, nor their names. They are told apart by where
//! they lie: a surface of the mesh is the face of the part that its triangles' corners lie on,
//! judged against the triangles the kernel draws the part with.

use std::collections::BTreeMap;

use tenon_geom::Vec3;
use tenon_kernel::Mesh;

use crate::mesh::TetMesh;

/// How many triangles of a surface (each by its three corners), or lines of a curve, are looked
/// at to tell which face or edge it is.
const SAMPLES: usize = 12;

/// How far along its normal what lies on a flat face can part, as a share of the mesh's size: more
/// than what the drawing's single precision leaves of the face and its normal.
const FLAT: f64 = 1e-6;

/// How far off a round face its drawing may stand, and a little: the kernel's chords, and rounding.
const CHORDS: f64 = 1.5 * tenon_geom::tol::MESH_LINEAR;

/// The square of the distance from `p` to the triangle `a`, `b`, `c`.
pub(crate) fn dist2_to_triangle(p: Vec3, a: Vec3, b: Vec3, c: Vec3) -> f64 {
    // (The closest point of a triangle, region by region: a corner, an edge, or its inside.)
    let (ab, ac, ap) = (b - a, c - a, p - a);
    let (d1, d2) = (ab.dot(ap), ac.dot(ap));
    if d1 <= 0.0 && d2 <= 0.0 {
        return ap.len2();
    }
    let bp = p - b;
    let (d3, d4) = (ab.dot(bp), ac.dot(bp));
    if d3 >= 0.0 && d4 <= d3 {
        return bp.len2();
    }
    let vc = d1 * d4 - d3 * d2;
    if vc <= 0.0 && d1 >= 0.0 && d3 <= 0.0 {
        let v = d1 / (d1 - d3);
        return (p - (a + ab * v)).len2();
    }
    let cp = p - c;
    let (d5, d6) = (ab.dot(cp), ac.dot(cp));
    if d6 >= 0.0 && d5 <= d6 {
        return cp.len2();
    }
    let vb = d5 * d2 - d1 * d6;
    if vb <= 0.0 && d2 >= 0.0 && d6 <= 0.0 {
        let w = d2 / (d2 - d6);
        return (p - (a + ac * w)).len2();
    }
    let va = d3 * d6 - d5 * d4;
    if va <= 0.0 && (d4 - d3) >= 0.0 && (d5 - d6) >= 0.0 {
        let w = (d4 - d3) / ((d4 - d3) + (d5 - d6));
        return (p - (b + (c - b) * w)).len2();
    }
    let denom = 1.0 / (va + vb + vc);
    let (v, w) = (vb * denom, vc * denom);
    (p - (a + ab * v + ac * w)).len2()
}

/// For each surface of the mesh, the face of the part it lies on (`part` is the part as the
/// kernel draws it: triangles by face). An error when a surface lies on no one face.
///
/// A surface is judged by the corners of its triangles, which the mesher puts on the part's own
/// face. (Not by the triangles' middles: with straight mid-side nodes those lie on the chord, off
/// a round face and inside the material, and on a thin round part that is nearer a flat face
/// beside it than the round face itself.) The face taken is the one all the corners looked at lie
/// nearest, as the squares of their distances add up: of a thin rim's corners, half lie on the
/// edge it shares with a flat face, but the other half lie off that face and on the rim.
///
/// The drawing of a round face stands off it, by as much as the kernel's chords are allowed. Where
/// a plate is thinner than that, the corners of a bore's wall are nearer the flat faces beside it
/// than the drawing of the wall itself. But they lie at two heights, and nothing on a flat face does:
/// so such a face gives way to one they all lie on to within the chords, if there is one.
pub fn match_surfaces(mesh: &TetMesh, part: &Mesh) -> Result<BTreeMap<u32, u32>, String> {
    let at = |i: u32| part.positions.get(i as usize).map(|p| Vec3::new(f64::from(p[0]), f64::from(p[1]), f64::from(p[2])));
    // The part's triangles, face by face.
    let mut drawn: BTreeMap<u32, Vec<(Vec3, Vec3, Vec3)>> = BTreeMap::new();
    for range in &part.faces {
        let (first, end) = (range.first as usize, (range.first as usize).saturating_add(range.count as usize));
        for tri in part.indices.get(first..end.min(part.indices.len())).unwrap_or(&[]).as_chunks::<3>().0 {
            if let (Some(a), Some(b), Some(c)) = (at(tri[0]), at(tri[1]), at(tri[2])) {
                drawn.entry(range.face).or_default().push((a, b, c));
            }
        }
    }
    if drawn.is_empty() {
        return Err("the part has no faces to hold the mesh against".into());
    }
    let mut by_surface: BTreeMap<u32, Vec<usize>> = BTreeMap::new();
    for (i, t) in mesh.tris.iter().enumerate() {
        by_surface.entry(t.surface).or_default().push(i);
    }
    // (Near enough to be on a face: within what two triangulations of one curved face differ
    // by, which is small beside the part.)
    let near = (mesh.extent() * 0.02).max(1e-6);
    let mut found = BTreeMap::new();
    for (surface, tris) in &by_surface {
        let step = (tris.len() / SAMPLES).max(1);
        let corners: Vec<Vec3> =
            tris.iter().step_by(step).take(SAMPLES).flat_map(|t| mesh.tris[*t].nodes[..3].iter().map(|n| mesh.nodes[*n as usize])).collect();
        let off_it = |face: u32| off_a_flat_face(&drawn[&face], &corners, FLAT * mesh.extent());
        let best = nearest_of(&drawn, &corners, near, &|_| true);
        let best = match best {
            Some((_, face)) if off_it(face) => nearest_of(&drawn, &corners, CHORDS, &|other| !off_it(other)).or(best),
            _ => best,
        };
        match best {
            Some((_, face)) => {
                found.insert(*surface, face);
            }
            None => return Err(format!("the mesh does not lie on the part as it is: its surface {surface} is on no one face of it")),
        }
    }
    Ok(found)
}

/// Of the faces `drawn` that `wanted` says, the one `corners` lie nearest, as the squares of their
/// distances add up, with how much that comes to: none if a corner is further than `near` off each.
fn nearest_of(drawn: &BTreeMap<u32, Vec<(Vec3, Vec3, Vec3)>>, corners: &[Vec3], near: f64, wanted: &dyn Fn(u32) -> bool) -> Option<(f64, u32)> {
    let mut best: Option<(f64, u32)> = None;
    // (A corner off a face rules it out at once, so most faces take one look.)
    'faces: for (face, triangles) in drawn.iter().filter(|(face, _)| wanted(**face)) {
        let mut sum = 0.0;
        for p in corners {
            let d2 = triangles.iter().map(|(a, b, c)| dist2_to_triangle(*p, *a, *b, *c)).fold(f64::INFINITY, f64::min);
            sum += d2;
            if d2.sqrt() > near || best.is_some_and(|(least, _)| sum >= least) {
                continue 'faces;
            }
        }
        best = Some((sum, *face));
    }
    best
}

/// Whether the face drawn by `triangles` is flat, and `corners` lie further apart along its normal
/// than `within`: at two heights, so that they cannot all be on it.
fn off_a_flat_face(triangles: &[(Vec3, Vec3, Vec3)], corners: &[Vec3], within: f64) -> bool {
    let across = |t: &(Vec3, Vec3, Vec3)| (t.1 - t.0).cross(t.2 - t.0);
    let Some(normal) = triangles.iter().map(across).max_by(|a, b| a.len2().total_cmp(&b.len2())) else { return false };
    if normal.len2() == 0.0 {
        return false;
    }
    let normal = normal * (1.0 / normal.len());
    let apart = |points: &mut dyn Iterator<Item = Vec3>| {
        let (lo, hi) = points.map(|p| p.dot(normal)).fold((f64::INFINITY, f64::NEG_INFINITY), |(lo, hi), d| (lo.min(d), hi.max(d)));
        hi - lo
    };
    apart(&mut triangles.iter().flat_map(|t| [t.0, t.1, t.2])) <= within && apart(&mut corners.iter().copied()) > within
}

/// The edges of the part that close on themselves (a round hole's edge, say): each by its number,
/// with its length.
pub fn closed_edges(part: &Mesh) -> Vec<(u32, f64)> {
    let at = |p: &[f32; 3]| Vec3::new(f64::from(p[0]), f64::from(p[1]), f64::from(p[2]));
    part.edges
        .iter()
        .filter_map(|e| {
            let (first, last) = (e.points.first()?, e.points.last()?);
            let length: f64 = e.points.windows(2).map(|w| at(&w[0]).dist(at(&w[1]))).sum();
            // (Its two ends, which are single-precision here, within a thousandth of its length.)
            // (An edge with no length, at a cone's tip, say, holds nothing open.)
            (e.points.len() > 2 && length > 0.0 && at(first).dist(at(last)) <= 1e-3 * length).then_some((e.edge, length))
        })
        .collect()
}

/// How far it is across a face of the part, as the kernel draws it: the diagonal of its box.
pub fn across_face(part: &Mesh, face: u32) -> f64 {
    let at = |p: &[f32; 3]| Vec3::new(f64::from(p[0]), f64::from(p[1]), f64::from(p[2]));
    let mut corners = part.faces.iter().filter(|range| range.face == face).flat_map(|range| {
        let (first, end) = (range.first as usize, (range.first as usize).saturating_add(range.count as usize));
        part.indices.get(first..end.min(part.indices.len())).unwrap_or(&[]).iter().filter_map(|i| part.positions.get(*i as usize)).map(at)
    });
    let Some(first) = corners.next() else { return 0.0 };
    let (lo, hi) = corners.fold((first, first), |(lo, hi), p| (lo.min(p), hi.max(p)));
    (hi - lo).len()
}

/// The length of the shortest edge of the part that closes on itself, if it has one. A closed edge
/// has nothing to hold it open but its own length: an element much longer than a third of it closes
/// the hole up.
pub fn shortest_closed_edge(part: &Mesh) -> Option<f64> {
    closed_edges(part).into_iter().map(|(_, length)| length).min_by(f64::total_cmp)
}

/// The lines where the part touches itself (two bores just touching, a bore just touching a side):
/// sides on which more than two of the triangles the kernel draws it with meet, whichever faces
/// those are of. A mesh of such a part has sides with four triangles on them there too, as a hole
/// folded flat does, and it is no fault of the mesher's.
pub fn lines_of_no_thickness(part: &Mesh) -> Vec<(Vec3, Vec3)> {
    let key = |p: &[f32; 3]| p.map(|c| (c + 0.0).to_bits());
    let mut sides: BTreeMap<([u32; 3], [u32; 3]), (u32, [f32; 3], [f32; 3])> = BTreeMap::new();
    for tri in part.indices.as_chunks::<3>().0 {
        let at = |i: u32| part.positions.get(i as usize).copied();
        let (Some(a), Some(b), Some(c)) = (at(tri[0]), at(tri[1]), at(tri[2])) else { continue };
        for (p, q) in [(a, b), (b, c), (c, a)] {
            let (kp, kq) = (key(&p), key(&q));
            if kp != kq {
                sides.entry((kp.min(kq), kp.max(kq))).or_insert((0, p, q)).0 += 1;
            }
        }
    }
    let at = |p: [f32; 3]| Vec3::new(f64::from(p[0]), f64::from(p[1]), f64::from(p[2]));
    sides.into_values().filter(|(n, _, _)| *n > 2).map(|(_, p, q)| (at(p), at(q))).collect()
}

/// Whether the mesh's sides `open` all lie along `lines`: within `near` at both ends.
pub(crate) fn all_along(mesh: &TetMesh, open: &[[u32; 2]], lines: &[(Vec3, Vec3)], near: f64) -> bool {
    open.iter().all(|side| {
        let (p, q) = (mesh.nodes[side[0] as usize], mesh.nodes[side[1] as usize]);
        lines.iter().any(|(a, b)| dist2_to_segment(p, *a, *b).sqrt() <= near && dist2_to_segment(q, *a, *b).sqrt() <= near)
    })
}

/// The square of the distance from `p` to the piece of line from `a` to `b`.
fn dist2_to_segment(p: Vec3, a: Vec3, b: Vec3) -> f64 {
    let ab = b - a;
    let len2 = ab.len2();
    let t = if len2 > 0.0 { ((p - a).dot(ab) / len2).clamp(0.0, 1.0) } else { 0.0 };
    (p - (a + ab * t)).len2()
}

/// For each curve of the mesh that lies along an edge of the part, that edge (`part` is the
/// part as the kernel draws it: its edges as lines of points). A curve that lies along none
/// (the seam of a round face, which the part does not count as an edge) is left out.
pub fn match_curves(mesh: &TetMesh, part: &Mesh) -> BTreeMap<u32, u32> {
    let at = |p: &[f32; 3]| Vec3::new(f64::from(p[0]), f64::from(p[1]), f64::from(p[2]));
    let drawn: Vec<(u32, Vec<Vec3>)> = part.edges.iter().map(|e| (e.edge, e.points.iter().map(at).collect())).collect();
    let mut by_curve: BTreeMap<u32, Vec<usize>> = BTreeMap::new();
    for (i, l) in mesh.lines.iter().enumerate() {
        by_curve.entry(l.curve).or_default().push(i);
    }
    let near = (mesh.extent() * 0.02).max(1e-6);
    let mut found = BTreeMap::new();
    for (curve, lines) in &by_curve {
        let step = (lines.len() / SAMPLES).max(1);
        let mut votes: BTreeMap<u32, usize> = BTreeMap::new();
        let mut asked = 0;
        for l in lines.iter().step_by(step).take(SAMPLES) {
            // (Its middle node is on the curve itself; or, in a shape's mesh with straight
            // mid-side nodes, on the straight line beside it, well within `near`.)
            let p = mesh.nodes[mesh.lines[*l].nodes[2] as usize];
            let nearest = drawn
                .iter()
                .filter_map(|(edge, points)| points.windows(2).map(|w| dist2_to_segment(p, w[0], w[1])).min_by(f64::total_cmp).map(|d2| (d2, *edge)))
                .min_by(|x, y| x.0.total_cmp(&y.0));
            asked += 1;
            if let Some((d2, edge)) = nearest
                && d2.sqrt() <= near
            {
                *votes.entry(edge).or_default() += 1;
            }
        }
        if let Some((edge, n)) = votes.iter().max_by_key(|(_, n)| **n)
            && *n * 3 >= asked * 2
        {
            found.insert(*curve, *edge);
        }
    }
    found
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mesh::read_msh;
    use crate::mesh::tests::ONE_TET;
    use tenon_kernel::FaceRange;

    /// The unit tetrahedron as a kernel would draw it: one triangle to a face, the faces
    /// numbered another way round than the mesher's surfaces.
    fn drawn() -> Mesh {
        let positions = vec![[0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]];
        // face 0: the slanted one; 1: x = 0; 2: y = 0; 3: z = 0.
        let indices = vec![1, 2, 3, 0, 3, 2, 0, 1, 3, 0, 2, 1];
        let faces = (0..4).map(|f| FaceRange { face: f, first: f * 3, count: 3 }).collect();
        // Its edges, numbered another way round than the mesher's curves: 5 from the origin
        // along y, 6 the slanted one of the bottom, 7 from the origin along x.
        let line = |edge: u32, a: [f32; 3], b: [f32; 3]| tenon_kernel::EdgePolyline { edge, points: vec![a, b] };
        let edges =
            vec![line(5, [0.0, 0.0, 0.0], [0.0, 1.0, 0.0]), line(6, [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]), line(7, [0.0, 0.0, 0.0], [1.0, 0.0, 0.0])];
        Mesh { positions, normals: Vec::new(), indices, faces, edges, ..Mesh::default() }
    }

    #[test]
    fn the_distance_to_a_triangle_is_to_its_nearest_point() {
        let (a, b, c) = (Vec3::ZERO, Vec3::X, Vec3::Y);
        let d = |x: f64, y: f64, z: f64| dist2_to_triangle(Vec3::new(x, y, z), a, b, c).sqrt();
        assert!((d(0.25, 0.25, 2.0) - 2.0).abs() < 1e-12, "above its inside");
        assert!((d(-3.0, -4.0, 0.0) - 5.0).abs() < 1e-12, "off a corner");
        assert!((d(0.5, -2.0, 0.0) - 2.0).abs() < 1e-12, "off an edge");
        assert!((d(1.0, 1.0, 0.0) - 0.5f64.sqrt()).abs() < 1e-12, "off the slanted edge");
        assert!((d(3.0, 0.0, 0.0) - 2.0).abs() < 1e-12 && (d(0.0, 3.0, 4.0) - 20f64.sqrt()).abs() < 1e-12);
        assert!(d(0.2, 0.3, 0.0) < 1e-12, "on it");
    }

    #[test]
    fn each_surface_of_the_mesh_is_told_to_be_a_face_of_the_part() {
        let mesh = read_msh(ONE_TET).unwrap();
        let on = match_surfaces(&mesh, &drawn()).unwrap();
        // The mesher's surfaces 1 to 4 are z = 0, y = 0, x = 0 and the slanted face.
        assert_eq!(on.into_iter().collect::<Vec<_>>(), [(1, 3), (2, 2), (3, 1), (4, 0)]);
    }

    #[test]
    fn each_curve_of_the_mesh_is_told_to_be_an_edge_of_the_part() {
        let mesh = read_msh(ONE_TET).unwrap();
        // The mesher's curves 1 to 3: along x, the slanted side, along y.
        assert_eq!(match_curves(&mesh, &drawn()).into_iter().collect::<Vec<_>>(), [(1, 7), (2, 6), (3, 5)]);
        // A curve that lies along no edge the part counts is left out, without complaint.
        let mut fewer = drawn();
        fewer.edges.remove(1);
        assert_eq!(match_curves(&mesh, &fewer).into_iter().collect::<Vec<_>>(), [(1, 7), (3, 5)]);
        let d = |x: f64, y: f64| dist2_to_segment(Vec3::new(x, y, 0.0), Vec3::ZERO, Vec3::X).sqrt();
        assert!((d(0.5, 2.0) - 2.0).abs() < 1e-12 && (d(-3.0, 4.0) - 5.0).abs() < 1e-12 && (d(4.0, 0.0) - 3.0).abs() < 1e-12);
    }

    /// A disc of radius 50 and thickness `thick` as a kernel draws it, finely: face 0 its rim,
    /// face 1 its bottom, face 2 its top.
    fn disc(thick: f32) -> Mesh {
        let sides = 180u32;
        let round = |i: u32, z: f32| {
            let a = f64::from(i) * std::f64::consts::TAU / f64::from(sides);
            [(50.0 * a.cos()) as f32, (50.0 * a.sin()) as f32, z]
        };
        let mut positions = Vec::new();
        for i in 0..sides {
            positions.push(round(i, 0.0));
            positions.push(round(i, thick));
        }
        positions.push([0.0, 0.0, 0.0]);
        positions.push([0.0, 0.0, thick]);
        let (bottom_mid, top_mid) = (2 * sides, 2 * sides + 1);
        let at = |i: u32, up: u32| 2 * (i % sides) + up;
        let mut indices = Vec::new();
        for i in 0..sides {
            indices.extend([at(i, 0), at(i + 1, 0), at(i + 1, 1), at(i, 0), at(i + 1, 1), at(i, 1)]);
        }
        let rim = indices.len() as u32;
        for i in 0..sides {
            indices.extend([bottom_mid, at(i + 1, 0), at(i, 0)]);
        }
        let bottom = indices.len() as u32 - rim;
        for i in 0..sides {
            indices.extend([top_mid, at(i, 1), at(i + 1, 1)]);
        }
        let top = indices.len() as u32 - rim - bottom;
        let faces = vec![
            FaceRange { face: 0, first: 0, count: rim },
            FaceRange { face: 1, first: rim, count: bottom },
            FaceRange { face: 2, first: rim + bottom, count: top },
        ];
        Mesh { positions, normals: Vec::new(), indices, faces, ..Mesh::default() }
    }

    /// That disc as a shape's mesh has it, on few sides: its rim (surface 7) long flat strips
    /// between the two round edges with their mid-side nodes straight, its ends surfaces 8 and 9.
    fn on_few_sides(thick: f64, sides: u32) -> TetMesh {
        let round = |i: u32, z: f64| {
            let a = f64::from(i % sides) * std::f64::consts::TAU / f64::from(sides);
            Vec3::new(50.0 * a.cos(), 50.0 * a.sin(), z)
        };
        let (mut nodes, mut tris) = (Vec::new(), Vec::new());
        let mut tri = |a: Vec3, b: Vec3, c: Vec3, surface: u32| {
            let first = nodes.len() as u32;
            nodes.extend([a, b, c, (a + b) * 0.5, (b + c) * 0.5, (c + a) * 0.5]);
            tris.push(crate::mesh::Tri6 { nodes: [first, first + 1, first + 2, first + 3, first + 4, first + 5], surface });
        };
        for i in 0..sides {
            let (a, b, c, d) = (round(i, 0.0), round(i + 1, 0.0), round(i + 1, thick), round(i, thick));
            tri(a, b, c, 7);
            tri(a, c, d, 7);
            tri(Vec3::ZERO, b, a, 8);
            tri(Vec3::new(0.0, 0.0, thick), d, c, 9);
        }
        TetMesh { nodes, tris, ..TetMesh::default() }
    }

    #[test]
    fn a_line_where_the_part_touches_itself_is_found_from_how_it_is_drawn() {
        // A tetrahedron has none: each side has two triangles on it.
        assert!(lines_of_no_thickness(&drawn()).is_empty());
        // Two tetrahedra sharing just one side: four triangles on it, whichever face they are of,
        // whatever way round it is drawn, and with its own copies of the points.
        let (a, b) = ([0.0, 0.0, 0.0], [0.0, 0.0, 10.0]);
        let positions = vec![a, b, [1.0, 0.0, 5.0], [0.0, 1.0, 5.0], b, a, [-1.0, 0.0, 5.0], [0.0, -1.0, 5.0], [-0.0, 0.0, 0.0]];
        let indices = vec![0, 1, 2, 1, 0, 3, 4, 5, 6, 5, 4, 7];
        let faces = (0..4).map(|f| FaceRange { face: f, first: f * 3, count: 3 }).collect();
        let part = Mesh { positions, normals: Vec::new(), indices, faces, edges: Vec::new(), ..Mesh::default() };
        let lines = lines_of_no_thickness(&part);
        assert_eq!(lines.len(), 1, "{lines:?}");
        let (p, q) = lines[0];
        assert!(p.z.min(q.z) == 0.0 && p.z.max(q.z) == 10.0 && p.x == 0.0 && q.y == 0.0, "{lines:?}");
    }

    #[test]
    fn a_thin_rim_with_straight_mid_sides_is_told_from_the_flat_faces_beside_it() {
        // The middles of these strips lie inside the disc, nearer the flat faces than the rim; of
        // their corners, every one lies on the rim, and only half on either flat face.
        for (thick, sides) in [(0.5, 8), (0.1, 12), (0.1, 24), (1.0, 6), (5.0, 8)] {
            let on = match_surfaces(&on_few_sides(thick, sides), &disc(thick as f32)).unwrap();
            assert_eq!(on.into_iter().collect::<Vec<_>>(), [(7, 0), (8, 1), (9, 2)], "{thick} thick, {sides} sides");
        }
    }

    /// A plate with a bore of radius 150 as a kernel draws it, on chords that stand off the round by
    /// about 0.01: face 0 the wall, 1 the bottom, 2 the top. And as a mesh has it, on few sides whose
    /// corners are on the true round: the wall surface 7, the bottom 8, the top 9. Its corners lie
    /// further into the material than the drawing of the wall, where the flat faces are drawn too.
    fn bored_plate(thick: f64) -> (Mesh, TetMesh) {
        let (bore, outside, sides) = (150.0, 200.0, 272u32);
        let at = |r: f64, a: f64, z: f64| Vec3::new(r * a.cos(), r * a.sin(), z);
        let turn = |i: u32, n: u32| f64::from(i) * std::f64::consts::TAU / f64::from(n);
        let (mut positions, mut indices, mut faces) = (Vec::new(), Vec::new(), Vec::new());
        let single = |p: Vec3| [p.x as f32, p.y as f32, p.z as f32];
        let mut face = |number: u32, corners: &mut dyn FnMut(u32) -> [Vec3; 4]| {
            let first = indices.len() as u32;
            for i in 0..sides {
                let [a, b, c, d] = corners(i);
                let base = positions.len() as u32;
                positions.extend([single(a), single(b), single(c), single(d)]);
                indices.extend([base, base + 1, base + 2, base, base + 2, base + 3]);
            }
            faces.push(FaceRange { face: number, first, count: indices.len() as u32 - first });
        };
        face(0, &mut |i| {
            let (a, b) = (turn(i, sides), turn(i + 1, sides));
            [at(bore, b, 0.0), at(bore, a, 0.0), at(bore, a, thick), at(bore, b, thick)]
        });
        for (number, z) in [(1, 0.0), (2, thick)] {
            face(number, &mut |i| {
                let (a, b) = (turn(i, sides), turn(i + 1, sides));
                [at(bore, a, z), at(bore, b, z), at(outside, b, z), at(outside, a, z)]
            });
        }
        let part = Mesh { positions, normals: Vec::new(), indices, faces, ..Mesh::default() };
        // (Half a chord on from the drawing's corners, so as far off the drawing of the wall as can be.)
        let few = 34u32;
        let mine = |i: u32| (f64::from(i) * 8.0 + 0.5) * std::f64::consts::TAU / f64::from(sides);
        let (mut nodes, mut tris) = (Vec::new(), Vec::new());
        let mut tri = |a: Vec3, b: Vec3, c: Vec3, surface: u32| {
            let first = nodes.len() as u32;
            nodes.extend([a, b, c, (a + b) * 0.5, (b + c) * 0.5, (c + a) * 0.5]);
            tris.push(crate::mesh::Tri6 { nodes: [first, first + 1, first + 2, first + 3, first + 4, first + 5], surface });
        };
        for i in 0..few {
            let (a, b) = (mine(i), mine(i + 1));
            tri(at(bore, b, 0.0), at(bore, a, 0.0), at(bore, a, thick), 7);
            tri(at(bore, b, 0.0), at(bore, a, thick), at(bore, b, thick), 7);
            tri(at(bore, a, 0.0), at(bore, b, 0.0), at(outside, b, 0.0), 8);
            tri(at(bore, a, thick), at(outside, b, thick), at(bore, b, thick), 9);
        }
        (part, TetMesh { nodes, tris, ..TetMesh::default() })
    }

    #[test]
    fn a_bores_wall_on_a_plate_thinner_than_the_chords_is_told_from_the_flat_faces_beside_it() {
        // At 0.005 and under the corners came out nearer a flat face than the wall as it is drawn.
        for thick in [0.5, 0.02, 0.01, 0.006, 0.005, 0.003, 0.002, 0.001] {
            let (part, mesh) = bored_plate(thick);
            let on = match_surfaces(&mesh, &part).unwrap();
            assert_eq!(on.into_iter().collect::<Vec<_>>(), [(7, 0), (8, 1), (9, 2)], "{thick} thick");
        }
    }

    #[test]
    fn a_flat_face_gives_way_only_to_one_within_the_chords() {
        // Corners at two heights, on a flat face and nothing else within the chords: that face stands.
        let (mut part, mesh) = bored_plate(0.5);
        let wall = part.faces.remove(0);
        part.indices.drain(wall.first as usize..(wall.first + wall.count) as usize);
        for f in &mut part.faces {
            f.first -= wall.count;
        }
        let on = match_surfaces(&mesh, &part).unwrap();
        assert!(matches!(on.get(&7), Some(1 | 2)), "{on:?}");
        // What is not flat, or not at two heights, is not off a flat face.
        let flat = [(Vec3::ZERO, Vec3::X, Vec3::Y)];
        assert!(off_a_flat_face(&flat, &[Vec3::ZERO, Vec3::new(0.0, 0.0, 0.01)], 1e-3));
        assert!(!off_a_flat_face(&flat, &[Vec3::ZERO, Vec3::new(5.0, 3.0, 0.0)], 1e-3));
        assert!(!off_a_flat_face(&flat, &[Vec3::ZERO, Vec3::new(0.0, 0.0, 1e-4)], 1e-3));
        let bent = [(Vec3::ZERO, Vec3::X, Vec3::Y), (Vec3::X, Vec3::Y, Vec3::new(1.0, 1.0, 0.5))];
        assert!(!off_a_flat_face(&bent, &[Vec3::ZERO, Vec3::new(0.0, 0.0, 0.01)], 1e-3));
        assert!(!off_a_flat_face(&[], &[Vec3::ZERO, Vec3::Z], 1e-3));
    }

    #[test]
    fn the_shortest_edge_that_closes_on_itself_is_found() {
        let line = |edge: u32, points: Vec<[f32; 3]>| tenon_kernel::EdgePolyline { edge, points };
        let round = |r: f32| -> Vec<[f32; 3]> {
            (0..=120).map(|i| (i as f32 * std::f32::consts::TAU / 120.0, r)).map(|(a, r)| [r * a.cos(), r * a.sin(), 1.0]).collect()
        };
        let part = |edges| Mesh { edges, ..Mesh::default() };
        // An open edge does not count, however short; a closed one does, by its length.
        assert_eq!(shortest_closed_edge(&part(vec![line(0, vec![[0.0; 3], [1.0, 0.0, 0.0]])])), None);
        let some = part(vec![line(0, vec![[0.0; 3], [0.1, 0.0, 0.0]]), line(1, round(10.0)), line(2, round(2.0))]);
        let found = shortest_closed_edge(&some).unwrap();
        assert!((found - 2.0 * std::f64::consts::TAU).abs() < 1e-2, "{found}");
        // And each by its number. One with no length is none.
        assert_eq!(closed_edges(&some).iter().map(|(edge, _)| *edge).collect::<Vec<_>>(), [1, 2]);
        assert_eq!(closed_edges(&part(vec![line(0, vec![[1.0; 3]; 5])])), []);
    }

    #[test]
    fn how_far_it_is_across_a_face_goes_by_that_faces_triangles() {
        // The unit tetrahedron: its slanted face goes from corner to corner, and so does the base.
        let part = drawn();
        assert!((across_face(&part, 0) - 3f64.sqrt()).abs() < 1e-6, "{}", across_face(&part, 0));
        assert!((across_face(&part, 3) - 2f64.sqrt()).abs() < 1e-6, "{}", across_face(&part, 3));
        assert_eq!(across_face(&part, 9), 0.0);
    }

    #[test]
    fn a_mesh_that_is_not_of_this_part_is_refused() {
        let mut mesh = read_msh(ONE_TET).unwrap();
        // The same mesh moved away: on no face of the part.
        for p in &mut mesh.nodes {
            *p = *p + Vec3::new(5.0, 0.0, 0.0);
        }
        let e = match_surfaces(&mesh, &drawn()).unwrap_err();
        assert!(e.contains("is on no one face"), "{e}");
        assert!(match_surfaces(&mesh, &Mesh::default()).unwrap_err().contains("no faces"));
    }
}
