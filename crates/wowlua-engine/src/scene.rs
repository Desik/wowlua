//! The scene (spec debugger §18.9, decision T309): what the screen shows,
//! as a draw list in the client's order, built from the engine's state on
//! request. Coordinates are screen pixels from the top left.

use wowlua_vm::{Value, Vm};

use crate::engine;
use crate::widgets::{self, WidgetId, stored_ref};

/// A rectangle in screen pixels: left, top, width, height.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Rect {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
}

impl Rect {
    pub fn intersect(self, o: Rect) -> Rect {
        let x0 = self.x.max(o.x);
        let y0 = self.y.max(o.y);
        let x1 = (self.x + self.w).min(o.x + o.w);
        let y1 = (self.y + self.h).min(o.y + o.h);
        Rect {
            x: x0,
            y: y0,
            w: (x1 - x0).max(0.0),
            h: (y1 - y0).max(0.0),
        }
    }

    pub fn contains(self, px: f32, py: f32) -> bool {
        px >= self.x && py >= self.y && px < self.x + self.w && py < self.y + self.h
    }
}

/// What a texture shows.
#[derive(Clone, Debug, PartialEq)]
pub enum Image {
    /// A file by path (`Interface\Buttons\WHITE8X8`), as given.
    File(String),
    /// A file by its ID.
    FileId(u32),
    /// A plain color (`SetColorTexture`): the item's color is it.
    Color,
    /// A unit's portrait (`SetPortraitTexture`): its model through its
    /// portrait camera, else the flat stand-in file.
    Portrait {
        source: Box<ModelSource>,
        fallback: String,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Blend {
    Blend,
    Add,
    Mod,
    AlphaKey,
    Disable,
}

/// A mask on a texture: its image, rectangle, and its part of the image
/// (left, right, top, bottom; an atlas's region), with the atlas's name
/// for a renderer with the build's own atlases.
#[derive(Clone, Debug, PartialEq)]
pub struct Mask {
    pub image: Image,
    pub rect: Rect,
    pub region: [f32; 4],
    pub atlas: Option<String>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Quad {
    pub widget: usize,
    pub rect: Rect,
    pub image: Image,
    /// Texture coordinates of the corners: top left, bottom left, top
    /// right, bottom right (x, y each), as `SetTexCoord`'s eight values.
    pub texcoord: [f32; 8],
    /// The atlas set (`SetAtlas`), and the corners' coordinates within
    /// its region: a renderer with the build's own atlases places them.
    pub atlas: Option<String>,
    pub atlas_texcoord: [f32; 8],
    /// Vertex color times the effective alpha (and a color texture's
    /// color).
    pub color: [f32; 4],
    pub blend: Blend,
    pub desaturated: bool,
    /// Radians, about the rectangle's center.
    pub rotation: f32,
    pub tile: (bool, bool),
    pub clip: Option<Rect>,
    pub masks: Vec<Mask>,
    /// Screen pixels per UI unit of the region (its effective scale): a
    /// sliced or tiled atlas's pixels are drawn this size.
    pub unit: f32,
    /// Its edges on whole pixels (`SetSnapToPixelGrid`, on by default).
    pub snap: bool,
    /// Its color's gradient (`SetGradient`): horizontal (else vertical),
    /// the minimum's color (left, bottom) and the maximum's (right, top),
    /// times `color`.
    pub gradient: Option<(bool, [f32; 4], [f32; 4])>,
}

/// A run of characters of one color.
#[derive(Clone, Debug, PartialEq)]
pub struct Run {
    pub text: String,
    pub color: [f32; 4],
    /// Each character's left edge, pixels from the line's start.
    pub xs: Vec<f32>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct TextLine {
    /// Where the line starts (its left edge and top).
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub runs: Vec<Run>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Text {
    pub widget: usize,
    /// The textures and atlases inline in it (`|T…|t`, `|A…|a`), drawn
    /// over it where they stand.
    pub images: Vec<Quad>,
    pub rect: Rect,
    /// The font file as set (`Fonts\FRIZQT__.TTF`).
    pub font: String,
    /// The font's pixel size.
    pub px: f32,
    pub line_height: f32,
    pub outline: u8,
    pub monochrome: bool,
    pub shadow: Option<([f32; 4], f32, f32)>,
    pub lines: Vec<TextLine>,
    pub clip: Option<Rect>,
}

/// What a model shows (spec debugger §18.9.7).
#[derive(Clone, Debug, PartialEq)]
pub enum ModelSource {
    File(u32),
    Path(String),
    /// A creature display (`CreatureDisplayInfo`).
    Display(u32),
    /// A creature (`Creature`): its first display.
    Creature(u32),
    /// A character as a unit looks.
    Character(Look),
    /// An item's own model (`SetItem`).
    Item(u32),
    /// An item appearance's model (`SetItemAppearance`).
    Appearance(u32),
}

/// How a character looks: race, sex (0 male, 1 female), class, and what it
/// wears (inventory slot, item ID); items tried on (links or appearance
/// IDs) over that.
#[derive(Clone, Debug, PartialEq, Default)]
pub struct Look {
    pub race: u32,
    pub sex: u32,
    pub class: u32,
    pub items: Vec<(u32, u32)>,
    pub tried: Vec<String>,
}

/// A model placed in its frame or scene.
#[derive(Clone, Debug, PartialEq)]
pub struct Actor {
    pub source: ModelSource,
    pub position: [f32; 3],
    /// Radians: about the up axis (a frame's facing), then pitch, roll.
    pub yaw: f32,
    pub pitch: f32,
    pub roll: f32,
    /// Whether pitch and roll turn about the view's fixed axes after the
    /// facing (a model frame's: probed in WoW Forever, 2026-10-23), not
    /// about the model's own turned axes (a scene actor's, unverified).
    pub fixed_axes: bool,
    pub scale: f32,
    pub alpha: f32,
    /// The animation (`AnimationData` ID, 0 standing) and its variation.
    pub animation: u32,
    pub variation: u32,
}

/// How a model is looked at.
#[derive(Clone, Debug, PartialEq)]
pub enum ModelCamera {
    /// A model frame's: the whole model in view, toward its portrait
    /// camera by `portrait_zoom` (0 to 1), farther by `distance_scale`; a
    /// camera of the model's own when `SetCamera` chose one.
    Frame {
        portrait_zoom: f32,
        distance_scale: f32,
        camera: Option<i32>,
    },
    /// A model scene's: where it is, its axes, field of view (radians,
    /// vertical) and clip planes.
    Scene {
        position: [f32; 3],
        forward: [f32; 3],
        right: [f32; 3],
        up: [f32; 3],
        fov: f32,
        near: f32,
        far: f32,
    },
}

/// A model frame or scene's light.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Light {
    pub enabled: bool,
    /// The direction the light shines in, in the model's space.
    pub direction: [f32; 3],
    pub ambient: [f32; 3],
    pub diffuse: [f32; 3],
}

/// A model frame or scene: what it draws into its rectangle.
#[derive(Clone, Debug, PartialEq)]
pub struct Model {
    pub widget: usize,
    pub rect: Rect,
    pub clip: Option<Rect>,
    pub alpha: f32,
    pub actors: Vec<Actor>,
    pub camera: ModelCamera,
    pub light: Light,
    /// Seconds since the session began (the animations' clock).
    pub time: f64,
}

/// A drawn thing. Quads and texts are shared: a frame's are kept from
/// frame to frame while nothing they show changes (decision T542).
#[derive(Clone, Debug, PartialEq)]
pub enum Item {
    Quad(std::sync::Arc<Quad>),
    Text(std::sync::Arc<Text>),
    Model(Model),
}

impl Item {
    pub fn quad(q: Quad) -> Item {
        Item::Quad(std::sync::Arc::new(q))
    }

    pub fn text(t: Text) -> Item {
        Item::Text(std::sync::Arc::new(t))
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Scene {
    pub width: u32,
    pub height: u32,
    pub items: Vec<Item>,
    /// The 3D world behind the UI, while it is on (decision T522).
    pub world: Option<WorldView>,
    /// The items that stand over units as the game's camera sees them: the
    /// nameplates' and the names over units (decision T546), by their
    /// places in `items`. A view of the world from another camera leaves
    /// them out.
    pub over_units: Vec<std::ops::Range<usize>>,
}

/// The 3D world as the camera sees it (decision T522): the map, the eye
/// and its axes (north, west and up yards), the horizontal field of view
/// (radians) and how far the world is drawn (yards, `farclip`); within
/// `detail` yards the terrain in full (`terrainLodDist`), beyond it its
/// tiles' minimap pictures; within `objects` yards the buildings and
/// doodads (`horizonStart`), a doodad while it looks at least `cull`
/// pixels big (`lodObjectCullSize`; decision T525). It is drawn with
/// `render_scale` of the screen's pixels each way (`RenderScale`, 0.33 to
/// 2) and stretched to the screen, the UI over it in full (decision T539).
#[derive(Clone, Debug, PartialEq)]
pub struct WorldView {
    pub map: u32,
    pub eye: [f64; 3],
    pub look: [f64; 3],
    pub up: [f64; 3],
    pub fov: f64,
    pub far: f64,
    pub detail: f64,
    pub objects: f64,
    pub cull: f64,
    pub render_scale: f64,
    /// The units within `objects` yards, the player first (decision T526).
    pub units: Vec<WorldUnit>,
    /// The clock (seconds): how far into their animations.
    pub time: f64,
}

/// A unit as the 3D world shows it (decision T526): its model, where it
/// stands (north, west, up), its facing and animation (`AnimationData`: 0
/// standing, 1 dead, 4 walking, 5 running).
#[derive(Clone, Debug, PartialEq)]
pub struct WorldUnit {
    /// The unit (its index among the units; the player 0: T546).
    pub id: usize,
    pub source: ModelSource,
    pub at: [f64; 3],
    pub facing: f64,
    pub animation: u32,
}

/// The units the 3D world shows within `reach` yards of the eye: the
/// player as their character, the others by their data's display
/// (`displayID`) or creature (`npcID`); none for a unit at no place or
/// of neither.
fn world_units(vm: &mut Vm, eye: [f64; 3], reach: f64) -> Vec<WorldUnit> {
    let mut out = Vec::new();
    let near = |p: (f64, f64, f64)| (p.0 - eye[0]).hypot(p.1 - eye[1]) <= reach;
    if let Some(at) = crate::character::place_of(vm, crate::units::PLAYER)
        && near(at)
    {
        let look = look_of(vm, "player");
        // (running, walking backwards, strafing turned toward the way,
        // shuffling as it turns: T536)
        let (animation, turned) = engine(vm).world.movement.pose();
        if look.race > 0 {
            out.push(WorldUnit {
                id: crate::units::PLAYER.0,
                source: ModelSource::Character(look),
                at: [at.0, at.1, at.2],
                facing: crate::unitinfo::player_facing(vm) + turned,
                animation,
            });
        }
    }
    let count = engine(vm).units.list.len();
    for i in 0..count {
        let id = crate::units::UnitId(i);
        if id == crate::units::PLAYER {
            continue;
        }
        let (alive, model, dead, walking) = {
            let u = &engine(vm).units.list[i];
            (
                u.alive_in_world && u.world.is_some(),
                // (a display its data names, else its creature's)
                u.display
                    .map(ModelSource::Display)
                    .or(u.npc.map(ModelSource::Creature)),
                u.dead,
                u.walk.clone(),
            )
        };
        let Some(source) = model.filter(|_| alive) else {
            continue;
        };
        let Some(at) = crate::character::place_of(vm, id).filter(|p| near(*p)) else {
            continue;
        };
        let animation = match (dead, walking) {
            (true, _) => 1,
            (false, Some(crate::movement::Walk::Patrol { .. })) => 4,
            (false, Some(_)) => 5,
            (false, None) => 0,
        };
        out.push(WorldUnit {
            id: id.0,
            source,
            at: [at.0, at.1, at.2],
            facing: crate::character::unit_facing(vm, id).unwrap_or(0.0),
            animation,
        });
    }
    out
}

/// The world's view while it is on and the player is at a place.
fn world_view(vm: &mut Vm) -> Option<WorldView> {
    if !engine(vm).world_on {
        return None;
    }
    let map = crate::character::player_world(vm)?.3;
    let e = crate::camera::eye(vm)?;
    let setting = |vm: &mut Vm, name: &str, default: f64| {
        crate::framescript::cvar_get(vm, name)
            .and_then(|v| v.parse::<f64>().ok())
            .unwrap_or(default)
    };
    let far = setting(vm, "farclip", 1000.0);
    let detail = setting(vm, "terrainLodDist", 500.0).min(far);
    let objects = setting(vm, "horizonStart", 1900.0).min(far);
    let cull = setting(vm, "lodObjectCullSize", 18.0);
    let render_scale = match setting(vm, "RenderScale", 1.0) {
        s if s.is_finite() => s.clamp(0.33, 2.0),
        _ => 1.0,
    };
    let units = world_units(vm, [e.at.0, e.at.1, e.at.2], objects);
    let time = vm.clock.elapsed_ms / 1000.0;
    let v = |a: (f64, f64, f64)| [a.0, a.1, a.2];
    Some(WorldView {
        map: map as u32,
        eye: v(e.at),
        look: v(e.look),
        up: v(e.up),
        fov: e.fov,
        far,
        detail,
        objects,
        cull,
        render_scale,
        units,
        time,
    })
}

pub(crate) const LAYERS: [&str; 5] = ["BACKGROUND", "BORDER", "ARTWORK", "OVERLAY", "HIGHLIGHT"];

/// Screen pixels per UIParent unit, and UIParent's height in its units.
pub fn pixel_scale(vm: &Vm) -> (f64, f64) {
    let e = engine(vm);
    let (w, h) = e.screen.ui_parent_size();
    (e.screen.width / w.max(1.0), h)
}

/// A widget's rectangle in screen pixels, if it has one.
pub fn rect_of(vm: &Vm, id: WidgetId) -> Option<Rect> {
    let (k, ui_h) = pixel_scale(vm);
    let (l, b, w, h) = widgets::bounds(vm, id, 0)?;
    Some(Rect {
        x: (l * k) as f32,
        y: ((ui_h - b - h) * k) as f32,
        w: (w * k) as f32,
        h: (h * k) as f32,
    })
}

fn first(vm: &Vm, id: WidgetId, stem: &str) -> Option<Value> {
    let props = engine(vm).world.widgets[id.0].props;
    stored_ref(vm, props, stem).and_then(|v| v.first().copied())
}

fn args(vm: &Vm, id: WidgetId, stem: &str) -> Option<Vec<Value>> {
    let props = engine(vm).world.widgets[id.0].props;
    stored_ref(vm, props, stem)
}

fn num(vm: &Vm, v: Option<&Value>, default: f64) -> f64 {
    match v.map(|v| vm.hidden(*v)) {
        Some(Value::Num(n)) => n,
        _ => default,
    }
}

fn text_of(vm: &Vm, v: Value) -> Option<String> {
    match vm.hidden(v) {
        Value::Str(s) => Some(String::from_utf8_lossy(vm.heap.str_bytes(s)).into_owned()),
        Value::Num(n) => Some(format!("{n}")),
        _ => None,
    }
}

fn color(vm: &Vm, v: Option<Vec<Value>>, default: [f32; 4]) -> [f32; 4] {
    match v {
        Some(v) => [
            num(vm, v.first(), f64::from(default[0])) as f32,
            num(vm, v.get(1), f64::from(default[1])) as f32,
            num(vm, v.get(2), f64::from(default[2])) as f32,
            num(vm, v.get(3), f64::from(default[3])) as f32,
        ],
        None => default,
    }
}

thread_local! {
    /// The animations' effects for the scene being built.
    static EFFECTS: std::cell::RefCell<std::collections::HashMap<usize, crate::animations::Effect>> =
        std::cell::RefCell::default();
}

fn effect_of(id: WidgetId) -> crate::animations::Effect {
    match EFFECTS.with(|e| e.borrow().get(&id.0).copied()) {
        Some(effect) => {
            // (a playing animation changes it each frame)
            volatile();
            effect
        }
        None => Default::default(),
    }
}

thread_local! {
    /// What the frame's items being laid out read changes by itself (the
    /// clock, the player's facing, an animation): they are not kept.
    static VOLATILE: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

/// The items being laid out change by themselves (decision T542).
fn volatile() {
    VOLATILE.with(|v| v.set(true));
}

/// A frame's items as the last scene laid them out (decision T542): its
/// group's frames and their children, which of its frames the mouse was
/// over. A change to any of them, or to what they are anchored to or
/// masked by, marks them (`widgets::mark`): they are laid out again.
#[derive(Default)]
pub(crate) struct Kept {
    epoch: u64,
    groups: rustc_hash::FxHashMap<usize, Group>,
    /// The frames in drawing order, and those drawn on their parent's
    /// level, while `scene_order_valid` (decision T543).
    order: Option<(Vec<WidgetId>, std::collections::HashSet<usize>)>,
}

struct Group {
    members: Vec<usize>,
    hover: Option<usize>,
    items: Vec<Item>,
}

/// Its alpha times its parents', up to one that ignores its parent's (a
/// playing alpha animation's in place of a region's own).
fn effective_alpha(vm: &Vm, id: WidgetId) -> f32 {
    let mut a = 1.0;
    let mut cur = Some(id);
    for _ in 0..256 {
        let Some(w) = cur else { break };
        let widget = &engine(vm).world.widgets[w.0];
        a *= effect_of(w).alpha.unwrap_or(widget.alpha);
        if first(vm, w, "IgnoreParentAlpha").is_some_and(|v| v.truthy()) {
            break;
        }
        cur = widget.parent;
    }
    a as f32
}

/// The rectangle everything of `id` is clipped to: the frames above it
/// that clip their children (scroll frames among them).
fn clip_of(vm: &Vm, id: WidgetId) -> Option<Rect> {
    let mut clip: Option<Rect> = None;
    let mut cur = engine(vm).world.widgets[id.0].parent;
    let mut seen = 0;
    while let Some(p) = cur {
        seen += 1;
        if seen > 256 {
            break;
        }
        let w = &engine(vm).world.widgets[p.0];
        let clips = first(vm, p, "ClipsChildren").is_some_and(|v| v.truthy())
            || widgets::is_a(engine(vm), &w.class, "ScrollFrame");
        if clips && let Some(r) = rect_of(vm, p) {
            clip = Some(match clip {
                Some(c) => c.intersect(r),
                None => r,
            });
        }
        cur = w.parent;
    }
    clip
}

/// A texture's gradient (`SetGradient(orientation, min, max)`).
fn gradient_of(vm: &Vm, id: WidgetId) -> Option<(bool, [f32; 4], [f32; 4])> {
    let a = args(vm, id, "Gradient")?;
    let horizontal = a
        .first()
        .and_then(|v| text_of(vm, *v))
        .is_some_and(|o| o.eq_ignore_ascii_case("HORIZONTAL"));
    let n = |i: usize| num(vm, a.get(i), 1.0) as f32;
    Some((
        horizontal,
        [n(1), n(2), n(3), n(4)],
        [n(5), n(6), n(7), n(8)],
    ))
}

fn blend_of(vm: &Vm, id: WidgetId) -> Blend {
    let set = first(vm, id, "BlendMode").and_then(|v| text_of(vm, v));
    // A button's highlight adds, unless set otherwise.
    let highlight = set.is_none()
        && engine(vm).world.widgets[id.0].parent.is_some_and(|p| {
            first(vm, p, "HighlightTexture")
                .and_then(|v| widgets::widget_of(vm, v))
                .is_some_and(|h| h == id)
        });
    if highlight {
        return Blend::Add;
    }
    match set.as_deref() {
        Some("ADD") => Blend::Add,
        Some("MOD") => Blend::Mod,
        Some("ALPHAKEY") => Blend::AlphaKey,
        Some("DISABLE") => Blend::Disable,
        _ => Blend::Blend,
    }
}

/// A texture's image, the atlas's region of its file (left, right, top,
/// bottom), whether the atlas tiles, and a color texture's color.
type ImageOf = (Image, Option<[f32; 4]>, (bool, bool), Option<[f32; 4]>);

/// A texture's image and its texture coordinates as the atlas or the
/// file gives them.
fn image_of(vm: &Vm, id: WidgetId) -> Option<ImageOf> {
    // A portrait the client draws: its flat stand-in (`facts.rs`).
    let props = engine(vm).world.widgets[id.0].props;
    if let Some(k) = vm.heap.interned("\u{0}portrait".as_bytes())
        && let Some(path) = text_of(vm, vm.heap.table(props).get(Value::Str(k)))
    {
        let look = vm
            .heap
            .interned("\u{0}portraitlook".as_bytes())
            .and_then(|k| text_of(vm, vm.heap.table(props).get(Value::Str(k))))
            .and_then(|t| decode_look(&t));
        let display = vm
            .heap
            .interned("\u{0}portraitdisplay".as_bytes())
            .map(|k| vm.heap.table(props).get(Value::Str(k)));
        let image = match (look, display) {
            (Some(look), _) => Image::Portrait {
                source: Box::new(ModelSource::Character(look)),
                fallback: path,
            },
            (None, Some(Value::Num(d))) if d > 0.0 => Image::Portrait {
                source: Box::new(ModelSource::Display(d as u32)),
                fallback: path,
            },
            _ => Image::File(path),
        };
        return Some((image, None, (false, false), None));
    }
    if let Some(name) = first(vm, id, "Atlas").and_then(|v| text_of(vm, v))
        && let Some(a) = crate::measures::atlas(vm, &name)
        && let Some(info) = &a.info
    {
        let c = info.coords;
        return Some((
            Image::FileId(info.file as u32),
            Some([c[0] as f32, c[1] as f32, c[2] as f32, c[3] as f32]),
            (info.tiles_horizontally, info.tiles_vertically),
            None,
        ));
    }
    if let Some(v) = first(vm, id, "Texture") {
        match vm.hidden(v) {
            Value::Str(_) => {
                let path = text_of(vm, v)?;
                if path.is_empty() {
                    return None;
                }
                return Some((Image::File(path), None, (false, false), None));
            }
            Value::Num(n) if n < 0.0 => {
                return Some((Image::File(loose_path(vm, n)?), None, (false, false), None));
            }
            Value::Num(0.0) => return None,
            Value::Num(n) => return Some((Image::FileId(n as u32), None, (false, false), None)),
            _ => {}
        }
    }
    let c = args(vm, id, "ColorTexture")?;
    let rgba = color(vm, Some(c), [1.0; 4]);
    Some((Image::Color, None, (false, false), Some(rgba)))
}

/// The path of a loose file's ID (`GetFileIDFromPath`'s, `GetTexture`'s
/// for an addon's file), without its extension.
fn loose_path(vm: &Vm, id: f64) -> Option<String> {
    engine(vm)
        .world
        .file_ids
        .iter()
        .find(|(_, v)| **v == id)
        .map(|(k, _)| k.clone())
}

fn texcoord_of(vm: &Vm, id: WidgetId, base: Option<[f32; 4]>) -> [f32; 8] {
    let from_lrtb = |l: f32, r: f32, t: f32, b: f32| [l, t, l, b, r, t, r, b];
    let [bl, br, bt, bb] = base.unwrap_or([0.0, 1.0, 0.0, 1.0]);
    let Some(tc) = args(vm, id, "TexCoord") else {
        return from_lrtb(bl, br, bt, bb);
    };
    let n: Vec<f32> = tc.iter().map(|v| num(vm, Some(v), 0.0) as f32).collect();
    // Within the atlas's region when it has one.
    let map = |x: f32, y: f32| [bl + x * (br - bl), bt + y * (bb - bt)];
    let corners: [[f32; 2]; 4] = if n.len() >= 8 {
        [
            map(n[0], n[1]),
            map(n[2], n[3]),
            map(n[4], n[5]),
            map(n[6], n[7]),
        ]
    } else if n.len() >= 4 {
        let (l, r, t, b) = (n[0], n[1], n[2], n[3]);
        [map(l, t), map(l, b), map(r, t), map(r, b)]
    } else {
        return from_lrtb(bl, br, bt, bb);
    };
    [
        corners[0][0],
        corners[0][1],
        corners[1][0],
        corners[1][1],
        corners[2][0],
        corners[2][1],
        corners[3][0],
        corners[3][1],
    ]
}

fn masks_of(vm: &Vm, id: WidgetId) -> Vec<Mask> {
    let props = engine(vm).world.widgets[id.0].props;
    let Some(key) = vm.heap.interned("\u{0}masks".as_bytes()) else {
        return Vec::new();
    };
    let Value::Table(t) = vm.heap.table(props).get(Value::Str(key)) else {
        return Vec::new();
    };
    let n = vm.heap.table(t).border();
    let mut out = Vec::new();
    for i in 1..=n {
        let v = vm.heap.table(t).get(Value::Num(i as f64));
        let Some(m) = widgets::widget_of(vm, v) else {
            continue;
        };
        let (Some((image, base, _, _)), Some(rect)) = (image_of(vm, m), rect_of(vm, m)) else {
            continue;
        };
        out.push(Mask {
            image,
            rect,
            region: base.unwrap_or([0.0, 1.0, 0.0, 1.0]),
            atlas: first(vm, m, "Atlas").and_then(|v| text_of(vm, v)),
        });
    }
    out
}

/// A status bar's texture: the part its value fills. Without points of its
/// own, the layout has placed it there already (`bars::fill_place`).
fn bar_fill(vm: &Vm, id: WidgetId, rect: &mut Rect, tc: &mut [f32; 8]) {
    let Some((_, f, vertical, reverse)) = crate::bars::fill_place(vm, id) else {
        return;
    };
    let f = f as f32;
    let laid_out = engine(vm).world.widgets[id.0].points.is_empty();
    let mut cut = *rect;
    let lerp = |a: f32, b: f32, t: f32| a + (b - a) * t;
    if vertical {
        let h = cut.h * f;
        if !reverse {
            cut.y += cut.h - h;
            // Keep the bottom part of the image.
            for i in [1, 5] {
                tc[i] = lerp(tc[i + 2], tc[i], f);
            }
        } else {
            for i in [3, 7] {
                tc[i] = lerp(tc[i - 2], tc[i], f);
            }
        }
        cut.h = h;
    } else {
        let w = cut.w * f;
        if reverse {
            cut.x += cut.w - w;
            tc[0] = lerp(tc[4], tc[0], f);
            tc[2] = lerp(tc[6], tc[2], f);
        } else {
            tc[4] = lerp(tc[0], tc[4], f);
            tc[6] = lerp(tc[2], tc[6], f);
        }
        cut.w = w;
    }
    if !laid_out {
        *rect = cut;
    }
}

fn quad_of(vm: &Vm, id: WidgetId) -> Option<Quad> {
    let (image, base, tile, plain) = image_of(vm, id)?;
    let mut rect = rect_of(vm, id)?;
    if rect.w <= 0.0 || rect.h <= 0.0 {
        return None;
    }
    let mut texcoord = texcoord_of(vm, id, base);
    let atlas = first(vm, id, "Atlas").and_then(|v| text_of(vm, v));
    let mut atlas_texcoord = texcoord_of(vm, id, None);
    // A flip book playing: its frame's part.
    if let Some((rows, cols, frame)) = effect_of(id).flip {
        let (col, row) = (frame % cols, frame / cols);
        let (u0, u1) = (col as f32 / cols as f32, (col + 1) as f32 / cols as f32);
        let (v0, v1) = (row as f32 / rows as f32, (row + 1) as f32 / rows as f32);
        texcoord = sub_rect(texcoord, u0, u1, v0, v1);
        atlas_texcoord = sub_rect(atlas_texcoord, u0, u1, v0, v1);
    }
    let mut fill_rect = rect;
    bar_fill(vm, id, &mut fill_rect, &mut atlas_texcoord);
    bar_fill(vm, id, &mut rect, &mut texcoord);
    let vc = color(vm, args(vm, id, "VertexColor"), [1.0; 4]);
    let pc = plain.unwrap_or([1.0; 4]);
    let a = effective_alpha(vm, id);
    Some(Quad {
        widget: id.0,
        rect,
        image,
        texcoord,
        atlas,
        atlas_texcoord,
        color: [
            vc[0] * pc[0],
            vc[1] * pc[1],
            vc[2] * pc[2],
            vc[3] * pc[3] * a,
        ],
        blend: blend_of(vm, id),
        desaturated: first(vm, id, "Desaturated").is_some_and(|v| v.truthy()),
        rotation: num(vm, first(vm, id, "Rotation").as_ref(), 0.0) as f32 + compass_turn(vm, id),
        tile: (
            tile.0 || first(vm, id, "HorizTile").is_some_and(|v| v.truthy()),
            tile.1 || first(vm, id, "VertTile").is_some_and(|v| v.truthy()),
        ),
        clip: clip_of(vm, id),
        masks: masks_of(vm, id),
        unit: unit_of(vm, id),
        snap: crate::measures::snaps_to_pixel_grid(vm, id),
        gradient: gradient_of(vm, id),
    })
}

/// The minimap's compass (the texture named `MinimapCompassTexture`) as
/// the client turns it with `rotateMinimap`: its north toward north, by
/// the player's facing (probed 2026-10-23: facing east, the pointer at its
/// left; no Lua turns it). Not otherwise, nor for another texture.
fn compass_turn(vm: &Vm, id: WidgetId) -> f32 {
    let named = engine(vm).world.widgets[id.0].name.as_deref() == Some("MinimapCompassTexture");
    if !named {
        return 0.0;
    }
    volatile();
    let rotates = engine(vm)
        .world
        .cvars
        .get("rotateMinimap")
        .is_some_and(|v| v.trim() == "1");
    if rotates {
        -crate::unitinfo::player_facing(vm) as f32
    } else {
        0.0
    }
}

/// Screen pixels per UI unit of a region.
fn unit_of(vm: &Vm, id: WidgetId) -> f32 {
    let (k, _) = pixel_scale(vm);
    let e = engine(vm);
    let ui = e.screen.ui_parent_scale();
    (k * widgets::effective_scale(e, id) / ui.max(1e-6)) as f32
}

/// A named colour (`|cnNAME:`): the global colour object of that name
/// (`VISITABLE_URL_DEFAULT_CHAT_LINK_COLOR`, made from the client's
/// colours), its `r`, `g`, `b`, `a`.
fn named_color(vm: &Vm, name: &str) -> Option<[f32; 4]> {
    let k = vm.heap.interned(name.as_bytes())?;
    let Value::Table(t) = vm.heap.table(vm.globals).get(Value::Str(k)) else {
        return None;
    };
    let field = |f: &str| -> Option<f32> {
        let k = vm.heap.interned(f.as_bytes())?;
        match vm.heap.table(t).get(Value::Str(k)) {
            Value::Num(n) => Some(n as f32),
            _ => None,
        }
    };
    Some([
        field("r")?,
        field("g")?,
        field("b")?,
        field("a").unwrap_or(1.0),
    ])
}

/// A text's inline textures and atlases, in order: the file or atlas,
/// whether an atlas, the width in UI units (0: its height).
fn inline_images(raw: &str) -> Vec<(String, bool, u32)> {
    let mut out = Vec::new();
    let mut rest = raw;
    while let Some(i) = rest.find('|') {
        let after = &rest[i + 1..];
        let kind = after.chars().next();
        match kind {
            Some('|') => rest = &after[1..],
            Some(k @ ('T' | 'A')) => {
                let body = &after[1..];
                let end = body.find(['|']).unwrap_or(body.len());
                let (file, _, w) = crate::text::inline_spec(&body[..end]);
                out.push((file, k == 'A', w));
                rest = body.get(end + 2..).unwrap_or("");
            }
            _ => rest = after,
        }
    }
    out
}

/// An inline texture's or atlas's picture at a place.
fn inline_quad(vm: &Vm, id: WidgetId, file: &str, atlas: bool, place: Rect) -> Option<Quad> {
    let full = [0.0, 0.0, 0.0, 1.0, 1.0, 0.0, 1.0, 1.0];
    let (image, texcoord) = if atlas {
        let info = crate::measures::atlas(vm, file)?.info.as_ref()?;
        let c = info.coords;
        let (l, r, t, b) = (c[0] as f32, c[1] as f32, c[2] as f32, c[3] as f32);
        (Image::FileId(info.file as u32), [l, t, l, b, r, t, r, b])
    } else if let Ok(n) = file.trim().parse::<u32>() {
        (Image::FileId(n), full)
    } else if let Ok(n) = file.trim().parse::<i64>() {
        (Image::File(loose_path(vm, n as f64)?), full)
    } else {
        (Image::File(file.to_string()), full)
    };
    Some(Quad {
        widget: id.0,
        rect: place,
        image,
        texcoord,
        atlas: None,
        atlas_texcoord: texcoord,
        color: [1.0, 1.0, 1.0, effective_alpha(vm, id)],
        blend: Blend::Blend,
        desaturated: false,
        rotation: 0.0,
        tile: (false, false),
        clip: clip_of(vm, id),
        masks: Vec::new(),
        unit: unit_of(vm, id),
        snap: true,
        gradient: None,
    })
}

/// A text's characters with their colors (`|cAARRGGBB … |r`, `|cnNAME:`),
/// the escapes taken out as [`crate::text::visible`] takes them.
fn colored_chars(vm: &Vm, raw: &str, base: [f32; 4]) -> Vec<(char, [f32; 4])> {
    let mut out = Vec::new();
    let mut stack: Vec<[f32; 4]> = Vec::new();
    let mut chars = raw.chars().peekable();
    let cur = |stack: &Vec<[f32; 4]>| *stack.last().unwrap_or(&base);
    while let Some(c) = chars.next() {
        if c != '|' {
            out.push((c, cur(&stack)));
            continue;
        }
        match chars.next() {
            Some('|') => out.push(('|', cur(&stack))),
            Some('c') if chars.peek() == Some(&'n') => {
                chars.next();
                let name: String = chars.by_ref().take_while(|&x| x != ':').collect();
                let c = named_color(vm, &name).unwrap_or(cur(&stack));
                stack.push([c[0], c[1], c[2], base[3] * c[3]]);
            }
            Some('c') => {
                let hex: String = (0..8).filter_map(|_| chars.next()).collect();
                let byte = |i: usize| {
                    u8::from_str_radix(hex.get(i..i + 2).unwrap_or("ff"), 16).unwrap_or(255)
                };
                // The `AA` byte is not used: the client draws the color
                // opaque (the beta's tooltip line
                // `|c0042b1fePress F6 to submit an issue…` shows in game).
                stack.push([
                    f32::from(byte(2)) / 255.0,
                    f32::from(byte(4)) / 255.0,
                    f32::from(byte(6)) / 255.0,
                    base[3],
                ]);
            }
            Some('r') => {
                stack.pop();
            }
            Some('h') => {}
            Some('n') => out.push(('\n', cur(&stack))),
            Some('4') => {
                let spec: String = chars.by_ref().take_while(|&x| x != ';').collect();
                let before: String = out.iter().map(|(ch, _)| *ch).collect();
                for ch in crate::text::plural(&before, &spec).chars() {
                    out.push((ch, cur(&stack)));
                }
            }
            Some('H') => {
                while let Some(x) = chars.next() {
                    if x == '|' && chars.peek() == Some(&'h') {
                        chars.next();
                        break;
                    }
                }
            }
            Some('T') | Some('A') => {
                let mut spec = String::new();
                while let Some(x) = chars.next() {
                    if x == '|' && matches!(chars.peek(), Some('t') | Some('a')) {
                        chars.next();
                        break;
                    }
                    spec.push(x);
                }
                let h = crate::text::inline_spec(&spec).1;
                out.push((crate::text::inline_char(h), cur(&stack)));
            }
            Some(other) => {
                out.push(('|', cur(&stack)));
                out.push((other, cur(&stack)));
            }
            None => out.push(('|', cur(&stack))),
        }
    }
    out
}

/// A font string's (or an edit box's) text, laid out as the engine
/// measures it.
fn text_item(vm: &Vm, id: WidgetId, edit_box: bool) -> Option<Text> {
    text_item_in(vm, id, edit_box, None)
}

/// [`text_item`] in a given rectangle (a message frame's line), from its
/// top, left aligned.
fn text_item_in(vm: &Vm, id: WidgetId, edit_box: bool, place: Option<Rect>) -> Option<Text> {
    let w = &engine(vm).world.widgets[id.0];
    let raw = w.text.and_then(|v| text_of(vm, v))?;
    if raw.is_empty() {
        return None;
    }
    // A button's text without points of its own sits in the button's
    // middle, as the client places it.
    let mut rect = place.or_else(|| rect_of(vm, id)).or_else(|| {
        let parent = w.parent?;
        let class = &engine(vm).world.widgets[parent.0].class;
        if !w.points.is_empty() || !widgets::is_a(engine(vm), class, "Button") {
            return None;
        }
        rect_of(vm, parent)
    })?;
    if edit_box && let Some(ins) = args(vm, id, "TextInsets") {
        let (k, _) = pixel_scale(vm);
        let k = k as f32;
        let l = num(vm, ins.first(), 0.0) as f32 * k;
        let r = num(vm, ins.get(1), 0.0) as f32 * k;
        let t = num(vm, ins.get(2), 0.0) as f32 * k;
        let b = num(vm, ins.get(3), 0.0) as f32 * k;
        rect = Rect {
            x: rect.x + l,
            y: rect.y + t,
            w: (rect.w - l - r).max(0.0),
            h: (rect.h - t - b).max(0.0),
        };
    }
    let font_args = widgets::font_chain_ref(vm, id, "Font").unwrap_or_default();
    let font = font_args
        .first()
        .and_then(|v| text_of(vm, *v))
        .unwrap_or_default();
    let flags = font_args
        .get(2)
        .and_then(|v| text_of(vm, *v))
        .unwrap_or_default()
        .to_ascii_uppercase();
    let base = color(
        vm,
        widgets::font_chain_ref(vm, id, "TextColor"),
        [1.0, 1.0, 1.0, 1.0],
    );
    let alpha = effective_alpha(vm, id);
    let base = [base[0], base[1], base[2], base[3] * alpha];
    let shadow = {
        let off = widgets::font_chain_ref(vm, id, "ShadowOffset");
        let (dx, dy) = match &off {
            Some(o) => (num(vm, o.first(), 0.0), num(vm, o.get(1), 0.0)),
            None => (0.0, 0.0),
        };
        if dx != 0.0 || dy != 0.0 {
            let c = color(
                vm,
                widgets::font_chain_ref(vm, id, "ShadowColor"),
                [0.0, 0.0, 0.0, 1.0],
            );
            let (k, _) = pixel_scale(vm);
            Some((
                [c[0], c[1], c[2], c[3] * alpha],
                (dx * k) as f32,
                (-dy * k) as f32,
            ))
        } else {
            None
        }
    };
    let wraps = !edit_box
        && widgets::font_chain_ref(vm, id, "WordWrap")
            .and_then(|v| v.first().copied())
            .is_none_or(|v| v.truthy());
    // The region's scale over UIParent's too: a font string in a frame
    // scaled 0.8 is drawn 0.8 the size, as its rectangle is (the Settings
    // list's "NEW" badges, `scale=".8"`, were drawn full size and cut to
    // "N...").
    let relative = {
        let e = engine(vm);
        widgets::effective_scale(e, id) / e.screen.ui_parent_scale().max(1e-6)
    };
    let (k, _) = pixel_scale(vm);
    // its width in its own units, as the text is laid out in them
    let units_w = f64::from(rect.w) / k / relative.max(1e-6);
    let lines = crate::text::wrapped_lines(vm, id, &raw, wraps.then_some(units_w));
    let first_line = crate::text::glyph_line(vm, id, "");
    // Pixel sizes in the region's own pixels; the screen's are these
    // times the pixel ratio.
    let ratio = (f64::from(crate::text::pixel(vm, id)) * k * relative) as f32;
    let line_h = first_line.line_height_px as f32 * ratio;
    let spacing = num(vm, first(vm, id, "Spacing").as_ref(), 0.0) as f32 * k as f32;
    let justify_h = widgets::font_chain_ref(vm, id, "JustifyH")
        .and_then(|v| v.first().copied())
        .and_then(|v| text_of(vm, v))
        .unwrap_or_else(|| {
            if edit_box {
                "LEFT".into()
            } else {
                "CENTER".into()
            }
        });
    let justify_v = if place.is_some() {
        "TOP".to_string()
    } else {
        widgets::font_chain_ref(vm, id, "JustifyV")
            .and_then(|v| v.first().copied())
            .and_then(|v| text_of(vm, v))
            .unwrap_or_else(|| "MIDDLE".into())
    };
    // Lines that fit (at least one).
    let fit = if rect.h > 0.0 {
        ((rect.h + spacing) / (line_h + spacing)).floor().max(1.0) as usize
    } else {
        lines.len()
    };
    let shown: Vec<String> = lines.into_iter().take(fit.max(1)).collect();
    let total_h = shown.len() as f32 * line_h + (shown.len().saturating_sub(1)) as f32 * spacing;
    let top = match justify_v.as_str() {
        "TOP" => rect.y,
        "BOTTOM" => rect.y + rect.h - total_h,
        _ => rect.y + (rect.h - total_h) / 2.0,
    };
    let colored = colored_chars(vm, &raw, base);
    let inline = inline_images(&raw);
    let mut images = Vec::new();
    let font_size = crate::text::font_of(vm, id).size.max(1.0) as f32;
    let text_px = g_px(&first_line) * ratio;
    let mut seen_inline = 0usize;
    let mut cursor = 0usize;
    let mut out_lines = Vec::new();
    for (i, line) in shown.iter().enumerate() {
        // Each laid-out line's characters, in order, with their colors
        // (the spaces and new lines wrapping dropped skipped).
        let mut picked: Vec<(char, [f32; 4])> = Vec::new();
        for ch in line.chars() {
            while cursor < colored.len() && colored[cursor].0 != ch {
                cursor += 1;
            }
            if cursor < colored.len() {
                picked.push(colored[cursor]);
                cursor += 1;
            } else {
                picked.push((ch, base));
            }
        }
        let mut text: String = line.clone();
        let mut g = crate::text::glyph_line(vm, id, &text);
        let mut width = g.width_px as f32 * ratio;
        // Too wide without wrapping: cut, with an ellipsis.
        if width > rect.w + 0.5 && rect.w > 0.0 {
            let mut chars: Vec<char> = text.chars().collect();
            while !chars.is_empty() {
                chars.pop();
                let candidate: String = chars.iter().collect::<String>() + "...";
                let cg = crate::text::glyph_line(vm, id, &candidate);
                if cg.width_px as f32 * ratio <= rect.w + 0.5 {
                    text = candidate;
                    g = cg;
                    break;
                }
            }
            let kept = chars.len();
            picked.truncate(kept);
            for _ in 0..3 {
                picked.push(('.', picked.last().map_or(base, |p| p.1)));
            }
            width = g.width_px as f32 * ratio;
        }
        let x = match justify_h.as_str() {
            "LEFT" => rect.x,
            "RIGHT" => rect.x + rect.w - width,
            _ => rect.x + (rect.w - width) / 2.0,
        };
        let y = top + i as f32 * (line_h + spacing);
        let mut runs: Vec<Run> = Vec::new();
        for (j, (ch, c)) in picked.iter().enumerate() {
            let cx = g.xs.get(j).copied().unwrap_or(0.0) as f32 * ratio;
            // An inline texture: drawn as a picture where it stands.
            if let Some(h) = crate::text::inline_height(*ch) {
                if let Some((file, atlas, w_units)) = inline.get(seen_inline) {
                    let hp = if h == 0 {
                        text_px
                    } else {
                        h as f32 * text_px / font_size
                    };
                    let wp = if *w_units == 0 {
                        hp
                    } else {
                        *w_units as f32 * text_px / font_size
                    };
                    let place = Rect {
                        x: (x + cx).round(),
                        y: (y + (line_h - hp) / 2.0).round(),
                        w: wp,
                        h: hp,
                    };
                    if let Some(q) = inline_quad(vm, id, file, *atlas, place) {
                        images.push(q);
                    }
                }
                seen_inline += 1;
                continue;
            }
            match runs.last_mut() {
                Some(r) if r.color == *c => {
                    r.text.push(*ch);
                    r.xs.push(cx);
                }
                _ => runs.push(Run {
                    text: ch.to_string(),
                    color: *c,
                    xs: vec![cx],
                }),
            }
        }
        out_lines.push(TextLine {
            x: x.round(),
            y: y.round(),
            width,
            runs,
        });
    }
    Some(Text {
        widget: id.0,
        images,
        rect,
        font,
        px: g_px(&first_line) * ratio,
        line_height: line_h,
        outline: if flags.contains("THICKOUTLINE") {
            2
        } else if flags.contains("OUTLINE") {
            1
        } else {
            0
        },
        monochrome: flags.contains("MONOCHROME"),
        shadow,
        lines: out_lines,
        clip: clip_of(vm, id),
    })
}

/// A message frame's messages, as the client shows them: newest at the
/// bottom of the frame, each wrapped to its width, older ones above while
/// they fit. Unverified: fading (`SetTimeVisible`), the insertion mode
/// (`SetInsertMode`), the frame's scroll.
fn message_items(vm: &Vm, frame: WidgetId) -> Vec<Text> {
    let Some(list) = engine(vm).world.messages.get(&frame.0) else {
        return Vec::new();
    };
    let Some(r) = rect_of(vm, frame) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    let mut bottom = r.y + r.h;
    for (_, v) in list.iter().rev() {
        let Some(fs) = widgets::widget_of(vm, *v) else {
            continue;
        };
        if !engine(vm).world.widgets[fs.0].shown {
            continue;
        }
        let tall = Rect {
            x: r.x,
            y: r.y,
            w: r.w,
            h: f32::MAX / 4.0,
        };
        let Some(mut t) = text_item_in(vm, fs, false, Some(tall)) else {
            continue;
        };
        let h = t.lines.len() as f32 * t.line_height;
        let top = bottom - h;
        if top < r.y - 0.5 {
            break;
        }
        let dy = top - r.y;
        for line in &mut t.lines {
            line.y += dy;
            line.x = r.x;
        }
        t.rect = Rect {
            x: r.x,
            y: top,
            w: r.w,
            h,
        };
        t.clip = Some(match t.clip {
            Some(c) => c.intersect(r),
            None => r,
        });
        out.push(t);
        bottom = top;
    }
    out.reverse();
    out
}

fn g_px(g: &crate::text::GlyphLine) -> f32 {
    g.px as f32
}

/// A region's draw layer rank and sublevel.
fn layer_of(vm: &Vm, id: WidgetId) -> (usize, i32) {
    let props = engine(vm).world.widgets[id.0].props;
    // A region no layer was given: a font string on `OVERLAY`, the others
    // on `ARTWORK`, as a fresh one's `GetDrawLayer` answers in the client
    // (recorded; a button's `ButtonText`, outside any `<Layer>`, was drawn
    // under its normal texture: the Legacy challenges' categories). Read
    // without copies: every region asks each frame (T541).
    let fallback = if widgets::kind(engine(vm), id) & widgets::KIND_FONT_STRING != 0 {
        3
    } else {
        2
    };
    let layer = match widgets::stored_arg(vm, props, "DrawLayer", 0).map(|v| vm.hidden(v)) {
        Some(Value::Str(s)) => {
            let name = vm.heap.str_bytes(s);
            LAYERS
                .iter()
                .position(|l| l.as_bytes() == name)
                .unwrap_or(2)
        }
        Some(Value::Nil) | None => fallback,
        Some(_) => 2,
    };
    let sub = widgets::stored_arg(vm, props, "DrawLayer", 1);
    (layer, num(vm, sub.as_ref(), 0.0) as i32)
}

/// Whether a widget is a region (drawn by its frame), by its type: XML
/// builds regions as it builds frames.
fn is_region(vm: &Vm, id: WidgetId) -> bool {
    widgets::kind(engine(vm), id) & widgets::KIND_REGION != 0
}

/// Whether a widget is a frame (drawn in the strata's order).
fn is_frame(vm: &Vm, id: WidgetId) -> bool {
    let e = engine(vm);
    let k = widgets::kind(e, id);
    k & widgets::KIND_REGION == 0 && (e.world.widgets[id.0].frame || k & widgets::KIND_FRAME != 0)
}

/// Whether a frame is drawn with its parent: it uses its parent's level
/// (`useParentLevel`, `SetUsingParentLevel(true)`), in its parent's strata.
fn merges_into_parent(vm: &mut Vm, id: WidgetId) -> bool {
    let props = engine(vm).world.widgets[id.0].props;
    let using = widgets::stored_args(vm, props, "UsingParentLevel")
        .and_then(|v| v.first().copied())
        .is_some_and(|v| v.truthy());
    let Some(parent) = engine(vm).world.widgets[id.0].parent else {
        return false;
    };
    using
        && is_frame(vm, parent)
        && widgets::strata_rank(vm, id) == widgets::strata_rank(vm, parent)
}

/// The visible frames, in draw order (back to front).
fn frames_in_order(vm: &mut Vm) -> Vec<WidgetId> {
    let mut frames: Vec<(usize, f64, usize)> = Vec::new();
    // (walked from the widgets without a parent through the shown ones
    // alone: a hidden frame's widgets, most of a large UI's, are never
    // looked at; a frame's strata its own, else its parent's; T541)
    // (each frame's level from its parent's as `widgets::frame_level`
    // finds it: its own, else its parent's and one unless it uses its
    // parent's; T542)
    let mut stack: Vec<(usize, usize, Option<f64>)> = engine(vm)
        .world
        .widgets
        .iter()
        .enumerate()
        .filter(|(_, w)| w.parent.is_none() && w.shown)
        .map(|(i, _)| (i, 2, None))
        .collect();
    while let Some((i, inherited, parent_level)) = stack.pop() {
        let id = WidgetId(i);
        let frame = is_frame(vm, id);
        let (strata, level) = if frame {
            let props = engine(vm).world.widgets[i].props;
            let level = match widgets::stored_arg(vm, props, "FrameLevel", 0) {
                Some(Value::Num(n)) => n,
                _ => {
                    let using = widgets::stored_arg(vm, props, "UsingParentLevel", 0)
                        .is_some_and(|v| v.truthy());
                    parent_level.map_or(0.0, |p| if using { p } else { p + 1.0 })
                }
            };
            (widgets::own_strata(vm, id).unwrap_or(inherited), level)
        } else {
            (inherited, parent_level.unwrap_or(0.0))
        };
        if frame && !engine(vm).world.widgets[i].forbidden {
            frames.push((strata, level, i));
        }
        let e = engine(vm);
        stack.extend(
            e.world.widgets[i]
                .children
                .iter()
                .filter(|c| e.world.widgets[c.0].shown)
                .map(|c| (c.0, strata, Some(level))),
        );
    }
    frames.sort_by(|a, b| a.0.cmp(&b.0).then(a.1.total_cmp(&b.1)).then(a.2.cmp(&b.2)));
    frames.into_iter().map(|(_, _, i)| WidgetId(i)).collect()
}

fn mouse_enabled(vm: &Vm, id: WidgetId) -> bool {
    match first(vm, id, "MouseEnabled") {
        Some(v) => v.truthy(),
        None => {
            let class = &engine(vm).world.widgets[id.0].class;
            widgets::mouse_by_default(engine(vm), class)
        }
    }
}

/// The part (`u0..u1`, `v0..v1` of the unit square) of texture
/// coordinates given by their corners (top left, bottom left, top right,
/// bottom right).
fn sub_rect(tc: [f32; 8], u0: f32, u1: f32, v0: f32, v1: f32) -> [f32; 8] {
    let at = |u: f32, v: f32| {
        [
            tc[0] + (tc[4] - tc[0]) * u + (tc[2] - tc[0]) * v,
            tc[1] + (tc[5] - tc[1]) * u + (tc[3] - tc[1]) * v,
        ]
    };
    let [a, b, c, d] = [at(u0, v0), at(u0, v1), at(u1, v0), at(u1, v1)];
    [a[0], a[1], b[0], b[1], c[0], c[1], d[0], d[1]]
}

fn nums(vm: &Vm, id: WidgetId, stem: &str, n: usize) -> Option<Vec<f32>> {
    let a = args(vm, id, stem)?;
    Some((0..n).map(|i| num(vm, a.get(i), 0.0) as f32).collect())
}

/// A unit's look: its race, sex, class and what it wears, as the API
/// answers them.
pub(crate) fn look_of(vm: &mut Vm, unit: &str) -> Look {
    let u = vm.str(unit);
    let call = |vm: &mut Vm, name: &str, extra: Option<Value>| -> Vec<Value> {
        let f = vm.get_global(name);
        if !matches!(f, Value::Func(_)) {
            return Vec::new();
        }
        let mut a = vec![u];
        a.extend(extra);
        vm.pcall(f, &a).unwrap_or_default()
    };
    let id_at = |v: &[Value], i: usize| match v.get(i) {
        Some(Value::Num(n)) => *n as u32,
        _ => 0,
    };
    let race = id_at(&call(vm, "UnitRace", None), 2);
    let sex = match call(vm, "UnitSex", None).first() {
        Some(Value::Num(n)) if *n == 3.0 => 1,
        _ => 0,
    };
    let class = id_at(&call(vm, "UnitClass", None), 2);
    let mut items = Vec::new();
    for slot in 1..=19u32 {
        let r = call(vm, "GetInventoryItemID", Some(Value::Num(f64::from(slot))));
        if let Some(Value::Num(n)) = r.first()
            && *n > 0.0
        {
            items.push((slot, *n as u32));
        }
    }
    Look {
        race,
        sex,
        class,
        items,
        tried: Vec::new(),
    }
}

/// A look as text (`race:sex:class:slot=item,…`), for a portrait kept
/// as the unit looked when it was set.
pub(crate) fn encode_look(l: &Look) -> String {
    let items: Vec<String> = l.items.iter().map(|(s, i)| format!("{s}={i}")).collect();
    format!("{}:{}:{}:{}", l.race, l.sex, l.class, items.join(","))
}

fn decode_look(t: &str) -> Option<Look> {
    let mut p = t.split(':');
    let race = p.next()?.parse().ok()?;
    let sex = p.next()?.parse().ok()?;
    let class = p.next()?.parse().ok()?;
    let items = p
        .next()
        .unwrap_or("")
        .split(',')
        .filter_map(|e| {
            let (s, i) = e.split_once('=')?;
            Some((s.parse().ok()?, i.parse().ok()?))
        })
        .collect();
    (race > 0).then_some(Look {
        race,
        sex,
        class,
        items,
        tried: Vec::new(),
    })
}

/// An actor of a model frame or a scene's actor widget.
fn actor_of(vm: &mut Vm, id: WidgetId, frame: bool) -> Option<Actor> {
    let (source, tried, undressed) = crate::models::source_of(vm, id)?;
    let source = match source {
        crate::models::Source::File(f) => ModelSource::File(f),
        crate::models::Source::Path(p) => ModelSource::Path(p),
        crate::models::Source::Display(d) => ModelSource::Display(d),
        crate::models::Source::Creature(c) => ModelSource::Creature(c),
        crate::models::Source::Item(i) => ModelSource::Item(i),
        crate::models::Source::Appearance(a) => ModelSource::Appearance(a),
        crate::models::Source::Unit(u) => {
            let mut look = look_of(vm, &u);
            if undressed {
                look.items.clear();
            }
            let emptied = crate::models::undressed_slots(vm, id);
            look.items.retain(|(s, _)| !emptied.contains(s));
            look.tried = tried;
            ModelSource::Character(look)
        }
    };
    let position = nums(vm, id, "Position", 3).unwrap_or(vec![0.0; 3]);
    let (yaw, scale, alpha) = if frame {
        let facing = first(vm, id, "Facing")
            .or_else(|| first(vm, id, "Rotation"))
            .map_or(0.0, |v| num(vm, Some(&v), 0.0));
        (
            facing as f32,
            num(vm, first(vm, id, "ModelScale").as_ref(), 1.0) as f32,
            num(vm, first(vm, id, "ModelAlpha").as_ref(), 1.0) as f32,
        )
    } else {
        (
            num(vm, first(vm, id, "Yaw").as_ref(), 0.0) as f32,
            num(vm, first(vm, id, "Scale").as_ref(), 1.0) as f32,
            num(vm, first(vm, id, "Alpha").as_ref(), 1.0) as f32,
        )
    };
    let anim = args(vm, id, "Animation").unwrap_or_default();
    Some(Actor {
        source,
        position: [position[0], position[1], position[2]],
        yaw,
        pitch: num(vm, first(vm, id, "Pitch").as_ref(), 0.0) as f32,
        roll: num(vm, first(vm, id, "Roll").as_ref(), 0.0) as f32,
        fixed_axes: frame,
        scale,
        alpha,
        animation: num(vm, anim.first(), 0.0).max(0.0) as u32,
        variation: num(vm, anim.get(1), 0.0).max(0.0) as u32,
    })
}

/// The light of a model frame (`SetLight`) or scene (`SetLightDirection`,
/// …); else a light from the front above (unverified: the client's
/// default light).
fn light_of(vm: &Vm, id: WidgetId) -> Light {
    let mut light = Light {
        enabled: true,
        direction: [-1.0, 0.3, -0.6],
        ambient: [0.55, 0.55, 0.55],
        diffuse: [0.75, 0.75, 0.75],
    };
    if let Some(d) = nums(vm, id, "LightDirection", 3) {
        light.direction = [d[0], d[1], d[2]];
    }
    if let Some(c) = nums(vm, id, "LightAmbientColor", 3) {
        light.ambient = [c[0], c[1], c[2]];
    }
    if let Some(c) = nums(vm, id, "LightDiffuseColor", 3) {
        light.diffuse = [c[0], c[1], c[2]];
    }
    if let Some(v) = first(vm, id, "LightVisible") {
        light.enabled = v.truthy();
    }
    light
}

/// The field of view of a model frame's placed camera, across the frame's
/// diagonal: 50 degrees (fitted to WoW Forever, 2026-10-23: UI camera 542
/// on 200 by 250 and 400 by 250 frames, the model the same size to a
/// pixel or two, larger by the diagonals' ratio in the wider frame).
const PLACED_CAMERA_FOV: f32 = 50.0 * std::f32::consts::PI / 180.0;

/// A model frame's or model scene's model item, if it shows a model.
fn model_item(vm: &mut Vm, f: WidgetId, class: &str) -> Option<Model> {
    let e = engine(vm);
    let is_model = widgets::is_a(e, class, "Model");
    let is_scene = widgets::is_a(e, class, "ModelScene");
    if !is_model && !is_scene {
        return None;
    }
    let rect = rect_of(vm, f)?;
    if rect.w <= 0.0 || rect.h <= 0.0 {
        return None;
    }
    let (actors, camera) = if is_model {
        let actor = actor_of(vm, f, true)?;
        // A camera the code placed (`SetCameraPosition`,
        // `SetCameraTarget`: `Model_ApplyUICamera`'s), else the frame's.
        let placed = nums(vm, f, "CameraPosition", 3).zip(nums(vm, f, "CameraTarget", 3));
        // Its position and target are in model space, the model drawn in
        // camera space: one camera-space unit is 0.8171 times the frame's
        // effective scale there (`TransformCameraSpaceToModelSpace`, probed).
        let camera = if let Some((p, t)) = placed {
            let k = crate::methods3::camera_to_model(vm, f).max(1e-6) as f32;
            let back = |v: Vec<f32>| -> Vec<f32> { v.iter().map(|c| c / k).collect() };
            let (p, t) = (back(p), back(t));
            let forward = [t[0] - p[0], t[1] - p[1], t[2] - p[2]];
            ModelCamera::Scene {
                position: [p[0], p[1], p[2]],
                forward,
                right: [forward[1], -forward[0], 0.0],
                up: [0.0, 0.0, 1.0],
                fov: {
                    let half = (PLACED_CAMERA_FOV / 2.0).tan();
                    let a = rect.w / rect.h;
                    2.0 * (half / (1.0 + a * a).sqrt()).atan()
                },
                near: 0.1,
                far: 100.0,
            }
        } else {
            ModelCamera::Frame {
                portrait_zoom: num(vm, first(vm, f, "PortraitZoom").as_ref(), 0.0) as f32,
                distance_scale: num(vm, first(vm, f, "CamDistanceScale").as_ref(), 1.0) as f32,
                camera: first(vm, f, "Camera").map(|v| num(vm, Some(&v), 0.0) as i32),
            }
        };
        (vec![actor], camera)
    } else {
        let list = widgets::actors(vm, f);
        let n = vm.heap.table(list).border();
        let mut actors = Vec::new();
        for k in 1..=n {
            let v = vm.heap.table(list).get(Value::Num(k as f64));
            let Some(a) = widgets::widget_of(vm, v) else {
                continue;
            };
            if !engine(vm).world.widgets[a.0].shown {
                continue;
            }
            if let Some(actor) = actor_of(vm, a, false) {
                actors.push(actor);
            }
        }
        if actors.is_empty() {
            return None;
        }
        let position = nums(vm, f, "CameraPosition", 3).unwrap_or(vec![5.0, 0.0, 1.0]);
        // The axes set, or those of the yaw, pitch and roll set.
        let set = args(vm, f, "CameraOrientationByAxisVectors").is_some()
            || args(vm, f, "CameraOrientationByYawPitchRoll").is_some();
        let (forward, right, up) = if set {
            let a = crate::methods3::scene_axes(vm, f);
            let v = |i: usize| [a[i][0] as f32, a[i][1] as f32, a[i][2] as f32];
            (v(0), v(1), v(2))
        } else {
            ([-1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0])
        };
        let camera = ModelCamera::Scene {
            position: [position[0], position[1], position[2]],
            forward,
            right,
            up,
            fov: num(vm, first(vm, f, "CameraFieldOfView").as_ref(), 0.7) as f32,
            near: num(vm, first(vm, f, "CameraNearClip").as_ref(), 0.2) as f32,
            far: num(vm, first(vm, f, "CameraFarClip").as_ref(), 100.0) as f32,
        };
        (actors, camera)
    };
    Some(Model {
        widget: f.0,
        rect,
        clip: clip_of(vm, f),
        alpha: effective_alpha(vm, f),
        actors,
        camera,
        light: light_of(vm, f),
        time: crate::clock::now(vm),
    })
}

/// The scene: every visible region, back to front.
/// The yards a minimap spans outdoors at each zoom (0 to 5), as the
/// client draws it (`C_Minimap.GetViewRadius` twice these: 233 1/3 at 0,
/// 100 at 4, 66 2/3 at 5, probed in WoW Forever).
pub(crate) const MINIMAP_YARDS: [f64; 6] = [
    466.0 + 2.0 / 3.0,
    400.0,
    333.0 + 1.0 / 3.0,
    266.0 + 2.0 / 3.0,
    200.0,
    133.0 + 1.0 / 3.0,
];

/// The yards a terrain tile spans.
const TILE_YARDS: f64 = 1600.0 / 3.0;

/// A minimap's mask: the one `SetMaskTexture` set (an atlas or a file),
/// stretched over the minimap; none set, none (unverified: the client's
/// own default, `Textures\\MinimapMask` on old clients, is not in
/// camelot's files).
fn minimap_mask(vm: &Vm, f: WidgetId, rect: Rect) -> Option<Mask> {
    let set = first(vm, f, "MaskTexture").and_then(|v| match vm.hidden(v) {
        Value::Num(n) => Some(Err(n as u32)),
        v => text_of(vm, v).filter(|t| !t.is_empty()).map(Ok),
    });
    let atlas = |name: &str| {
        let info = crate::measures::atlas(vm, name)?.info.as_ref()?;
        let c = info.coords;
        Some(Mask {
            image: Image::FileId(info.file as u32),
            rect,
            region: [c[0] as f32, c[1] as f32, c[2] as f32, c[3] as f32],
            atlas: Some(name.to_string()),
        })
    };
    let file = |image: Image| Mask {
        image,
        rect,
        region: [0.0, 1.0, 0.0, 1.0],
        atlas: None,
    };
    Some(match set? {
        Ok(name) => atlas(&name).unwrap_or_else(|| file(Image::File(name))),
        Err(id) => file(Image::FileId(id)),
    })
}

/// The minimap's zoom (the frame named `Minimap`, `SetZoom`), 0 to 5.
pub(crate) fn minimap_zoom(vm: &mut Vm) -> usize {
    let m = vm.get_global("Minimap");
    match widgets::widget_of(vm, m) {
        Some(f) => num(vm, first(vm, f, "Zoom").as_ref(), 0.0).clamp(0.0, 5.0) as usize,
        None => 0,
    }
}

/// Whether the minimap turns with the player (`rotateMinimap`).
pub(crate) fn minimap_rotates(vm: &mut Vm) -> bool {
    crate::framescript::cvar_get(vm, "rotateMinimap").is_some_and(|v| v.trim() == "1")
}

/// A minimap's terrain about the player (spec debugger §18.11): the
/// client's minimap textures of the terrain tiles it covers at its zoom,
/// through its mask; north up, each cut to the minimap, or with
/// `rotateMinimap` turned so the player's facing is up, the whole tiles
/// turned about the minimap's center (the mask and the minimap's
/// rectangle cut them). Indoors nothing (the client draws the building's
/// own map there, not read).
fn minimap_tiles(vm: &mut Vm, f: WidgetId) -> Vec<Quad> {
    let Some(rect) = rect_of(vm, f) else {
        return Vec::new();
    };
    if rect.w <= 0.0 || rect.h <= 0.0 || engine(vm).world.character.position.indoors {
        return Vec::new();
    }
    let Some((north, west, _, instance)) = crate::character::player_world(vm) else {
        return Vec::new();
    };
    let zoom = num(vm, first(vm, f, "Zoom").as_ref(), 0.0).clamp(0.0, 5.0) as usize;
    // Yards per pixel: the minimap's width spans the zoom's yards.
    let ypp = MINIMAP_YARDS[zoom] / rect.w as f64;
    let (cx, cy) = (
        rect.x as f64 + rect.w as f64 / 2.0,
        rect.y as f64 + rect.h as f64 / 2.0,
    );
    // Turned, the player's facing up: the map turns clockwise by it.
    let turn = if minimap_rotates(vm) {
        crate::unitinfo::player_facing(vm)
    } else {
        0.0
    };
    let (mut half_w, mut half_h) = (rect.w as f64 / 2.0 * ypp, rect.h as f64 / 2.0 * ypp);
    if turn != 0.0 {
        // The tiles under any corner of the turned square.
        let r = half_w.hypot(half_h);
        (half_w, half_h) = (r, r);
    }
    // Tile columns run east from the west edge (`32 - y / tile`), rows
    // south from the north edge (`32 - x / tile`).
    let col = |w: f64| (32.0 - w / TILE_YARDS).floor() as i64;
    let row = |n: f64| (32.0 - n / TILE_YARDS).floor() as i64;
    let (c0, c1) = (col(west + half_w), col(west - half_w));
    let (r0, r1) = (row(north + half_h), row(north - half_h));
    let size = TILE_YARDS / ypp;
    let alpha = effective_alpha(vm, f);
    // Turned tiles reach past the minimap: cut to it.
    let clip = if turn != 0.0 {
        Some(match clip_of(vm, f) {
            Some(c) => c.intersect(rect),
            None => rect,
        })
    } else {
        clip_of(vm, f)
    };
    let unit = unit_of(vm, f);
    let mask = minimap_mask(vm, f, rect);
    let (sin, cos) = turn.sin_cos();
    let mut out = Vec::new();
    for ty in r0.max(0)..=r1.min(63) {
        for tx in c0.max(0)..=c1.min(63) {
            let Some(file) =
                crate::clientdata::minimap_tile(vm, instance as u32, tx as u32, ty as u32)
            else {
                continue;
            };
            // The tile's place on screen, north up.
            let left = cx + (west - (32.0 - tx as f64) * TILE_YARDS) / ypp;
            let top = cy + (north - (32.0 - ty as f64) * TILE_YARDS) / ypp;
            let (place, texcoord, rotation) = if turn != 0.0 {
                // Its center turned clockwise about the minimap's (screen
                // y down), the tile with it.
                let (dx, dy) = (left + size / 2.0 - cx, top + size / 2.0 - cy);
                let (tcx, tcy) = (cx + dx * cos - dy * sin, cy + dx * sin + dy * cos);
                let place = Rect {
                    x: (tcx - size / 2.0) as f32,
                    y: (tcy - size / 2.0) as f32,
                    w: size as f32,
                    h: size as f32,
                };
                (
                    place,
                    [0.0, 0.0, 0.0, 1.0, 1.0, 0.0, 1.0, 1.0],
                    -turn as f32,
                )
            } else {
                // Cut to the minimap.
                let x0 = left.max(rect.x as f64);
                let x1 = (left + size).min((rect.x + rect.w) as f64);
                let y0 = top.max(rect.y as f64);
                let y1 = (top + size).min((rect.y + rect.h) as f64);
                if x1 <= x0 || y1 <= y0 {
                    continue;
                }
                let (u0, u1) = (((x0 - left) / size) as f32, ((x1 - left) / size) as f32);
                let (v0, v1) = (((y0 - top) / size) as f32, ((y1 - top) / size) as f32);
                let place = Rect {
                    x: x0 as f32,
                    y: y0 as f32,
                    w: (x1 - x0) as f32,
                    h: (y1 - y0) as f32,
                };
                (place, [u0, v0, u0, v1, u1, v0, u1, v1], 0.0)
            };
            out.push(Quad {
                widget: f.0,
                rect: place,
                image: Image::FileId(file),
                texcoord,
                atlas: None,
                atlas_texcoord: texcoord,
                color: [1.0, 1.0, 1.0, alpha],
                blend: Blend::Blend,
                desaturated: false,
                rotation,
                tile: (false, false),
                clip,
                masks: mask.iter().cloned().collect(),
                unit,
                // Whole tiles meet without gaps.
                snap: false,
                gradient: None,
            });
        }
    }
    out
}

/// A world place (north, west) on a minimap: its screen point, when within
/// the minimap's circle, as the minimap draws its terrain (its zoom, the
/// player at its center, turned with `rotateMinimap`).
fn minimap_point(
    vm: &mut Vm,
    f: WidgetId,
    rect: Rect,
    north: f64,
    west: f64,
) -> Option<(f32, f32)> {
    let (pn, pw, _, _) = crate::character::player_world(vm)?;
    let zoom = num(vm, first(vm, f, "Zoom").as_ref(), 0.0).clamp(0.0, 5.0) as usize;
    let ypp = MINIMAP_YARDS[zoom] / rect.w as f64;
    // East and south of the player, in pixels.
    let (mut dx, mut dy) = ((pw - west) / ypp, (pn - north) / ypp);
    if minimap_rotates(vm) {
        let (sin, cos) = crate::unitinfo::player_facing(vm).sin_cos();
        (dx, dy) = (dx * cos - dy * sin, dx * sin + dy * cos);
    }
    if dx.hypot(dy) > rect.w as f64 / 2.0 {
        return None;
    }
    Some((
        rect.x + rect.w / 2.0 + dx as f32,
        rect.y + rect.h / 2.0 + dy as f32,
    ))
}

/// How a minimap draws its marks: its alpha, pixels per UI unit, clip.
#[derive(Clone, Copy)]
struct MarkLook {
    alpha: f32,
    unit: f32,
    clip: Option<Rect>,
}

/// A square quad of a whole texture (or its part) centered at a point.
fn minimap_icon(
    f: WidgetId,
    at: (f32, f32),
    size: f32,
    image: Image,
    tc: [f32; 4],
    rotation: f32,
    look: MarkLook,
) -> Quad {
    let [u0, u1, v0, v1] = tc;
    let texcoord = [u0, v0, u0, v1, u1, v0, u1, v1];
    Quad {
        widget: f.0,
        rect: Rect {
            x: at.0 - size / 2.0,
            y: at.1 - size / 2.0,
            w: size,
            h: size,
        },
        image,
        texcoord,
        atlas: None,
        atlas_texcoord: texcoord,
        color: [1.0, 1.0, 1.0, look.alpha],
        blend: Blend::Blend,
        desaturated: false,
        rotation,
        tile: (false, false),
        clip: look.clip,
        masks: Vec::new(),
        unit: look.unit,
        snap: false,
        gradient: None,
    }
}

/// A minimap's group members (party or raid, with a place in the world)
/// as their class's blips (`Interface\Minimap\PartyRaidBlips`: an 8 by 4
/// grid, the classes in ID order, the solid dots first; 16 UI units,
/// unverified), within its circle.
fn minimap_blips(vm: &mut Vm, f: WidgetId) -> Vec<Quad> {
    let Some(rect) = rect_of(vm, f) else {
        return Vec::new();
    };
    let members: Vec<(f64, (f64, f64))> = {
        let u = &engine(vm).units;
        u.group
            .iter()
            .filter(|g| **g != crate::units::PLAYER)
            .filter_map(|g| {
                let m = &u.list[g.0];
                m.world.map(|w| (m.class_id, w))
            })
            .collect()
    };
    let unit = unit_of(vm, f);
    let look = MarkLook {
        alpha: effective_alpha(vm, f),
        unit,
        clip: clip_of(vm, f),
    };
    let mut out = Vec::new();
    for (class, (north, west)) in members {
        let Some(at) = minimap_point(vm, f, rect, north, west) else {
            continue;
        };
        let cell = (class.max(1.0) as u32 - 1) % 16;
        let (cx, cy) = (cell % 8, cell / 8);
        let tc = [
            cx as f32 / 8.0,
            (cx + 1) as f32 / 8.0,
            cy as f32 / 4.0,
            (cy + 1) as f32 / 4.0,
        ];
        let image = Image::File("Interface\\Minimap\\PartyRaidBlips".into());
        out.push(minimap_icon(f, at, 16.0 * unit, image, tc, 0.0, look));
    }
    out
}

/// How long a ping shows (unverified).
const PING_SECONDS: f64 = 5.0;

/// A minimap's last ping (`PingLocation`), for its first seconds: its
/// center, a ring and the turning marks (`UI-Minimap-Ping-Center`,
/// `-Expand`, `-Rotate`; their sizes and motion unverified), where in the
/// world it was (the minimap draws it as it draws the terrain).
fn minimap_ping(vm: &mut Vm, f: WidgetId) -> Vec<Quad> {
    let Some(rect) = rect_of(vm, f) else {
        return Vec::new();
    };
    let Some(Value::Table(t)) = first(vm, f, "MinimapPing") else {
        return Vec::new();
    };
    let get = |i: f64| match vm.heap.table(t).get(Value::Num(i)) {
        Value::Num(n) => n,
        _ => 0.0,
    };
    let (north, west, at) = (get(1.0), get(2.0), get(3.0));
    let age = crate::clock::now(vm) - at;
    if !(0.0..PING_SECONDS).contains(&age) {
        return Vec::new();
    }
    let Some(p) = minimap_point(vm, f, rect, north, west) else {
        return Vec::new();
    };
    let unit = unit_of(vm, f);
    let look = MarkLook {
        alpha: effective_alpha(vm, f),
        unit,
        clip: clip_of(vm, f),
    };
    let whole = [0.0, 1.0, 0.0, 1.0];
    let tex = |n: &str| Image::File(format!("Interface\\Minimap\\UI-Minimap-Ping-{n}"));
    // The ring grows each second; the marks turn once a second.
    let grow = (age.fract() as f32) * 0.5 + 0.5;
    let turn = (age.fract() * std::f64::consts::TAU) as f32;
    vec![
        minimap_icon(f, p, 32.0 * unit * grow, tex("Expand"), whole, 0.0, look),
        minimap_icon(f, p, 32.0 * unit, tex("Rotate"), whole, turn, look),
        minimap_icon(f, p, 16.0 * unit, tex("Center"), whole, 0.0, look),
    ]
}

/// The player's arrow at a minimap's center, turned to the player's
/// facing, or up when the minimap turns instead (the client's own texture,
/// 32 by 32 UI units; unverified).
fn minimap_arrow(vm: &mut Vm, f: WidgetId) -> Option<Quad> {
    let rect = rect_of(vm, f)?;
    crate::character::player_world(vm)?;
    let rotation = if minimap_rotates(vm) {
        0.0
    } else {
        crate::unitinfo::player_facing(vm) as f32
    };
    let unit = unit_of(vm, f);
    let size = 32.0 * unit;
    let texcoord = [0.0, 0.0, 0.0, 1.0, 1.0, 0.0, 1.0, 1.0];
    Some(Quad {
        widget: f.0,
        rect: Rect {
            x: rect.x + (rect.w - size) / 2.0,
            y: rect.y + (rect.h - size) / 2.0,
            w: size,
            h: size,
        },
        image: Image::File("Interface\\Minimap\\MinimapArrow".into()),
        texcoord,
        atlas: None,
        atlas_texcoord: texcoord,
        color: [1.0, 1.0, 1.0, effective_alpha(vm, f)],
        blend: Blend::Blend,
        desaturated: false,
        rotation,
        tile: (false, false),
        clip: clip_of(vm, f),
        masks: Vec::new(),
        unit,
        snap: true,
        gradient: None,
    })
}

/// A unit position frame's player pin (the world map's arrow): the pin
/// `AddUnit("player", asset, width, height, …, showFacing)` gave it
/// (`UI-WorldMapArrow`), at the player's place on its map (`SetUiMapID`)
/// across the frame, turned as the player faces when `showFacing`; drawn
/// by the client, no region of the frame's. Unverified: other units' pins.
fn unit_position_player(vm: &mut Vm, f: WidgetId) -> Option<Quad> {
    let rect = rect_of(vm, f)?;
    let map = num(vm, first(vm, f, "UiMapID").as_ref(), 0.0);
    let (x, y) = crate::character::player_position_on(vm, map)?;
    let pin = args(vm, f, "Unit:player")?;
    let texture = pin.first().and_then(|v| text_of(vm, *v))?;
    let unit = unit_of(vm, f);
    let size = num(vm, pin.get(1), 24.0) as f32 * unit;
    let facing = pin.get(8).is_none_or(|v| v.truthy());
    let (image, texcoord) = match crate::measures::atlas(vm, &texture).and_then(|a| a.info.as_ref())
    {
        Some(info) => {
            let c = info.coords;
            let (l, r, t, b) = (c[0] as f32, c[1] as f32, c[2] as f32, c[3] as f32);
            (Image::FileId(info.file as u32), [l, t, l, b, r, t, r, b])
        }
        None => (
            Image::File(texture),
            [0.0, 0.0, 0.0, 1.0, 1.0, 0.0, 1.0, 1.0],
        ),
    };
    Some(Quad {
        widget: f.0,
        rect: Rect {
            x: rect.x + x as f32 * rect.w - size / 2.0,
            y: rect.y + y as f32 * rect.h - size / 2.0,
            w: size,
            h: size,
        },
        image,
        texcoord,
        atlas: None,
        atlas_texcoord: texcoord,
        color: [1.0, 1.0, 1.0, effective_alpha(vm, f)],
        blend: Blend::Blend,
        desaturated: false,
        rotation: if facing {
            crate::unitinfo::player_facing(vm) as f32
        } else {
            0.0
        },
        tile: (false, false),
        clip: clip_of(vm, f),
        masks: Vec::new(),
        unit,
        snap: true,
        gradient: None,
    })
}

pub fn build(vm: &mut Vm) -> Scene {
    let effects = crate::animations::effects(vm);
    EFFECTS.with(|e| *e.borrow_mut() = effects);
    let scene = widgets::with_scene_bounds(vm, |vm| build_scene(vm, true));
    // (a debug build, or WOWLUA_SCENE_CHECK: the kept items checked
    // against the scene laid out anew, T542)
    static CHECK: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    let check = *CHECK.get_or_init(|| std::env::var_os("WOWLUA_SCENE_CHECK").is_some());
    if cfg!(debug_assertions) || check {
        let fresh = widgets::with_scene_bounds(vm, |vm| build_scene(vm, false));
        check_kept(vm, &scene, &fresh);
    }
    EFFECTS.with(|e| e.borrow_mut().clear());
    widgets::scene_built(crate::engine_mut(vm));
    scene
}

/// Panics where the scene of kept items differs from the one laid out
/// anew: a change that marked nothing (decision T542).
fn check_kept(vm: &Vm, kept: &Scene, fresh: &Scene) {
    if kept == fresh {
        return;
    }
    let widget = |i: &Item| match i {
        Item::Quad(q) => q.widget,
        Item::Text(t) => t.widget,
        Item::Model(m) => m.widget,
    };
    let at = kept
        .items
        .iter()
        .zip(&fresh.items)
        .position(|(a, b)| a != b)
        .unwrap_or(kept.items.len().min(fresh.items.len()));
    let describe = |s: &Scene| match s.items.get(at) {
        Some(i) => match engine(vm).world.widgets.get(widget(i)) {
            Some(w) => format!("{} {:?}: {i:?}", w.class, w.name),
            None => format!("{i:?}"),
        },
        None => "nothing".into(),
    };
    panic!(
        "the scene's kept items differ from its items laid out anew (a change that \
         marked nothing, T542) at item {at} of {} and {}:\nkept: {}\nanew: {}",
        kept.items.len(),
        fresh.items.len(),
        describe(kept),
        describe(fresh)
    );
}

fn build_scene(vm: &mut Vm, keep: bool) -> Scene {
    let (width, height) = {
        let s = &engine(vm).screen;
        (s.width.round() as u32, s.height.round() as u32)
    };
    let hover = engine(vm).world.input.mouse_focus;
    let mut all = Vec::new();
    let epoch = engine(vm).world.scene_epoch;
    // (the frames' order as the last scene found it, while no frame was
    // shown, hidden, made, reparented or re-layered: T543)
    let kept_order = if keep && engine(vm).world.scene_order_valid {
        let kept = engine(vm).scene_kept.borrow();
        kept.order.clone().filter(|_| kept.epoch == epoch)
    } else {
        None
    };
    let (order, merged) = match kept_order {
        Some(o) => o,
        None => {
            let order = frames_in_order(vm);
            // A frame on its parent's level (`useParentLevel`) is drawn
            // with its parent: their regions sorted together by layer and
            // sublevel (the gossip window's parchment, its own
            // `BACKGROUND`, shows over its inset's marble, the inset's
            // `BACKGROUND` -5, as in game).
            let merged: std::collections::HashSet<usize> = order
                .iter()
                .filter(|f| merges_into_parent(vm, **f))
                .map(|f| f.0)
                .collect();
            if keep {
                engine(vm).scene_kept.borrow_mut().order = Some((order.clone(), merged.clone()));
                crate::engine_mut(vm).world.scene_order_valid = true;
            }
            (order, merged)
        }
    };
    let mut old = if keep {
        let mut kept = engine(vm).scene_kept.borrow_mut();
        if kept.epoch != epoch {
            kept.groups.clear();
            kept.epoch = epoch;
        }
        std::mem::take(&mut kept.groups)
    } else {
        Default::default()
    };
    let mut new: rustc_hash::FxHashMap<usize, Group> = Default::default();
    let mut over_units: Vec<std::ops::Range<usize>> = Vec::new();
    let plates: Vec<usize> = engine(vm)
        .units
        .nameplate_frames
        .iter()
        .map(|f| f.0)
        .collect();
    for f in order {
        if merged.contains(&f.0) {
            continue;
        }
        let (children, class) = {
            let w = &engine(vm).world.widgets[f.0];
            (w.children.clone(), w.class.clone())
        };
        // The frame and the frames drawn with it, the frame first.
        let mut group = vec![f];
        let mut k = 0;
        while k < group.len() {
            let kids = engine(vm).world.widgets[group[k].0].children.clone();
            group.extend(kids.into_iter().filter(|c| merged.contains(&c.0)));
            k += 1;
        }
        let children: Vec<(usize, WidgetId)> = std::iter::once(children)
            .chain(
                group[1..]
                    .iter()
                    .map(|g| engine(vm).world.widgets[g.0].children.clone()),
            )
            .enumerate()
            .flat_map(|(i, cs)| cs.into_iter().map(move |c| (i, c)))
            .collect();
        // (a nameplate's items stand over its unit: T546)
        let start = all.len();
        let on_plate = !plates.is_empty() && {
            let mut cur = Some(f);
            let mut found = false;
            while let Some(c) = cur {
                if plates.contains(&c.0) {
                    found = true;
                    break;
                }
                cur = engine(vm).world.widgets[c.0].parent;
            }
            found
        };
        // (its kept items, while nothing they show changed: T542)
        let members: Vec<usize> = group
            .iter()
            .map(|g| g.0)
            .chain(children.iter().map(|(_, c)| c.0))
            .collect();
        let hover_at = group.iter().position(|g| Some(*g) == hover);
        if keep
            && let Some(g) = old.remove(&f.0)
            && g.members == members
            && g.hover == hover_at
            && !members
                .iter()
                .any(|&m| widgets::scene_changed(engine(vm), m))
        {
            all.extend(g.items.iter().cloned());
            if on_plate && all.len() > start {
                over_units.push(start..all.len());
            }
            new.insert(f.0, g);
            continue;
        }
        let mut items = Vec::new();
        VOLATILE.with(|v| v.set(false));
        // A message frame's messages are laid out as its lines.
        let messages: Vec<usize> = engine(vm)
            .world
            .messages
            .get(&f.0)
            .map(|l| {
                l.iter()
                    .filter_map(|(_, v)| widgets::widget_of(vm, *v))
                    .map(|w| w.0)
                    .collect()
            })
            .unwrap_or_default();
        let mut regions: Vec<(usize, i32, bool, bool, usize, usize)> = Vec::new();
        for (owner, c) in children {
            let w = &engine(vm).world.widgets[c.0];
            if !w.shown || !is_region(vm, c) || messages.contains(&c.0) {
                continue;
            }
            let (layer, sub) = layer_of(vm, c);
            let props = engine(vm).world.widgets[group[owner].0].props;
            if let Some(Value::Num(disabled)) =
                widgets::stored_arg(vm, props, "DisabledDrawLayers", 0)
                && (disabled as u32) & (1 << layer) != 0
            {
                continue;
            }
            // The highlight layer while the mouse is over the frame.
            if layer == 4 && hover != Some(group[owner]) {
                continue;
            }
            // Within a layer and sublevel, masked textures after the others
            // (the action buttons' masked icons show over their slot art,
            // created after them; unverified beyond that).
            let masked = !masks_of(vm, c).is_empty();
            // Font strings after the textures of their layer and sublevel
            // (the Settings panel's title, a font string on its border
            // frame's `OVERLAY`, shows over the border's top edge, made
            // later on the same layer; a screenshot, 2026-10-23).
            let text = widgets::kind(engine(vm), c) & widgets::KIND_FONT_STRING != 0;
            regions.push((layer, sub, text, masked, owner, c.0));
        }
        regions.sort();
        // A minimap's terrain under its regions, its player arrow over them.
        let minimap = widgets::is_a(engine(vm), &class, "Minimap");
        if minimap {
            items.extend(minimap_tiles(vm, f).into_iter().map(Item::quad));
        }
        for (_, _, text, _, _, r) in regions {
            let id = WidgetId(r);
            if text {
                if let Some(mut t) = text_item(vm, id, false) {
                    let images = std::mem::take(&mut t.images);
                    items.push(Item::text(t));
                    items.extend(images.into_iter().map(Item::quad));
                }
            } else if widgets::kind(engine(vm), id) & widgets::KIND_MASK != 0 {
                continue;
            } else if let Some(q) = quad_of(vm, id) {
                items.push(Item::quad(q));
            }
        }
        if minimap {
            let marks = minimap_blips(vm, f).into_iter().chain(minimap_ping(vm, f));
            items.extend(marks.map(Item::quad));
            if let Some(q) = minimap_arrow(vm, f) {
                items.push(Item::quad(q));
            }
        }
        for &g in &group {
            let class = engine(vm).world.widgets[g.0].class.clone();
            // A model frame's model over its regions (unverified: the
            // client's order of a model and its frame's own regions).
            if let Some(m) = model_item(vm, g, &class) {
                items.push(Item::Model(m));
            }
            if widgets::is_a(engine(vm), &class, "EditBox")
                && let Some(t) = text_item(vm, g, true)
            {
                items.push(Item::text(t));
            }
            if widgets::is_a(engine(vm), &class, "UnitPositionFrame")
                && let Some(q) = unit_position_player(vm, g)
            {
                items.push(Item::quad(q));
            }
            if g == f && !messages.is_empty() {
                for mut t in message_items(vm, f) {
                    let images = std::mem::take(&mut t.images);
                    items.push(Item::text(t));
                    items.extend(images.into_iter().map(Item::quad));
                }
            }
        }
        // (a minimap, a model, a message frame or a unit's place change by
        // themselves: laid out each frame)
        let volatile = VOLATILE.with(std::cell::Cell::get)
            || minimap
            || !messages.is_empty()
            || items.iter().any(|i| matches!(i, Item::Model(_)))
            || group.iter().any(|g| {
                widgets::is_a(
                    engine(vm),
                    &engine(vm).world.widgets[g.0].class,
                    "UnitPositionFrame",
                )
            });
        all.extend(items.iter().cloned());
        if on_plate && all.len() > start {
            over_units.push(start..all.len());
        }
        if keep && !volatile {
            new.insert(
                f.0,
                Group {
                    members,
                    hover: hover_at,
                    items,
                },
            );
        }
    }
    if keep {
        engine(vm).scene_kept.borrow_mut().groups = new;
    }
    let mut items = all;
    let world = world_view(vm);
    // (the names over the units, behind the UI, T534)
    if world.is_some() {
        let names = crate::unitnames::items(vm, f64::from(width), f64::from(height));
        let n = names.len();
        items.splice(0..0, names);
        for r in &mut over_units {
            *r = r.start + n..r.end + n;
        }
        if n > 0 {
            over_units.insert(0, 0..n);
        }
    }
    Scene {
        width,
        height,
        items,
        world,
        over_units,
    }
}

/// The topmost visible mouse-enabled frame at a point (screen pixels from
/// the top left), as a click there finds it.
pub fn hit_test(vm: &mut Vm, x: f64, y: f64) -> Option<WidgetId> {
    let order = frames_in_order(vm);
    order.into_iter().rev().find(|&f| {
        if !mouse_enabled(vm, f) {
            return false;
        }
        let Some(r) = rect_of(vm, f) else {
            return false;
        };
        let inside = r.contains(x as f32, y as f32);
        let clipped = clip_of(vm, f).is_some_and(|c| !c.contains(x as f32, y as f32));
        inside && !clipped
    })
}

/// The widget drawn topmost at a point of `scene` (screen pixels from the
/// top left): a region or a frame, whatever draws there, taking the mouse
/// or not (an inspector's pick, decision T484).
pub fn drawn_at(scene: &Scene, x: f64, y: f64) -> Option<WidgetId> {
    let (x, y) = (x as f32, y as f32);
    scene.items.iter().rev().find_map(|item| {
        let (widget, rect, clip) = match item {
            Item::Quad(q) => (q.widget, q.rect, q.clip),
            Item::Text(t) => (t.widget, t.rect, t.clip),
            Item::Model(m) => (m.widget, m.rect, m.clip),
        };
        let inside = rect.contains(x, y) && clip.is_none_or(|c| c.contains(x, y));
        inside.then_some(WidgetId(widget))
    })
}

/// A widget's alpha with its parents' (as drawn).
pub fn alpha_of(vm: &Vm, id: WidgetId) -> f64 {
    f64::from(effective_alpha(vm, id))
}

/// A region's draw layer and sublevel (`GetDrawLayer`).
pub fn draw_layer(vm: &Vm, id: WidgetId) -> (&'static str, i32) {
    let (rank, sublevel) = layer_of(vm, id);
    (LAYERS[rank.min(LAYERS.len() - 1)], sublevel)
}

/// Whether a frame takes the mouse (a click finds it).
pub fn takes_mouse(vm: &Vm, id: WidgetId) -> bool {
    mouse_enabled(vm, id)
}

/// The rectangle that holds a frame and everything drawn of it (a
/// screenshot of one frame).
pub fn extent(vm: &Vm, id: WidgetId) -> Option<Rect> {
    let mut acc: Option<Rect> = rect_of(vm, id);
    let mut stack = engine(vm).world.widgets[id.0].children.clone();
    while let Some(c) = stack.pop() {
        if !engine(vm).world.widgets[c.0].shown {
            continue;
        }
        if let Some(r) = rect_of(vm, c) {
            acc = Some(match acc {
                None => r,
                Some(a) => {
                    let x0 = a.x.min(r.x);
                    let y0 = a.y.min(r.y);
                    let x1 = (a.x + a.w).max(r.x + r.w);
                    let y1 = (a.y + a.h).max(r.y + r.h);
                    Rect {
                        x: x0,
                        y: y0,
                        w: x1 - x0,
                        h: y1 - y0,
                    }
                }
            });
        }
        stack.extend(engine(vm).world.widgets[c.0].children.iter().copied());
    }
    acc
}
