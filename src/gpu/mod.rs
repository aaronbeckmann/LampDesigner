//! GPU renderer for the interactive 3D view.
//!
//! Runs on its own wgpu device on a render thread: the UI sends the latest
//! [`Request`] (scene, simulation result, camera and settings), the thread
//! renders it and publishes the picture through a [`rune::Pixels`], which
//! redraws only that element. Newer requests replace older ones that have not
//! started. Without a usable GPU the thread falls back to the CPU ray tracer
//! (`crate::render`), which also stays in use for saved still images.

use crate::bulb::Bulb;
use crate::material::Optics;
use crate::mesh::Tag;
use crate::render::{self, Camera, RenderSettings, ViewMode};
use crate::scene::Scene;
use crate::sim::{Map2, SimResult};
use glam::{Mat4, Vec3};
use std::ops::Range;
use std::sync::{mpsc, Arc, Mutex};
use std::time::Instant;
use wgpu::util::DeviceExt;

const HDR: wgpu::TextureFormat = wgpu::TextureFormat::Rgba16Float;
const DEPTH: wgpu::TextureFormat = wgpu::TextureFormat::Depth32Float;
const OUT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8Unorm;

// extra scene parts after the mesh tags (see scene.wgsl)
const TAG_BULB: u32 = 5;
const TAG_TABLE: u32 = 6;
const TAG_WALL: u32 = 7;

/// What one frame shows.
#[derive(Clone)]
pub struct FrameParams {
    pub width: u32,
    pub height: u32,
    /// Supersampling factor per axis (1 = off).
    pub ssaa: u32,
    pub cam: Camera,
    pub rs: RenderSettings,
    /// Exposure for the physical views (before `rs.ev`).
    pub exposure: f32,
    pub optics: Optics,
    pub bulb: Bulb,
    /// Softening point of the material (temperature colour scale).
    pub hdt: f32,
}

pub struct Request {
    pub scene: Arc<Scene>,
    pub sim: Arc<SimResult>,
    pub params: FrameParams,
}

/// Handle the UI keeps: send requests, show `pixels`.
pub struct GpuView {
    tx: mpsc::Sender<Request>,
    pub pixels: rune::Pixels,
    status: Arc<Mutex<Status>>,
}

/// What the render thread reports back.
#[derive(Clone, Debug, Default)]
pub struct Status {
    /// "GPU (adapter name)" or "CPU fallback (reason)".
    pub backend: String,
    pub last_ms: f32,
    pub frames: u64,
}

impl GpuView {
    pub fn new() -> GpuView {
        let (tx, rx) = mpsc::channel::<Request>();
        let pixels = rune::Pixels::new();
        let status = Arc::new(Mutex::new(Status { backend: "starting…".into(), ..Default::default() }));
        let (px, st) = (pixels.clone(), Arc::clone(&status));
        std::thread::Builder::new()
            .name("lampdesigner-render".into())
            .spawn(move || render_thread(rx, px, st))
            .expect("render thread");
        GpuView { tx, pixels, status }
    }

    /// Renders this frame next (dropping older pending requests).
    pub fn request(&self, r: Request) {
        let _ = self.tx.send(r);
    }

    pub fn status(&self) -> Status {
        self.status.lock().map(|s| s.clone()).unwrap_or_default()
    }
}

impl Default for GpuView {
    fn default() -> Self {
        GpuView::new()
    }
}

fn render_thread(rx: mpsc::Receiver<Request>, pixels: rune::Pixels, status: Arc<Mutex<Status>>) {
    let mut gpu = match Renderer::new() {
        Ok(r) => {
            set_status(&status, |s| s.backend = format!("GPU ({})", r.adapter_name));
            Some(r)
        }
        Err(e) => {
            set_status(&status, |s| s.backend = format!("CPU fallback ({e})"));
            None
        }
    };
    while let Ok(mut req) = rx.recv() {
        // latest wins
        while let Ok(newer) = rx.try_recv() {
            req = newer;
        }
        let t0 = Instant::now();
        let p = &req.params;
        let rgba = match gpu.as_mut().map(|g| g.render(&req)) {
            Some(Ok(rgba)) => rgba,
            failed => {
                if let Some(Err(e)) = failed {
                    set_status(&status, |s| s.backend = format!("CPU fallback ({e})"));
                    gpu = None;
                }
                let rs = RenderSettings { samples: p.ssaa.clamp(1, 2), ..p.rs };
                render::render(&req.scene, &req.sim, &p.optics, &p.bulb, p.hdt, &p.cam, &rs, p.width as usize, p.height as usize, Some(p.exposure)).rgba
            }
        };
        pixels.set(p.width, p.height, rgba);
        let ms = t0.elapsed().as_secs_f32() * 1000.0;
        set_status(&status, |s| {
            s.last_ms = ms;
            s.frames += 1;
        });
    }
}

fn set_status(status: &Mutex<Status>, f: impl FnOnce(&mut Status)) {
    if let Ok(mut s) = status.lock() {
        f(&mut s);
    }
}

#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct StaticVert {
    pos: [f32; 3],
    nrm: [f32; 3],
    tag: u32,
    thick: f32,
    over: f32,
}

#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct DynVert {
    emit: [f32; 3],
    temp: f32,
}

#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct Uniforms {
    view_proj: [[f32; 4]; 4],
    eye: [f32; 4],
    key: [f32; 4],
    room: [f32; 4],
    bulb_c: [f32; 4],
    bulb_col: [f32; 4],
    l_bulb: [f32; 4],
    km: [f32; 4],
    km2: [f32; 4],
    clay: [f32; 4],
    clip: [f32; 4],
    clip_on: [f32; 4],
    albedo: [f32; 4],
    table_rect: [f32; 4],
    wall_rect: [f32; 4],
    planes: [f32; 4],
}

/// Index ranges of the scene parts in the index buffer.
#[derive(Clone, Default)]
struct Parts {
    shell: Range<u32>,
    solids: Range<u32>,
    bulb: Range<u32>,
    table: Range<u32>,
    wall: Range<u32>,
}

struct SceneBuffers {
    /// Keeps the scene alive, so `Arc::ptr_eq` reliably says "same scene".
    scene: Arc<Scene>,
    statics: wgpu::Buffer,
    indices: wgpu::Buffer,
    parts: Parts,
    vertex_count: usize,
}

struct SimBuffers {
    sim: Arc<SimResult>,
    dynamic: wgpu::Buffer,
    group0: wgpu::BindGroup,
    table_rect: [f32; 4],
    wall_rect: [f32; 4],
}

struct Targets {
    key: (u32, u32, u32),
    hdr: wgpu::Texture,
    depth: wgpu::TextureView,
    shell_depth: wgpu::TextureView,
    out: wgpu::Texture,
    readback: wgpu::Buffer,
    padded_row: u32,
    group1: wgpu::BindGroup,
    tonemap_group: wgpu::BindGroup,
}

struct Renderer {
    adapter_name: String,
    device: wgpu::Device,
    queue: wgpu::Queue,
    bgl0: wgpu::BindGroupLayout,
    bgl1: wgpu::BindGroupLayout,
    bgl_tonemap: wgpu::BindGroupLayout,
    opaque: wgpu::RenderPipeline,
    prepass: wgpu::RenderPipeline,
    far: wgpu::RenderPipeline,
    near: wgpu::RenderPipeline,
    tonemap: wgpu::RenderPipeline,
    uniforms: wgpu::Buffer,
    tonemap_uniforms: wgpu::Buffer,
    scene: Option<SceneBuffers>,
    sim: Option<SimBuffers>,
    targets: Option<Targets>,
}

fn vertex_layouts() -> [wgpu::VertexBufferLayout<'static>; 2] {
    const STATIC: [wgpu::VertexAttribute; 5] =
        wgpu::vertex_attr_array![0 => Float32x3, 1 => Float32x3, 2 => Uint32, 3 => Float32, 4 => Float32];
    const DYNAMIC: [wgpu::VertexAttribute; 2] = wgpu::vertex_attr_array![5 => Float32x3, 6 => Float32];
    [
        wgpu::VertexBufferLayout {
            array_stride: std::mem::size_of::<StaticVert>() as u64,
            step_mode: wgpu::VertexStepMode::Vertex,
            attributes: &STATIC,
        },
        wgpu::VertexBufferLayout {
            array_stride: std::mem::size_of::<DynVert>() as u64,
            step_mode: wgpu::VertexStepMode::Vertex,
            attributes: &DYNAMIC,
        },
    ]
}

/// dst = src.rgb + dst.rgb * src.a (src.a = transmittance of the wall)
const TRANSMIT_BLEND: wgpu::BlendState = wgpu::BlendState {
    color: wgpu::BlendComponent {
        src_factor: wgpu::BlendFactor::One,
        dst_factor: wgpu::BlendFactor::SrcAlpha,
        operation: wgpu::BlendOperation::Add,
    },
    alpha: wgpu::BlendComponent {
        src_factor: wgpu::BlendFactor::Zero,
        dst_factor: wgpu::BlendFactor::One,
        operation: wgpu::BlendOperation::Add,
    },
};

impl Renderer {
    fn new() -> Result<Renderer, String> {
        let instance = wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle());
        let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
            power_preference: wgpu::PowerPreference::HighPerformance,
            force_fallback_adapter: false,
            compatible_surface: None,
            apply_limit_buckets: false,
        }))
        .map_err(|e| format!("no GPU adapter: {e}"))?;
        let adapter_name = adapter.get_info().name;
        let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
            label: Some("lampdesigner"),
            required_features: wgpu::Features::empty(),
            required_limits: wgpu::Limits::default().using_resolution(adapter.limits()),
            experimental_features: Default::default(),
            memory_hints: wgpu::MemoryHints::MemoryUsage,
            trace: Default::default(),
        }))
        .map_err(|e| format!("no GPU device: {e}"))?;

        let scene_module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("scene"),
            source: wgpu::ShaderSource::Wgsl(include_str!("scene.wgsl").into()),
        });
        let tonemap_module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("tonemap"),
            source: wgpu::ShaderSource::Wgsl(include_str!("tonemap.wgsl").into()),
        });

        let tex_entry = |binding: u32, stage: wgpu::ShaderStages| wgpu::BindGroupLayoutEntry {
            binding,
            visibility: stage,
            ty: wgpu::BindingType::Texture {
                sample_type: wgpu::TextureSampleType::Float { filterable: false },
                view_dimension: wgpu::TextureViewDimension::D2,
                multisampled: false,
            },
            count: None,
        };
        let uniform_entry = |binding: u32, stage: wgpu::ShaderStages| wgpu::BindGroupLayoutEntry {
            binding,
            visibility: stage,
            ty: wgpu::BindingType::Buffer { ty: wgpu::BufferBindingType::Uniform, has_dynamic_offset: false, min_binding_size: None },
            count: None,
        };
        let vf = wgpu::ShaderStages::VERTEX_FRAGMENT;
        let bgl0 = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("scene"),
            entries: &[uniform_entry(0, vf), tex_entry(1, vf), tex_entry(2, vf)],
        });
        let bgl1 = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("shell depth"),
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Texture {
                    sample_type: wgpu::TextureSampleType::Depth,
                    view_dimension: wgpu::TextureViewDimension::D2,
                    multisampled: false,
                },
                count: None,
            }],
        });
        let bgl_tonemap = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("tonemap"),
            entries: &[tex_entry(0, wgpu::ShaderStages::FRAGMENT), uniform_entry(1, wgpu::ShaderStages::FRAGMENT)],
        });
        let layout0 = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("scene"),
            bind_group_layouts: &[Some(&bgl0)],
            immediate_size: 0,
        });
        let layout1 = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("scene translucent"),
            bind_group_layouts: &[Some(&bgl0), Some(&bgl1)],
            immediate_size: 0,
        });
        let layout_tonemap = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("tonemap"),
            bind_group_layouts: &[Some(&bgl_tonemap)],
            immediate_size: 0,
        });

        let buffers = vertex_layouts();
        let depth_state = |write: bool, compare: wgpu::CompareFunction| wgpu::DepthStencilState {
            format: DEPTH,
            depth_write_enabled: Some(write),
            depth_compare: Some(compare),
            stencil: Default::default(),
            bias: Default::default(),
        };
        let scene_pipeline = |label: &str,
                              layout: &wgpu::PipelineLayout,
                              fs: Option<&str>,
                              cull: Option<wgpu::Face>,
                              depth: wgpu::DepthStencilState,
                              blend: Option<wgpu::BlendState>| {
            let targets = [Some(wgpu::ColorTargetState { format: HDR, blend, write_mask: wgpu::ColorWrites::ALL })];
            device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some(label),
                layout: Some(layout),
                vertex: wgpu::VertexState {
                    module: &scene_module,
                    entry_point: Some("vs"),
                    compilation_options: Default::default(),
                    buffers: &[Some(buffers[0].clone()), Some(buffers[1].clone())],
                },
                fragment: fs.map(|entry| wgpu::FragmentState {
                    module: &scene_module,
                    entry_point: Some(entry),
                    compilation_options: Default::default(),
                    targets: &targets,
                }),
                primitive: wgpu::PrimitiveState { cull_mode: cull, ..Default::default() },
                depth_stencil: Some(depth),
                multisample: Default::default(),
                multiview_mask: None,
                cache: None,
            })
        };
        use wgpu::CompareFunction as C;
        let opaque = scene_pipeline("opaque", &layout0, Some("fs_opaque"), None, depth_state(true, C::Less), None);
        let prepass = scene_pipeline("shell prepass", &layout0, None, Some(wgpu::Face::Back), depth_state(true, C::Less), None);
        let far = scene_pipeline("shell far", &layout1, Some("fs_far"), Some(wgpu::Face::Back), depth_state(true, C::Less), Some(TRANSMIT_BLEND));
        let near =
            scene_pipeline("shell near", &layout1, Some("fs_near"), Some(wgpu::Face::Back), depth_state(false, C::LessEqual), Some(TRANSMIT_BLEND));
        let tonemap_targets = [Some(wgpu::ColorTargetState { format: OUT, blend: None, write_mask: wgpu::ColorWrites::ALL })];
        let tonemap = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("tonemap"),
            layout: Some(&layout_tonemap),
            vertex: wgpu::VertexState {
                module: &tonemap_module,
                entry_point: Some("vs"),
                compilation_options: Default::default(),
                buffers: &[],
            },
            fragment: Some(wgpu::FragmentState {
                module: &tonemap_module,
                entry_point: Some("fs"),
                compilation_options: Default::default(),
                targets: &tonemap_targets,
            }),
            primitive: Default::default(),
            depth_stencil: None,
            multisample: Default::default(),
            multiview_mask: None,
            cache: None,
        });

        let uniforms = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("uniforms"),
            size: std::mem::size_of::<Uniforms>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let tonemap_uniforms = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("tonemap uniforms"),
            size: 16,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        Ok(Renderer {
            adapter_name,
            device,
            queue,
            bgl0,
            bgl1,
            bgl_tonemap,
            opaque,
            prepass,
            far,
            near,
            tonemap,
            uniforms,
            tonemap_uniforms,
            scene: None,
            sim: None,
            targets: None,
        })
    }

    /// Uploads the scene mesh (plus bulb and room planes) when it changed.
    fn ensure_scene(&mut self, scene: &Arc<Scene>) {
        if self.scene.as_ref().is_some_and(|s| Arc::ptr_eq(&s.scene, scene)) {
            return;
        }
        let m = &scene.mesh;
        let nv = m.pos.len();
        // a vertex belongs to its lowest tag: outer/inner over the rims that share it
        let mut vtag = vec![u32::MAX; nv];
        for (t, &tag) in m.tris.iter().zip(&m.tags) {
            for &v in t {
                vtag[v as usize] = vtag[v as usize].min(tag as u32);
            }
        }
        let mut verts: Vec<StaticVert> = (0..nv)
            .map(|v| StaticVert {
                pos: m.pos[v].to_array(),
                nrm: scene.vnormal[v].to_array(),
                tag: vtag[v].min(Tag::Proxy as u32),
                thick: scene.thickness[v],
                over: scene.overhang[v],
            })
            .collect();
        let mut by_group: [Vec<u32>; 2] = [Vec::new(), Vec::new()];
        for (t, &tag) in m.tris.iter().zip(&m.tags) {
            let g = usize::from(!matches!(tag, Tag::Outer | Tag::Inner | Tag::Rim));
            by_group[g].extend_from_slice(t);
        }
        let mut indices: Vec<u32> = Vec::with_capacity(m.tris.len() * 3 + 4096);
        let mut parts = Parts::default();
        let push_group = |indices: &mut Vec<u32>, idx: &[u32]| -> Range<u32> {
            let start = indices.len() as u32;
            indices.extend_from_slice(idx);
            start..indices.len() as u32
        };
        parts.shell = push_group(&mut indices, &by_group[0]);
        parts.solids = push_group(&mut indices, &by_group[1]);
        // bulb: ellipsoid
        let (bv, bi) = ellipsoid(scene.bulb_c, scene.bulb_r, verts.len() as u32);
        verts.extend(bv);
        parts.bulb = push_group(&mut indices, &bi);
        // room planes, large enough to reach the horizon from any sensible view
        let r = 4000.0;
        let quad = |verts: &mut Vec<StaticVert>, c: [Vec3; 4], tag: u32, n: Vec3| -> Vec<u32> {
            let b = verts.len() as u32;
            for p in c {
                verts.push(StaticVert { pos: p.to_array(), nrm: n.to_array(), tag, thick: 0.0, over: 0.0 });
            }
            vec![b, b + 1, b + 2, b, b + 2, b + 3]
        };
        let (zt, wy) = (scene.z_table, scene.wall_y);
        let ti = quad(
            &mut verts,
            [Vec3::new(-r, -r, zt), Vec3::new(r, -r, zt), Vec3::new(r, wy, zt), Vec3::new(-r, wy, zt)],
            TAG_TABLE,
            Vec3::Z,
        );
        parts.table = push_group(&mut indices, &ti);
        let wi = quad(
            &mut verts,
            [Vec3::new(-r, wy, zt), Vec3::new(r, wy, zt), Vec3::new(r, wy, zt + r), Vec3::new(-r, wy, zt + r)],
            TAG_WALL,
            -Vec3::Y,
        );
        parts.wall = push_group(&mut indices, &wi);

        let statics = self.device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("vertices"),
            contents: bytemuck::cast_slice(&verts),
            usage: wgpu::BufferUsages::VERTEX,
        });
        let indices = self.device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("indices"),
            contents: bytemuck::cast_slice(&indices),
            usage: wgpu::BufferUsages::INDEX,
        });
        self.scene = Some(SceneBuffers { scene: Arc::clone(scene), statics, indices, parts, vertex_count: verts.len() });
        // per-vertex simulation data belongs to the scene: upload it again
        self.sim = None;
    }

    /// Uploads per-vertex luminance / temperature and the light maps when the result changed.
    fn ensure_sim(&mut self, sim: &Arc<SimResult>) {
        if self.sim.as_ref().is_some_and(|s| Arc::ptr_eq(&s.sim, sim)) {
            return;
        }
        let Some(sb) = &self.scene else { return };
        let n = sim.l_emit.len().min(sim.temp.len());
        let dynamic: Vec<DynVert> = (0..sb.vertex_count)
            .map(|v| {
                if v < n {
                    DynVert { emit: sim.l_emit[v].to_array(), temp: sim.temp[v] }
                } else {
                    DynVert { emit: [0.0; 3], temp: crate::sim::AMBIENT_C }
                }
            })
            .collect();
        let dynamic = self.device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("simulation"),
            contents: bytemuck::cast_slice(&dynamic),
            usage: wgpu::BufferUsages::VERTEX,
        });
        let table = self.map_texture(&sim.table, "table light");
        let wall = self.map_texture(&sim.wall, "wall light");
        let group0 = self.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("scene"),
            layout: &self.bgl0,
            entries: &[
                wgpu::BindGroupEntry { binding: 0, resource: self.uniforms.as_entire_binding() },
                wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::TextureView(&table) },
                wgpu::BindGroupEntry { binding: 2, resource: wgpu::BindingResource::TextureView(&wall) },
            ],
        });
        let rect = |m: &Map2| [m.x0, m.x1, m.y0, m.y1];
        self.sim =
            Some(SimBuffers { sim: Arc::clone(sim), dynamic, group0, table_rect: rect(&sim.table), wall_rect: rect(&sim.wall) });
    }

    fn map_texture(&self, m: &Map2, label: &str) -> wgpu::TextureView {
        let data: Vec<[f32; 4]> = m.data.iter().map(|c| [c.x, c.y, c.z, 1.0]).collect();
        let tex = self.device.create_texture_with_data(
            &self.queue,
            &wgpu::TextureDescriptor {
                label: Some(label),
                size: wgpu::Extent3d { width: m.nx as u32, height: m.ny as u32, depth_or_array_layers: 1 },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: wgpu::TextureFormat::Rgba32Float,
                usage: wgpu::TextureUsages::TEXTURE_BINDING,
                view_formats: &[],
            },
            wgpu::util::TextureDataOrder::LayerMajor,
            bytemuck::cast_slice(&data),
        );
        tex.create_view(&Default::default())
    }

    fn ensure_targets(&mut self, w: u32, h: u32, ss: u32) {
        if self.targets.as_ref().is_some_and(|t| t.key == (w, h, ss)) {
            return;
        }
        let tex = |label: &str, w: u32, h: u32, format: wgpu::TextureFormat, usage: wgpu::TextureUsages| {
            self.device.create_texture(&wgpu::TextureDescriptor {
                label: Some(label),
                size: wgpu::Extent3d { width: w, height: h, depth_or_array_layers: 1 },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format,
                usage,
                view_formats: &[],
            })
        };
        use wgpu::TextureUsages as U;
        let (sw, sh) = (w * ss, h * ss);
        let hdr = tex("hdr", sw, sh, HDR, U::RENDER_ATTACHMENT | U::TEXTURE_BINDING);
        let depth = tex("depth", sw, sh, DEPTH, U::RENDER_ATTACHMENT).create_view(&Default::default());
        let shell_depth = tex("shell depth", sw, sh, DEPTH, U::RENDER_ATTACHMENT | U::TEXTURE_BINDING).create_view(&Default::default());
        let out = tex("out", w, h, OUT, U::RENDER_ATTACHMENT | U::COPY_SRC);
        let padded_row = (w * 4).div_ceil(wgpu::COPY_BYTES_PER_ROW_ALIGNMENT) * wgpu::COPY_BYTES_PER_ROW_ALIGNMENT;
        let readback = self.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("readback"),
            size: u64::from(padded_row) * u64::from(h),
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });
        let group1 = self.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("shell depth"),
            layout: &self.bgl1,
            entries: &[wgpu::BindGroupEntry { binding: 0, resource: wgpu::BindingResource::TextureView(&shell_depth) }],
        });
        let hdr_view = hdr.create_view(&Default::default());
        let tonemap_group = self.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("tonemap"),
            layout: &self.bgl_tonemap,
            entries: &[
                wgpu::BindGroupEntry { binding: 0, resource: wgpu::BindingResource::TextureView(&hdr_view) },
                wgpu::BindGroupEntry { binding: 1, resource: self.tonemap_uniforms.as_entire_binding() },
            ],
        });
        self.targets = Some(Targets { key: (w, h, ss), hdr, depth, shell_depth, out, readback, padded_row, group1, tonemap_group });
    }

    fn render(&mut self, req: &Request) -> Result<Vec<u8>, String> {
        let p = &req.params;
        let (w, h, ss) = (p.width.max(1), p.height.max(1), p.ssaa.clamp(1, 4));
        self.ensure_scene(&req.scene);
        self.ensure_sim(&req.sim);
        self.ensure_targets(w, h, ss);
        let (Some(sb), Some(sim), Some(t)) = (&self.scene, &self.sim, &self.targets) else {
            return Err("scene not uploaded".into());
        };

        let mode = p.rs.mode;
        let physical = matches!(mode, ViewMode::Night | ViewMode::Day);
        let translucent = physical && p.rs.clip.is_none();
        self.queue.write_buffer(&self.uniforms, 0, bytemuck::bytes_of(&uniforms(req, w, h, sim)));
        let kind = match mode {
            ViewMode::Night | ViewMode::Day => 0.0,
            ViewMode::Shape => 1.0,
            _ => 2.0,
        };
        let k = if kind == 2.0 { 1.0 } else { 2f32.powf(p.rs.ev) * if kind == 0.0 { p.exposure } else { 1.0 } };
        self.queue.write_buffer(&self.tonemap_uniforms, 0, bytemuck::cast_slice(&[k, kind, ss as f32, 0.0]));

        let clear = match mode {
            ViewMode::Night | ViewMode::Day => {
                let b = p.rs.room_lux * 0.3 / std::f32::consts::PI;
                wgpu::Color { r: f64::from(0.9 * b), g: f64::from(0.95 * b), b: f64::from(b), a: 1.0 }
            }
            ViewMode::Shape => wgpu::Color { r: 0.05, g: 0.055, b: 0.066, a: 1.0 },
            _ => wgpu::Color { r: 0.12, g: 0.12, b: 0.12, a: 1.0 },
        };
        let hdr_view = t.hdr.create_view(&Default::default());
        let mut enc = self.device.create_command_encoder(&Default::default());
        let draw = |pass: &mut wgpu::RenderPass<'_>, r: &Range<u32>| {
            if !r.is_empty() {
                pass.draw_indexed(r.clone(), 0, 0..1);
            }
        };
        let bind = |pass: &mut wgpu::RenderPass<'_>| {
            pass.set_vertex_buffer(0, sb.statics.slice(..));
            pass.set_vertex_buffer(1, sim.dynamic.slice(..));
            pass.set_index_buffer(sb.indices.slice(..), wgpu::IndexFormat::Uint32);
            pass.set_bind_group(0, &sim.group0, &[]);
        };
        {
            let mut pass = enc.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("opaque"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &hdr_view,
                    resolve_target: None,
                    depth_slice: None,
                    ops: wgpu::Operations { load: wgpu::LoadOp::Clear(clear), store: wgpu::StoreOp::Store },
                })],
                depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                    view: &t.depth,
                    depth_ops: Some(wgpu::Operations { load: wgpu::LoadOp::Clear(1.0), store: wgpu::StoreOp::Store }),
                    stencil_ops: None,
                }),
                ..Default::default()
            });
            pass.set_pipeline(&self.opaque);
            bind(&mut pass);
            draw(&mut pass, &sb.parts.solids);
            draw(&mut pass, &sb.parts.bulb);
            if !translucent {
                draw(&mut pass, &sb.parts.shell);
            }
            if p.rs.show_room && matches!(mode, ViewMode::Night | ViewMode::Day | ViewMode::Shape) {
                draw(&mut pass, &sb.parts.table);
            }
            if p.rs.show_room && physical {
                draw(&mut pass, &sb.parts.wall);
            }
        }
        if translucent {
            {
                let mut pass = enc.begin_render_pass(&wgpu::RenderPassDescriptor {
                    label: Some("shell prepass"),
                    color_attachments: &[],
                    depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                        view: &t.shell_depth,
                        depth_ops: Some(wgpu::Operations { load: wgpu::LoadOp::Clear(1.0), store: wgpu::StoreOp::Store }),
                        stencil_ops: None,
                    }),
                    ..Default::default()
                });
                pass.set_pipeline(&self.prepass);
                bind(&mut pass);
                draw(&mut pass, &sb.parts.shell);
            }
            for (label, pipeline) in [("shell far", &self.far), ("shell near", &self.near)] {
                let mut pass = enc.begin_render_pass(&wgpu::RenderPassDescriptor {
                    label: Some(label),
                    color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                        view: &hdr_view,
                        resolve_target: None,
                        depth_slice: None,
                        ops: wgpu::Operations { load: wgpu::LoadOp::Load, store: wgpu::StoreOp::Store },
                    })],
                    depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                        view: &t.depth,
                        depth_ops: Some(wgpu::Operations { load: wgpu::LoadOp::Load, store: wgpu::StoreOp::Store }),
                        stencil_ops: None,
                    }),
                    ..Default::default()
                });
                pass.set_pipeline(pipeline);
                bind(&mut pass);
                pass.set_bind_group(1, &t.group1, &[]);
                draw(&mut pass, &sb.parts.shell);
            }
        }
        {
            let out_view = t.out.create_view(&Default::default());
            let mut pass = enc.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("tonemap"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &out_view,
                    resolve_target: None,
                    depth_slice: None,
                    ops: wgpu::Operations { load: wgpu::LoadOp::Clear(wgpu::Color::BLACK), store: wgpu::StoreOp::Store },
                })],
                ..Default::default()
            });
            pass.set_pipeline(&self.tonemap);
            pass.set_bind_group(0, &t.tonemap_group, &[]);
            pass.draw(0..3, 0..1);
        }
        enc.copy_texture_to_buffer(
            wgpu::TexelCopyTextureInfo { texture: &t.out, mip_level: 0, origin: wgpu::Origin3d::ZERO, aspect: wgpu::TextureAspect::All },
            wgpu::TexelCopyBufferInfo {
                buffer: &t.readback,
                layout: wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(t.padded_row), rows_per_image: Some(h) },
            },
            wgpu::Extent3d { width: w, height: h, depth_or_array_layers: 1 },
        );
        self.queue.submit([enc.finish()]);

        let slice = t.readback.slice(..);
        slice.map_async(wgpu::MapMode::Read, |_| {});
        self.device.poll(wgpu::PollType::wait_indefinitely()).map_err(|e| format!("GPU poll failed: {e}"))?;
        let mut rgba = vec![0u8; (w * h * 4) as usize];
        {
            let data = slice.get_mapped_range().map_err(|e| format!("readback failed: {e}"))?;
            let row = (w * 4) as usize;
            for y in 0..h as usize {
                let src = y * t.padded_row as usize;
                rgba[y * row..(y + 1) * row].copy_from_slice(&data[src..src + row]);
            }
        }
        t.readback.unmap();
        Ok(rgba)
    }
}

fn uniforms(req: &Request, w: u32, h: u32, sim: &SimBuffers) -> Uniforms {
    let p = &req.params;
    let sc = &req.scene;
    let eye = p.cam.eye();
    let proj = Mat4::perspective_rh(p.cam.fov_deg.to_radians(), w as f32 / h as f32, 2.0, 60_000.0);
    let view = Mat4::look_at_rh(eye, p.cam.target, Vec3::Z);
    let mode = p.rs.mode;
    let mode_index = match mode {
        ViewMode::Shape => 0.0,
        ViewMode::Night => 1.0,
        ViewMode::Day => 2.0,
        ViewMode::Thickness => 3.0,
        ViewMode::Overhang => 4.0,
        ViewMode::Temperature => 5.0,
    };
    let lamp_on = mode != ViewMode::Day;
    let key = Vec3::new(-0.4, -0.6, 0.7).normalize();
    let e_key = if mode == ViewMode::Day { p.rs.room_lux * 2.5 } else { 0.0 };
    let cos_dark = p.bulb.dark_cone_deg.to_radians().cos();
    let col = p.bulb.color();
    let l_bulb = col * p.bulb.luminance();
    let o = &p.optics;
    let alb = o.albedo();
    let (clip, clip_on) = match p.rs.clip {
        Some(c) => {
            let sign = if c.keep_above { 1.0 } else { -1.0 };
            let mut n = [0.0f32; 3];
            n[c.axis] = sign;
            ([n[0], n[1], n[2], c.pos * sign], [1.0, c.axis as f32, 0.0, 0.0])
        }
        None => ([0.0; 4], [0.0; 4]),
    };
    let v4 = |v: Vec3, w: f32| [v.x, v.y, v.z, w];
    Uniforms {
        view_proj: (proj * view).to_cols_array_2d(),
        eye: v4(eye, mode_index),
        key: v4(key, e_key),
        room: [p.rs.room_lux, if lamp_on { 1.0 } else { 0.0 }, if p.rs.show_room { 1.0 } else { 0.0 }, render::temp_scale_max(p.hdt)],
        bulb_c: v4(sc.bulb_c, cos_dark),
        bulb_col: v4(col, p.bulb.intensity(1.0)),
        l_bulb: v4(l_bulb, 0.0),
        km: v4(o.k, o.s),
        km2: [o.haze, o.ior, o.f0, sc.max_thickness],
        clay: v4(p.rs.clay, 0.0),
        clip,
        clip_on,
        albedo: v4(alb, 0.0),
        table_rect: sim.table_rect,
        wall_rect: sim.wall_rect,
        planes: [sc.z_table, sc.wall_y, 0.0, 0.0],
    }
}

/// Ellipsoid mesh around `c` with radii `r` (vertex indices start at `base`).
fn ellipsoid(c: Vec3, r: Vec3, base: u32) -> (Vec<StaticVert>, Vec<u32>) {
    let (nu, nv) = (48u32, 24u32);
    let mut verts = Vec::with_capacity(((nu + 1) * (nv + 1)) as usize);
    for j in 0..=nv {
        let th = std::f32::consts::PI * j as f32 / nv as f32;
        for i in 0..=nu {
            let ph = std::f32::consts::TAU * i as f32 / nu as f32;
            let n = Vec3::new(th.sin() * ph.cos(), th.sin() * ph.sin(), th.cos());
            verts.push(StaticVert { pos: (c + n * r).to_array(), nrm: n.to_array(), tag: TAG_BULB, thick: 0.0, over: 0.0 });
        }
    }
    let mut idx = Vec::with_capacity((nu * nv * 6) as usize);
    for j in 0..nv {
        for i in 0..nu {
            let a = base + j * (nu + 1) + i;
            let b = a + nu + 1;
            idx.extend_from_slice(&[a, b, a + 1, a + 1, b, b + 1]);
        }
    }
    (verts, idx)
}

/// Exposure for the physical views, from the simulation rather than from
/// rendered pixels (the CPU renderer's median-of-shade-pixels, estimated):
/// lamp on: the median glow of the outer wall; daylight: the wall's
/// reflectance under the room light.
pub fn auto_exposure(sc: &Scene, sim: &SimResult, mode: ViewMode, room_lux: f32, op: &Optics) -> f32 {
    use crate::material::lum;
    use std::f32::consts::PI;
    match mode {
        ViewMode::Night => {
            let g = sc.grid;
            let mut ls: Vec<f32> = (g.outer0..g.outer0 + g.nt * g.nz)
                .filter_map(|v| sim.l_emit.get(v).map(|c| lum(*c)))
                .filter(|l| *l > 0.0)
                .collect();
            if ls.len() < 16 {
                return 0.35 / (room_lux / PI).max(1.0);
            }
            let mid = ls.len() / 2;
            ls.select_nth_unstable_by(mid, |a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
            0.35 / ls[mid].max(1e-3)
        }
        ViewMode::Day => {
            let t = sc.min_thickness.max(0.5);
            let albedo = lum(op.slab(t * 1.3, t).r_diff).max(0.05);
            0.25 / (albedo * room_lux * 2.25 / PI).max(1e-3)
        }
        _ => 1.0,
    }
}

/// Renders one frame headlessly (tests and the CLI): GPU if available.
pub fn render_once(req: &Request) -> Result<(Vec<u8>, String), String> {
    let mut r = Renderer::new()?;
    let name = r.adapter_name.clone();
    r.render(req).map(|rgba| (rgba, name))
}
