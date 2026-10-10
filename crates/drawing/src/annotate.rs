//! Building a sheet: views (their lines, centrelines, section and detail markings, labels),
//! annotations (dimensions, hole tables, balloons, parts lists, notes), the border and the title
//! block. Dimensions and the rest are worked out from the model's current geometry each time,
//! so they follow model changes.

use std::collections::BTreeMap;

use tenon_assembly::ComponentId;
use tenon_geom::{Frame, Vec2, Vec3};
use tenon_kernel::{CurveKind, SurfaceKind};
use tenon_model::{FeatureKind, FeatureStatus, HoleExtent, HoleType};

use crate::graphics::{Align, EXT_GAP, EXT_OVER, Graphics, Owner, Pen, TEXT};
use crate::model::{AnnotKind, Annotation, DimKind, Drawing, GeomPick, PickPoint, SheetId, Standard, View, ViewId, ViewKind};
use crate::sheets::{BORDER, projection_symbol};
use crate::views::{Evaluation, ModelGeometry, ViewGeometry, project, section_frame};

/// Where a point of a view (view coordinates) lands on the sheet.
pub fn to_sheet(v: &View, g: &ViewGeometry, p: Vec2) -> Vec2 {
    v.center + (p - g.center) * v.scale
}

/// Where a point of the sheet is in a view (view coordinates).
pub fn from_sheet(v: &View, g: &ViewGeometry, p: Vec2) -> Vec2 {
    g.center + (p - v.center) * (1.0 / v.scale)
}

/// A picked edge's geometry in the model (assembly or part coordinates).
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Resolved {
    Point(Vec3),
    Line(Vec3, Vec3),
    Circle { center: Vec3, normal: Vec3, radius: f64 },
}

/// The edge a pick means, now.
pub fn resolve_pick(m: &ModelGeometry, pick: &GeomPick) -> Result<Resolved, String> {
    if m.instances.is_empty()
        && let Some(e) = &m.error
    {
        return Err(e.clone());
    }
    let inst = m.instance(pick.component).ok_or("the component is not in the model")?;
    let scene = m.scenes.get(&inst.part).ok_or("the part's geometry is not available")?;
    let (b, e) = tenon_assembly::geometry::find_edge(scene, &pick.edge)?;
    let body = scene.bodies.get(b).ok_or("no such body")?;
    let [s, en] = *body.ends.get(e).ok_or("no such edge")?;
    let f = &inst.frame;
    let turn = |v: Vec3| f.x() * v.x + f.y() * v.y + f.z() * v.z;
    let (s, en) = (f.to_world(s), f.to_world(en));
    let circle = match body.curves.get(e) {
        Some(CurveKind::Circle { axis, radius }) => Some((f.to_world(axis.origin()), turn(axis.dir()), *radius)),
        _ => None,
    };
    Ok(match pick.point {
        PickPoint::Start => Resolved::Point(s),
        PickPoint::End => Resolved::Point(en),
        PickPoint::Mid => Resolved::Point((s + en) * 0.5),
        PickPoint::Center => match circle {
            Some((c, _, _)) => Resolved::Point(c),
            None => return Err("only a circular edge has a centre".into()),
        },
        PickPoint::Whole => match circle {
            Some((center, normal, radius)) => Resolved::Circle { center, normal, radius },
            None => Resolved::Line(s, en),
        },
    })
}

/// A section view's plane: a point on it and the direction it is seen in (model coordinates).
/// What lies on the eye's side of it is cut away.
pub(crate) fn section_cut(v: &View, ev: &Evaluation) -> Option<(Vec3, Vec3)> {
    let ViewKind::Section { parent, a, b, flip } = &v.kind else { return None };
    let p = ev.view(*parent)?;
    let (_, point, look) = section_frame(&p.frame, *a, *b, *flip);
    Some((point, look))
}

/// An edge found under a sheet point.
#[derive(Clone, Debug, PartialEq)]
pub struct EdgeHit {
    /// The component it belongs to (assembly views).
    pub component: Option<ComponentId>,
    pub edge: tenon_model::EdgeRef,
    /// The point of the edge under the sheet point, in its part's coordinates.
    pub local: Vec3,
}

/// The edge of a view's model drawn nearest a sheet point, within `reach` (sheet mm), as a
/// clicked pick would find it. Of edges drawn on top of each other, the one nearest the eye.
/// Edges cut away by a section, or outside a detail's circle, are not there to pick.
pub fn edge_at(d: &Drawing, ev: &Evaluation, view: ViewId, at: Vec2, reach: f64) -> Option<EdgeHit> {
    let v = d.view(view)?;
    let g = ev.view(view)?;
    let m = ev.models.get(&v.model)?;
    let f = &g.frame;
    let cut = section_cut(v, ev);
    let circle = match &v.kind {
        ViewKind::Detail { center, radius, .. } => Some((*center, *radius)),
        _ => None,
    };
    let shown =
        |p: Vec3| cut.is_none_or(|(point, look)| (p - point).dot(look) >= -1e-6) && circle.is_none_or(|(c, r)| project(f, p).dist(c) <= r + 1e-6);
    // (distance on the sheet, depth towards the eye, the hit)
    let mut best: Option<(f64, f64, EdgeHit)> = None;
    for inst in &m.instances {
        let Some(scene) = m.scenes.get(&inst.part) else { continue };
        for body in &scene.bodies {
            for poly in &body.mesh.edges {
                let Some(r) = body.edge_ref(poly.edge) else { continue };
                let local: Vec<Vec3> = poly.points.iter().map(|p| Vec3::new(f64::from(p[0]), f64::from(p[1]), f64::from(p[2]))).collect();
                let pts: Vec<Vec3> = local.iter().map(|p| inst.frame.to_world(*p)).collect();
                // (distance, depth, point in part coordinates)
                let mut near: Option<(f64, f64, Vec3)> = None;
                for (i, s) in pts.windows(2).enumerate() {
                    if !shown(s[0]) && !shown(s[1]) {
                        continue;
                    }
                    let (a, b) = (to_sheet(v, g, project(f, s[0])), to_sheet(v, g, project(f, s[1])));
                    let ab = b - a;
                    let l2 = ab.dot(ab);
                    let t = if l2 > 0.0 { ((at - a).dot(ab) / l2).clamp(0.0, 1.0) } else { 0.0 };
                    let dist = at.dist(a + ab * t);
                    let depth = (s[0] + (s[1] - s[0]) * t).dot(f.z());
                    if near.is_none_or(|(nd, ..)| dist < nd) {
                        near = Some((dist, depth, local[i] + (local[i + 1] - local[i]) * t));
                    }
                }
                let Some((dist, depth, point)) = near.filter(|(dist, ..)| *dist <= reach) else { continue };
                let better = match &best {
                    None => true,
                    Some((bd, bz, _)) => dist < bd - 0.05 || ((dist - bd).abs() <= 0.05 && depth > bz + 1e-6),
                };
                if better {
                    best = Some((dist, depth, EdgeHit { component: inst.component, edge: r, local: point }));
                }
            }
        }
    }
    best.map(|(_, _, h)| h)
}

/// A number with at most `precision` decimals, no trailing zeros.
pub fn format_number(v: f64, precision: u8) -> String {
    let s = format!("{:.*}", usize::from(precision), v);
    let s = if s.contains('.') { s.trim_end_matches('0').trim_end_matches('.').to_owned() } else { s };
    if s == "-0" { "0".into() } else { s }
}

/// A dimension's measured value (mm or degrees) and its text.
pub fn dimension(d: &Drawing, ev: &Evaluation, a: &Annotation) -> Result<(f64, String), String> {
    let AnnotKind::Dimension { view, dim, a: pa, b: pb, text, precision, .. } = &a.kind else { return Err("not a dimension".into()) };
    let v = d.view(*view).ok_or("the view is gone")?;
    let g = ev.view(*view).ok_or("the view is not computed")?;
    let m = ev.models.get(&v.model).ok_or("the model is not loaded")?;
    let ra = resolve_pick(m, pa)?;
    let rb = pb.as_ref().map(|p| resolve_pick(m, p)).transpose()?;
    let f = &g.frame;
    let AnnotKind::Dimension { offset, .. } = &a.kind else { return Err("not a dimension".into()) };
    let at = from_sheet(v, g, v.center + *offset);
    let (value, prefix, suffix) = measure(*dim, f, &ra, rb.as_ref(), at)?;
    let shown = format!("{prefix}{}{suffix}", format_number(value, *precision));
    let text = match text {
        Some(t) => t.replace("<>", &shown),
        None => shown,
    };
    Ok((value, text))
}

/// Where two lines meet (`a1`-`a2` and `b1`-`b2`), or between them when parallel.
fn meeting(a1: Vec2, a2: Vec2, b1: Vec2, b2: Vec2) -> Vec2 {
    let (u, w) = (a2 - a1, b2 - b1);
    let den = u.x * w.y - u.y * w.x;
    if den.abs() > 1e-12 {
        let t = ((b1 - a1).x * w.y - (b1 - a1).y * w.x) / den;
        a1 + u * t
    } else {
        (a1 + b1) * 0.5
    }
}

/// Where what a dimension measures is on the sheet: the middle of its two points, a circle's
/// centre, or where two lines meet. `drw.dimension` with `by` places the dimension from here.
pub fn dimension_anchor(v: &View, g: &ViewGeometry, m: &ModelGeometry, dim: DimKind, a: &GeomPick, b: Option<&GeomPick>) -> Result<Vec2, String> {
    let f = &g.frame;
    let ra = resolve_pick(m, a)?;
    let rb = b.map(|p| resolve_pick(m, p)).transpose()?;
    let p = match (dim, &ra, rb.as_ref()) {
        (DimKind::Angle, Resolved::Line(a1, a2), Some(Resolved::Line(b1, b2))) => {
            meeting(project(f, *a1), project(f, *a2), project(f, *b1), project(f, *b2))
        }
        (_, Resolved::Circle { center, .. }, None) => project(f, *center),
        _ => match linear_points(f, &ra, rb.as_ref()) {
            (Some(p), Some(q)) => (p + q) * 0.5,
            (Some(p), None) => p,
            _ => return Err("pick a point or a straight edge".into()),
        },
    };
    Ok(to_sheet(v, g, p))
}

/// The directions of two lines from their meeting point that bound the sector `at` is in (of
/// the four the lines make).
fn towards(a1: Vec2, a2: Vec2, b1: Vec2, b2: Vec2, at: Vec2) -> (Vec2, Vec2, Vec2) {
    let vertex = meeting(a1, a2, b1, b2);
    let t = at - vertex;
    let (u, w) = ((a2 - a1).normalized(), (b2 - b1).normalized());
    let det = u.x * w.y - u.y * w.x;
    if det.abs() < 1e-12 {
        // Parallel: no sector; each line's direction towards the point.
        let u = if u.dot(t) < 0.0 { -u } else { u };
        let w = if w.dot(t) < 0.0 { -w } else { w };
        return (vertex, u, w);
    }
    // `t` = alpha u + beta w: the signs say which half of each line bounds its sector.
    let alpha = (t.x * w.y - t.y * w.x) / det;
    let beta = (u.x * t.y - u.y * t.x) / det;
    (vertex, if alpha < 0.0 { -u } else { u }, if beta < 0.0 { -w } else { w })
}

/// The value of a dimension in a view, with the text before and after it. `at` is where it is
/// placed (view coordinates): an angle measures the side it is on.
fn measure(kind: DimKind, f: &Frame, a: &Resolved, b: Option<&Resolved>, at: Vec2) -> Result<(f64, &'static str, &'static str), String> {
    let (p1, p2) = linear_points(f, a, b);
    Ok(match kind {
        DimKind::Horizontal => ((p2.ok_or("pick two points or a straight edge")? - p1.ok_or("pick a point")?).x.abs(), "", ""),
        DimKind::Vertical => ((p2.ok_or("pick two points or a straight edge")? - p1.ok_or("pick a point")?).y.abs(), "", ""),
        DimKind::Aligned => (p1.ok_or("pick a point")?.dist(p2.ok_or("pick two points or a straight edge")?), "", ""),
        DimKind::Diameter => match a {
            Resolved::Circle { radius, .. } => (2.0 * radius, "Ø", ""),
            _ => return Err("a diameter needs a circular edge".into()),
        },
        DimKind::Radius => match a {
            Resolved::Circle { radius, .. } => (*radius, "R", ""),
            _ => return Err("a radius needs a circular edge".into()),
        },
        DimKind::Angle => {
            let (Resolved::Line(a1, a2), Some(Resolved::Line(b1, b2))) = (a, b) else {
                return Err("an angle needs two straight edges".into());
            };
            let (_, u, w) = towards(project(f, *a1), project(f, *a2), project(f, *b1), project(f, *b2), at);
            (u.dot(w).clamp(-1.0, 1.0).acos().to_degrees(), "", "°")
        }
    })
}

/// The two points a linear dimension measures between (view coordinates).
fn linear_points(f: &Frame, a: &Resolved, b: Option<&Resolved>) -> (Option<Vec2>, Option<Vec2>) {
    let pt = |r: &Resolved| match r {
        Resolved::Point(p) => Some(project(f, *p)),
        Resolved::Circle { center, .. } => Some(project(f, *center)),
        Resolved::Line(..) => None,
    };
    match (a, b) {
        (Resolved::Line(s, e), None) => (Some(project(f, *s)), Some(project(f, *e))),
        (x, Some(y)) => {
            // A line picked with a point: its nearest end.
            let near = |l: &Resolved, to: Option<Vec2>| match (l, to) {
                (Resolved::Line(s, e), Some(t)) => {
                    let (s, e) = (project(f, *s), project(f, *e));
                    Some(if s.dist(t) <= e.dist(t) { s } else { e })
                }
                (Resolved::Line(s, _), None) => Some(project(f, *s)),
                (other, _) => pt(other),
            };
            let p1 = near(x, pt(y));
            (p1, near(y, p1))
        }
        (x, None) => (pt(x), None),
    }
}

fn text_line(g: &mut Graphics, o: Owner, from: Vec2, to: Vec2, text: &str, at: Vec2) {
    // The dimension line with a gap for the text where it crosses it.
    let w = crate::stroke::text_width(text, TEXT) + 2.0;
    let d = to - from;
    let len = d.len();
    if len < 1e-9 {
        return;
    }
    let u = d * (1.0 / len);
    let t = (at - from).dot(u).clamp(0.0, len);
    let (g0, g1) = ((t - w / 2.0).max(0.0), (t + w / 2.0).min(len));
    if g1 - g0 >= len - 1e-9 || (at - (from + u * t)).len() > TEXT {
        g.line(o, Pen::Thin, from, to);
    } else {
        if g0 > 1e-9 {
            g.line(o, Pen::Thin, from, from + u * g0);
        }
        if g1 < len - 1e-9 {
            g.line(o, Pen::Thin, from + u * g1, to);
        }
    }
}

/// Draws a dimension. Its line (or text, for diameters and radii) is at `at` on the sheet.
fn draw_dimension(gr: &mut Graphics, o: Owner, v: &View, g: &ViewGeometry, m: &ModelGeometry, a: &Annotation, text: &str) -> Result<(), String> {
    let AnnotKind::Dimension { dim, a: pa, b: pb, offset, .. } = &a.kind else { return Ok(()) };
    let f = &g.frame;
    let ra = resolve_pick(m, pa)?;
    let rb = pb.as_ref().map(|p| resolve_pick(m, p)).transpose()?;
    let s = |p: Vec2| to_sheet(v, g, p);
    let at = v.center + *offset;
    let text_at = |p: Vec2| Vec2::new(p.x, p.y - TEXT / 2.0);
    match dim {
        DimKind::Horizontal | DimKind::Vertical | DimKind::Aligned => {
            let (Some(p1), Some(p2)) = linear_points(f, &ra, rb.as_ref()) else { return Err("pick two points or a straight edge".into()) };
            let (p1, p2) = (s(p1), s(p2));
            // The direction measured along, and across it towards the dimension line.
            let along = match dim {
                DimKind::Horizontal => Vec2::new(1.0, 0.0),
                DimKind::Vertical => Vec2::new(0.0, 1.0),
                _ => {
                    let d = (p2 - p1).normalized();
                    if d.is_finite() && d.len() > 0.5 { d } else { Vec2::new(1.0, 0.0) }
                }
            };
            let across = Vec2::new(-along.y, along.x);
            // Both extension lines run across to the level of `at`.
            let level = (at - p1).dot(across);
            let q1 = p1 + across * level;
            let q2 = p2 + across * (level - (p2 - p1).dot(across));
            // Extension lines from just off the object to just past the dimension line.
            for (p, q) in [(p1, q1), (p2, q2)] {
                let d = q - p;
                let len = d.len();
                if len > EXT_GAP {
                    let u = d * (1.0 / len);
                    gr.line(o, Pen::Thin, p + u * EXT_GAP, q + u * EXT_OVER);
                }
            }
            let mid = (q1 + q2) * 0.5;
            let along_d = (q2 - q1).dot(along);
            let text_pos = if (at - mid).dot(along).abs() < along_d.abs() / 2.0 { q1 + along * (at - q1).dot(along) } else { mid };
            text_line(gr, o, q1, q2, text, text_pos);
            if along_d.abs() > 1e-9 {
                let dir = along * along_d.signum();
                gr.arrow(o, q1, -dir);
                gr.arrow(o, q2, dir);
            }
            gr.text(o, text_at(text_pos), TEXT, text, Align::Center);
        }
        DimKind::Diameter | DimKind::Radius => {
            let Resolved::Circle { center, radius, .. } = ra else { return Err("pick a circular edge".into()) };
            let c = s(project(f, center));
            let r = radius * v.scale;
            let dir = (at - c).normalized();
            let dir = if dir.is_finite() && dir.len() > 0.5 { dir } else { Vec2::new(1.0, 0.0) };
            let tip = c + dir * r;
            // Leader from the text to the circle, the arrow on the circle pointing in.
            let shoulder = if dir.x >= 0.0 { 1.0 } else { -1.0 };
            let elbow = Vec2::new(at.x - shoulder * 4.0, at.y);
            gr.polyline(o, Pen::Thin, vec![tip, elbow, at]);
            gr.arrow(o, tip, -dir);
            if *dim == DimKind::Radius {
                gr.line(o, Pen::Thin, c, tip);
            }
            gr.text(o, Vec2::new(at.x + shoulder * 1.0, at.y + 0.8), TEXT, text, if shoulder > 0.0 { Align::Left } else { Align::Right });
        }
        DimKind::Angle => {
            let (Resolved::Line(a1, a2), Some(Resolved::Line(b1, b2))) = (ra, rb) else { return Err("pick two straight edges".into()) };
            let (a1, a2, b1, b2) = (s(project(f, a1)), s(project(f, a2)), s(project(f, b1)), s(project(f, b2)));
            // The arc from one line to the other through the side `at` is on.
            let (vertex, ua, wb) = towards(a1, a2, b1, b2, at);
            let radius = at.dist(vertex).max(5.0);
            let (a0, a1x) = (ua.y.atan2(ua.x), wb.y.atan2(wb.x));
            let mut sweep = a1x - a0;
            while sweep > std::f64::consts::PI {
                sweep -= std::f64::consts::TAU;
            }
            while sweep < -std::f64::consts::PI {
                sweep += std::f64::consts::TAU;
            }
            let pts: Vec<Vec2> = (0..=32)
                .map(|i| {
                    let t = a0 + sweep * f64::from(i) / 32.0;
                    vertex + Vec2::new(t.cos(), t.sin()) * radius
                })
                .collect();
            if let (Some(first), Some(last)) = (pts.first().copied(), pts.last().copied()) {
                let tangent = |t: f64| Vec2::new(-t.sin(), t.cos()) * sweep.signum();
                gr.arrow(o, first, -tangent(a0));
                gr.arrow(o, last, tangent(a0 + sweep));
            }
            gr.polyline(o, Pen::Thin, pts);
            gr.text(o, text_at(at), TEXT, text, Align::Center);
        }
    }
    Ok(())
}

/// A hole of a hole table.
#[derive(Clone, Debug, PartialEq)]
pub struct HoleRow {
    pub tag: String,
    /// From the table's origin (view coordinates, model mm).
    pub x: f64,
    pub y: f64,
    pub description: String,
    /// Where the hole is in the view (view coordinates).
    pub at: Vec2,
    /// Its largest radius (of the counterbore or countersink if it has one), to tag it clear of
    /// its edge.
    pub radius: f64,
}

fn hole_description(h: &tenon_model::Hole) -> String {
    let n = |v: f64| format_number(v, 2);
    let depth = match h.extent {
        HoleExtent::ThroughAll => "THRU".to_string(),
        HoleExtent::Distance(d) => format!("DEEP {}", n(d)),
    };
    match &h.kind {
        HoleType::Simple => format!("Ø{} {depth}", n(h.diameter)),
        HoleType::Counterbore { diameter, depth: cd } => format!("Ø{} {depth}, CBORE Ø{} DEEP {}", n(h.diameter), n(*diameter), n(*cd)),
        HoleType::Countersink { diameter, angle } => format!("Ø{} {depth}, CSK Ø{} X {}°", n(h.diameter), n(*diameter), n(angle.to_degrees())),
    }
}

/// The holes of a part view that are seen end-on, tagged A1, A2, B1... by kind, measured from
/// `origin` (default: the bottom-left of the view).
pub fn hole_rows(d: &Drawing, ev: &Evaluation, view: ViewId, origin: Option<&GeomPick>) -> Result<Vec<HoleRow>, String> {
    let v = d.view(view).ok_or("no such view")?;
    let g = ev.view(view).ok_or("the view is not computed")?;
    let m = ev.models.get(&v.model).ok_or("the model is not loaded")?;
    if m.instances.is_empty()
        && let Some(e) = &m.error
    {
        return Err(e.clone());
    }
    if m.assembly.is_some() {
        return Err("hole tables are for part views".into());
    }
    let inst = m.instance(None).ok_or("no part")?;
    let doc = m.documents.get(&inst.part).ok_or("no part document")?;
    let scene = m.scenes.get(&inst.part).ok_or("no part geometry")?;
    let f = &g.frame;
    let zero = match origin {
        Some(p) => match resolve_pick(m, p)? {
            Resolved::Point(q) | Resolved::Circle { center: q, .. } => project(f, q),
            Resolved::Line(s, _) => project(f, s),
        },
        None => g.bounds.map_or(Vec2::new(0.0, 0.0), |b| b.0),
    };
    let mut kinds: Vec<String> = Vec::new();
    let mut rows = Vec::new();
    for feat in doc.features() {
        let FeatureKind::Hole(h) = &feat.kind else { continue };
        if !matches!(scene.status.iter().find(|(id, _)| *id == feat.id).map(|(_, s)| s), Some(FeatureStatus::Ok)) {
            continue;
        }
        let Some(frame) = scene.sketch_frames.get(&h.sketch) else { continue };
        if frame.z().dot(f.z()).abs() < 1.0 - 1e-9 {
            continue;
        }
        let Some(sk) = doc.sketch(h.sketch) else { continue };
        let desc = hole_description(h);
        let radius = match &h.kind {
            HoleType::Simple => h.diameter,
            HoleType::Counterbore { diameter, .. } | HoleType::Countersink { diameter, .. } => diameter.max(h.diameter),
        } / 2.0;
        let k = match kinds.iter().position(|x| *x == desc) {
            Some(k) => k,
            None => {
                kinds.push(desc.clone());
                kinds.len() - 1
            }
        };
        for pid in &h.points {
            let Some(pos) = sk.point(*pid) else { continue };
            let at = project(f, frame.plane_point(pos));
            rows.push((k, at, desc.clone(), radius));
        }
    }
    let mut counters = vec![0usize; kinds.len()];
    let mut out = Vec::with_capacity(rows.len());
    for (k, at, description, radius) in rows {
        counters[k] += 1;
        let letter = char::from(b'A' + u8::try_from(k % 26).unwrap_or(0));
        let rel = at - zero;
        out.push(HoleRow { tag: format!("{letter}{}", counters[k]), x: rel.x, y: rel.y, description, at, radius });
    }
    Ok(out)
}

/// A parts list row.
#[derive(Clone, Debug, PartialEq)]
pub struct PartsRow {
    pub item: usize,
    pub quantity: usize,
    pub part_number: String,
    pub description: String,
    pub part: String,
}

/// The parts list of an assembly model: the assembly's bill of materials
/// (`tenon_assembly::session::bom_with`), so item numbers match the assembly's own list.
pub fn parts_rows(m: &ModelGeometry) -> Vec<PartsRow> {
    let Some(asm) = &m.assembly else { return Vec::new() };
    tenon_assembly::session::bom_with(asm, |key| m.documents.get(key).map(|d| d.name.clone()), |_| None)
        .into_iter()
        .map(|r| PartsRow { item: r.item, quantity: r.quantity, part_number: r.part, description: r.name, part: r.key })
        .collect()
}

/// The item number of a component in its model's parts list.
pub fn item_of(m: &ModelGeometry, component: ComponentId) -> Option<usize> {
    let part = m.assembly.as_ref()?.component(component)?.key();
    parts_rows(m).iter().find(|r| r.part == part).map(|r| r.item)
}

/// A table: rows of cells, column widths, top-left at `at`, header first.
fn table(gr: &mut Graphics, o: Owner, at: Vec2, widths: &[f64], rows: &[Vec<String>]) {
    let h = 6.0;
    let total: f64 = widths.iter().sum();
    let n = rows.len() as f64;
    for i in 0..=rows.len() {
        let y = at.y - h * i as f64;
        gr.line(o, Pen::Thin, Vec2::new(at.x, y), Vec2::new(at.x + total, y));
    }
    let mut x = at.x;
    for w in std::iter::once(&0.0).chain(widths.iter()) {
        x += w;
        gr.line(o, Pen::Thin, Vec2::new(x, at.y), Vec2::new(x, at.y - h * n));
    }
    for (r, row) in rows.iter().enumerate() {
        let mut x = at.x;
        for (c, cell) in row.iter().enumerate() {
            let w = widths.get(c).copied().unwrap_or(10.0);
            gr.text(o, Vec2::new(x + w / 2.0, at.y - h * (r as f64 + 1.0) + 1.6), 2.5, cell.clone(), Align::Center);
            x += w;
        }
    }
}

/// Size of a table (width, height), for placing it.
pub fn hole_table_size(rows: usize) -> (f64, f64) {
    (16.0 + 22.0 + 22.0 + 70.0, 6.0 * (rows as f64 + 1.0))
}
pub fn parts_list_size(rows: usize) -> (f64, f64) {
    (12.0 + 12.0 + 40.0 + 50.0, 6.0 * (rows as f64 + 1.0))
}

/// The scale as written: 1:2, 1:1, 2:1.
pub fn scale_text(s: f64) -> String {
    if s >= 1.0 { format!("{}:1", format_number(s, 3)) } else { format!("1:{}", format_number(1.0 / s, 3)) }
}

/// Centre marks (circles seen end-on) and centrelines (cylinders seen side-on) of a view. In a
/// section (`cut`: the plane's point and the direction it is seen in), only of what is left: no
/// cylinder cut away, and side-on only those the plane cuts.
fn centerlines(gr: &mut Graphics, o: Owner, v: &View, g: &ViewGeometry, m: &ModelGeometry, cut: Option<(Vec3, Vec3)>) {
    let f = &g.frame;
    let z = f.z();
    let mut marks: Vec<(Vec2, f64)> = Vec::new();
    let mut lines: Vec<(Vec2, Vec2)> = Vec::new();
    let over = 2.0 / v.scale;
    for inst in &m.instances {
        let Some(scene) = m.scenes.get(&inst.part) else { continue };
        let fr = &inst.frame;
        let turn = |d: Vec3| fr.x() * d.x + fr.y() * d.y + fr.z() * d.z;
        for b in &scene.bodies {
            for (fi, (_, info)) in b.faces.iter().enumerate() {
                let (axis, radius) = match &info.surface {
                    SurfaceKind::Cylinder { axis, radius } => (axis, *radius),
                    _ => continue,
                };
                let (o3, d3) = (fr.to_world(axis.origin()), turn(axis.dir()));
                // The face's extent along its axis, from its mesh.
                let pts: Vec<Vec3> = b
                    .mesh
                    .faces
                    .iter()
                    .filter(|r| r.face as usize == fi)
                    .flat_map(|r| b.mesh.indices.get(r.first as usize..(r.first + r.count) as usize).unwrap_or(&[]).iter())
                    .filter_map(|i| b.mesh.positions.get(*i as usize))
                    .map(|p| fr.to_world(Vec3::new(f64::from(p[0]), f64::from(p[1]), f64::from(p[2]))))
                    .collect();
                if pts.is_empty() {
                    continue;
                }
                let ts: Vec<f64> = pts.iter().map(|p| (*p - o3).dot(d3)).collect();
                let (t0, t1) = (ts.iter().copied().fold(f64::MAX, f64::min), ts.iter().copied().fold(f64::MIN, f64::max));
                // Only round holes and bosses (at least half a turn), not fillets.
                let length = t1 - t0;
                if length <= 1e-9 || info.area / (radius * length) < std::f64::consts::PI * 0.98 {
                    continue;
                }
                if let Some((point, look)) = cut {
                    let behind = |p: &Vec3| (*p - point).dot(look) >= -1e-6;
                    if !pts.iter().any(behind) || (d3.dot(z).abs() < 1e-6 && (o3 - point).dot(look).abs() > radius + 1e-6) {
                        continue;
                    }
                }
                if d3.dot(z).abs() > 1.0 - 1e-6 {
                    let c = project(f, o3);
                    match marks.iter_mut().find(|(p, _)| p.dist(c) < 1e-6) {
                        Some(m) => m.1 = m.1.max(radius),
                        None => marks.push((c, radius)),
                    }
                } else if d3.dot(z).abs() < 1e-6 {
                    let (a, b2) = (project(f, o3 + d3 * (t0 - over)), project(f, o3 + d3 * (t1 + over)));
                    if !lines.iter().any(|(p, q)| (p.dist(a) < 1e-6 && q.dist(b2) < 1e-6) || (p.dist(b2) < 1e-6 && q.dist(a) < 1e-6)) {
                        lines.push((a, b2));
                    }
                }
            }
        }
    }
    for (c, r) in marks {
        let c = to_sheet(v, g, c);
        let arm = r * v.scale + CENTER_OVER;
        gr.line(o, Pen::Center, c - Vec2::new(arm, 0.0), c + Vec2::new(arm, 0.0));
        gr.line(o, Pen::Center, c - Vec2::new(0.0, arm), c + Vec2::new(0.0, arm));
    }
    for (a, b) in lines {
        gr.line(o, Pen::Center, to_sheet(v, g, a), to_sheet(v, g, b));
    }
}

/// Draws a view: its lines, hatching, centrelines, the markings of its sections and details, its
/// label.
fn draw_view(gr: &mut Graphics, d: &Drawing, v: &View, ev: &Evaluation) {
    let o = Owner::View(v.id);
    let Some(g) = ev.view(v.id) else { return };
    for c in &g.curves {
        let pen = if c.visible { Pen::Visible } else { Pen::Hidden };
        gr.polyline(o, pen, c.points.iter().map(|p| to_sheet(v, g, *p)).collect());
    }
    for h in &g.hatch {
        gr.line(o, Pen::Hatch, to_sheet(v, g, h[0]), to_sheet(v, g, h[1]));
    }
    if v.centerlines
        && !matches!(v.kind, ViewKind::Detail { .. })
        && let Some(m) = ev.models.get(&v.model)
    {
        centerlines(gr, o, v, g, m, section_cut(v, ev));
    }
    if let ViewKind::Detail { radius, .. } = &v.kind {
        gr.circle(o, Pen::Thin, v.center, radius * v.scale);
    }
    // Section lines and detail circles drawn on this view for its children.
    for child in d.views.iter().filter(|c| c.kind.parent() == Some(v.id)) {
        match &child.kind {
            ViewKind::Section { a, b, flip, .. } => {
                let (pa, pb) = (to_sheet(v, g, *a), to_sheet(v, g, *b));
                let dir = (pb - pa).normalized();
                let (pa, pb) = (pa - dir * 5.0, pb + dir * 5.0);
                gr.line(o, Pen::Cutting, pa, pb);
                let (_, _, look) = section_frame(&g.frame, *a, *b, *flip);
                let look2 = project(&g.frame, look).normalized();
                for p in [pa, pb] {
                    let tail = p - look2 * 0.0;
                    let tip = p + look2 * 8.0;
                    gr.line(o, Pen::Thin, tail, tip);
                    gr.arrow(o, tip, look2);
                    gr.text(o, tip + look2 * 4.0 + Vec2::new(0.0, -TEXT / 2.0), 5.0, child.name.clone(), Align::Center);
                }
            }
            ViewKind::Detail { center, radius, .. } => {
                let c = to_sheet(v, g, *center);
                let r = radius * v.scale;
                gr.circle(o, Pen::Thin, c, r);
                gr.text(o, c + Vec2::new(r * 0.75 + 1.0, r * 0.75 + 1.0), 5.0, child.name.clone(), Align::Left);
            }
            _ => {}
        }
    }
    if v.label {
        let low = gr.bounds_of(o).map_or(v.center.y - 10.0, |b| b.0.y);
        let title = match &v.kind {
            ViewKind::Section { .. } => format!("SECTION {0}-{0}", v.name),
            ViewKind::Detail { .. } => format!("DETAIL {}", v.name),
            _ => v.name.clone(),
        };
        gr.text(o, Vec2::new(v.center.x, low - 8.0), 4.0, title, Align::Center);
        gr.text(o, Vec2::new(v.center.x, low - 13.5), 3.0, format!("SCALE {}", scale_text(v.scale)), Align::Center);
    }
}

/// Draws one annotation (its value is worked out again from the model).
fn draw_annotation(gr: &mut Graphics, d: &Drawing, ev: &Evaluation, a: &Annotation) -> Result<(), String> {
    let o = Owner::Annotation(a.id);
    match &a.kind {
        AnnotKind::Dimension { view, .. } => {
            let (_, text) = dimension(d, ev, a)?;
            let v = d.view(*view).ok_or("the view is gone")?;
            let g = ev.view(*view).ok_or("the view is not computed")?;
            let m = ev.models.get(&v.model).ok_or("the model is not loaded")?;
            draw_dimension(gr, o, v, g, m, a, &text)
        }
        AnnotKind::HoleTable { view, at, origin } => {
            let rows = hole_rows(d, ev, *view, origin.as_ref())?;
            let v = d.view(*view).ok_or("the view is gone")?;
            let g = ev.view(*view).ok_or("the view is not computed")?;
            let mut cells = vec![vec!["HOLE".to_string(), "XDIM".into(), "YDIM".into(), "DESCRIPTION".into()]];
            for r in &rows {
                cells.push(vec![r.tag.clone(), format_number(r.x, 2), format_number(r.y, 2), r.description.clone()]);
                // Up and right of the hole, clear of its edge and centre mark.
                let p = to_sheet(v, g, r.at);
                let off = r.radius * v.scale * std::f64::consts::FRAC_1_SQRT_2 + 1.5;
                gr.text(o, p + Vec2::new(off, off), 2.5, r.tag.clone(), Align::Left);
            }
            table(gr, o, *at, &[16.0, 22.0, 22.0, 70.0], &cells);
            Ok(())
        }
        AnnotKind::Balloon { view, component, attach, offset } => {
            let v = d.view(*view).ok_or("the view is gone")?;
            let g = ev.view(*view).ok_or("the view is not computed")?;
            let m = ev.models.get(&v.model).ok_or("the model is not loaded")?;
            let inst = m.instance(Some(*component)).ok_or("the component is not in the model")?;
            let item = item_of(m, *component).ok_or("the component is not in the parts list")?;
            let p = to_sheet(v, g, project(&g.frame, inst.frame.to_world(*attach)));
            let c = v.center + *offset;
            let r = 4.0;
            gr.circle(o, Pen::Thin, c, r);
            let dir = (p - c).normalized();
            if dir.is_finite() && p.dist(c) > r {
                gr.line(o, Pen::Thin, c + dir * r, p);
            }
            gr.dot(o, p, 0.6);
            gr.text(o, Vec2::new(c.x, c.y - TEXT / 2.0), TEXT, item.to_string(), Align::Center);
            Ok(())
        }
        AnnotKind::PartsList { view, at } => {
            let v = d.view(*view).ok_or("the view is gone")?;
            let m = ev.models.get(&v.model).ok_or("the model is not loaded")?;
            if m.assembly.is_none() {
                return Err("a parts list needs an assembly view".into());
            }
            let mut cells = vec![vec!["ITEM".to_string(), "QTY".into(), "PART NUMBER".into(), "DESCRIPTION".into()]];
            for r in parts_rows(m) {
                cells.push(vec![r.item.to_string(), r.quantity.to_string(), r.part_number, r.description]);
            }
            table(gr, o, *at, &[12.0, 12.0, 40.0, 50.0], &cells);
            Ok(())
        }
        AnnotKind::Note { at, text, height, .. } => {
            for (i, line) in text.lines().enumerate() {
                gr.text(o, Vec2::new(at.x, at.y - i as f64 * height * 1.6), *height, line, Align::Left);
            }
            Ok(())
        }
        AnnotKind::CenterMark { .. } | AnnotKind::Centerline { .. } | AnnotKind::CenterlineBisector { .. } => {
            for [p, q] in center_segments(d, ev, a)? {
                gr.line(o, Pen::Center, p, q);
            }
            Ok(())
        }
    }
}

/// How far centre marks and centre lines run past what they mark (sheet mm).
pub const CENTER_OVER: f64 = 2.0;

/// A point a pick stands for in a view: a circle's centre, a line's middle, or the point.
fn pick_point(f: &Frame, r: &Resolved) -> Vec2 {
    match r {
        Resolved::Point(p) | Resolved::Circle { center: p, .. } => project(f, *p),
        Resolved::Line(s, e) => (project(f, *s) + project(f, *e)) * 0.5,
    }
}

/// The segments (sheet) of a centre mark or centre line placed by hand, from the model as it
/// is now.
pub fn center_segments(d: &Drawing, ev: &Evaluation, a: &Annotation) -> Result<Vec<[Vec2; 2]>, String> {
    let (view, pa, pb) = match &a.kind {
        AnnotKind::CenterMark { view, a } => (*view, a, None),
        AnnotKind::Centerline { view, a, b } | AnnotKind::CenterlineBisector { view, a, b } => (*view, a, Some(b)),
        _ => return Err("not a centre mark or line".into()),
    };
    let v = d.view(view).ok_or("the view is gone")?;
    let g = ev.view(view).ok_or("the view is not computed")?;
    let m = ev.models.get(&v.model).ok_or("the model is not loaded")?;
    let f = &g.frame;
    let s = |p: Vec2| to_sheet(v, g, p);
    let ra = resolve_pick(m, pa)?;
    let rb = pb.map(|p| resolve_pick(m, p)).transpose()?;
    // A segment from p to q, run CENTER_OVER past both ends.
    let long = |p: Vec2, q: Vec2| -> Result<[Vec2; 2], String> {
        let u = (q - p).normalized();
        if !(u.is_finite() && u.len() > 0.5) {
            return Err("the two places are the same".into());
        }
        Ok([p - u * CENTER_OVER, q + u * CENTER_OVER])
    };
    match (&a.kind, ra, rb) {
        (AnnotKind::CenterMark { .. }, Resolved::Circle { center, radius, .. }, _) => {
            let c = s(project(f, center));
            let arm = radius * v.scale + CENTER_OVER;
            Ok(vec![[c - Vec2::new(arm, 0.0), c + Vec2::new(arm, 0.0)], [c - Vec2::new(0.0, arm), c + Vec2::new(0.0, arm)]])
        }
        (AnnotKind::CenterMark { .. }, _, _) => Err("a centre mark needs a circle or an arc".into()),
        (AnnotKind::Centerline { .. }, ra, Some(rb)) => Ok(vec![long(s(pick_point(f, &ra)), s(pick_point(f, &rb)))?]),
        (AnnotKind::CenterlineBisector { .. }, Resolved::Line(a1, a2), Some(Resolved::Line(b1, b2))) => {
            let (a1, a2, b1, b2) = (s(project(f, a1)), s(project(f, a2)), s(project(f, b1)), s(project(f, b2)));
            let (u, w) = ((a2 - a1).normalized(), (b2 - b1).normalized());
            if !(u.is_finite() && w.is_finite()) {
                return Err("a line of no length".into());
            }
            let cross = u.x * w.y - u.y * w.x;
            if cross.abs() < 1e-9 {
                // Parallel: midway between them, along all of both.
                let n = Vec2::new(-u.y, u.x);
                if (b1 - a1).dot(n).abs() < 1e-9 {
                    return Err("the two lines are one line".into());
                }
                let mid = a1 + n * ((b1 - a1).dot(n) / 2.0);
                let ts = [a1, a2, b1, b2].map(|p| (p - a1).dot(u));
                let (lo, hi) = (ts.iter().copied().fold(f64::MAX, f64::min), ts.iter().copied().fold(f64::MIN, f64::max));
                Ok(vec![long(mid + u * lo, mid + u * hi)?])
            } else {
                // Meeting: the bisector of the angle the two lines span, over their length.
                let vertex = meeting(a1, a2, b1, b2);
                let away = |p: Vec2, q: Vec2| if p.dist(vertex) > q.dist(vertex) { (p - vertex).normalized() } else { (q - vertex).normalized() };
                let dir = (away(a1, a2) + away(b1, b2)).normalized();
                if !(dir.is_finite() && dir.len() > 0.5) {
                    return Err("the two lines run in opposite directions from where they meet".into());
                }
                let ts = [a1, a2, b1, b2].map(|p| (p - vertex).dot(dir));
                let (lo, hi) = (ts.iter().copied().fold(f64::MAX, f64::min).max(0.0), ts.iter().copied().fold(f64::MIN, f64::max));
                Ok(vec![long(vertex + dir * lo, vertex + dir * hi)?])
            }
        }
        (AnnotKind::CenterlineBisector { .. }, _, _) => Err("a centre line between lines needs two straight edges".into()),
        _ => Err("a centre line needs two places".into()),
    }
}

/// The border and the title block.
fn draw_frame(gr: &mut Graphics, d: &Drawing, sheet: SheetId) {
    let Some(s) = d.sheet(sheet) else { return };
    let o = Owner::Frame;
    let (w, h) = (s.size.width, s.size.height);
    if s.border {
        gr.polyline(
            o,
            Pen::Border,
            vec![
                Vec2::new(BORDER, BORDER),
                Vec2::new(w - BORDER, BORDER),
                Vec2::new(w - BORDER, h - BORDER),
                Vec2::new(BORDER, h - BORDER),
                Vec2::new(BORDER, BORDER),
            ],
        );
    }
    let corner = Vec2::new(w, 0.0);
    for l in &s.title_block.lines {
        gr.line(o, Pen::Thin, corner + l.a, corner + l.b);
    }
    let sheets = d.sheets.len();
    let index = d.sheets.iter().position(|x| x.id == sheet).unwrap_or(0) + 1;
    let scale = d.views_on(sheet).find(|v| matches!(v.kind, ViewKind::Base { .. })).map_or_else(|| "1:1".to_string(), |v| scale_text(v.scale));
    for fld in &s.title_block.fields {
        let p = corner + fld.at;
        gr.text(o, Vec2::new(p.x, p.y + fld.height + 1.8), 1.8, fld.label.clone(), Align::Left);
        let value = match fld.key.as_str() {
            "title" => d.props.title.clone(),
            "number" => d.props.number.clone(),
            "revision" => d.props.revision.clone(),
            "company" => d.props.company.clone(),
            "drawn_by" => d.props.drawn_by.clone(),
            "date" => d.props.date.clone(),
            "scale" => scale.clone(),
            "sheet" => format!("{index} OF {sheets}"),
            "size" => s.size.name.clone(),
            "units" => "mm".into(),
            "text" => String::new(),
            other => format!("<{other}>"),
        };
        gr.text(o, p, fld.height, value, Align::Left);
    }
    if let Some(c) = s.title_block.projection_symbol {
        let (lines, circles) = projection_symbol(corner + c, d.standard == Standard::Ansi);
        for l in lines {
            gr.line(o, Pen::Thin, l[0], l[1]);
        }
        for (c, r) in circles {
            gr.circle(o, Pen::Thin, c, r);
        }
    }
}

/// A sheet as primitives, with the problems met (annotations that cannot be drawn).
pub fn build(d: &Drawing, sheet: SheetId, ev: &Evaluation) -> (Graphics, BTreeMap<crate::model::AnnotId, String>) {
    let mut gr = Graphics { sheet: Some(sheet), ..Graphics::default() };
    if let Some(s) = d.sheet(sheet) {
        gr.width = s.size.width;
        gr.height = s.size.height;
    }
    draw_frame(&mut gr, d, sheet);
    for v in d.views_on(sheet) {
        draw_view(&mut gr, d, v, ev);
    }
    let mut problems = BTreeMap::new();
    for a in &d.annotations {
        let on_sheet = match &a.kind {
            AnnotKind::Note { sheet: s, .. } => *s == sheet,
            k => k.view().and_then(|v| d.view(v)).is_some_and(|v| v.sheet == sheet),
        };
        if !on_sheet {
            continue;
        }
        if let Err(e) = draw_annotation(&mut gr, d, ev, a) {
            problems.insert(a.id, e);
        }
    }
    (gr, problems)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn numbers_and_scales() {
        assert_eq!(format_number(12.0, 2), "12");
        assert_eq!(format_number(12.5, 2), "12.5");
        assert_eq!(format_number(12.256, 2), "12.26");
        assert_eq!(format_number(-0.0001, 2), "0");
        assert_eq!(scale_text(0.5), "1:2");
        assert_eq!(scale_text(2.0), "2:1");
        assert_eq!(scale_text(1.0), "1:1");
    }
}
