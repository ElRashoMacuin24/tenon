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
    Mesh { normals: vec![[0.0, 0.0, 1.0]; 8], positions, indices, faces, edges }
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
    // Highlighting a face changes pixels.
    let lit = render(&[&m], &c, 320, 240, &Style { face_colors: vec![(0, 1, [255, 0, 0, 255])], ..Style::default() });
    assert_ne!(lit.rgba, img.rgba);
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
