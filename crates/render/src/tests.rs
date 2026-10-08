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
    let colors = crate::gpu::BodyColors { face: [0.7, 0.7, 0.7], edge: [0.0; 3], ..Default::default() };
    vp.set_bodies(&device, &[(&m, &colors)]);
    let c = framed(StdView::Home);
    let (_, recreated) = vp.render(&device, &queue, &c, 64, 48, 10.0).unwrap();
    assert!(recreated);
    let (_, again) = vp.render(&device, &queue, &c, 64, 48, 10.0).unwrap();
    assert!(!again, "same size keeps the texture");
    let _ = device.poll(wgpu::PollType::wait_indefinitely());
}

/// Minimal executor for wgpu's futures (they complete immediately on native backends).
fn pollster_block<F: std::future::Future>(f: F) -> F::Output {
    use std::task::{Context, Poll, RawWaker, RawWakerVTable, Waker};
    fn noop(_: *const ()) {}
    fn clone(p: *const ()) -> RawWaker {
        RawWaker::new(p, &VTABLE)
    }
    static VTABLE: RawWakerVTable = RawWakerVTable::new(clone, noop, noop, noop);
    let waker = Waker::noop();
    let _ = &VTABLE;
    let mut cx = Context::from_waker(waker);
    let mut f = std::pin::pin!(f);
    loop {
        if let Poll::Ready(v) = f.as_mut().poll(&mut cx) {
            return v;
        }
        std::thread::yield_now();
    }
}
