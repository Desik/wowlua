//! Models (spec debugger §18.11, decision T318): a model frame's or scene's
//! models drawn with depth and light into a texture of the frame's size,
//! which the UI pass then draws in the frame's place.
//!
//! Model files are read once and kept with the sequences read so far;
//! each frame poses them on the CPU (bones, then skinning) and draws one
//! vertex buffer. The 3D world's units are skinned on the GPU instead
//! (decision T540): each model file's vertices uploaded once, a frame's
//! bones and its batches' records each frame.

use std::collections::HashMap;

use wowlua_casc::display::{Composite, Display, Texture};
use wowlua_casc::m2::{self, Mat4, Model, Skin, Vec3};
use wowlua_engine::scene::{self, Image, ModelCamera};

use crate::{Assets, Renderer};

#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
pub(crate) struct MVertex {
    pos: [f32; 3],
    normal: [f32; 3],
    t1: [f32; 2],
    t2: [f32; 2],
    raw2: [f32; 2],
    color: [f32; 4],
    flags: u32,
    shaders: u32,
}

/// A model file's vertex as it is, for the GPU to pose (decision T540).
#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
pub(crate) struct SVertex {
    pos: [f32; 3],
    normal: [f32; 3],
    uv: [f32; 2],
    uv2: [f32; 2],
    bones: [u8; 4],
    weights: [u8; 4],
}

/// A batch the GPU poses: where its part stands, its two texture
/// transforms, its color, and its flags, shaders, first bone and bone
/// count (`model.wgsl`'s `Record`).
#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
pub(crate) struct Record {
    world: [f32; 16],
    uv1: [f32; 16],
    uv2: [f32; 16],
    color: [f32; 4],
    info: [u32; 4],
}

/// A batch the GPU poses: its model file's indices' range, how it blends,
/// its textures, and its record.
#[derive(Clone)]
pub(crate) struct SkinDraw {
    file: u32,
    start: u32,
    end: u32,
    blend: u16,
    depth_write: bool,
    /// An actor's faded by the camera's closeness (T562): its depth first.
    faded: bool,
    tex: [usize; 3],
    record: Record,
}

/// Actors as the GPU poses them: their batches, and the bones the records
/// point into.
#[derive(Clone, Default)]
pub(crate) struct Skinned {
    draws: Vec<SkinDraw>,
    bones: Vec<Mat4>,
}

const CPU_ATTRIBUTES: [wgpu::VertexAttribute; 8] = wgpu::vertex_attr_array![0 => Float32x3, 1 => Float32x3, 2 => Float32x2, 3 => Float32x2, 4 => Float32x2, 5 => Float32x4, 6 => Uint32, 7 => Uint32];
const SKINNED_ATTRIBUTES: [wgpu::VertexAttribute; 6] = wgpu::vertex_attr_array![0 => Float32x3, 1 => Float32x3, 2 => Float32x2, 3 => Float32x2, 4 => Uint8x4, 5 => Unorm8x4];

const UNLIT: u32 = 1;
const ALPHA_KEY: u32 = 2;
const OPAQUE: u32 = 4;

/// A model file read, with its first skin profile.
pub(crate) struct Loaded {
    model: Model,
    skin: Skin,
}

/// A particle to draw, facing the camera.
#[derive(Clone, Copy)]
pub(crate) struct Sprite {
    pos: Vec3,
    size: f32,
    spin: f32,
    color: [f32; 4],
    tex: usize,
    blend: u16,
    /// Its cell of the texture: left, top, right, bottom.
    cell: [f32; 4],
    /// A tail's: where it reaches back to (from `pos`), drawn across.
    tail: Option<Vec3>,
    /// A ribbon's piece: its four corners as they are (head top, head
    /// bottom, tail top, tail bottom).
    corners: Option<[Vec3; 4]>,
}

/// The most edges a ribbon keeps.
const RIBBON_EDGES: usize = 64;

/// A random stream (xorshift), the same for the same seed.
struct Rng(u64);

impl Rng {
    fn new(seed: u64) -> Rng {
        Rng(seed.wrapping_mul(0x9E37_79B9_7F4A_7C15) ^ 0xD1B5_4A32_D192_ED03)
    }
    /// A number in 0 to 1.
    fn unit(&mut self) -> f32 {
        let mut x = self.0;
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        self.0 = x;
        (x >> 40) as f32 / (1u64 << 24) as f32
    }
    /// A number in -1 to 1.
    fn signed(&mut self) -> f32 {
        self.unit() * 2.0 - 1.0
    }
}

/// The body's pose and attachment points, for the parts on it.
pub(crate) struct Body {
    points: HashMap<u32, Mat4>,
    pose: Vec<Mat4>,
    /// Toward the camera and its up, in the scene's space.
    facing: (Vec3, Vec3),
}

/// Toward the camera and its up in the scene's space, as the model pass
/// will place it (`finish_model`): a scene's camera; a frame's own camera
/// (one of the model's); else from in front (+X), turned toward the
/// model's portrait camera by the portrait zoom.
fn camera_facing(m: &m2::Model, camera: &ModelCamera, world: &Mat4) -> (Vec3, Vec3) {
    let up = [0.0, 0.0, 1.0];
    let from = |c: &m2::Camera| {
        sub(
            m2::transform(world, c.position),
            m2::transform(world, c.target),
        )
    };
    match camera {
        ModelCamera::Scene { forward, up, .. } => ([-forward[0], -forward[1], -forward[2]], *up),
        ModelCamera::Frame {
            camera: Some(i), ..
        } => match m.cameras.get((*i).max(0) as usize) {
            Some(c) => (from(c), up),
            None => ([1.0, 0.0, 0.0], up),
        },
        ModelCamera::Frame { portrait_zoom, .. } => match m.camera(0) {
            Some(c) if *portrait_zoom > 0.0 => {
                let p = from(c);
                let l = dot(p, p).sqrt().max(1e-6);
                let z = portrait_zoom.clamp(0.0, 1.0);
                (
                    lerp3([1.0, 0.0, 0.0], [p[0] / l, p[1] / l, p[2] / l], z),
                    up,
                )
            }
            _ => ([1.0, 0.0, 0.0], up),
        },
    }
}

/// A direction of the scene's space in a model's (its world matrix's
/// rotation undone; scale left out, the direction's length unused).
fn into_model(world: &Mat4, d: Vec3) -> Vec3 {
    [
        world[0] * d[0] + world[1] * d[1] + world[2] * d[2],
        world[4] * d[0] + world[5] * d[1] + world[6] * d[2],
        world[8] * d[0] + world[9] * d[1] + world[10] * d[2],
    ]
}

/// What the model pass keeps between frames.
#[derive(Default)]
pub(crate) struct Models {
    /// Model files by ID (none: not readable).
    files: HashMap<u32, Option<Loaded>>,
    /// Displays by their source.
    displays: HashMap<String, Option<Display>>,
    /// By blend mode, depth writing, and skinned on the GPU.
    pipelines: HashMap<(u16, bool, bool), wgpu::RenderPipeline>,
    /// What skinning on the GPU binds (its records and bones) and its
    /// pipelines' layout; none where the GPU cannot (decision T540), not
    /// known yet before the first world.
    skin_layout: Option<Option<(wgpu::BindGroupLayout, wgpu::PipelineLayout)>>,
    /// Model files' vertices and indices on the GPU, by file.
    meshes: HashMap<u32, (wgpu::Buffer, wgpu::Buffer)>,
    /// While the world's units are posed: what the GPU will pose.
    skinned_out: Option<Skinned>,
    layout: Option<(
        wgpu::BindGroupLayout,
        wgpu::BindGroupLayout,
        wgpu::PipelineLayout,
    )>,
    shader: Option<wgpu::ShaderModule>,
    sampler: Option<wgpu::Sampler>,
    groups: HashMap<[usize; 3], wgpu::BindGroup>,
    /// Composited textures by their layers.
    composites: HashMap<String, usize>,
    /// Portraits drawn, by their source.
    portraits: HashMap<String, Option<usize>>,
    /// Drawing a portrait: on black, kept.
    portrait: bool,
    /// The particles of the model item being drawn.
    sprites: Vec<Sprite>,
}

/// Actors posed: what [`Renderer::pose_actors`] gives.
#[derive(Clone)]
pub(crate) struct Posed {
    verts: Vec<MVertex>,
    /// The triangles' corners: indices into `verts` (T538).
    indices: Vec<u32>,
    draws: Vec<Draw>,
    lo: [f32; 3],
    hi: [f32; 3],
    portrait: Option<(Vec3, Vec3, f32)>,
    own_camera: Option<(Vec3, Vec3, f32)>,
    /// What the GPU poses (the world's units, decision T540).
    skinned: Option<Skinned>,
    /// Its particles, kept with its pose (the world's, decision T559).
    sprites: Vec<Sprite>,
}

/// The world's models, ready to draw (decision T526): the ones posed on
/// the CPU, and the ones the GPU poses with their records and bones.
pub(crate) struct WorldModels {
    vertices: Option<wgpu::Buffer>,
    indices: Option<wgpu::Buffer>,
    group: wgpu::BindGroup,
    draws: Vec<Draw>,
    skinned: Option<(wgpu::BindGroup, Vec<SkinDraw>)>,
}

/// The pipeline key's blend for a faded actor's depth alone (T562).
const DEPTH_ONLY: u16 = u16::MAX;

/// A batch to draw: its indices' range and how.
#[derive(Clone)]
pub(crate) struct Draw {
    blend: u16,
    depth_write: bool,
    /// An actor's faded by the camera's closeness (T562): its depth first.
    faded: bool,
    tex: [usize; 3],
    start: u32,
    end: u32,
}

/// The view and projection of a camera, and whether anything was framed.
pub(crate) fn look_at(eye: Vec3, forward: Vec3, up: Vec3) -> Mat4 {
    let f = normalize(forward);
    let r = normalize(cross(f, up));
    let u = cross(r, f);
    [
        r[0],
        u[0],
        -f[0],
        0.0,
        r[1],
        u[1],
        -f[1],
        0.0,
        r[2],
        u[2],
        -f[2],
        0.0,
        -dot(r, eye),
        -dot(u, eye),
        dot(f, eye),
        1.0,
    ]
}

pub(crate) fn perspective(fov_y: f32, aspect: f32, near: f32, far: f32) -> Mat4 {
    let f = 1.0 / (fov_y / 2.0).tan();
    let mut m = [0.0; 16];
    m[0] = f / aspect;
    m[5] = f;
    m[10] = far / (near - far);
    m[11] = -1.0;
    m[14] = near * far / (near - far);
    m
}

fn dot(a: Vec3, b: Vec3) -> f32 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

fn cross(a: Vec3, b: Vec3) -> Vec3 {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
}

fn normalize(a: Vec3) -> Vec3 {
    let l = dot(a, a).sqrt().max(1e-9);
    [a[0] / l, a[1] / l, a[2] / l]
}

fn sub(a: Vec3, b: Vec3) -> Vec3 {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}

fn lerp3(a: Vec3, b: Vec3, t: f32) -> Vec3 {
    [
        a[0] + (b[0] - a[0]) * t,
        a[1] + (b[1] - a[1]) * t,
        a[2] + (b[2] - a[2]) * t,
    ]
}

/// A rotation about Z (yaw), Y (pitch), X (roll): pitch and roll about
/// the model's turned axes, or with `fixed` about the view's after the yaw.
fn orientation(yaw: f32, pitch: f32, roll: f32, fixed: bool) -> Mat4 {
    let (sy, cy) = yaw.sin_cos();
    let (sp, cp) = pitch.sin_cos();
    let (sr, cr) = roll.sin_cos();
    let rz: Mat4 = [
        cy, sy, 0.0, 0.0, -sy, cy, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0,
    ];
    let ry: Mat4 = [
        cp, 0.0, -sp, 0.0, 0.0, 1.0, 0.0, 0.0, sp, 0.0, cp, 0.0, 0.0, 0.0, 0.0, 1.0,
    ];
    let rx: Mat4 = [
        1.0, 0.0, 0.0, 0.0, 0.0, cr, sr, 0.0, 0.0, -sr, cr, 0.0, 0.0, 0.0, 0.0, 1.0,
    ];
    if fixed {
        m2::mul(&m2::mul(&rx, &ry), &rz)
    } else {
        m2::mul(&m2::mul(&rz, &ry), &rx)
    }
}

/// The vertical field of view of a diagonal one at an aspect ratio.
fn vertical_fov(diagonal: f32, aspect: f32) -> f32 {
    2.0 * ((diagonal / 2.0).tan() / (1.0 + aspect * aspect).sqrt()).atan()
}

/// A display's key in the cache.
fn source_key(s: &scene::ModelSource) -> String {
    format!("{s:?}")
}

/// How an M2 blend mode blends.
fn blend_state(blend: u16) -> Option<wgpu::BlendState> {
    use wgpu::{BlendComponent as C, BlendFactor as F, BlendOperation as O};
    let c = |src, dst| C {
        src_factor: src,
        dst_factor: dst,
        operation: O::Add,
    };
    let keep = c(F::Zero, F::One);
    match blend {
        0 | 1 => None,
        2 => Some(wgpu::BlendState {
            color: c(F::SrcAlpha, F::OneMinusSrcAlpha),
            alpha: c(F::One, F::OneMinusSrcAlpha),
        }),
        3 => Some(wgpu::BlendState {
            color: c(F::One, F::One),
            alpha: keep,
        }),
        4 => Some(wgpu::BlendState {
            color: c(F::SrcAlpha, F::One),
            alpha: keep,
        }),
        5 => Some(wgpu::BlendState {
            color: c(F::Dst, F::Zero),
            alpha: keep,
        }),
        6 => Some(wgpu::BlendState {
            color: c(F::Dst, F::Src),
            alpha: keep,
        }),
        // BlendAdd: adds, never darkening what is behind
        // (in game a texture without alpha draws no black: a
        // totem's rune, probed 2026-10-23).
        7 => Some(wgpu::BlendState {
            color: c(F::One, F::One),
            alpha: keep,
        }),
        _ => Some(wgpu::BlendState {
            color: c(F::One, F::OneMinusSrcAlpha),
            alpha: c(F::One, F::OneMinusSrcAlpha),
        }),
    }
}

fn texture_entry(binding: u32) -> wgpu::BindGroupLayoutEntry {
    wgpu::BindGroupLayoutEntry {
        binding,
        visibility: wgpu::ShaderStages::FRAGMENT,
        ty: wgpu::BindingType::Texture {
            sample_type: wgpu::TextureSampleType::Float { filterable: true },
            view_dimension: wgpu::TextureViewDimension::D2,
            multisampled: false,
        },
        count: None,
    }
}

impl Renderer {
    fn model_layouts(&mut self) {
        if self.models.layout.is_some() {
            return;
        }
        let d = &self.device;
        let globals = d.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("model globals"),
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Uniform,
                    has_dynamic_offset: false,
                    min_binding_size: None,
                },
                count: None,
            }],
        });
        let tex = d.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("model texture"),
            entries: &[
                texture_entry(0),
                texture_entry(1),
                texture_entry(2),
                wgpu::BindGroupLayoutEntry {
                    binding: 3,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
            ],
        });
        let layout = d.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("model"),
            bind_group_layouts: &[Some(&globals), Some(&tex)],
            immediate_size: 0,
        });
        self.models.shader = Some(d.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("model"),
            source: wgpu::ShaderSource::Wgsl(include_str!("model.wgsl").into()),
        }));
        self.models.sampler = Some(d.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("model"),
            address_mode_u: wgpu::AddressMode::Repeat,
            address_mode_v: wgpu::AddressMode::Repeat,
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            mipmap_filter: wgpu::MipmapFilterMode::Linear,
            ..Default::default()
        }));
        self.models.layout = Some((globals, tex, layout));
    }

    /// The pipeline of an M2 blend mode, writing depth or not, its vertices
    /// posed already or `skinned` on the GPU (decision T540).
    fn model_pipeline(
        &mut self,
        blend: u16,
        depth_write: bool,
        skinned: bool,
    ) -> &wgpu::RenderPipeline {
        let layout = match &self.models.skin_layout {
            Some(Some((_, layout))) if skinned => layout,
            _ => &self.models.layout.as_ref().unwrap().2,
        };
        let shader = self.models.shader.as_ref().unwrap();
        let device = &self.device;
        self.models
            .pipelines
            .entry((blend, depth_write, skinned))
            .or_insert_with(|| {
                // (a faded actor's depth alone: T562)
                let (state, mask) = if blend == DEPTH_ONLY {
                    (None, wgpu::ColorWrites::empty())
                } else {
                    (blend_state(blend), wgpu::ColorWrites::ALL)
                };
                device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                    label: Some("model"),
                    layout: Some(layout),
                    vertex: wgpu::VertexState {
                        module: shader,
                        entry_point: Some(if skinned { "vs_skinned" } else { "vs" }),
                        compilation_options: Default::default(),
                        buffers: &[Some(if skinned {
                            wgpu::VertexBufferLayout {
                                array_stride: std::mem::size_of::<SVertex>() as u64,
                                step_mode: wgpu::VertexStepMode::Vertex,
                                attributes: &SKINNED_ATTRIBUTES,
                            }
                        } else {
                            wgpu::VertexBufferLayout {
                                array_stride: std::mem::size_of::<MVertex>() as u64,
                                step_mode: wgpu::VertexStepMode::Vertex,
                                attributes: &CPU_ATTRIBUTES,
                            }
                        })],
                    },
                    primitive: wgpu::PrimitiveState::default(),
                    depth_stencil: Some(wgpu::DepthStencilState {
                        format: wgpu::TextureFormat::Depth32Float,
                        depth_write_enabled: Some(depth_write),
                        depth_compare: Some(wgpu::CompareFunction::LessEqual),
                        stencil: wgpu::StencilState::default(),
                        bias: wgpu::DepthBiasState::default(),
                    }),
                    multisample: wgpu::MultisampleState::default(),
                    fragment: Some(wgpu::FragmentState {
                        module: shader,
                        entry_point: Some("fs"),
                        compilation_options: Default::default(),
                        targets: &[Some(wgpu::ColorTargetState {
                            format: crate::FORMAT,
                            blend: state,
                            write_mask: mask,
                        })],
                    }),
                    multiview_mask: None,
                    cache: None,
                })
            })
    }

    /// Whether the GPU poses the world's units (decision T540): it needs
    /// storage buffers in the vertex stage, which WebGL lacks. Their
    /// layouts are made on the first call.
    fn skinning(&mut self) -> bool {
        if let Some(known) = &self.models.skin_layout {
            return known.is_some();
        }
        self.model_layouts();
        let can = self.device.limits().max_storage_buffers_per_shader_stage >= 2
            && self.device.adapter_info().backend != wgpu::Backend::Gl;
        let layouts = can.then(|| {
            let storage = |binding| wgpu::BindGroupLayoutEntry {
                binding,
                visibility: wgpu::ShaderStages::VERTEX,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Storage { read_only: true },
                    has_dynamic_offset: false,
                    min_binding_size: None,
                },
                count: None,
            };
            let d = &self.device;
            let skin = d.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                label: Some("model skin"),
                entries: &[storage(0), storage(1)],
            });
            let (globals, tex, _) = self.models.layout.as_ref().unwrap();
            let layout = d.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                label: Some("model skinned"),
                bind_group_layouts: &[Some(globals), Some(tex), Some(&skin)],
                immediate_size: 0,
            });
            (skin, layout)
        });
        self.models.skin_layout = Some(layouts);
        can
    }

    /// Poses the 3D world's units on the CPU even where the GPU could (as
    /// on WebGL): to compare the two (decision T540).
    pub fn skin_on_the_cpu(&mut self) {
        self.models.skin_layout = Some(None);
    }

    /// A model file's vertices and indices on the GPU, uploaded once.
    fn skin_mesh(&mut self, file: u32, l: &Loaded) {
        if self.models.meshes.contains_key(&file) {
            return;
        }
        let verts: Vec<SVertex> = l
            .model
            .vertices
            .iter()
            .map(|v| SVertex {
                pos: v.position,
                normal: v.normal,
                uv: v.uv,
                uv2: v.uv2,
                bones: v.bones,
                weights: v.weights,
            })
            .collect();
        let vertices = crate::buffer_with(
            &self.device,
            &self.queue,
            "model vertices",
            bytemuck::cast_slice(&verts),
            wgpu::BufferUsages::VERTEX,
        );
        let indices = crate::buffer_with(
            &self.device,
            &self.queue,
            "model indices",
            bytemuck::cast_slice(&l.skin.indices),
            wgpu::BufferUsages::INDEX,
        );
        self.models.meshes.insert(file, (vertices, indices));
    }

    fn model_group(&mut self, tex: [usize; 3]) {
        if self.models.groups.contains_key(&tex) {
            return;
        }
        let view = |i: usize| wgpu::BindGroupEntry {
            binding: i as u32,
            resource: wgpu::BindingResource::TextureView(&self.textures[tex[i]].view),
        };
        let g = self.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: None,
            layout: &self.models.layout.as_ref().unwrap().1,
            entries: &[
                view(0),
                view(1),
                view(2),
                wgpu::BindGroupEntry {
                    binding: 3,
                    resource: wgpu::BindingResource::Sampler(self.models.sampler.as_ref().unwrap()),
                },
            ],
        });
        self.models.groups.insert(tex, g);
    }

    /// A model file, read once (with its skeleton and first skin).
    /// A model file's radius (yards), its file read (not drawn) on first
    /// use: how big a doodad looks (decision T525).
    pub(crate) fn model_radius(&mut self, file: u32, assets: &mut dyn Assets) -> Option<f32> {
        if !self.loaded(file, assets) {
            return None;
        }
        Some(self.models.files.get(&file)?.as_ref()?.model.radius)
    }

    /// How far a unit's body reaches across the ground (yards, decision
    /// T563): its body model's collision box, scaled; none before it is
    /// read.
    pub(crate) fn source_reach(
        &mut self,
        source: &scene::ModelSource,
        assets: &mut dyn Assets,
    ) -> Option<f32> {
        let key = source_key(source);
        if !self.models.displays.contains_key(&key) {
            let d = assets.model(source);
            self.models.displays.insert(key.clone(), d);
        }
        let display = self.models.displays.get(&key)?.as_ref()?;
        let (file, scale) = (display.parts.first()?.file, display.scale);
        if !self.loaded(file, assets) {
            return None;
        }
        let [lo, hi] = self.models.files.get(&file)?.as_ref()?.model.collision;
        let reach = [lo[0], lo[1], hi[0], hi[1]]
            .iter()
            .fold(0.0f32, |m, v| m.max(v.abs()));
        Some(reach * scale)
    }

    /// A model file's standing pose as one static mesh (the 3D world's
    /// doodads): its vertices (posed, with their first coordinates), its
    /// indices, and each batch's range, texture (its own, kind 0; else
    /// none), blend mode and flags.
    #[allow(clippy::type_complexity)]
    pub(crate) fn model_mesh(
        &mut self,
        file: u32,
        assets: &mut dyn Assets,
    ) -> Option<(
        Vec<crate::world::BVertex>,
        Vec<u32>,
        Vec<(u32, u32, u32, u16, u16)>,
    )> {
        if !self.loaded(file, assets) {
            return None;
        }
        let Loaded { model: m, skin } = self.models.files.get(&file)?.as_ref()?;
        let bones = m.pose(m.sequence(0), 0);
        let posed = m.skinned(&bones);
        let verts = m
            .vertices
            .iter()
            .zip(posed)
            .map(|(v, (pos, normal))| crate::world::BVertex {
                pos,
                normal,
                uv: v.uv,
                color: [0.5, 0.5, 0.5, 1.0],
            })
            .collect();
        let mut batches = Vec::new();
        for b in &skin.batches {
            let Some(sub) = skin.submeshes.get(b.submesh as usize) else {
                continue;
            };
            let index = m
                .texture_combos
                .get(b.texture_combo as usize)
                .copied()
                .unwrap_or(0) as usize;
            let texture = match m.textures.get(index) {
                Some(t) if t.kind == 0 => t.file,
                _ => 0,
            };
            let material = m.materials.get(b.material as usize).copied();
            batches.push((
                sub.index_start,
                sub.index_count,
                texture,
                material.map_or(0, |x| x.blend),
                material.map_or(0, |x| x.flags),
            ));
        }
        Some((verts, skin.indices.clone(), batches))
    }

    fn loaded(&mut self, file: u32, assets: &mut dyn Assets) -> bool {
        if let Some(l) = self.models.files.get(&file) {
            return l.is_some();
        }
        let loaded = (|| {
            let bytes = assets.file(file)?;
            let mut model = Model::parse(&bytes)?;
            if let Some(skel) = model.skeleton_file
                && let Some(b) = assets.file(skel)
            {
                if let Some(parent) = Model::skeleton_parent(&b)
                    && let Some(pb) = assets.file(parent)
                {
                    model.load_skeleton(&pb);
                }
                model.load_skeleton(&b);
            }
            let skin = Skin::parse(&assets.file(*model.skin_files.first()?)?)?;
            Some(Loaded { model, skin })
        })();
        let ok = loaded.is_some();
        self.models.files.insert(file, loaded);
        ok
    }

    /// Reads a sequence's `.anim` file in, where it is elsewhere.
    fn ensure_anim(&mut self, file: u32, sequence: usize, assets: &mut dyn Assets) {
        let Some(Some(l)) = self.models.files.get(&file) else {
            return;
        };
        if let Some(anim) = l.model.needs_anim(sequence) {
            let bytes = assets.file(anim);
            if let Some(Some(l)) = self.models.files.get_mut(&file) {
                match bytes {
                    Some(b) => l.model.load_anim(sequence, &b),
                    // Unreadable: marked read, so not asked again.
                    None => l.model.load_anim(sequence, &[]),
                }
            }
        }
    }

    /// A composited texture (a character's skin), made once.
    fn composite(&mut self, c: &Composite, assets: &mut dyn Assets) -> usize {
        let key = format!("{c:?}");
        if let Some(&t) = self.models.composites.get(&key) {
            return t;
        }
        let img = crate::composite::composite(c, &mut |f| assets.image(&Image::FileId(f)));
        let t = self.upload_mipmapped(&img);
        self.models.composites.insert(key, t);
        t
    }

    /// A unit's portrait (`SetPortraitTexture`): its model through its
    /// portrait camera, on black, drawn once.
    pub(crate) fn portrait(
        &mut self,
        source: &scene::ModelSource,
        assets: &mut dyn Assets,
    ) -> Option<usize> {
        let key = format!("{source:?}");
        if let Some(t) = self.models.portraits.get(&key) {
            return *t;
        }
        let item = scene::Model {
            widget: 0,
            rect: scene::Rect {
                x: 0.0,
                y: 0.0,
                w: 128.0,
                h: 128.0,
            },
            clip: None,
            alpha: 1.0,
            actors: vec![scene::Actor {
                source: source.clone(),
                position: [0.0; 3],
                yaw: 0.0,
                pitch: 0.0,
                roll: 0.0,
                fixed_axes: false,
                scale: 1.0,
                alpha: 1.0,
                animation: 0,
                variation: 0,
                placement: None,
                phase: 0,
            }],
            camera: ModelCamera::Frame {
                portrait_zoom: 1.0,
                distance_scale: 1.0,
                camera: None,
            },
            light: scene::Light {
                enabled: true,
                direction: [-1.0, 0.3, -0.6],
                ambient: [0.55; 3],
                diffuse: [0.75; 3],
            },
            time: 0.0,
        };
        self.models.portrait = true;
        let t = self.draw_model(&item, 1.0, assets);
        self.models.portrait = false;
        self.models.portraits.insert(key, t);
        t
    }

    /// Draws a model item into a texture of its size at `scale`: the
    /// texture's index, or none when nothing could be drawn.
    pub(crate) fn draw_model(
        &mut self,
        item: &scene::Model,
        scale: f32,
        assets: &mut dyn Assets,
    ) -> Option<usize> {
        let max = self.device.limits().max_texture_dimension_2d;
        let width = ((item.rect.w * scale).round() as u32).clamp(1, max);
        let height = ((item.rect.h * scale).round() as u32).clamp(1, max);
        let aspect = width as f32 / height as f32;
        let ms = (item.time * 1000.0) as u32;
        let p = self.pose_actors(&item.actors, &item.camera, ms, assets);
        self.finish_model(
            item,
            width,
            height,
            aspect,
            p.verts,
            p.indices,
            p.draws,
            p.lo,
            p.hi,
            p.portrait,
            p.own_camera,
        )
    }

    /// Actors posed (`ms` into their animations) for a camera: their
    /// vertices and draws, the bounds of their points, the portrait's and
    /// the model's own camera.
    fn pose_actors(
        &mut self,
        actors: &[scene::Actor],
        camera: &ModelCamera,
        ms: u32,
        assets: &mut dyn Assets,
    ) -> Posed {
        self.model_layouts();
        self.models.sprites.clear();
        let mut verts: Vec<MVertex> = Vec::new();
        let mut indices: Vec<u32> = Vec::new();
        let mut draws: Vec<Draw> = Vec::new();
        // The posed points of every actor, for a frame's camera.
        let mut lo = [f32::MAX; 3];
        let mut hi = [f32::MIN; 3];
        let mut portrait: Option<(Vec3, Vec3, f32)> = None;
        let mut own_camera: Option<(Vec3, Vec3, f32)> = None;
        for actor in actors {
            let key = source_key(&actor.source);
            if !self.models.displays.contains_key(&key) {
                let d = assets.model(&actor.source);
                self.models.displays.insert(key.clone(), d);
            }
            let Some(display) = self.models.displays.get(&key).cloned().flatten() else {
                continue;
            };
            let scale = actor.scale * display.scale;
            let world = match actor.placement {
                // (a prop's place on its map, T558)
                Some(m) => m2::mul(&m, &m2::scaling([display.scale; 3])),
                None => {
                    let mut world = m2::translation(actor.position);
                    world = m2::mul(
                        &world,
                        &orientation(actor.yaw, actor.pitch, actor.roll, actor.fixed_axes),
                    );
                    m2::mul(&world, &m2::scaling([scale; 3]))
                }
            };
            let ms = ms.wrapping_add(actor.phase);
            // The body's attachment points, for parts there.
            let mut body_points: Option<Body> = None;
            for part in &display.parts {
                if !self.loaded(part.file, assets) {
                    continue;
                }
                let seq = {
                    let Some(Some(l)) = self.models.files.get(&part.file) else {
                        continue;
                    };
                    l.model
                        .sequence(actor.animation as u16)
                        .or_else(|| l.model.sequence(0))
                };
                if let Some(s) = seq {
                    self.ensure_anim(part.file, s, assets);
                }
                // Taken out while drawn (its textures load meanwhile).
                let Some(Some(l)) = self.models.files.remove(&part.file) else {
                    continue;
                };
                self.draw_part(
                    &l,
                    part,
                    actor,
                    &display,
                    camera,
                    world,
                    seq,
                    ms,
                    &mut body_points,
                    &mut portrait,
                    &mut own_camera,
                    &mut verts,
                    &mut indices,
                    &mut draws,
                    &mut lo,
                    &mut hi,
                    assets,
                );
                self.models.files.insert(part.file, Some(l));
            }
        }
        Posed {
            verts,
            indices,
            draws,
            lo,
            hi,
            portrait,
            own_camera,
            skinned: None,
            sprites: Vec::new(),
        }
    }

    /// Particles as quads facing a camera (its `view`): spun heads, tails
    /// drawn across, ribbons' pieces as they are; each draw without depth.
    fn sprite_quads(
        &self,
        sprites: Vec<Sprite>,
        view: &Mat4,
        verts: &mut Vec<MVertex>,
        indices: &mut Vec<u32>,
        draws: &mut Vec<Draw>,
    ) {
        let right = [view[0], view[4], view[8]];
        let up = [view[1], view[5], view[9]];
        let back = [view[2], view[6], view[10]];
        for sp in sprites {
            let (sn, cs) = sp.spin.sin_cos();
            let h = sp.size / 2.0;
            // A tail: from its head back to its end, its width across the
            // view; a head: square to the view, spun.
            let tail = sp.tail.map(|end| {
                let along = sub(end, sp.pos);
                let side = cross(along, back);
                let l = dot(side, side).sqrt();
                let side = if l > 1e-6 {
                    [side[0] / l * h, side[1] / l * h, side[2] / l * h]
                } else {
                    [right[0] * h, right[1] * h, right[2] * h]
                };
                (end, side)
            });
            let corner = |x: f32, y: f32| {
                // A ribbon's piece: x -1 the head, 1 the tail; y 1 the top.
                if let Some(c) = sp.corners {
                    return match (x < 0.0, y > 0.0) {
                        (true, true) => c[0],
                        (true, false) => c[1],
                        (false, true) => c[2],
                        (false, false) => c[3],
                    };
                }
                if let Some((end, side)) = tail {
                    // x: -1 the head, 1 the tail's end; y across.
                    let at = if x < 0.0 { sp.pos } else { end };
                    return [
                        at[0] + side[0] * y,
                        at[1] + side[1] * y,
                        at[2] + side[2] * y,
                    ];
                }
                let (rx, ry) = (x * cs - y * sn, x * sn + y * cs);
                [
                    sp.pos[0] + (right[0] * rx + up[0] * ry) * h,
                    sp.pos[1] + (right[1] * rx + up[1] * ry) * h,
                    sp.pos[2] + (right[2] * rx + up[2] * ry) * h,
                ]
            };
            let [u0, v0, u1, v1] = sp.cell;
            let start = indices.len() as u32;
            let flags = UNLIT | (u32::from(sp.blend.min(7)) << 8);
            let mk = |p: Vec3, uv: [f32; 2]| MVertex {
                pos: p,
                normal: [0.0, 0.0, 1.0],
                t1: uv,
                t2: uv,
                raw2: uv,
                color: sp.color,
                flags,
                shaders: crate::combiners::ps::MOD,
            };
            let (a, b, c, d) = (
                mk(corner(-1.0, 1.0), [u0, v0]),
                mk(corner(-1.0, -1.0), [u0, v1]),
                mk(corner(1.0, 1.0), [u1, v0]),
                mk(corner(1.0, -1.0), [u1, v1]),
            );
            let first = verts.len() as u32;
            verts.extend_from_slice(&[a, b, c, d]);
            indices.extend([0, 1, 2, 2, 1, 3].map(|k| first + k));
            let tex = [sp.tex, self.white, self.white];
            match draws.last_mut() {
                Some(last)
                    if last.tex == tex
                        && last.blend == sp.blend
                        && !last.depth_write
                        && last.end == start =>
                {
                    last.end = indices.len() as u32
                }
                _ => draws.push(Draw {
                    blend: sp.blend,
                    depth_write: false,
                    faded: false,
                    tex,
                    start,
                    end: indices.len() as u32,
                }),
            }
        }
    }

    /// The 3D world's units as their models (decision T526): posed for the
    /// world's camera (`view`, `proj`), lit by `light` (its direction,
    /// ambient and diffuse), with their particles (decision T559), ready to
    /// draw in the world's pass; none for nothing to draw.
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn world_models(
        &mut self,
        actors: &[scene::Actor],
        camera: &ModelCamera,
        ms: u32,
        view: Mat4,
        proj: Mat4,
        light: ([f32; 3], [f32; 3], [f32; 3]),
        assets: &mut dyn Assets,
    ) -> Option<WorldModels> {
        // Each unit posed apart, a farther one less often: within 12
        // yards every frame, to 30 yards 20 times a second, beyond 10;
        // between, its last pose (T538; the game's rate unverified).
        let eye = match camera {
            ModelCamera::Scene { position, .. } => *position,
            _ => [0.0; 3],
        };
        let mut p = Posed {
            verts: std::mem::take(&mut self.world_verts),
            indices: std::mem::take(&mut self.world_indices),
            draws: Vec::new(),
            lo: [f32::MAX; 3],
            hi: [f32::MIN; 3],
            portrait: None,
            own_camera: None,
            skinned: None,
            sprites: Vec::new(),
        };
        p.verts.clear();
        p.indices.clear();
        // (each unit's and prop's particles, as last posed: T559)
        let mut sprites: Vec<Sprite> = Vec::new();
        // (posed on the GPU where it can: the bones of every unit's parts
        // together, each batch's record pointing at its own; T540)
        let skinning = self.skinning();
        let mut bones: Vec<Mat4> = Vec::new();
        let mut skin_draws: Vec<SkinDraw> = Vec::new();
        let mut kept = std::mem::take(&mut self.world_poses);
        let mut fresh = HashMap::new();
        for actor in actors {
            let far = dot(sub(actor.position, eye), sub(actor.position, eye)).sqrt();
            let period = if far < 12.0 {
                0
            } else if far < 30.0 {
                50
            } else {
                100
            };
            let moment = ms.checked_div(period).unwrap_or(ms);
            let key = format!(
                "{}|{:?}|{}|{}|{}|{}",
                source_key(&actor.source),
                actor.position.map(f32::to_bits),
                actor.yaw.to_bits(),
                actor.animation,
                actor.scale.to_bits(),
                period
            );
            let one = match kept.remove(&key) {
                Some((at, one)) if period != 0 && at == moment => one,
                _ => {
                    if skinning {
                        self.models.skinned_out = Some(Skinned::default());
                    }
                    let mut one = self.pose_actors(std::slice::from_ref(actor), camera, ms, assets);
                    one.skinned = self.models.skinned_out.take();
                    one.sprites = std::mem::take(&mut self.models.sprites);
                    one
                }
            };
            if let Some(s) = &one.skinned {
                let base = bones.len() as u32;
                bones.extend_from_slice(&s.bones);
                skin_draws.extend(s.draws.iter().map(|d| {
                    let mut d = d.clone();
                    d.record.info[2] += base;
                    d
                }));
            }
            sprites.extend_from_slice(&one.sprites);
            let (vbase, ibase) = (p.verts.len() as u32, p.indices.len() as u32);
            p.verts.extend_from_slice(&one.verts);
            p.indices.extend(one.indices.iter().map(|i| i + vbase));
            p.draws.extend(one.draws.iter().map(|d| Draw {
                start: d.start + ibase,
                end: d.end + ibase,
                ..d.clone()
            }));
            if period != 0 {
                fresh.insert(key, (moment, one));
            }
        }
        // (the poses of units no longer drawn go)
        self.world_poses = fresh;
        drop(kept);
        self.models.sprites.clear();
        // Their particles, facing the world's camera (T559).
        self.world.particles = sprites.len();
        self.sprite_quads(sprites, &view, &mut p.verts, &mut p.indices, &mut p.draws);
        if p.verts.is_empty() && skin_draws.is_empty() {
            self.world_verts = p.verts;
            self.world_indices = p.indices;
            return None;
        }
        let view_proj = m2::mul(&proj, &view);
        let (dir, ambient, diffuse) = light;
        let mut globals = Vec::with_capacity(44);
        globals.extend_from_slice(&view_proj);
        globals.extend_from_slice(&view);
        globals.extend_from_slice(&[dir[0], dir[1], dir[2], 0.0]);
        globals.extend_from_slice(&[ambient[0], ambient[1], ambient[2], 1.0]);
        globals.extend_from_slice(&[diffuse[0], diffuse[1], diffuse[2], 1.0]);
        let ubuf = crate::buffer_with(
            &self.device,
            &self.queue,
            "world model globals",
            bytemuck::cast_slice(&globals),
            wgpu::BufferUsages::UNIFORM,
        );
        let group = self.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: None,
            layout: &self.models.layout.as_ref().unwrap().0,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: ubuf.as_entire_binding(),
            }],
        });
        let (vertices, indices) = if p.verts.is_empty() {
            (None, None)
        } else {
            (
                Some(crate::buffer_with(
                    &self.device,
                    &self.queue,
                    "world model vertices",
                    bytemuck::cast_slice(&p.verts),
                    wgpu::BufferUsages::VERTEX,
                )),
                Some(crate::buffer_with(
                    &self.device,
                    &self.queue,
                    "world model indices",
                    bytemuck::cast_slice(&p.indices),
                    wgpu::BufferUsages::INDEX,
                )),
            )
        };
        for d in &p.draws {
            self.model_pipeline(d.blend.min(7), d.depth_write, false);
            if d.faded {
                self.model_pipeline(DEPTH_ONLY, true, false);
            }
        }
        for d in &skin_draws {
            self.model_pipeline(d.blend.min(7), d.depth_write, true);
            if d.faded {
                self.model_pipeline(DEPTH_ONLY, true, true);
            }
        }
        let skinned = match (&self.models.skin_layout, skin_draws.is_empty()) {
            (Some(Some((layout, _))), false) => {
                let records: Vec<Record> = skin_draws.iter().map(|d| d.record).collect();
                // (a binding holds one bone at least)
                if bones.is_empty() {
                    bones.push(m2::IDENTITY);
                }
                let storage = |label, bytes: &[u8]| {
                    crate::buffer_with(
                        &self.device,
                        &self.queue,
                        label,
                        bytes,
                        wgpu::BufferUsages::STORAGE,
                    )
                };
                let records = storage("world model records", bytemuck::cast_slice(&records));
                let bones = storage("world model bones", bytemuck::cast_slice(&bones));
                let group = self.device.create_bind_group(&wgpu::BindGroupDescriptor {
                    label: Some("world model skin"),
                    layout,
                    entries: &[
                        wgpu::BindGroupEntry {
                            binding: 0,
                            resource: records.as_entire_binding(),
                        },
                        wgpu::BindGroupEntry {
                            binding: 1,
                            resource: bones.as_entire_binding(),
                        },
                    ],
                });
                Some((group, skin_draws))
            }
            _ => None,
        };
        // (the lists kept for the next frame's, their room with them)
        self.world_verts = p.verts;
        self.world_indices = p.indices;
        Some(WorldModels {
            vertices,
            indices,
            group,
            draws: p.draws,
            skinned,
        })
    }

    /// Draws the world's models into its pass: the ones posed on the CPU,
    /// then the ones the GPU poses (each batch its record, the draw's
    /// instance; decision T540).
    ///
    /// The actors the camera's closeness fades come last (T562): their
    /// depth first, then their colors over their nearest surfaces only, so
    /// one fades whole and what is behind it shows through.
    pub(crate) fn record_models<'a>(&'a self, pass: &mut wgpu::RenderPass<'a>, m: &'a WorldModels) {
        // (0: the others; 1: the faded's depth; 2: the faded's colors)
        for step in 0..3 {
            let wanted = |faded: bool| (step == 0) != faded;
            let key = |blend: u16, depth_write: bool, skinned: bool| {
                if step == 1 {
                    (DEPTH_ONLY, true, skinned)
                } else {
                    (blend.min(7), depth_write, skinned)
                }
            };
            pass.set_bind_group(0, &m.group, &[]);
            if let (Some(vertices), Some(indices)) = (&m.vertices, &m.indices) {
                pass.set_vertex_buffer(0, vertices.slice(..));
                pass.set_index_buffer(indices.slice(..), wgpu::IndexFormat::Uint32);
                for d in m.draws.iter().filter(|d| wanted(d.faded)) {
                    pass.set_pipeline(&self.models.pipelines[&key(d.blend, d.depth_write, false)]);
                    pass.set_bind_group(1, &self.models.groups[&d.tex], &[]);
                    pass.draw_indexed(d.start..d.end, 0, 0..1);
                }
            }
            let Some((group, draws)) = &m.skinned else {
                continue;
            };
            pass.set_bind_group(2, group, &[]);
            let mut bound = None;
            for (i, d) in draws.iter().enumerate().filter(|(_, d)| wanted(d.faded)) {
                let Some((vertices, indices)) = self.models.meshes.get(&d.file) else {
                    continue;
                };
                if bound != Some(d.file) {
                    pass.set_vertex_buffer(0, vertices.slice(..));
                    pass.set_index_buffer(indices.slice(..), wgpu::IndexFormat::Uint32);
                    bound = Some(d.file);
                }
                pass.set_pipeline(&self.models.pipelines[&key(d.blend, d.depth_write, true)]);
                pass.set_bind_group(1, &self.models.groups[&d.tex], &[]);
                let i = i as u32;
                pass.draw_indexed(d.start..d.end, 0, i..i + 1);
            }
        }
    }

    /// One part's batches, posed, into the vertex list.
    #[allow(clippy::too_many_arguments)]
    fn draw_part(
        &mut self,
        l: &Loaded,
        part: &wowlua_casc::display::Part,
        actor: &scene::Actor,
        display: &Display,
        camera: &ModelCamera,
        world: Mat4,
        seq: Option<usize>,
        ms: u32,
        body_points: &mut Option<Body>,
        portrait: &mut Option<(Vec3, Vec3, f32)>,
        own_camera: &mut Option<(Vec3, Vec3, f32)>,
        verts: &mut Vec<MVertex>,
        indices: &mut Vec<u32>,
        draws: &mut Vec<Draw>,
        lo: &mut [f32; 3],
        hi: &mut [f32; 3],
        assets: &mut dyn Assets,
    ) {
        let (m, skin) = (&l.model, &l.skin);
        let time = seq.map_or(0, |s| {
            let d = m.sequences[s].duration;
            if d > 0 { ms % d } else { 0 }
        });
        let mut part_world = world;
        if let Some(att) = part.attachment {
            let Some(a) = body_points.as_ref().and_then(|p| p.points.get(&att)) else {
                return;
            };
            part_world = m2::mul(&world, a);
        }
        // Billboarded bones face the camera, in the part's own space.
        let facing = match body_points.as_ref() {
            Some(b) => b.facing,
            None => camera_facing(m, camera, &world),
        };
        let toward = m2::Facing {
            toward: into_model(&part_world, facing.0),
            up: into_model(&part_world, facing.1),
        };
        // A part made for the body's skeleton takes its pose.
        let pose = match body_points.as_ref() {
            Some(b) if part.skinned && b.pose.len() >= m.bones.len() => b.pose.clone(),
            _ => m.pose_facing(seq, time, Some(toward)),
        };
        // (posed on the GPU: its bones once, its batches as records; T540)
        let bone_base = self.models.skinned_out.as_mut().map(|out| {
            let base = out.bones.len() as u32;
            out.bones.extend_from_slice(&pose);
            base
        });
        if bone_base.is_some() {
            self.skin_mesh(part.file, l);
        }
        // (each vertex posed and placed once, as a visible triangle first
        // uses it: hidden geosets cost nothing, shared corners once; the
        // cache's stamp tells this part's from earlier ones, T538)
        self.posed_stamp = self.posed_stamp.wrapping_add(1);
        if self.posed_stamp == 0 {
            self.posed.clear();
            self.posed_stamp = 1;
        }
        let stamp = self.posed_stamp;
        if self.posed.len() < m.vertices.len() {
            self.posed.resize(m.vertices.len(), (0, [0.0; 3], [0.0; 3]));
        }
        let mut posed = std::mem::take(&mut self.posed);
        if self.emitted.len() < m.vertices.len() {
            self.emitted.resize(m.vertices.len(), (0, 0));
        }
        let mut emitted = std::mem::take(&mut self.emitted);
        if part.attachment.is_none() && body_points.is_none() {
            *body_points = Some(Body {
                points: m
                    .attachments
                    .iter()
                    .filter_map(|a| Some((a.id, m.attachment(a.id, &pose)?)))
                    .collect(),
                pose: pose.clone(),
                facing,
            });
            if let Some(c) = m.camera(0) {
                *portrait = Some((
                    m2::transform(&world, c.position),
                    m2::transform(&world, c.target),
                    c.fov,
                ));
            }
            if let ModelCamera::Frame {
                camera: Some(i), ..
            } = camera
                && let Some(c) = m.cameras.get((*i).max(0) as usize)
            {
                *own_camera = Some((
                    m2::transform(&world, c.position),
                    m2::transform(&world, c.target),
                    c.fov,
                ));
            }
        }
        // Opaque batches first, then the rest by priority.
        let mut order: Vec<usize> = (0..skin.batches.len()).collect();
        order.sort_by_key(|&b| {
            let batch = &skin.batches[b];
            let blend = m
                .materials
                .get(batch.material as usize)
                .map_or(0, |x| x.blend);
            (blend > 1, batch.priority, b)
        });
        for b in order {
            let batch = skin.batches[b];
            let Some(sub) = skin.submeshes.get(batch.submesh as usize) else {
                continue;
            };
            if !part.shows(sub.id) {
                continue;
            }
            let mat = m
                .materials
                .get(batch.material as usize)
                .copied()
                .unwrap_or(m2::Material { flags: 0, blend: 0 });
            let tint = m.tint(&batch, seq, time);
            let alpha = tint.alpha * actor.alpha * display.alpha;
            if alpha <= 0.002 {
                continue;
            }
            // The batch's textures (up to three) and their shaders.
            let count = (batch.texture_count as usize).clamp(1, 3);
            let mut tex = [self.white; 3];
            for (k, slot) in tex.iter_mut().enumerate().take(count) {
                let index = m
                    .texture_combos
                    .get(batch.texture_combo as usize + k)
                    .copied()
                    .unwrap_or(0) as usize;
                *slot = match m.textures.get(index) {
                    Some(t) if t.kind == 0 => self.model_texture(t.file, assets),
                    Some(t) => match part.textures.get(&t.kind) {
                        Some(Texture::File(f)) => self.model_texture(*f, assets),
                        Some(Texture::Composite(c)) => self.composite(c, assets),
                        None => self.white,
                    },
                    None => self.white,
                };
            }
            self.model_group(tex);
            let (pixel, vertex) = crate::combiners::shaders(batch.shader, batch.texture_count);
            let uv_m = m.uv_transform(&batch, seq, time);
            let uv2_m = m.uv_transform_of(&batch, 1, seq, time);
            // (a faded actor's opaque parts blended, its alpha seen: the
            // camera close to a unit, decision T562)
            let faded = actor.alpha < 0.999;
            let blend = if faded && mat.blend <= 1 {
                2
            } else {
                mat.blend
            };
            let mut flags = u32::from(blend.min(255)) << 8;
            if mat.flags & 1 != 0 {
                flags |= UNLIT;
            }
            match blend {
                0 => flags |= OPAQUE,
                1 => flags |= ALPHA_KEY,
                _ => {}
            }
            let shaders = pixel | (vertex << 8);
            let color = [tint.color[0], tint.color[1], tint.color[2], alpha];
            if let (Some(base), Some(out)) = (bone_base, self.models.skinned_out.as_mut()) {
                let first = sub.index_start as usize;
                let end = (first + sub.index_count as usize).min(skin.indices.len());
                if end > first {
                    out.draws.push(SkinDraw {
                        file: part.file,
                        start: first as u32,
                        end: end as u32,
                        blend,
                        depth_write: !faded && mat.blend <= 1 && mat.flags & 0x10 == 0,
                        faded,
                        tex,
                        record: Record {
                            world: part_world,
                            uv1: uv_m.unwrap_or(m2::IDENTITY),
                            uv2: uv2_m.unwrap_or(m2::IDENTITY),
                            color,
                            info: [flags, shaders, base, pose.len() as u32],
                        },
                    });
                }
                continue;
            }
            // (the batch's vertices once each, its corners as indices)
            self.emit_stamp = self.emit_stamp.wrapping_add(1);
            if self.emit_stamp == 0 {
                emitted.iter_mut().for_each(|e| e.0 = 0);
                self.emit_stamp = 1;
            }
            let batch_stamp = self.emit_stamp;
            let start = indices.len() as u32;
            let first = sub.index_start as usize;
            let end = (first + sub.index_count as usize).min(skin.indices.len());
            for &vi in &skin.indices[first.min(end)..end] {
                if let Some(e) = emitted.get(vi as usize)
                    && e.0 == batch_stamp
                {
                    indices.push(e.1);
                    continue;
                }
                let Some(slot) = posed.get_mut(vi as usize) else {
                    continue;
                };
                if slot.0 != stamp {
                    let Some((p, n)) = m.skinned_vertex(vi as usize, &pose) else {
                        continue;
                    };
                    *slot = (
                        stamp,
                        m2::transform(&part_world, p),
                        normalize(m2::transform_dir(&part_world, n)),
                    );
                }
                let (wp, wn) = (slot.1, slot.2);
                let v = &m.vertices[vi as usize];
                for k in 0..3 {
                    lo[k] = lo[k].min(wp[k]);
                    hi[k] = hi[k].max(wp[k]);
                }
                let through = |m: &Option<Mat4>, uv: [f32; 2]| match m {
                    Some(t) => {
                        let q = m2::transform(t, [uv[0], uv[1], 0.0]);
                        [q[0], q[1]]
                    }
                    None => uv,
                };
                emitted[vi as usize] = (batch_stamp, verts.len() as u32);
                indices.push(verts.len() as u32);
                verts.push(MVertex {
                    pos: wp,
                    normal: wn,
                    t1: through(&uv_m, v.uv),
                    t2: through(&uv2_m, v.uv2),
                    raw2: v.uv2,
                    color,
                    flags,
                    shaders,
                });
            }
            let end = indices.len() as u32;
            if end > start {
                draws.push(Draw {
                    blend,
                    depth_write: !faded && mat.blend <= 1 && mat.flags & 0x10 == 0,
                    faded,
                    tex,
                    start,
                    end,
                });
            }
        }
        self.posed = posed;
        self.emitted = emitted;
        // Its ribbons: where their bones' points were over their edges'
        // lifetimes (the model posed at those moments), each edge its
        // height above and below the point along the bone's Z axis, the
        // older ones pulled down by its gravity.
        for (k, rb) in m.ribbons.iter().enumerate() {
            let Some(look) = m.ribbon_look(k, seq, time) else {
                continue;
            };
            let a = look.alpha * actor.alpha * display.alpha;
            if rb.edges_per_second <= 0.0 || rb.edge_lifetime <= 0.0 || a <= 0.002 {
                continue;
            }
            let edges =
                ((rb.edge_lifetime * rb.edges_per_second).ceil() as usize).clamp(1, RIBBON_EDGES);
            let step = 1000.0 / rb.edges_per_second;
            let duration = seq.map_or(0, |s| m.sequences[s].duration);
            let points: Vec<(Vec3, Vec3)> = (0..=edges)
                .map(|i| {
                    let back = (i as f32 * step) as i64;
                    let t = if duration > 0 {
                        (i64::from(ms) - back).rem_euclid(i64::from(duration)) as u32
                    } else {
                        0
                    };
                    let posed = m.pose_facing(seq, t, Some(toward));
                    let bone = posed.get(rb.bone as usize).copied().unwrap_or(m2::IDENTITY);
                    let at = m2::mul(&part_world, &bone);
                    let mut p = m2::transform(&at, rb.position);
                    let age = i as f32 / rb.edges_per_second;
                    p[2] -= 0.5 * rb.gravity * age * age;
                    let up = m2::transform_dir(&at, [0.0, 0.0, 1.0]);
                    let l = dot(up, up).sqrt().max(1e-6);
                    (p, [up[0] / l, up[1] / l, up[2] / l])
                })
                .collect();
            let index = rb.textures.first().copied().unwrap_or(0) as usize;
            let tex = match m.textures.get(index) {
                Some(t) if t.kind == 0 => self.model_texture(t.file, assets),
                Some(t) => match part.textures.get(&t.kind) {
                    Some(Texture::File(f)) => self.model_texture(*f, assets),
                    _ => self.white,
                },
                None => self.white,
            };
            self.model_group([tex, self.white, self.white]);
            let blend = rb
                .materials
                .first()
                .and_then(|i| m.materials.get(*i as usize))
                .map_or(2, |mt| mt.blend);
            let color = [look.color[0], look.color[1], look.color[2], a];
            let edge = |(p, up): (Vec3, Vec3)| -> (Vec3, Vec3) {
                (
                    [
                        p[0] + up[0] * look.above,
                        p[1] + up[1] * look.above,
                        p[2] + up[2] * look.above,
                    ],
                    [
                        p[0] - up[0] * look.below,
                        p[1] - up[1] * look.below,
                        p[2] - up[2] * look.below,
                    ],
                )
            };
            for (i, pair) in points.windows(2).enumerate() {
                let (head_top, head_bottom) = edge(pair[0]);
                let (tail_top, tail_bottom) = edge(pair[1]);
                let (u0, u1) = (i as f32 / edges as f32, (i + 1) as f32 / edges as f32);
                self.models.sprites.push(Sprite {
                    pos: pair[0].0,
                    size: 0.0,
                    spin: 0.0,
                    color,
                    tex,
                    blend,
                    cell: [u0, 0.0, u1, 1.0],
                    tail: None,
                    corners: Some([head_top, head_bottom, tail_top, tail_bottom]),
                });
            }
        }
        // Its particles: the emitters' steady state at the moment.
        for (k, e) in m.particles.iter().enumerate() {
            let life = e.lifespan * (1.0 + e.lifespan_variation.abs());
            let rate = e.rate * (1.0 + e.rate_variation.abs() * 0.5);
            if life <= 0.0 || rate <= 0.0 || e.alphas.iter().all(|(_, a)| *a <= 0.0) {
                continue;
            }
            let bone = pose.get(e.bone as usize).copied().unwrap_or(m2::IDENTITY);
            let emitter = m2::mul(&m2::mul(&part_world, &bone), &m2::translation(e.position));
            let index = if e.flags & 0x1000_0000 != 0 {
                e.texture & 0x1f
            } else {
                e.texture
            } as usize;
            let tex = match m.textures.get(index) {
                Some(t) if t.kind == 0 => self.model_texture(t.file, assets),
                Some(t) => match part.textures.get(&t.kind) {
                    Some(Texture::File(f)) => self.model_texture(*f, assets),
                    _ => self.white,
                },
                None => self.white,
            };
            self.model_group([tex, self.white, self.white]);
            let now = ms as f64 / 1000.0;
            let spawned = (now * f64::from(rate)).floor();
            let frac = (now * f64::from(rate) - spawned) as f32;
            let count = ((life * rate).ceil() as u32).min(400);
            for n in 0..count {
                let age = (n as f32 + frac) / rate;
                let seed = (u64::from(part.file) << 20)
                    ^ ((k as u64) << 52)
                    ^ (spawned as u64).wrapping_sub(u64::from(n));
                let mut rng = Rng::new(seed);
                let span = e.lifespan * (1.0 + e.lifespan_variation * rng.signed());
                if span <= 0.0 || age > span {
                    continue;
                }
                let start = match e.kind {
                    1 => [
                        rng.signed() * e.length * 0.5,
                        rng.signed() * e.width * 0.5,
                        0.0,
                    ],
                    2 => {
                        let (a, b) = (rng.unit() * std::f32::consts::TAU, rng.signed().acos());
                        let r = e.length * rng.unit();
                        [r * b.sin() * a.cos(), r * b.sin() * a.sin(), r * b.cos()]
                    }
                    _ => [0.0; 3],
                };
                let v = rng.signed() * e.vertical_range;
                let h = rng.unit() * e.horizontal_range;
                let dir = [v.sin() * h.cos(), v.sin() * h.sin(), v.cos()];
                let speed = e.speed * (1.0 + e.speed_variation * rng.signed());
                let g = e.gravity;
                let p = [
                    start[0] + dir[0] * speed * age + 0.5 * g[0] * age * age,
                    start[1] + dir[1] * speed * age + 0.5 * g[1] * age * age,
                    start[2] + dir[2] * speed * age + 0.5 * g[2] * age * age,
                ];
                let velocity = [
                    dir[0] * speed + g[0] * age,
                    dir[1] * speed + g[1] * age,
                    dir[2] * speed + g[2] * age,
                ];
                let t = age / span;
                // Its texture cell: a random one (flag 0x10000), else its
                // track's at its age, else the first.
                let cells = u32::from(e.columns) * u32::from(e.rows);
                let random = (rng.unit() * cells as f32) as u32;
                let cell_at = |track: &[(f32, f32)]| -> u32 {
                    let n = if e.flags & 0x10000 != 0 {
                        random
                    } else {
                        m2::Particle::at(track, t, 0.0).max(0.0) as u32
                    };
                    n.min(cells.saturating_sub(1))
                };
                let (cu, cv) = (1.0 / f32::from(e.columns), 1.0 / f32::from(e.rows));
                let cell = |n: u32| {
                    let (c, r) = (
                        (n % u32::from(e.columns)) as f32,
                        (n / u32::from(e.columns)) as f32,
                    );
                    [c * cu, r * cv, (c + 1.0) * cu, (r + 1.0) * cv]
                };
                let c = m2::Particle::at(&e.colors, t, [255.0; 3]);
                let a = m2::Particle::at(&e.alphas, t, 1.0) * actor.alpha * display.alpha;
                let size = m2::Particle::at(&e.sizes, t, [1.0, 1.0])[0]
                    * (1.0 + e.size_variation * rng.signed());
                if a <= 0.002 || size <= 0.0 {
                    continue;
                }
                let pos = m2::transform(&emitter, p);
                let color = [c[0] / 255.0, c[1] / 255.0, c[2] / 255.0, a];
                // Its head, and its tail back along its velocity (1 a tail
                // alone, 2 both; other values, which emitters of the newer
                // kind give, a head).
                if e.head_or_tail != 1 {
                    self.models.sprites.push(Sprite {
                        pos,
                        size,
                        spin: e.spin * age,
                        color,
                        tex,
                        blend: u16::from(e.blend),
                        cell: cell(cell_at(&e.head_cells)),
                        tail: None,
                        corners: None,
                    });
                }
                if matches!(e.head_or_tail, 1 | 2) && e.tail_length > 0.0 {
                    let back = [
                        p[0] - velocity[0] * e.tail_length,
                        p[1] - velocity[1] * e.tail_length,
                        p[2] - velocity[2] * e.tail_length,
                    ];
                    let track = if e.tail_cells.is_empty() {
                        &e.head_cells
                    } else {
                        &e.tail_cells
                    };
                    self.models.sprites.push(Sprite {
                        pos,
                        size,
                        spin: 0.0,
                        color,
                        tex,
                        blend: u16::from(e.blend),
                        cell: cell(cell_at(track)),
                        tail: Some(m2::transform(&emitter, back)),
                        corners: None,
                    });
                }
            }
        }
    }

    /// The camera, and the vertices drawn into the model's texture.
    #[allow(clippy::too_many_arguments)]
    fn finish_model(
        &mut self,
        item: &scene::Model,
        width: u32,
        height: u32,
        aspect: f32,
        mut verts: Vec<MVertex>,
        mut indices: Vec<u32>,
        mut draws: Vec<Draw>,
        lo: [f32; 3],
        hi: [f32; 3],
        portrait: Option<(Vec3, Vec3, f32)>,
        own_camera: Option<(Vec3, Vec3, f32)>,
    ) -> Option<usize> {
        if verts.is_empty() && self.models.sprites.is_empty() {
            return None;
        }
        // The camera.
        let (view, proj) = match &item.camera {
            ModelCamera::Scene {
                position,
                forward,
                up,
                fov,
                near,
                far,
                ..
            } => (
                look_at(*position, *forward, *up),
                perspective(fov.max(0.01), aspect, near.max(0.001), far.max(near + 0.01)),
            ),
            ModelCamera::Frame {
                portrait_zoom,
                distance_scale,
                ..
            } => {
                let (eye, target, fov_y) = match own_camera {
                    Some((eye, target, fov)) => (eye, target, vertical_fov(fov, aspect)),
                    None => {
                        // The whole model in view from in front (+X).
                        let center = lerp3(lo, hi, 0.5);
                        let fov_y: f32 = 0.6;
                        let half_h = (hi[2] - lo[2]) / 2.0;
                        let half_w = (hi[1] - lo[1]) / 2.0;
                        let t = (fov_y / 2.0).tan();
                        let dist =
                            (half_h / t).max(half_w / (t * aspect)) * 1.05 + (hi[0] - lo[0]) / 2.0;
                        let full = (
                            [center[0] + dist * distance_scale, center[1], center[2]],
                            center,
                            fov_y,
                        );
                        match portrait {
                            Some((pe, pt, pf)) if *portrait_zoom > 0.0 => {
                                let z = portrait_zoom.clamp(0.0, 1.0);
                                (
                                    lerp3(full.0, pe, z),
                                    lerp3(full.1, pt, z),
                                    full.2 + (vertical_fov(pf, aspect) - full.2) * z,
                                )
                            }
                            _ => full,
                        }
                    }
                };
                let forward = sub(target, eye);
                let dist = dot(forward, forward).sqrt();
                let radius = dot(sub(hi, lo), sub(hi, lo)).sqrt().max(0.01);
                (
                    look_at(eye, forward, [0.0, 0.0, 1.0]),
                    perspective(
                        fov_y,
                        aspect,
                        (dist - radius).max(dist * 0.01).max(0.01),
                        dist + radius * 2.0,
                    ),
                )
            }
        };
        let view_proj = m2::mul(&proj, &view);
        // The particles, facing the camera, over the meshes.
        let sprites = std::mem::take(&mut self.models.sprites);
        self.sprite_quads(sprites, &view, &mut verts, &mut indices, &mut draws);
        if verts.is_empty() {
            return None;
        }
        let l = &item.light;
        let (ambient, diffuse) = if l.enabled {
            (l.ambient, l.diffuse)
        } else {
            ([1.0; 3], [0.0; 3])
        };
        let mut globals = Vec::with_capacity(44);
        globals.extend_from_slice(&view_proj);
        globals.extend_from_slice(&view);
        globals.extend_from_slice(&[l.direction[0], l.direction[1], l.direction[2], 0.0]);
        globals.extend_from_slice(&[ambient[0], ambient[1], ambient[2], 1.0]);
        globals.extend_from_slice(&[diffuse[0], diffuse[1], diffuse[2], 1.0]);
        let ubuf = crate::buffer_with(
            &self.device,
            &self.queue,
            "model globals",
            bytemuck::cast_slice(&globals),
            wgpu::BufferUsages::UNIFORM,
        );
        let ugroup = self.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: None,
            layout: &self.models.layout.as_ref().unwrap().0,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: ubuf.as_entire_binding(),
            }],
        });
        let vbuf = crate::buffer_with(
            &self.device,
            &self.queue,
            "model vertices",
            bytemuck::cast_slice(&verts),
            wgpu::BufferUsages::VERTEX,
        );
        let ibuf = crate::buffer_with(
            &self.device,
            &self.queue,
            "model indices",
            bytemuck::cast_slice(&indices),
            wgpu::BufferUsages::INDEX,
        );
        let size = wgpu::Extent3d {
            width,
            height,
            depth_or_array_layers: 1,
        };
        let color = self.device.create_texture(&wgpu::TextureDescriptor {
            label: Some("model"),
            size,
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: crate::FORMAT,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
            view_formats: &[],
        });
        let depth = self.device.create_texture(&wgpu::TextureDescriptor {
            label: Some("model depth"),
            size,
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Depth32Float,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            view_formats: &[],
        });
        let color_view = color.create_view(&wgpu::TextureViewDescriptor::default());
        let depth_view = depth.create_view(&wgpu::TextureViewDescriptor::default());
        for d in &draws {
            self.model_pipeline(d.blend.min(7), d.depth_write, false);
        }
        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("model"),
            });
        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("model"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &color_view,
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(if self.models.portrait {
                            wgpu::Color::BLACK
                        } else {
                            wgpu::Color::TRANSPARENT
                        }),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                    view: &depth_view,
                    depth_ops: Some(wgpu::Operations {
                        load: wgpu::LoadOp::Clear(1.0),
                        store: wgpu::StoreOp::Discard,
                    }),
                    stencil_ops: None,
                }),
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
            pass.set_vertex_buffer(0, vbuf.slice(..));
            pass.set_index_buffer(ibuf.slice(..), wgpu::IndexFormat::Uint32);
            pass.set_bind_group(0, &ugroup, &[]);
            for d in &draws {
                pass.set_pipeline(&self.models.pipelines[&(d.blend.min(7), d.depth_write, false)]);
                pass.set_bind_group(1, &self.models.groups[&d.tex], &[]);
                pass.draw_indexed(d.start..d.end, 0, 0..1);
            }
        }
        self.queue.submit([encoder.finish()]);
        // The frame's model textures take the same slots each frame; a
        // portrait is kept.
        let tex = crate::Tex {
            texture: color,
            view: color_view,
            width,
            height,
        };
        if self.models.portrait {
            self.textures.push(tex);
            return Some(self.textures.len() - 1);
        }
        let slot = match self.model_slots.get(self.model_slot_next).copied() {
            Some(s) => {
                self.textures[s] = tex;
                self.bind_groups.retain(|k, _| k.0 != s && k.1 != s);
                s
            }
            None => {
                self.textures.push(tex);
                self.model_slots.push(self.textures.len() - 1);
                self.textures.len() - 1
            }
        };
        self.model_slot_next += 1;
        Some(slot)
    }
}
