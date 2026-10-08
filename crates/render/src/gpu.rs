//! The interactive viewport renderer (wgpu). Draws shaded faces and edges into an offscreen
//! texture with its own depth buffer and multisampling; the UI shows that texture. Knows nothing
//! about any UI toolkit.

use std::num::NonZeroU64;

use tenon_kernel::Mesh;
use wgpu::util::DeviceExt;

use crate::Camera;

/// Format of the texture handed to the UI. egui-wgpu samples native textures as gamma-encoded
/// `Rgba8Unorm`, so the shaders light in linear space and encode to sRGB themselves (an
/// `...Srgb` target would hand egui linear values and the model would look far too dark).
pub const COLOR_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8Unorm;
const DEPTH_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Depth32Float;
const SAMPLES: u32 = 4;

const SHADER: &str = r#"
struct Uniforms {
    view_proj: mat4x4<f32>,
    light: vec4<f32>,
};
@group(0) @binding(0) var<uniform> u: Uniforms;

struct VIn {
    @location(0) pos: vec3<f32>,
    @location(1) normal: vec3<f32>,
    @location(2) color: vec3<f32>,
};
struct VOut {
    @builtin(position) clip: vec4<f32>,
    @location(0) normal: vec3<f32>,
    @location(1) color: vec3<f32>,
};

@vertex
fn vs_main(v: VIn) -> VOut {
    var o: VOut;
    o.clip = u.view_proj * vec4<f32>(v.pos, 1.0);
    o.normal = v.normal;
    o.color = v.color;
    return o;
}

fn srgb_from_linear(c: vec3<f32>) -> vec3<f32> {
    let lo = c * 12.92;
    let hi = 1.055 * pow(c, vec3<f32>(1.0 / 2.4)) - 0.055;
    return select(hi, lo, c <= vec3<f32>(0.0031308));
}

@fragment
fn fs_face(i: VOut) -> @location(0) vec4<f32> {
    let n = normalize(i.normal);
    let d = abs(dot(n, u.light.xyz));
    // Same shading as raster::shade.
    return vec4<f32>(srgb_from_linear(i.color * (0.35 + 0.65 * d)), 1.0);
}

@fragment
fn fs_line(i: VOut) -> @location(0) vec4<f32> {
    return vec4<f32>(srgb_from_linear(i.color), 1.0);
}
"#;

#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct Vertex {
    pos: [f32; 3],
    normal: [f32; 3],
    color: [f32; 3],
}

#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct Uniforms {
    view_proj: [[f32; 4]; 4],
    light: [f32; 4],
}

/// Linear RGB colour.
pub type Color = [f32; 3];

/// Colours of one body: base, per-face overrides, edges, per-edge overrides.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct BodyColors {
    pub face: Color,
    pub faces: Vec<(u32, Color)>,
    pub edge: Color,
    pub edges: Vec<(u32, Color)>,
}

struct GpuBody {
    faces: wgpu::Buffer,
    face_vertices: u32,
    lines: wgpu::Buffer,
    line_vertices: u32,
}

struct Targets {
    size: (u32, u32),
    msaa: wgpu::TextureView,
    depth: wgpu::TextureView,
    color_texture: wgpu::Texture,
    color: wgpu::TextureView,
}

/// Offscreen viewport renderer.
pub struct Viewport {
    faces: wgpu::RenderPipeline,
    lines: wgpu::RenderPipeline,
    uniforms: wgpu::Buffer,
    bind: wgpu::BindGroup,
    bodies: Vec<GpuBody>,
    targets: Option<Targets>,
    /// What to draw (visual style): shaded faces, edges.
    show_faces: bool,
    show_edges: bool,
}

fn vertex_layout() -> wgpu::VertexBufferLayout<'static> {
    const ATTRS: [wgpu::VertexAttribute; 3] = wgpu::vertex_attr_array![0 => Float32x3, 1 => Float32x3, 2 => Float32x3];
    wgpu::VertexBufferLayout { array_stride: std::mem::size_of::<Vertex>() as u64, step_mode: wgpu::VertexStepMode::Vertex, attributes: &ATTRS }
}

fn pipeline(device: &wgpu::Device, layout: &wgpu::PipelineLayout, module: &wgpu::ShaderModule, lines: bool) -> wgpu::RenderPipeline {
    let buffers = [Some(vertex_layout())];
    let targets = [Some(wgpu::ColorTargetState { format: COLOR_FORMAT, blend: None, write_mask: wgpu::ColorWrites::ALL })];
    device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: Some(if lines { "tenon-lines" } else { "tenon-faces" }),
        layout: Some(layout),
        vertex: wgpu::VertexState { module, entry_point: Some("vs_main"), compilation_options: Default::default(), buffers: &buffers },
        primitive: wgpu::PrimitiveState {
            topology: if lines { wgpu::PrimitiveTopology::LineList } else { wgpu::PrimitiveTopology::TriangleList },
            ..Default::default()
        },
        depth_stencil: Some(wgpu::DepthStencilState {
            format: DEPTH_FORMAT,
            depth_write_enabled: Some(!lines),
            depth_compare: Some(wgpu::CompareFunction::LessEqual),
            stencil: Default::default(),
            // Push faces back a little so their own edges draw on top.
            bias: if lines { Default::default() } else { wgpu::DepthBiasState { constant: 2, slope_scale: 1.5, clamp: 0.0 } },
        }),
        multisample: wgpu::MultisampleState { count: SAMPLES, ..Default::default() },
        fragment: Some(wgpu::FragmentState {
            module,
            entry_point: Some(if lines { "fs_line" } else { "fs_face" }),
            compilation_options: Default::default(),
            targets: &targets,
        }),
        multiview_mask: None,
        cache: None,
    })
}

impl Viewport {
    pub fn new(device: &wgpu::Device) -> Viewport {
        let module = device
            .create_shader_module(wgpu::ShaderModuleDescriptor { label: Some("tenon-viewport"), source: wgpu::ShaderSource::Wgsl(SHADER.into()) });
        let bind_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("tenon-uniforms"),
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Uniform,
                    has_dynamic_offset: false,
                    min_binding_size: NonZeroU64::new(std::mem::size_of::<Uniforms>() as u64),
                },
                count: None,
            }],
        });
        let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("tenon-viewport"),
            bind_group_layouts: &[Some(&bind_layout)],
            immediate_size: 0,
        });
        let uniforms = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("tenon-uniforms"),
            size: std::mem::size_of::<Uniforms>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let bind = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("tenon-uniforms"),
            layout: &bind_layout,
            entries: &[wgpu::BindGroupEntry { binding: 0, resource: uniforms.as_entire_binding() }],
        });
        Viewport {
            faces: pipeline(device, &layout, &module, false),
            lines: pipeline(device, &layout, &module, true),
            uniforms,
            bind,
            bodies: Vec::new(),
            targets: None,
            show_faces: true,
            show_edges: true,
        }
    }

    /// Visual style: shaded faces, edges, or both.
    pub fn set_visible(&mut self, faces: bool, edges: bool) {
        self.show_faces = faces;
        self.show_edges = edges;
    }

    /// Uploads meshes (one per body) with their colours. Call again when colours change.
    pub fn set_bodies(&mut self, device: &wgpu::Device, meshes: &[(&Mesh, &BodyColors)]) {
        self.bodies = meshes
            .iter()
            .map(|(m, colors)| {
                let mut tri = Vec::with_capacity(m.indices.len());
                for range in &m.faces {
                    let color = colors.faces.iter().find(|(f, _)| *f == range.face).map_or(colors.face, |c| c.1);
                    let idx = m.indices.get(range.first as usize..(range.first as usize).saturating_add(range.count as usize)).unwrap_or(&[]);
                    for i in idx {
                        let (Some(p), n) = (m.positions.get(*i as usize), m.normals.get(*i as usize).copied().unwrap_or([0.0, 0.0, 1.0])) else {
                            continue;
                        };
                        tri.push(Vertex { pos: *p, normal: n, color });
                    }
                }
                let mut lines = Vec::new();
                for e in &m.edges {
                    let color = colors.edges.iter().find(|(id, _)| *id == e.edge).map_or(colors.edge, |c| c.1);
                    for seg in e.points.windows(2) {
                        lines.push(Vertex { pos: seg[0], normal: [0.0; 3], color });
                        lines.push(Vertex { pos: seg[1], normal: [0.0; 3], color });
                    }
                }
                let make = |data: &[Vertex], label: &str| {
                    // wgpu rejects empty buffers; keep one zero vertex and draw none.
                    let fallback = [Vertex { pos: [0.0; 3], normal: [0.0; 3], color: [0.0; 3] }];
                    let contents: &[Vertex] = if data.is_empty() { &fallback } else { data };
                    device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                        label: Some(label),
                        contents: bytemuck::cast_slice(contents),
                        usage: wgpu::BufferUsages::VERTEX,
                    })
                };
                GpuBody {
                    faces: make(&tri, "tenon-faces"),
                    face_vertices: u32::try_from(tri.len()).unwrap_or(0),
                    lines: make(&lines, "tenon-lines"),
                    line_vertices: u32::try_from(lines.len()).unwrap_or(0),
                }
            })
            .collect();
    }

    fn ensure_targets(&mut self, device: &wgpu::Device, w: u32, h: u32) {
        if self.targets.as_ref().is_some_and(|t| t.size == (w, h)) {
            return;
        }
        let make = |format, samples, usage, label| {
            device.create_texture(&wgpu::TextureDescriptor {
                label: Some(label),
                size: wgpu::Extent3d { width: w, height: h, depth_or_array_layers: 1 },
                mip_level_count: 1,
                sample_count: samples,
                dimension: wgpu::TextureDimension::D2,
                format,
                usage,
                view_formats: &[],
            })
        };
        let view = |t: wgpu::Texture| t.create_view(&Default::default());
        let color_texture = make(
            COLOR_FORMAT,
            1,
            wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_SRC,
            "tenon-color",
        );
        self.targets = Some(Targets {
            size: (w, h),
            msaa: view(make(COLOR_FORMAT, SAMPLES, wgpu::TextureUsages::RENDER_ATTACHMENT, "tenon-msaa")),
            depth: view(make(DEPTH_FORMAT, SAMPLES, wgpu::TextureUsages::RENDER_ATTACHMENT, "tenon-depth")),
            color: color_texture.create_view(&Default::default()),
            color_texture,
        });
    }

    /// Renders and returns the colour texture (transparent where nothing was drawn) and whether
    /// it was recreated (the UI must re-register a new texture). `radius` bounds the scene for
    /// the depth range.
    pub fn render(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        camera: &Camera,
        w: u32,
        h: u32,
        radius: f64,
    ) -> Option<(&wgpu::TextureView, bool)> {
        let (w, h) = (w.clamp(1, 8192), h.clamp(1, 8192));
        let recreated = self.targets.as_ref().is_none_or(|t| t.size != (w, h));
        self.ensure_targets(device, w, h);
        let f = camera.key_light();
        let u = Uniforms { view_proj: camera.view_proj(f64::from(w) / f64::from(h), radius), light: [f.x as f32, f.y as f32, f.z as f32, 0.0] };
        queue.write_buffer(&self.uniforms, 0, bytemuck::bytes_of(&u));
        let mut enc = device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: Some("tenon-viewport") });
        if let Some(t) = &self.targets {
            let mut pass = enc.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("tenon-viewport"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &t.msaa,
                    depth_slice: None,
                    resolve_target: Some(&t.color),
                    ops: wgpu::Operations { load: wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT), store: wgpu::StoreOp::Discard },
                })],
                depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                    view: &t.depth,
                    depth_ops: Some(wgpu::Operations { load: wgpu::LoadOp::Clear(1.0), store: wgpu::StoreOp::Discard }),
                    stencil_ops: None,
                }),
                ..Default::default()
            });
            pass.set_bind_group(0, &self.bind, &[]);
            if self.show_faces {
                pass.set_pipeline(&self.faces);
                for b in &self.bodies {
                    if b.face_vertices > 0 {
                        pass.set_vertex_buffer(0, b.faces.slice(..));
                        pass.draw(0..b.face_vertices, 0..1);
                    }
                }
            }
            if self.show_edges {
                pass.set_pipeline(&self.lines);
                for b in &self.bodies {
                    if b.line_vertices > 0 {
                        pass.set_vertex_buffer(0, b.lines.slice(..));
                        pass.draw(0..b.line_vertices, 0..1);
                    }
                }
            }
        }
        queue.submit([enc.finish()]);
        self.targets.as_ref().map(|t| (&t.color, recreated))
    }

    /// The last rendered image as tightly packed RGBA rows, top to bottom (gamma-encoded, with
    /// premultiplied alpha). Blocks until the GPU has finished; for tests and screenshots.
    pub fn read_pixels(&self, device: &wgpu::Device, queue: &wgpu::Queue) -> Option<(u32, u32, Vec<u8>)> {
        let t = self.targets.as_ref()?;
        let (w, h) = t.size;
        // Rows of a texture-to-buffer copy are padded to 256 bytes.
        let row = w * 4;
        let padded = row.div_ceil(wgpu::COPY_BYTES_PER_ROW_ALIGNMENT) * wgpu::COPY_BYTES_PER_ROW_ALIGNMENT;
        let buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("tenon-readback"),
            size: u64::from(padded) * u64::from(h),
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });
        let mut enc = device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: Some("tenon-readback") });
        enc.copy_texture_to_buffer(
            t.color_texture.as_image_copy(),
            wgpu::TexelCopyBufferInfo {
                buffer: &buffer,
                layout: wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(padded), rows_per_image: Some(h) },
            },
            wgpu::Extent3d { width: w, height: h, depth_or_array_layers: 1 },
        );
        queue.submit([enc.finish()]);
        let slice = buffer.slice(..);
        let (tx, rx) = std::sync::mpsc::channel();
        slice.map_async(wgpu::MapMode::Read, move |r| {
            let _ = tx.send(r);
        });
        device.poll(wgpu::PollType::wait_indefinitely()).ok()?;
        rx.recv().ok()?.ok()?;
        let data = slice.get_mapped_range().ok()?;
        let mut out = Vec::with_capacity((row * h) as usize);
        for r in 0..h as usize {
            out.extend_from_slice(data.get(r * padded as usize..r * padded as usize + row as usize)?);
        }
        drop(data);
        buffer.unmap();
        Some((w, h, out))
    }
}
