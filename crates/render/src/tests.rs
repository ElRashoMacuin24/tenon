use tenon_geom::{Aabb3, Vec3};
use tenon_kernel::{EdgePolyline, FaceRange, Mesh};

use crate::pick::{pick_edge, pick_face};
use crate::raster::{Style, encode_png, render};
use crate::{Camera, StdView};

/// Unit cube 0..10 with one face range per side (bottom, top, front -Y, back +Y, right +X,
/// left -X) and its 12 edges.
fn cube() -> Mesh {
    let s = 10.0f32;
    let positions = vec![[0.0, 0.0, 0.0], [s, 0.0, 0.0], [s, s, 0.0], [0.0, s, 0.0], [0.0, 0.0, s], [s, 0.0, s], [s, s, s], [0.0, s, s]];
    let quads: [[u32; 4]; 6] = [[0, 3, 2, 1], [4, 5, 6, 7], [0, 1, 5, 4], [2, 3, 7, 6], [1, 2, 6, 5], [3, 0, 4, 7]];
    let mut indices = Vec::new();
    let mut faces = Vec::new();
    for (f, q) in quads.iter().enumerate() {
        faces.push(FaceRange { face: f as u32, first: indices.len() as u32, count: 6 });
        indices.extend_from_slice(&[q[0], q[1], q[2], q[0], q[2], q[3]]);
    }
    let pairs = [(0, 1), (1, 2), (2, 3), (3, 0), (4, 5), (5, 6), (6, 7), (7, 4), (0, 4), (1, 5), (2, 6), (3, 7)];
    let edges = pairs.iter().enumerate().map(|(i, (a, b))| EdgePolyline { edge: i as u32, points: vec![positions[*a], positions[*b]] }).collect();
    Mesh { normals: vec![[0.0, 0.0, 1.0]; 8], positions, indices, faces, edges, ..Mesh::default() }
}

fn framed(view: StdView) -> Camera {
    let mut c = Camera::default();
    c.set_view(view);
    c.fit(&Aabb3::new(Vec3::ZERO, Vec3::new(10.0, 10.0, 10.0)));
    c
}

#[test]
fn ray_through_the_centre_hits_the_facing_side() {
    let m = cube();
    for (view, face) in [(StdView::Front, 2), (StdView::Top, 1), (StdView::Right, 4), (StdView::Back, 3), (StdView::Left, 5), (StdView::Bottom, 0)] {
        let c = framed(view);
        let (o, d) = c.ray(400.0, 300.0, 800.0, 600.0);
        let hit = pick_face(&[&m], o, d).unwrap_or_else(|| panic!("{view:?}: no hit"));
        assert_eq!(hit.face, face, "{view:?}");
    }
    let c = framed(StdView::Front);
    let (o, d) = c.ray(2.0, 2.0, 800.0, 600.0);
    assert!(pick_face(&[&m], o, d).is_none(), "the corner of the viewport misses");
}

#[test]
fn edges_are_picked_near_the_pointer_and_not_through_faces() {
    let m = cube();
    let c = framed(StdView::Front);
    // The front face's bottom edge (0-1, edge 0) projects to a horizontal line below the centre.
    let (x, y, _) = c.project(Vec3::new(5.0, 0.0, 0.0), 800.0, 600.0).unwrap();
    let e = pick_edge(&[&m], &c, 800.0, 600.0, x, y + 2.0, 6.0).unwrap();
    assert_eq!(e.edge, 0);
    // The back bottom edge (2-3) projects nearby (perspective) but is hidden behind the front.
    let (x, y, _) = c.project(Vec3::new(5.0, 10.0, 0.0), 800.0, 600.0).unwrap();
    let hidden = pick_edge(&[&m], &c, 800.0, 600.0, x, y, 1.0);
    assert!(hidden.is_none_or(|h| h.edge != 2), "{hidden:?}");
}

#[test]
fn every_visible_edge_is_picked_along_its_length_in_perspective() {
    // Regression: the hidden test used the screen fraction along an edge as the 3D fraction,
    // which in perspective put the point behind the faces and rejected visible edges.
    let m = cube();
    let c = framed(StdView::Home);
    let quads: [[usize; 4]; 6] = [[0, 3, 2, 1], [4, 5, 6, 7], [0, 1, 5, 4], [2, 3, 7, 6], [1, 2, 6, 5], [3, 0, 4, 7]];
    let normals = [-Vec3::Z, Vec3::Z, -Vec3::Y, Vec3::Y, Vec3::X, -Vec3::X];
    let pairs = [(0, 1), (1, 2), (2, 3), (3, 0), (4, 5), (5, 6), (6, 7), (7, 4), (0, 4), (1, 5), (2, 6), (3, 7)];
    let mut visible = 0;
    for (i, (a, b)) in pairs.iter().enumerate() {
        let facing = quads.iter().zip(normals).any(|(q, n)| q.contains(a) && q.contains(b) && n.dot(c.eye_dir()) > 0.0);
        if !facing {
            continue;
        }
        visible += 1;
        let (pa, pb) = (m.positions[*a], m.positions[*b]);
        let (pa, pb) = (Vec3::new(pa[0].into(), pa[1].into(), pa[2].into()), Vec3::new(pb[0].into(), pb[1].into(), pb[2].into()));
        for t in [0.25, 0.5, 0.75] {
            let p = pa.lerp(pb, t);
            let (x, y, _) = c.project(p, 800.0, 600.0).unwrap();
            let e = pick_edge(&[&m], &c, 800.0, 600.0, x, y, 3.0).unwrap_or_else(|| panic!("edge {i} at {t}: not picked"));
            assert_eq!(e.edge, i as u32, "edge {i} at {t}");
            assert!(e.point.dist(p) < 1e-6, "edge {i} at {t}: {:?} vs {p:?}", e.point);
        }
    }
    assert_eq!(visible, 9, "three faces show in the home view");
}

#[test]
fn software_render_draws_the_cube_and_encodes_png() {
    let m = cube();
    let c = framed(StdView::Home);
    let style = Style::default();
    let img = render(&[&m], &c, 320, 240, &style);
    assert_eq!(img.rgba.len(), 320 * 240 * 4);
    let bg_top = style.background_top;
    // The centre pixel is the shaded body, not the background.
    let i = (120 * 320 + 160) * 4;
    let px = &img.rgba[i..i + 4];
    assert_ne!(&px[..3], &bg_top[..3]);
    // With a pure green body, every body pixel is clearly greener than red or blue.
    let green = render(&[&m], &c, 320, 240, &Style { body: [0, 255, 0, 255], ..Style::default() });
    let body = green.rgba.as_chunks::<4>().0.iter().filter(|p| p[1] > p[0].saturating_add(50) && p[1] > p[2].saturating_add(50)).count();
    assert!(body > 5000, "the cube covers a good part of the image: {body} pixels");
    // A body with a colour of its own (its part's material) is drawn in it, exactly as if it
    // were the colour of every body; a colour for some other body changes nothing.
    let own = render(&[&m], &c, 320, 240, &Style { body_colors: vec![(0, [0, 255, 0, 255])], ..Style::default() });
    assert_eq!(own.rgba, green.rgba);
    let other = render(&[&m], &c, 320, 240, &Style { body_colors: vec![(1, [0, 255, 0, 255])], ..Style::default() });
    assert_eq!(other.rgba, img.rgba);
    // Highlighting a face changes pixels, also on a body with its own colour.
    let lit = render(&[&m], &c, 320, 240, &Style { face_colors: vec![(0, 1, [255, 0, 0, 255])], ..Style::default() });
    assert_ne!(lit.rgba, img.rgba);
    let lit_own = render(
        &[&m],
        &c,
        320,
        240,
        &Style { body_colors: vec![(0, [0, 255, 0, 255])], face_colors: vec![(0, 1, [255, 0, 0, 255])], ..Style::default() },
    );
    assert!(lit_own.rgba != own.rgba && lit_own.rgba != lit.rgba);
    let png = encode_png(&img).unwrap();
    assert!(png.starts_with(&[0x89, b'P', b'N', b'G']));
    // Degenerate sizes are clamped, not panics.
    assert_eq!(render(&[&m], &c, 0, 0, &style).width, 1);
}

fn pixel(rgba: &[u8], w: u32, x: f64, y: f64) -> [u8; 4] {
    let i = ((y as u32 * w + x as u32) * 4) as usize;
    [rgba[i], rgba[i + 1], rgba[i + 2], rgba[i + 3]]
}

#[test]
fn the_three_faces_of_an_iso_view_shade_differently() {
    // Regression: a headlight along the view direction lit +X, -Y and +Z equally in iso view.
    let (m, c) = (cube(), framed(StdView::Home));
    let img = render(&[&m], &c, 320, 240, &Style::default());
    let at = |p: Vec3| {
        let (x, y, _) = c.project(p, 320.0, 240.0).unwrap();
        pixel(&img.rgba, 320, x, y)[0]
    };
    let (top, front, right) = (at(Vec3::new(5.0, 5.0, 10.0)), at(Vec3::new(5.0, 0.0, 5.0)), at(Vec3::new(10.0, 5.0, 5.0)));
    for (a, b) in [(top, front), (top, right), (front, right)] {
        assert!(a.abs_diff(b) >= 8, "faces too alike: top {top}, front {front}, right {right}");
    }
    assert!(top > front && top > right, "the top is lit most: top {top}, front {front}, right {right}");
}

#[test]
fn gpu_viewport_renders_when_an_adapter_exists() {
    let instance = wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle_from_env());
    let Ok(adapter) = pollster_block(instance.request_adapter(&wgpu::RequestAdapterOptions { force_fallback_adapter: false, ..Default::default() }))
    else {
        eprintln!("no GPU adapter; skipping");
        return;
    };
    let Ok((device, queue)) = pollster_block(adapter.request_device(&wgpu::DeviceDescriptor::default())) else {
        eprintln!("no device; skipping");
        return;
    };
    let mut vp = crate::gpu::Viewport::new(&device);
    let m = cube();
    let style = Style::default();
    let linear = |v: u8| {
        let s = f32::from(v) / 255.0;
        if s <= 0.04045 { s / 12.92 } else { ((s + 0.055) / 1.055).powf(2.4) }
    };
    let body = style.body;
    let colors = crate::gpu::BodyColors { face: [linear(body[0]), linear(body[1]), linear(body[2])], edge: [0.0; 3], ..Default::default() };
    vp.set_bodies(&device, &[(&m, &colors)]);
    let c = framed(StdView::Home);
    let (w, h) = (160, 120);
    let (_, recreated) = vp.render(&device, &queue, &c, w, h, 10.0).unwrap();
    assert!(recreated);
    let (_, again) = vp.render(&device, &queue, &c, w, h, 10.0).unwrap();
    assert!(!again, "same size keeps the texture");

    // Regression: the texture handed to egui must hold gamma-encoded colours. The GPU and the
    // software renderer agree on the lit top face (the test cube's vertex normals are all +Z,
    // so only the top face is meaningful on the GPU).
    let (rw, rh, gpu) = vp.read_pixels(&device, &queue).unwrap();
    assert_eq!((rw, rh, gpu.len()), (w, h, (w * h * 4) as usize));
    let (x, y, _) = c.project(Vec3::new(5.0, 5.0, 10.0), f64::from(w), f64::from(h)).unwrap();
    let g = pixel(&gpu, w, x, y);
    let expected = crate::raster::shade(body, Vec3::Z, c.key_light());
    let soft = render(&[&m], &c, w, h, &style);
    let s = pixel(&soft.rgba, w, x, y);
    for ch in 0..3 {
        assert!(g[ch].abs_diff(expected[ch]) <= 3, "GPU {g:?} vs shade() {expected:?}");
        assert!(g[ch].abs_diff(s[ch]) <= 3, "GPU {g:?} vs software {s:?}");
    }
    assert_eq!(g[3], 255, "opaque where the body is");
    assert_eq!(pixel(&gpu, w, 1.0, 1.0)[3], 0, "transparent background");

    // Highlights draw over the base at the same depth: the top face (1) turns red, and clearing
    // the highlight brings the base colour back, without uploading the geometry again.
    let red = crate::gpu::BodyColors { faces: vec![(1, [1.0, 0.0, 0.0])], ..colors.clone() };
    vp.set_highlights(&device, &[(&m, &red)]);
    vp.render(&device, &queue, &c, w, h, 10.0).unwrap();
    let (_, _, lit) = vp.read_pixels(&device, &queue).unwrap();
    let r = pixel(&lit, w, x, y);
    assert!(r[0] > 150 && r[1] < 40 && r[2] < 40, "highlighted top face: {r:?}");
    vp.set_highlights(&device, &[(&m, &colors)]);
    vp.render(&device, &queue, &c, w, h, 10.0).unwrap();
    let (_, _, back) = vp.read_pixels(&device, &queue).unwrap();
    assert_eq!(pixel(&back, w, x, y), g, "the base colour again");

    // Keyed bodies: only new keys are uploaded; the picture is the same.
    assert_eq!(vp.set_bodies_keyed(&device, &[(1, &m, &colors), (2, &m, &colors)]), 2);
    assert_eq!(vp.set_bodies_keyed(&device, &[(1, &m, &colors), (3, &m, &colors)]), 1, "body 1 is kept");
    assert_eq!(vp.set_bodies_keyed(&device, &[(1, &m, &colors)]), 0);
    vp.render(&device, &queue, &c, w, h, 10.0).unwrap();
    let (_, _, keyed) = vp.read_pixels(&device, &queue).unwrap();
    assert_eq!(pixel(&keyed, w, x, y), g);
}

#[test]
fn a_body_is_drawn_in_full_from_its_far_mesh_or_not_at_all() {
    use crate::detail::{Detail, FAR_BELOW, NEAR_ABOVE, detail};
    let (w, h) = (1200.0, 800.0);
    for projection in [crate::Projection::Perspective, crate::Projection::Orthographic] {
        let mut c = framed(StdView::Front);
        c.projection = projection;
        // Pixels per millimetre where the view looks, and so the ball drawn `px` pixels in radius.
        let (_, y0, _) = c.project(c.target, w, h).unwrap();
        let (_, y1, _) = c.project(c.target + c.up(), w, h).unwrap();
        let scale = (y0 - y1).abs();
        let ball = |px: f64| px / scale;
        let at = |px: f64, was| detail(&c, w, h, c.target, ball(px), was);
        // Small on the screen: from afar. Large: in full.
        assert_eq!(at(20.0, None), Detail::Far, "{projection:?}");
        assert_eq!(at(200.0, None), Detail::Near);
        assert_eq!(at(FAR_BELOW - 1.0, Some(Detail::Near)), Detail::Far);
        assert_eq!(at(FAR_BELOW + 1.0, Some(Detail::Near)), Detail::Near);
        // Between the two limits a body stays as it was: it does not flicker as the view moves.
        let between = (FAR_BELOW + NEAR_ABOVE) / 2.0;
        assert_eq!(at(between, Some(Detail::Far)), Detail::Far);
        assert_eq!(at(between, Some(Detail::Near)), Detail::Near);
        assert_eq!(at(between, None), Detail::Near);
        assert_eq!(at(between, Some(Detail::Hidden)), Detail::Near);
        assert_eq!(at(NEAR_ABOVE + 1.0, Some(Detail::Far)), Detail::Near);
        // Off the screen: not at all. Partly on it: drawn.
        let aside = |mm: f64, px: f64| detail(&c, w, h, c.target + c.right() * mm, ball(px), None);
        let half = w / 2.0 / scale;
        assert_eq!(aside(half * 2.0, 20.0), Detail::Hidden, "{projection:?}");
        assert_eq!(aside(-half * 2.0, 20.0), Detail::Hidden);
        assert_eq!(detail(&c, w, h, c.target + c.up() * (half * 2.0), ball(20.0), None), Detail::Hidden);
        assert_eq!(aside(half + ball(10.0), 20.0), Detail::Far, "its edge is on the screen");
        assert_eq!(aside(half * 2.0, 400.0), Detail::Near, "large enough to reach in from outside");
    }
    // In perspective, what is behind the eye is not drawn; what is round the eye is, in full.
    let c = framed(StdView::Front);
    let behind = c.eye() - c.forward() * 50.0;
    assert_eq!(detail(&c, w, h, behind, 10.0, None), Detail::Hidden);
    assert_eq!(detail(&c, w, h, behind, 60.0, None), Detail::Near);
    assert_eq!(detail(&c, w, h, c.eye() + c.forward() * 5.0, 3.0, None), Detail::Near);
}

#[test]
fn bodies_left_out_are_kept_aside_and_highlights_are_sent_only_where_there_are_any() {
    let instance = wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle_from_env());
    let Ok(adapter) = pollster_block(instance.request_adapter(&wgpu::RequestAdapterOptions { force_fallback_adapter: false, ..Default::default() }))
    else {
        eprintln!("no GPU adapter; skipping");
        return;
    };
    let Ok((device, queue)) = pollster_block(adapter.request_device(&wgpu::DeviceDescriptor::default())) else {
        eprintln!("no device; skipping");
        return;
    };
    let mut vp = crate::gpu::Viewport::new(&device);
    let m = cube();
    let plain = crate::gpu::BodyColors { face: [0.5; 3], edge: [0.0; 3], ..Default::default() };
    // Three bodies; one goes off the screen and comes back: it is not sent again.
    assert_eq!(vp.set_bodies_keyed(&device, &[(1, &m, &plain), (2, &m, &plain), (3, &m, &plain)]), 3);
    assert_eq!(vp.drawn(), (3, 36, 36));
    assert_eq!(vp.set_bodies_keyed(&device, &[(1, &m, &plain), (3, &m, &plain)]), 0);
    assert_eq!(vp.drawn(), (2, 24, 24));
    assert_eq!(vp.held_vertices(), (2 * 60, 60), "36 corners of triangles and 24 ends of edges each");
    assert_eq!(vp.set_bodies_keyed(&device, &[(1, &m, &plain), (2, &m, &plain), (3, &m, &plain)]), 0, "body 2 was kept aside");
    assert_eq!(vp.held_vertices(), (3 * 60, 0));
    // What is kept aside is bounded: the bodies left out longest go first.
    let many: Vec<u64> = (10..10 + crate::gpu::SPARE_VERTICES / 60 + 50).collect();
    let all: Vec<(u64, &Mesh, &crate::gpu::BodyColors)> = many.iter().map(|k| (*k, &m, &plain)).collect();
    assert_eq!(vp.set_bodies_keyed(&device, &all), all.len());
    assert_eq!(vp.held_vertices().1, 3 * 60, "the three from before");
    assert_eq!(vp.set_bodies_keyed(&device, &all[..1]), 0);
    let (drawn, aside) = vp.held_vertices();
    assert_eq!(drawn, 60);
    assert!(aside <= crate::gpu::SPARE_VERTICES && aside > crate::gpu::SPARE_VERTICES - 120, "{aside}");
    assert_eq!(vp.set_bodies_keyed(&device, &[(1, &m, &plain)]), 1, "left out longest, so gone");
    assert_eq!(vp.set_bodies_keyed(&device, &all[all.len() - 40..]), 0, "left out last, so still there");

    // Highlights: a body with none sends nothing, and the picture is the base colour there.
    let red = crate::gpu::BodyColors { faces: vec![(1, [0.0, 0.0, 1.0]), (1, [1.0, 0.0, 0.0])], ..plain.clone() };
    vp.set_bodies_keyed(&device, &[(1, &m, &plain), (2, &m, &red)]);
    let c = framed(StdView::Home);
    let (w, h) = (160, 120);
    vp.render(&device, &queue, &c, w, h, 10.0).unwrap();
    let (_, _, lit) = vp.read_pixels(&device, &queue).unwrap();
    let (x, y, _) = c.project(Vec3::new(5.0, 5.0, 10.0), f64::from(w), f64::from(h)).unwrap();
    let r = pixel(&lit, w, x, y);
    assert!(r[0] > 150 && r[1] < 40 && r[2] < 40, "the later of two colours for a face: {r:?}");
    vp.set_highlights(&device, &[(&m, &plain), (&m, &plain)]);
    vp.render(&device, &queue, &c, w, h, 10.0).unwrap();
    let (_, _, back) = vp.read_pixels(&device, &queue).unwrap();
    let g = pixel(&back, w, x, y);
    assert!(g[0] == g[1] && g[1] == g[2] && g[3] == 255, "the base colour again: {g:?}");
}

/// Minimal executor for wgpu's futures (they complete immediately on native backends).
fn pollster_block<F: std::future::Future>(f: F) -> F::Output {
    use std::task::{Context, Poll, Waker};
    let waker = Waker::noop();
    let mut cx = Context::from_waker(waker);
    let mut f = std::pin::pin!(f);
    loop {
        if let Poll::Ready(v) = f.as_mut().poll(&mut cx) {
            return v;
        }
        std::thread::yield_now();
    }
}

/// Every triangle and every segment in turn, with no index: what picking must agree with.
mod slow {
    use tenon_geom::Vec3;
    use tenon_kernel::Mesh;
    use tenon_kernel::index::ray_triangle;

    use crate::Camera;
    use crate::pick::{EdgeHit, FaceHit};

    fn v3(p: [f32; 3]) -> Vec3 {
        Vec3::new(f64::from(p[0]), f64::from(p[1]), f64::from(p[2]))
    }

    pub fn pick_face(meshes: &[&Mesh], o: Vec3, d: Vec3) -> Option<FaceHit> {
        let mut best: Option<FaceHit> = None;
        for (bi, m) in meshes.iter().enumerate() {
            for range in &m.faces {
                let tris = &m.indices[range.first as usize..(range.first + range.count) as usize];
                for t in tris.as_chunks::<3>().0 {
                    let (a, b, c) = (m.positions[t[0] as usize], m.positions[t[1] as usize], m.positions[t[2] as usize]);
                    if let Some(dist) = ray_triangle(o, d, v3(a), v3(b), v3(c))
                        && best.is_none_or(|h| dist < h.t)
                    {
                        best = Some(FaceHit { body: bi, face: range.face, point: o + d * dist, t: dist });
                    }
                }
            }
        }
        best
    }

    pub fn pick_edge(meshes: &[&Mesh], camera: &Camera, w: f64, h: f64, x: f64, y: f64, radius: f64) -> Option<EdgeHit> {
        let (o, d) = camera.ray(x, y, w, h);
        let mut near: Vec<EdgeHit> = Vec::new();
        for (bi, m) in meshes.iter().enumerate() {
            for e in &m.edges {
                for seg in e.points.windows(2) {
                    let (pa, pb) = (v3(seg[0]), v3(seg[1]));
                    let (Some(a), Some(b)) = (camera.project(pa, w, h), camera.project(pb, w, h)) else { continue };
                    // The nearest point of the segment on the screen, and the same in space.
                    let (dx, dy) = (b.0 - a.0, b.1 - a.1);
                    let len2 = dx * dx + dy * dy;
                    let t = if len2 > 0.0 { (((x - a.0) * dx + (y - a.1) * dy) / len2).clamp(0.0, 1.0) } else { 0.0 };
                    let (cx, cy) = (a.0 + dx * t, a.1 + dy * t);
                    let dist = ((x - cx).powi(2) + (y - cy).powi(2)).sqrt();
                    if dist <= radius {
                        let u = pb - pa;
                        let w0 = pa - o;
                        let (uu, ud, dd, uw, dw) = (u.dot(u), u.dot(d), d.dot(d), u.dot(w0), d.dot(w0));
                        let den = uu * dd - ud * ud;
                        let s = if uu <= 0.0 || den.abs() < 1e-12 * uu * dd { 0.0 } else { ((ud * dw - dd * uw) / den).clamp(0.0, 1.0) };
                        near.push(EdgeHit { body: bi, edge: e.edge, point: pa + u * s, pixels: dist });
                    }
                }
            }
        }
        near.sort_by(|a, b| a.pixels.total_cmp(&b.pixels));
        let slack = 1e-3 * camera.distance.max(1.0);
        near.into_iter().find(|hit| match camera.project(hit.point, w, h) {
            Some((px, py, _)) => {
                let (o2, d2) = camera.ray(px, py, w, h);
                !pick_face(meshes, o2, d2).is_some_and(|f| f.t + slack < (hit.point - o2).dot(d2))
            }
            None => true,
        })
    }
}

/// A ring of `sides` flat sides round the z axis at `(cx, cy)`, radius 4 and 12 tall, with a
/// lid: one face per side, an edge between each two, and the rims as polylines of many points.
fn drum(cx: f32, cy: f32, sides: u32) -> Mesh {
    let at = |i: u32, z: f32| {
        let a = std::f32::consts::TAU * (i % sides) as f32 / sides as f32;
        [cx + 4.0 * a.cos(), cy + 4.0 * a.sin(), z]
    };
    let mut m = Mesh::default();
    for i in 0..sides {
        let base = m.positions.len() as u32;
        m.positions.extend([at(i, 0.0), at(i + 1, 0.0), at(i + 1, 12.0), at(i, 12.0)]);
        m.faces.push(FaceRange { face: i, first: m.indices.len() as u32, count: 6 });
        m.indices.extend([base, base + 1, base + 2, base, base + 2, base + 3]);
        m.edges.push(EdgePolyline { edge: i, points: vec![at(i, 0.0), at(i, 12.0)] });
    }
    let base = m.positions.len() as u32;
    m.positions.push([cx, cy, 12.0]);
    m.positions.extend((0..sides).map(|i| at(i, 12.0)));
    m.faces.push(FaceRange { face: sides, first: m.indices.len() as u32, count: 3 * sides });
    for i in 0..sides {
        m.indices.extend([base, base + 1 + i, base + 1 + (i + 1) % sides]);
    }
    for (n, z) in [(0, 0.0), (1, 12.0)] {
        m.edges.push(EdgePolyline { edge: sides + n, points: (0..=sides).map(|i| at(i, z)).collect() });
    }
    m.normals = vec![[0.0, 0.0, 1.0]; m.positions.len()];
    m
}

#[test]
fn picking_with_the_index_gives_what_trying_everything_gives() {
    // A field of drums, as an assembly is: most are nowhere near the ray or the pointer.
    let drums: Vec<Mesh> = (0..36).map(|i| drum((i % 6) as f32 * 14.0, (i / 6) as f32 * 14.0, 24)).collect();
    let meshes: Vec<&Mesh> = drums.iter().collect();
    let (w, h) = (900.0, 600.0);
    let (mut faces, mut edges) = (0, 0);
    for (view, projection) in [(StdView::Home, crate::Projection::Perspective), (StdView::Front, crate::Projection::Orthographic)] {
        let mut c = Camera::default();
        c.set_view(view);
        c.projection = projection;
        c.fit(&Aabb3::new(Vec3::new(-4.0, -4.0, 0.0), Vec3::new(74.0, 74.0, 12.0)));
        // Close in as well, so that some drums are behind the eye and off the screen.
        for zoom in [1.0, 0.25] {
            c.distance *= zoom;
            for i in 0..45 {
                for j in 0..30 {
                    let (x, y) = (f64::from(i) * 20.0 + 3.0, f64::from(j) * 20.0 + 7.0);
                    let (o, d) = c.ray(x, y, w, h);
                    let face = pick_face(&meshes, o, d);
                    assert_eq!(face, slow::pick_face(&meshes, o, d), "{view:?} x{zoom} at {x}, {y}");
                    let edge = pick_edge(&meshes, &c, w, h, x, y, 5.0);
                    assert_eq!(edge, slow::pick_edge(&meshes, &c, w, h, x, y, 5.0), "{view:?} x{zoom} at {x}, {y}");
                    faces += usize::from(face.is_some());
                    edges += usize::from(edge.is_some());
                }
            }
        }
    }
    assert!(faces > 1500 && edges > 400, "{faces} faces, {edges} edges");
}
