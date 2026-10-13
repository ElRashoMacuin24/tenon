//! The interactive viewport renderer (wgpu). Draws shaded faces and edges into an offscreen
//! texture with its own depth buffer and multisampling; the UI shows that texture. Knows nothing
//! about any UI toolkit.
//!
//! A mesh is sent to the graphics card once, however many bodies are drawn from it: the
//! components of one part in an assembly are one set of buffers and a short record each of
//! where it stands and in what colours, drawn in one call (DEC-089).

use std::collections::HashMap;
use std::num::NonZeroU64;
use std::ops::Range;

use tenon_geom::Frame;
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

// A corner of the mesh, in the mesh's own coordinates.
struct VIn {
    @location(0) pos: vec3<f32>,
    @location(1) normal: vec3<f32>,
};
// One body drawn from the mesh: where it stands, and its colours.
struct IIn {
    @location(2) m0: vec4<f32>,
    @location(3) m1: vec4<f32>,
    @location(4) m2: vec4<f32>,
    @location(5) m3: vec4<f32>,
    @location(6) face: vec4<f32>,
    @location(7) edge: vec4<f32>,
};
struct VOut {
    // (Invariant: a highlight drawn over its face must come to the very same depth.)
    @invariant @builtin(position) clip: vec4<f32>,
    @location(0) normal: vec3<f32>,
    @location(1) color: vec3<f32>,
};

fn placed(v: VIn, i: IIn, color: vec3<f32>) -> VOut {
    let model = mat4x4<f32>(i.m0, i.m1, i.m2, i.m3);
    var o: VOut;
    o.clip = u.view_proj * (model * vec4<f32>(v.pos, 1.0));
    o.normal = (model * vec4<f32>(v.normal, 0.0)).xyz;
    o.color = color;
    return o;
}

@vertex
fn vs_face(v: VIn, i: IIn) -> VOut {
    return placed(v, i, i.face.rgb);
}

@vertex
fn vs_line(v: VIn, i: IIn) -> VOut {
    return placed(v, i, i.edge.rgb);
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

/// A corner of a mesh as the card holds it: 24 bytes.
#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct Vertex {
    pos: [f32; 3],
    normal: [f32; 3],
}

/// One body drawn from a mesh: where it stands and its colours. 96 bytes.
#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct Instance {
    model: [[f32; 4]; 4],
    face: [f32; 4],
    edge: [f32; 4],
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

/// A body to draw: its mesh, where it stands (None: where the mesh's own coordinates put it)
/// and its colours. Bodies given the same mesh share its copy on the card.
#[derive(Clone, Copy, Debug)]
pub struct Body<'a> {
    pub mesh: &'a Mesh,
    pub at: Option<&'a Frame>,
    pub colors: &'a BodyColors,
}

impl<'a> Body<'a> {
    pub fn new(mesh: &'a Mesh, at: Option<&'a Frame>, colors: &'a BodyColors) -> Body<'a> {
        Body { mesh, at, colors }
    }

    fn record(&self, face: Color, edge: Color) -> Instance {
        let f = self.at.copied().unwrap_or(Frame::WORLD);
        let col = |v: tenon_geom::Vec3, w: f32| [v.x as f32, v.y as f32, v.z as f32, w];
        Instance {
            model: [col(f.x(), 0.0), col(f.y(), 0.0), col(f.z(), 0.0), col(f.origin(), 1.0)],
            face: [face[0], face[1], face[2], 1.0],
            edge: [edge[0], edge[1], edge[2], 1.0],
        }
    }
}

/// A mesh on the graphics card: its corners once, its triangles as numbers of corners, and
/// its edges as pairs of points.
struct Geometry {
    vertices: wgpu::Buffer,
    indices: wgpu::Buffer,
    index_count: u32,
    lines: wgpu::Buffer,
    line_vertices: u32,
    /// What it takes on the card.
    bytes: u64,
}

/// The bodies drawn from one mesh: a run of the instance records.
struct Group {
    mesh: u64,
    instances: Range<u32>,
}

/// One highlighted face or edge: a stretch of its mesh, drawn again in another colour.
struct Highlight {
    mesh: u64,
    /// Corners of triangles (in the mesh's list of them) or points of edge lines.
    range: Range<u32>,
    lines: bool,
    instance: u32,
}

struct Targets {
    size: (u32, u32),
    msaa: wgpu::TextureView,
    depth: wgpu::TextureView,
    color_texture: wgpu::Texture,
    color: wgpu::TextureView,
}

/// How many bytes of meshes not drawn at the moment are kept on the graphics card, so that
/// panning and zooming an assembly does not send the same meshes again and again.
pub const SPARE_BYTES: u64 = 64 << 20;

/// Offscreen viewport renderer.
pub struct Viewport {
    faces: wgpu::RenderPipeline,
    lines: wgpu::RenderPipeline,
    uniforms: wgpu::Buffer,
    bind: wgpu::BindGroup,
    /// The meshes drawn now, by [`tenon_kernel::index::MeshIndex::id`].
    meshes: HashMap<u64, Geometry>,
    /// Meshes sent and not drawn now, with when each was last drawn.
    spare: HashMap<u64, (u64, Geometry)>,
    /// The most bytes kept aside ([`SPARE_BYTES`] unless set otherwise).
    spare_limit: u64,
    /// Counts calls of `set_bodies`.
    clock: u64,
    /// The bodies in their base colours, mesh by mesh.
    groups: Vec<Group>,
    instances: Option<wgpu::Buffer>,
    /// How many bodies `instances` holds.
    bodies: u32,
    /// Highlighted faces and edges (selection, hover), drawn over the base at the same depth;
    /// small, rebuilt whenever the highlight changes.
    highlights: Vec<Highlight>,
    highlight_instances: Option<wgpu::Buffer>,
    targets: Option<Targets>,
    /// What to draw (visual style): shaded faces, edges.
    show_faces: bool,
    show_edges: bool,
}

/// The colour overrides to draw, one per face or edge: where one is listed twice, the later.
fn last_of_each(overrides: &[(u32, Color)]) -> Vec<(u32, Color)> {
    let mut seen = std::collections::HashSet::new();
    let mut kept: Vec<(u32, Color)> = overrides.iter().rev().filter(|(id, _)| seen.insert(*id)).copied().collect();
    kept.sort_by_key(|(id, _)| *id);
    kept
}

/// The points of the edge lines of a mesh, two for each stretch, edge after edge; and where
/// each edge's begin and end among them.
fn line_points(m: &Mesh) -> (Vec<Vertex>, Vec<(u32, Range<u32>)>) {
    let mut lines = Vec::new();
    let mut ranges = Vec::with_capacity(m.edges.len());
    for e in &m.edges {
        let start = u32::try_from(lines.len()).unwrap_or(u32::MAX);
        for seg in e.points.windows(2) {
            lines.push(Vertex { pos: seg[0], normal: [0.0; 3] });
            lines.push(Vertex { pos: seg[1], normal: [0.0; 3] });
        }
        ranges.push((e.edge, start..u32::try_from(lines.len()).unwrap_or(u32::MAX)));
    }
    (lines, ranges)
}

/// Where each edge's points are among a mesh's edge line points ([`line_points`]), without
/// making them.
fn line_ranges(m: &Mesh) -> Vec<(u32, Range<u32>)> {
    let mut at = 0_u32;
    m.edges
        .iter()
        .map(|e| {
            let start = at;
            at = at.saturating_add(u32::try_from(2 * e.points.len().saturating_sub(1)).unwrap_or(u32::MAX));
            (e.edge, start..at)
        })
        .collect()
}

/// Sends a mesh to the card.
fn upload(device: &wgpu::Device, m: &Mesh) -> Geometry {
    let vertices: Vec<Vertex> =
        m.positions.iter().enumerate().map(|(i, p)| Vertex { pos: *p, normal: m.normals.get(i).copied().unwrap_or([0.0, 0.0, 1.0]) }).collect();
    // (A triangle with a corner that is not there is sent as three times the first corner:
    // nothing is drawn of it, and the triangles after it keep their places in the list.)
    let count = u32::try_from(vertices.len()).unwrap_or(u32::MAX);
    let whole = |t: &[u32; 3]| if t.iter().all(|i| *i < count) { *t } else { [0; 3] };
    let indices: Vec<u32> = if count == 0 { Vec::new() } else { m.indices.as_chunks::<3>().0.iter().flat_map(whole).collect() };
    let (lines, _) = line_points(m);
    let stride = std::mem::size_of::<Vertex>() as u64;
    let make = |contents: &[u8], usage: wgpu::BufferUsages, label: &str| {
        // wgpu rejects empty buffers; keep a few zero bytes and draw none.
        let fallback = [0_u8; 24];
        device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some(label),
            contents: if contents.is_empty() { &fallback } else { contents },
            usage,
        })
    };
    Geometry {
        vertices: make(bytemuck::cast_slice(&vertices), wgpu::BufferUsages::VERTEX, "tenon-corners"),
        indices: make(bytemuck::cast_slice(&indices), wgpu::BufferUsages::INDEX, "tenon-triangles"),
        index_count: u32::try_from(indices.len()).unwrap_or(0),
        lines: make(bytemuck::cast_slice(&lines), wgpu::BufferUsages::VERTEX, "tenon-lines"),
        line_vertices: u32::try_from(lines.len()).unwrap_or(0),
        bytes: stride * (vertices.len() as u64 + lines.len() as u64) + 4 * indices.len() as u64,
    }
}

fn vertex_layouts() -> [Option<wgpu::VertexBufferLayout<'static>>; 2] {
    const CORNER: [wgpu::VertexAttribute; 2] = wgpu::vertex_attr_array![0 => Float32x3, 1 => Float32x3];
    const BODY: [wgpu::VertexAttribute; 6] =
        wgpu::vertex_attr_array![2 => Float32x4, 3 => Float32x4, 4 => Float32x4, 5 => Float32x4, 6 => Float32x4, 7 => Float32x4];
    [
        Some(wgpu::VertexBufferLayout {
            array_stride: std::mem::size_of::<Vertex>() as u64,
            step_mode: wgpu::VertexStepMode::Vertex,
            attributes: &CORNER,
        }),
        Some(wgpu::VertexBufferLayout {
            array_stride: std::mem::size_of::<Instance>() as u64,
            step_mode: wgpu::VertexStepMode::Instance,
            attributes: &BODY,
        }),
    ]
}

fn pipeline(device: &wgpu::Device, layout: &wgpu::PipelineLayout, module: &wgpu::ShaderModule, lines: bool) -> wgpu::RenderPipeline {
    let buffers = vertex_layouts();
    let targets = [Some(wgpu::ColorTargetState { format: COLOR_FORMAT, blend: None, write_mask: wgpu::ColorWrites::ALL })];
    device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: Some(if lines { "tenon-lines" } else { "tenon-faces" }),
        layout: Some(layout),
        vertex: wgpu::VertexState {
            module,
            entry_point: Some(if lines { "vs_line" } else { "vs_face" }),
            compilation_options: Default::default(),
            buffers: &buffers,
        },
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

fn instance_buffer(device: &wgpu::Device, records: &[Instance], label: &str) -> Option<wgpu::Buffer> {
    (!records.is_empty()).then(|| {
        device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some(label),
            contents: bytemuck::cast_slice(records),
            usage: wgpu::BufferUsages::VERTEX,
        })
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
            meshes: HashMap::new(),
            spare: HashMap::new(),
            spare_limit: SPARE_BYTES,
            clock: 0,
            groups: Vec::new(),
            instances: None,
            bodies: 0,
            highlights: Vec::new(),
            highlight_instances: None,
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

    /// Sets what is drawn: the bodies in their base colours, and their highlights. Call when
    /// the bodies, where they stand or their base colours change; for highlight changes alone,
    /// [`Viewport::set_highlights`] is far cheaper. Returns how many meshes were sent to the
    /// card.
    ///
    /// A mesh is sent once however many bodies are drawn from it, and one that is on the card
    /// already is not sent again: moving a component of an assembly sends a record of where it
    /// stands and nothing else. Meshes that were drawn and are not now (their bodies gone off
    /// the screen, or drawn at another level of detail) are kept aside for when they come
    /// back, up to [`SPARE_BYTES`], those left out longest going first.
    pub fn set_bodies(&mut self, device: &wgpu::Device, bodies: &[Body]) -> usize {
        self.clock += 1;
        // Mesh by mesh, in the order each is first met.
        let ids: Vec<u64> = bodies.iter().map(|b| b.mesh.index().id()).collect();
        let mut order: Vec<u64> = Vec::new();
        let mut of_mesh: HashMap<u64, Vec<usize>> = HashMap::new();
        for (i, id) in ids.iter().enumerate() {
            of_mesh
                .entry(*id)
                .or_insert_with(|| {
                    order.push(*id);
                    Vec::new()
                })
                .push(i);
        }
        let mut old = std::mem::take(&mut self.meshes);
        let mut sent = 0;
        let mut records: Vec<Instance> = Vec::with_capacity(bodies.len());
        self.groups.clear();
        for id in order {
            let Some(which) = of_mesh.get(&id) else { continue };
            let Some(first) = which.first().and_then(|i| bodies.get(*i)) else { continue };
            let geometry = old.remove(&id).or_else(|| self.spare.remove(&id).map(|(_, g)| g)).unwrap_or_else(|| {
                sent += 1;
                upload(device, first.mesh)
            });
            self.meshes.insert(id, geometry);
            let start = u32::try_from(records.len()).unwrap_or(u32::MAX);
            records.extend(which.iter().filter_map(|i| bodies.get(*i)).map(|b| b.record(b.colors.face, b.colors.edge)));
            self.groups.push(Group { mesh: id, instances: start..u32::try_from(records.len()).unwrap_or(u32::MAX) });
        }
        self.bodies = u32::try_from(records.len()).unwrap_or(u32::MAX);
        self.instances = instance_buffer(device, &records, "tenon-bodies");
        // What is no longer drawn is kept aside, the longest unused going first when there is
        // too much of it.
        self.spare.extend(old.into_iter().map(|(id, g)| (id, (self.clock, g))));
        let mut held: u64 = self.spare.values().map(|(_, g)| g.bytes).sum();
        if held > self.spare_limit {
            let mut oldest: Vec<(u64, u64, u64)> = self.spare.iter().map(|(id, (at, g))| (*at, *id, g.bytes)).collect();
            oldest.sort_unstable();
            for (_, id, bytes) in oldest {
                if held <= self.spare_limit {
                    break;
                }
                self.spare.remove(&id);
                held -= bytes;
            }
        }
        self.set_highlights(device, bodies);
        sent
    }

    /// Sets how many bytes of meshes not drawn are kept on the graphics card.
    pub fn set_spare_limit(&mut self, bytes: u64) {
        self.spare_limit = bytes;
    }

    /// Sets only the highlighted faces and edges (the per-face and per-edge colours) of the
    /// bodies last given to [`Viewport::set_bodies`]: a short record for each, and nothing of
    /// the meshes sent again.
    pub fn set_highlights(&mut self, device: &wgpu::Device, bodies: &[Body]) {
        let mut records: Vec<Instance> = Vec::new();
        self.highlights.clear();
        for b in bodies.iter().filter(|b| !b.colors.faces.is_empty() || !b.colors.edges.is_empty()) {
            let id = b.mesh.index().id();
            let Some(geometry) = self.meshes.get(&id) else { continue };
            let mut add = |range: Range<u32>, lines: bool, color: Color, records: &mut Vec<Instance>| {
                if range.is_empty() {
                    return;
                }
                let instance = u32::try_from(records.len()).unwrap_or(u32::MAX);
                records.push(b.record(color, color));
                self.highlights.push(Highlight { mesh: id, range, lines, instance });
            };
            // (Face `f` is the mesh's range `f` as the kernel makes them; looked for if it is not.)
            for (f, color) in last_of_each(&b.colors.faces) {
                let within = |r: &tenon_kernel::FaceRange| r.first..r.first.saturating_add(r.count).min(geometry.index_count);
                match b.mesh.faces.get(f as usize).filter(|r| r.face == f) {
                    Some(r) => add(within(r), false, color, &mut records),
                    None => b.mesh.faces.iter().filter(|r| r.face == f).for_each(|r| add(within(r), false, color, &mut records)),
                }
            }
            if !b.colors.edges.is_empty() {
                let ranges = line_ranges(b.mesh);
                for (e, color) in last_of_each(&b.colors.edges) {
                    let within = |r: &Range<u32>| r.start..r.end.min(geometry.line_vertices);
                    match ranges.get(e as usize).filter(|(edge, _)| *edge == e) {
                        Some((_, r)) => add(within(r), true, color, &mut records),
                        None => ranges.iter().filter(|(edge, _)| *edge == e).for_each(|(_, r)| add(within(r), true, color, &mut records)),
                    }
                }
            }
        }
        self.highlight_instances = instance_buffer(device, &records, "tenon-highlights");
    }

    /// What a frame draws: bodies, triangles, and edge segments (highlights not counted).
    pub fn drawn(&self) -> (usize, u64, u64) {
        let (mut triangles, mut segments) = (0_u64, 0_u64);
        for g in &self.groups {
            if let Some(m) = self.meshes.get(&g.mesh) {
                let n = u64::from(g.instances.end.saturating_sub(g.instances.start));
                triangles += n * u64::from(m.index_count / 3);
                segments += n * u64::from(m.line_vertices / 2);
            }
        }
        (self.bodies as usize, if self.show_faces { triangles } else { 0 }, if self.show_edges { segments } else { 0 })
    }

    /// How many meshes are on the graphics card: those drawn, and those kept aside.
    pub fn held_meshes(&self) -> (usize, usize) {
        (self.meshes.len(), self.spare.len())
    }

    /// How many bytes are held on the graphics card: the meshes drawn with the records of
    /// their bodies, and the meshes kept aside.
    pub fn held_bytes(&self) -> (u64, u64) {
        let records = (u64::from(self.bodies) + self.highlights.len() as u64) * std::mem::size_of::<Instance>() as u64;
        (self.meshes.values().map(|g| g.bytes).sum::<u64>() + records, self.spare.values().map(|(_, g)| g.bytes).sum())
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
            // Each mesh once, for all the bodies drawn from it. Highlights come after the base
            // at the same depth (LessEqual), so they replace it.
            for lines in [false, true] {
                if (lines && !self.show_edges) || (!lines && !self.show_faces) {
                    continue;
                }
                pass.set_pipeline(if lines { &self.lines } else { &self.faces });
                if let Some(records) = &self.instances {
                    pass.set_vertex_buffer(1, records.slice(..));
                    for g in &self.groups {
                        let Some(m) = self.meshes.get(&g.mesh) else { continue };
                        if lines && m.line_vertices > 0 {
                            pass.set_vertex_buffer(0, m.lines.slice(..));
                            pass.draw(0..m.line_vertices, g.instances.clone());
                        } else if !lines && m.index_count > 0 {
                            pass.set_vertex_buffer(0, m.vertices.slice(..));
                            pass.set_index_buffer(m.indices.slice(..), wgpu::IndexFormat::Uint32);
                            pass.draw_indexed(0..m.index_count, 0, g.instances.clone());
                        }
                    }
                }
                if let Some(records) = &self.highlight_instances {
                    pass.set_vertex_buffer(1, records.slice(..));
                    for hl in self.highlights.iter().filter(|hl| hl.lines == lines) {
                        let Some(m) = self.meshes.get(&hl.mesh) else { continue };
                        let one = hl.instance..hl.instance.saturating_add(1);
                        if lines {
                            pass.set_vertex_buffer(0, m.lines.slice(..));
                            pass.draw(hl.range.clone(), one);
                        } else {
                            pass.set_vertex_buffer(0, m.vertices.slice(..));
                            pass.set_index_buffer(m.indices.slice(..), wgpu::IndexFormat::Uint32);
                            pass.draw_indexed(hl.range.clone(), 0, one);
                        }
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
