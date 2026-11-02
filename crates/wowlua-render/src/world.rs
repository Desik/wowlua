//! The 3D world behind the UI (decision T522): the terrain of the tiles
//! within the view distance (`farclip`), as the camera sees it, under the
//! sky, lit by the place's light at the realm's time of day and faded into
//! its fog (decision T555: the installation's light tables; without them a
//! plain sky and sun); the UI is drawn over it.
//!
//! A tile is read once (`Assets::terrain`) and kept on the GPU: its
//! chunks' grids (each square four triangles around its middle, holes
//! left out) in one vertex and index buffer, each chunk with its textures
//! and a blend texture of its layers (red, green and blue the second to
//! fourth layer's).
//!
//! The buildings the tiles place (decision T523) are read once each
//! (`Assets::building`) and drawn wherever they stand, by their
//! placement's matrix: their groups' batches with their materials'
//! textures, the blended ones last. The doodads (decision T525), the
//! tiles' and the buildings' own, are their models' standing poses, drawn
//! together for each model.

use std::collections::HashMap;

use wgpu::util::DeviceExt;
use wowlua_casc::adt::{self, CHUNK, TILE, Tile, UNIT};
use wowlua_engine::scene::WorldView;

use crate::{Assets, Renderer, Rgba};

#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct TVertex {
    pos: [f32; 3],
    normal: [f32; 3],
    uv: [f32; 2],
}

#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct Globals {
    view: [f32; 16],
    proj: [f32; 16],
    sun: [f32; 4],
    fog: [f32; 4],
    eye: [f32; 4],
    direct: [f32; 4],
    ambient: [f32; 4],
    /// The light's water colors (T557): rivers' and lakes' near and far,
    /// the sea's, each with its alpha (shallow, deep).
    river: [f32; 4],
    river_far: [f32; 4],
    ocean: [f32; 4],
    ocean_far: [f32; 4],
    /// The units' blob shadows (decision T563): where each stands and its
    /// radius; `sun.w` how many.
    blobs: [[f32; 4]; BLOBS],
    /// The sun's shadow maps (decision T564): each map's matrix, where each
    /// ends (yards ahead of the eye), a texel's size in yards in each; how
    /// many (0: none), soft, blended where they meet, a texel across (0 to
    /// 1); the way the eye looks.
    shadow_mats: [[f32; 16]; 4],
    shadow_ends: [f32; 4],
    shadow_texels: [f32; 4],
    shadow: [f32; 4],
    look: [f32; 4],
    /// The terrain's shine (decision T579): x on (`specular`), y its
    /// exponent.
    terrain: [f32; 4],
}

/// The terrain's shine's exponent (decision T579; unverified).
const TERRAIN_SHINE: f32 = 20.0;

/// The game's blob shadow (`textures/shadowblob.blp`, decision T563).
const BLOB_FILE: u32 = 186_183;
/// The most units with a blob shadow, the nearest the eye.
const BLOBS: usize = 16;

/// A liquid's surface point (`liquid.wgsl`).
#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct LVertex {
    pos: [f32; 3],
    uv: [f32; 2],
    depth: f32,
}

/// A liquid type's part of a tile's liquid mesh: its frames (textures),
/// whether it glows, whether it is the sea, its indices.
struct LiquidDraw {
    frames: Vec<usize>,
    /// How it is drawn: glowing, the sea, its own colors (`liquid.wgsl`).
    how: wgpu::BindGroup,
    start: u32,
    count: u32,
}

struct GpuLiquid {
    vertices: wgpu::Buffer,
    indices: wgpu::Buffer,
    draws: Vec<LiquidDraw>,
}

struct LiquidSetup {
    pipeline: wgpu::RenderPipeline,
    frame_layout: wgpu::BindGroupLayout,
    how_layout: wgpu::BindGroupLayout,
    /// Each frame's texture (by its index) bound.
    frames: HashMap<usize, wgpu::BindGroup>,
}

/// A liquid type's part of a tile's mesh: its frames (files), glowing, the
/// sea, its own colors (near, far), its indices' start and count.
type LiquidPart = (Vec<u32>, bool, bool, Option<([f32; 3], [f32; 3])>, u32, u32);

/// The liquids' animation: frames a second (unverified: a 30-frame
/// cycle in two seconds).
const LIQUID_FPS: f64 = 15.0;

/// The sky pass's (`sky.wgsl`).
#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct SkyGlobals {
    colors: [[f32; 4]; 6],
    forward: [f32; 4],
    right: [f32; 4],
    up: [f32; 4],
}

/// Without the light tables: the sky's and the fog's color.
const SKY: [f32; 3] = [0.62, 0.74, 0.86];
/// Without them: toward the sun (north, west, up), and how much light the
/// shade keeps.
const SUN: [f32; 4] = [0.35, 0.25, 0.9, 0.45];
/// Where the fog starts, of the view distance.
const FOG_START: f32 = 0.55;
/// The nearest the camera draws (yards).
const NEAR: f32 = 0.2;
/// A unit as seen by an eye close to it (decision T562): invisible with
/// the eye inside its body (within 0.4 yards of its upright line, 2.2
/// yards from its feet), fading in over the next half yard, else whole:
/// only a very close camera fades it (the distances unverified).
fn near_fade(eye: [f64; 3], at: [f64; 3]) -> f32 {
    const BODY: f64 = 0.4;
    const FADE: f64 = 0.5;
    const HEIGHT: f64 = 2.2;
    let z = eye[2].clamp(at[2], at[2] + HEIGHT);
    let d = ((eye[0] - at[0]).powi(2) + (eye[1] - at[1]).powi(2) + (eye[2] - z).powi(2)).sqrt();
    ((d - BODY) / FADE).clamp(0.0, 1.0) as f32
}

/// How much of the light a shadow takes with no light tables
/// (decision T563).
const SHADOW: f32 = 0.4;

/// A blob shadow's radius for a body reaching `reach` yards across the
/// ground (decision T563; the game's unverified): twice as wide (a human's
/// about a yard across), at least 0.4 yards.
fn blob_radius(reach: f32) -> f32 {
    (reach * 2.0).max(0.4)
}

/// How far ahead of the eye the sun's shadow maps reach (yards; decision
/// T564, unverified): none at `shadowMode` 0, the units' shadows as far
/// as `entityShadowFadeScale` at 1, then 100, 200 and 300 yards; within
/// the view distance.
fn sun_reach(s: &wowlua_engine::scene::Shadows, far: f64) -> f64 {
    let reach = match s.mode {
        0 => 0.0,
        1 => s.entity_fade,
        2 => 100.0,
        3 => 200.0,
        _ => 300.0,
    };
    reach.min(far)
}

/// How far behind its stretch of the view a shadow map reaches toward the
/// sun (yards): what stands there casts into it.
const SUN_BACK: f64 = 300.0;

/// One of the sun's shadow maps (decision T564): the sun's view and its
/// box, where it ends ahead of the eye, a texel's size (yards), the box's
/// half width and depth.
#[derive(Clone, Debug)]
struct Cascade {
    view: [f32; 16],
    proj: [f32; 16],
    end: f32,
    texel: f32,
    radius: f64,
    depth: f64,
}

impl Cascade {
    /// Whether a ball (`middle`, `reach` yards) is within the box.
    fn covers(&self, middle: [f64; 3], reach: f64) -> bool {
        let v = &self.view;
        let at = |r: usize| {
            f64::from(v[r]) * middle[0]
                + f64::from(v[4 + r]) * middle[1]
                + f64::from(v[8 + r]) * middle[2]
                + f64::from(v[12 + r])
        };
        let (x, y, z) = (at(0), at(1), -at(2));
        x.abs() <= self.radius + reach
            && y.abs() <= self.radius + reach
            && z >= -reach
            && z <= self.depth + reach
    }
}

/// The sun's shadow maps for the world's view (decision T564): `count`
/// maps splitting the first `reach` yards ahead of the eye (the nearer
/// smaller, the split 0.8 logarithmic), each a square about its stretch
/// of the view seen from `toward` (the way to the sun) from `SUN_BACK`
/// yards behind it, its middle kept on its texels so its shadows hold
/// still as the eye moves.
fn cascades(
    w: &WorldView,
    tan_x: f64,
    tan_y: f64,
    toward: [f64; 3],
    reach: f64,
    count: u32,
    size: u32,
) -> Vec<Cascade> {
    const LAMBDA: f64 = 0.8;
    let n = f64::from(count.max(1));
    let first = 1.0f64.min(reach);
    let split = |i: f64| {
        LAMBDA * first * (reach / first).powf(i / n)
            + (1.0 - LAMBDA) * (first + (reach - first) * i / n)
    };
    let right = [
        w.look[1] * w.up[2] - w.look[2] * w.up[1],
        w.look[2] * w.up[0] - w.look[0] * w.up[2],
        w.look[0] * w.up[1] - w.look[1] * w.up[0],
    ];
    let len = (toward[0].powi(2) + toward[1].powi(2) + toward[2].powi(2)).sqrt();
    let toward = toward.map(|v| v / len.max(1e-9));
    let mut out = Vec::new();
    let mut near = f64::from(NEAR);
    for i in 1..=count.max(1) {
        let far = split(f64::from(i));
        let corners: Vec<[f64; 3]> = [near, far]
            .iter()
            .flat_map(|&d| {
                [(-1.0, -1.0), (-1.0, 1.0), (1.0, -1.0), (1.0, 1.0)].map(|(sx, sy)| {
                    [0, 1, 2].map(|k| {
                        w.eye[k]
                            + w.look[k] * d
                            + right[k] * sx * d * tan_x
                            + w.up[k] * sy * d * tan_y
                    })
                })
            })
            .collect();
        let middle = [0, 1, 2].map(|k| corners.iter().map(|c| c[k]).sum::<f64>() / 8.0);
        let radius = corners
            .iter()
            .map(|c| {
                ((c[0] - middle[0]).powi(2)
                    + (c[1] - middle[1]).powi(2)
                    + (c[2] - middle[2]).powi(2))
                .sqrt()
            })
            .fold(0.0, f64::max);
        // (a steady size: whole half yards)
        let radius = (radius * 2.0).ceil() / 2.0;
        let back = radius + SUN_BACK;
        let from = [0, 1, 2].map(|k| (middle[k] + toward[k] * back) as f32);
        let up = if toward[2].abs() > 0.99 {
            [1.0, 0.0, 0.0]
        } else {
            [0.0, 0.0, 1.0]
        };
        let mut view = crate::model::look_at(
            from,
            [-toward[0] as f32, -toward[1] as f32, -toward[2] as f32],
            up,
        );
        let texel = 2.0 * radius / f64::from(size.max(1));
        for k in [12, 13] {
            view[k] = ((f64::from(view[k]) / texel).round() * texel) as f32;
        }
        let depth = back + radius;
        let mut proj = [0.0f32; 16];
        proj[0] = (1.0 / radius) as f32;
        proj[5] = (1.0 / radius) as f32;
        proj[10] = (-1.0 / depth) as f32;
        proj[15] = 1.0;
        out.push(Cascade {
            view,
            proj,
            end: far as f32,
            texel: texel as f32,
            radius,
            depth,
        });
        near = far;
    }
    out
}

impl Renderer {
    /// Whether the eye is inside a placed building's indoor group (decision
    /// T581): the eye in the building's own space (its placement's axes,
    /// scaled) within the group's bounds.
    fn eye_indoors(&self, placed: &[(u32, [f32; 16])], eye: [f64; 3]) -> bool {
        placed.iter().any(|(file, m)| {
            let Some(Some(b)) = self.world.buildings.get(file) else {
                return false;
            };
            let d = [0, 1, 2].map(|r| eye[r] - f64::from(m[12 + r]));
            let local = [0, 1, 2].map(|k| {
                let a = [m[k * 4], m[k * 4 + 1], m[k * 4 + 2]].map(f64::from);
                let len2 = (a[0] * a[0] + a[1] * a[1] + a[2] * a[2]).max(1e-9);
                (d[0] * a[0] + d[1] * a[1] + d[2] * a[2]) / len2
            });
            b.groups.iter().filter_map(|g| g.indoor).any(|[lo, hi]| {
                (0..3).all(|k| local[k] >= f64::from(lo[k]) && local[k] <= f64::from(hi[k]))
            })
        })
    }

    /// Draws the sun's shadow maps (decision T564), each the depth it sees
    /// in its box: by `shadowMode`, the units (1), the buildings and props
    /// (2), the ground (3 and up).
    #[allow(clippy::too_many_arguments)]
    fn draw_sun_maps(
        &self,
        encoder: &mut wgpu::CommandEncoder,
        w: &WorldView,
        maps: &[Cascade],
        detailed: &[(i32, i32)],
        placed: &[(u32, [f32; 16])],
        sun_ranges: &[Vec<(u32, u32, u32)>],
        models: Option<&crate::model::WorldModels>,
        model_groups: &[wgpu::BindGroup],
    ) {
        let setup = self.world.setup.as_ref().expect("the world's setup");
        let Some(layers) = setup.maps.as_ref().map(|m| &m.layers) else {
            return;
        };
        let mode = w.shadows.mode;
        for (k, c) in maps.iter().enumerate() {
            let Some(layer) = layers.get(k) else {
                break;
            };
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("world sun"),
                color_attachments: &[],
                depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                    view: layer,
                    depth_ops: Some(wgpu::Operations {
                        load: wgpu::LoadOp::Clear(1.0),
                        store: wgpu::StoreOp::Store,
                    }),
                    stencil_ops: None,
                }),
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
            let group = &setup.sun[k].1;
            if mode >= 3 {
                pass.set_pipeline(&setup.sun_pipeline);
                pass.set_bind_group(0, group, &[]);
                for &(x, y) in detailed {
                    let Some(Some(tile)) = self.world.tiles.get(&(w.map, x, y)) else {
                        continue;
                    };
                    pass.set_vertex_buffer(0, tile.vertices.slice(..));
                    pass.set_index_buffer(tile.indices.slice(..), wgpu::IndexFormat::Uint32);
                    for ch in &tile.chunks {
                        if ch.count == 0 || !c.covers(ch.center, CHUNK) {
                            continue;
                        }
                        pass.draw_indexed(ch.start..ch.start + ch.count, 0, 0..1);
                    }
                }
            }
            if mode >= 2
                && let (Some(bsetup), Some((instances, _))) = (
                    self.world.building_setup.as_ref(),
                    self.world.instances.as_ref(),
                )
            {
                pass.set_pipeline(&bsetup.sun);
                pass.set_bind_group(0, group, &[]);
                for (i, (file, m)) in placed.iter().enumerate() {
                    let Some(Some(b)) = self.world.buildings.get(file) else {
                        continue;
                    };
                    let at = i as u64 * 64;
                    pass.set_vertex_buffer(1, instances.slice(at..at + 64));
                    let axis = |k: usize| [m[k * 4], m[k * 4 + 1], m[k * 4 + 2]];
                    let scale = (0..3)
                        .map(|k| {
                            let a = axis(k);
                            (a[0] * a[0] + a[1] * a[1] + a[2] * a[2]).sqrt()
                        })
                        .fold(0.0f32, f32::max);
                    for g in &b.groups {
                        let mid = g.middle;
                        let world = [0, 1, 2].map(|r| {
                            f64::from(
                                m[12 + r] + m[r] * mid[0] + m[4 + r] * mid[1] + m[8 + r] * mid[2],
                            )
                        });
                        if !c.covers(world, f64::from(g.reach * scale)) {
                            continue;
                        }
                        pass.set_vertex_buffer(0, g.vertices.slice(..));
                        pass.set_index_buffer(g.indices.slice(..), wgpu::IndexFormat::Uint16);
                        for batch in g.batches.iter().filter(|b| !b.blended) {
                            pass.set_bind_group(1, &batch.group, &[]);
                            pass.draw_indexed(batch.start..batch.start + batch.count, 0, 0..1);
                        }
                    }
                }
                // (the props reaching into its box only: T581)
                let Some((sun_instances, _)) = self.world.sun_instances.as_ref() else {
                    continue;
                };
                for &(file, first, count) in sun_ranges.get(k).map_or(&[][..], |r| &r[..]) {
                    let Some(Some(d)) = self.world.doodads.get(&file) else {
                        continue;
                    };
                    let (at, end) = (u64::from(first) * 64, u64::from(first + count) * 64);
                    pass.set_vertex_buffer(1, sun_instances.slice(at..end));
                    pass.set_vertex_buffer(0, d.vertices.slice(..));
                    pass.set_index_buffer(d.indices.slice(..), wgpu::IndexFormat::Uint32);
                    for batch in d.batches.iter().filter(|b| !b.blended) {
                        pass.set_bind_group(1, &batch.group, &[]);
                        pass.draw_indexed(batch.start..batch.start + batch.count, 0, 0..count);
                    }
                }
            }
            if mode >= 1
                && let (Some(m), Some(g)) = (models, model_groups.get(k))
            {
                self.record_models_sun(&mut pass, m, g);
            }
        }
    }
}

/// How far from the eye props move (yards; T558: nearer ones posed one by
/// one, farther ones as they stand).
const PROP_MOTION: f64 = 80.0;
/// How far the sky's models reach about the eye (yards).
const SKY_FAR: f32 = 100_000.0;

/// Where a point of the world (north, west, up yards) shows on a screen
/// `width` by `height` pixels as the world pass draws `w` (top left
/// origin); none behind the eye (decision T546).
pub fn project(w: &WorldView, width: u32, height: u32, p: [f64; 3]) -> Option<[f32; 2]> {
    let f32s = |a: [f64; 3]| [a[0] as f32, a[1] as f32, a[2] as f32];
    let aspect = width as f32 / height.max(1) as f32;
    let fov_y = 2.0 * ((w.fov as f32 / 2.0).tan() / aspect).atan();
    let view = crate::model::look_at(f32s(w.eye), f32s(w.look), f32s(w.up));
    let proj = crate::model::perspective(fov_y, aspect, NEAR, w.far as f32);
    let m = wowlua_casc::m2::mul(&proj, &view);
    let v = [p[0] as f32, p[1] as f32, p[2] as f32, 1.0];
    let row = |r: usize| (0..4).map(|c| m[c * 4 + r] * v[c]).sum::<f32>();
    let (x, y, clip_w) = (row(0), row(1), row(3));
    if clip_w <= NEAR * 0.5 {
        return None;
    }
    Some([
        (x / clip_w + 1.0) / 2.0 * width as f32,
        (1.0 - y / clip_w) / 2.0 * height as f32,
    ])
}

/// The sun's shadow maps (decision T564): one texture of `cascades`
/// layers, `size` texels a side; its view to look them up, each layer's
/// to draw it.
struct SunMaps {
    array: wgpu::TextureView,
    layers: Vec<wgpu::TextureView>,
    size: u32,
    cascades: u32,
}

fn sun_maps(d: &wgpu::Device, size: u32, cascades: u32) -> SunMaps {
    let texture = d.create_texture(&wgpu::TextureDescriptor {
        label: Some("world sun maps"),
        size: wgpu::Extent3d {
            width: size,
            height: size,
            depth_or_array_layers: cascades,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Depth32Float,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
        view_formats: &[],
    });
    let array = texture.create_view(&wgpu::TextureViewDescriptor {
        dimension: Some(wgpu::TextureViewDimension::D2Array),
        ..Default::default()
    });
    let layers = (0..cascades)
        .map(|k| {
            texture.create_view(&wgpu::TextureViewDescriptor {
                dimension: Some(wgpu::TextureViewDimension::D2),
                base_array_layer: k,
                array_layer_count: Some(1),
                ..Default::default()
            })
        })
        .collect();
    SunMaps {
        array,
        layers,
        size,
        cascades,
    }
}

struct GpuChunk {
    start: u32,
    count: u32,
    group: wgpu::BindGroup,
    center: [f64; 3],
}

struct GpuTile {
    vertices: wgpu::Buffer,
    indices: wgpu::Buffer,
    chunks: Vec<GpuChunk>,
    liquid: Option<GpuLiquid>,
}

struct Setup {
    pipeline: wgpu::RenderPipeline,
    sky_pipeline: wgpu::RenderPipeline,
    sky: wgpu::Buffer,
    sky_group: wgpu::BindGroup,
    globals_layout: wgpu::BindGroupLayout,
    globals: wgpu::Buffer,
    globals_group: wgpu::BindGroup,
    /// The blob shadow's texture the globals bind (T563; white: none).
    blob: usize,
    /// The sun's shadow maps (T564): the ground's depth pipeline, each
    /// map's globals as the sun sees (binding no maps), the maps (none
    /// while the settings draw none), what stands in for them, and how
    /// they are compared.
    sun_pipeline: wgpu::RenderPipeline,
    sun: Vec<(wgpu::Buffer, wgpu::BindGroup)>,
    maps: Option<SunMaps>,
    no_maps: SunMaps,
    compare: wgpu::Sampler,
    chunk_layout: wgpu::BindGroupLayout,
    tiled: wgpu::Sampler,
    /// The terrain's layers' (T579): as `tiled`, from `terrainMipLevel`.
    terrain: wgpu::Sampler,
    clamped: wgpu::Sampler,
}

/// The world's GPU state: the pipeline, the tiles read so far and the
/// depth buffer.
#[derive(Default)]
pub(crate) struct World {
    setup: Option<Setup>,
    /// The samples the world's pipelines and targets have (its MSAA,
    /// decision T578; 0 before the first frame).
    pub(crate) samples: u32,
    /// Its samplers' filtering and the terrain's textures' detail
    /// (`textureFilteringMode`, `terrainMipLevel`; decision T579).
    textures: (u32, u32),
    /// The multisampled color target the world draws into with MSAA, the
    /// depth buffer's size.
    msaa_color: Option<wgpu::TextureView>,
    /// Whether each prop model moves as it stands (T558).
    moves: HashMap<u32, bool>,
    /// The props posed one by one in the last frame (T558).
    pub(crate) moving: usize,
    /// The particles drawn in the last frame (T559).
    pub(crate) particles: usize,
    /// The ground's clutter placed in the last frame (T560).
    pub(crate) clutter: usize,
    /// The units' blob shadows drawn in the last frame (T563).
    pub(crate) blobs: usize,
    /// The sun's shadow maps drawn in the last frame (T564).
    pub(crate) sun_maps: usize,
    /// Every prop as it stands, none posed one by one (the frame time
    /// measurement's, `bench.rs`).
    pub(crate) still_props: bool,
    liquid_setup: Option<LiquidSetup>,
    /// What draws the weather (T565), and how many fell in the last frame.
    weather_setup: Option<WeatherSetup>,
    pub(crate) weather_drops: u32,
    tiles: HashMap<(u32, i32, i32), Option<GpuTile>>,
    depth: Option<(wgpu::TextureView, u32, u32)>,
    building_setup: Option<BuildingSetup>,
    buildings: HashMap<u32, Option<GpuBuilding>>,
    doodads: HashMap<u32, Option<GpuDoodad>>,
    /// The distant tiles' coarse meshes (decision T525).
    lods: HashMap<(u32, i32, i32), Option<GpuLod>>,
    /// One placement that leaves a mesh where it is (the distant tiles').
    identity: Option<wgpu::Buffer>,
    /// The placements' matrices of a draw, and the buffer's size.
    instances: Option<(wgpu::Buffer, u64)>,
    /// The props' placements each of the sun's maps draws, and the
    /// buffer's size (T581).
    sun_instances: Option<(wgpu::Buffer, u64)>,
    /// The props' instances drawn into the sun's maps in the last frame.
    pub(crate) sun_props: usize,
    /// What stretches the world drawn at another scale to the screen, and
    /// the texture it is drawn into (decision T539).
    stretch: Option<Stretch>,
    scaled: Option<Scaled>,
}

struct Stretch {
    pipeline: wgpu::RenderPipeline,
    layout: wgpu::BindGroupLayout,
    sampler: wgpu::Sampler,
    /// The image-based smoothing's settings (T578): a texel's size, the
    /// kind (`ffxAntiAliasingMode`).
    how: wgpu::Buffer,
}

/// A pipeline's multisampling: the world's samples (its MSAA, decision
/// T578).
pub(crate) fn msaa(count: u32) -> wgpu::MultisampleState {
    wgpu::MultisampleState {
        count: count.max(1),
        ..Default::default()
    }
}

/// A repeating sampler as the game's texture filtering says (decision
/// T579, `textureFilteringMode`): 0 bilinear, 1 trilinear, 2 to 5
/// anisotropic 2x to 16x; from mip `min_lod` (`terrainMipLevel`).
fn world_sampler(d: &wgpu::Device, filtering: u32, min_lod: f32) -> wgpu::Sampler {
    d.create_sampler(&wgpu::SamplerDescriptor {
        label: Some("world"),
        address_mode_u: wgpu::AddressMode::Repeat,
        address_mode_v: wgpu::AddressMode::Repeat,
        mag_filter: wgpu::FilterMode::Linear,
        min_filter: wgpu::FilterMode::Linear,
        mipmap_filter: if filtering == 0 {
            wgpu::MipmapFilterMode::Nearest
        } else {
            wgpu::MipmapFilterMode::Linear
        },
        lod_min_clamp: min_lod,
        anisotropy_clamp: if filtering >= 2 {
            1 << (filtering - 1).min(4)
        } else {
            1
        },
        ..Default::default()
    })
}

/// The samples the world draws with for multisampling's (decision T578):
/// 4 for any (the count every GPU has for every format WebGPU renders to),
/// else 1.
fn samples_for(requested: u32) -> u32 {
    if requested >= 2 { 4 } else { 1 }
}

/// The world drawn at `RenderScale`: its texture's view, what binds it to
/// the stretch and its size.
struct Scaled {
    view: wgpu::TextureView,
    group: wgpu::BindGroup,
    size: (u32, u32),
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

fn sampler_entry(binding: u32) -> wgpu::BindGroupLayoutEntry {
    wgpu::BindGroupLayoutEntry {
        binding,
        visibility: wgpu::ShaderStages::FRAGMENT,
        ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
        count: None,
    }
}

/// The world's globals bound with the blob shadow's texture (T563) and
/// the sun's shadow maps (T564), and their samplers.
fn globals_group(
    d: &wgpu::Device,
    layout: &wgpu::BindGroupLayout,
    globals: &wgpu::Buffer,
    [blob, maps]: [&wgpu::TextureView; 2],
    [sampler, compare]: [&wgpu::Sampler; 2],
) -> wgpu::BindGroup {
    d.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("world globals"),
        layout,
        entries: &[
            wgpu::BindGroupEntry {
                binding: 0,
                resource: globals.as_entire_binding(),
            },
            wgpu::BindGroupEntry {
                binding: 1,
                resource: wgpu::BindingResource::TextureView(blob),
            },
            wgpu::BindGroupEntry {
                binding: 2,
                resource: wgpu::BindingResource::Sampler(sampler),
            },
            wgpu::BindGroupEntry {
                binding: 3,
                resource: wgpu::BindingResource::TextureView(maps),
            },
            wgpu::BindGroupEntry {
                binding: 4,
                resource: wgpu::BindingResource::Sampler(compare),
            },
        ],
    })
}

fn sub(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}

fn cross(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
}

/// A tile's grids: each chunk's 145 points (north, west, up) and its
/// triangles, with each chunk's range of indices.
fn mesh(tile: &Tile, column: i32, row: i32) -> (Vec<TVertex>, Vec<u32>, Vec<(u32, u32)>) {
    let (north, west) = adt::corner(column, row);
    let mut verts = Vec::with_capacity(tile.chunks.len() * 145);
    let mut indices = Vec::with_capacity(tile.chunks.len() * 768);
    let mut ranges = Vec::with_capacity(tile.chunks.len());
    for c in &tile.chunks {
        let base = verts.len() as u32;
        let n0 = north - f64::from(c.y) * CHUNK;
        let w0 = west - f64::from(c.x) * CHUNK;
        for j in 0..17 {
            let (row, middle) = (j / 2, j % 2 == 1);
            let cols = if middle { 8 } else { 9 };
            let off = if middle { 0.5 } else { 0.0 };
            for col in 0..cols {
                let (r, k) = (row as f64 + off, col as f64 + off);
                verts.push(TVertex {
                    pos: [
                        (n0 - r * UNIT) as f32,
                        (w0 - k * UNIT) as f32,
                        c.height(row, col, middle),
                    ],
                    normal: [0.0; 3],
                    uv: [(k / 8.0) as f32, (r / 8.0) as f32],
                });
            }
        }
        let start = indices.len() as u32;
        for row in 0..8 {
            for col in 0..8 {
                if c.hole(row, col) {
                    continue;
                }
                let nw = base + (row * 17 + col) as u32;
                let (ne, sw) = (nw + 1, nw + 17);
                let (se, m) = (sw + 1, nw + 9);
                indices.extend([m, nw, ne, m, ne, se, m, se, sw, m, sw, nw]);
            }
        }
        // (each point's normal: its triangles', facing up)
        let mut normals = vec![[0f32; 3]; 145];
        for t in indices[start as usize..].chunks(3) {
            let p = |i: u32| verts[i as usize].pos;
            let mut n = cross(sub(p(t[1]), p(t[0])), sub(p(t[2]), p(t[0])));
            if n[2] < 0.0 {
                n = [-n[0], -n[1], -n[2]];
            }
            for &i in t {
                let a = &mut normals[(i - base) as usize];
                a[0] += n[0];
                a[1] += n[1];
                a[2] += n[2];
            }
        }
        for (v, n) in verts[base as usize..].iter_mut().zip(normals) {
            let l = (n[0] * n[0] + n[1] * n[1] + n[2] * n[2]).sqrt();
            v.normal = if l > 0.0 {
                [n[0] / l, n[1] / l, n[2] / l]
            } else {
                [0.0, 0.0, 1.0]
            };
        }
        ranges.push((start, indices.len() as u32 - start));
    }
    (verts, indices, ranges)
}

/// A chunk's blend texture: the second to fourth layers' blends in red,
/// green and blue, its baked shadow in alpha (64 × 64, from the
/// north-west; decision T563).
fn blend_of(c: &adt::Chunk) -> Rgba {
    let mut pixels = vec![0u8; 64 * 64 * 4];
    for (k, layer) in c.layers.iter().enumerate().skip(1).take(3) {
        if let Some(a) = &layer.alpha {
            for (i, v) in a.iter().enumerate().take(4096) {
                pixels[i * 4 + k - 1] = *v;
            }
        }
    }
    if let Some(rows) = &c.shadow {
        for (row, bits) in rows.iter().enumerate().take(64) {
            for col in 0..64 {
                if bits >> col & 1 != 0 {
                    pixels[(row * 64 + col) * 4 + 3] = 255;
                }
            }
        }
    }
    Rgba {
        width: 64,
        height: 64,
        pixels,
    }
}

impl Renderer {
    fn world_setup(&mut self) {
        if self.world.setup.is_some() {
            return;
        }
        // (the world's multisampling: T578)
        let samples = self.world.samples;
        let d = &self.device;
        let globals_layout = d.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("world globals"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
                // (the blob shadow's texture: T563)
                texture_entry(1),
                sampler_entry(2),
                // (the sun's shadow maps: T564)
                wgpu::BindGroupLayoutEntry {
                    binding: 3,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Depth,
                        view_dimension: wgpu::TextureViewDimension::D2Array,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 4,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Comparison),
                    count: None,
                },
            ],
        });
        let chunk_layout = d.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("world chunk"),
            entries: &[
                texture_entry(0),
                texture_entry(1),
                texture_entry(2),
                texture_entry(3),
                texture_entry(4),
                sampler_entry(5),
                sampler_entry(6),
            ],
        });
        let layout = d.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("world"),
            bind_group_layouts: &[Some(&globals_layout), Some(&chunk_layout)],
            immediate_size: 0,
        });
        let shader = d.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("world"),
            source: wgpu::ShaderSource::Wgsl(include_str!("world.wgsl").into()),
        });
        let pipeline = d.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("world"),
            layout: Some(&layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vs"),
                compilation_options: Default::default(),
                buffers: &[Some(wgpu::VertexBufferLayout {
                    array_stride: std::mem::size_of::<TVertex>() as u64,
                    step_mode: wgpu::VertexStepMode::Vertex,
                    attributes: &wgpu::vertex_attr_array![0 => Float32x3, 1 => Float32x3, 2 => Float32x2],
                })],
            },
            primitive: wgpu::PrimitiveState::default(),
            depth_stencil: Some(wgpu::DepthStencilState {
                format: wgpu::TextureFormat::Depth32Float,
                depth_write_enabled: Some(true),
                depth_compare: Some(wgpu::CompareFunction::Less),
                stencil: wgpu::StencilState::default(),
                bias: wgpu::DepthBiasState::default(),
            }),
            multisample: msaa(samples),
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some("fs"),
                compilation_options: Default::default(),
                targets: &[Some(wgpu::ColorTargetState {
                    format: crate::FORMAT,
                    blend: None,
                    write_mask: wgpu::ColorWrites::ALL,
                })],
            }),
            multiview_mask: None,
            cache: None,
        });
        let globals = d.create_buffer(&wgpu::BufferDescriptor {
            label: Some("world globals"),
            size: std::mem::size_of::<Globals>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let sampler = |address| {
            d.create_sampler(&wgpu::SamplerDescriptor {
                label: Some("world"),
                address_mode_u: address,
                address_mode_v: address,
                mag_filter: wgpu::FilterMode::Linear,
                min_filter: wgpu::FilterMode::Linear,
                mipmap_filter: wgpu::MipmapFilterMode::Linear,
                ..Default::default()
            })
        };
        // The sky: one triangle over the screen, behind everything.
        let sky_layout = d.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("world sky"),
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Uniform,
                    has_dynamic_offset: false,
                    min_binding_size: None,
                },
                count: None,
            }],
        });
        let sky_shader = d.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("world sky"),
            source: wgpu::ShaderSource::Wgsl(include_str!("sky.wgsl").into()),
        });
        let sky_pipeline = d.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("world sky"),
            layout: Some(&d.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                label: Some("world sky"),
                bind_group_layouts: &[Some(&sky_layout)],
                immediate_size: 0,
            })),
            vertex: wgpu::VertexState {
                module: &sky_shader,
                entry_point: Some("vs"),
                compilation_options: Default::default(),
                buffers: &[],
            },
            primitive: wgpu::PrimitiveState::default(),
            depth_stencil: Some(wgpu::DepthStencilState {
                format: wgpu::TextureFormat::Depth32Float,
                depth_write_enabled: Some(false),
                depth_compare: Some(wgpu::CompareFunction::Always),
                stencil: wgpu::StencilState::default(),
                bias: wgpu::DepthBiasState::default(),
            }),
            multisample: msaa(samples),
            fragment: Some(wgpu::FragmentState {
                module: &sky_shader,
                entry_point: Some("fs"),
                compilation_options: Default::default(),
                targets: &[Some(wgpu::ColorTargetState {
                    format: crate::FORMAT,
                    blend: None,
                    write_mask: wgpu::ColorWrites::ALL,
                })],
            }),
            multiview_mask: None,
            cache: None,
        });
        // The liquids: the world's globals, a frame, how it is drawn.
        let frame_layout = d.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("world liquid frame"),
            entries: &[texture_entry(0), sampler_entry(1)],
        });
        let how_layout = d.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("world liquid how"),
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Uniform,
                    has_dynamic_offset: false,
                    min_binding_size: None,
                },
                count: None,
            }],
        });
        let liquid_shader = d.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("world liquid"),
            source: wgpu::ShaderSource::Wgsl(include_str!("liquid.wgsl").into()),
        });
        let liquid_pipeline = d.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("world liquid"),
            layout: Some(&d.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                label: Some("world liquid"),
                bind_group_layouts: &[
                    Some(&globals_layout),
                    Some(&frame_layout),
                    Some(&how_layout),
                ],
                immediate_size: 0,
            })),
            vertex: wgpu::VertexState {
                module: &liquid_shader,
                entry_point: Some("vs"),
                compilation_options: Default::default(),
                buffers: &[Some(wgpu::VertexBufferLayout {
                    array_stride: std::mem::size_of::<LVertex>() as u64,
                    step_mode: wgpu::VertexStepMode::Vertex,
                    attributes: &wgpu::vertex_attr_array![0 => Float32x3, 1 => Float32x2, 2 => Float32],
                })],
            },
            primitive: wgpu::PrimitiveState::default(),
            depth_stencil: Some(wgpu::DepthStencilState {
                format: wgpu::TextureFormat::Depth32Float,
                depth_write_enabled: Some(false),
                depth_compare: Some(wgpu::CompareFunction::Less),
                stencil: wgpu::StencilState::default(),
                bias: wgpu::DepthBiasState::default(),
            }),
            multisample: msaa(samples),
            fragment: Some(wgpu::FragmentState {
                module: &liquid_shader,
                entry_point: Some("fs"),
                compilation_options: Default::default(),
                targets: &[Some(wgpu::ColorTargetState {
                    format: crate::FORMAT,
                    blend: Some(wgpu::BlendState::ALPHA_BLENDING),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
            }),
            multiview_mask: None,
            cache: None,
        });
        self.world.liquid_setup = Some(LiquidSetup {
            pipeline: liquid_pipeline,
            frame_layout,
            how_layout,
            frames: HashMap::new(),
        });
        let sky = d.create_buffer(&wgpu::BufferDescriptor {
            label: Some("world sky"),
            size: std::mem::size_of::<SkyGlobals>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let sky_group = d.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("world sky"),
            layout: &sky_layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: sky.as_entire_binding(),
            }],
        });
        let clamped = sampler(wgpu::AddressMode::ClampToEdge);
        let compare = d.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("world sun maps"),
            address_mode_u: wgpu::AddressMode::ClampToEdge,
            address_mode_v: wgpu::AddressMode::ClampToEdge,
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            compare: Some(wgpu::CompareFunction::LessEqual),
            ..Default::default()
        });
        let no_maps = sun_maps(d, 1, 1);
        let white = &self.textures[self.white].view;
        let main_group = globals_group(
            d,
            &globals_layout,
            &globals,
            [white, &no_maps.array],
            [&clamped, &compare],
        );
        // The sun's view of the ground, its depth alone (T564).
        let sun = (0..4)
            .map(|_| {
                let b = d.create_buffer(&wgpu::BufferDescriptor {
                    label: Some("world sun globals"),
                    size: std::mem::size_of::<Globals>() as u64,
                    usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
                    mapped_at_creation: false,
                });
                let g = globals_group(
                    d,
                    &globals_layout,
                    &b,
                    [white, &no_maps.array],
                    [&clamped, &compare],
                );
                (b, g)
            })
            .collect();
        let sun_pipeline = d.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("world sun"),
            layout: Some(&d.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                label: Some("world sun"),
                bind_group_layouts: &[Some(&globals_layout)],
                immediate_size: 0,
            })),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vs"),
                compilation_options: Default::default(),
                buffers: &[Some(wgpu::VertexBufferLayout {
                    array_stride: std::mem::size_of::<TVertex>() as u64,
                    step_mode: wgpu::VertexStepMode::Vertex,
                    attributes: &wgpu::vertex_attr_array![0 => Float32x3, 1 => Float32x3, 2 => Float32x2],
                })],
            },
            primitive: wgpu::PrimitiveState::default(),
            depth_stencil: Some(wgpu::DepthStencilState {
                format: wgpu::TextureFormat::Depth32Float,
                depth_write_enabled: Some(true),
                depth_compare: Some(wgpu::CompareFunction::LessEqual),
                stencil: wgpu::StencilState::default(),
                bias: crate::model::SUN_BIAS,
            }),
            multisample: wgpu::MultisampleState::default(),
            fragment: None,
            multiview_mask: None,
            cache: None,
        });
        self.world.setup = Some(Setup {
            pipeline,
            sky_pipeline,
            sky,
            sky_group,
            globals_layout,
            globals,
            globals_group: main_group,
            blob: self.white,
            sun_pipeline,
            sun,
            maps: None,
            no_maps,
            compare,
            chunk_layout,
            tiled: world_sampler(d, self.world.textures.0, 0.0),
            terrain: world_sampler(d, self.world.textures.0, self.world.textures.1 as f32),
            clamped,
        });
    }

    /// The game's blob shadow (decision T563), read on first use, and the
    /// sun's shadow maps the settings want (`maps`: their size and how
    /// many; T564), made as they change: bound with the globals. Whether
    /// the build has the blob.
    fn world_bindings(&mut self, assets: &mut dyn Assets, maps: Option<(u32, u32)>) -> bool {
        let t = match self.model_texture(BLOB_FILE, assets) {
            t if t == self.missing => self.white,
            t => t,
        };
        let setup = self.world.setup.as_mut().expect("the world's setup");
        let mut changed = setup.blob != t;
        if setup.maps.as_ref().map(|m| (m.size, m.cascades)) != maps {
            setup.maps = maps.map(|(size, n)| sun_maps(&self.device, size, n));
            changed = true;
        }
        if changed {
            let array = setup.maps.as_ref().unwrap_or(&setup.no_maps);
            setup.globals_group = globals_group(
                &self.device,
                &setup.globals_layout,
                &setup.globals,
                [&self.textures[t].view, &array.array],
                [&setup.clamped, &setup.compare],
            );
            setup.blob = t;
        }
        t != self.white
    }

    /// A tile on the GPU, read and built on first use.
    fn world_tile(&mut self, map: u32, x: i32, y: i32, assets: &mut dyn Assets) {
        if self.world.tiles.contains_key(&(map, x, y)) {
            return;
        }
        let gpu = assets.terrain(map, x, y).map(|tile| {
            let (verts, indices, ranges) = mesh(&tile, x, y);
            let vertices = self
                .device
                .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                    label: Some("terrain"),
                    contents: bytemuck::cast_slice(&verts),
                    usage: wgpu::BufferUsages::VERTEX,
                });
            let index_buffer = self
                .device
                .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                    label: Some("terrain"),
                    contents: bytemuck::cast_slice(&indices),
                    usage: wgpu::BufferUsages::INDEX,
                });
            let (north, west) = adt::corner(x, y);
            let mut chunks = Vec::with_capacity(tile.chunks.len());
            for (c, (start, count)) in tile.chunks.iter().zip(ranges) {
                let mut layers = [self.white; 4];
                for (slot, l) in layers.iter_mut().zip(&c.layers) {
                    if let Some(&file) = tile.textures.get(l.texture as usize) {
                        *slot = self.model_texture(file, assets);
                    }
                }
                let blend = self.upload(&blend_of(c));
                let setup = self.world.setup.as_ref().expect("the world's setup");
                let view = |i: usize| wgpu::BindingResource::TextureView(&self.textures[i].view);
                let group = self.device.create_bind_group(&wgpu::BindGroupDescriptor {
                    label: Some("terrain chunk"),
                    layout: &setup.chunk_layout,
                    entries: &[
                        wgpu::BindGroupEntry {
                            binding: 0,
                            resource: view(layers[0]),
                        },
                        wgpu::BindGroupEntry {
                            binding: 1,
                            resource: view(layers[1]),
                        },
                        wgpu::BindGroupEntry {
                            binding: 2,
                            resource: view(layers[2]),
                        },
                        wgpu::BindGroupEntry {
                            binding: 3,
                            resource: view(layers[3]),
                        },
                        wgpu::BindGroupEntry {
                            binding: 4,
                            resource: view(blend),
                        },
                        wgpu::BindGroupEntry {
                            binding: 5,
                            resource: wgpu::BindingResource::Sampler(&setup.terrain),
                        },
                        wgpu::BindGroupEntry {
                            binding: 6,
                            resource: wgpu::BindingResource::Sampler(&setup.clamped),
                        },
                    ],
                });
                let middle = c.height(4, 4, false);
                chunks.push(GpuChunk {
                    start,
                    count,
                    group,
                    center: [
                        north - (f64::from(c.y) + 0.5) * CHUNK,
                        west - (f64::from(c.x) + 0.5) * CHUNK,
                        f64::from(middle),
                    ],
                });
            }
            let liquid = self.gpu_liquid(&tile, x, y, assets);
            GpuTile {
                vertices,
                indices: index_buffer,
                chunks,
                liquid,
            }
        });
        self.world.tiles.insert((map, x, y), gpu);
    }

    /// A tile's liquids on the GPU (decision T557), their frames' textures
    /// read and bound; none for a tile without.
    fn gpu_liquid(
        &mut self,
        tile: &Tile,
        x: i32,
        y: i32,
        assets: &mut dyn Assets,
    ) -> Option<GpuLiquid> {
        if tile.liquids.is_empty() {
            return None;
        }
        let kinds = assets.liquids()?;
        let (verts, indices, parts) = liquid_mesh(tile, x, y, &kinds);
        if indices.is_empty() {
            return None;
        }
        let mut draws = Vec::with_capacity(parts.len());
        for (files, glowing, ocean, colors, start, count) in parts {
            let frames: Vec<usize> = files
                .iter()
                .map(|&f| self.model_texture(f, assets))
                .collect();
            let lsetup = self.world.liquid_setup.as_ref()?;
            let flag = |b: bool| if b { 1.0 } else { 0.0 };
            let (near, far) = colors.unwrap_or_default();
            let v: [f32; 12] = [
                flag(glowing),
                flag(ocean),
                flag(colors.is_some()),
                0.0,
                near[0],
                near[1],
                near[2],
                1.0,
                far[0],
                far[1],
                far[2],
                1.0,
            ];
            let buffer = self
                .device
                .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                    label: Some("world liquid how"),
                    contents: bytemuck::cast_slice(&v),
                    usage: wgpu::BufferUsages::UNIFORM,
                });
            let how = self.device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("world liquid how"),
                layout: &lsetup.how_layout,
                entries: &[wgpu::BindGroupEntry {
                    binding: 0,
                    resource: buffer.as_entire_binding(),
                }],
            });
            draws.push(LiquidDraw {
                frames,
                how,
                start,
                count,
            });
        }
        let setup = self.world.setup.as_ref()?;
        let lsetup = self.world.liquid_setup.as_mut()?;
        for &f in draws.iter().flat_map(|d| d.frames.iter()) {
            if lsetup.frames.contains_key(&f) {
                continue;
            }
            let group = self.device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("world liquid frame"),
                layout: &lsetup.frame_layout,
                entries: &[
                    wgpu::BindGroupEntry {
                        binding: 0,
                        resource: wgpu::BindingResource::TextureView(&self.textures[f].view),
                    },
                    wgpu::BindGroupEntry {
                        binding: 1,
                        resource: wgpu::BindingResource::Sampler(&setup.tiled),
                    },
                ],
            });
            lsetup.frames.insert(f, group);
        }
        let vertices = self
            .device
            .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("liquid"),
                contents: bytemuck::cast_slice(&verts),
                usage: wgpu::BufferUsages::VERTEX,
            });
        let indices = self
            .device
            .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("liquid"),
                contents: bytemuck::cast_slice(&indices),
                usage: wgpu::BufferUsages::INDEX,
            });
        Some(GpuLiquid {
            vertices,
            indices,
            draws,
        })
    }

    /// Draws the world into `view`: at `RenderScale` of its pixels each
    /// way, stretched to it (decision T539).
    pub(crate) fn draw_world(
        &mut self,
        encoder: &mut wgpu::CommandEncoder,
        view: &wgpu::TextureView,
        width: u32,
        height: u32,
        w: &WorldView,
        assets: &mut dyn Assets,
    ) {
        // (multisampling, T578: a new sample count makes the world's
        // pipelines and targets anew)
        // (and the texture settings, T579: samplers anew)
        let samples = samples_for(w.msaa);
        let textures = (w.filtering, w.terrain_mip);
        if self.world.samples != samples || self.world.textures != textures {
            let still = self.world.still_props;
            self.world = World::default();
            self.world.still_props = still;
            self.world.samples = samples;
            self.world.textures = textures;
        }
        let scale = w.render_scale;
        let size = (
            ((f64::from(width) * scale).round() as u32).max(1),
            ((f64::from(height) * scale).round() as u32).max(1),
        );
        // (smoothed after (`ffxAntiAliasingMode`, T578): through the
        // stretch too)
        if size == (width, height) && w.post_aa == 0 {
            self.draw_world_at(encoder, view, width, height, height, w, assets);
            return;
        }
        let scaled = self.scaled_target(size);
        self.draw_world_at(encoder, &scaled.0, size.0, size.1, height, w, assets);
        let stretch = self.world.stretch.as_ref().expect("the stretch");
        let how: [f32; 4] = [
            1.0 / size.0 as f32,
            1.0 / size.1 as f32,
            w.post_aa as f32,
            0.0,
        ];
        self.queue
            .write_buffer(&stretch.how, 0, bytemuck::cast_slice(&how));
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("world stretched"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view,
                depth_slice: None,
                resolve_target: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Clear(wgpu::Color::BLACK),
                    store: wgpu::StoreOp::Store,
                },
            })],
            depth_stencil_attachment: None,
            timestamp_writes: None,
            occlusion_query_set: None,
            multiview_mask: None,
        });
        pass.set_pipeline(&stretch.pipeline);
        pass.set_bind_group(0, &scaled.1, &[]);
        pass.draw(0..3, 0..1);
    }

    /// The texture the world is drawn into at `size`, made again when the
    /// size changes, and its binding to the stretch.
    fn scaled_target(&mut self, size: (u32, u32)) -> (wgpu::TextureView, wgpu::BindGroup) {
        let d = &self.device;
        let stretch = self.world.stretch.get_or_insert_with(|| {
            let layout = d.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                label: Some("world stretch"),
                entries: &[
                    texture_entry(0),
                    sampler_entry(1),
                    wgpu::BindGroupLayoutEntry {
                        binding: 2,
                        visibility: wgpu::ShaderStages::FRAGMENT,
                        ty: wgpu::BindingType::Buffer {
                            ty: wgpu::BufferBindingType::Uniform,
                            has_dynamic_offset: false,
                            min_binding_size: None,
                        },
                        count: None,
                    },
                ],
            });
            let pipeline_layout = d.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                label: Some("world stretch"),
                bind_group_layouts: &[Some(&layout)],
                immediate_size: 0,
            });
            let shader = d.create_shader_module(wgpu::ShaderModuleDescriptor {
                label: Some("world stretch"),
                source: wgpu::ShaderSource::Wgsl(include_str!("stretch.wgsl").into()),
            });
            let pipeline = d.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some("world stretch"),
                layout: Some(&pipeline_layout),
                vertex: wgpu::VertexState {
                    module: &shader,
                    entry_point: Some("vs"),
                    compilation_options: Default::default(),
                    buffers: &[],
                },
                primitive: wgpu::PrimitiveState::default(),
                depth_stencil: None,
                multisample: wgpu::MultisampleState::default(),
                fragment: Some(wgpu::FragmentState {
                    module: &shader,
                    entry_point: Some("fs"),
                    compilation_options: Default::default(),
                    targets: &[Some(wgpu::ColorTargetState {
                        format: crate::FORMAT,
                        blend: None,
                        write_mask: wgpu::ColorWrites::ALL,
                    })],
                }),
                multiview_mask: None,
                cache: None,
            });
            let sampler = d.create_sampler(&wgpu::SamplerDescriptor {
                label: Some("world stretch"),
                mag_filter: wgpu::FilterMode::Linear,
                min_filter: wgpu::FilterMode::Linear,
                ..Default::default()
            });
            let how = d.create_buffer(&wgpu::BufferDescriptor {
                label: Some("world stretch"),
                size: 16,
                usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            });
            Stretch {
                pipeline,
                layout,
                sampler,
                how,
            }
        });
        if self.world.scaled.as_ref().is_none_or(|s| s.size != size) {
            let texture = d.create_texture(&wgpu::TextureDescriptor {
                label: Some("world scaled"),
                size: wgpu::Extent3d {
                    width: size.0,
                    height: size.1,
                    depth_or_array_layers: 1,
                },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: crate::FORMAT,
                usage: wgpu::TextureUsages::RENDER_ATTACHMENT
                    | wgpu::TextureUsages::TEXTURE_BINDING,
                view_formats: &[],
            });
            let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
            let group = d.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("world scaled"),
                layout: &stretch.layout,
                entries: &[
                    wgpu::BindGroupEntry {
                        binding: 0,
                        resource: wgpu::BindingResource::TextureView(&view),
                    },
                    wgpu::BindGroupEntry {
                        binding: 1,
                        resource: wgpu::BindingResource::Sampler(&stretch.sampler),
                    },
                    wgpu::BindGroupEntry {
                        binding: 2,
                        resource: stretch.how.as_entire_binding(),
                    },
                ],
            });
            self.world.scaled = Some(Scaled { view, group, size });
        }
        let s = self.world.scaled.as_ref().expect("the scaled target");
        (s.view.clone(), s.group.clone())
    }

    /// Draws the world into `view` (cleared to the sky first), with a
    /// depth buffer of its size: the tiles within the view distance, the
    /// chunks in front of the camera. A doodad's size in pixels is
    /// measured on a screen `screen_height` tall (the world's scale does
    /// not change which are drawn).
    #[allow(clippy::too_many_arguments)]
    fn draw_world_at(
        &mut self,
        encoder: &mut wgpu::CommandEncoder,
        view: &wgpu::TextureView,
        width: u32,
        height: u32,
        screen_height: u32,
        w: &WorldView,
        assets: &mut dyn Assets,
    ) {
        // (the samples `draw_world` chose: T578)
        let samples = self.world.samples.max(1);
        self.models.world_samples = samples;
        self.world_setup();
        if self
            .world
            .depth
            .as_ref()
            .is_none_or(|(_, dw, dh)| (*dw, *dh) != (width, height))
        {
            let depth = self.device.create_texture(&wgpu::TextureDescriptor {
                label: Some("world depth"),
                size: wgpu::Extent3d {
                    width,
                    height,
                    depth_or_array_layers: 1,
                },
                mip_level_count: 1,
                sample_count: samples,
                dimension: wgpu::TextureDimension::D2,
                format: wgpu::TextureFormat::Depth32Float,
                usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
                view_formats: &[],
            });
            let v = depth.create_view(&wgpu::TextureViewDescriptor::default());
            self.world.depth = Some((v, width, height));
            self.world.msaa_color = (samples > 1).then(|| {
                self.device
                    .create_texture(&wgpu::TextureDescriptor {
                        label: Some("world msaa"),
                        size: wgpu::Extent3d {
                            width,
                            height,
                            depth_or_array_layers: 1,
                        },
                        mip_level_count: 1,
                        sample_count: samples,
                        dimension: wgpu::TextureDimension::D2,
                        format: crate::FORMAT,
                        usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
                        view_formats: &[],
                    })
                    .create_view(&wgpu::TextureViewDescriptor::default())
            });
        }
        // The tiles within the view distance of the eye: in full within
        // the detail distance, beyond it their coarse meshes (T525).
        let (cx, cy) = adt::tile_of(w.eye[0], w.eye[1]);
        // (the coarse terrain to the horizon, T580)
        let horizon = w.horizon.max(w.far);
        let reach = (horizon / TILE).ceil() as i32;
        let (mut detailed, mut distant, mut near) = (Vec::new(), Vec::new(), Vec::new());
        for y in cy - reach..=cy + reach {
            for x in cx - reach..=cx + reach {
                let (n, wst) = adt::corner(x, y);
                // (the tile's nearest point to the eye)
                let dn = (w.eye[0] - w.eye[0].clamp(n - TILE, n)).abs();
                let dw = (w.eye[1] - w.eye[1].clamp(wst - TILE, wst)).abs();
                let d = dn.hypot(dw);
                if d > horizon {
                    continue;
                }
                if d <= w.detail {
                    detailed.push((x, y));
                } else {
                    distant.push((x, y));
                }
                if d <= w.objects {
                    near.push((x, y));
                }
            }
        }
        self.building_setup();
        // (what is out of reach goes, once a cache holds twice what is used)
        let keep =
            |set: &[(i32, i32)], k: &(u32, i32, i32)| k.0 == w.map && set.contains(&(k.1, k.2));
        if self.world.tiles.len() > 2 * detailed.len() + 16 {
            self.world.tiles.retain(|k, _| keep(&detailed, k));
        }
        if self.world.lods.len() > 2 * distant.len() + 64 {
            self.world.lods.retain(|k, _| keep(&distant, k));
        }
        for &(x, y) in &detailed {
            self.world_tile(w.map, x, y, assets);
        }
        for &(x, y) in &distant {
            self.world_lod(w.map, x, y, assets);
        }
        let aspect = width as f32 / height.max(1) as f32;
        let fov_y = 2.0 * ((w.fov as f32 / 2.0).tan() / aspect).atan();
        // (how many pixels a yard is, a yard away)
        let focal = f64::from(screen_height) / 2.0 / f64::from((fov_y / 2.0).tan());
        // The buildings the tiles place within the objects' distance, each
        // once (one standing on several tiles is placed by each).
        let mut placed: Vec<(u32, [f32; 16])> = Vec::new();
        let mut homes: Vec<wowlua_casc::obj::Building> = Vec::new();
        let mut seen = std::collections::HashSet::new();
        for &(x, y) in &near {
            let Some(objects) = assets.objects(w.map, x, y) else {
                continue;
            };
            for b in &objects.buildings {
                let dn = w.eye[0] - w.eye[0].clamp(b.bounds[0][0], b.bounds[1][0]);
                let dw = w.eye[1] - w.eye[1].clamp(b.bounds[0][1], b.bounds[1][1]);
                if dn.hypot(dw) > w.objects || !seen.insert(b.unique) {
                    continue;
                }
                self.world_building(b.file, assets);
                if matches!(self.world.buildings.get(&b.file), Some(Some(_))) {
                    placed.push((b.file, matrix_of(b)));
                    homes.push(b.clone());
                }
            }
        }
        // The doodads within the view distance: the tiles' (each once) and
        // the buildings' own (their first set and the one placed), by model.
        let mut by_model: HashMap<u32, Vec<[f32; 16]>> = HashMap::new();
        let mut seen = std::collections::HashSet::new();
        // (within the objects' distance, and looking big enough: a model's
        // size on the screen at least the cull size, T525)
        let shown =
            |r: &mut Renderer, assets: &mut dyn Assets, file: u32, p: [f64; 3], scale: f64| {
                let d = ((p[0] - w.eye[0]).powi(2)
                    + (p[1] - w.eye[1]).powi(2)
                    + (p[2] - w.eye[2]).powi(2))
                .sqrt();
                if d > w.objects {
                    return false;
                }
                let radius = r.model_radius(file, assets).map_or(1.0, f64::from);
                2.0 * radius * scale * focal / d.max(0.1) >= w.cull
            };
        for &(x, y) in &near {
            let Some(objects) = assets.objects(w.map, x, y) else {
                continue;
            };
            for d in &objects.doodads {
                if !seen.insert(d.unique) || !shown(self, assets, d.file, d.at, f64::from(d.scale))
                {
                    continue;
                }
                let m = matrix_from(wowlua_casc::obj::placed(d.at, d.rotation, d.scale));
                by_model.entry(d.file).or_default().push(m);
            }
        }
        for b in &homes {
            let Some(wmo) = assets.building(b.file) else {
                continue;
            };
            let place = wowlua_casc::obj::placed(b.at, b.rotation, b.scale);
            let mut sets = vec![0usize];
            if b.doodad_set != 0 {
                sets.push(b.doodad_set as usize);
            }
            for set in sets {
                let Some(&(first, count)) = wmo.root.doodad_sets.get(set) else {
                    continue;
                };
                let (first, count) = (first as usize, count as usize);
                for d in wmo.root.doodads.iter().skip(first).take(count) {
                    if d.file == 0 {
                        continue;
                    }
                    let local = d.local();
                    let f = |v: [f32; 3]| place(local(v));
                    let scale = f64::from(d.scale) * f64::from(b.scale);
                    if !shown(self, assets, d.file, f([0.0; 3]), scale) {
                        continue;
                    }
                    by_model.entry(d.file).or_default().push(matrix_from(f));
                }
            }
        }
        // The ground's clutter (T560): in each square of the chunks within
        // its distance, the models its layer's effect scatters, at steady
        // random places on the ground, drawn as the props that stand.
        let mut clutter: HashMap<u32, Vec<[f32; 16]>> = HashMap::new();
        if w.clutter_dist > 0.0
            && let Some(kinds) = assets.clutter()
        {
            let reach = w.clutter_dist;
            for &(x, y) in &detailed {
                let Some(tile) = assets.terrain(w.map, x, y) else {
                    continue;
                };
                let (north, west) = adt::corner(x, y);
                let far = |n: f64, wst: f64| (n - w.eye[0]).hypot(wst - w.eye[1]);
                for c in &tile.chunks {
                    let (south0, east0) = (f64::from(c.y) * CHUNK, f64::from(c.x) * CHUNK);
                    if far(north - south0 - CHUNK / 2.0, west - east0 - CHUNK / 2.0) > reach + CHUNK
                    {
                        continue;
                    }
                    for cell in 0..64usize {
                        let (row, col) = (cell / 8, cell % 8);
                        if c.no_effect & (1 << cell) != 0 || c.hole(row, col) {
                            continue;
                        }
                        let Some(effect) = c
                            .layers
                            .get(usize::from(c.effects[cell]))
                            .and_then(|l| kinds.effect(l.effect))
                        else {
                            continue;
                        };
                        let count =
                            (f64::from(effect.density) * w.clutter_density / 64.0).round() as u64;
                        let mut seed = (x as u64) << 48
                            ^ (y as u64) << 40
                            ^ u64::from(c.x) << 32
                            ^ u64::from(c.y) << 24
                            ^ (cell as u64) << 16;
                        let mut unit = || {
                            // (splitmix64: the same places each frame)
                            seed = seed.wrapping_add(0x9E37_79B9_7F4A_7C15);
                            let mut z = seed;
                            z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
                            z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
                            ((z ^ (z >> 31)) >> 11) as f64 / (1u64 << 53) as f64
                        };
                        for _ in 0..count {
                            let south = south0 + (row as f64 + unit()) * UNIT;
                            let east = east0 + (col as f64 + unit()) * UNIT;
                            let (pn, pw) = (north - south, west - east);
                            let pick = unit() as f32;
                            let (size, turn) = (unit() as f32, unit() as f32);
                            if far(pn, pw) > reach {
                                continue;
                            }
                            let (Some(h), Some(d)) =
                                (tile.height_at(south, east), effect.pick(pick))
                            else {
                                continue;
                            };
                            let scale = d.size.0 + (d.size.1 - d.size.0) * size;
                            let yaw = d.turn.0 + (d.turn.1 - d.turn.0) * turn;
                            clutter.entry(d.file).or_default().push(matrix_from(
                                wowlua_casc::obj::placed([pn, pw, h], [0.0, yaw, 0.0], scale),
                            ));
                        }
                    }
                }
            }
        }
        self.world.clutter = clutter.values().map(Vec::len).sum();
        // (the buildings and doodad models out of reach go, as the tiles)
        if self.world.buildings.len() > 2 * placed.len() + 64 {
            let used: std::collections::HashSet<u32> = placed.iter().map(|(f, _)| *f).collect();
            self.world.buildings.retain(|f, _| used.contains(f));
        }
        if self.world.doodads.len() > 2 * (by_model.len() + clutter.len()) + 256 {
            self.world
                .doodads
                .retain(|f, _| by_model.contains_key(f) || clutter.contains_key(f));
        }
        let mut doodad_ranges: Vec<(u32, u32, u32)> = Vec::new();
        let mut matrices: Vec<[f32; 16]> = placed.iter().map(|(_, m)| *m).collect();
        // The props that move, near the eye, posed one by one with the
        // units, each in its own phase (T558); the others as they stand.
        let mut moving: Vec<(u32, [f32; 16])> = Vec::new();
        for (file, mut ms) in by_model {
            if !self.world.still_props && self.prop_moves(file, assets) {
                let near = |m: &[f32; 16]| {
                    let d = [
                        f64::from(m[12]) - w.eye[0],
                        f64::from(m[13]) - w.eye[1],
                        f64::from(m[14]) - w.eye[2],
                    ];
                    (d[0] * d[0] + d[1] * d[1] + d[2] * d[2]).sqrt() <= PROP_MOTION
                };
                moving.extend(ms.iter().filter(|m| near(m)).map(|m| (file, *m)));
                ms.retain(|m| !near(m));
                if ms.is_empty() {
                    continue;
                }
            }
            self.world_doodad(file, assets);
            if !matches!(self.world.doodads.get(&file), Some(Some(_))) {
                continue;
            }
            doodad_ranges.push((file, matrices.len() as u32, ms.len() as u32));
            matrices.extend(ms);
        }
        // (the clutter as it stands, together per model: T560)
        for (file, ms) in clutter {
            self.world_doodad(file, assets);
            if !matches!(self.world.doodads.get(&file), Some(Some(_))) {
                continue;
            }
            doodad_ranges.push((file, matrices.len() as u32, ms.len() as u32));
            matrices.extend(ms);
        }
        if !matrices.is_empty() {
            let bytes: Vec<u8> = matrices
                .iter()
                .flat_map(|m| bytemuck::cast_slice::<f32, u8>(m).to_vec())
                .collect();
            let len = bytes.len() as u64;
            if self
                .world
                .instances
                .as_ref()
                .is_none_or(|(_, size)| *size < len)
            {
                let size = len.next_power_of_two().max(4096);
                let buffer = self.device.create_buffer(&wgpu::BufferDescriptor {
                    label: Some("building placements"),
                    size,
                    usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
                    mapped_at_creation: false,
                });
                self.world.instances = Some((buffer, size));
            }
            let (buffer, _) = self
                .world
                .instances
                .as_ref()
                .expect("the placements' buffer");
            self.queue.write_buffer(buffer, 0, &bytes);
        }
        let f32s = |a: [f64; 3]| [a[0] as f32, a[1] as f32, a[2] as f32];
        // The place's light at the realm's time of day (T555), else a plain
        // sky and sun.
        // (the view and its fog to the horizon: T580)
        let far = w.horizon.max(w.far) as f32;
        // (the weather's storm set blended in by its intensity, as the
        // game: T565)
        let mut light = assets.light(w.map, w.eye, w.day_time, LightKind::Clear);
        let (weather, intensity) = (w.weather.0, w.weather.1 as f32);
        if weather != 0
            && intensity > 0.0
            && let Some(l) = light.as_mut()
            && let Some(storm) = assets.light(w.map, w.eye, w.day_time, LightKind::Storm)
        {
            l.mix(&storm, intensity);
        }
        let (mut sun, direct, ambient, fog, fog_start, fog_end, sky) = match &light {
            Some(l) => {
                let d = wowlua_casc::light::light_direction(w.day_time);
                let end = l.fog_end.min(far).max(1.0);
                (
                    [-d[0] as f32, -d[1] as f32, -d[2] as f32, 0.0],
                    l.direct,
                    l.ambient,
                    l.fog,
                    l.fog_start.min(end * 0.99),
                    end,
                    l.sky,
                )
            }
            None => {
                let l = (SUN[0] * SUN[0] + SUN[1] * SUN[1] + SUN[2] * SUN[2]).sqrt();
                (
                    [SUN[0] / l, SUN[1] / l, SUN[2] / l, 0.0],
                    [1.0 - SUN[3]; 3],
                    [SUN[3]; 3],
                    SKY,
                    far * FOG_START,
                    far,
                    [SKY; 6],
                )
            }
        };
        let rgb = |c: [f32; 3]| [c[0], c[1], c[2], 1.0];
        let with = |c: [f32; 3], a: f32| [c[0], c[1], c[2], a];
        let water = match &light {
            Some(l) => (
                with(l.river_close, l.water_alpha[0]),
                with(l.river_far, l.water_alpha[1]),
                with(l.ocean_close, l.water_alpha[2]),
                with(l.ocean_far, l.water_alpha[3]),
            ),
            None => (
                [0.2, 0.35, 0.45, 0.5],
                [0.1, 0.2, 0.3, 0.85],
                [0.2, 0.35, 0.45, 0.5],
                [0.1, 0.2, 0.3, 0.85],
            ),
        };
        // The units' blob shadows (decision T563): the nearest the eye
        // whose models are read, each by how wide its body is.
        // (by the shadow settings, T564: the units' blobs where the sun's
        // maps draw none of theirs, the ground's baked shadows where they
        // draw none of the buildings' and props')
        let mode = w.shadows.mode;
        let reach = sun_reach(&w.shadows, w.far);
        let maps = (reach > 0.0).then_some((w.shadows.size, w.shadows.cascades));
        let mut blobs = [[0.0f32; 4]; BLOBS];
        self.world.blobs = 0;
        if self.world_bindings(assets, maps) && mode == 0 {
            let mut near: Vec<&wowlua_engine::scene::WorldUnit> = w.units.iter().collect();
            let d2 = |u: &&wowlua_engine::scene::WorldUnit| {
                (u.at[0] - w.eye[0]).powi(2) + (u.at[1] - w.eye[1]).powi(2)
            };
            near.sort_by(|a, b| d2(a).total_cmp(&d2(b)));
            let mut n = 0;
            for u in near {
                if n == BLOBS {
                    break;
                }
                if let Some(reach) = self.source_reach(&u.source, assets) {
                    blobs[n] = [
                        u.at[0] as f32,
                        u.at[1] as f32,
                        u.at[2] as f32,
                        blob_radius(reach),
                    ];
                    n += 1;
                }
            }
            sun[3] = n as f32;
            self.world.blobs = n;
        }
        let shadow = if mode <= 1 {
            light.as_ref().map_or(SHADOW, |l| l.shadow)
        } else {
            0.0
        };
        // The sun's shadow maps (T564): from the sun (the moon at night),
        // each about its stretch of the view.
        let toward = [f64::from(sun[0]), f64::from(sun[1]), f64::from(sun[2])];
        let sun_maps = match maps {
            Some((size, n)) if toward[2] > 0.0 => cascades(
                w,
                (w.fov / 2.0).tan(),
                (f64::from(fov_y) / 2.0).tan(),
                toward,
                reach,
                n,
                size,
            ),
            _ => Vec::new(),
        };
        self.world.sun_maps = sun_maps.len();
        // The props each of the sun's maps draws (T581): those whose ball
        // (the model's radius, by its placement's scale) reaches into its
        // box, every map's after the other in one buffer.
        let mut sun_ranges: Vec<Vec<(u32, u32, u32)>> = Vec::new();
        self.world.sun_props = 0;
        if w.shadows.mode >= 2 && !sun_maps.is_empty() {
            let mut sun_matrices: Vec<[f32; 16]> = Vec::new();
            let radii: HashMap<u32, f64> = doodad_ranges
                .iter()
                .map(|&(file, _, _)| (file, self.model_radius(file, assets).map_or(1.0, f64::from)))
                .collect();
            for c in &sun_maps {
                let mut ranges = Vec::new();
                for &(file, first, count) in &doodad_ranges {
                    let start = sun_matrices.len() as u32;
                    for m in &matrices[first as usize..(first + count) as usize] {
                        let scale = f64::from((m[0] * m[0] + m[1] * m[1] + m[2] * m[2]).sqrt());
                        let at = [m[12], m[13], m[14]].map(f64::from);
                        if c.covers(at, radii[&file] * scale) {
                            sun_matrices.push(*m);
                        }
                    }
                    let n = sun_matrices.len() as u32 - start;
                    if n > 0 {
                        ranges.push((file, start, n));
                    }
                }
                sun_ranges.push(ranges);
            }
            self.world.sun_props = sun_matrices.len();
            if !sun_matrices.is_empty() {
                let bytes: &[u8] = bytemuck::cast_slice(&sun_matrices);
                let len = bytes.len() as u64;
                if self
                    .world
                    .sun_instances
                    .as_ref()
                    .is_none_or(|(_, size)| *size < len)
                {
                    let size = len.next_power_of_two().max(4096);
                    let buffer = self.device.create_buffer(&wgpu::BufferDescriptor {
                        label: Some("sun placements"),
                        size,
                        usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
                        mapped_at_creation: false,
                    });
                    self.world.sun_instances = Some((buffer, size));
                }
                let (buffer, _) = self.world.sun_instances.as_ref().expect("the buffer");
                self.queue.write_buffer(buffer, 0, bytes);
            }
        }
        let mut shadow_mats = [[0.0f32; 16]; 4];
        let (mut shadow_ends, mut shadow_texels) = ([0.0f32; 4], [0.0f32; 4]);
        for (k, c) in sun_maps.iter().enumerate() {
            shadow_mats[k] = wowlua_casc::m2::mul(&c.proj, &c.view);
            shadow_ends[k] = c.end;
            shadow_texels[k] = c.texel;
        }
        let globals = Globals {
            view: crate::model::look_at(f32s(w.eye), f32s(w.look), f32s(w.up)),
            proj: crate::model::perspective(fov_y, aspect, NEAR, far),
            sun,
            fog: [fog[0], fog[1], fog[2], fog_start],
            eye: [w.eye[0] as f32, w.eye[1] as f32, w.eye[2] as f32, fog_end],
            direct: rgb(direct),
            // (and how much of the light a shadow takes: T563)
            ambient: with(ambient, shadow),
            river: water.0,
            river_far: water.1,
            ocean: water.2,
            ocean_far: water.3,
            blobs,
            shadow_mats,
            shadow_ends,
            shadow_texels,
            shadow: [
                sun_maps.len() as f32,
                f32::from(u8::from(w.shadows.soft)),
                f32::from(u8::from(w.shadows.blend)),
                1.0 / w.shadows.size.max(1) as f32,
            ],
            look: {
                let l = f32s(w.look);
                [l[0], l[1], l[2], 0.0]
            },
            terrain: [f32::from(u8::from(w.specular)), TERRAIN_SHINE, 0.0, 0.0],
        };
        // The weather (T565): what falls, how much, lit by the light.
        let drops = if self.eye_indoors(&placed, w.eye) {
            // (under a roof: none falls, T581)
            self.world.weather_drops = 0;
            0
        } else {
            self.world_weather(w, &globals, direct, ambient, assets)
        };
        let sky_globals = {
            let l = f32s(w.look);
            let u = f32s(w.up);
            let tan_y = (fov_y / 2.0).tan();
            SkyGlobals {
                colors: sky.map(rgb),
                forward: [l[0], l[1], l[2], 0.0],
                right: [
                    l[1] * u[2] - l[2] * u[1],
                    l[2] * u[0] - l[0] * u[2],
                    l[0] * u[1] - l[1] * u[0],
                    tan_y * aspect,
                ],
                up: [u[0], u[1], u[2], tan_y],
            }
        };
        // The units as their models, posed for this camera (T526): those
        // the camera can see (4 yards' room around each, T538).
        let (tan_x, tan_y) = ((w.fov / 2.0).tan(), (f64::from(fov_y) / 2.0).tan());
        let right = [
            w.look[1] * w.up[2] - w.look[2] * w.up[1],
            w.look[2] * w.up[0] - w.look[0] * w.up[2],
            w.look[0] * w.up[1] - w.look[1] * w.up[0],
        ];
        let seen = |at: [f64; 3]| {
            let d = [at[0] - w.eye[0], at[1] - w.eye[1], at[2] + 1.0 - w.eye[2]];
            let along = |a: [f64; 3]| d[0] * a[0] + d[1] * a[1] + d[2] * a[2];
            let depth = along(w.look);
            const ROOM: f64 = 4.0;
            depth > -ROOM
                && along(right).abs() <= depth.max(0.0) * tan_x + ROOM
                && along(w.up).abs() <= depth.max(0.0) * tan_y + ROOM
        };
        let actors: Vec<wowlua_engine::scene::Actor> = w
            .units
            .iter()
            .filter(|u| seen(u.at))
            .map(|u| wowlua_engine::scene::Actor {
                source: u.source.clone(),
                position: f32s(u.at),
                yaw: u.facing as f32,
                pitch: 0.0,
                roll: 0.0,
                fixed_axes: false,
                scale: 1.0,
                alpha: near_fade(w.eye, u.at),
                animation: u.animation,
                variation: 0,
                placement: None,
                phase: 0,
            })
            .collect();
        // (and the moving props the camera can see, T558: each in its own
        // phase, by where it stands)
        let mut actors = actors;
        self.world.moving = 0;
        for (file, m) in moving {
            if !seen([f64::from(m[12]), f64::from(m[13]), f64::from(m[14])]) {
                continue;
            }
            let phase = (m[12].to_bits().wrapping_mul(73_856_093)
                ^ m[13].to_bits().wrapping_mul(19_349_663))
                % 600_000;
            self.world.moving += 1;
            actors.push(wowlua_engine::scene::Actor {
                source: wowlua_engine::scene::ModelSource::File(file),
                position: [m[12], m[13], m[14]],
                yaw: 0.0,
                pitch: 0.0,
                roll: 0.0,
                fixed_axes: false,
                scale: 1.0,
                alpha: 1.0,
                animation: 0,
                variation: 0,
                placement: Some(m),
                phase,
            });
        }
        // The light's sky models about the eye (decision T556): unlit,
        // each by its weight, drawn behind the world; posed before the
        // units, whose poses are kept for the next frame.
        let skyboxes: Vec<wowlua_engine::scene::Actor> = light
            .as_ref()
            .map(|l| {
                l.skyboxes
                    .iter()
                    .map(|&(file, _, weight)| wowlua_engine::scene::Actor {
                        source: wowlua_engine::scene::ModelSource::File(file),
                        position: [0.0; 3],
                        yaw: 0.0,
                        pitch: 0.0,
                        roll: 0.0,
                        fixed_axes: false,
                        scale: 1.0,
                        alpha: weight,
                        animation: 0,
                        variation: 0,
                        placement: None,
                        phase: 0,
                    })
                    .collect()
            })
            .unwrap_or_default();
        let sky_models = (!skyboxes.is_empty())
            .then(|| {
                let (l, u) = (f32s(w.look), f32s(w.up));
                let camera = wowlua_engine::scene::ModelCamera::Scene {
                    position: [0.0; 3],
                    forward: l,
                    right: [
                        l[1] * u[2] - l[2] * u[1],
                        l[2] * u[0] - l[0] * u[2],
                        l[0] * u[1] - l[1] * u[0],
                    ],
                    up: u,
                    fov: fov_y,
                    near: NEAR,
                    far: SKY_FAR,
                };
                self.world_models(
                    &skyboxes,
                    &camera,
                    (w.time * 1000.0) as u32,
                    crate::model::look_at([0.0; 3], l, u),
                    crate::model::perspective(fov_y, aspect, NEAR, SKY_FAR),
                    ([0.0, 0.0, -1.0], [1.0; 3], [0.0; 3]),
                    None,
                    assets,
                )
            })
            .flatten();
        let models = (!actors.is_empty())
            .then(|| {
                let right = {
                    let (l, u) = (f32s(w.look), f32s(w.up));
                    [
                        l[1] * u[2] - l[2] * u[1],
                        l[2] * u[0] - l[0] * u[2],
                        l[0] * u[1] - l[1] * u[0],
                    ]
                };
                let camera = wowlua_engine::scene::ModelCamera::Scene {
                    position: f32s(w.eye),
                    forward: f32s(w.look),
                    right,
                    up: f32s(w.up),
                    fov: fov_y,
                    near: NEAR,
                    far: w.far as f32,
                };
                let sun = globals.sun;
                // (the sun's shadow maps the units take: T564)
                let lookup = self
                    .world
                    .setup
                    .as_ref()
                    .and_then(|s| s.maps.as_ref())
                    .map(|m| {
                        let mut uniform = [0.0f32; crate::model::SUN_LOOKUP];
                        let parts: [&[f32]; 6] = [
                            globals.shadow_mats.as_flattened(),
                            &globals.shadow_ends,
                            &globals.shadow_texels,
                            &globals.shadow,
                            &globals.look,
                            &globals.eye,
                        ];
                        let mut at = 0;
                        for p in parts {
                            uniform[at..at + p.len()].copy_from_slice(p);
                            at += p.len();
                        }
                        crate::model::SunLookup {
                            uniform,
                            maps: m.array.clone(),
                        }
                    });
                self.world_models(
                    &actors,
                    &camera,
                    (w.time * 1000.0) as u32,
                    globals.view,
                    globals.proj,
                    ([-sun[0], -sun[1], -sun[2]], ambient, direct),
                    lookup,
                    assets,
                )
            })
            .flatten();
        let setup = self.world.setup.as_ref().expect("the world's setup");
        self.queue
            .write_buffer(&setup.globals, 0, bytemuck::bytes_of(&globals));
        self.queue
            .write_buffer(&setup.sky, 0, bytemuck::bytes_of(&sky_globals));
        let depth = &self.world.depth.as_ref().expect("the depth buffer").0;
        // (with MSAA, T578: drawn into the multisampled target, resolved
        // into the view at the end)
        let (color, resolve) = match &self.world.msaa_color {
            Some(c) => (c, Some(view)),
            None => (view, None),
        };
        let clear = wgpu::LoadOp::Clear(wgpu::Color {
            r: f64::from(fog[0]),
            g: f64::from(fog[1]),
            b: f64::from(fog[2]),
            a: 1.0,
        });
        // The sun's shadow maps first (T564): each as the sun sees it.
        if !sun_maps.is_empty() {
            for (k, c) in sun_maps.iter().enumerate() {
                let mut g = globals;
                g.view = c.view;
                g.proj = c.proj;
                self.queue
                    .write_buffer(&setup.sun[k].0, 0, bytemuck::bytes_of(&g));
            }
            let groups: Vec<wgpu::BindGroup> = match &models {
                Some(_) if mode >= 1 => sun_maps
                    .iter()
                    .map(|c| self.sun_model_group(wowlua_casc::m2::mul(&c.proj, &c.view)))
                    .collect(),
                _ => Vec::new(),
            };
            self.draw_sun_maps(
                encoder,
                w,
                &sun_maps,
                &detailed,
                &placed,
                &sun_ranges,
                models.as_ref(),
                &groups,
            );
        }
        // The sky and its models first, in a pass of their own: the
        // world's then starts from a cleared depth.
        if let Some(m) = &sky_models {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("world sky"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: color,
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: clear,
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                    view: depth,
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
            pass.set_pipeline(&setup.sky_pipeline);
            pass.set_bind_group(0, &setup.sky_group, &[]);
            pass.draw(0..3, 0..1);
            self.record_models(&mut pass, m);
        }
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("world"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: color,
                depth_slice: None,
                resolve_target: resolve,
                ops: wgpu::Operations {
                    load: if sky_models.is_some() {
                        wgpu::LoadOp::Load
                    } else {
                        clear
                    },
                    store: wgpu::StoreOp::Store,
                },
            })],
            depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                view: depth,
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
        // (what the camera can see: a ball of `reach` yards about a middle,
        // within the view's four sides; T538)
        let side = [
            w.look[1] * w.up[2] - w.look[2] * w.up[1],
            w.look[2] * w.up[0] - w.look[0] * w.up[2],
            w.look[0] * w.up[1] - w.look[1] * w.up[0],
        ];
        let (tan_x, tan_y) = ((w.fov / 2.0).tan(), (f64::from(fov_y) / 2.0).tan());
        let (cos_x, cos_y) = (
            1.0 / (1.0 + tan_x * tan_x).sqrt(),
            1.0 / (1.0 + tan_y * tan_y).sqrt(),
        );
        let in_view = |middle: [f64; 3], reach: f64| {
            let d = [
                middle[0] - w.eye[0],
                middle[1] - w.eye[1],
                middle[2] - w.eye[2],
            ];
            let along = |a: [f64; 3]| d[0] * a[0] + d[1] * a[1] + d[2] * a[2];
            let depth = along(w.look);
            // (a side plane's distance: the ball within it by its reach)
            depth > -reach
                && (along(side).abs() - depth * tan_x) * cos_x <= reach
                && (along(w.up).abs() - depth * tan_y) * cos_y <= reach
        };
        if sky_models.is_none() {
            pass.set_pipeline(&setup.sky_pipeline);
            pass.set_bind_group(0, &setup.sky_group, &[]);
            pass.draw(0..3, 0..1);
        }
        pass.set_pipeline(&setup.pipeline);
        pass.set_bind_group(0, &setup.globals_group, &[]);
        for &(x, y) in &detailed {
            let Some(Some(tile)) = self.world.tiles.get(&(w.map, x, y)) else {
                continue;
            };
            pass.set_vertex_buffer(0, tile.vertices.slice(..));
            pass.set_index_buffer(tile.indices.slice(..), wgpu::IndexFormat::Uint32);
            for c in &tile.chunks {
                let rel = [
                    c.center[0] - w.eye[0],
                    c.center[1] - w.eye[1],
                    c.center[2] - w.eye[2],
                ];
                let ahead = rel[0] * w.look[0] + rel[1] * w.look[1] + rel[2] * w.look[2];
                let far = (rel[0] * rel[0] + rel[1] * rel[1]).sqrt();
                // (a chunk wholly behind the eye, or past the view distance)
                if c.count == 0
                    || ahead < -1.5 * CHUNK
                    || far > w.far + CHUNK
                    || !in_view(c.center, CHUNK)
                {
                    continue;
                }
                pass.set_bind_group(1, &c.group, &[]);
                pass.draw_indexed(c.start..c.start + c.count, 0, 0..1);
            }
        }
        if let Some(m) = &models {
            self.record_models(&mut pass, m);
        }
        // The distant tiles: their coarse meshes, as they are.
        if let (Some(bsetup), Some(identity)) = (
            self.world.building_setup.as_ref(),
            self.world.identity.as_ref(),
        ) {
            pass.set_pipeline(&bsetup.opaque);
            pass.set_bind_group(0, &setup.globals_group, &[]);
            pass.set_vertex_buffer(1, identity.slice(..));
            for (x, y) in distant {
                let Some(Some(lod)) = self.world.lods.get(&(w.map, x, y)) else {
                    continue;
                };
                pass.set_vertex_buffer(0, lod.vertices.slice(..));
                pass.set_index_buffer(lod.indices.slice(..), wgpu::IndexFormat::Uint32);
                pass.set_bind_group(1, &lod.group, &[]);
                pass.draw_indexed(0..lod.count, 0, 0..1);
            }
        }
        // The buildings: their opaque batches, then the blended ones.
        let (Some(bsetup), Some((instances, _))) = (
            self.world.building_setup.as_ref(),
            self.world.instances.as_ref(),
        ) else {
            return;
        };
        for blended in [false, true] {
            pass.set_pipeline(if blended {
                &bsetup.blended
            } else {
                &bsetup.opaque
            });
            pass.set_bind_group(0, &setup.globals_group, &[]);
            for (i, (file, m)) in placed.iter().enumerate() {
                let Some(Some(b)) = self.world.buildings.get(file) else {
                    continue;
                };
                let at = i as u64 * 64;
                pass.set_vertex_buffer(1, instances.slice(at..at + 64));
                // (the placement's axes: their lengths its scale)
                let axis = |k: usize| [m[k * 4], m[k * 4 + 1], m[k * 4 + 2]];
                let scale = (0..3)
                    .map(|k| {
                        let a = axis(k);
                        (a[0] * a[0] + a[1] * a[1] + a[2] * a[2]).sqrt()
                    })
                    .fold(0.0f32, f32::max);
                for g in &b.groups {
                    let c = g.middle;
                    let world = [0, 1, 2].map(|r| {
                        f64::from(m[12 + r] + m[r] * c[0] + m[4 + r] * c[1] + m[8 + r] * c[2])
                    });
                    if !in_view(world, f64::from(g.reach * scale)) {
                        continue;
                    }
                    pass.set_vertex_buffer(0, g.vertices.slice(..));
                    pass.set_index_buffer(g.indices.slice(..), wgpu::IndexFormat::Uint16);
                    for batch in g.batches.iter().filter(|b| b.blended == blended) {
                        pass.set_bind_group(1, &batch.group, &[]);
                        pass.draw_indexed(batch.start..batch.start + batch.count, 0, 0..1);
                    }
                }
            }
            // (each model's doodads together)
            for &(file, first, count) in &doodad_ranges {
                let Some(Some(d)) = self.world.doodads.get(&file) else {
                    continue;
                };
                let (at, end) = (u64::from(first) * 64, u64::from(first + count) * 64);
                pass.set_vertex_buffer(1, instances.slice(at..end));
                pass.set_vertex_buffer(0, d.vertices.slice(..));
                pass.set_index_buffer(d.indices.slice(..), wgpu::IndexFormat::Uint32);
                for batch in d.batches.iter().filter(|b| b.blended == blended) {
                    pass.set_bind_group(1, &batch.group, &[]);
                    pass.draw_indexed(batch.start..batch.start + batch.count, 0, 0..count);
                }
            }
        }
        // The liquids last, seen through: what is under them shows (T557).
        if let Some(lsetup) = self.world.liquid_setup.as_ref() {
            pass.set_pipeline(&lsetup.pipeline);
            pass.set_bind_group(0, &setup.globals_group, &[]);
            let tick = (w.time * LIQUID_FPS) as usize;
            for &(x, y) in &detailed {
                let Some(Some(tile)) = self.world.tiles.get(&(w.map, x, y)) else {
                    continue;
                };
                let Some(l) = &tile.liquid else {
                    continue;
                };
                pass.set_vertex_buffer(0, l.vertices.slice(..));
                pass.set_index_buffer(l.indices.slice(..), wgpu::IndexFormat::Uint32);
                for d in &l.draws {
                    let Some(group) = d
                        .frames
                        .get(tick % d.frames.len().max(1))
                        .and_then(|f| lsetup.frames.get(f))
                    else {
                        continue;
                    };
                    pass.set_bind_group(1, group, &[]);
                    pass.set_bind_group(2, &d.how, &[]);
                    pass.draw_indexed(d.start..d.start + d.count, 0, 0..1);
                }
            }
        }
        // The weather over it all (T565).
        if drops > 0
            && let Some(ws) = self.world.weather_setup.as_ref()
            && let Some((_, group)) = &ws.group
        {
            pass.set_pipeline(&ws.pipeline);
            pass.set_bind_group(0, group, &[]);
            pass.draw(0..6, 0..drops);
        }
    }
}

use wowlua_casc::light::Kind as LightKind;

/// The weather's globals (`weather.wgsl`).
#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct WeatherGlobals {
    view_proj: [f32; 16],
    eye: [f32; 4],
    right: [f32; 4],
    up: [f32; 4],
    color: [f32; 4],
    how: [f32; 4],
}

/// The weather's pipeline, its globals, and their binding with the
/// texture that falls (its texture's index).
struct WeatherSetup {
    pipeline: wgpu::RenderPipeline,
    layout: wgpu::BindGroupLayout,
    globals: wgpu::Buffer,
    sampler: wgpu::Sampler,
    group: Option<(usize, wgpu::BindGroup)>,
}

/// How a kind of weather falls (decision T565; the game's unverified):
/// the box about the eye (yards), how fast (yards a second), a quad's size
/// (yards; rain's its width, eight times as long), the most at full
/// intensity and `weatherDensity` 3, and how opaque.
fn fall_of(kind: u32) -> Option<(f32, f32, f32, f32, f32)> {
    match kind {
        1 => Some((30.0, 20.0, 0.08, 8000.0, 0.8)),
        2 => Some((30.0, 1.5, 0.12, 5000.0, 0.9)),
        3 => Some((40.0, 6.0, 1.5, 1500.0, 0.25)),
        _ => None,
    }
}

/// How many drops fall: the kind's most, by the intensity and by
/// `weatherDensity` (0 to 3: a quarter to all).
fn drops_of(most: f32, intensity: f64, density: f64) -> u32 {
    (f64::from(most) * intensity.clamp(0.0, 1.0) * (density.clamp(0.0, 3.0) + 1.0) / 4.0).round()
        as u32
}

impl Renderer {
    /// Readies the weather's drawing for a frame (decision T565): its
    /// globals written, its texture bound; how many drops fall (none for
    /// clear or a build without the weather's look).
    fn world_weather(
        &mut self,
        w: &WorldView,
        globals: &Globals,
        direct: [f32; 3],
        ambient: [f32; 3],
        assets: &mut dyn Assets,
    ) -> u32 {
        self.world.weather_drops = 0;
        let (kind, intensity) = w.weather;
        let Some((side, speed, size, most, opacity)) = fall_of(kind) else {
            return 0;
        };
        let drops = drops_of(most, intensity, w.weather_density);
        let look = wowlua_casc::weather::Kind::of(kind).and_then(|k| match assets.weathers() {
            Some(ws) => ws.look(k, intensity as f32),
            None => k.texture().map(|texture| wowlua_casc::weather::Look {
                texture,
                color: [1.0; 3],
            }),
        });
        let Some(look) = look else {
            return 0;
        };
        if drops == 0 {
            return 0;
        }
        self.weather_setup();
        let tex = self.model_texture(look.texture, assets);
        if tex == self.missing {
            return 0;
        }
        let lit = [0, 1, 2].map(|k| look.color[k] * (ambient[k] + direct[k]).min(1.0));
        let l = [w.look[0] as f32, w.look[1] as f32, w.look[2] as f32];
        let u = [w.up[0] as f32, w.up[1] as f32, w.up[2] as f32];
        let right = [
            l[1] * u[2] - l[2] * u[1],
            l[2] * u[0] - l[0] * u[2],
            l[0] * u[1] - l[1] * u[0],
        ];
        let g = WeatherGlobals {
            view_proj: wowlua_casc::m2::mul(&globals.proj, &globals.view),
            eye: [
                w.eye[0] as f32,
                w.eye[1] as f32,
                w.eye[2] as f32,
                w.time as f32,
            ],
            right: [right[0], right[1], right[2], 0.0],
            up: [u[0], u[1], u[2], 0.0],
            color: [lit[0], lit[1], lit[2], opacity],
            how: [kind as f32, side, speed, size],
        };
        let ws = self
            .world
            .weather_setup
            .as_mut()
            .expect("the weather's setup");
        self.queue
            .write_buffer(&ws.globals, 0, bytemuck::bytes_of(&g));
        if ws.group.as_ref().map(|(t, _)| *t) != Some(tex) {
            let group = self.device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("world weather"),
                layout: &ws.layout,
                entries: &[
                    wgpu::BindGroupEntry {
                        binding: 0,
                        resource: ws.globals.as_entire_binding(),
                    },
                    wgpu::BindGroupEntry {
                        binding: 1,
                        resource: wgpu::BindingResource::TextureView(&self.textures[tex].view),
                    },
                    wgpu::BindGroupEntry {
                        binding: 2,
                        resource: wgpu::BindingResource::Sampler(&ws.sampler),
                    },
                ],
            });
            ws.group = Some((tex, group));
        }
        self.world.weather_drops = drops;
        drops
    }

    fn weather_setup(&mut self) {
        if self.world.weather_setup.is_some() {
            return;
        }
        // (the world's multisampling: T578)
        let samples = self.world.samples;
        let d = &self.device;
        let layout = d.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("world weather"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
                texture_entry(1),
                sampler_entry(2),
            ],
        });
        let shader = d.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("world weather"),
            source: wgpu::ShaderSource::Wgsl(include_str!("weather.wgsl").into()),
        });
        let pipeline = d.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("world weather"),
            layout: Some(&d.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                label: Some("world weather"),
                bind_group_layouts: &[Some(&layout)],
                immediate_size: 0,
            })),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vs"),
                compilation_options: Default::default(),
                buffers: &[],
            },
            primitive: wgpu::PrimitiveState::default(),
            depth_stencil: Some(wgpu::DepthStencilState {
                format: wgpu::TextureFormat::Depth32Float,
                depth_write_enabled: Some(false),
                depth_compare: Some(wgpu::CompareFunction::LessEqual),
                stencil: wgpu::StencilState::default(),
                bias: wgpu::DepthBiasState::default(),
            }),
            multisample: msaa(samples),
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some("fs"),
                compilation_options: Default::default(),
                targets: &[Some(wgpu::ColorTargetState {
                    format: crate::FORMAT,
                    blend: Some(wgpu::BlendState::ALPHA_BLENDING),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
            }),
            multiview_mask: None,
            cache: None,
        });
        let globals = d.create_buffer(&wgpu::BufferDescriptor {
            label: Some("world weather"),
            size: std::mem::size_of::<WeatherGlobals>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let sampler = d.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("world weather"),
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            mipmap_filter: wgpu::MipmapFilterMode::Linear,
            ..Default::default()
        });
        self.world.weather_setup = Some(WeatherSetup {
            pipeline,
            layout,
            globals,
            sampler,
            group: None,
        });
    }
}

/// A tile's liquids as one mesh (decision T557): each liquid's points over
/// its chunk's squares (its heights, or its level where it stores none),
/// its type's frames, the squares it has; by type. Its texture repeats
/// every two of the grid's squares (unverified). WoW Forever's PBR water
/// is drawn as its water until its own look is (T557).
#[allow(clippy::type_complexity)]
fn liquid_mesh(
    tile: &Tile,
    column: i32,
    row: i32,
    kinds: &wowlua_casc::liquid::Liquids,
) -> (Vec<LVertex>, Vec<u32>, Vec<LiquidPart>) {
    use wowlua_casc::liquid::Look;
    let (north, west) = adt::corner(column, row);
    let mut verts = Vec::new();
    let mut by_kind: Vec<(u32, LiquidPart, Vec<u32>)> = Vec::new();
    for l in &tile.liquids {
        let Some(r) = kinds.resolve(l) else {
            continue;
        };
        let (frames, glowing) = match r.kind.look {
            Look::Glowing => (r.kind.frames.clone(), true),
            Look::Pbr => (
                kinds.kind(1).map_or_else(Vec::new, |k| k.frames.clone()),
                false,
            ),
            Look::Water => (r.kind.frames.clone(), false),
        };
        if frames.is_empty() {
            continue;
        }
        let (heights, depths) = l.heights(r.format);
        let (w, h) = (usize::from(l.width), usize::from(l.height));
        let n0 = north - f64::from(l.chunk.1) * CHUNK - f64::from(l.y) * UNIT;
        let w0 = west - f64::from(l.chunk.0) * CHUNK - f64::from(l.x) * UNIT;
        let base = verts.len() as u32;
        for j in 0..=h {
            for i in 0..=w {
                let k = j * (w + 1) + i;
                let (pn, pw) = (n0 - j as f64 * UNIT, w0 - i as f64 * UNIT);
                let height = heights
                    .get(k)
                    .copied()
                    .filter(|v| v.is_finite())
                    .unwrap_or(l.min);
                verts.push(LVertex {
                    pos: [pn as f32, pw as f32, height],
                    uv: [(-pw / (UNIT * 2.0)) as f32, (-pn / (UNIT * 2.0)) as f32],
                    depth: f32::from(depths.get(k).copied().unwrap_or(255)) / 255.0,
                });
            }
        }
        let slot = match by_kind.iter().position(|b| b.0 == r.kind.id) {
            Some(s) => s,
            None => {
                let colors = match r.kind.colors.as_slice() {
                    [near, far, ..] => Some((*near, *far)),
                    [one] => Some((*one, *one)),
                    [] => None,
                };
                by_kind.push((
                    r.kind.id,
                    (frames, glowing, r.kind.ocean, colors, 0, 0),
                    Vec::new(),
                ));
                by_kind.len() - 1
            }
        };
        let list = &mut by_kind[slot].2;
        for sy in 0..h {
            for sx in 0..w {
                if !l.exists.get(sy * w + sx).copied().unwrap_or(false) {
                    continue;
                }
                let nw = base + (sy * (w + 1) + sx) as u32;
                let (ne, sw) = (nw + 1, nw + (w + 1) as u32);
                let se = sw + 1;
                list.extend([nw, ne, se, nw, se, sw]);
            }
        }
    }
    let mut indices = Vec::new();
    let mut parts = Vec::new();
    for (_, (frames, glowing, ocean, colors, _, _), list) in by_kind {
        if list.is_empty() {
            continue;
        }
        let start = indices.len() as u32;
        indices.extend(list);
        parts.push((
            frames,
            glowing,
            ocean,
            colors,
            start,
            indices.len() as u32 - start,
        ));
    }
    (verts, indices, parts)
}

#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
pub(crate) struct BVertex {
    pub(crate) pos: [f32; 3],
    pub(crate) normal: [f32; 3],
    pub(crate) uv: [f32; 2],
    pub(crate) color: [f32; 4],
}

/// A batch of a building's group: its indices and its material.
struct GpuBatch {
    start: u32,
    count: u32,
    group: wgpu::BindGroup,
    blended: bool,
}

struct GpuGroup {
    vertices: wgpu::Buffer,
    indices: wgpu::Buffer,
    batches: Vec<GpuBatch>,
    /// Its middle and how far it reaches from it, in the building's own
    /// space (what the view must see of it, T538).
    middle: [f32; 3],
    reach: f32,
    /// Its bounds in the building's own space where it is indoors (its
    /// flags: T581), else none.
    indoor: Option<[[f32; 3]; 2]>,
}

/// A building on the GPU: its groups, drawn wherever it is placed.
struct GpuBuilding {
    groups: Vec<GpuGroup>,
}

struct BuildingSetup {
    opaque: wgpu::RenderPipeline,
    blended: wgpu::RenderPipeline,
    /// The depth the sun sees (T564).
    sun: wgpu::RenderPipeline,
    material_layout: wgpu::BindGroupLayout,
}

/// A placement's matrix (columns): the model's axes and origin on the map.
fn matrix_of(b: &wowlua_casc::obj::Building) -> [f32; 16] {
    let f = wowlua_casc::obj::placed(b.at, b.rotation, b.scale);
    let o = f([0.0; 3]);
    let axis = |v: [f32; 3]| {
        let p = f(v);
        [
            (p[0] - o[0]) as f32,
            (p[1] - o[1]) as f32,
            (p[2] - o[2]) as f32,
        ]
    };
    let (x, y, z) = (
        axis([1.0, 0.0, 0.0]),
        axis([0.0, 1.0, 0.0]),
        axis([0.0, 0.0, 1.0]),
    );
    [
        x[0],
        x[1],
        x[2],
        0.0,
        y[0],
        y[1],
        y[2],
        0.0,
        z[0],
        z[1],
        z[2],
        0.0,
        o[0] as f32,
        o[1] as f32,
        o[2] as f32,
        1.0,
    ]
}

impl Renderer {
    fn building_setup(&mut self) {
        if self.world.building_setup.is_some() {
            return;
        }
        // (the world's multisampling: T578)
        let samples = self.world.samples;
        let d = &self.device;
        let globals_layout = &self
            .world
            .setup
            .as_ref()
            .expect("the world's setup")
            .globals_layout;
        let material_layout = d.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("building material"),
            entries: &[
                texture_entry(0),
                sampler_entry(1),
                wgpu::BindGroupLayoutEntry {
                    binding: 2,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
            ],
        });
        let layout = d.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("buildings"),
            bind_group_layouts: &[Some(globals_layout), Some(&material_layout)],
            immediate_size: 0,
        });
        let shader = d.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("buildings"),
            source: wgpu::ShaderSource::Wgsl(include_str!("building.wgsl").into()),
        });
        // (and the depth the sun sees: T564)
        let pipeline = |blended: bool, sun: bool| {
            let color = [Some(wgpu::ColorTargetState {
                format: crate::FORMAT,
                blend: blended.then_some(wgpu::BlendState::ALPHA_BLENDING),
                write_mask: wgpu::ColorWrites::ALL,
            })];
            d.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some("buildings"),
                layout: Some(&layout),
                vertex: wgpu::VertexState {
                    module: &shader,
                    entry_point: Some("vs"),
                    compilation_options: Default::default(),
                    buffers: &[
                        Some(wgpu::VertexBufferLayout {
                            array_stride: std::mem::size_of::<BVertex>() as u64,
                            step_mode: wgpu::VertexStepMode::Vertex,
                            attributes: &wgpu::vertex_attr_array![0 => Float32x3, 1 => Float32x3, 2 => Float32x2, 3 => Float32x4],
                        }),
                        Some(wgpu::VertexBufferLayout {
                            array_stride: 64,
                            step_mode: wgpu::VertexStepMode::Instance,
                            attributes: &wgpu::vertex_attr_array![4 => Float32x4, 5 => Float32x4, 6 => Float32x4, 7 => Float32x4],
                        }),
                    ],
                },
                primitive: wgpu::PrimitiveState::default(),
                depth_stencil: Some(wgpu::DepthStencilState {
                    format: wgpu::TextureFormat::Depth32Float,
                    depth_write_enabled: Some(!blended),
                    depth_compare: Some(wgpu::CompareFunction::LessEqual),
                    stencil: wgpu::StencilState::default(),
                    bias: if sun {
                        crate::model::SUN_BIAS
                    } else {
                        wgpu::DepthBiasState::default()
                    },
                }),
                multisample: if sun { msaa(1) } else { msaa(samples) },
                fragment: Some(wgpu::FragmentState {
                    module: &shader,
                    entry_point: Some(if sun { "fs_shadow" } else { "fs" }),
                    compilation_options: Default::default(),
                    targets: if sun { &[] } else { &color },
                }),
                multiview_mask: None,
                cache: None,
            })
        };
        let (opaque, blended, sun) = (
            pipeline(false, false),
            pipeline(true, false),
            pipeline(false, true),
        );
        let one: [f32; 16] = [
            1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0,
        ];
        self.world.identity = Some(d.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("in place"),
            contents: bytemuck::cast_slice(&one),
            usage: wgpu::BufferUsages::VERTEX,
        }));
        self.world.building_setup = Some(BuildingSetup {
            opaque,
            blended,
            sun,
            material_layout,
        });
    }

    /// A building on the GPU, read and built on first use.
    fn world_building(&mut self, file: u32, assets: &mut dyn Assets) {
        if self.world.buildings.contains_key(&file) {
            return;
        }
        let gpu = assets.building(file).map(|wmo| {
            let mut groups = Vec::new();
            for g in &wmo.groups {
                if g.positions.is_empty() || g.indices.is_empty() {
                    continue;
                }
                let indoors = g.flags & 0x2000 != 0 && g.flags & 0x8 == 0;
                let verts: Vec<BVertex> = (0..g.positions.len())
                    .map(|i| BVertex {
                        pos: g.positions[i],
                        normal: g.normals.get(i).copied().unwrap_or([0.0, 0.0, 1.0]),
                        uv: g.uvs.get(i).copied().unwrap_or([0.0, 0.0]),
                        color: g.colors.get(i).map_or([0.5, 0.5, 0.5, 1.0], |c| {
                            // (BGRA)
                            [
                                f32::from(c[2]) / 255.0,
                                f32::from(c[1]) / 255.0,
                                f32::from(c[0]) / 255.0,
                                f32::from(c[3]) / 255.0,
                            ]
                        }),
                    })
                    .collect();
                // (an even count: the buffer's size a multiple of four bytes)
                let mut indices = g.indices.clone();
                if indices.len() % 2 == 1 {
                    indices.push(0);
                }
                let vertices = self
                    .device
                    .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                        label: Some("building"),
                        contents: bytemuck::cast_slice(&verts),
                        usage: wgpu::BufferUsages::VERTEX,
                    });
                let index_buffer =
                    self.device
                        .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                            label: Some("building"),
                            contents: bytemuck::cast_slice(&indices),
                            usage: wgpu::BufferUsages::INDEX,
                        });
                let mut batches = Vec::new();
                for b in &g.batches {
                    let Some(m) = wmo.root.materials.get(b.material as usize) else {
                        continue;
                    };
                    let tex = match m.textures[0] {
                        0 => self.white,
                        f => self.model_texture(f, assets),
                    };
                    let how: [f32; 4] = [
                        if m.blend == 1 { 1.0 } else { 0.0 },
                        if indoors { 1.0 } else { 0.0 },
                        if m.flags & 0x1 != 0 { 1.0 } else { 0.0 },
                        0.0,
                    ];
                    let how = self
                        .device
                        .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                            label: Some("building material"),
                            contents: bytemuck::cast_slice(&how),
                            usage: wgpu::BufferUsages::UNIFORM,
                        });
                    let setup = self.world.setup.as_ref().expect("the world's setup");
                    let bsetup = self
                        .world
                        .building_setup
                        .as_ref()
                        .expect("the buildings' setup");
                    let group = self.device.create_bind_group(&wgpu::BindGroupDescriptor {
                        label: Some("building material"),
                        layout: &bsetup.material_layout,
                        entries: &[
                            wgpu::BindGroupEntry {
                                binding: 0,
                                resource: wgpu::BindingResource::TextureView(
                                    &self.textures[tex].view,
                                ),
                            },
                            wgpu::BindGroupEntry {
                                binding: 1,
                                resource: wgpu::BindingResource::Sampler(&setup.tiled),
                            },
                            wgpu::BindGroupEntry {
                                binding: 2,
                                resource: how.as_entire_binding(),
                            },
                        ],
                    });
                    batches.push(GpuBatch {
                        start: b.start,
                        count: b.count,
                        group,
                        blended: m.blend >= 2,
                    });
                }
                let (lo, hi) = (g.bounds[0], g.bounds[1]);
                let half = [
                    (hi[0] - lo[0]) / 2.0,
                    (hi[1] - lo[1]) / 2.0,
                    (hi[2] - lo[2]) / 2.0,
                ];
                groups.push(GpuGroup {
                    vertices,
                    indices: index_buffer,
                    batches,
                    middle: [lo[0] + half[0], lo[1] + half[1], lo[2] + half[2]],
                    reach: (half[0] * half[0] + half[1] * half[1] + half[2] * half[2]).sqrt(),
                    indoor: indoors.then_some(g.bounds),
                });
            }
            GpuBuilding { groups }
        });
        self.world.buildings.insert(file, gpu);
    }
}

/// A doodad model on the GPU: its standing pose's mesh, its batches.
struct GpuDoodad {
    vertices: wgpu::Buffer,
    indices: wgpu::Buffer,
    batches: Vec<GpuBatch>,
}

/// A matrix (columns) of a place-turning function: the model's axes and
/// origin on the map.
fn matrix_from(f: impl Fn([f32; 3]) -> [f64; 3]) -> [f32; 16] {
    let o = f([0.0; 3]);
    let axis = |v: [f32; 3]| {
        let p = f(v);
        [
            (p[0] - o[0]) as f32,
            (p[1] - o[1]) as f32,
            (p[2] - o[2]) as f32,
        ]
    };
    let (x, y, z) = (
        axis([1.0, 0.0, 0.0]),
        axis([0.0, 1.0, 0.0]),
        axis([0.0, 0.0, 1.0]),
    );
    [
        x[0],
        x[1],
        x[2],
        0.0,
        y[0],
        y[1],
        y[2],
        0.0,
        z[0],
        z[1],
        z[2],
        0.0,
        o[0] as f32,
        o[1] as f32,
        o[2] as f32,
        1.0,
    ]
}

impl Renderer {
    /// A doodad model on the GPU, read and posed on first use: its
    /// standing pose (the first frame of "Stand"), its own textures.
    /// Whether a prop's model moves as it stands (T558), read once.
    fn prop_moves(&mut self, file: u32, assets: &mut dyn Assets) -> bool {
        if let Some(&m) = self.world.moves.get(&file) {
            return m;
        }
        let m = assets
            .file(file)
            .and_then(|b| wowlua_casc::m2::Model::parse(&b))
            .is_some_and(|m| m.moves());
        self.world.moves.insert(file, m);
        m
    }

    fn world_doodad(&mut self, file: u32, assets: &mut dyn Assets) {
        if self.world.doodads.contains_key(&file) {
            return;
        }
        // (a model of nothing to draw: none)
        let Some((verts, indices, batches)) = self
            .model_mesh(file, assets)
            .filter(|(v, i, _)| !v.is_empty() && !i.is_empty())
        else {
            self.world.doodads.insert(file, None);
            return;
        };
        let mut gpu_batches = Vec::new();
        for (start, count, texture, blend, flags) in batches {
            let tex = match texture {
                0 => self.white,
                f => self.model_texture(f, assets),
            };
            let how: [f32; 4] = [
                if blend == 1 { 1.0 } else { 0.0 },
                0.0,
                if flags & 0x1 != 0 { 1.0 } else { 0.0 },
                0.0,
            ];
            let how = self
                .device
                .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                    label: Some("doodad material"),
                    contents: bytemuck::cast_slice(&how),
                    usage: wgpu::BufferUsages::UNIFORM,
                });
            let setup = self.world.setup.as_ref().expect("the world's setup");
            let bsetup = self
                .world
                .building_setup
                .as_ref()
                .expect("the buildings' setup");
            let group = self.device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("doodad material"),
                layout: &bsetup.material_layout,
                entries: &[
                    wgpu::BindGroupEntry {
                        binding: 0,
                        resource: wgpu::BindingResource::TextureView(&self.textures[tex].view),
                    },
                    wgpu::BindGroupEntry {
                        binding: 1,
                        resource: wgpu::BindingResource::Sampler(&setup.tiled),
                    },
                    wgpu::BindGroupEntry {
                        binding: 2,
                        resource: how.as_entire_binding(),
                    },
                ],
            });
            gpu_batches.push(GpuBatch {
                start,
                count,
                group,
                blended: blend >= 2,
            });
        }
        let vertices = self
            .device
            .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("doodad"),
                contents: bytemuck::cast_slice(&verts),
                usage: wgpu::BufferUsages::VERTEX,
            });
        let index_buffer = self
            .device
            .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("doodad"),
                contents: bytemuck::cast_slice(&indices),
                usage: wgpu::BufferUsages::INDEX,
            });
        self.world.doodads.insert(
            file,
            Some(GpuDoodad {
                vertices,
                indices: index_buffer,
                batches: gpu_batches,
            }),
        );
    }
}

/// A distant tile's coarse mesh (decision T525): its ground every
/// sixteenth of a chunk's side twice over, its minimap picture on it.
struct GpuLod {
    vertices: wgpu::Buffer,
    indices: wgpu::Buffer,
    count: u32,
    group: wgpu::BindGroup,
}

/// How many points a distant tile's coarse mesh has a side.
const LOD_SIDE: usize = 33;
/// The side (pixels) a distant tile's picture is drawn at.
const LOD_PICTURE: u32 = 128;

/// An image made smaller (`side` by `side`), each pixel its square's
/// average.
fn shrunk(img: &Rgba, side: u32) -> Rgba {
    let mut pixels = vec![0u8; (side * side * 4) as usize];
    let (fx, fy) = (
        img.width.max(1) as f64 / f64::from(side),
        img.height.max(1) as f64 / f64::from(side),
    );
    for y in 0..side {
        for x in 0..side {
            let (x0, y0) = ((f64::from(x) * fx) as u32, (f64::from(y) * fy) as u32);
            let (x1, y1) = (
                ((f64::from(x + 1) * fx) as u32).clamp(x0 + 1, img.width.max(1)),
                ((f64::from(y + 1) * fy) as u32).clamp(y0 + 1, img.height.max(1)),
            );
            let mut sum = [0u32; 4];
            let mut n = 0;
            for yy in y0..y1 {
                for xx in x0..x1 {
                    let i = ((yy * img.width + xx) * 4) as usize;
                    for (k, s) in sum.iter_mut().enumerate() {
                        *s += u32::from(img.pixels.get(i + k).copied().unwrap_or(0));
                    }
                    n += 1;
                }
            }
            let o = ((y * side + x) * 4) as usize;
            for (p, s) in pixels[o..o + 4].iter_mut().zip(sum) {
                *p = (s / n.max(1)) as u8;
            }
        }
    }
    Rgba {
        width: side,
        height: side,
        pixels,
    }
}

impl Renderer {
    /// A distant tile's coarse mesh on the GPU, made on first use.
    fn world_lod(&mut self, map: u32, x: i32, y: i32, assets: &mut dyn Assets) {
        if self.world.lods.contains_key(&(map, x, y)) {
            return;
        }
        let Some(tile) = assets.terrain(map, x, y) else {
            self.world.lods.insert((map, x, y), None);
            return;
        };
        let (north, west) = adt::corner(x, y);
        let step = TILE / (LOD_SIDE - 1) as f64;
        let mut verts = Vec::with_capacity(LOD_SIDE * LOD_SIDE);
        let mut last = tile.chunks.first().map_or(0.0, |c| f64::from(c.base));
        for r in 0..LOD_SIDE {
            for c in 0..LOD_SIDE {
                let (s, e) = (
                    (r as f64 * step).min(TILE - 1e-3),
                    (c as f64 * step).min(TILE - 1e-3),
                );
                // (over a hole: the height before it)
                let h = tile.height_at(s, e).unwrap_or(last);
                last = h;
                verts.push(BVertex {
                    pos: [(north - s) as f32, (west - e) as f32, h as f32],
                    normal: [0.0, 0.0, 1.0],
                    uv: [
                        c as f32 / (LOD_SIDE - 1) as f32,
                        r as f32 / (LOD_SIDE - 1) as f32,
                    ],
                    color: [0.5, 0.5, 0.5, 1.0],
                });
            }
        }
        let mut indices = Vec::with_capacity((LOD_SIDE - 1) * (LOD_SIDE - 1) * 6);
        for r in 0..LOD_SIDE - 1 {
            for c in 0..LOD_SIDE - 1 {
                let i = (r * LOD_SIDE + c) as u32;
                let s = LOD_SIDE as u32;
                indices.extend([i, i + 1, i + s, i + 1, i + s + 1, i + s]);
            }
        }
        let tex = assets
            .minimap_tile(map, x, y)
            .and_then(|f| assets.image(&wowlua_engine::scene::Image::FileId(f)))
            .map_or(self.white, |img| self.upload(&shrunk(&img, LOD_PICTURE)));
        // (the minimap's picture is lit already)
        let how: [f32; 4] = [0.0, 0.0, 1.0, 0.0];
        let how = self
            .device
            .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("distant tile"),
                contents: bytemuck::cast_slice(&how),
                usage: wgpu::BufferUsages::UNIFORM,
            });
        let setup = self.world.setup.as_ref().expect("the world's setup");
        let bsetup = self
            .world
            .building_setup
            .as_ref()
            .expect("the buildings' setup");
        let group = self.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("distant tile"),
            layout: &bsetup.material_layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(&self.textures[tex].view),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::Sampler(&setup.clamped),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: how.as_entire_binding(),
                },
            ],
        });
        let vertices = self
            .device
            .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("distant tile"),
                contents: bytemuck::cast_slice(&verts),
                usage: wgpu::BufferUsages::VERTEX,
            });
        let index_buffer = self
            .device
            .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("distant tile"),
                contents: bytemuck::cast_slice(&indices),
                usage: wgpu::BufferUsages::INDEX,
            });
        self.world.lods.insert(
            (map, x, y),
            Some(GpuLod {
                vertices,
                indices: index_buffer,
                count: indices.len() as u32,
                group,
            }),
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_unit_fades_as_the_eye_comes_in() {
        let at = [0.0, 0.0, 0.0];
        assert_eq!(near_fade([5.0, 0.0, 1.5], at), 1.0);
        assert_eq!(near_fade([0.2, 0.0, 1.5], at), 0.0);
        assert!((near_fade([0.65, 0.0, 1.5], at) - 0.5).abs() < 1e-6);
        assert_eq!(near_fade([1.2, 0.0, 1.5], at), 1.0);
        // (above its head)
        assert_eq!(near_fade([0.0, 0.0, 2.4], at), 0.0);
        assert_eq!(near_fade([0.0, 0.0, 4.0], at), 1.0);
    }

    #[test]
    fn a_chunk_s_baked_shadow_is_its_blend_s_alpha() {
        let mut shadow = vec![0u64; 64];
        shadow[0] = 1 | 1 << 63;
        shadow[63] = 2;
        let chunk = adt::Chunk {
            x: 0,
            y: 0,
            base: 0.0,
            heights: vec![0.0; 145],
            normals: vec![[0, 0, 127]; 145],
            holes: 0,
            area: 0,
            layers: Vec::new(),
            effects: [0; 64],
            no_effect: 0,
            shadow: Some(shadow),
            flags: 0,
        };
        let blend = blend_of(&chunk);
        let alpha = |row: usize, col: usize| blend.pixels[(row * 64 + col) * 4 + 3];
        assert_eq!((alpha(0, 0), alpha(0, 1), alpha(0, 63)), (255, 0, 255));
        assert_eq!((alpha(63, 0), alpha(63, 1)), (0, 255));
        let lit = blend_of(&adt::Chunk {
            shadow: None,
            ..chunk
        });
        assert!(lit.pixels.chunks(4).all(|p| p[3] == 0));
    }

    #[test]
    fn a_blob_is_wider_than_its_body() {
        assert_eq!(blob_radius(0.5), 1.0);
        assert_eq!(blob_radius(0.1), 0.4);
    }

    #[test]
    fn the_sun_s_maps_reach_by_the_shadow_mode() {
        let s = |mode| wowlua_engine::scene::Shadows {
            mode,
            entity_fade: 50.0,
            ..Default::default()
        };
        assert_eq!(sun_reach(&s(0), 1000.0), 0.0);
        assert_eq!(sun_reach(&s(1), 1000.0), 50.0);
        assert_eq!(sun_reach(&s(3), 1000.0), 200.0);
        assert_eq!(sun_reach(&s(4), 150.0), 150.0);
    }

    #[test]
    fn the_sun_s_maps_split_the_view_and_hold_what_they_cover() {
        let w = WorldView {
            map: 0,
            eye: [-8950.0, -130.0, 90.0],
            look: [1.0, 0.0, 0.0],
            up: [0.0, 0.0, 1.0],
            fov: 90f64.to_radians(),
            far: 1000.0,
            detail: 500.0,
            objects: 500.0,
            cull: 18.0,
            render_scale: 1.0,
            units: Vec::new(),
            time: 0.0,
            day_time: 1440.0,
            clutter_dist: 0.0,
            clutter_density: 0.0,
            shadows: Default::default(),
            weather: (0, 0.0),
            weather_density: 2.0,
            msaa: 1,
            post_aa: 0,
            filtering: 5,
            specular: true,
            terrain_mip: 0,
            horizon: 1000.0,
        };
        let t = (45f64).to_radians().tan();
        let maps = cascades(&w, t, t * 9.0 / 16.0, [0.3, 0.2, 0.9], 200.0, 3, 2048);
        assert_eq!(maps.len(), 3);
        // (each farther and bigger, the last ending where they reach)
        assert!(maps[0].end < maps[1].end && maps[1].end < maps[2].end);
        assert!((maps[2].end - 200.0).abs() < 1e-3);
        assert!(maps[0].texel < maps[1].texel && maps[1].texel < maps[2].texel);
        // (a point ahead in each map's stretch is in its box, one far
        // behind the eye in none)
        let ahead = |d: f64| [w.eye[0] + d, w.eye[1], w.eye[2]];
        assert!(maps[0].covers(ahead(f64::from(maps[0].end) / 2.0), 0.0));
        assert!(maps[2].covers(ahead(150.0), 0.0));
        assert!(!maps[0].covers(ahead(-500.0), 0.0));
        // (the point in the map's middle lands in its middle)
        let m = wowlua_casc::m2::mul(&maps[1].proj, &maps[1].view);
        let p = ahead(f64::from(maps[0].end + maps[1].end) / 2.0);
        let x = |r: usize| {
            f64::from(m[r]) * p[0]
                + f64::from(m[4 + r]) * p[1]
                + f64::from(m[8 + r]) * p[2]
                + f64::from(m[12 + r])
        };
        assert!(x(0).abs() < 1.0 && x(1).abs() < 1.0 && (0.0..1.0).contains(&x(2)));
    }
}
