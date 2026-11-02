//! The renderer (spec debugger §18.11, decision T311): draws a scene of the
//! engine (`wowlua_engine::scene`) with `wgpu`, off-screen into an image or
//! into a texture view the host presents (Studio's window).
//!
//! Fast by keeping work: images and glyphs are uploaded once and kept on
//! the GPU; a frame is one vertex buffer and a draw call per run of quads
//! sharing a texture, mask, blend mode and clip rectangle.

mod answers;
#[cfg(test)]
mod bench;
pub mod client;
mod combiners;
pub mod compare;
mod composite;
mod glyphs;
mod model;
mod world;

pub use world::project;

use std::collections::HashMap;

pub use wowlua_casc::Rgba;
pub use wowlua_casc::display::Display;
use wowlua_engine::scene::{Blend, Image, Item, ModelSource, Rect, Scene};

/// Where images and fonts come from: the addon's folder, the user's
/// installation.
pub trait Assets {
    /// An image, decoded.
    fn image(&mut self, image: &Image) -> Option<Rgba>;
    /// A font file's bytes (`Fonts\FRIZQT__.TTF`).
    fn font(&mut self, path: &str) -> Option<Vec<u8>>;
    /// An atlas as the build's own tables give it (its file, region,
    /// size, slices, tiling); none to use the engine's.
    fn atlas(&mut self, _name: &str) -> Option<AtlasRegion> {
        None
    }
    /// A file of the client's storage by ID (models, skins, animations).
    fn file(&mut self, _id: u32) -> Option<Vec<u8>> {
        None
    }
    /// What a model shows, as the build's tables give it.
    fn model(&mut self, _source: &ModelSource) -> Option<Display> {
        None
    }
    /// A map's terrain tile, by column and row from the north-west
    /// (decision T522).
    fn terrain(
        &mut self,
        _map: u32,
        _x: i32,
        _y: i32,
    ) -> Option<std::sync::Arc<wowlua_casc::adt::Tile>> {
        None
    }
    /// A terrain tile's buildings and doodads (decision T523).
    fn objects(
        &mut self,
        _map: u32,
        _x: i32,
        _y: i32,
    ) -> Option<std::sync::Arc<wowlua_casc::obj::Objects>> {
        None
    }
    /// A building with its groups, by its root file (decision T523).
    fn building(&mut self, _file: u32) -> Option<std::sync::Arc<wowlua_casc::display::Building>> {
        None
    }
    /// A map's minimap picture of a terrain tile (a file ID; decision T525).
    fn minimap_tile(&mut self, _map: u32, _x: i32, _y: i32) -> Option<u32> {
        None
    }
    /// The light at a place of a map (north, west, up) at a time of day in
    /// half-minutes (decision T555), its clear or its storm set (the
    /// weather's, T565); none for a plain sky.
    fn light(
        &mut self,
        _map: u32,
        _at: [f64; 3],
        _day_time: f64,
        _kind: wowlua_casc::light::Kind,
    ) -> Option<wowlua_casc::light::Sky> {
        None
    }
    /// The build's weathers (decision T565).
    fn weathers(&mut self) -> Option<std::sync::Arc<wowlua_casc::weather::Weathers>> {
        None
    }
    /// The build's liquids (decision T557).
    fn liquids(&mut self) -> Option<std::sync::Arc<wowlua_casc::liquid::Liquids>> {
        None
    }
    /// The build's ground effects (decision T560).
    fn clutter(&mut self) -> Option<std::sync::Arc<wowlua_casc::clutter::Clutter>> {
        None
    }
}

/// An atlas's part of its file.
#[derive(Clone, Debug)]
pub struct AtlasRegion {
    pub image: Image,
    /// Left, right, top, bottom (0 to 1).
    pub coords: [f32; 4],
    /// Its size in the file's pixels.
    pub size: (u32, u32),
    /// Nine-slice margins in its pixels (left, top, right, bottom) and the
    /// middle's mode (0 stretched, 1 tiled).
    pub slice: Option<([u32; 4], u8)>,
    pub tiles: (bool, bool),
}

/// One axis of a sliced or tiled quad: (destination start, length, source
/// start, length) pieces.
fn axis_pieces(
    start: f32,
    len: f32,
    src: (f32, f32),
    px: f32,
    margins: Option<(f32, f32)>,
    tile_middle: bool,
    unit: f32,
) -> Vec<(f32, f32, f32, f32)> {
    let (s0, s1) = src;
    let span = s1 - s0;
    let mut out = Vec::new();
    // The ends, as they are (scaled down where they would overlap).
    let (a, b) = margins.unwrap_or((0.0, 0.0));
    let (mut da, mut db) = (a * unit, b * unit);
    if da + db > len && da + db > 0.0 {
        let k = len / (da + db);
        da *= k;
        db *= k;
    }
    if da > 0.0 {
        out.push((start, da, s0, span * a / px));
    }
    let (ms, ml) = (start + da, len - da - db);
    let (src_ms, src_ml) = (s0 + span * a / px, span * (px - a - b) / px);
    if ml > 0.0 && src_ml > 0.0 {
        let tile = (px - a - b) * unit;
        if tile_middle && tile > 0.5 && ml / tile < 512.0 {
            let mut at = 0.0;
            while at < ml - 0.01 {
                let l = tile.min(ml - at);
                out.push((ms + at, l, src_ms, src_ml * l / tile));
                at += tile;
            }
        } else {
            out.push((ms, ml, src_ms, src_ml));
        }
    }
    if db > 0.0 {
        out.push((
            start + len - db,
            db,
            s0 + span * (px - b) / px,
            span * b / px,
        ));
    }
    out
}

/// A quad's rectangle on whole pixels, as the client draws textures that
/// snap to the pixel grid (not turned ones).
/// An image and its mipmaps down to 1x1, each level the average of the
/// one above's 2x2 texels.
fn mipmaps(img: &Rgba) -> Vec<Rgba> {
    let mut levels = vec![img.clone()];
    loop {
        let l = levels.last().unwrap();
        if l.width <= 1 && l.height <= 1 {
            break;
        }
        let (w, h) = ((l.width / 2).max(1), (l.height / 2).max(1));
        let mut pixels = Vec::with_capacity((w * h * 4) as usize);
        for y in 0..h {
            let (y0, y1) = ((y * 2).min(l.height - 1), (y * 2 + 1).min(l.height - 1));
            for x in 0..w {
                let (x0, x1) = ((x * 2).min(l.width - 1), (x * 2 + 1).min(l.width - 1));
                for k in 0..4 {
                    let at =
                        |x: u32, y: u32| u32::from(l.pixels[((y * l.width + x) * 4) as usize + k]);
                    let sum = at(x0, y0) + at(x1, y0) + at(x0, y1) + at(x1, y1);
                    pixels.push(((sum + 2) / 4) as u8);
                }
            }
        }
        levels.push(Rgba {
            width: w,
            height: h,
            pixels,
        });
    }
    levels
}

fn snapped(q: &wowlua_engine::scene::Quad, r: Rect) -> Rect {
    if !q.snap || q.rotation != 0.0 {
        return r;
    }
    let (x0, y0) = (r.x.round(), r.y.round());
    let (x1, y1) = ((r.x + r.w).round(), (r.y + r.h).round());
    Rect {
        x: x0,
        y: y0,
        w: x1 - x0,
        h: y1 - y0,
    }
}

/// A buffer holding `contents`, written through the queue (no buffer
/// mapped at creation: a browser's WebGPU can refuse those).
pub(crate) fn buffer_with(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    label: &str,
    contents: &[u8],
    usage: wgpu::BufferUsages,
) -> wgpu::Buffer {
    // (writes are whole multiples of 4 bytes)
    let size = (contents.len() as u64)
        .next_multiple_of(wgpu::COPY_BUFFER_ALIGNMENT)
        .max(4);
    let buffer = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some(label),
        size,
        usage: usage | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    });
    if contents.len() as u64 == size {
        queue.write_buffer(&buffer, 0, contents);
    } else {
        let mut padded = contents.to_vec();
        padded.resize(size as usize, 0);
        queue.write_buffer(&buffer, 0, &padded);
    }
    buffer
}

/// No assets: every image a placeholder, every font a stand-in.
pub struct NoAssets;

impl Assets for NoAssets {
    fn image(&mut self, _: &Image) -> Option<Rgba> {
        None
    }
    fn font(&mut self, _: &str) -> Option<Vec<u8>> {
        None
    }
}

/// The stand-in fonts (open licenses), for fonts not at hand.
const SANS: &[u8] = include_bytes!("../../wowlua-runtime/fonts/Figtree-Regular.ttf");
const SERIF: &[u8] = include_bytes!("../../wowlua-runtime/fonts/MarcellusSC-Regular.ttf");

#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct Vertex {
    pos: [f32; 2],
    uv: [f32; 2],
    color: [f32; 4],
    mask_uv: [f32; 2],
    flags: u32,
}

const DESATURATE: u32 = 1;
const ALPHA_KEY: u32 = 2;

#[derive(Clone, PartialEq, Eq, Hash)]
enum TexKey {
    Path(String),
    Id(u32),
    /// A model's texture, with its mipmaps.
    Model(u32),
}

pub(crate) struct Tex {
    texture: wgpu::Texture,
    view: wgpu::TextureView,
    width: u32,
    height: u32,
}

/// A run of vertices drawn together.
struct Batch {
    pipeline: usize,
    tex: usize,
    mask: usize,
    clip: Option<Rect>,
    start: u32,
    end: u32,
}

pub struct Renderer {
    /// The vertices of the part being drawn, posed and placed (T538): each
    /// with the stamp of the part that posed it.
    posed: Vec<(u32, [f32; 3], [f32; 3])>,
    posed_stamp: u32,
    /// Which vertex a model's vertex became in the batch being drawn, with
    /// the batch's stamp.
    emitted: Vec<(u32, u32)>,
    emit_stamp: u32,
    /// The world's units' vertices and indices, kept from frame to frame
    /// (their room reused), and the farther units' last poses by unit
    /// (T538).
    world_verts: Vec<model::MVertex>,
    world_indices: Vec<u32>,
    world_poses: std::collections::HashMap<String, (u32, model::Posed)>,
    device: wgpu::Device,
    queue: wgpu::Queue,
    pipelines: Vec<wgpu::RenderPipeline>,
    tex_layout: wgpu::BindGroupLayout,
    sampler: wgpu::Sampler,
    textures: Vec<Tex>,
    by_key: HashMap<TexKey, usize>,
    bind_groups: HashMap<(usize, usize), wgpu::BindGroup>,
    fonts: Vec<fontdue::Font>,
    font_ids: HashMap<String, usize>,
    atlas: glyphs::Atlas,
    /// Indexes into `textures`.
    white: usize,
    missing: usize,
    atlas_tex: usize,
    models: model::Models,
    /// The 3D world's terrain and pipeline (decision T522).
    world: world::World,
    /// The textures the frame's models are drawn into, reused each frame.
    model_slots: Vec<usize>,
    model_slot_next: usize,
    /// The adapter's name, for reports.
    pub adapter: String,
    /// The screen's size (the vertex shader's uniform) and its bind group,
    /// rewritten each draw.
    screen: wgpu::Buffer,
    screen_group: wgpu::BindGroup,
    /// The quads' vertices and the buffer's size, rewritten each draw and
    /// made again only when a scene needs more.
    quads: Option<(wgpu::Buffer, u64)>,
    /// The memory both are written through, kept between draws: a buffer
    /// made is a wait in the GPU driver (`queue.write_buffer` makes one per
    /// write), and the size of its chunks.
    belt: wgpu::util::StagingBelt,
    belt_size: u64,
}

/// The color format the renderer draws in.
pub const FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8Unorm;

const PIPELINE_BLEND: usize = 0;
const PIPELINE_ADD: usize = 1;
const PIPELINE_MOD: usize = 2;
const PIPELINE_OPAQUE: usize = 3;
/// Colors premultiplied by alpha (a model's texture).
const PIPELINE_PREMUL: usize = 4;

impl Renderer {
    /// The 3D world's props posed one by one in its last frame: those that
    /// move, near the eye (decision T558).
    pub fn moving_props(&self) -> usize {
        self.world.moving
    }

    /// The 3D world's particles drawn in its last frame (decision T559).
    pub fn world_particles(&self) -> usize {
        self.world.particles
    }

    /// The ground's clutter placed in the 3D world's last frame (decision
    /// T560).
    pub fn world_clutter(&self) -> usize {
        self.world.clutter
    }

    /// The units' blob shadows drawn in the 3D world's last frame
    /// (decision T563).
    pub fn world_blobs(&self) -> usize {
        self.world.blobs
    }

    /// The sun's shadow maps drawn in the 3D world's last frame (decision
    /// T564).
    pub fn world_sun_maps(&self) -> usize {
        self.world.sun_maps
    }

    /// The weather's drops (rain, snowflakes, sand) drawn in the 3D
    /// world's last frame (decision T565).
    pub fn world_weather_drops(&self) -> u32 {
        self.world.weather_drops
    }

    /// A renderer on the first adapter there is (a software one when there
    /// is no GPU).
    pub async fn new() -> Result<Renderer, String> {
        let instance =
            wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle_from_env());
        let mut adapter = instance
            .request_adapter(&wgpu::RequestAdapterOptions {
                power_preference: wgpu::PowerPreference::default(),
                force_fallback_adapter: false,
                compatible_surface: None,
                ..Default::default()
            })
            .await;
        if adapter.is_err() {
            adapter = instance
                .request_adapter(&wgpu::RequestAdapterOptions {
                    power_preference: wgpu::PowerPreference::default(),
                    force_fallback_adapter: true,
                    compatible_surface: None,
                    ..Default::default()
                })
                .await;
        }
        let adapter = adapter.map_err(|e| {
            format!("no GPU adapter to draw with ({e}); a software one (WARP, lavapipe) works too")
        })?;
        let name = adapter.get_info().name;
        let mut limits =
            wgpu::Limits::downlevel_webgl2_defaults().using_resolution(adapter.limits());
        // (storage buffers where the adapter has them in the vertex stage:
        // the world's units skinned on the GPU, decision T540)
        if adapter
            .get_downlevel_capabilities()
            .flags
            .contains(wgpu::DownlevelFlags::VERTEX_STORAGE)
        {
            let most = adapter.limits();
            limits.max_storage_buffers_per_shader_stage =
                most.max_storage_buffers_per_shader_stage.min(8);
            limits.max_storage_buffer_binding_size = most.max_storage_buffer_binding_size;
        }
        let (device, queue) = adapter
            .request_device(&wgpu::DeviceDescriptor {
                label: Some("wowlua"),
                required_limits: limits,
                ..Default::default()
            })
            .await
            .map_err(|e| format!("the GPU device: {e}"))?;
        Ok(Renderer::with_device(device, queue, name))
    }

    /// [`new`](Self::new), blocking (native).
    #[cfg(not(target_arch = "wasm32"))]
    pub fn new_blocking() -> Result<Renderer, String> {
        pollster::block_on(Renderer::new())
    }

    /// A renderer on the host's device (Studio's).
    pub fn with_device(device: wgpu::Device, queue: wgpu::Queue, adapter: String) -> Renderer {
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("ui"),
            source: wgpu::ShaderSource::Wgsl(include_str!("shader.wgsl").into()),
        });
        let screen_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("screen"),
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::VERTEX,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Uniform,
                    has_dynamic_offset: false,
                    min_binding_size: None,
                },
                count: None,
            }],
        });
        let tex_entry = |binding: u32| wgpu::BindGroupLayoutEntry {
            binding,
            visibility: wgpu::ShaderStages::FRAGMENT,
            ty: wgpu::BindingType::Texture {
                sample_type: wgpu::TextureSampleType::Float { filterable: true },
                view_dimension: wgpu::TextureViewDimension::D2,
                multisampled: false,
            },
            count: None,
        };
        let tex_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("textures"),
            entries: &[
                tex_entry(0),
                tex_entry(1),
                wgpu::BindGroupLayoutEntry {
                    binding: 2,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
            ],
        });
        let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("ui"),
            bind_group_layouts: &[Some(&screen_layout), Some(&tex_layout)],
            immediate_size: 0,
        });
        let blend = |b: Option<wgpu::BlendState>| {
            device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some("ui"),
                layout: Some(&layout),
                vertex: wgpu::VertexState {
                    module: &shader,
                    entry_point: Some("vs"),
                    compilation_options: Default::default(),
                    buffers: &[Some(wgpu::VertexBufferLayout {
                        array_stride: std::mem::size_of::<Vertex>() as u64,
                        step_mode: wgpu::VertexStepMode::Vertex,
                        attributes: &wgpu::vertex_attr_array![0 => Float32x2, 1 => Float32x2, 2 => Float32x4, 3 => Float32x2, 4 => Uint32],
                    })],
                },
                primitive: wgpu::PrimitiveState::default(),
                depth_stencil: None,
                multisample: wgpu::MultisampleState::default(),
                fragment: Some(wgpu::FragmentState {
                    module: &shader,
                    entry_point: Some("fs"),
                    compilation_options: Default::default(),
                    targets: &[Some(wgpu::ColorTargetState {
                        format: FORMAT,
                        blend: b,
                        write_mask: wgpu::ColorWrites::ALL,
                    })],
                }),
                multiview_mask: None,
                cache: None,
            })
        };
        use wgpu::{BlendComponent as C, BlendFactor as F, BlendOperation as O};
        let keep_alpha = C {
            src_factor: F::Zero,
            dst_factor: F::One,
            operation: O::Add,
        };
        let pipelines = vec![
            blend(Some(wgpu::BlendState {
                color: C {
                    src_factor: F::SrcAlpha,
                    dst_factor: F::OneMinusSrcAlpha,
                    operation: O::Add,
                },
                alpha: C {
                    src_factor: F::One,
                    dst_factor: F::OneMinusSrcAlpha,
                    operation: O::Add,
                },
            })),
            blend(Some(wgpu::BlendState {
                color: C {
                    src_factor: F::SrcAlpha,
                    dst_factor: F::One,
                    operation: O::Add,
                },
                alpha: keep_alpha,
            })),
            blend(Some(wgpu::BlendState {
                color: C {
                    src_factor: F::Dst,
                    dst_factor: F::Zero,
                    operation: O::Add,
                },
                alpha: keep_alpha,
            })),
            blend(None),
            blend(Some(wgpu::BlendState {
                color: C {
                    src_factor: F::One,
                    dst_factor: F::OneMinusSrcAlpha,
                    operation: O::Add,
                },
                alpha: C {
                    src_factor: F::One,
                    dst_factor: F::OneMinusSrcAlpha,
                    operation: O::Add,
                },
            })),
        ];
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("ui"),
            address_mode_u: wgpu::AddressMode::ClampToEdge,
            address_mode_v: wgpu::AddressMode::ClampToEdge,
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            ..Default::default()
        });
        let screen = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("screen"),
            size: 16,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let screen_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: None,
            layout: &screen_layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: screen.as_entire_binding(),
            }],
        });
        let mut r = Renderer {
            belt: wgpu::util::StagingBelt::new(device.clone(), 1 << 20),
            belt_size: 1 << 20,
            device,
            queue,
            pipelines,
            screen,
            screen_group,
            quads: None,
            tex_layout,
            sampler,
            textures: Vec::new(),
            by_key: HashMap::new(),
            bind_groups: HashMap::new(),
            fonts: Vec::new(),
            font_ids: HashMap::new(),
            atlas: glyphs::Atlas::default(),
            white: 0,
            posed: Vec::new(),
            posed_stamp: 0,
            emitted: Vec::new(),
            emit_stamp: 0,
            world_verts: Vec::new(),
            world_indices: Vec::new(),
            world_poses: std::collections::HashMap::new(),
            missing: 0,
            atlas_tex: 0,
            models: model::Models::default(),
            world: world::World::default(),
            model_slots: Vec::new(),
            model_slot_next: 0,
            adapter,
        };
        r.white = r.upload(&Rgba {
            width: 1,
            height: 1,
            pixels: vec![255; 4],
        });
        let mut checker = Vec::new();
        for y in 0..8 {
            for x in 0..8 {
                checker.extend_from_slice(if (x / 2 + y / 2) % 2 == 0 {
                    &[255, 0, 255, 255]
                } else {
                    &[40, 0, 40, 255]
                });
            }
        }
        r.missing = r.upload(&Rgba {
            width: 8,
            height: 8,
            pixels: checker,
        });
        r.atlas_tex = r.upload(&Rgba {
            width: glyphs::ATLAS,
            height: glyphs::ATLAS,
            pixels: vec![0; (glyphs::ATLAS * glyphs::ATLAS * 4) as usize],
        });
        for f in [SANS, SERIF] {
            if let Ok(font) = fontdue::Font::from_bytes(f, fontdue::FontSettings::default()) {
                r.fonts.push(font);
            }
        }
        r
    }

    fn upload(&mut self, img: &Rgba) -> usize {
        self.upload_levels(&[img])
    }

    /// Uploads an image with its mipmaps, as the client draws models'
    /// textures (seen small, they average their texels).
    fn upload_mipmapped(&mut self, img: &Rgba) -> usize {
        let levels = mipmaps(img);
        self.upload_levels(&levels.iter().collect::<Vec<_>>())
    }

    /// Uploads an image's levels (the first the image).
    fn upload_levels(&mut self, levels: &[&Rgba]) -> usize {
        let img = levels[0];
        let size = wgpu::Extent3d {
            width: img.width.max(1),
            height: img.height.max(1),
            depth_or_array_layers: 1,
        };
        let texture = self.device.create_texture(&wgpu::TextureDescriptor {
            label: None,
            size,
            mip_level_count: levels.len() as u32,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: FORMAT,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });
        for (level, l) in levels.iter().enumerate() {
            let (w, h) = (l.width.max(1), l.height.max(1));
            self.queue.write_texture(
                wgpu::TexelCopyTextureInfo {
                    texture: &texture,
                    mip_level: level as u32,
                    origin: wgpu::Origin3d::ZERO,
                    aspect: wgpu::TextureAspect::All,
                },
                &l.pixels,
                wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(4 * w),
                    rows_per_image: Some(h),
                },
                wgpu::Extent3d {
                    width: w,
                    height: h,
                    depth_or_array_layers: 1,
                },
            );
        }
        let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
        self.textures.push(Tex {
            texture,
            view,
            width: size.width,
            height: size.height,
        });
        self.textures.len() - 1
    }

    /// The texture of an image: uploaded once (the placeholder when it
    /// cannot be read).
    fn texture(&mut self, image: &Image, assets: &mut dyn Assets) -> usize {
        let key = match image {
            Image::Color => return self.white,
            Image::File(p) => TexKey::Path(p.replace('/', "\\").to_ascii_lowercase()),
            Image::FileId(id) => TexKey::Id(*id),
            // The unit's model, else the flat stand-in.
            Image::Portrait { source, fallback } => {
                return match self.portrait(source, assets) {
                    Some(t) => t,
                    None => self.texture(&Image::File(fallback.clone()), assets),
                };
            }
        };
        if let Some(&t) = self.by_key.get(&key) {
            return t;
        }
        let max = self.device.limits().max_texture_dimension_2d;
        let t = match assets.image(image) {
            Some(img) if img.width <= max && img.height <= max => self.upload(&img),
            _ => self.missing,
        };
        self.by_key.insert(key, t);
        t
    }

    /// A model's texture (a file ID), with its mipmaps; uploaded once.
    fn model_texture(&mut self, file: u32, assets: &mut dyn Assets) -> usize {
        let key = TexKey::Model(file);
        if let Some(&t) = self.by_key.get(&key) {
            return t;
        }
        let max = self.device.limits().max_texture_dimension_2d;
        let t = match assets.image(&Image::FileId(file)) {
            Some(img) if img.width <= max && img.height <= max => self.upload_mipmapped(&img),
            _ => self.missing,
        };
        self.by_key.insert(key, t);
        t
    }

    /// The font of a path: the file's, else a stand-in.
    fn font(&mut self, path: &str, assets: &mut dyn Assets) -> usize {
        let key = path.replace('/', "\\").to_ascii_lowercase();
        if let Some(&f) = self.font_ids.get(&key) {
            return f;
        }
        let loaded = assets
            .font(path)
            .and_then(|b| fontdue::Font::from_bytes(b, fontdue::FontSettings::default()).ok());
        let id = match loaded {
            Some(f) => {
                self.fonts.push(f);
                self.fonts.len() - 1
            }
            None => {
                let serif = key.contains("morpheus") || key.contains("skurri");
                usize::from(serif && self.fonts.len() > 1)
            }
        };
        self.font_ids.insert(key, id);
        id
    }

    fn bind_group(&mut self, tex: usize, mask: usize) -> &wgpu::BindGroup {
        let (device, layout, sampler, textures) = (
            &self.device,
            &self.tex_layout,
            &self.sampler,
            &self.textures,
        );
        self.bind_groups.entry((tex, mask)).or_insert_with(|| {
            device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: None,
                layout,
                entries: &[
                    wgpu::BindGroupEntry {
                        binding: 0,
                        resource: wgpu::BindingResource::TextureView(&textures[tex].view),
                    },
                    wgpu::BindGroupEntry {
                        binding: 1,
                        resource: wgpu::BindingResource::TextureView(&textures[mask].view),
                    },
                    wgpu::BindGroupEntry {
                        binding: 2,
                        resource: wgpu::BindingResource::Sampler(sampler),
                    },
                ],
            })
        })
    }

    /// The scene's vertices and batches, at `scale`.
    fn build(
        &mut self,
        scene: &Scene,
        scale: f32,
        assets: &mut dyn Assets,
    ) -> (Vec<Vertex>, Vec<Batch>) {
        let mut verts: Vec<Vertex> = Vec::new();
        let mut batches: Vec<Batch> = Vec::new();
        let push = |verts: &mut Vec<Vertex>,
                    batches: &mut Vec<Batch>,
                    pipeline: usize,
                    tex: usize,
                    mask: usize,
                    clip: Option<Rect>,
                    quad: [Vertex; 4]| {
            let start = verts.len() as u32;
            verts.extend_from_slice(&[quad[0], quad[1], quad[2], quad[2], quad[1], quad[3]]);
            let end = verts.len() as u32;
            match batches.last_mut() {
                Some(b)
                    if b.pipeline == pipeline
                        && b.tex == tex
                        && b.mask == mask
                        && b.clip == clip
                        && b.end == start =>
                {
                    b.end = end
                }
                _ => batches.push(Batch {
                    pipeline,
                    tex,
                    mask,
                    clip,
                    start,
                    end,
                }),
            }
        };
        let s = |r: Rect| Rect {
            x: r.x * scale,
            y: r.y * scale,
            w: r.w * scale,
            h: r.h * scale,
        };
        self.model_slot_next = 0;
        for item in &scene.items {
            match item {
                Item::Model(m) => {
                    let Some(tex) = self.draw_model(m, scale, assets) else {
                        continue;
                    };
                    let r = s(m.rect);
                    let c = [m.alpha; 4];
                    let mk = |pos: [f32; 2], uv: [f32; 2]| Vertex {
                        pos,
                        uv,
                        color: c,
                        mask_uv: [0.5, 0.5],
                        flags: 0,
                    };
                    let quad = [
                        mk([r.x, r.y], [0.0, 0.0]),
                        mk([r.x, r.y + r.h], [0.0, 1.0]),
                        mk([r.x + r.w, r.y], [1.0, 0.0]),
                        mk([r.x + r.w, r.y + r.h], [1.0, 1.0]),
                    ];
                    push(
                        &mut verts,
                        &mut batches,
                        PIPELINE_PREMUL,
                        tex,
                        self.white,
                        m.clip.map(s),
                        quad,
                    );
                }
                Item::Quad(q) => {
                    // The build's atlas, where the assets know it.
                    let own = q.atlas.as_deref().and_then(|n| assets.atlas(n));
                    let (image, tc) = match &own {
                        Some(a_) => {
                            let [l, r, t, b] = a_.coords;
                            let a = q.atlas_texcoord;
                            let mut tc = [0.0f32; 8];
                            for k in 0..4 {
                                tc[k * 2] = l + a[k * 2] * (r - l);
                                tc[k * 2 + 1] = t + a[k * 2 + 1] * (b - t);
                            }
                            (&a_.image, tc)
                        }
                        None => (&q.image, q.texcoord),
                    };
                    // A sliced or tiling atlas drawn whole (not turned, not
                    // flipped): its pieces.
                    let pieces: Option<Vec<(Rect, [f32; 4])>> = own.as_ref().and_then(|a_| {
                        let plain = q.rotation == 0.0
                            && q.atlas_texcoord == [0.0, 0.0, 0.0, 1.0, 1.0, 0.0, 1.0, 1.0];
                        if !plain || (a_.slice.is_none() && !a_.tiles.0 && !a_.tiles.1) {
                            return None;
                        }
                        let r = snapped(q, s(q.rect));
                        let unit = q.unit * scale;
                        let [l, rr, t, b] = a_.coords;
                        let (pw, ph) = (a_.size.0.max(1) as f32, a_.size.1.max(1) as f32);
                        let (m, mode) = match a_.slice {
                            Some((m, mode)) => (Some(m.map(|v| v as f32)), mode),
                            None => (None, 0),
                        };
                        let xs = axis_pieces(
                            r.x,
                            r.w,
                            (l, rr),
                            pw,
                            m.map(|m| (m[0], m[2])),
                            if m.is_some() { mode == 1 } else { a_.tiles.0 },
                            unit,
                        );
                        let ys = axis_pieces(
                            r.y,
                            r.h,
                            (t, b),
                            ph,
                            m.map(|m| (m[1], m[3])),
                            if m.is_some() { mode == 1 } else { a_.tiles.1 },
                            unit,
                        );
                        let mut out = Vec::with_capacity(xs.len() * ys.len());
                        for &(y, h, v0, vl) in &ys {
                            for &(x, w, u0, ul) in &xs {
                                out.push((Rect { x, y, w, h }, [u0, u0 + ul, v0, v0 + vl]));
                            }
                        }
                        Some(out)
                    });
                    let tex = self.texture(image, assets);
                    // The mask's image and its region (the build's atlas's
                    // where the assets know it).
                    let (mask, mask_rect, region) = match q.masks.first() {
                        Some(m) => {
                            let own = m.atlas.as_deref().and_then(|n| assets.atlas(n));
                            let (image, region) = match &own {
                                Some(a) => (&a.image, a.coords),
                                None => (&m.image, m.region),
                            };
                            (self.texture(image, assets), Some(s(m.rect)), region)
                        }
                        None => (self.white, None, [0.0, 1.0, 0.0, 1.0]),
                    };
                    let r = snapped(q, s(q.rect));
                    let (cx, cy) = (r.x + r.w / 2.0, r.y + r.h / 2.0);
                    let (sin, cos) = (-q.rotation).sin_cos();
                    let corner = |x: f32, y: f32| -> [f32; 2] {
                        let (dx, dy) = (x - cx, y - cy);
                        [cx + dx * cos - dy * sin, cy + dx * sin + dy * cos]
                    };
                    let flags = (if q.desaturated { DESATURATE } else { 0 })
                        | (if q.blend == Blend::AlphaKey {
                            ALPHA_KEY
                        } else {
                            0
                        });
                    // A gradient: each vertex's color by its place across
                    // the rectangle (the minimum's at left or bottom).
                    let shade = |pos: [f32; 2]| -> [f32; 4] {
                        let Some((horizontal, min, max)) = q.gradient else {
                            return q.color;
                        };
                        let t = if horizontal {
                            if r.w > 0.0 {
                                ((pos[0] - r.x) / r.w).clamp(0.0, 1.0)
                            } else {
                                0.0
                            }
                        } else if r.h > 0.0 {
                            ((r.y + r.h - pos[1]) / r.h).clamp(0.0, 1.0)
                        } else {
                            0.0
                        };
                        let mut c = q.color;
                        for k in 0..4 {
                            c[k] *= min[k] + (max[k] - min[k]) * t;
                        }
                        c
                    };
                    let mk = |pos: [f32; 2], uv: [f32; 2]| Vertex {
                        pos,
                        uv,
                        color: shade(pos),
                        mask_uv: match mask_rect {
                            Some(m) if m.w > 0.0 && m.h > 0.0 => {
                                let (u, v) = ((pos[0] - m.x) / m.w, (pos[1] - m.y) / m.h);
                                [
                                    region[0] + u * (region[1] - region[0]),
                                    region[2] + v * (region[3] - region[2]),
                                ]
                            }
                            _ => [0.5, 0.5],
                        },
                        flags,
                    };
                    let pipeline = match q.blend {
                        Blend::Blend => PIPELINE_BLEND,
                        Blend::Add => PIPELINE_ADD,
                        Blend::Mod => PIPELINE_MOD,
                        Blend::Disable | Blend::AlphaKey => PIPELINE_OPAQUE,
                    };
                    let quads: Vec<[Vertex; 4]> = match &pieces {
                        Some(pieces) => pieces
                            .iter()
                            .map(|(p, [u0, u1, v0, v1])| {
                                [
                                    mk([p.x, p.y], [*u0, *v0]),
                                    mk([p.x, p.y + p.h], [*u0, *v1]),
                                    mk([p.x + p.w, p.y], [*u1, *v0]),
                                    mk([p.x + p.w, p.y + p.h], [*u1, *v1]),
                                ]
                            })
                            .collect(),
                        None => vec![[
                            mk(corner(r.x, r.y), [tc[0], tc[1]]),
                            mk(corner(r.x, r.y + r.h), [tc[2], tc[3]]),
                            mk(corner(r.x + r.w, r.y), [tc[4], tc[5]]),
                            mk(corner(r.x + r.w, r.y + r.h), [tc[6], tc[7]]),
                        ]],
                    };
                    for quad in quads {
                        push(
                            &mut verts,
                            &mut batches,
                            pipeline,
                            tex,
                            mask,
                            q.clip.map(s),
                            quad,
                        );
                    }
                }
                Item::Text(t) => {
                    let font = self.font(&t.font, assets);
                    let px = (t.px * scale).max(1.0);
                    // The baseline: the font's height shared between its
                    // ascent and descent (the default UI's page number sits
                    // between its arrows so, as in game).
                    let ascent =
                        self.fonts[font]
                            .horizontal_line_metrics(px)
                            .map_or(px * 0.8, |m| {
                                let span = m.ascent - m.descent;
                                if span > 0.0 {
                                    px * m.ascent / span
                                } else {
                                    m.ascent
                                }
                            });
                    let line_h = t.line_height * scale;
                    // Shadow, outline, then the text.
                    let mut passes: Vec<([f32; 4], f32, f32, bool)> = Vec::new();
                    if let Some((c, dx, dy)) = t.shadow {
                        passes.push((c, dx * scale, dy * scale, true));
                    }
                    if t.outline > 0 {
                        let o = f32::from(t.outline) * scale.max(1.0);
                        for (dx, dy) in [
                            (-o, 0.0),
                            (o, 0.0),
                            (0.0, -o),
                            (0.0, o),
                            (-o, -o),
                            (o, -o),
                            (-o, o),
                            (o, o),
                        ] {
                            passes.push(([0.0, 0.0, 0.0, 1.0], dx, dy, true));
                        }
                    }
                    passes.push(([1.0; 4], 0.0, 0.0, false));
                    for (pc, dx, dy, fixed) in passes {
                        for line in &t.lines {
                            let baseline = line.y * scale + (line_h - px) / 2.0 + ascent;
                            for run in &line.runs {
                                let color = if fixed {
                                    [pc[0], pc[1], pc[2], pc[3] * run.color[3]]
                                } else {
                                    run.color
                                };
                                for (i, ch) in run.text.chars().enumerate() {
                                    let Some(g) = self.atlas.glyph(&self.fonts, font, px, ch)
                                    else {
                                        continue;
                                    };
                                    let x = (line.x * scale
                                        + run.xs.get(i).copied().unwrap_or(0.0) * scale
                                        + g.left
                                        + dx)
                                        .round();
                                    let y = (baseline - g.top + dy).round();
                                    let a = glyphs::ATLAS as f32;
                                    let (u0, v0, u1, v1) = (
                                        g.x as f32 / a,
                                        g.y as f32 / a,
                                        (g.x + g.w) as f32 / a,
                                        (g.y + g.h) as f32 / a,
                                    );
                                    let (w, h) = (g.w as f32, g.h as f32);
                                    let mk = |pos: [f32; 2], uv: [f32; 2]| Vertex {
                                        pos,
                                        uv,
                                        color,
                                        mask_uv: [0.5, 0.5],
                                        flags: 0,
                                    };
                                    let quad = [
                                        mk([x, y], [u0, v0]),
                                        mk([x, y + h], [u0, v1]),
                                        mk([x + w, y], [u1, v0]),
                                        mk([x + w, y + h], [u1, v1]),
                                    ];
                                    push(
                                        &mut verts,
                                        &mut batches,
                                        PIPELINE_BLEND,
                                        self.atlas_tex,
                                        self.white,
                                        t.clip.map(s),
                                        quad,
                                    );
                                }
                            }
                        }
                    }
                }
            }
        }
        (verts, batches)
    }

    fn upload_atlas(&mut self) {
        let Some((y0, y1)) = self.atlas.dirty.take() else {
            return;
        };
        let a = glyphs::ATLAS;
        let rows = &self.atlas.pixels[(y0 * a * 4) as usize..(y1 * a * 4) as usize];
        self.queue.write_texture(
            wgpu::TexelCopyTextureInfo {
                texture: &self.textures[self.atlas_tex].texture,
                mip_level: 0,
                origin: wgpu::Origin3d { x: 0, y: y0, z: 0 },
                aspect: wgpu::TextureAspect::All,
            },
            rows,
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(4 * a),
                rows_per_image: Some(y1 - y0),
            },
            wgpu::Extent3d {
                width: a,
                height: y1 - y0,
                depth_or_array_layers: 1,
            },
        );
    }

    /// Draws the scene into `view` (`width` × `height` pixels, the
    /// renderer's [`FORMAT`]) at `scale`, cleared to `clear` first. The
    /// encoder is submitted before the next draw.
    pub fn draw(
        &mut self,
        scene: &Scene,
        view: &wgpu::TextureView,
        width: u32,
        height: u32,
        scale: f32,
        assets: &mut dyn Assets,
    ) -> wgpu::CommandEncoder {
        let (verts, batches) = self.build(scene, scale, assets);
        self.upload_atlas();
        for b in &batches {
            self.bind_group(b.tex, b.mask);
        }
        // One chunk holds a draw's writes: the belt keeps every chunk it
        // made, so a larger scene makes the belt again rather than add one.
        let upload = (std::mem::size_of_val(&verts[..]) as u64 + 64)
            .next_power_of_two()
            .max(1 << 20);
        if upload > self.belt_size {
            self.belt = wgpu::util::StagingBelt::new(self.device.clone(), upload);
            self.belt_size = upload;
        }
        // The previous draw's encoder is submitted: its memory is free again.
        self.belt.recall();
        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor { label: Some("ui") });
        // The 3D world first, while it is on: the UI over it (T522).
        let world = scene.world.as_ref().inspect(|w| {
            self.draw_world(&mut encoder, view, width, height, w, assets);
        });
        let screen: [f32; 4] = [width as f32, height as f32, 0.0, 0.0];
        self.belt
            .write_buffer(
                &mut encoder,
                &self.screen,
                0,
                wgpu::BufferSize::new(16).expect("the screen's size"),
            )
            .copy_from_slice(bytemuck::cast_slice(&screen));
        let screen_group = &self.screen_group;
        let vbuf = (!verts.is_empty()).then(|| {
            let bytes: &[u8] = bytemuck::cast_slice(&verts);
            let len = (bytes.len() as u64).next_multiple_of(wgpu::COPY_BUFFER_ALIGNMENT);
            if self.quads.as_ref().is_none_or(|(_, size)| *size < len) {
                // Room to grow: a larger scene makes it again, seldom.
                let size = len.next_power_of_two().max(64 * 1024);
                let buffer = self.device.create_buffer(&wgpu::BufferDescriptor {
                    label: Some("quads"),
                    size,
                    usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
                    mapped_at_creation: false,
                });
                self.quads = Some((buffer, size));
            }
            let (buffer, _) = self.quads.as_ref().expect("the quads' buffer");
            let mut view = self.belt.write_buffer(
                &mut encoder,
                buffer,
                0,
                wgpu::BufferSize::new(len).expect("a quad's vertices"),
            );
            view.slice(..bytes.len()).copy_from_slice(bytes);
            let pad = len as usize - bytes.len();
            view.slice(bytes.len()..).copy_from_slice(&[0; 4][..pad]);
            buffer.slice(..len)
        });
        self.belt.finish();
        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("ui"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view,
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: match world {
                            Some(_) => wgpu::LoadOp::Load,
                            None => wgpu::LoadOp::Clear(wgpu::Color::BLACK),
                        },
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
            if let Some(vbuf) = vbuf {
                pass.set_vertex_buffer(0, vbuf);
                pass.set_bind_group(0, screen_group, &[]);
                for b in &batches {
                    let (x, y, w, h) = match b.clip {
                        Some(c) => {
                            let x0 = c.x.max(0.0).min(width as f32);
                            let y0 = c.y.max(0.0).min(height as f32);
                            let x1 = (c.x + c.w).max(0.0).min(width as f32);
                            let y1 = (c.y + c.h).max(0.0).min(height as f32);
                            (x0 as u32, y0 as u32, (x1 - x0) as u32, (y1 - y0) as u32)
                        }
                        None => (0, 0, width, height),
                    };
                    if w == 0 || h == 0 {
                        continue;
                    }
                    pass.set_scissor_rect(x, y, w, h);
                    pass.set_pipeline(&self.pipelines[b.pipeline]);
                    pass.set_bind_group(1, &self.bind_groups[&(b.tex, b.mask)], &[]);
                    pass.draw(b.start..b.end, 0..1);
                }
            }
        }
        encoder
    }

    /// The scene as an image (`scale` times the screen's size), the part
    /// `crop` (screen pixels) gives, or all of it.
    pub async fn render(
        &mut self,
        scene: &Scene,
        scale: f32,
        crop: Option<Rect>,
        assets: &mut dyn Assets,
    ) -> Result<Rgba, String> {
        let width = ((scene.width as f32 * scale).round() as u32).max(1);
        let height = ((scene.height as f32 * scale).round() as u32).max(1);
        let max = self.device.limits().max_texture_dimension_2d;
        if width > max || height > max {
            return Err(format!(
                "{width}x{height} is larger than the GPU draws ({max})"
            ));
        }
        let target = self.device.create_texture(&wgpu::TextureDescriptor {
            label: Some("screenshot"),
            size: wgpu::Extent3d {
                width,
                height,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: FORMAT,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
            view_formats: &[],
        });
        let view = target.create_view(&wgpu::TextureViewDescriptor::default());
        let mut encoder = self.draw(scene, &view, width, height, scale, assets);
        let row = (4 * width).div_ceil(wgpu::COPY_BYTES_PER_ROW_ALIGNMENT)
            * wgpu::COPY_BYTES_PER_ROW_ALIGNMENT;
        let buffer = self.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("readback"),
            size: u64::from(row * height),
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });
        encoder.copy_texture_to_buffer(
            wgpu::TexelCopyTextureInfo {
                texture: &target,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            wgpu::TexelCopyBufferInfo {
                buffer: &buffer,
                layout: wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(row),
                    rows_per_image: Some(height),
                },
            },
            wgpu::Extent3d {
                width,
                height,
                depth_or_array_layers: 1,
            },
        );
        self.queue.submit([encoder.finish()]);
        let slice = buffer.slice(..);
        let mapped = Mapped::default();
        let done = mapped.clone();
        slice.map_async(wgpu::MapMode::Read, move |r| done.finish(r));
        // Natively the poll waits for the GPU; in the browser the event
        // loop calls back, and the future wakes.
        let _ = self.device.poll(wgpu::PollType::wait_indefinitely());
        mapped
            .await
            .map_err(|e| format!("reading the screenshot back: {e}"))?;
        let data = slice
            .get_mapped_range()
            .map_err(|e| format!("reading the screenshot back: {e}"))?;
        // The part asked for, opaque (the world behind the UI is black).
        let (cx, cy, cw, ch) = match crop {
            Some(c) => {
                let x0 = ((c.x * scale).floor().max(0.0) as u32).min(width);
                let y0 = ((c.y * scale).floor().max(0.0) as u32).min(height);
                let x1 = (((c.x + c.w) * scale).ceil().max(0.0) as u32).min(width);
                let y1 = (((c.y + c.h) * scale).ceil().max(0.0) as u32).min(height);
                (x0, y0, (x1 - x0).max(1), (y1 - y0).max(1))
            }
            None => (0, 0, width, height),
        };
        let mut pixels = Vec::with_capacity((cw * ch * 4) as usize);
        for y in cy..(cy + ch).min(height) {
            let start = (y * row + cx * 4) as usize;
            let end = start + ((cw.min(width - cx)) * 4) as usize;
            pixels.extend_from_slice(&data[start..end]);
        }
        drop(data);
        buffer.unmap();
        for p in pixels.chunks_mut(4) {
            p[3] = 255;
        }
        Ok(Rgba {
            width: cw,
            height: ch,
            pixels,
        })
    }

    /// [`render`](Self::render), blocking (native).
    #[cfg(not(target_arch = "wasm32"))]
    pub fn render_blocking(
        &mut self,
        scene: &Scene,
        scale: f32,
        crop: Option<Rect>,
        assets: &mut dyn Assets,
    ) -> Result<Rgba, String> {
        pollster::block_on(self.render(scene, scale, crop, assets))
    }

    /// Textures uploaded so far (placeholders included).
    pub fn texture_count(&self) -> usize {
        self.textures.len()
    }

    /// The size of a texture uploaded for an image, if it was.
    pub fn texture_size(&self, image: &Image) -> Option<(u32, u32)> {
        let key = match image {
            Image::Color => return Some((1, 1)),
            Image::File(p) => TexKey::Path(p.replace('/', "\\").to_ascii_lowercase()),
            Image::FileId(id) => TexKey::Id(*id),
            Image::Portrait { .. } => return Some((128, 128)),
        };
        self.by_key
            .get(&key)
            .map(|&t| (self.textures[t].width, self.textures[t].height))
    }
}

/// A buffer's mapping finishing: a future the map callback completes.
#[derive(Clone, Default)]
struct Mapped(std::sync::Arc<std::sync::Mutex<MappedState>>);

#[derive(Default)]
struct MappedState {
    result: Option<Result<(), wgpu::BufferAsyncError>>,
    waker: Option<std::task::Waker>,
}

impl Mapped {
    fn finish(&self, r: Result<(), wgpu::BufferAsyncError>) {
        if let Ok(mut s) = self.0.lock() {
            s.result = Some(r);
            if let Some(w) = s.waker.take() {
                w.wake();
            }
        }
    }
}

impl std::future::Future for Mapped {
    type Output = Result<(), wgpu::BufferAsyncError>;

    fn poll(
        self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Self::Output> {
        let Ok(mut s) = self.0.lock() else {
            return std::task::Poll::Ready(Err(wgpu::BufferAsyncError));
        };
        match s.result.take() {
            Some(r) => std::task::Poll::Ready(r),
            None => {
                s.waker = Some(cx.waker().clone());
                std::task::Poll::Pending
            }
        }
    }
}

/// An image as PNG.
pub fn encode_png(img: &Rgba) -> Result<Vec<u8>, String> {
    let mut out = Vec::new();
    {
        let mut enc = png::Encoder::new(&mut out, img.width, img.height);
        enc.set_color(png::ColorType::Rgba);
        enc.set_depth(png::BitDepth::Eight);
        let mut w = enc.write_header().map_err(|e| e.to_string())?;
        w.write_image_data(&img.pixels).map_err(|e| e.to_string())?;
    }
    Ok(out)
}

/// A PNG's pixels, as RGBA.
pub fn decode_png(bytes: &[u8]) -> Result<Rgba, String> {
    let mut dec = png::Decoder::new(std::io::Cursor::new(bytes));
    dec.set_transformations(png::Transformations::EXPAND | png::Transformations::ALPHA);
    let mut reader = dec.read_info().map_err(|e| e.to_string())?;
    let mut buf = vec![0; reader.output_buffer_size().unwrap_or(0)];
    let info = reader.next_frame(&mut buf).map_err(|e| e.to_string())?;
    buf.truncate(info.buffer_size());
    let pixels = match info.color_type {
        png::ColorType::Rgba => buf,
        png::ColorType::GrayscaleAlpha => buf
            .chunks(2)
            .flat_map(|p| [p[0], p[0], p[0], p[1]])
            .collect(),
        _ => return Err("an RGBA or gray PNG".into()),
    };
    Ok(Rgba {
        width: info.width,
        height: info.height,
        pixels,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Each level halves the one above (odd sizes round down, never below
    /// one) and averages its 2x2 texels.
    #[test]
    fn mipmaps_average_down_to_one_texel() {
        let img = Rgba {
            width: 4,
            height: 2,
            pixels: [
                [0, 0, 0, 255],
                [255, 255, 255, 255],
                [0, 0, 0, 0],
                [0, 0, 0, 0],
                [0, 0, 0, 255],
                [255, 255, 255, 255],
                [0, 0, 0, 0],
                [0, 0, 0, 0],
            ]
            .concat(),
        };
        let levels = mipmaps(&img);
        let sizes: Vec<(u32, u32)> = levels.iter().map(|l| (l.width, l.height)).collect();
        assert_eq!(sizes, [(4, 2), (2, 1), (1, 1)]);
        assert_eq!(levels[1].pixels, [128, 128, 128, 255, 0, 0, 0, 0]);
        assert_eq!(levels[2].pixels, [64, 64, 64, 128]);
    }
}
