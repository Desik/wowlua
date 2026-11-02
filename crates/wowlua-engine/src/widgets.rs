//! Widgets (spec emulator §17.6): frames, textures and font strings as the
//! client makes them. A widget is a Lua table holding its object (a
//! userdata) at `[0]`, whose metatable's `__index` is its class's method
//! table: the documented methods of the class and its bases, as natives.
//! A method runs its behaviour here, stores what a setter without one was
//! given (its getter returns it), does nothing if it returns nothing, or
//! fails as not mocked.
//!
//! As `/stdtest engine` recorded on WoW Forever 1.60.1 (fixture
//! `tests/client/forever-camelot-engine.lua`, checked by `wowlua-cli`'s
//! `tests/engine_probe.rs`): events reach frames in the order they first
//! registered one;
//! `OnShow` / `OnHide` run children first; `SetScript` clears the hooks;
//! each class has its own method functions; methods check `self` and their
//! documented arguments first (counting `self` as #1, usage `self:Name(…)`,
//! `number` a C float); the client's error messages, with a "Lua Taint:"
//! line where it adds one; `Is…` getters are false until set (a button
//! takes the mouse); alpha keeps eight bits. A template only Blizzard's UI
//! code defines warns while that code is not loaded; any other unknown
//! name fails (decided 2026-10-16). Unverified: `OnSizeChanged` runs with
//! the next frame.

use std::collections::HashMap;

use wowlua_vm::debug::DispatchKind;
use wowlua_vm::heap::Userdata;
use wowlua_vm::stdlib::Ret;
use wowlua_vm::{LuaError, NativeCall, TableRef, Value, Vm};

use crate::{Engine, engine, engine_mut};

/// A widget's object payload: its index in [`World::widgets`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct WidgetId(pub usize);

/// A script object's userdata payload (a curve, a numeric formatter, a
/// duration): the widget it is. Its own type, so a frame's `[0]` is not
/// taken for one.
#[derive(Clone, Copy, Debug)]
pub struct ScriptObject(pub WidgetId);

/// One anchor: `point`, the frame it is relative to, its point, offsets.
#[derive(Clone, Debug)]
pub struct Point {
    pub point: String,
    pub relative: Option<Value>,
    pub relative_point: String,
    pub x: f64,
    pub y: f64,
}

pub struct Widget {
    /// The class whose methods it has (`Frame`, `Button`, `FontString`).
    pub class: String,
    /// Its class's kind, found once ([`kind`]): 0 not yet.
    pub kind: std::cell::Cell<u8>,
    /// The Lua object (a script object's: the engine's own record, its
    /// userdata is [`Widget::object`]).
    pub table: TableRef,
    /// A script object's userdata: what Lua gets.
    pub object: Option<wowlua_vm::heap::UdRef>,
    /// Its scripts (`OnEvent` → function) and hooks (`OnEvent` → list), in
    /// Lua tables the registry keeps.
    pub scripts: TableRef,
    pub hooks: TableRef,
    /// Values setters without a behaviour stored, by method name's stem.
    pub props: TableRef,
    pub name: Option<String>,
    pub parent: Option<WidgetId>,
    pub children: Vec<WidgetId>,
    pub shown: bool,
    /// Registered events, with the units of unit events.
    pub events: HashMap<String, Option<Vec<String>>>,
    /// `RegisterAllEvents`: when (every event comes; the combat log's not,
    /// restricted).
    pub all_events: Option<u64>,
    /// When it first registered an event, `RegisterAllEvents` too (its
    /// place in the dispatch order: 0 never).
    pub first_registration: u64,
    /// `IsProtected`'s second result: protected by a secure template.
    pub explicitly_protected: bool,
    /// Made by Blizzard's secure code (`UseSecureEnvironment`): addon code
    /// may not use it (WoW Forever's `StoreFrame`).
    pub forbidden: bool,
    /// Its forbidden aspects (`Enum.ForbiddenAspect` flags) from its
    /// templates' `<ForbiddenAspects>` and secure `AddForbiddenAspects`
    /// calls (`crate::protection::forbidden_aspects`).
    pub forbidden_aspects: u32,
    /// Has an intrinsic type's handlers (`intrinsic_key`).
    pub intrinsic_scripts: bool,
    pub points: Vec<Point>,
    pub width: f64,
    pub height: f64,
    pub scale: f64,
    /// `SetIgnoreParentScale(true)`: its effective scale is its own.
    pub ignore_parent_scale: bool,
    pub alpha: f64,
    /// Set from a secret (its alpha aspect secret: `GetAlpha` too).
    pub secret_alpha: bool,
    pub text: Option<Value>,
    pub attributes: TableRef,
    pub protected: bool,
    pub moving: bool,
    pub sizing: bool,
    pub enabled: bool,
    pub resize_bounds: Option<(f64, f64, f64, f64)>,
    /// Created by `CreateFrame` (not a region or another script object).
    pub frame: bool,
    /// The scroll frame this is the scroll child of (`SetScrollChild`): it
    /// places the child.
    pub scroll_parent: Option<WidgetId>,
    /// The Lua line that made it: chunk name and line (an inspector's
    /// "created at", decision T484); none from the client's own loading.
    pub created: Option<(std::rc::Rc<[u8]>, u32)>,
}

/// A line a chat window showed (decision T514).
#[derive(Clone, Debug, PartialEq)]
pub struct ChatLine {
    /// Its number in the session, from 0.
    pub seq: u64,
    /// The chat window: `ChatFrame1`, ….
    pub frame: String,
    pub text: String,
    /// Its colour, when `AddMessage` gave one.
    pub color: Option<[f64; 3]>,
}

/// How many chat lines a session keeps.
pub const CHAT_LOG_LINES: usize = 1000;

/// The widgets, and each class's method table and metatable.
#[derive(Default)]
pub struct World {
    pub widgets: Vec<Widget>,
    /// UIParent (made at install): the units the engine lays regions out
    /// in, and the scale of the screen (decision T237).
    pub ui_parent: Option<WidgetId>,
    /// Loose files' IDs given out (`GetFileIDFromPath`), by normalized path.
    pub file_ids: std::collections::HashMap<String, f64>,
    /// The NPC interactions open (`Enum.PlayerInteractionType`, as bits),
    /// from `PLAYER_INTERACTION_MANAGER_FRAME_SHOW`/`_HIDE`.
    pub interactions: Vec<u64>,
    /// The widgets given an `OnUpdate` (set, hooked or from XML) or an
    /// intrinsic type's handlers, ever: the ones a frame looks at, each
    /// checked as its turn comes (most of the UI's widgets never update);
    /// in order, each once (`World::updates`).
    pub updating: Vec<usize>,
    /// The widgets ever given an `OnSizeChanged` script or hook: the ones
    /// a layout change looks at (decision T541), in order, each once.
    pub size_watching: Vec<usize>,
    /// The widgets changed since the scene was last built, by widget (a
    /// widget made since counts as changed; decision T542).
    pub scene_marks: Vec<bool>,
    /// Bumped when what every text or region shows may have changed (a
    /// font object, the screen): the scene keeps nothing across it.
    pub scene_epoch: u64,
    /// By widget, the widgets anchored to it or masked by it: they move or
    /// change with it (decision T542; some may be no longer).
    pub anchored_by: Vec<Vec<usize>>,
    /// The scene's order of frames still holds: no frame was shown,
    /// hidden, made, reparented or re-layered since (decision T543).
    pub scene_order_valid: bool,
    /// The widgets that ever registered for events: the ones an event
    /// looks at.
    pub listening: std::collections::BTreeSet<usize>,
    /// The duration text bindings made (`C_DurationUtil.
    /// CreateDurationTextBinding`): each frame looks at them.
    pub text_bindings: Vec<WidgetId>,
    /// The status bars following a duration (`SetTimerDuration`): each
    /// frame moves their values.
    pub timer_bars: Vec<WidgetId>,
    classes: HashMap<String, TableRef>,
    /// Templates and frame types tests (or stand-ins for Blizzard's UI
    /// code) define: name → (base class, function applied to the frame),
    /// the functions kept in the registry.
    pub templates: HashMap<String, Value>,
    pub frame_types: HashMap<String, (String, Value)>,
    /// The last event registration's number.
    pub next_registration: u64,
    /// Widgets whose size changed since the last frame (`OnSizeChanged`
    /// runs with the next frame, not at `SetSize`).
    pub resized: Vec<WidgetId>,
    /// The same widgets, by index: whether one is already among them.
    pub resizing: std::collections::HashSet<usize>,
    /// A widget's points, size, scale or parent changed since the last
    /// frame: the frame looks for sizes that followed (`OnSizeChanged` of
    /// a frame sized by its anchors).
    pub layout_changed: bool,
    /// The laid-out size each anchored widget with an `OnSizeChanged`
    /// handler last had, by index.
    pub laid_out: HashMap<usize, (f64, f64)>,
    /// The widgets a keyboard setting (`KeyboardEnabled`) was ever stored
    /// for, in order, each once: the only ones that can take keys
    /// (`input::keyboard_frames`).
    pub keyboard: Vec<usize>,
    /// Edit boxes Lua set the text of this frame: `OnTextChanged(false)`
    /// runs once each after the frame's `OnUpdate`, not at `SetText`, with
    /// the last text, also when it did not change, the last box set first
    /// (`/stdtest engine`; unverified: by setting or making order).
    pub text_set: Vec<WidgetId>,
    /// A color select's color: red, green, blue, alpha.
    pub colors: HashMap<usize, [f64; 4]>,
    /// A color select's hue, saturation and value as `SetColorHSV` gave
    /// them (until a color is set by its red, green and blue).
    pub color_hsv: HashMap<usize, [f64; 3]>,
    /// A slider's or a status bar's range and value.
    pub bars: HashMap<usize, crate::bars::Bar>,
    /// The loot window.
    pub loot: crate::loot::Loot,
    pub quests: crate::quests::Quests,
    pub gossip: crate::gossip::Gossip,
    pub trainers: crate::trainer::Trainers,
    pub taxis: crate::taxi::Taxis,
    pub death: crate::death::Death,
    /// The instance the player is in and its encounter (`encounters.rs`).
    pub encounters: crate::encounters::Encounters,
    /// The player's pet's state (`pets.rs`).
    pub pets: crate::pets::Pets,
    /// The class's hidden group buffs (`HIDDEN_GROUP_BUFFS_CHANGED`).
    pub hidden_group_buffs: Option<Vec<f64>>,
    /// The player rests (an inn, a city: `setResting`).
    pub resting: bool,
    /// The realm is a Classic Era hardcore realm (`setHardcore`).
    pub hardcore: bool,
    /// The session ended (`ReloadUI`, logging out).
    pub logged_out: bool,
    /// A loading screen up, the world not entered yet: no map.
    pub loading_screen: bool,
    /// A loading screen's inventory, not loaded yet.
    pub inventory_loading: bool,
    /// The server's blocks of a loading screen's inventory, in its order
    /// (`setInventoryLoad`).
    pub inventory_load: Option<Vec<String>>,
    /// The player on a flight.
    pub on_taxi: bool,
    /// The sounds played (`C_Sound.PlaySound`'s handles).
    pub sound_handles: f64,
    /// The weather (decision T565): its kind (`Enum.WeatherType`: 0 clear,
    /// 1 rain, 2 snow, 3 sandstorm) and intensity (0 to 1).
    pub weather: (u32, f64),
    /// The mailboxes, the inbox, the letter being written.
    pub mail: crate::mail::Mail,
    /// The bankers, the character's bank tabs.
    pub bank: crate::bank::Bank,
    /// The auction houses, their searches, the player's auctions.
    pub auction: crate::auction::AuctionHouse,
    /// The player's party, a trade.
    pub party: crate::party::Party,
    /// What inspecting other players found.
    pub inspect: crate::inspect::Inspect,
    /// The trait configs' ranks (talents, the legacy trees) and renown.
    pub traits: crate::traits::Traits,
    /// The achievement search's results.
    pub achievements: crate::achievements::Achievements,
    /// The profession window open, the craft running.
    pub tradeskill: crate::tradeskill::TradeSkill,
    /// The global cooldown running: its start (`GetTime`) and length.
    pub global_cooldown: Option<(f64, f64)>,
    /// Spells by trait (`setSpellTraits`: `offGlobalCooldown`,
    /// `tracking`, `profession`).
    pub spell_traits: std::collections::BTreeMap<String, std::collections::BTreeSet<i64>>,
    /// A flight's stage: 0 none, 1 taking off (no control), 2 on the taxi.
    pub taxi_stage: u8,
    /// The player mounted (`IsMounted`; a taxi's mount).
    pub mounted: bool,
    /// The levels a test defined (`levels.rs`).
    pub levels: std::collections::BTreeMap<i64, wowlua_vm::TableRef>,
    /// The chat's last line number (each message's eleventh value).
    pub next_chat_line: u64,
    /// The time a request takes to come back from the server
    /// (`mock.setLatency`; `None`: 0.4 s).
    pub latency: Option<f64>,
    /// When the profile's snapshot was taken on the engine's clock
    /// (`snapshotTaken`): its cooldowns' time left counts from then.
    pub snapshot_at: Option<f64>,
    /// The slider whose thumb the user drags.
    pub dragging_thumb: Option<WidgetId>,
    /// Edit Mode's saved layouts and active one.
    pub edit_mode: crate::editmode::EditMode,
    /// A message frame's messages: its message ID (if given) and the font
    /// string showing it, oldest first.
    pub messages: HashMap<usize, Vec<(Option<f64>, Value)>>,
    /// What the chat windows (`ChatFrame1`, …) showed, oldest first, the
    /// last [`CHAT_LOG_LINES`] (a session's chat, decision T514).
    pub chat_log: std::collections::VecDeque<ChatLine>,
    /// The next chat log line's number.
    pub chat_log_next: u64,
    /// The camera (decision T520).
    pub camera: crate::camera::Camera,
    /// The action slots whose range is told (`EnableActionRangeCheck`),
    /// each with what was told last: in range, checks range (T519).
    pub range_checks: std::collections::BTreeMap<i64, (bool, bool)>,
    /// The chat windows whose `AddMessage` a hook logs (Blizzard's, a Lua
    /// mixin's; T514).
    pub chat_hooked: std::collections::HashSet<usize>,
    /// Warnings for gaps that would make every test fail if they were errors
    /// (templates without Blizzard's UI code).
    pub warnings: Vec<String>,
    /// Blizzard's UI code, when loaded: its templates and frame types.
    pub ui: crate::ui::Registry,
    /// Event callbacks (`RegisterEventCallback`), in registration order.
    pub callbacks: Vec<crate::framescript::Callback>,
    /// CVars: name and value, as strings.
    pub cvars: Cvars,
    /// The floors found under places of the world (map, north, west, up
    /// in centimeters): the world's geometry stays, so a place's floor
    /// does (decision T538).
    pub floors: HashMap<(i64, i64, i64, i64), Option<f64>>,
    /// The CVars' defaults (the client's, for camelot), in its order.
    pub cvar_defaults: Vec<(String, String)>,
    /// Whether `PLAYER_LOGIN` fired.
    pub logged_in: bool,
    /// The events told, while a host keeps a log of them (`eventlog`).
    pub event_log: Option<crate::eventlog::EventLog>,
    /// Addons `ADDON_LOADED` fired for, lower-case.
    pub loaded_addons: std::collections::HashSet<String>,
    /// Spells and items the test defined.
    pub data: crate::gamedata::GameData,
    /// Addon messages and the guild.
    pub chat: crate::chat::Chat,
    /// Keybindings and the addons a test defined.
    pub bindings: crate::bindings::Bindings,
    /// The player's money, equipment, bags and place.
    pub character: crate::character::Character,
    pub merchant: crate::merchant::Merchant,
    /// The mouse's and the keyboard's focus.
    pub input: crate::input::Input,
    /// The directions the player holds (decision T516).
    pub movement: crate::movement::Movement,
    /// The group finder: the player's listing, the searches (`lfg.rs`).
    pub lfg: crate::lfg::Lfg,
    /// The collections a test set (`collected.rs`).
    pub collected: crate::collected::Collected,
    /// The encounter timeline's script events (`timeline.rs`).
    pub timeline: crate::timeline::Timeline,
    /// The chat windows' settings set since login (`chatwindows.rs`).
    pub chat_windows: crate::chatwindows::ChatWindows,
}

impl World {
    /// Widget `i` among those a frame looks at (`updating`).
    pub fn updates(&mut self, i: usize) {
        insert_sorted(&mut self.updating, i);
    }
}

/// `i` in a list kept in order, once.
fn insert_sorted(list: &mut Vec<usize>, i: usize) {
    if let Err(at) = list.binary_search(&i) {
        list.insert(at, i);
    }
}

/// Keeps one of the engine's Lua values alive (in a registry table).
pub fn keep(vm: &mut Vm, v: Value) {
    let registry = vm.registry;
    let key = vm.heap.intern(b"wowlua.engine");
    let list = match vm.heap.table(registry).get_str(key) {
        Value::Table(t) => t,
        _ => {
            let t = vm.new_table();
            vm.heap.table_mut(registry).set_str(key, Value::Table(t));
            t
        }
    };
    let n = vm.heap.table(list).border() as f64 + 1.0;
    let _ = vm.heap.table_mut(list).set(Value::Num(n), v);
}

/// Keeps `v` alive as `name`'s, in place of what was kept as it before (a
/// value the engine replaces: Edit Mode's saved layouts).
pub fn keep_as(vm: &mut Vm, name: &str, v: Value) {
    let registry = vm.registry;
    let key = vm.heap.intern(b"wowlua.engine.named");
    let named = match vm.heap.table(registry).get_str(key) {
        Value::Table(t) => t,
        _ => {
            let t = vm.new_table();
            vm.heap.table_mut(registry).set_str(key, Value::Table(t));
            t
        }
    };
    let k = vm.str(name);
    let _ = vm.heap.table_mut(named).set(k, v);
}

/// An error the client's C code raises: no position, and with `taint_line`
/// a "Lua Taint: <addon>" line while execution is tainted (as recorded by
/// `/stdtest engine`), naming `Vm::error_taint`'s addon.
pub fn c_error(vm: &mut Vm, msg: &str, taint_line: bool) -> LuaError {
    let mut text = msg.to_string();
    if taint_line && let Some(name) = vm.taint_name(vm.error_taint()) {
        text.push_str(&format!("\nLua Taint: {name}"));
    }
    let v = vm.str(&text);
    vm.raise(v)
}

/// The widget a value is, if it is one.
pub fn widget_of(vm: &Vm, v: Value) -> Option<WidgetId> {
    if let Value::Userdata(u) = v {
        return vm
            .heap
            .userdata(u)
            .data
            .downcast_ref::<ScriptObject>()
            .map(|o| o.0);
    }
    let Value::Table(t) = v else { return None };
    let Value::Userdata(u) = vm.heap.table(t).get(Value::Num(0.0)) else {
        return None;
    };
    vm.heap.userdata(u).data.downcast_ref::<WidgetId>().copied()
}

/// The class `name` and its bases, nearest first.
pub(crate) fn lineage(e: &Engine, name: &str) -> std::rc::Rc<[String]> {
    if let Some(known) = e.lineages.borrow().get(name) {
        return known.clone();
    }
    let out: std::rc::Rc<[String]> = lineage_of(e, name).into();
    e.lineages
        .borrow_mut()
        .insert(name.to_string(), out.clone());
    out
}

fn lineage_of(e: &Engine, name: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut todo = vec![name.to_string()];
    while let Some(c) = todo.pop() {
        if out.contains(&c) {
            continue;
        }
        if let Some(class) = e.api.classes.get(&c) {
            todo.extend(class.bases.iter().rev().cloned());
        }
        out.push(c);
    }
    out
}

/// The class a name means in any case: the annotations also declare
/// case variants (`alpha`, `ALPHA`) deriving from the real one (`Alpha`).
pub fn canonical_class(e: &Engine, name: &str) -> Option<String> {
    e.api
        .classes
        .iter()
        .filter(|(k, _)| k.eq_ignore_ascii_case(name))
        .find(|(k, c)| !c.bases.iter().any(|b| b.eq_ignore_ascii_case(k) && b != *k))
        .map(|(k, _)| k.clone())
}

/// Whether class `name` is a script object the client makes a userdata.
pub fn is_userdata_class(e: &Engine, name: &str) -> bool {
    lineage(e, name)
        .iter()
        .any(|c| e.api.classes.get(c).is_some_and(|c| c.userdata))
}

/// The Lua value widget `id` is: its table, or a script object's
/// userdata.
pub fn value_of(e: &Engine, id: WidgetId) -> Value {
    let w = &e.world.widgets[id.0];
    match w.object {
        Some(u) => Value::Userdata(u),
        None => Value::Table(w.table),
    }
}

/// Whether class `name` is or derives from `base` (`IsObjectType`).
pub fn is_a(e: &Engine, name: &str, base: &str) -> bool {
    lineage(e, name)
        .iter()
        .any(|c| c.eq_ignore_ascii_case(base))
}

/// Methods the documentation or the declarations give a class whose
/// client list was not recorded, that the client's class lacks (camelot).
/// SuuUI probe minimap: the Minimap's metatable's methods about textures,
/// blips, the player, icons, arrows, POIs, zoom, pings, rotation and masks
/// are 20, none of these among them.
const ABSENT_METHODS: &[(&str, &str)] = &[
    ("Minimap", "SetBlipTexture"),
    ("Minimap", "SetCorpsePOIArrowTexture"),
    ("Minimap", "SetIconTexture"),
    ("Minimap", "SetPOIArrowTexture"),
    ("Minimap", "SetPlayerTexture"),
    ("Minimap", "SetQuestBlobOutsideSelectedTexture"),
    ("Minimap", "SetStaticPOIArrowTexture"),
    ("Minimap", "SetTaskBlobOutsideSelectedTexture"),
];

/// The metatable of class `name`, made on first use: `__index` holds the
/// methods of the class and its bases (the nearest class's wins).
pub fn class_meta(vm: &mut Vm, name: &str) -> TableRef {
    // An aura container is a frame (Blizzard_AuraContainer's intrinsic of
    // a `Frame`): the client names it one and gives it exactly a frame's
    // methods (`/stdtest engine`), and ElvUI, which adds its API to the
    // frame type's methods once, calls it on its aura containers
    // error-free in the client. Unverified: the shared table itself.
    let name = if name == "AuraContainer" {
        "Frame"
    } else {
        name
    };
    if let Some(&t) = engine(vm).world.classes.get(name) {
        return t;
    }
    let methods = vm.new_table();
    let classes = lineage(engine(vm), name);
    // The client's own list where recorded (camelot): exactly those
    // methods, documented or not (`/stdtest engine`).
    let recorded = crate::widgetdata::data(&engine(vm).flavor)
        .and_then(|d| d.methods.get(name))
        .cloned();
    if let Some(names) = recorded {
        for short in names {
            // The nearest documented method of that name.
            let doc = classes.iter().find_map(|class| {
                let e = engine(vm);
                e.api.classes.get(class).and_then(|c| {
                    c.methods
                        .iter()
                        .copied()
                        .find(|&i| e.api.methods[i].short == short)
                })
            });
            let native = match doc {
                Some(i) => {
                    let short = engine(vm).api.methods[i].short;
                    vm.native_with(short, method, vec![Value::Num(i as f64)])
                }
                None => {
                    let full = vm.str(&format!("{name}:{short}"));
                    let leaked: &'static str = Box::leak(short.clone().into_boxed_str());
                    vm.native_with(leaked, undocumented_method, vec![full])
                }
            };
            keep(vm, native);
            let key = vm.heap.intern(short.as_bytes());
            vm.heap.table_mut(methods).set_str(key, native);
        }
        let meta = vm.new_table();
        let index = vm.heap.intern(b"__index");
        vm.heap
            .table_mut(meta)
            .set_str(index, Value::Table(methods));
        keep(vm, Value::Table(meta));
        engine_mut(vm).world.classes.insert(name.to_string(), meta);
        return meta;
    }
    // Farthest base first, so nearer classes override.
    for class in classes.iter().rev() {
        let indexes = engine(vm)
            .api
            .classes
            .get(class)
            .map(|c| c.methods.clone())
            .unwrap_or_default();
        for i in indexes {
            let short = engine(vm).api.methods[i].short;
            // A function per class and method: `Frame.Show ~= Button.Show`
            // (`/stdtest engine`).
            let native = vm.native_with(short, method, vec![Value::Num(i as f64)]);
            keep(vm, native);
            let key = vm.heap.intern(short.as_bytes());
            vm.heap.table_mut(methods).set_str(key, native);
        }
    }
    // What the declarations add (`ModelScene:SetAllowOverlappedModels`),
    // the class's and its declared bases', as undocumented methods.
    let mut declared = Vec::new();
    let mut cur = Some(name.to_string());
    let mut depth = 0;
    while let Some(c) = cur.take() {
        depth += 1;
        if depth > 16 {
            break;
        }
        if let Some((base, names)) = engine(vm).api.declared.get(&c) {
            declared.extend(names.iter().cloned());
            cur = base.clone();
        }
    }
    for short in declared {
        let key = vm.heap.intern(short.as_bytes());
        if !vm.heap.table(methods).get_str(key).is_nil() {
            continue;
        }
        let full = vm.str(&format!("{name}:{short}"));
        let leaked: &'static str = Box::leak(short.into_boxed_str());
        let native = vm.native_with(leaked, undocumented_method, vec![full]);
        keep(vm, native);
        vm.heap.table_mut(methods).set_str(key, native);
    }
    // Methods documented or declared that the client's class lacks.
    if engine(vm).flavor == "camelot" {
        for &(class, short) in ABSENT_METHODS {
            if class == name {
                let key = vm.heap.intern(short.as_bytes());
                vm.heap.table_mut(methods).set_str(key, Value::Nil);
            }
        }
    }
    let meta = vm.new_table();
    let index = vm.heap.intern(b"__index");
    vm.heap
        .table_mut(meta)
        .set_str(index, Value::Table(methods));
    keep(vm, Value::Table(meta));
    engine_mut(vm).world.classes.insert(name.to_string(), meta);
    meta
}

/// Makes a widget of `class` (already resolved), named and parented.
pub fn create(vm: &mut Vm, class: &str, name: Option<String>, parent: Option<WidgetId>) -> Value {
    let meta = class_meta(vm, class);
    let table = vm.new_table();
    let id = WidgetId(engine(vm).world.widgets.len());
    let ud = vm.heap.alloc_userdata(Userdata {
        serial: 0,
        meta: None,
        env: None,
        data: Box::new(id),
    });
    let _ = vm
        .heap
        .table_mut(table)
        .set(Value::Num(0.0), Value::Userdata(ud));
    vm.heap.table_mut(table).meta = Some(meta);
    let scripts = vm.new_table();
    let hooks = vm.new_table();
    let props = vm.new_table();
    let attributes = vm.new_table();
    for t in [table, scripts, hooks, props, attributes] {
        keep(vm, Value::Table(t));
    }
    let w = Widget {
        kind: std::cell::Cell::new(0),
        class: class.to_string(),
        table,
        object: None,
        scripts,
        hooks,
        props,
        name: name.clone(),
        parent,
        children: Vec::new(),
        shown: true,
        events: HashMap::new(),
        points: Vec::new(),
        width: 0.0,
        height: 0.0,
        scale: 1.0,
        ignore_parent_scale: false,
        alpha: 1.0,
        secret_alpha: false,
        text: None,
        attributes,
        protected: false,
        all_events: None,
        first_registration: 0,
        explicitly_protected: false,
        forbidden: false,
        forbidden_aspects: 0,
        intrinsic_scripts: false,
        moving: false,
        sizing: false,
        enabled: true,
        resize_bounds: None,
        frame: false,
        scroll_parent: None,
        created: vm.caller_place(1),
    };
    let mut w = w;
    // Secure code's widgets are forbidden; their names are the addons'
    // globals all the same (`StoreFrame`).
    let main = engine(vm).main_globals;
    let forbidden = vm.globals != main;
    let world = &mut engine_mut(vm).world;
    w.forbidden = forbidden;
    world.widgets.push(w);
    world.scene_order_valid = false;
    if let Some(p) = parent {
        world.widgets[p.0].children.push(id);
    }
    if let Some(n) = name {
        let k = vm.heap.intern(n.as_bytes());
        vm.heap.table_mut(main).set_str(k, Value::Table(table));
    }
    // A font object's text starts white (`/stdtest engine answers`: fonts
    // without a color of their own).
    if class == "Font" {
        store(vm, id, "TextColor", &[Value::Num(1.0); 4]);
    }
    // Regions the client makes with the widget (`/stdtest engine`: a new
    // edit box has 2, a new cooldown 1). A cooldown's is its countdown
    // text (ElvUI takes it as `cooldown:GetRegions()`); an edit box's
    // first its text, then a texture (ElvUI's color picker sets the font
    // of `HexBox:GetRegions()`, error-free in the client; unverified: the
    // second's type).
    match class {
        "EditBox" => {
            create(vm, "FontString", None, Some(id));
            create(vm, "Texture", None, Some(id));
        }
        "Cooldown" => {
            let fs = create(vm, "FontString", None, Some(id));
            store(vm, id, "\u{0}CountdownFontString", &[fs]);
            // Centered a unit low, in SystemFont_Shadow_Large_Outline (SuuUI
            // probe trees: the equipment slots' cooldowns, from
            // CooldownFrameTemplate, and the PvP rank frame's own
            // `<Cooldown>` report it so).
            if let Some(fid) = widget_of(vm, fs) {
                engine_mut(vm).world.widgets[fid.0].points.push(Point {
                    point: "CENTER".into(),
                    relative: None,
                    relative_point: "CENTER".into(),
                    x: 0.0,
                    y: -1.0,
                });
                let font = vm.get_global("SystemFont_Shadow_Large_Outline");
                if widget_of(vm, font)
                    .is_some_and(|f| engine(vm).world.widgets[f.0].class == "Font")
                {
                    store(vm, fid, "FontObject", &[font]);
                }
            }
        }
        _ => {}
    }
    // A script object the documentation calls a userdata is one, with its
    // class's methods (`ObjectType = "Userdata"`: `type(curve)` is
    // "userdata", and `securecopy` passes it on where it refuses a frame,
    // as SuuUI's aura button options with curves and formatters run
    // error-free in the client). It keeps fields as a frame does
    // (Blizzard's `TimerUtil.CreateTimedSignalCallbackMap` defines a
    // method on its signal map): reads and writes go to the record behind
    // it, its class's methods below. Its metatable is hidden: `getmetatable`
    // gives false (recorded for a curve; the other script objects' taken to
    // be alike, unverified).
    if is_userdata_class(engine(vm), class) {
        let own = vm.new_table();
        let index = vm.heap.intern(b"__index");
        let newindex = vm.heap.intern(b"__newindex");
        let hidden = vm.heap.intern(b"__metatable");
        vm.heap.table_mut(own).set_str(index, Value::Table(table));
        vm.heap
            .table_mut(own)
            .set_str(newindex, Value::Table(table));
        vm.heap.table_mut(own).set_str(hidden, Value::Bool(false));
        // A duration prints as the client's `LuaDurationObject: 0x…` (the
        // full probe, 1.60.1; unverified: the other script objects' names).
        if class == "DurationObject" {
            let k = vm.heap.intern(b"__tostring");
            let f = vm.native_with("__tostring", duration_tostring, vec![]);
            vm.heap.table_mut(own).set_str(k, f);
        }
        keep(vm, Value::Table(own));
        let ud = vm.heap.alloc_userdata(Userdata {
            serial: 0,
            meta: Some(own),
            env: None,
            data: Box::new(ScriptObject(id)),
        });
        keep(vm, Value::Userdata(ud));
        engine_mut(vm).world.widgets[id.0].object = Some(ud);
        return Value::Userdata(ud);
    }
    Value::Table(table)
}

/// `tostring(duration)`: its type and address, as the client prints it.
fn duration_tostring(vm: &mut Vm, c: NativeCall) -> Ret {
    let raw = vm.raw_tostring(c.arg(1));
    let text = String::from_utf8_lossy(&raw).replacen("userdata", "LuaDurationObject", 1);
    Ok(vec![vm.str(&text)])
}

/// Addon code may not use a forbidden widget (unverified wording). The
/// error names the documented method's usage (made only for the error:
/// every method call comes here).
fn refuse_forbidden(
    vm: &mut Vm,
    id: WidgetId,
    short: &str,
    documented: Option<&crate::api::Function>,
) -> Result<(), LuaError> {
    // Attributes stay open: Blizzard's "Inbound" files talk to a forbidden
    // frame through them from any code (`StoreFrame:SetAttribute("action",
    // ...)`, `StoreFrame_IsShown`'s `GetAttribute("isshown")`; the user's
    // probe: StoreFrame forbidden, the shop's old path taken, SuuUI
    // error-free; unverified for other methods).
    if engine(vm).world.widgets[id.0].forbidden
        && !matches!(short, "IsForbidden" | "GetAttribute" | "SetAttribute")
        && crate::protection::tainted(vm)
    {
        // `/stdtest engine`: "calling 'GetObjectType' on bad self (Attempt
        // to access forbidden object from code tainted by an AddOn - Usage:
        // local objectType = self:GetObjectType())".
        let usage = documented
            .map(|f| format!(" - {}", crate::args::usage(f)))
            .unwrap_or_default();
        let msg = format!(
            "calling '{short}' on bad self (Attempt to access forbidden object from code tainted by an AddOn{usage})"
        );
        return Err(vm.error(&msg));
    }
    Ok(())
}

/// `CreateFrame(frameType [, name, parent, template, id])`.
pub fn create_frame(vm: &mut Vm, c: NativeCall) -> Ret {
    // The client's errors (`/stdtest engine`).
    let kind = match c.arg(1) {
        Value::Str(s) => String::from_utf8_lossy(vm.str_bytes(s)).into_owned(),
        Value::Num(n) => {
            let v = vm.tostring(Value::Num(n))?;
            match v {
                Value::Str(s) => String::from_utf8_lossy(vm.str_bytes(s)).into_owned(),
                _ => String::new(),
            }
        }
        _ => {
            return Err(c_error(
                vm,
                "Usage: CreateFrame(\"frameType\" [, \"name\"] [, parent] [, \"template\"] [, id])",
                false,
            ));
        }
    };
    // The client takes the type in any case.
    let class = engine(vm)
        .api
        .classes
        .iter()
        .find(|(_, cl)| {
            cl.widget
                .as_deref()
                .is_some_and(|w| w.eq_ignore_ascii_case(&kind))
        })
        .map(|(name, _)| name.clone());
    // A frame type defined for the run (`defineFrameType`): its base class
    // and the function that makes it what it is.
    let custom = engine(vm)
        .world
        .frame_types
        .iter()
        .find(|(name, _)| name.eq_ignore_ascii_case(&kind))
        .map(|(_, v)| v.clone());
    let intrinsic = engine(vm)
        .world
        .ui
        .intrinsics
        .get(&kind.to_ascii_lowercase())
        .cloned();
    // An intrinsic type of Blizzard's UI code, not loaded: a frame
    // (unverified stand-in).
    let blizzard = engine(vm)
        .api
        .templates
        .contains(&kind.to_ascii_lowercase());
    if class.is_none() && custom.is_none() && intrinsic.is_none() && blizzard {
        engine_mut(vm).world.warnings.push(format!(
            "CreateFrame: frame type \"{kind}\" made a Frame (Blizzard's UI code is not loaded)"
        ));
    }
    let class = class
        .or_else(|| custom.as_ref().map(|(base, _)| base.clone()))
        .or_else(|| intrinsic.as_ref().map(|(base, _)| base.clone()))
        .or_else(|| blizzard.then(|| "Frame".to_string()));
    let Some(class) = class else {
        return Err(c_error(
            vm,
            &format!("CreateFrame(): Unknown frame type '{kind}'"),
            true,
        ));
    };
    let parent = match c.arg(3) {
        Value::Nil => None,
        v => match widget_of(vm, v) {
            Some(p) => Some(p),
            None => return Err(c_error(vm, "Wrong object type for function", false)),
        },
    };
    let name = match vm.opt_str(&c, 2)? {
        Some(s) => {
            let mut n = String::from_utf8_lossy(vm.str_bytes(s)).into_owned();
            if n.contains("$parent") || n.contains("$Parent") {
                let pname = parent
                    .and_then(|p| engine(vm).world.widgets[p.0].name.clone())
                    .unwrap_or_default();
                n = n.replace("$parent", &pname).replace("$Parent", &pname);
            }
            Some(n)
        }
        None => None,
    };
    let templates: Vec<String> = match vm.opt_str(&c, 4)? {
        Some(t) => String::from_utf8_lossy(vm.str_bytes(t))
            .split(',')
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .collect(),
        None => Vec::new(),
    };
    let frame = create(vm, &class, name, parent);
    if let Some(id) = widget_of(vm, frame) {
        let w = &mut engine_mut(vm).world.widgets[id.0];
        w.frame = true;
        w.kind.set(0);
        // Blizzard's secure templates are protected (unverified without
        // their XML).
        w.protected = templates
            .iter()
            .any(|t| t.starts_with("Secure") && t.ends_with("Template"));
        w.explicitly_protected = w.protected;
    }
    if let Some((_, f)) = custom {
        vm.call(f, &[frame])?;
    }
    let from_xml = intrinsic.is_some()
        || templates.iter().any(|t| {
            engine(vm)
                .world
                .ui
                .templates
                .contains_key(&t.to_ascii_lowercase())
        });
    // The ID before the templates and OnLoad (Blizzard's action buttons
    // read it there for their slot). An ID that is no number is ignored
    // (unverified: DetailsFramework's fill panel passes a template's name
    // there, in Details!'s options window, as the client's other checks of
    // CreateFrame are its own, none a Lua argument error).
    if c.has(5) && !c.arg(5).is_nil() {
        let id = match c.arg(5) {
            Value::Num(n) => Some(n),
            Value::Str(s) => String::from_utf8_lossy(vm.str_bytes(s)).trim().parse().ok(),
            _ => None,
        };
        if let Some(id) = id {
            let w = widget_of(vm, frame).expect("a widget");
            store(vm, w, "ID", &[Value::Num(id)]);
        }
    }
    // A type or template declared with `allowUntaintedCreation` is built
    // untainted, its scripts compiled and its mixins copied as Blizzard's
    // code would, then the caller's taint is back. Evidence: the client
    // runs the custom aura container's layout (its `OnUpdate` script, a
    // `method` handler, calling `SetSize` with secrets, allowed only
    // untainted) without an error for an addon's container (SuuUI), and
    // the attribute's name. Unverified: a frame mixing such a template
    // with others is built untainted as a whole.
    let untainted = {
        let ui = &engine(vm).world.ui;
        intrinsic.is_some() && ui.untainted_creation.contains(&kind.to_ascii_lowercase())
            || templates
                .iter()
                .any(|t| ui.untainted_creation.contains(&t.to_ascii_lowercase()))
    };
    let saved = vm.taint().saved();
    if untainted {
        vm.taint_mut().stack = wowlua_vm::Taint::SECURE;
    }
    let built = build_from_xml(vm, frame, intrinsic.map(|(_, el)| el), &templates, from_xml);
    if untainted {
        vm.taint_mut().restore(saved);
    }
    built?;
    Ok(vec![frame])
}

/// A new frame's intrinsic type and templates applied, then its `OnLoad`
/// (templates of the UI code run it once applied).
fn build_from_xml(
    vm: &mut Vm,
    frame: Value,
    intrinsic: Option<std::rc::Rc<crate::ui::xml::Element>>,
    templates: &[String],
    from_xml: bool,
) -> Result<(), LuaError> {
    // Within the caller's execution, its children's too: an addon's frame
    // from Blizzard's template loads tainted (its handlers are Blizzard's
    // code; decision T243).
    let outer = std::mem::replace(&mut engine_mut(vm).world.ui.creating, true);
    let r = build_with_scripts(vm, frame, intrinsic, templates, from_xml);
    engine_mut(vm).world.ui.creating = outer;
    r
}

fn build_with_scripts(
    vm: &mut Vm,
    frame: Value,
    intrinsic: Option<std::rc::Rc<crate::ui::xml::Element>>,
    templates: &[String],
    from_xml: bool,
) -> Result<(), LuaError> {
    if let Some(el) = intrinsic {
        crate::ui::build::apply_intrinsic(vm, frame, &el);
    }
    for t in templates {
        apply_template(vm, frame, t)?;
    }
    if from_xml && let Some(id) = widget_of(vm, frame) {
        run_script(vm, id, "OnLoad", &[]);
        // Built visible from XML: shown, as a frame of an XML file is
        // (`ui::build`; warcraft.wiki.gg's UIHANDLER_OnShow, "if a frame is
        // defined in XML then OnShow also fires after OnLoad unless hidden
        // at the time"). Unverified for CreateFrame's templates: Auctionator
        // sets itself up in the OnShow of a frame it creates under the
        // shown auction house.
        if visible(engine(vm), id) {
            run_script(vm, id, "OnShow", &[]);
        }
    }
    Ok(())
}

/// Applies template `name` to a frame: the function a test defined for it
/// (`defineTemplate`), or a warning (Blizzard's UI code is not loaded).
pub fn apply_template(vm: &mut Vm, frame: Value, name: &str) -> Result<(), LuaError> {
    let defined = engine(vm)
        .world
        .templates
        .iter()
        .find(|(t, _)| t.eq_ignore_ascii_case(name))
        .map(|(_, f)| *f);
    match defined {
        Some(f) => {
            vm.call(f, &[frame])?;
        }
        None => {
            if !crate::ui::build::apply_template(vm, frame, name) {
                // A template of Blizzard's code only warns while it is not
                // loaded; any other name fails as in the client (decided
                // 2026-10-16, `/stdtest engine`). With Blizzard's UI loaded,
                // one its loaded files lack fails too (SuuUI probe bags:
                // `BankItemButtonGenericTemplate`, a vanilla game type's).
                let e = engine(vm);
                if !e.world.ui.loaded.is_empty()
                    || !e.api.templates.contains(&name.to_ascii_lowercase())
                {
                    return Err(c_error(
                        vm,
                        &format!("CreateFrame(): Couldn't find inherited node '{name}'"),
                        true,
                    ));
                }
                engine_mut(vm).world.warnings.push(format!(
                    "CreateFrame: template \"{name}\" ignored (Blizzard's UI code is not loaded)"
                ));
            }
        }
    }
    Ok(())
}

/// Every widget method: checks `self` and the arguments, then runs its
/// behaviour, a stored setter or getter, or fails as not mocked.
fn method(vm: &mut Vm, c: NativeCall) -> Ret {
    let Some(Value::Num(i)) = c.upvals.first().copied() else {
        return Err(vm.error("a widget method without its index"));
    };
    let api = engine(vm).api;
    let f = &api.methods[i as usize];
    // `self`, as the client checks it (`/stdtest engine`).
    let id = match c.arg(1) {
        Value::Nil => return Err(vm.api_arg_error(1, &crate::args::usage(f))),
        Value::Table(_) | Value::Userdata(_) => match widget_of(vm, c.arg(1)) {
            Some(id) => id,
            None => {
                let msg = format!("Wrong object type for function - {}", crate::args::usage(f));
                return Err(vm.api_arg_error(1, &msg));
            }
        },
        _ => {
            let msg = format!(
                "Attempt to find 'this' in non-table object (used '.' instead of ':' ?) - {}",
                crate::args::usage(f)
            );
            return Err(vm.api_arg_error(1, &msg));
        }
    };
    refuse_forbidden(vm, id, f.short, Some(f))?;
    // The documented arguments first (`/stdtest engine blocked`: a bad
    // argument errors on a blocked frame), but where the client's method takes more
    // than the documentation lists (`SetPoint(point, x, y)`): its
    // behaviour checks those.
    const OWN_CHECKS: &[&str] = &[
        "SetPoint",
        // `SetAllPoints()` anchors to the parent; the documentation says
        // `relativeTo` is required.
        "SetAllPoints",
        "GetAttribute",
        "SetAttribute",
        // Its arguments are any values (a unit menu passes the frame).
        "ExecuteAttribute",
        "SetFontObject",
        "RegisterUnitEvent",
        "CreateFontString",
        "CreateTexture",
        "CreateMaskTexture",
        "CreateLine",
    ];
    if !OWN_CHECKS.contains(&f.short) {
        crate::args::check_from(vm, api, f, &c, 2)?;
    }
    if crate::protection::refuses_method(vm, id, f) {
        let api = engine(vm).api;
        return Ok(crate::defaults(vm, api, &f.doc));
    }
    // (a call that may change it: the scene lays it out again, T542)
    if !reads_only(f.short) {
        mark(engine_mut(vm), id);
    }
    let behaviour = engine(vm).method_behaviour(f.name, f.short);
    if let Some(b) = behaviour {
        return b(vm, id, c);
    }
    stored(vm, id, f, &c)
}

/// A method the client has but the documentation lacks (`CheckButton:
/// GetChecked`, `GameTooltip:AddLine`): its behaviour, the stored setter
/// and getter rules, nothing for Blizzard's code, else not mocked.
fn undocumented_method(vm: &mut Vm, c: NativeCall) -> Ret {
    let full = match c.upvals.first().copied() {
        Some(Value::Str(s)) => String::from_utf8_lossy(vm.str_bytes(s)).into_owned(),
        _ => "?".into(),
    };
    let short = full
        .split_once(':')
        .map_or(full.as_str(), |(_, s)| s)
        .to_string();
    let Some(id) = widget_of(vm, c.arg(1)) else {
        let msg = format!("Wrong object type for function - Usage: self:{short}()");
        return Err(vm.api_arg_error(1, &msg));
    };
    refuse_forbidden(vm, id, &short, None)?;
    if !reads_only(&short) {
        mark(engine_mut(vm), id);
    }
    let behaviour = engine(vm).method_behaviour(&full, &short);
    if let Some(b) = behaviour {
        return b(vm, id, c);
    }
    let props = engine(vm).world.widgets[id.0].props;
    if let Some(stem) = short.strip_prefix("Set") {
        let args: Vec<Value> = c.args.iter().skip(1).copied().collect();
        store(vm, id, stem, &args);
        return Ok(vec![]);
    }
    for prefix in ["Get", "Is"] {
        if let Some(stem) = short.strip_prefix(prefix)
            && let Some(values) = stored_args(vm, props, stem)
        {
            return Ok(if prefix == "Is" {
                vec![Value::Bool(values.first().is_some_and(|v| v.truthy()))]
            } else {
                values
            });
        }
    }
    let class = engine(vm).world.widgets[id.0].class.clone();
    if let Some(values) = recorded_default(engine(vm), &class, &short) {
        return Ok(values.iter().map(|r| r.value(vm)).collect());
    }
    if crate::protection::blizzard_caller(vm) {
        return Ok(vec![]);
    }
    Err(vm.error(&format!("{full} is not mocked")))
}

/// A fresh widget's results for a getter, as the client gave them for the
/// class or its nearest recorded base (`/stdtest engine`).
pub fn recorded_default(
    e: &Engine,
    class: &str,
    getter: &str,
) -> Option<&'static Vec<crate::widgetdata::Recorded>> {
    let data = crate::widgetdata::data(&e.flavor)?;
    let mut key = (String::new(), getter.to_string());
    lineage(e, class).iter().find_map(|c| {
        c.clone_into(&mut key.0);
        data.defaults.get(&key)
    })
}

/// The stored arguments of setter `stem`, if it ran.
pub fn stored_args(vm: &mut Vm, props: TableRef, stem: &str) -> Option<Vec<Value>> {
    let key = vm.str(stem);
    stored_args_at(vm, props, key)
}

/// [`stored_args`] by the stem's string.
pub fn stored_args_at(vm: &mut Vm, props: TableRef, key: Value) -> Option<Vec<Value>> {
    let Value::Table(list) = vm.heap.table(props).get(key) else {
        return None;
    };
    let n = vm.str("n");
    let count = match vm.heap.table(list).get(n) {
        Value::Num(n) => n as usize,
        _ => 0,
    };
    Some(
        (1..=count)
            .map(|k| vm.heap.table(list).get(Value::Num(k as f64)))
            .collect(),
    )
}

/// A method without behaviour (decided for S3, unverified): `SetX(...)` and
/// `EnableX(...)` store their arguments; `GetX()` returns what `SetX` stored,
/// `IsX()` whether its first value is true (`IsXEnabled` reads `EnableX`);
/// another method that returns nothing does nothing; anything else is not
/// mocked.
fn stored(vm: &mut Vm, id: WidgetId, f: &crate::api::Function, c: &NativeCall) -> Ret {
    let (short, name) = (f.short, f.name);
    let props = engine(vm).world.widgets[id.0].props;
    let returns = f
        .doc
        .get("Returns")
        .and_then(serde_json::Value::as_array)
        .map_or(&[][..], Vec::as_slice);
    let setter = short
        .strip_prefix("Set")
        .map(str::to_string)
        .or_else(|| short.strip_prefix("Enable").map(|s| format!("{s}Enabled")));
    if let Some(stem) = setter {
        let args: Vec<Value> = c.args.iter().skip(1).copied().collect();
        store(vm, id, &stem, &args);
        // A setter's documented results (`success`): it worked.
        return Ok(returns
            .iter()
            .map(
                |r| match r.get("Type").and_then(serde_json::Value::as_str) {
                    Some("bool") => Value::Bool(true),
                    _ => Value::Nil,
                },
            )
            .collect());
    }
    if let Some(stem) = short.strip_prefix("Get")
        && let Some(values) = stored_args(vm, props, stem)
    {
        return Ok(values);
    }
    // Nothing stored: a fresh widget's results, where recorded.
    let has_stored = short
        .strip_prefix("Is")
        .is_some_and(|stem| stored_args(vm, props, stem).is_some());
    if !has_stored {
        let class = engine(vm).world.widgets[id.0].class.clone();
        if let Some(values) = recorded_default(engine(vm), &class, short) {
            return Ok(values.iter().map(|r| r.value(vm)).collect());
        }
    }
    if let Some(stem) = short.strip_prefix("Is")
        && returns.len() == 1
    {
        let on = match stored_args(vm, props, stem) {
            Some(values) => values.first().is_some_and(|v| v.truthy()),
            // Off until set, but the kinds that take the mouse do
            // (`/stdtest engine`, the widget defaults).
            None => {
                let class = engine(vm).world.widgets[id.0].class.clone();
                short == "IsMouseEnabled" && mouse_by_default(engine(vm), &class)
            }
        };
        return Ok(vec![Value::Bool(on)]);
    }
    if returns.is_empty() {
        return Ok(vec![]);
    }
    // What the engine draws or keeps nothing of: the documented empty
    // values for everyone (`widgetops::EMPTY_METHODS`).
    if crate::widgetops::EMPTY_METHODS.contains(&name) {
        let api = engine(vm).api;
        return Ok(crate::defaults(vm, api, &f.doc));
    }
    // Blizzard's code gets empty values; the addon's code an error.
    if crate::protection::blizzard_caller(vm) {
        engine_mut(vm).defaulted.insert(name);
        crate::note_default(vm, name, c, 2);
        let api = engine(vm).api;
        return Ok(crate::defaults(vm, api, &f.doc));
    }
    Err(vm.error(&format!("{name} is not mocked")))
}

// ------------------------------------------------------------ scripts

/// The script types a class has (from the annotations' `Script<Class>`
/// unions; unverified), with `OnLoad` and `OnUpdate` for every widget.
pub fn has_script(e: &Engine, class: &str, script: &str) -> bool {
    // The client's answers where recorded (camelot: `/stdtest engine`).
    if let Some(data) = crate::widgetdata::data(&e.flavor)
        && let Some(&yes) = {
            let mut key = (String::new(), script.to_string());
            lineage(e, class).iter().find_map(|c| {
                c.clone_into(&mut key.0);
                data.scripts.get(&key)
            })
        }
    {
        return yes;
    }
    if script == "OnLoad" || script == "OnUpdate" {
        return true;
    }
    // Scripts the client has that neither the probe asked about nor the
    // documentation lists: Blizzard's dressing room sets `OnDressModel` on
    // its model scene, the client tells it of each item an actor puts on.
    if script == "OnDressModel"
        && lineage(e, class)
            .iter()
            .any(|c| c == "ModelScene" || c == "DressUpModel")
    {
        return true;
    }
    // `ScriptButton = ScriptFrame | …`: the named bases are in the lineage
    // too.
    lineage(e, class).iter().any(|c| {
        e.api
            .aliases
            .get(&format!("Script{c}"))
            .is_some_and(|list| {
                list.split('|')
                    .any(|part| part.trim().trim_matches('"') == script)
            })
    })
}

/// Runs a widget's script and its hooks, as the client does: each from a
/// secure execution, an error going to the error handler.
/// Where an intrinsic type's handler for `script` is kept (`precall` or
/// `postcall`), among the widget's properties.
pub fn intrinsic_key(order: &str, script: &str) -> String {
    format!("intrinsic {order} {script}")
}

pub fn run_script(vm: &mut Vm, id: WidgetId, script: &str, args: &[Value]) {
    run_script_of(vm, id, script, args, false);
}

/// [`run_script`], with `trusted_only` only the secure handlers and hooks
/// (an object's forbidden untrusted layout scripts).
fn run_script_of(vm: &mut Vm, id: WidgetId, script: &str, args: &[Value], trusted_only: bool) {
    let d = if vm.dispatch_wanted() {
        let name = script_label(engine(vm), id, script);
        let kind = if script == "OnUpdate" {
            DispatchKind::Update
        } else {
            DispatchKind::Script
        };
        vm.dispatch_enter(kind, || name)
    } else {
        None
    };
    run_script_body(vm, id, script, args, trusted_only);
    vm.dispatch_exit(d);
}

/// How the debugger and profiler name a widget's script:
/// `MyFrame:OnClick`, `Button#12:OnClick` for one without a name.
pub fn script_label(e: &Engine, id: WidgetId, script: &str) -> String {
    format!("{}:{script}", widget_label(e, id))
}

/// A widget's name, or its type and number.
pub fn widget_label(e: &Engine, id: WidgetId) -> String {
    let w = &e.world.widgets[id.0];
    match &w.name {
        Some(n) => n.clone(),
        None => format!("{}#{}", w.class, id.0),
    }
}

fn run_script_body(vm: &mut Vm, id: WidgetId, script: &str, args: &[Value], trusted_only: bool) {
    let untrusted = |vm: &Vm, f: Value| {
        trusted_only && matches!(f, Value::Func(r) if !vm.heap.func(r).taint().is_secure())
    };
    // (fresh: as the client calls a script; else within `CreateFrame`)
    let fresh = !engine(vm).world.ui.creating;
    let (table, scripts, hooks, props, has_intrinsic) = {
        let w = &engine(vm).world.widgets[id.0];
        (w.table, w.scripts, w.hooks, w.props, w.intrinsic_scripts)
    };
    let key = vm.heap.intern(script.as_bytes());
    let mut call = vec![Value::Table(table)];
    call.extend_from_slice(args);
    let intrinsic = |vm: &mut Vm, order: &str| {
        if !has_intrinsic {
            return Value::Nil;
        }
        let k = vm.str(&intrinsic_key(order, script));
        vm.heap.table(props).get(k)
    };
    let pre = intrinsic(vm, "precall");
    if !pre.is_nil() {
        vm.call_from_engine_in(pre, &call, fresh);
    }
    let f = vm.heap.table(scripts).get_str(key);
    if !f.is_nil() && !untrusted(vm, f) {
        vm.call_from_engine_in(f, &call, fresh);
    }
    let post = intrinsic(vm, "postcall");
    if !post.is_nil() {
        vm.call_from_engine_in(post, &call, fresh);
    }
    if let Value::Table(list) = vm.heap.table(hooks).get_str(key) {
        let n = vm.heap.table(list).border();
        for k in 1..=n {
            let h = vm.heap.table(list).get(Value::Num(k as f64));
            if !untrusted(vm, h) {
                vm.call_from_engine_in(h, &call, fresh);
            }
        }
    }
}

/// Whether any widget class has script type `script`.
fn known_script(e: &Engine, script: &str) -> bool {
    script == "OnLoad"
        || script == "OnUpdate"
        || e.api.aliases.iter().any(|(name, list)| {
            name.starts_with("Script")
                && list
                    .split('|')
                    .any(|part| part.trim().trim_matches('"') == script)
        })
}

/// The script type argument: a type no widget has is an argument error; one
/// this widget lacks, "Cannot assign script handler" (`/stdtest engine`).
fn script_name(vm: &mut Vm, id: WidgetId, c: &NativeCall, op: &str) -> Result<String, LuaError> {
    let s = vm.check_str(c, 2)?;
    let s = String::from_utf8_lossy(vm.str_bytes(s)).into_owned();
    let class = engine(vm).world.widgets[id.0].class.clone();
    // Blizzard's code knows the client's scripts better than the lists.
    if has_script(engine(vm), &class, &s) || crate::protection::blizzard_caller(vm) {
        return Ok(s);
    }
    if !known_script(engine(vm), &s) {
        let api = engine(vm).api;
        let usage = api
            .methods
            .iter()
            .find(|m| m.short == op)
            .map(crate::args::usage)
            .unwrap_or_default();
        return Err(vm.api_arg_error(2, &usage));
    }
    let lower = s.to_lowercase();
    Err(c_error(
        vm,
        &format!(
            "{class}:{op}(): Cannot assign script handler for '{lower}' (script type not supported by this object)"
        ),
        true,
    ))
}

fn set_script(vm: &mut Vm, id: WidgetId, c: NativeCall) -> Ret {
    let s = script_name(vm, id, &c, "SetScript")?;
    let (scripts, hooks) = {
        let w = &engine(vm).world.widgets[id.0];
        (w.scripts, w.hooks)
    };
    let key = vm.heap.intern(s.as_bytes());
    let f = c.arg(3);
    vm.heap.table_mut(scripts).set_str(key, f);
    vm.heap.table_mut(hooks).set_str(key, Value::Nil);
    script_stored(vm, id, &s);
    if !f.is_nil() {
        input_script_set(vm, id, &s);
    }
    Ok(vec![])
}

/// A script stored for a widget (set, hooked or from XML): an `OnUpdate`
/// makes it one a frame looks at.
pub fn script_stored(vm: &mut Vm, id: WidgetId, script: &str) {
    match script {
        "OnUpdate" => engine_mut(vm).world.updates(id.0),
        "OnSizeChanged" => insert_sorted(&mut engine_mut(vm).world.size_watching, id.0),
        _ => {}
    }
}

/// A script that takes input, given (set, hooked or from XML), turns that
/// input on; taking it away does not turn it off. A key script the
/// keyboard (`/stdtest engine answers` "Keyboard script enables":
/// `KeybindListener` listens by its scripts alone); `OnEnter`, `OnLeave`,
/// `OnMouseDown` and `OnMouseUp` the mouse, `OnMouseWheel` the wheel
/// (decision T365; unverified on WoW Forever: the side tabs of the
/// character frame and the group finder are frames clicked by their
/// `OnMouseUp` alone).
pub fn input_script_set(vm: &mut Vm, id: WidgetId, script: &str) {
    match script {
        "OnKeyDown" | "OnKeyUp" => store(vm, id, "KeyboardEnabled", &[Value::Bool(true)]),
        "OnEnter" | "OnLeave" | "OnMouseDown" | "OnMouseUp" => {
            store(vm, id, "MouseEnabled", &[Value::Bool(true)])
        }
        "OnMouseWheel" => store(vm, id, "MouseWheelEnabled", &[Value::Bool(true)]),
        _ => {}
    }
}

/// `GetPropagateKeyboardInput()`: false until set.
fn get_propagate_keyboard_input(vm: &mut Vm, id: WidgetId, _c: NativeCall) -> Ret {
    let props = engine(vm).world.widgets[id.0].props;
    let on = stored_args(vm, props, "PropagateKeyboardInput")
        .and_then(|v| v.first().copied())
        .is_some_and(|v| v.truthy());
    Ok(vec![Value::Bool(on)])
}

fn get_script(vm: &mut Vm, id: WidgetId, c: NativeCall) -> Ret {
    let s = script_name(vm, id, &c, "GetScript")?;
    let scripts = engine(vm).world.widgets[id.0].scripts;
    let key = vm.heap.intern(s.as_bytes());
    Ok(vec![vm.heap.table(scripts).get_str(key)])
}

fn hook_script(vm: &mut Vm, id: WidgetId, c: NativeCall) -> Ret {
    let s = script_name(vm, id, &c, "HookScript")?;
    let hooks = engine(vm).world.widgets[id.0].hooks;
    let key = vm.heap.intern(s.as_bytes());
    let list = match vm.heap.table(hooks).get_str(key) {
        Value::Table(t) => t,
        _ => {
            let t = vm.new_table();
            vm.heap.table_mut(hooks).set_str(key, Value::Table(t));
            t
        }
    };
    let n = vm.heap.table(list).border() as f64 + 1.0;
    let _ = vm.heap.table_mut(list).set(Value::Num(n), c.arg(3));
    script_stored(vm, id, &s);
    input_script_set(vm, id, &s);
    Ok(vec![])
}

fn has_script_method(vm: &mut Vm, id: WidgetId, c: NativeCall) -> Ret {
    let s = vm.check_str(&c, 2)?;
    let s = String::from_utf8_lossy(vm.str_bytes(s)).into_owned();
    let class = engine(vm).world.widgets[id.0].class.clone();
    Ok(vec![Value::Bool(has_script(engine(vm), &class, &s))])
}

// ------------------------------------------------------------- events

/// Whether an event is a unit event: its payload starts with a unit token
/// (undocumented ones: named `UNIT_…`).
fn unit_event(e: &Engine, event: &str) -> bool {
    match e.api.event_docs.get(event) {
        Some(doc) => doc
            .get("Payload")
            .and_then(serde_json::Value::as_array)
            .and_then(|p| p.first())
            .and_then(|p| p.get("Type"))
            .and_then(serde_json::Value::as_str)
            .is_some_and(|t| t.starts_with("UnitToken")),
        None => event.starts_with("UNIT_"),
    }
}

/// The event argument, refused as the client refuses it: `RegisterEvent`'s
/// message names the method twice, `RegisterUnitEvent` refuses events that
/// are not unit events (`/stdtest engine`).
fn event_name(vm: &mut Vm, c: &NativeCall, op: &str) -> Result<String, LuaError> {
    let e = vm.check_str(c, 2)?;
    let e = String::from_utf8_lossy(vm.str_bytes(e)).into_owned();
    // Blizzard's code knows the client's events better than the lists.
    let known = engine(vm).api.events.contains(&e) || crate::protection::blizzard_caller(vm);
    let unit_ok = op != "RegisterUnitEvent" || unit_event(engine(vm), &e);
    if !known || !unit_ok {
        let class = widget_of(vm, c.arg(1))
            .map(|id| engine(vm).world.widgets[id.0].class.clone())
            .unwrap_or_default();
        let prefix = if op == "RegisterEvent" {
            format!("{class}:{op}(): {class}:{op}(): ")
        } else {
            format!("{class}:{op}(): ")
        };
        return Err(c_error(
            vm,
            &format!("{prefix}Attempt to register unknown event \"{e}\""),
            false,
        ));
    }
    Ok(e)
}

/// Registers an event, keeping when the frame first registered one
/// (frames get events in that order); false if it already was.
fn register(vm: &mut Vm, id: WidgetId, event: String, units: Option<Vec<String>>) -> bool {
    let world = &mut engine_mut(vm).world;
    world.next_registration += 1;
    let seq = world.next_registration;
    world.listening.insert(id.0);
    let w = &mut world.widgets[id.0];
    if w.first_registration == 0 {
        w.first_registration = seq;
    }
    let fresh = !w.events.contains_key(&event);
    w.events.insert(event, units);
    fresh
}

fn register_event(vm: &mut Vm, id: WidgetId, c: NativeCall) -> Ret {
    let e = event_name(vm, &c, "RegisterEvent")?;
    let fresh = register(vm, id, e, None);
    Ok(vec![Value::Bool(fresh)])
}

fn register_unit_event(vm: &mut Vm, id: WidgetId, c: NativeCall) -> Ret {
    let e = event_name(vm, &c, "RegisterUnitEvent")?;
    let mut units = Vec::new();
    for n in 3..=c.len() {
        if let Value::Str(s) = c.arg(n) {
            units.push(String::from_utf8_lossy(vm.str_bytes(s)).into_owned());
        }
    }
    register(vm, id, e, Some(units));
    Ok(vec![Value::Bool(true)])
}

fn unregister_event(vm: &mut Vm, id: WidgetId, c: NativeCall) -> Ret {
    let e = vm.check_str(&c, 2)?;
    let e = String::from_utf8_lossy(vm.str_bytes(e)).into_owned();
    let w = &mut engine_mut(vm).world.widgets[id.0];
    w.events.remove(&e);
    Ok(vec![])
}

/// `RegisterAllEvents()`: every event but the restricted ones.
fn register_all_events(vm: &mut Vm, id: WidgetId, _c: NativeCall) -> Ret {
    let world = &mut engine_mut(vm).world;
    world.next_registration += 1;
    let seq = world.next_registration;
    world.listening.insert(id.0);
    world.widgets[id.0].all_events = Some(seq);
    if world.widgets[id.0].first_registration == 0 {
        world.widgets[id.0].first_registration = seq;
    }
    Ok(vec![])
}

/// Events addons' frames are not given (the documentation's
/// `HasRestrictions`: the combat log, read by Blizzard's code securely).
fn restricted_event(event: &str) -> bool {
    matches!(event, "COMBAT_LOG_EVENT" | "COMBAT_LOG_EVENT_UNFILTERED")
}

fn unregister_all_events(vm: &mut Vm, id: WidgetId, _c: NativeCall) -> Ret {
    let w = &mut engine_mut(vm).world.widgets[id.0];
    w.events.clear();
    w.all_events = None;
    Ok(vec![])
}

fn is_event_registered(vm: &mut Vm, id: WidgetId, c: NativeCall) -> Ret {
    let e = vm.check_str(&c, 2)?;
    let e = String::from_utf8_lossy(vm.str_bytes(e)).into_owned();
    // (isRegistered, units): the units a unit event was registered for
    // (unverified).
    let units = match engine(vm).world.widgets[id.0].events.get(&e) {
        None => return Ok(vec![Value::Bool(false)]),
        Some(units) => units.clone().unwrap_or_default(),
    };
    let mut out = vec![Value::Bool(true)];
    for u in units {
        out.push(vm.str(&u));
    }
    Ok(out)
}

/// Fires `event` at every widget registered for it, in creation order
/// (unverified); a unit event only where the unit (the first argument)
/// is one the widget registered.
pub fn fire_event(vm: &mut Vm, event: &str, args: &[Value]) {
    // One the client batches to the frame's end: owed (`batch`).
    if crate::batch::defer(vm, event, args) {
        return;
    }
    fire_now(vm, event, args);
}

/// Fires `event` now, batched or not (`fire_event`).
pub fn fire_now(vm: &mut Vm, event: &str, args: &[Value]) {
    crate::batch::run_before(vm, event);
    if restricted_event(event) {
        crate::classic::note_combat_log_event(vm, args);
    }
    let unit = match args.first() {
        Some(Value::Str(s)) => Some(String::from_utf8_lossy(vm.str_bytes(*s)).into_owned()),
        _ => None,
    };
    // In the order the frames first registered an event, whichever
    // (`/stdtest engine order`: the registrations' order, not the frames'
    // creation; recorded, `inspect` 11.157: the event registry's frame,
    // registering `INSPECT_READY` only after the recorder's
    // `RegisterAllEvents` but registered for others since the UI loaded,
    // got it first: the talents it loaded told before; ordered by each
    // event's registration that one fails, with `RegisterAllEvents` frames
    // last `profession` loses 12 lines).
    let world = &engine(vm).world;
    let mut targets: Vec<(u64, WidgetId)> = world
        .listening
        .iter()
        .map(|&i| (i, &world.widgets[i]))
        .filter(|(_, w)| match w.events.get(event) {
            Some(None) => true,
            Some(Some(units)) => unit.as_ref().is_some_and(|u| units.contains(u)),
            None => w.all_events.is_some() && !restricted_event(event),
        })
        .map(|(i, w)| (w.first_registration, WidgetId(i)))
        .collect();
    targets.sort_by_key(|(seq, _)| *seq);
    let targets: Vec<WidgetId> = targets.into_iter().map(|(_, id)| id).collect();
    if event == "PLAYER_LOGIN" {
        engine_mut(vm).world.logged_in = true;
        crate::chat::hook_chat_windows(vm);
    }
    // The interactions open, as the client tells them (asked by
    // `C_PlayerInteractionManager.IsInteractingWithNpcOfType`).
    if let (true, Some(Value::Num(n))) = (
        event.starts_with("PLAYER_INTERACTION_MANAGER_FRAME_"),
        args.first().copied(),
    ) {
        let open = &mut engine_mut(vm).world.interactions;
        if event.ends_with("_SHOW") && !open.contains(&n.to_bits()) {
            open.push(n.to_bits());
        } else if event.ends_with("_HIDE") {
            open.retain(|&o| o != n.to_bits());
        }
    }
    if event == "ADDON_LOADED"
        && let Some(Value::Str(s)) = args.first()
    {
        let name = String::from_utf8_lossy(vm.str_bytes(*s)).to_lowercase();
        engine_mut(vm).world.loaded_addons.insert(name);
    }
    let name = vm.str(event);
    let mut call = vec![name];
    call.extend_from_slice(args);
    let logged = crate::eventlog::begin(vm, event, args, &targets);
    let d = vm.dispatch_enter(DispatchKind::Event, || event.to_string());
    for id in targets {
        // A handler may unregister others meanwhile.
        let w = &engine(vm).world.widgets[id.0];
        if w.events.contains_key(event) || w.all_events.is_some() {
            run_script(vm, id, "OnEvent", &call);
        }
    }
    crate::framescript::run_callbacks(vm, event, args);
    vm.dispatch_exit(d);
    crate::eventlog::end(vm, logged);
}

// --------------------------------------------------------- visibility

/// Whether the widget has a handler for `script` (its own, an intrinsic
/// type's or a hook), without running it.
pub fn has_handler(vm: &mut Vm, id: WidgetId, script: &str) -> bool {
    let (scripts, hooks, intrinsic) = {
        let w = &engine(vm).world.widgets[id.0];
        (w.scripts, w.hooks, w.intrinsic_scripts)
    };
    let key = vm.heap.intern(script.as_bytes());
    intrinsic
        || !vm.heap.table(scripts).get_str(key).is_nil()
        || !vm.heap.table(hooks).get_str(key).is_nil()
}

/// Whether a widget is visible: shown, and its parents too.
pub fn visible(e: &Engine, id: WidgetId) -> bool {
    let mut cur = Some(id);
    while let Some(c) = cur {
        let w = &e.world.widgets[c.0];
        if !w.shown {
            return false;
        }
        cur = w.parent;
    }
    true
}

/// Whether a widget is protected: itself (a secure template, `protected`
/// in its XML), or as the parent of a protected frame (decision T254: in
/// the client a frame that holds a protected frame is protected too, its
/// `IsProtected()` true but not explicitly; found in game: the spells
/// window, protected through its secure children).
pub fn protected(e: &Engine, id: WidgetId) -> bool {
    let mut stack = vec![id];
    while let Some(w) = stack.pop() {
        let widget = &e.world.widgets[w.0];
        if widget.protected {
            return true;
        }
        stack.extend(widget.children.iter().copied());
    }
    false
}

/// The widget and its descendants, children before their parent (deepest
/// first, siblings in creation order).
/// A widget and those under it through shown ones, children first.
fn shown_subtree(e: &Engine, id: WidgetId) -> Vec<WidgetId> {
    fn visit(e: &Engine, id: WidgetId, out: &mut Vec<WidgetId>) {
        for &c in &e.world.widgets[id.0].children {
            if e.world.widgets[c.0].shown {
                visit(e, c, out);
            }
        }
        out.push(id);
    }
    let mut out = Vec::new();
    visit(e, id, &mut out);
    out
}

/// Shows or hides a widget; `OnShow` / `OnHide` run for each widget whose
/// visibility changed, children before their parent (`/stdtest engine`:
/// "g- c- p-", "g+ c+").
pub fn set_shown(vm: &mut Vm, id: WidgetId, shown: bool) {
    // (shown or hidden as it is: nothing is shown or hidden, no script
    // runs; a hidden widget's focus still goes; T545)
    if engine(vm).world.widgets[id.0].shown == shown {
        if !shown {
            crate::input::drop_hidden_focus(vm);
        }
        return;
    }
    reorder(engine_mut(vm));
    // (the widgets whose visibility changes: it and those under it
    // through shown ones, while its parent shows; children first, T545)
    let changing = match engine(vm).world.widgets[id.0].parent {
        Some(p) if !visible(engine(vm), p) => Vec::new(),
        _ => shown_subtree(engine(vm), id),
    };
    let was = !shown;
    // (a hiding widget's frames and regions are no longer drawn: it alone
    // is marked, for its parent's; a showing one's are marked, T545)
    if !shown {
        mark_alone(engine_mut(vm), id);
    }
    engine_mut(vm).world.widgets[id.0].shown = shown;
    if shown {
        let e = engine_mut(vm);
        if let Some(m) = e.world.scene_marks.get_mut(id.0) {
            *m = false;
        }
        mark(e, id);
    }
    for w in changing {
        let now = visible(engine(vm), w);
        if now != was {
            run_script(vm, w, if now { "OnShow" } else { "OnHide" }, &[]);
        }
    }
    if !shown {
        crate::input::drop_hidden_focus(vm);
    }
}

fn show(vm: &mut Vm, id: WidgetId, _c: NativeCall) -> Ret {
    set_shown(vm, id, true);
    Ok(vec![])
}

fn hide(vm: &mut Vm, id: WidgetId, _c: NativeCall) -> Ret {
    set_shown(vm, id, false);
    Ok(vec![])
}

fn set_shown_method(vm: &mut Vm, id: WidgetId, c: NativeCall) -> Ret {
    set_shown(vm, id, c.arg(2).truthy());
    Ok(vec![])
}

fn is_shown(vm: &mut Vm, id: WidgetId, _c: NativeCall) -> Ret {
    Ok(vec![Value::Bool(engine(vm).world.widgets[id.0].shown)])
}

fn is_visible(vm: &mut Vm, id: WidgetId, _c: NativeCall) -> Ret {
    Ok(vec![Value::Bool(visible(engine(vm), id))])
}

// ------------------------------------------------------------ identity

fn get_name(vm: &mut Vm, id: WidgetId, _c: NativeCall) -> Ret {
    let name = engine(vm).world.widgets[id.0].name.clone();
    Ok(vec![name.map_or(Value::Nil, |n| vm.str(&n))])
}

/// `GetDebugName()`: the name; else the parent's debug name and the key
/// the parent holds it under, or its address (recorded: `Parent.Child`,
/// `UIParent.75fbc94630`, `75fbc944a0`; the engine's addresses are its
/// own).
fn debug_name(vm: &mut Vm, id: WidgetId) -> String {
    let w = &engine(vm).world.widgets[id.0];
    if let Some(name) = w.name.clone() {
        return name;
    }
    let me = w.table;
    let address = format!("{:x}", 0x0075_fbc9_0000u64 + id.0 as u64 * 0x10);
    let Some(parent) = w.parent else {
        return address;
    };
    let ptable = engine(vm).world.widgets[parent.0].table;
    let key = vm
        .heap
        .table(ptable)
        .entries()
        .find_map(|(k, v)| match (k, v) {
            (Value::Str(k), Value::Table(t)) if t == me => {
                Some(String::from_utf8_lossy(vm.str_bytes(k)).into_owned())
            }
            _ => None,
        });
    format!("{}.{}", debug_name(vm, parent), key.unwrap_or(address))
}

fn get_debug_name(vm: &mut Vm, id: WidgetId, _c: NativeCall) -> Ret {
    let name = debug_name(vm, id);
    Ok(vec![vm.str(&name)])
}

/// `GetObjectTable()`: the object's own table (Blizzard's aura containers
/// hand it to an addon's button initializer, which builds on it as on the
/// frame; unverified: a forbidden object's).
fn get_object_table(vm: &mut Vm, id: WidgetId, _c: NativeCall) -> Ret {
    Ok(vec![Value::Table(engine(vm).world.widgets[id.0].table)])
}

fn get_object_type(vm: &mut Vm, id: WidgetId, _c: NativeCall) -> Ret {
    let class = engine(vm).world.widgets[id.0].class.clone();
    // As the client names them (`/stdtest engine`).
    let shown = match class.as_str() {
        "AuraContainer" => "Frame",
        "Checkout" => "BlizzardCheckout",
        other => other,
    };
    Ok(vec![vm.str(shown)])
}

/// The documentation's mixin classes: not object types
/// (`IsObjectType("ScriptRegion")` is false, `/stdtest engine`).
const MIXIN_CLASSES: &[&str] = &[
    "ScriptObject",
    "FrameScriptObject",
    "ScriptRegion",
    "ScriptRegionResizing",
    "TextureBase",
    "AnimatableObject",
];

/// `IsObjectType(type)`: the widget's object types (Object, Region, Frame,
/// Button, …; lines and mask textures are textures too).
fn is_object_type(vm: &mut Vm, id: WidgetId, c: NativeCall) -> Ret {
    let t = vm.check_str(&c, 2)?;
    let t = String::from_utf8_lossy(vm.str_bytes(t)).into_owned();
    let class = engine(vm).world.widgets[id.0].class.clone();
    let e = engine(vm);
    let texture =
        matches!(class.as_str(), "Line" | "MaskTexture") && t.eq_ignore_ascii_case("Texture");
    let yes = texture
        || lineage(e, &class).iter().any(|c| {
            c.eq_ignore_ascii_case(&t) && !MIXIN_CLASSES.iter().any(|m| m.eq_ignore_ascii_case(c))
        });
    Ok(vec![Value::Bool(yes)])
}

fn get_parent(vm: &mut Vm, id: WidgetId, _c: NativeCall) -> Ret {
    let e = engine(vm);
    Ok(vec![e.world.widgets[id.0].parent.map_or(Value::Nil, |p| {
        Value::Table(e.world.widgets[p.0].table)
    })])
}

fn set_parent(vm: &mut Vm, id: WidgetId, c: NativeCall) -> Ret {
    reorder(engine_mut(vm));
    mark(engine_mut(vm), id);
    let parent = match c.arg(2) {
        Value::Nil => None,
        Value::Str(s) => {
            let name = String::from_utf8_lossy(vm.str_bytes(s)).into_owned();
            let g = vm.get_global(&name);
            widget_of(vm, g)
        }
        v => widget_of(vm, v),
    };
    let before = visible(engine(vm), id);
    let world = &mut engine_mut(vm).world;
    if let Some(old) = world.widgets[id.0].parent {
        world.widgets[old.0].children.retain(|&w| w != id);
    }
    world.widgets[id.0].parent = parent;
    if let Some(p) = parent {
        world.widgets[p.0].children.push(id);
    }
    world.layout_changed = true;
    let after = visible(engine(vm), id);
    if before != after {
        run_script(vm, id, if after { "OnShow" } else { "OnHide" }, &[]);
    }
    if !after {
        crate::input::drop_hidden_focus(vm);
    }
    Ok(vec![])
}

fn get_children(vm: &mut Vm, id: WidgetId, _c: NativeCall) -> Ret {
    let e = engine(vm);
    Ok(e.world.widgets[id.0]
        .children
        .iter()
        .filter(|&&k| is_a(e, &e.world.widgets[k.0].class, "Frame"))
        .map(|&k| Value::Table(e.world.widgets[k.0].table))
        .collect())
}

fn get_regions(vm: &mut Vm, id: WidgetId, _c: NativeCall) -> Ret {
    let e = engine(vm);
    Ok(e.world.widgets[id.0]
        .children
        .iter()
        .filter(|&&k| {
            let class = &e.world.widgets[k.0].class;
            !is_a(e, class, "Frame") && is_a(e, class, "Region")
        })
        .map(|&k| Value::Table(e.world.widgets[k.0].table))
        .collect())
}

fn get_num_children(vm: &mut Vm, id: WidgetId, c: NativeCall) -> Ret {
    let n = get_children(vm, id, c)?.len();
    Ok(vec![Value::Num(n as f64)])
}

/// `CreateFontString`, `CreateTexture`, `CreateMaskTexture`, `CreateLine`:
/// a region of that class, named and parented.
fn create_region(vm: &mut Vm, id: WidgetId, c: NativeCall, class: &str) -> Ret {
    mark(engine_mut(vm), id);
    let name = match vm.opt_str(&c, 2)? {
        Some(s) => {
            let mut n = String::from_utf8_lossy(vm.str_bytes(s)).into_owned();
            let pname = engine(vm).world.widgets[id.0]
                .name
                .clone()
                .unwrap_or_default();
            n = n.replace("$parent", &pname).replace("$Parent", &pname);
            Some(n)
        }
        None => None,
    };
    let region = create(vm, class, name, Some(id));
    // `CreateTexture(name, layer, inherits, sublevel)`: the layer given (a
    // fresh region's recorded default otherwise).
    if let Some(layer) = vm.opt_str(&c, 3)? {
        let rid = widget_of(vm, region).expect("a widget");
        let sublevel = match c.arg(5) {
            Value::Num(n) => n,
            _ => 0.0,
        };
        store(
            vm,
            rid,
            "DrawLayer",
            &[Value::Str(layer), Value::Num(sublevel)],
        );
    }
    // `CreateFontString(name, layer, inherits)`: a font object's font, or
    // a template.
    if let Some(t) = vm.opt_str(&c, 4)? {
        let t = String::from_utf8_lossy(vm.str_bytes(t)).into_owned();
        let g = vm.get_global(&t);
        let is_font =
            widget_of(vm, g).is_some_and(|w| engine(vm).world.widgets[w.0].class == "Font");
        let rid = widget_of(vm, region).expect("a widget");
        if is_font {
            let props = engine(vm).world.widgets[rid.0].props;
            let list = vm.new_table();
            let _ = vm.heap.table_mut(list).set(Value::Num(1.0), g);
            let n = vm.str("n");
            let _ = vm.heap.table_mut(list).set(n, Value::Num(1.0));
            let key = vm.str("FontObject");
            let _ = vm.heap.table_mut(props).set(key, Value::Table(list));
        } else {
            apply_template(vm, region, &t)?;
        }
    }
    Ok(vec![region])
}

/// A table of the test's as the client's answer: a fresh copy, every
/// table in it too (what the test wrote carries its taint; the client's
/// answers are made by C, secure).
pub fn answer_copy(vm: &mut Vm, v: Value) -> Value {
    fn copy(
        vm: &mut Vm,
        v: Value,
        done: &mut std::collections::HashMap<TableRef, TableRef>,
    ) -> Value {
        let Value::Table(t) = v else {
            return v;
        };
        if let Some(c) = done.get(&t) {
            return Value::Table(*c);
        }
        let out = vm.new_table();
        done.insert(t, out);
        let mut pairs = Vec::new();
        let mut key = Value::Nil;
        while let Ok(Some((k, val))) = vm.heap.table(t).next(key) {
            pairs.push((k, val));
            key = k;
        }
        for (k, val) in pairs {
            let k = copy(vm, k, done);
            let val = copy(vm, val, done);
            let _ = vm.heap.table_mut(out).set(k, val);
        }
        Value::Table(out)
    }
    copy(vm, v, &mut std::collections::HashMap::new())
}

/// A model scene's actors, in the order made.
pub(crate) fn actors(vm: &mut Vm, id: WidgetId) -> TableRef {
    let props = engine(vm).world.widgets[id.0].props;
    let key = vm.str("actors");
    if let Value::Table(t) = vm.heap.table(props).get(key) {
        return t;
    }
    let t = vm.new_table();
    let _ = vm.heap.table_mut(props).set(key, Value::Table(t));
    t
}

/// `CreateActor(name, template)`: a model scene's actor (its parent, not
/// among its children: `GetNumActors`, `GetActorAtIndex`), its template
/// applied (unverified: the client's actors beyond their methods).
fn create_actor(vm: &mut Vm, id: WidgetId, c: NativeCall) -> Ret {
    let name = vm
        .opt_str(&c, 2)?
        .map(|s| String::from_utf8_lossy(vm.str_bytes(s)).into_owned());
    let actor = create(vm, "ModelSceneFrameActor", name, Some(id));
    if let Some(t) = vm.opt_str(&c, 3)? {
        let t = String::from_utf8_lossy(vm.str_bytes(t)).into_owned();
        apply_template(vm, actor, &t)?;
    }
    let list = actors(vm, id);
    let n = vm.heap.table(list).border();
    let _ = vm
        .heap
        .table_mut(list)
        .set(Value::Num((n + 1) as f64), actor);
    Ok(vec![actor])
}

fn get_num_actors(vm: &mut Vm, id: WidgetId, _c: NativeCall) -> Ret {
    let list = actors(vm, id);
    Ok(vec![Value::Num(vm.heap.table(list).border() as f64)])
}

fn get_actor_at_index(vm: &mut Vm, id: WidgetId, c: NativeCall) -> Ret {
    let list = actors(vm, id);
    let i = c.arg(2);
    Ok(match vm.heap.table(list).get(i) {
        Value::Nil => vec![],
        a => vec![a],
    })
}

// --------------------------------------------------------- color selects

/// A color select's color, white until set, kept in eight bits each (0.4
/// reads back 0.40000003576279); `OnColorSelect` runs for a color set by
/// code; the hue, saturation and value read back as `SetColorHSV` gave
/// them, else from the kept color (recorded).
fn color_of(vm: &Vm, id: WidgetId) -> [f64; 4] {
    engine(vm)
        .world
        .colors
        .get(&id.0)
        .copied()
        .unwrap_or([1.0, 1.0, 1.0, 1.0])
}

/// Sets a color select's color; `OnColorSelect(r, g, b)` runs.
fn select_color(vm: &mut Vm, id: WidgetId, rgb: [f64; 3]) {
    engine_mut(vm).world.color_hsv.remove(&id.0);
    let a = color_of(vm, id)[3];
    engine_mut(vm)
        .world
        .colors
        .insert(id.0, [rgb[0], rgb[1], rgb[2], a]);
    let args = rgb.map(Value::Num);
    run_script(vm, id, "OnColorSelect", &args);
}

fn set_color_rgb(vm: &mut Vm, id: WidgetId, c: NativeCall) -> Ret {
    let rgb = [2, 3, 4].map(|i| match c.arg(i) {
        Value::Num(n) => byte_float(vm, n),
        _ => 0.0,
    });
    select_color(vm, id, rgb);
    Ok(vec![])
}

fn get_color_rgb(vm: &mut Vm, id: WidgetId, _c: NativeCall) -> Ret {
    let [r, g, b, _] = color_of(vm, id);
    Ok(vec![Value::Num(r), Value::Num(g), Value::Num(b)])
}

/// `SetColorHSV(hue 0-360, saturation, value)`.
fn set_color_hsv(vm: &mut Vm, id: WidgetId, c: NativeCall) -> Ret {
    let num = |v: Value| match v {
        Value::Num(n) => n,
        _ => 0.0,
    };
    let (h, s, v) = (
        num(c.arg(2)).rem_euclid(360.0),
        num(c.arg(3)).clamp(0.0, 1.0),
        num(c.arg(4)).clamp(0.0, 1.0),
    );
    // In single floats: the hue times 1/60, the sector's channels, each
    // byte rounded half to even (recorded: 300 is sector 4 at its end, 0.3
    // is 76).
    let (h32, s32, v32) = (h as f32, s as f32, v as f32);
    let hp = h32 * (1.0f32 / 60.0);
    let sector = hp.floor();
    let fr = hp - sector;
    let p = v32 * (1.0 - s32);
    let q = v32 * (1.0 - s32 * fr);
    let t = v32 * (1.0 - s32 * (1.0 - fr));
    let rgb = match (sector as i32).rem_euclid(6) {
        0 => [v32, t, p],
        1 => [q, v32, p],
        2 => [p, v32, t],
        3 => [p, q, v32],
        4 => [t, p, v32],
        _ => [v32, p, q],
    };
    let platform = engine(vm).platform;
    let byte = |x: f32| crate::byte_channel(platform, (x * 255.0).round_ties_even());
    select_color(vm, id, rgb.map(byte));
    let single = |x: f64| f64::from(x as f32);
    engine_mut(vm)
        .world
        .color_hsv
        .insert(id.0, [single(h), single(s), single(v)]);
    Ok(vec![])
}

fn get_color_hsv(vm: &mut Vm, id: WidgetId, _c: NativeCall) -> Ret {
    if let Some(hsv) = engine(vm).world.color_hsv.get(&id.0) {
        return Ok(hsv.map(Value::Num).to_vec());
    }
    // From the kept bytes times the float 1/255 on either machine (recorded:
    // the Mac's and Windows' hue, saturation and value alike, though their
    // RGB differ in the last digits, decision T236).
    let [r, g, b, _] = color_of(vm, id)
        .map(|x| crate::byte_channel(crate::Platform::Mac, (x * 255.0).round() as f32));
    let max = r.max(g).max(b);
    let min = r.min(g).min(b);
    let d = max - min;
    let h = if d == 0.0 {
        0.0
    } else if max == r {
        60.0 * ((g - b) / d).rem_euclid(6.0)
    } else if max == g {
        60.0 * ((b - r) / d + 2.0)
    } else {
        60.0 * ((r - g) / d + 4.0)
    };
    let s = if max == 0.0 { 0.0 } else { d / max };
    let single = |x: f64| Value::Num(f64::from(x as f32));
    Ok(vec![single(h), single(s), single(max)])
}

fn set_color_alpha(vm: &mut Vm, id: WidgetId, c: NativeCall) -> Ret {
    let a = match c.arg(2) {
        Value::Num(n) => n.clamp(0.0, 1.0),
        _ => 1.0,
    };
    let mut color = color_of(vm, id);
    color[3] = a;
    engine_mut(vm).world.colors.insert(id.0, color);
    Ok(vec![])
}

fn get_color_alpha(vm: &mut Vm, id: WidgetId, _c: NativeCall) -> Ret {
    Ok(vec![Value::Num(color_of(vm, id)[3])])
}

// ------------------------------------------------------- message frames

/// Calls `object:method(args…)` as Lua would.
pub(crate) fn call_method(vm: &mut Vm, object: Value, method: &str, args: &[Value]) -> Ret {
    let key = vm.str(method);
    let f = vm.index_slot(object, key, None)?.v;
    let mut all = vec![object];
    all.extend_from_slice(args);
    vm.call(f, &all)
}

/// `MessageFrame:AddMessage(text, r, g, b, a, messageID)`: a font string
/// of the frame shows it (`GetFontStringByID` finds it by its ID).
/// Unverified: fading, the line limit, the layout.
fn add_message(vm: &mut Vm, id: WidgetId, c: NativeCall) -> Ret {
    let class = engine(vm).world.widgets[id.0].class.clone();
    if !is_a(engine(vm), &class, "MessageFrame") {
        return Ok(vec![]);
    }
    let fs = create(vm, "FontString", None, Some(id));
    keep(vm, fs);
    // The frame's font.
    let me = Value::Table(engine(vm).world.widgets[id.0].table);
    let font = call_method(vm, me, "GetFontObject", &[])?;
    match font.first().copied() {
        Some(f) if widget_of(vm, f).is_some() => {
            call_method(vm, fs, "SetFontObject", &[f])?;
        }
        _ => {
            let font = call_method(vm, me, "GetFont", &[])?;
            if matches!(font.first(), Some(Value::Str(_))) {
                call_method(vm, fs, "SetFont", &font)?;
            }
        }
    }
    call_method(vm, fs, "SetText", &[c.arg(2)])?;
    let color: Vec<Value> = (3..=6).map(|i| c.arg(i)).collect();
    if color[..3].iter().all(|v| matches!(v, Value::Num(_))) {
        let color = if color[3].is_nil() {
            &color[..3]
        } else {
            &color[..]
        };
        call_method(vm, fs, "SetTextColor", color)?;
    }
    let message_id = match c.arg(7) {
        Value::Num(n) => Some(n),
        _ => None,
    };
    engine_mut(vm)
        .world
        .messages
        .entry(id.0)
        .or_default()
        .push((message_id, fs));
    // (a chat window's line, unless its hook logs it: the session's
    // chat, T514)
    if !engine(vm).world.chat_hooked.contains(&id.0) {
        log_chat_line(vm, id, &c);
    }
    Ok(vec![])
}

/// Whether a widget is a chat window (`ChatFrame1` and on).
pub fn chat_window(vm: &Vm, id: WidgetId) -> bool {
    engine(vm).world.widgets[id.0]
        .name
        .as_deref()
        .and_then(|n| n.strip_prefix("ChatFrame"))
        .is_some_and(|d| !d.is_empty() && d.bytes().all(|b| b.is_ascii_digit()))
}

/// A chat window's `AddMessage(text, r, g, b)` kept in the session's chat
/// log (decision T514): `c`'s arguments from the frame on.
pub fn log_chat_line(vm: &mut Vm, id: WidgetId, c: &NativeCall) {
    if !chat_window(vm, id) {
        return;
    }
    let frame = engine(vm).world.widgets[id.0]
        .name
        .clone()
        .unwrap_or_default();
    let text = match vm.hidden(c.arg(2)) {
        Value::Str(s) => String::from_utf8_lossy(vm.str_bytes(s)).into_owned(),
        Value::Num(n) => wowlua_vm::number::fmt_num(n),
        _ => String::new(),
    };
    let rgb: Vec<f64> = (3..=5)
        .filter_map(|i| match c.arg(i) {
            Value::Num(n) => Some(n),
            _ => None,
        })
        .collect();
    let world = &mut engine_mut(vm).world;
    let seq = world.chat_log_next;
    world.chat_log_next += 1;
    world.chat_log.push_back(ChatLine {
        seq,
        frame,
        text,
        color: (rgb.len() == 3).then(|| [rgb[0], rgb[1], rgb[2]]),
    });
    while world.chat_log.len() > CHAT_LOG_LINES {
        world.chat_log.pop_front();
    }
}

/// `GetFontStringByID(messageID)`: the font string of the last message
/// with that ID, nil without one.
fn get_font_string_by_id(vm: &mut Vm, id: WidgetId, c: NativeCall) -> Ret {
    let Value::Num(wanted) = c.arg(2) else {
        return Ok(vec![Value::Nil]);
    };
    let found = engine(vm).world.messages.get(&id.0).and_then(|list| {
        list.iter()
            .rev()
            .find(|(m, _)| *m == Some(wanted))
            .map(|(_, fs)| *fs)
    });
    Ok(vec![found.unwrap_or(Value::Nil)])
}

/// `Clear()`: a message frame's messages gone (other classes' `Clear`
/// does nothing yet).
fn clear_messages(vm: &mut Vm, id: WidgetId, _c: NativeCall) -> Ret {
    let list = engine_mut(vm)
        .world
        .messages
        .remove(&id.0)
        .unwrap_or_default();
    for (_, fs) in list {
        call_method(vm, fs, "Hide", &[])?;
    }
    Ok(vec![])
}

/// `ResetMessageFadeByID(messageID)`: nothing fades yet.
fn reset_message_fade(_vm: &mut Vm, _id: WidgetId, _c: NativeCall) -> Ret {
    Ok(vec![])
}

// -------------------------------------------------------------- layout

fn number_arg(vm: &mut Vm, c: &NativeCall, n: usize) -> Result<f64, LuaError> {
    vm.check_number(c, n)
}

/// The anchor points a region has.
const POINTS: &[&str] = &[
    "TOPLEFT",
    "TOP",
    "TOPRIGHT",
    "LEFT",
    "CENTER",
    "RIGHT",
    "BOTTOMLEFT",
    "BOTTOM",
    "BOTTOMRIGHT",
];

fn set_point(vm: &mut Vm, id: WidgetId, c: NativeCall) -> Ret {
    mark(engine_mut(vm), id);
    let class = engine(vm).world.widgets[id.0].class.clone();
    // Secrets are the values they hide here: the method refused them
    // before, unless allowed (untainted, as Blizzard's aura containers
    // anchor their buttons: `SetPoint(secretwrap(...))`).
    let arg = |vm: &Vm, n: usize| vm.hidden(c.arg(n));
    let Value::Str(point) = arg(vm, 2) else {
        return Err(c_error(
            vm,
            &format!(
                "{class}:SetPoint(): Usage: (\"point\" [, region or nil] [, \"relativePoint\"] [, offsetX, offsetY]"
            ),
            false,
        ));
    };
    let raw = String::from_utf8_lossy(vm.str_bytes(point)).into_owned();
    let point = raw.to_uppercase();
    if !POINTS.contains(&point.as_str()) {
        return Err(c_error(
            vm,
            &format!("{class}:SetPoint(): Invalid region point {raw}"),
            false,
        ));
    }
    // SetPoint(point [, relativeTo [, relativePoint]] [, x, y])
    let (relative, relative_point, x, y) = match arg(vm, 3) {
        Value::Num(_) => (
            None,
            point.clone(),
            number_arg(vm, &c, 3)?,
            vm.opt_number(&c, 4, 0.0)?,
        ),
        rel => {
            let relative = match rel {
                Value::Nil => None,
                Value::Str(s) => {
                    let name = String::from_utf8_lossy(vm.str_bytes(s)).into_owned();
                    Some(vm.get_global(&name))
                }
                v => Some(v),
            };
            let rp = match arg(vm, 4) {
                Value::Str(s) => String::from_utf8_lossy(vm.str_bytes(s)).to_uppercase(),
                _ => point.clone(),
            };
            let (xi, yi) = if matches!(arg(vm, 4), Value::Num(_)) {
                (4, 5)
            } else {
                (5, 6)
            };
            (
                relative,
                rp,
                vm.opt_number(&c, xi, 0.0)?,
                vm.opt_number(&c, yi, 0.0)?,
            )
        }
    };
    refuse_region_anchor(vm, id, relative, "SetPoint")?;
    let w = &mut engine_mut(vm).world.widgets[id.0];
    w.points.retain(|p| p.point != point);
    w.points.push(Point {
        point,
        relative,
        relative_point,
        x,
        y,
    });
    note_anchors(vm, id);
    layout_changed(vm);
    Ok(vec![])
}

/// `SetToDefaults()` of a region or frame: its anchors cleared, its size
/// 0 by 0 (Blizzard's Compositor.lua: "The anchors were already cleared
/// in SetToDefaults()", "SetToDefaults() will set the frame's size to
/// 0,0"; its menus' pooled textures are anchored anew after it).
/// Unverified: what else it resets.
fn set_to_defaults(vm: &mut Vm, id: WidgetId, _c: NativeCall) -> Ret {
    mark(engine_mut(vm), id);
    let w = &mut engine_mut(vm).world.widgets[id.0];
    w.points.clear();
    w.width = 0.0;
    w.height = 0.0;
    layout_changed(vm);
    Ok(vec![])
}

/// `ClearPoint(point)`: that point gone, the others kept (Blizzard's
/// menus flip one this way to open above their owner).
fn clear_point(vm: &mut Vm, id: WidgetId, c: NativeCall) -> Ret {
    let Value::Str(name) = c.arg(2) else {
        return Ok(vec![]);
    };
    let name = String::from_utf8_lossy(vm.str_bytes(name)).into_owned();
    let points = &mut engine_mut(vm).world.widgets[id.0].points;
    let before = points.len();
    points.retain(|p| !p.point.eq_ignore_ascii_case(&name));
    if points.len() != before {
        layout_changed(vm);
    }
    Ok(vec![])
}

fn clear_all_points(vm: &mut Vm, id: WidgetId, _c: NativeCall) -> Ret {
    mark(engine_mut(vm), id);
    engine_mut(vm).world.widgets[id.0].points.clear();
    layout_changed(vm);
    Ok(vec![])
}

fn set_all_points(vm: &mut Vm, id: WidgetId, c: NativeCall) -> Ret {
    mark(engine_mut(vm), id);
    let rel = match c.arg(2) {
        Value::Nil => None,
        v => Some(v),
    };
    refuse_region_anchor(vm, id, rel, "SetAllPoints")?;
    let w = &mut engine_mut(vm).world.widgets[id.0];
    w.points.clear();
    for p in ["TOPLEFT", "BOTTOMRIGHT"] {
        w.points.push(Point {
            point: p.into(),
            relative: rel,
            relative_point: p.into(),
            x: 0.0,
            y: 0.0,
        });
    }
    note_anchors(vm, id);
    layout_changed(vm);
    Ok(vec![])
}

/// A protected frame anchored to a region (a texture, a font string) is
/// refused, out of combat too, with the client's error (found in game,
/// SuuUI's spellbook: "Action[SetPoint] failed because[Cannot anchor
/// protected frames to regions]: attempted from: Button:SetPoint.";
/// decision T252). Unverified: `SetAllPoints`'s wording (taken to be the
/// same, named after it).
fn refuse_region_anchor(
    vm: &mut Vm,
    id: WidgetId,
    relative: Option<Value>,
    method: &str,
) -> Result<(), LuaError> {
    let Some(rel) = relative.and_then(|v| widget_of(vm, v)) else {
        return Ok(());
    };
    let e = engine(vm);
    let region = matches!(
        e.world.widgets[rel.0].class.as_str(),
        "Texture" | "FontString" | "Line" | "MaskTexture"
    );
    if !e.world.widgets[id.0].protected || !region {
        return Ok(());
    }
    let class = e.world.widgets[id.0].class.clone();
    Err(vm.error(&format!(
        "Action[{method}] failed because[Cannot anchor protected frames to regions]: attempted from: {class}:{method}."
    )))
}

/// A line has start and end points, not anchors (`/stdtest engine`).
fn not_for_lines(vm: &mut Vm, id: WidgetId, method: &str) -> Result<(), LuaError> {
    let class = engine(vm).world.widgets[id.0].class.clone();
    if class == "Line" {
        return Err(c_error(
            vm,
            &format!("{class}:{method}(): Wrong object type for function"),
            true,
        ));
    }
    Ok(())
}

/// `GetRect()`: left, bottom, width and height, nothing while unanchored.
fn get_rect(vm: &mut Vm, id: WidgetId, _c: NativeCall) -> Ret {
    let s = layout_scale(engine(vm), id);
    Ok(match bounds(vm, id, 0) {
        Some(b) if !reported_points(vm, id).is_empty() => vec![
            Value::Num(b.0 / s),
            Value::Num(b.1 / s),
            Value::Num(b.2 / s),
            Value::Num(b.3 / s),
        ],
        _ => vec![],
    })
}

/// A widget's points as `GetPoint` reports them. A slider's thumb texture
/// without points of its own has the one the slider gives it: `LEFT` to the
/// slider's `LEFT`, x along the free length, y 0 (probed in WoW Forever,
/// 2026-10-23); vertical, `TOP` to `TOP` with y down (unverified).
fn reported_points(vm: &Vm, id: WidgetId) -> Vec<Point> {
    let w = &engine(vm).world.widgets[id.0];
    if !w.points.is_empty() {
        return w.points.clone();
    }
    let Some((slider, at, vertical)) = crate::bars::thumb_place(vm, id) else {
        return vec![];
    };
    let Some(r) = bounds(vm, slider, 0) else {
        return vec![];
    };
    let s = layout_scale(engine(vm), id);
    let table = Value::Table(engine(vm).world.widgets[slider.0].table);
    let (point, x, y) = if vertical {
        ("TOP", 0.0, -at * (r.3 / s - w.height).max(0.0))
    } else {
        ("LEFT", at * (r.2 / s - w.width).max(0.0), 0.0)
    };
    vec![Point {
        point: point.into(),
        relative: Some(table),
        relative_point: point.into(),
        x,
        y,
    }]
}

fn get_num_points(vm: &mut Vm, id: WidgetId, _c: NativeCall) -> Ret {
    not_for_lines(vm, id, "GetNumPoints")?;
    Ok(vec![Value::Num(reported_points(vm, id).len() as f64)])
}

fn get_point(vm: &mut Vm, id: WidgetId, c: NativeCall) -> Ret {
    not_for_lines(vm, id, "GetPoint")?;
    let i = vm.opt_int(&c, 2, 1)?;
    let Some(p) = reported_points(vm, id)
        .get((i - 1).max(0) as usize)
        .cloned()
    else {
        return Ok(vec![]);
    };
    let parent = engine(vm).world.widgets[id.0]
        .parent
        .map(|p| Value::Table(engine(vm).world.widgets[p.0].table));
    let point = vm.str(&p.point);
    let rp = vm.str(&p.relative_point);
    Ok(vec![
        point,
        p.relative.or(parent).unwrap_or(Value::Nil),
        rp,
        Value::Num(p.x),
        Value::Num(p.y),
    ])
}

/// `GetPointByName(point)`: the anchor of that point as `GetPoint` gives
/// it; nothing without one. Unverified: `resolveCollapsed`.
fn get_point_by_name(vm: &mut Vm, id: WidgetId, c: NativeCall) -> Ret {
    let Value::Str(name) = c.arg(2) else {
        return Ok(vec![]);
    };
    let name = String::from_utf8_lossy(vm.str_bytes(name)).to_uppercase();
    let Some(p) = reported_points(vm, id)
        .into_iter()
        .find(|p| p.point.eq_ignore_ascii_case(&name))
    else {
        return Ok(vec![]);
    };
    let parent = engine(vm).world.widgets[id.0]
        .parent
        .map(|p| Value::Table(engine(vm).world.widgets[p.0].table));
    let point = vm.str(&p.point);
    let rp = vm.str(&p.relative_point);
    Ok(vec![
        point,
        p.relative.or(parent).unwrap_or(Value::Nil),
        rp,
        Value::Num(p.x),
        Value::Num(p.y),
    ])
}

fn set_size(vm: &mut Vm, id: WidgetId, c: NativeCall) -> Ret {
    let w = number_arg(vm, &c, 2)?;
    let h = number_arg(vm, &c, 3)?;
    resize(vm, id, Some(w), Some(h));
    Ok(vec![])
}

fn set_width(vm: &mut Vm, id: WidgetId, c: NativeCall) -> Ret {
    let w = number_arg(vm, &c, 2)?;
    resize(vm, id, Some(w), None);
    Ok(vec![])
}

fn set_height(vm: &mut Vm, id: WidgetId, c: NativeCall) -> Ret {
    let h = number_arg(vm, &c, 2)?;
    resize(vm, id, None, Some(h));
    Ok(vec![])
}

/// Sets the size; `OnSizeChanged` runs when it changes.
fn resize(vm: &mut Vm, id: WidgetId, w: Option<f64>, h: Option<f64>) {
    mark(engine_mut(vm), id);
    let widget = &mut engine_mut(vm).world.widgets[id.0];
    let old = (widget.width, widget.height);
    widget.width = w.unwrap_or(old.0);
    widget.height = h.unwrap_or(old.1);
    let new = (widget.width, widget.height);
    // `OnSizeChanged` waits: an anchored frame's runs before the next
    // `OnUpdate` (also when resized in `OnUpdate`), a frame without points
    // never gets one (`/stdtest engine`).
    let world = &mut engine_mut(vm).world;
    if new != old {
        world.layout_changed = true;
    }
    if new != old && world.resizing.insert(id.0) {
        world.resized.push(id);
    }
}

/// `OnSizeChanged` for the widgets resized since the last frame: those
/// `SetSize` resized, then those whose size follows their anchors and
/// changed with them (a `SetAllPoints` child of a resized frame, a
/// template's part anchored at both ends), with their laid-out size, as
/// the client runs it whenever the rectangle's size changes (SuuUI's
/// tests; unverified: their order).
pub fn run_size_changes(vm: &mut Vm) {
    let world = &mut engine_mut(vm).world;
    world.resizing.clear();
    let resized = std::mem::take(&mut world.resized);
    let mut told = std::collections::HashSet::new();
    for id in resized {
        let w = &engine(vm).world.widgets[id.0];
        if w.points.is_empty() {
            continue;
        }
        let size = [Value::Num(w.width), Value::Num(w.height)];
        // An object whose untrusted layout scripts are forbidden runs only
        // its secure ones (the documentation's `UntrustedLayoutScriptExecution`;
        // unverified by a probe).
        let trusted_only = w.forbidden_aspects & crate::protection::UNTRUSTED_LAYOUT_SCRIPTS != 0;
        told.insert(id.0);
        run_script_of(vm, id, "OnSizeChanged", &size, trusted_only);
    }
    if !std::mem::take(&mut engine_mut(vm).world.layout_changed) {
        return;
    }
    // The anchored widgets that listen, each one's size now.
    let key = vm.heap.intern(b"OnSizeChanged");
    let watching: Vec<WidgetId> = {
        let e = engine(vm);
        e.world
            .size_watching
            .iter()
            .filter_map(|&i| Some((i, e.world.widgets.get(i)?)))
            .filter(|(_, w)| !w.points.is_empty())
            .filter(|(_, w)| {
                !vm.heap.table(w.scripts).get_str(key).is_nil()
                    || !vm.heap.table(w.hooks).get_str(key).is_nil()
            })
            .map(|(i, _)| WidgetId(i))
            .collect()
    };
    let mut known = KnownBounds::default();
    let mut changed = Vec::new();
    for id in watching {
        let Some(b) = bounds_in(vm, id, 0, &mut known) else {
            continue;
        };
        let s = layout_scale(engine(vm), id);
        let now = (b.2 / s, b.3 / s);
        let w = &engine(vm).world.widgets[id.0];
        let set = (w.width, w.height);
        let before = engine_mut(vm)
            .world
            .laid_out
            .insert(id.0, now)
            .unwrap_or(set);
        let moved = (now.0 - before.0).abs() > 1e-6 || (now.1 - before.1).abs() > 1e-6;
        if moved && !told.contains(&id.0) {
            changed.push((id, now));
        }
    }
    for (id, (width, height)) in changed {
        let trusted_only = engine(vm).world.widgets[id.0].forbidden_aspects
            & crate::protection::UNTRUSTED_LAYOUT_SCRIPTS
            != 0;
        let size = [Value::Num(width), Value::Num(height)];
        run_script_of(vm, id, "OnSizeChanged", &size, trusted_only);
    }
}

/// A widget's points, size, scale or parent changed: the next frame
/// looks for sizes that followed ([`run_size_changes`]).
pub(crate) fn layout_changed(vm: &mut Vm) {
    engine_mut(vm).world.layout_changed = true;
}

/// The `OnTextChanged(false)` this frame's `SetText` owes, in the frames'
/// update after `OnUpdate`: a hidden edit box's waits until it shows, then
/// comes in that frame, once (`/stdtest engine textchange`, recorded).
pub fn run_text_changes(vm: &mut Vm) {
    let set = std::mem::take(&mut engine_mut(vm).world.text_set);
    let (now, later): (Vec<WidgetId>, Vec<WidgetId>) =
        set.into_iter().partition(|id| visible(engine(vm), *id));
    engine_mut(vm).world.text_set = later;
    for id in now.into_iter().rev() {
        run_script(vm, id, "OnTextChanged", &[Value::Bool(false)]);
    }
}

/// A region's size: as laid out when its points give it one (two opposite
/// points, `SetAllPoints`), else as set (unverified: recorded for fresh,
/// unanchored widgets only).
pub(crate) fn laid_out_size(vm: &mut Vm, id: WidgetId) -> (f64, f64) {
    let (w, h, anchored) = {
        let r = &engine(vm).world.widgets[id.0];
        (r.width, r.height, !r.points.is_empty())
    };
    let s = layout_scale(engine(vm), id);
    match bounds(vm, id, 0).filter(|_| anchored) {
        Some(b) => (b.2 / s, b.3 / s),
        None => unplaced_size(vm, id, w, h),
    }
}

/// A widget's size without a rectangle: as set, a font string's sides
/// not set its text's ([`text_extent`]).
fn unplaced_size(vm: &Vm, id: WidgetId, w: f64, h: f64) -> (f64, f64) {
    if engine(vm).world.widgets[id.0].class != "FontString" || (w != 0.0 && h != 0.0) {
        return (w, h);
    }
    let w = if w == 0.0 {
        text_extent(vm, id, None).0
    } else {
        w
    };
    let h = if h == 0.0 {
        text_extent(vm, id, Some(w)).1
    } else {
        h
    };
    (w, h)
}

/// The stored arguments of setter `stem`, read without interning (for the
/// layout, which only reads the world).
/// Argument `i` (from 0) of what setter `stem` stored, without the list
/// (decision T541).
pub(crate) fn stored_arg(vm: &Vm, props: TableRef, stem: &str, i: usize) -> Option<Value> {
    let key = vm.heap.interned(stem.as_bytes())?;
    let Value::Table(list) = vm.heap.table(props).get(Value::Str(key)) else {
        return None;
    };
    let n = vm.heap.interned(b"n")?;
    match vm.heap.table(list).get(Value::Str(n)) {
        Value::Num(n) if (n as usize) > i => {
            Some(vm.heap.table(list).get(Value::Num((i + 1) as f64)))
        }
        _ => None,
    }
}

pub(crate) fn stored_ref(vm: &Vm, props: TableRef, stem: &str) -> Option<Vec<Value>> {
    let key = vm.heap.interned(stem.as_bytes())?;
    let Value::Table(list) = vm.heap.table(props).get(Value::Str(key)) else {
        return None;
    };
    let n = vm.heap.interned(b"n")?;
    let count = match vm.heap.table(list).get(Value::Str(n)) {
        Value::Num(n) => n as usize,
        _ => 0,
    };
    Some(
        (1..=count)
            .map(|k| vm.heap.table(list).get(Value::Num(k as f64)))
            .collect(),
    )
}

/// What setter `stem` stored on the region or along its font objects, as
/// `methods2::font_chain` reads it, without changing the VM (for layout).
pub(crate) fn font_chain_ref(vm: &Vm, id: WidgetId, stem: &str) -> Option<Vec<Value>> {
    let mut cur = id;
    for _ in 0..32 {
        let props = engine(vm).world.widgets[cur.0].props;
        if let Some(v) = stored_ref(vm, props, stem) {
            return Some(v);
        }
        match stored_ref(vm, props, "FontObject")
            .and_then(|v| v.first().copied())
            .and_then(|f| widget_of(vm, f))
        {
            Some(n) if n != cur => cur = n,
            _ => break,
        }
    }
    None
}

/// A font string's text as the client lays it out (`text.rs`, decision
/// T237): its widest line (as `GetStringWidth`), its lines, each wrapped
/// at `width` when given and wrapping, at the font's line height with the
/// spacing between them (as `GetStringHeight`). Without text, 1 by 1.
///
/// SuuUI probe trees: the client's font strings report their text's size
/// on the sides neither their size nor two points set (the character
/// window's check box labels 158 wide, as set; two-line texts twice a
/// line; empty ones 1, as a new font string is 1 by 1, `/stdtest
/// engine`).
fn text_extent(vm: &Vm, id: WidgetId, width: Option<f64>) -> (f64, f64) {
    let w = &engine(vm).world.widgets[id.0];
    let text = match w.text.map(|v| vm.hidden(v)) {
        Some(Value::Str(s)) => String::from_utf8_lossy(vm.heap.str_bytes(s)).into_owned(),
        Some(Value::Num(n)) => format!("{n}"),
        _ => String::new(),
    };
    if text.is_empty() {
        return (1.0, 1.0);
    }
    let spacing = match stored_ref(vm, w.props, "Spacing").and_then(|v| v.first().copied()) {
        Some(Value::Num(s)) => s,
        _ => 0.0,
    };
    // As the client lays text out (`text.rs`, decisions T237, T282).
    let widest = crate::text::width(vm, id, &text);
    let lines = crate::text::layout_in(vm, id, &text, width.filter(|w| *w > 0.0)).shown as f64;
    let h = lines * crate::text::line_height(vm, id) + (lines - 1.0) * spacing;
    (widest, h)
}

fn get_width(vm: &mut Vm, id: WidgetId, _c: NativeCall) -> Ret {
    Ok(vec![Value::Num(laid_out_size(vm, id).0)])
}

fn get_height(vm: &mut Vm, id: WidgetId, _c: NativeCall) -> Ret {
    Ok(vec![Value::Num(laid_out_size(vm, id).1)])
}

fn get_size(vm: &mut Vm, id: WidgetId, _c: NativeCall) -> Ret {
    let (w, h) = laid_out_size(vm, id);
    Ok(vec![Value::Num(w), Value::Num(h)])
}

fn set_scale(vm: &mut Vm, id: WidgetId, c: NativeCall) -> Ret {
    mark(engine_mut(vm), id);
    let s = number_arg(vm, &c, 2)?;
    if s <= 0.0 {
        let class = engine(vm).world.widgets[id.0].class.clone();
        return Err(c_error(
            vm,
            &format!("{class}:SetScale(): Scale must be > 0"),
            true,
        ));
    }
    engine_mut(vm).world.widgets[id.0].scale = s;
    layout_changed(vm);
    Ok(vec![])
}

fn get_scale(vm: &mut Vm, id: WidgetId, _c: NativeCall) -> Ret {
    Ok(vec![Value::Num(engine(vm).world.widgets[id.0].scale)])
}

/// The scale a region is laid out at: its effective scale over UIParent's,
/// so regions' rectangles are in UIParent's units (exact for UIParent's
/// descendants: their scales up to it; decision T237).
pub fn layout_scale(e: &Engine, id: WidgetId) -> f64 {
    let ui = e.world.ui_parent;
    let mut s = 1.0;
    let mut cur = Some(id);
    while let Some(c) = cur {
        if Some(c) == ui {
            return s;
        }
        let w = &e.world.widgets[c.0];
        s *= w.scale;
        if w.ignore_parent_scale {
            break;
        }
        cur = w.parent;
    }
    s / ui_parent_scale(e)
}

/// UIParent's scale (the screen's, decision T237).
pub fn ui_parent_scale(e: &Engine) -> f64 {
    e.world
        .ui_parent
        .map_or(1.0, |u| e.world.widgets[u.0].scale)
}

/// The scale times its parents' scales, up to one that ignores its
/// parent's (`SetIgnoreParentScale(true)`).
pub fn effective_scale(e: &Engine, id: WidgetId) -> f64 {
    let mut s = 1.0;
    let mut cur = Some(id);
    while let Some(c) = cur {
        let w = &e.world.widgets[c.0];
        s *= w.scale;
        if w.ignore_parent_scale {
            break;
        }
        cur = w.parent;
    }
    s
}

fn get_effective_scale(vm: &mut Vm, id: WidgetId, _c: NativeCall) -> Ret {
    Ok(vec![Value::Num(effective_scale(engine(vm), id))])
}

/// `SetIgnoreParentScale(ignore)`: the effective scale its own, or its
/// parents' too again.
fn set_ignore_parent_scale(vm: &mut Vm, id: WidgetId, c: NativeCall) -> Ret {
    mark(engine_mut(vm), id);
    let on = c.arg(2).truthy();
    engine_mut(vm).world.widgets[id.0].ignore_parent_scale = on;
    layout_changed(vm);
    store(vm, id, "IgnoreParentScale", &[Value::Bool(on)]);
    Ok(vec![])
}

fn is_ignoring_parent_scale(vm: &mut Vm, id: WidgetId, _c: NativeCall) -> Ret {
    Ok(vec![Value::Bool(
        engine(vm).world.widgets[id.0].ignore_parent_scale,
    )])
}

/// `IsIgnoringParentAlpha()`: what `SetIgnoreParentAlpha` set (the
/// generic setter keeps it as `IgnoreParentAlpha`, which the generic
/// `IsX` rule, reading `IgnoringParentAlpha`, misses).
fn is_ignoring_parent_alpha(vm: &mut Vm, id: WidgetId, _c: NativeCall) -> Ret {
    let props = engine(vm).world.widgets[id.0].props;
    let on = stored_args(vm, props, "IgnoreParentAlpha")
        .and_then(|v| v.first().copied())
        .is_some_and(|v| v.truthy());
    Ok(vec![Value::Bool(on)])
}

/// `SetAlpha(alpha)`: a secret too, even from tainted code (the API
/// documentation: `AllowedWhenTainted`, adding the alpha aspect); the
/// region's alpha then reads back secret, and keeps reading back secret
/// after a plain alpha is set (found in game: SuuUI's fade read a secret
/// alpha from a frame it had set to 1; decision T251). Unverified: what
/// takes the aspect away again.
fn set_alpha(vm: &mut Vm, id: WidgetId, c: NativeCall) -> Ret {
    mark(engine_mut(vm), id);
    let secret = matches!(c.arg(2), Value::Secret(_));
    let a = match vm.hidden(c.arg(2)) {
        Value::Num(n) if secret => n,
        _ => number_arg(vm, &c, 2)?,
    };
    // Kept in eight bits, read back as a float: 0.5 is 128/255
    // (`/stdtest engine`).
    let a = byte_float(vm, a);
    let w = &mut engine_mut(vm).world.widgets[id.0];
    w.alpha = a;
    // Once secret, the alpha stays secret (decision T251).
    w.secret_alpha |= secret;
    Ok(vec![])
}

/// `SetAlphaFromBoolean(value [, alphaIfTrue [, alphaIfFalse]])`: the
/// alpha one of the two picks (1 and 0 by default; unverified), a secret
/// `value` or alpha making the alpha secret, as `SetAlpha` with a secret.
fn set_alpha_from_boolean(vm: &mut Vm, id: WidgetId, c: NativeCall) -> Ret {
    mark(engine_mut(vm), id);
    let which = c.arg(2);
    let on = vm.hidden(which).truthy();
    let (picked, default) = if on { (c.arg(3), 1.0) } else { (c.arg(4), 0.0) };
    let secret = matches!(which, Value::Secret(_)) || matches!(picked, Value::Secret(_));
    let a = match vm.hidden(picked) {
        Value::Nil => default,
        Value::Num(n) => n,
        _ => return Err(vm.arg_error(if on { 3 } else { 4 }, "number expected")),
    };
    let a = byte_float(vm, a.clamp(0.0, 1.0));
    let w = &mut engine_mut(vm).world.widgets[id.0];
    w.alpha = a;
    w.secret_alpha |= secret;
    Ok(vec![])
}

fn get_alpha(vm: &mut Vm, id: WidgetId, _c: NativeCall) -> Ret {
    let w = &engine(vm).world.widgets[id.0];
    let (a, secret) = (Value::Num(w.alpha), w.secret_alpha);
    Ok(vec![if secret { vm.secret_wrap(a) } else { a }])
}

/// Where an anchor point sits along a width and height (0 to 1).
fn anchor(point: &str) -> (f64, f64) {
    let x = if point.contains("LEFT") {
        0.0
    } else if point.contains("RIGHT") {
        1.0
    } else {
        0.5
    };
    let y = if point.contains("BOTTOM") {
        0.0
    } else if point.contains("TOP") {
        1.0
    } else {
        0.5
    };
    (x, y)
}

/// The widget's rectangle in screen units (left, bottom, width, height);
/// UIParent is the screen. Each edge comes from the points that set it (a
/// `LEFT` and a `RIGHT` point give the width, a `TOP` and a `BOTTOM` the
/// height); otherwise the size set, placed by the axis's first point.
pub fn bounds(vm: &Vm, id: WidgetId, depth: usize) -> Option<(f64, f64, f64, f64)> {
    // (while a scene is built, its rectangles worked out once: the layout
    // does not change meanwhile, T538)
    if let Ok(mut held) = engine(vm).scene_bounds.try_borrow_mut()
        && let Some(known) = held.as_mut()
    {
        return bounds_in(vm, id, depth, known);
    }
    bounds_in(vm, id, depth, &mut KnownBounds::default())
}

/// While `f` builds a scene, the rectangles it asks for worked out once
/// (decision T538).
pub fn with_scene_bounds<T>(vm: &mut Vm, f: impl FnOnce(&mut Vm) -> T) -> T {
    *engine(vm).scene_bounds.borrow_mut() = Some(KnownBounds::default());
    let out = f(vm);
    *engine(vm).scene_bounds.borrow_mut() = None;
    out
}

/// A rectangle (left, bottom, width, height), as [`bounds`] gives it.
type Rect = (f64, f64, f64, f64);

/// The rectangles worked out in one [`bounds`], by widget and depth.
pub(crate) type KnownBounds = rustc_hash::FxHashMap<(usize, usize), Option<Rect>>;

/// [`bounds`], each widget's at a depth worked out once (two points on the
/// same frame ask for it twice, and so on up the chain).
fn bounds_in(vm: &Vm, id: WidgetId, depth: usize, known: &mut KnownBounds) -> Option<Rect> {
    if let Some(&b) = known.get(&(id.0, depth)) {
        return b;
    }
    let b = bounds_of(vm, id, depth, known);
    known.insert((id.0, depth), b);
    b
}

fn bounds_of(vm: &Vm, id: WidgetId, depth: usize, known: &mut KnownBounds) -> Option<Rect> {
    let e = engine(vm);
    let w = &e.world.widgets[id.0];
    // UIParent and WorldFrame cover the screen.
    if matches!(w.name.as_deref(), Some("UIParent" | "WorldFrame")) || depth > 64 {
        let (w, h) = e.screen.ui_parent_size();
        return Some((0.0, 0.0, w, h));
    }
    let s = layout_scale(e, id);
    // A scroll child without points of its own sits at its scroll frame's
    // top left (unverified: the scroll offsets are not applied).
    if w.points.is_empty() {
        let (width, height) = (w.width * s, w.height * s);
        // A slider's thumb without points: where the slider puts it, along
        // its length by its value (`bars::thumb_place`).
        if let Some((slider, at, vertical)) = crate::bars::thumb_place(vm, id) {
            let r = bounds_in(vm, slider, depth + 1, known)?;
            return Some(if vertical {
                // The minimum at the top.
                let top = r.1 + r.3 - at * (r.3 - height).max(0.0);
                (r.0 + (r.2 - width) / 2.0, top - height, width, height)
            } else {
                let left = r.0 + at * (r.2 - width).max(0.0);
                (left, r.1 + (r.3 - height) / 2.0, width, height)
            });
        }
        // A status bar's texture without points: the part of the bar its
        // value fills, which a frame can anchor to (the default UI's heal
        // prediction, at the health fill's right).
        if let Some((bar, f, vertical, reverse)) = crate::bars::fill_place(vm, id) {
            let (l, b, w, h) = bounds_in(vm, bar, depth + 1, known)?;
            return Some(match (vertical, reverse) {
                (false, false) => (l, b, w * f, h),
                (false, true) => (l + w * (1.0 - f), b, w * f, h),
                (true, false) => (l, b, w, h * f),
                (true, true) => (l, b + h * (1.0 - f), w, h * f),
            });
        }
        let p = w.scroll_parent?;
        let r = bounds_in(vm, p, depth + 1, known)?;
        return Some((r.0, r.1 + r.3 - height, width, height));
    }
    // Each point's place on the screen, and which edges it sets.
    let mut left = None;
    let mut right = None;
    let mut center_x = None;
    let mut bottom = None;
    let mut top = None;
    let mut center_y = None;
    // The client keeps a slot per point and resolves them in
    // `Enum.FramePoint`'s order, whatever order they were set in: of two
    // points setting one edge, the earlier slot's wins (the default UI's
    // dropdowns: their text's `TOPRIGHT`, set first, and `TOPLEFT` both
    // set its top, and the client centers it by `TOPLEFT`'s).
    let mut points: Vec<&Point> = w.points.iter().collect();
    points.sort_by_key(|p| point_rank(&p.point));
    for p in points {
        let rel = match p.relative {
            Some(v) => widget_of(vm, v),
            None => w.parent,
        };
        // Anchored to a frame without a rectangle, it has none either
        // (SuuUI probe trees: the gamepad tab buttons of the character
        // window, sized but never placed, and their textures set to fill
        // them report no points' size, 0 by 0, where the engine gave them
        // the screen's).
        let r = match rel {
            Some(r) => bounds_in(vm, r, depth + 1, known)?,
            None => {
                let (w, h) = engine(vm).screen.ui_parent_size();
                (0.0, 0.0, w, h)
            }
        };
        let (rx, ry) = anchor(&p.relative_point);
        let ax = r.0 + rx * r.2 + p.x * s;
        let ay = r.1 + ry * r.3 + p.y * s;
        let slot = if p.point.contains("LEFT") {
            &mut left
        } else if p.point.contains("RIGHT") {
            &mut right
        } else {
            &mut center_x
        };
        slot.get_or_insert(ax);
        let slot = if p.point.contains("BOTTOM") {
            &mut bottom
        } else if p.point.contains("TOP") {
            &mut top
        } else {
            &mut center_y
        };
        slot.get_or_insert(ay);
    }
    let text = w.class == "FontString";
    let width = if text && w.width == 0.0 {
        text_extent(vm, id, None).0 * s
    } else {
        w.width * s
    };
    // A right edge left of the left one, a top below the bottom: no size,
    // at the left and bottom points (recorded: `GetWidth()` and
    // `GetHeight()` 0, `GetRect()` at the LEFT and BOTTOM points'
    // places; decision T266).
    let (x, width) = match (left, right, center_x) {
        (Some(l), Some(r), _) => (l, (r - l).max(0.0)),
        (Some(l), None, _) => (l, width),
        (None, Some(r), _) => (r - width, width),
        (None, None, Some(c)) => (c - width / 2.0, width),
        (None, None, None) => (0.0, width),
    };
    let height = if text && w.height == 0.0 {
        text_extent(vm, id, Some(width / s)).1 * s
    } else {
        w.height * s
    };
    let (y, height) = match (bottom, top, center_y) {
        (Some(b), Some(t), _) => (b, (t - b).max(0.0)),
        (Some(b), None, _) => (b, height),
        (None, Some(t), _) => (t - height, height),
        (None, None, Some(c)) => (c - height / 2.0, height),
        (None, None, None) => (0.0, height),
    };
    Some((x, y, width, height))
}

/// A point's slot in `Enum.FramePoint`'s order.
fn point_rank(point: &str) -> usize {
    const ORDER: [&str; 9] = [
        "TOPLEFT",
        "TOP",
        "TOPRIGHT",
        "LEFT",
        "CENTER",
        "RIGHT",
        "BOTTOMLEFT",
        "BOTTOM",
        "BOTTOMRIGHT",
    ];
    ORDER
        .iter()
        .position(|o| o.eq_ignore_ascii_case(point))
        .unwrap_or(ORDER.len())
}

fn edge(vm: &mut Vm, id: WidgetId, f: fn((f64, f64, f64, f64)) -> f64) -> Ret {
    let s = layout_scale(engine(vm), id);
    Ok(vec![
        bounds(vm, id, 0).map_or(Value::Nil, |b| Value::Num(f(b) / s)),
    ])
}

fn get_left(vm: &mut Vm, id: WidgetId, _c: NativeCall) -> Ret {
    edge(vm, id, |b| b.0)
}

fn get_right(vm: &mut Vm, id: WidgetId, _c: NativeCall) -> Ret {
    edge(vm, id, |b| b.0 + b.2)
}

fn get_bottom(vm: &mut Vm, id: WidgetId, _c: NativeCall) -> Ret {
    edge(vm, id, |b| b.1)
}

fn get_top(vm: &mut Vm, id: WidgetId, _c: NativeCall) -> Ret {
    edge(vm, id, |b| b.1 + b.3)
}

fn get_center(vm: &mut Vm, id: WidgetId, _c: NativeCall) -> Ret {
    let s = layout_scale(engine(vm), id);
    Ok(match bounds(vm, id, 0) {
        Some(b) => vec![
            Value::Num((b.0 + b.2 / 2.0) / s),
            Value::Num((b.1 + b.3 / 2.0) / s),
        ],
        None => vec![],
    })
}

// --------------------------------------------------------------- text

/// Whether a widget has a font: its own or a font object's.
pub(crate) fn has_font(vm: &mut Vm, id: WidgetId) -> bool {
    let props = engine(vm).world.widgets[id.0].props;
    stored_args(vm, props, "Font").is_some() || stored_args(vm, props, "FontObject").is_some()
}

/// A button's text font string gets its font from the button's normal
/// font object, when it has none of its own.
fn button_font_to_text(vm: &mut Vm, id: WidgetId) {
    let props = engine(vm).world.widgets[id.0].props;
    let font = stored_args(vm, props, "NormalFontObject").and_then(|v| v.first().copied());
    let text = stored_args(vm, props, "FontString").and_then(|v| v.first().copied());
    if let (Some(font), Some(text)) = (font, text)
        && let Some(fs) = widget_of(vm, text)
        && !has_font(vm, fs)
    {
        store(vm, fs, "FontObject", &[font]);
    }
}

/// `SetParentKey(key)`: the widget becomes its parent's `key` (and
/// `GetParentKey` says so).
fn set_parent_key(vm: &mut Vm, id: WidgetId, c: NativeCall) -> Ret {
    let key = c.arg(2);
    store(vm, id, "ParentKey", &[key]);
    let (table, parent) = {
        let w = &engine(vm).world.widgets[id.0];
        (w.table, w.parent)
    };
    if let Some(p) = parent
        && !key.is_nil()
    {
        let pt = engine(vm).world.widgets[p.0].table;
        let _ = vm.heap.table_mut(pt).set(key, Value::Table(table));
    }
    Ok(vec![])
}

/// A font object given by its name (`"GameFontNormal"`) is the global of
/// that name, as `SetFontObject` takes it; an object as it is.
fn font_object_value(vm: &mut Vm, v: Value) -> Value {
    if let Value::Str(s) = v {
        let name = String::from_utf8_lossy(vm.str_bytes(s)).into_owned();
        let g = vm.get_global(&name);
        if widget_of(vm, g).is_some() {
            return g;
        }
    }
    v
}

/// A button's `SetNormalFontObject`, `SetHighlightFontObject` or
/// `SetDisabledFontObject`: the font object, by name or itself (a name
/// was kept as the string, so the button's text had no font and was drawn
/// a pixel high: the Legacy challenges' categories, a button given
/// `"Game15Font_Shadow"`).
fn set_state_font_object(vm: &mut Vm, id: WidgetId, c: &NativeCall, stem: &str) {
    let mut args: Vec<Value> = c.args.iter().skip(1).copied().collect();
    if let Some(first) = args.first_mut() {
        *first = font_object_value(vm, *first);
    }
    store(vm, id, stem, &args);
}

fn set_normal_font_object(vm: &mut Vm, id: WidgetId, c: NativeCall) -> Ret {
    set_state_font_object(vm, id, &c, "NormalFontObject");
    button_font_to_text(vm, id);
    Ok(vec![])
}

fn set_highlight_font_object(vm: &mut Vm, id: WidgetId, c: NativeCall) -> Ret {
    set_state_font_object(vm, id, &c, "HighlightFontObject");
    Ok(vec![])
}

fn set_disabled_font_object(vm: &mut Vm, id: WidgetId, c: NativeCall) -> Ret {
    set_state_font_object(vm, id, &c, "DisabledFontObject");
    Ok(vec![])
}

fn set_font_string(vm: &mut Vm, id: WidgetId, c: NativeCall) -> Ret {
    let args: Vec<Value> = c.args.iter().skip(1).copied().collect();
    store(vm, id, "FontString", &args);
    button_font_to_text(vm, id);
    Ok(vec![])
}

/// `GetTextScale()`: what `SetTextScale` stored, else 1.
fn get_text_scale(vm: &mut Vm, id: WidgetId, _c: NativeCall) -> Ret {
    let props = engine(vm).world.widgets[id.0].props;
    let scale = stored_args(vm, props, "TextScale").and_then(|v| v.first().copied());
    Ok(vec![scale.unwrap_or(Value::Num(1.0))])
}

/// A status bar's bar texture: the one set, else its own (the client's
/// is always there: Blizzard's unit frames desaturate it unasked).
fn status_bar_texture(vm: &mut Vm, id: WidgetId) -> Value {
    let props = engine(vm).world.widgets[id.0].props;
    if let Some(t) =
        stored_args(vm, props, "StatusBarTextureObject").and_then(|v| v.first().copied())
    {
        return t;
    }
    let t = create(vm, "Texture", None, Some(id));
    if let Some(tid) = widget_of(vm, t) {
        let layer = vm.str("ARTWORK");
        store(vm, tid, "DrawLayer", &[layer, Value::Num(0.0)]);
    }
    store(vm, id, "StatusBarTextureObject", &[t]);
    t
}

fn get_status_bar_texture(vm: &mut Vm, id: WidgetId, _c: NativeCall) -> Ret {
    Ok(vec![status_bar_texture(vm, id)])
}

/// `SetStatusBarTexture(asset)`: a texture widget becomes the bar's, a
/// file its texture's file; an atlas's name its texture's atlas (Blizzard's
/// bars pass atlas names: the experience bar's
/// `UI-HUD-ExperienceBar-Fill-Experience`; unverified: what `GetTexture`
/// answers then).
fn set_status_bar_texture(vm: &mut Vm, id: WidgetId, c: NativeCall) -> Ret {
    let asset = c.arg(2);
    if widget_of(vm, asset).is_some() {
        store(vm, id, "StatusBarTextureObject", &[asset]);
    } else {
        let t = status_bar_texture(vm, id);
        if let Some(tid) = widget_of(vm, t) {
            let is_atlas = match asset {
                Value::Str(s) => {
                    let name = String::from_utf8_lossy(vm.str_bytes(s)).into_owned();
                    crate::measures::atlas(vm, &name).is_some()
                }
                _ => false,
            };
            if is_atlas {
                store(vm, tid, "Atlas", &[asset]);
                unstore(vm, tid, "Texture");
            } else {
                set_texture_file(vm, tid, &[asset]);
            }
        }
    }
    store(vm, id, "StatusBarTexture", &[asset]);
    Ok(vec![Value::Bool(true)])
}

fn set_text(vm: &mut Vm, id: WidgetId, c: NativeCall) -> Ret {
    set_text_value(vm, id, c.arg(2))
}

/// `SetText(text)`'s work, `SetFormattedText`'s too.
fn set_text_value(vm: &mut Vm, id: WidgetId, text: Value) -> Ret {
    // A font string needs a font first (`/stdtest engine`).
    if engine(vm).world.widgets[id.0].class == "FontString" && !has_font(vm, id) {
        return Err(c_error(vm, "FontString:SetText(): Font not set", true));
    }
    // A button's text goes to its font string, made if it has none
    // (unverified: the client makes one so).
    let class = engine(vm).world.widgets[id.0].class.clone();
    if is_a(engine(vm), &class, "Button") {
        let props = engine(vm).world.widgets[id.0].props;
        let fs = match stored_args(vm, props, "FontString").and_then(|v| v.first().copied()) {
            Some(fs) if widget_of(vm, fs).is_some() => fs,
            _ => {
                let fs = create(vm, "FontString", None, Some(id));
                store(vm, id, "FontString", &[fs]);
                button_font_to_text(vm, id);
                fs
            }
        };
        if let Some(fid) = widget_of(vm, fs) {
            let t = kept_text(vm, text);
            store_text(vm, fid, t);
        }
    }
    let t = kept_text(vm, text);
    store_text(vm, id, t);
    if is_a(engine(vm), &class, "EditBox") {
        crate::widgetops::text_set(vm, id);
        if !engine(vm).world.text_set.contains(&id) {
            engine_mut(vm).world.text_set.push(id);
        }
    }
    Ok(vec![])
}

/// `region:SetDrawLayer(layer [, sublevel])`: kept, the sublevel 0 when
/// not given (documented `Default = 0`; `GetDrawLayer` then answers both,
/// as Blizzard's menus' radio entries need: the compositor's textures are
/// set to `"ARTWORK"` alone, then raised a sublevel).
fn set_draw_layer(vm: &mut Vm, id: WidgetId, c: NativeCall) -> Ret {
    let sublevel = match c.arg(3) {
        Value::Num(n) => n,
        _ => 0.0,
    };
    store(vm, id, "DrawLayer", &[c.arg(2), Value::Num(sublevel)]);
    Ok(vec![])
}

/// `fontString:SetTextToFit([text])`: its text set as `SetText` sets it
/// (the documented `text`; Blizzard's menus' entries are labelled so),
/// then its width the text's (the name; unverified).
fn set_text_to_fit(vm: &mut Vm, id: WidgetId, c: NativeCall) -> Ret {
    set_text(vm, id, c)?;
    let text = match engine(vm).world.widgets[id.0].text {
        Some(Value::Str(s)) => String::from_utf8_lossy(vm.str_bytes(s)).into_owned(),
        _ => String::new(),
    };
    let width = crate::text::width(vm, id, &text);
    resize(vm, id, Some(width), None);
    Ok(vec![])
}

/// A widget's text set, kept alive while it is the widget's (its
/// properties hold it: the collector sees them, not the widget).
pub fn store_text(vm: &mut Vm, id: WidgetId, text: Option<Value>) {
    // (the same text again changes nothing: a binding writes its text
    // each frame, T542)
    let same = match (engine(vm).world.widgets[id.0].text, text) {
        (Some(a), Some(b)) => a.raw_eq(b),
        (None, None) => true,
        _ => false,
    };
    if same {
        return;
    }
    mark(engine_mut(vm), id);
    let props = engine(vm).world.widgets[id.0].props;
    let key = vm.str("\u{0}text");
    let _ = vm
        .heap
        .table_mut(props)
        .set(key, text.unwrap_or(Value::Nil));
    engine_mut(vm).world.widgets[id.0].text = text;
}

/// A text as a widget keeps it: a number as its string (`SetText(1)` reads
/// back "1"; unverified, queued: answers' "FontString SetText number").
fn kept_text(vm: &mut Vm, v: Value) -> Option<Value> {
    match v {
        Value::Nil => None,
        Value::Num(_) => vm.tostring(v).ok(),
        v => Some(v),
    }
}

fn get_text(vm: &mut Vm, id: WidgetId, _c: NativeCall) -> Ret {
    let w = &engine(vm).world.widgets[id.0];
    // An edit box's text starts empty, not nil (`/stdtest engine`).
    let edit_box = w.class == "EditBox";
    Ok(vec![match w.text {
        Some(t) => t,
        None if edit_box => vm.str(""),
        None => Value::Nil,
    }])
}

/// `editBox:SetNumber(n)`: its text the number's, as `SetText`'s
/// (unverified: not recorded; the auction house's quantity box).
fn set_number(vm: &mut Vm, id: WidgetId, c: NativeCall) -> Ret {
    if engine(vm).world.widgets[id.0].class != "EditBox" {
        return Ok(vec![]);
    }
    let n = match vm.hidden(c.arg(2)) {
        Value::Num(n) => n,
        _ => 0.0,
    };
    let text = if n.fract() == 0.0 {
        format!("{}", n as i64)
    } else {
        format!("{n}")
    };
    let t = vm.str(&text);
    store_text(vm, id, Some(t));
    if !engine(vm).world.text_set.contains(&id) {
        engine_mut(vm).world.text_set.push(id);
    }
    Ok(vec![])
}

/// `editBox:GetNumber()`: its text as a number, 0 when it is none
/// (unverified: not recorded).
fn get_number(vm: &mut Vm, id: WidgetId, _c: NativeCall) -> Ret {
    let text = match engine(vm).world.widgets[id.0].text {
        Some(Value::Str(s)) => String::from_utf8_lossy(vm.str_bytes(s)).into_owned(),
        _ => String::new(),
    };
    Ok(vec![Value::Num(text.trim().parse::<f64>().unwrap_or(0.0))])
}

/// `fontString:SetAlphaGradient(start, length)`: whether the gradient's
/// start is within the text, in characters (the quest text's fade ends
/// when it is not; a frame's returns nothing, as documented).
fn set_alpha_gradient(vm: &mut Vm, id: WidgetId, c: NativeCall) -> Ret {
    if engine(vm).world.widgets[id.0].class != "FontString" {
        return Ok(vec![]);
    }
    let start = match vm.hidden(c.arg(2)) {
        Value::Num(n) => n,
        _ => 0.0,
    };
    let chars = match engine(vm).world.widgets[id.0].text {
        Some(Value::Str(s)) => String::from_utf8_lossy(vm.str_bytes(s)).chars().count(),
        _ => 0,
    };
    Ok(vec![Value::Bool(start < chars as f64)])
}

fn set_formatted_text(vm: &mut Vm, id: WidgetId, c: NativeCall) -> Ret {
    let format = vm.get_global("string");
    let Value::Table(string) = format else {
        return Err(vm.error("no string library"));
    };
    let key = vm.str("format");
    let f = vm.heap.table(string).get(key);
    let args: Vec<Value> = c.args.iter().skip(1).copied().collect();
    let r = vm.call(f, &args)?;
    // As `SetText`: a button's goes to its font string (the macro window's
    // second tab, `SetFormattedText(CHARACTER_SPECIFIC_MACROS, name)`,
    // showed no text).
    set_text_value(vm, id, r.first().copied().unwrap_or(Value::Nil))
}

// --------------------------------------------------------- attributes

/// Attribute names are case-insensitive (unverified): kept lower-case.
fn set_attribute(vm: &mut Vm, id: WidgetId, c: NativeCall) -> Ret {
    let Value::Str(key) = c.arg(2) else {
        return Err(c_error(vm, "Arguments: (\"name\", value)", true));
    };
    let lower = String::from_utf8_lossy(vm.str_bytes(key)).to_lowercase();
    let name = vm.str(&lower);
    let attributes = engine(vm).world.widgets[id.0].attributes;
    let v = c.arg(3);
    let _ = vm.heap.table_mut(attributes).set(name, v);
    // Called as the client calls a script, also while `CreateFrame` runs a
    // new frame's `OnLoad` (decision T243's taint going on is the OnLoad's,
    // not this handler's): Forever 70707's smart navigation sets its secure
    // values from tainted template code through its own attribute handler,
    // which asserts it runs secure (decision T566; unverified).
    let creating = std::mem::replace(&mut engine_mut(vm).world.ui.creating, false);
    run_script(vm, id, "OnAttributeChanged", &[name, v]);
    engine_mut(vm).world.ui.creating = creating;
    Ok(vec![])
}

/// `SetAttributeNoHandler(name, value)`: the attribute, without
/// `OnAttributeChanged`.
fn set_attribute_no_handler(vm: &mut Vm, id: WidgetId, c: NativeCall) -> Ret {
    let Value::Str(key) = c.arg(2) else {
        return Err(c_error(vm, "Arguments: (\"name\", value)", true));
    };
    let lower = String::from_utf8_lossy(vm.str_bytes(key)).to_lowercase();
    let name = vm.str(&lower);
    let attributes = engine(vm).world.widgets[id.0].attributes;
    let _ = vm.heap.table_mut(attributes).set(name, c.arg(3));
    Ok(vec![])
}

fn attribute(vm: &mut Vm, id: WidgetId, name: &str) -> Value {
    let attributes = engine(vm).world.widgets[id.0].attributes;
    let key = vm.str(&name.to_lowercase());
    vm.heap.table(attributes).get(key)
}

/// `GetAttribute(name)`, or `GetAttribute(prefix, name, suffix)`: the first
/// set of `prefix..name..suffix`, `*..name..suffix`, `prefix..name..*`,
/// `*..name..*` and `name` (unverified order).
/// `frame:ExecuteAttribute(name, ...)`: the attribute's function called
/// with the arguments: true and its results; false without a function.
fn execute_attribute(vm: &mut Vm, id: WidgetId, c: NativeCall) -> Ret {
    let name = vm.check_str(&c, 2)?;
    let name = String::from_utf8_lossy(vm.str_bytes(name)).into_owned();
    let f = attribute(vm, id, &name);
    if !matches!(f, Value::Func(_)) {
        return Ok(vec![Value::Bool(false)]);
    }
    let args: Vec<Value> = c.args.iter().skip(2).copied().collect();
    let mut out = vec![Value::Bool(true)];
    out.extend(vm.call(f, &args)?);
    Ok(out)
}

fn get_attribute(vm: &mut Vm, id: WidgetId, c: NativeCall) -> Ret {
    let first = vm.check_str(&c, 2)?;
    let first = String::from_utf8_lossy(vm.str_bytes(first)).into_owned();
    if c.len() < 4 {
        return Ok(vec![attribute(vm, id, &first)]);
    }
    let name = vm.check_str(&c, 3)?;
    let name = String::from_utf8_lossy(vm.str_bytes(name)).into_owned();
    let suffix = vm.opt_str(&c, 4)?;
    let suffix = suffix
        .map(|s| String::from_utf8_lossy(vm.str_bytes(s)).into_owned())
        .unwrap_or_default();
    let prefix = first;
    for key in [
        format!("{prefix}{name}{suffix}"),
        format!("*{name}{suffix}"),
        format!("{prefix}{name}*"),
        format!("*{name}*"),
        name.clone(),
    ] {
        let v = attribute(vm, id, &key);
        if !v.is_nil() {
            return Ok(vec![v]);
        }
    }
    Ok(vec![Value::Nil])
}

fn is_protected(vm: &mut Vm, id: WidgetId, _c: NativeCall) -> Ret {
    let e = engine(vm);
    Ok(vec![
        Value::Bool(protected(e, id)),
        Value::Bool(e.world.widgets[id.0].explicitly_protected),
    ])
}

// --------------------------------------------------------------- levels

/// `GetFrameLevel()`: what `SetFrameLevel` set, else one above the parent
/// (unverified).
/// `SetGradient(orientation, minColor, maxColor)`: the texture's color
/// shaded from the minimum's (left, bottom) to the maximum's (right, top);
/// the colors are color objects (`CreateColor`) or tables of `r`, `g`, `b`,
/// `a`.
fn set_gradient(vm: &mut Vm, id: WidgetId, c: NativeCall) -> Ret {
    let mut args = vec![c.arg(2)];
    for v in [c.arg(3), c.arg(4)] {
        for k in ["r", "g", "b", "a"] {
            let n = match v {
                Value::Table(t) => {
                    let key = vm.str(k);
                    match vm.heap.table(t).get(key) {
                        Value::Num(n) => n,
                        _ => 1.0,
                    }
                }
                _ => 1.0,
            };
            args.push(Value::Num(n));
        }
    }
    store(vm, id, "Gradient", &args);
    Ok(vec![])
}

fn get_frame_level(vm: &mut Vm, id: WidgetId, _c: NativeCall) -> Ret {
    Ok(vec![Value::Num(frame_level(vm, id))])
}

/// `SetFrameLevel(level)`: the frame's level; its descendants that have a
/// level of their own move with it, by as much, keeping their place above
/// it (the others follow, a level over their parent). Blizzard's menus set
/// their entries 20 over the menu, then raise the menu to 502: the
/// entries stayed at 20, under its background, drawn faint.
fn set_frame_level(vm: &mut Vm, id: WidgetId, c: NativeCall) -> Ret {
    let Value::Num(level) = c.arg(2) else {
        return Err(vm.api_arg_error(2, "Usage: self:SetFrameLevel(frameLevel)"));
    };
    set_level(vm, id, level);
    Ok(vec![])
}

/// Sets a frame's level, its descendants with a level of their own moved
/// with it (`SetFrameLevel`, `Raise`, `Lower`).
pub(crate) fn set_level(vm: &mut Vm, id: WidgetId, level: f64) {
    let old = frame_level(vm, id);
    store(vm, id, "FrameLevel", &[Value::Num(level)]);
    let delta = level - old;
    if delta != 0.0 {
        let mut stack = engine(vm).world.widgets[id.0].children.clone();
        while let Some(child) = stack.pop() {
            let props = engine(vm).world.widgets[child.0].props;
            if let Some(Value::Num(n)) =
                stored_args(vm, props, "FrameLevel").and_then(|v| v.first().copied())
            {
                store(vm, child, "FrameLevel", &[Value::Num((n + delta).max(0.0))]);
            }
            stack.extend(engine(vm).world.widgets[child.0].children.iter().copied());
        }
    }
}

/// `SetUsingParentLevel(using)`: kept; using its parent's, a frame drops
/// the level set before (SuuUI probe trees: `InsetFrameTemplate`'s
/// `NineSlice`, `useParentLevel` over `NineSlicePanelTemplate`'s
/// `frameLevel="500"`, at its inset's level 2). Unverified: a level set
/// after.
fn set_using_parent_level(vm: &mut Vm, id: WidgetId, c: NativeCall) -> Ret {
    reorder(engine_mut(vm));
    mark(engine_mut(vm), id);
    let using = c.arg(2).truthy();
    store(vm, id, "UsingParentLevel", &[Value::Bool(using)]);
    if using {
        let props = engine(vm).world.widgets[id.0].props;
        let key = vm.str("FrameLevel");
        let _ = vm.heap.table_mut(props).set(key, Value::Nil);
    }
    Ok(vec![])
}

/// A frame's level: what `SetFrameLevel` set, else one above the parent
/// (unverified).
pub fn frame_level(vm: &mut Vm, id: WidgetId) -> f64 {
    let mut level = 0.0;
    let mut cur = Some(id);
    let mut depth = 0.0;
    while let Some(w) = cur {
        let props = engine(vm).world.widgets[w.0].props;
        if let Some(v) = stored_args(vm, props, "FrameLevel")
            && let Some(Value::Num(n)) = v.first()
        {
            level = *n;
            break;
        }
        // One using its parent's level is at it (SuuUI probe trees: the
        // character frame's panes, `useParentLevel`, at its level 1).
        let using = stored_args(vm, props, "UsingParentLevel")
            .and_then(|v| v.first().copied())
            .is_some_and(|v| v.truthy());
        cur = engine(vm).world.widgets[w.0].parent;
        if cur.is_some() && !using {
            depth += 1.0;
        }
    }
    level + depth
}

/// The stratas, lowest first.
const STRATAS: [&str; 9] = [
    "BACKGROUND",
    "LOW",
    "MEDIUM",
    "HIGH",
    "DIALOG",
    "FULLSCREEN",
    "FULLSCREEN_DIALOG",
    "TOOLTIP",
    "BLIZZARD",
];

/// A frame's strata by name (`GetFrameStrata`, as `strata_rank` finds it).
pub fn strata_name(vm: &mut Vm, id: WidgetId) -> &'static str {
    STRATAS[strata_rank(vm, id).min(STRATAS.len() - 1)]
}

/// A frame's strata as a rank (`STRATAS`): its own, else its parent's
/// (`PARENT`, or none set), `MEDIUM` at the top.
/// A frame's own strata set (`SetFrameStrata`, its XML's), as a rank.
pub fn own_strata(vm: &mut Vm, id: WidgetId) -> Option<usize> {
    let props = engine(vm).world.widgets[id.0].props;
    // (read without copies: each shown frame asks each frame, T542)
    if let Some(Value::Str(s)) = stored_arg(vm, props, "FrameStrata", 0) {
        let s = vm.str_bytes(s);
        return STRATAS
            .iter()
            .position(|x| x.as_bytes().eq_ignore_ascii_case(s));
    }
    None
}

pub fn strata_rank(vm: &mut Vm, id: WidgetId) -> usize {
    let mut cur = Some(id);
    while let Some(w) = cur {
        let props = engine(vm).world.widgets[w.0].props;
        if let Some(Value::Str(s)) =
            stored_args(vm, props, "FrameStrata").and_then(|v| v.first().copied())
        {
            let s = String::from_utf8_lossy(vm.str_bytes(s)).to_uppercase();
            if let Some(rank) = STRATAS.iter().position(|x| *x == s) {
                return rank;
            }
        }
        cur = engine(vm).world.widgets[w.0].parent;
    }
    2
}

fn get_id(vm: &mut Vm, id: WidgetId, _c: NativeCall) -> Ret {
    let props = engine(vm).world.widgets[id.0].props;
    let v = stored_args(vm, props, "ID")
        .and_then(|v| v.first().copied())
        .unwrap_or(Value::Num(0.0));
    Ok(vec![v])
}

/// A button's `GetTextWidth()`: its font string's
/// text, in that font string's font (the button's own has none: Blizzard's
/// money frames sized their coin buttons by it, and with 0 drew the amount
/// over the item's icon).
fn button_text_widget(vm: &mut Vm, id: WidgetId) -> WidgetId {
    let props = engine(vm).world.widgets[id.0].props;
    stored_args(vm, props, "FontString")
        .and_then(|v| v.first().copied())
        .and_then(|fs| widget_of(vm, fs))
        .unwrap_or(id)
}

fn get_text_width(vm: &mut Vm, id: WidgetId, c: NativeCall) -> Ret {
    let fs = button_text_widget(vm, id);
    get_string_width(vm, fs, c)
}

/// A font string's text as shown (a secret's own text), and whether secret.
fn shown_text(vm: &Vm, id: WidgetId) -> (String, bool) {
    let text = engine(vm).world.widgets[id.0].text;
    let secret = matches!(text, Some(Value::Secret(_)));
    let shown = match text.map(|t| vm.hidden(t)) {
        Some(Value::Str(s)) => String::from_utf8_lossy(vm.heap.str_bytes(s)).into_owned(),
        _ => String::new(),
    };
    (shown, secret)
}

/// `GetStringWidth()`: its widest line as the client lays text out
/// (`text.rs`, decision T237), or, held to fewer lines than it has, the
/// text it shows with the ellipsis (decision T282); a secret text's width
/// is secret too, as in the client (SuuUI's tests).
fn get_string_width(vm: &mut Vm, id: WidgetId, _c: NativeCall) -> Ret {
    let (shown, secret) = shown_text(vm, id);
    let w = if shown.is_empty() {
        0.0
    } else {
        let max = laid_out_size(vm, id).0;
        crate::text::layout_in(vm, id, &shown, (max > 0.0).then_some(max)).width
    };
    let width = Value::Num(w);
    Ok(vec![if secret { vm.secret_wrap(width) } else { width }])
}

/// `GetUnboundedStringWidth()`: its widest line, whatever its size.
fn get_unbounded_string_width(vm: &mut Vm, id: WidgetId, _c: NativeCall) -> Ret {
    let (shown, secret) = shown_text(vm, id);
    let width = Value::Num(crate::text::width(vm, id, &shown));
    Ok(vec![if secret { vm.secret_wrap(width) } else { width }])
}

// --------------------------------------------------------------- fonts

/// `SetFontObject(font)`: a font object or its name; the widget's font is
/// then the object's until `SetFont` sets its own.
/// `CopyFontObject(font)`: the font's attributes (file, height, flags,
/// colours, shadow, spacing, justification), its inherited ones too,
/// become this one's own; its font object stays. Blizzard's
/// `FontableFrameMixin` copies a frame's font so (the chat frames'), then
/// sets its height with `SetFont(GetFont())`. Unverified: what is copied
/// beyond those.
fn copy_font_object(vm: &mut Vm, id: WidgetId, c: NativeCall) -> Ret {
    let source = match c.arg(2) {
        Value::Str(s) => {
            let name = String::from_utf8_lossy(vm.str_bytes(s)).into_owned();
            vm.get_global(&name)
        }
        v => v,
    };
    let Some(mut cur) = widget_of(vm, source) else {
        return Err(vm.api_arg_error(2, "Usage: self:CopyFontObject(font)"));
    };
    let target = engine(vm).world.widgets[id.0].props;
    let skip = vm.str("FontObject");
    // The nearest level's value of each attribute.
    let mut copied: Vec<Value> = Vec::new();
    for _ in 0..32 {
        let props = engine(vm).world.widgets[cur.0].props;
        let mut key = Value::Nil;
        let mut entries = Vec::new();
        while let Ok(Some((k, v))) = vm.heap.table(props).next(key) {
            entries.push((k, v));
            key = k;
        }
        for (k, v) in entries {
            let internal = matches!(k, Value::Str(s) if vm.str_bytes(s).first() == Some(&0));
            if internal || k.raw_eq(skip) || copied.iter().any(|c| c.raw_eq(k)) {
                continue;
            }
            copied.push(k);
            let _ = vm.heap.table_mut(target).set(k, v);
        }
        match stored_args(vm, props, "FontObject")
            .and_then(|v| v.first().copied())
            .and_then(|f| widget_of(vm, f))
        {
            Some(n) if n != cur => cur = n,
            _ => break,
        }
    }
    Ok(vec![])
}

fn set_font_object(vm: &mut Vm, id: WidgetId, c: NativeCall) -> Ret {
    mark(engine_mut(vm), id);
    let class = engine(vm).world.widgets[id.0].class.clone();
    let props = engine(vm).world.widgets[id.0].props;
    let font = match c.arg(2) {
        // Nil clears it (`/stdtest engine`).
        Value::Nil => {
            let key = vm.str("FontObject");
            let _ = vm.heap.table_mut(props).set(key, Value::Nil);
            return Ok(vec![]);
        }
        Value::Str(s) => {
            let name = String::from_utf8_lossy(vm.str_bytes(s)).into_owned();
            let g = vm.get_global(&name);
            if widget_of(vm, g).is_none() {
                return Err(c_error(
                    vm,
                    &format!("{class}:SetFontObject(): Couldn't find font named {name}"),
                    false,
                ));
            }
            g
        }
        v => v,
    };
    if widget_of(vm, font).is_none() {
        return Err(vm.api_arg_error(2, "Usage: self:SetFontObject(font)"));
    }
    let list = vm.new_table();
    let _ = vm.heap.table_mut(list).set(Value::Num(1.0), font);
    let n = vm.str("n");
    let _ = vm.heap.table_mut(list).set(n, Value::Num(1.0));
    let key = vm.str("FontObject");
    let _ = vm.heap.table_mut(props).set(key, Value::Table(list));
    // Its own font gives way to the object's.
    let own = vm.str("Font");
    let _ = vm.heap.table_mut(props).set(own, Value::Nil);
    Ok(vec![])
}

/// `GetFont()`: the widget's own font, else its font object's; its size a
/// single float (recorded: NumberFontNormal's 13.999999046325684).
pub(crate) fn get_font(vm: &mut Vm, id: WidgetId, _c: NativeCall) -> Ret {
    let mut values = font_of_widget(vm, id);
    if let Some(Value::Num(n)) = values.get_mut(1) {
        *n = f64::from(*n as f32);
    }
    Ok(values)
}

fn font_of_widget(vm: &mut Vm, id: WidgetId) -> Vec<Value> {
    let mut cur = id;
    for _ in 0..32 {
        let props = engine(vm).world.widgets[cur.0].props;
        if let Some(v) = stored_args(vm, props, "Font") {
            return v;
        }
        let next = stored_args(vm, props, "FontObject")
            .and_then(|v| v.first().copied())
            .and_then(|f| widget_of(vm, f));
        match next {
            Some(n) => cur = n,
            None => break,
        }
    }
    // No font: no file, a height, no flags (`/stdtest engine`; a font
    // string's height is garbage there).
    let flags = vm.str("");
    vec![Value::Nil, Value::Num(0.0), flags]
}

/// `GetLineHeight()`: the font's height (its own or a font object's).
fn get_line_height(vm: &mut Vm, id: WidgetId, c: NativeCall) -> Ret {
    let font = get_font(vm, id, c)?;
    Ok(vec![font.get(1).copied().unwrap_or(Value::Num(0.0))])
}

/// Whether Blizzard's secure code made the widget.
fn is_forbidden(vm: &mut Vm, id: WidgetId, _c: NativeCall) -> Ret {
    Ok(vec![Value::Bool(engine(vm).world.widgets[id.0].forbidden)])
}

fn click(vm: &mut Vm, id: WidgetId, c: NativeCall) -> Ret {
    let button = match c.arg(2) {
        Value::Str(s) => Value::Str(s),
        _ => vm.str("LeftButton"),
    };
    run_script(vm, id, "PreClick", &[button, Value::Bool(false)]);
    run_script(vm, id, "OnClick", &[button, Value::Bool(false)]);
    run_script(vm, id, "PostClick", &[button, Value::Bool(false)]);
    Ok(vec![])
}

/// `RegisterForClicks(...)`: the clicks that run the button's `OnClick`
/// (`"LeftButtonUp"`, `"AnyDown"`, …; none: no click does), kept for
/// the user's clicks (`mock.click`).
fn register_for_clicks(vm: &mut Vm, id: WidgetId, c: NativeCall) -> Ret {
    let args: Vec<Value> = c.args.iter().skip(1).copied().collect();
    store(vm, id, "RegisterForClicks", &args);
    Ok(vec![])
}

/// Whether a user's click of `button` (`"LeftButton"`, …), going down or
/// up, runs the button's `OnClick`: as `RegisterForClicks` registered it,
/// by default the left button's up click.
pub fn registered_click(vm: &mut Vm, id: WidgetId, button: &str, down: bool) -> bool {
    let props = engine(vm).world.widgets[id.0].props;
    let way = if down { "Down" } else { "Up" };
    let Some(list) = stored_args(vm, props, "RegisterForClicks") else {
        return button == "LeftButton" && !down;
    };
    list.iter().any(|&v| match v {
        Value::Str(s) => {
            let s = String::from_utf8_lossy(vm.str_bytes(s));
            s.eq_ignore_ascii_case(&format!("Any{way}"))
                || s.eq_ignore_ascii_case(&format!("{button}{way}"))
        }
        _ => false,
    })
}

// ---------------------------------------------------- moving and state

fn start_moving(vm: &mut Vm, id: WidgetId, _c: NativeCall) -> Ret {
    let props = engine(vm).world.widgets[id.0].props;
    let movable =
        stored_args(vm, props, "Movable").is_some_and(|v| v.first().is_some_and(|m| m.truthy()));
    if !movable {
        let class = engine(vm).world.widgets[id.0].class.clone();
        return Err(c_error(
            vm,
            &format!("{class}:StartMoving(): Frame is not movable"),
            true,
        ));
    }
    engine_mut(vm).world.widgets[id.0].moving = true;
    Ok(vec![])
}

fn start_sizing(vm: &mut Vm, id: WidgetId, _c: NativeCall) -> Ret {
    engine_mut(vm).world.widgets[id.0].sizing = true;
    Ok(vec![])
}

fn stop_moving_or_sizing(vm: &mut Vm, id: WidgetId, _c: NativeCall) -> Ret {
    let w = &mut engine_mut(vm).world.widgets[id.0];
    w.moving = false;
    w.sizing = false;
    Ok(vec![])
}

fn set_resize_bounds(vm: &mut Vm, id: WidgetId, c: NativeCall) -> Ret {
    let min_w = number_arg(vm, &c, 2)?;
    let min_h = number_arg(vm, &c, 3)?;
    // Without a maximum, no limit (unverified).
    let max_w = vm.opt_number(&c, 4, 0.0)?;
    let max_h = vm.opt_number(&c, 5, 0.0)?;
    engine_mut(vm).world.widgets[id.0].resize_bounds = Some((min_w, min_h, max_w, max_h));
    Ok(vec![])
}

fn get_resize_bounds(vm: &mut Vm, id: WidgetId, _c: NativeCall) -> Ret {
    let (a, b, c, d) = engine(vm).world.widgets[id.0]
        .resize_bounds
        .unwrap_or_default();
    Ok(vec![
        Value::Num(a),
        Value::Num(b),
        Value::Num(c),
        Value::Num(d),
    ])
}

/// Enables or disables a button, edit box or slider; `OnEnable` /
/// `OnDisable` run when it changes (unverified).
fn set_enabled_to(vm: &mut Vm, id: WidgetId, on: bool) {
    let w = &mut engine_mut(vm).world.widgets[id.0];
    let was = std::mem::replace(&mut w.enabled, on);
    let class = w.class.clone();
    let script = if on { "OnEnable" } else { "OnDisable" };
    if was != on {
        crate::buttons::refresh_states(vm, id);
    }
    if was != on && has_script(engine(vm), &class, script) {
        run_script(vm, id, script, &[]);
    }
}

fn set_enabled(vm: &mut Vm, id: WidgetId, c: NativeCall) -> Ret {
    set_enabled_to(vm, id, c.arg(2).truthy());
    Ok(vec![])
}

fn enable(vm: &mut Vm, id: WidgetId, _c: NativeCall) -> Ret {
    set_enabled_to(vm, id, true);
    Ok(vec![])
}

fn disable(vm: &mut Vm, id: WidgetId, _c: NativeCall) -> Ret {
    set_enabled_to(vm, id, false);
    Ok(vec![])
}

fn is_enabled(vm: &mut Vm, id: WidgetId, _c: NativeCall) -> Ret {
    Ok(vec![Value::Bool(engine(vm).world.widgets[id.0].enabled)])
}

/// `SetVertexColor(r, g, b [, a])`: alpha 1 when left out; white until set
/// (unverified). Secrets too, even from tainted code, as `SetAlpha` (the
/// API documentation: `AllowedWhenTainted`, adding the vertex color
/// aspect); the color then reads back secret (unverified: each channel,
/// and the alpha aspect the documentation also names).
fn set_vertex_color(vm: &mut Vm, id: WidgetId, c: NativeCall) -> Ret {
    mark(engine_mut(vm), id);
    let secret = (2..=5).any(|i| matches!(c.arg(i), Value::Secret(_)));
    let mut rgba = [1.0; 4];
    for (i, v) in rgba.iter_mut().enumerate().take(3) {
        *v = match vm.hidden(c.arg(i + 2)) {
            Value::Num(n) if secret => n,
            _ => number_arg(vm, &c, i + 2)?,
        };
    }
    rgba[3] = match vm.hidden(c.arg(5)) {
        Value::Num(n) if secret => n,
        _ => vm.opt_number(&c, 5, 1.0)?,
    };
    let rgba = rgba.map(|v| byte_float(vm, v));
    let props = engine(vm).world.widgets[id.0].props;
    let list = vm.new_table();
    for (i, v) in rgba.iter().enumerate() {
        let v = if secret {
            vm.secret_wrap(Value::Num(*v))
        } else {
            Value::Num(*v)
        };
        let _ = vm.heap.table_mut(list).set(Value::Num(i as f64 + 1.0), v);
    }
    let n = vm.str("n");
    let _ = vm.heap.table_mut(list).set(n, Value::Num(4.0));
    let key = vm.str("VertexColor");
    let _ = vm.heap.table_mut(props).set(key, Value::Table(list));
    Ok(vec![])
}

fn get_vertex_color(vm: &mut Vm, id: WidgetId, _c: NativeCall) -> Ret {
    let props = engine(vm).world.widgets[id.0].props;
    Ok(stored_args(vm, props, "VertexColor").unwrap_or_else(|| vec![Value::Num(1.0); 4]))
}

/// Whether a kind of widget takes the mouse until told otherwise
/// (recorded: `camelot-widget-defaults.txt`'s `IsMouseEnabled` true:
/// buttons, edit boxes, sliders, color selects, browsers, checkouts, event
/// frames, contained alert frames).
pub fn mouse_by_default(e: &Engine, class: &str) -> bool {
    [
        "Button",
        "EditBox",
        "Slider",
        "ColorSelect",
        "Browser",
        "Checkout",
        "EventFrame",
        "ContainedAlertFrame",
    ]
    .iter()
    .any(|kind| is_a(e, class, kind))
}

/// A widget's kind is known.
pub const KIND_KNOWN: u8 = 1;
/// A region (a texture, a font string, a line, a mask).
pub const KIND_REGION: u8 = 2;
/// Of a frame's class.
pub const KIND_FRAME: u8 = 4;
/// A font string (decision T541).
pub const KIND_FONT_STRING: u8 = 8;
/// A mask texture (decision T541).
pub const KIND_MASK: u8 = 16;

/// A widget's kind (`KIND_*`): its class's, which never changes, found the
/// first time it is asked (the scene asks of every widget each frame;
/// decision T538).
pub fn kind(e: &Engine, id: WidgetId) -> u8 {
    let w = &e.world.widgets[id.0];
    let k = w.kind.get();
    if k != 0 {
        return k;
    }
    let region = ["Texture", "FontString", "Line", "MaskTexture"]
        .iter()
        .any(|base| is_a(e, &w.class, base));
    let frame = is_a(e, &w.class, "Frame");
    let font = is_a(e, &w.class, "FontString");
    let mask = is_a(e, &w.class, "MaskTexture");
    let k = KIND_KNOWN
        | if region { KIND_REGION } else { 0 }
        | if frame { KIND_FRAME } else { 0 }
        | if font { KIND_FONT_STRING } else { 0 }
        | if mask { KIND_MASK } else { 0 };
    w.kind.set(k);
    k
}

/// A widget and the widgets under it changed since the scene was last
/// built (decision T542): the scene lays their frames' items out again. A
/// font object's change reaches every text: the scene keeps nothing.
pub fn mark(e: &mut Engine, id: WidgetId) {
    let world = &mut e.world;
    let count = world.widgets.len();
    if world.scene_marks.len() < count {
        world.scene_marks.resize(count, true);
    }
    if world.widgets.get(id.0).is_some_and(|w| w.class == "Font") {
        world.scene_epoch += 1;
    }
    // (the shown widgets under it, and those anchored to or masked by
    // these: a hidden one draws nothing, nor do its, and showing it marks
    // them, but what is anchored to it moves with it; a marked widget's
    // are marked: the walk stops there; T545)
    let mut stack: Vec<(usize, Option<bool>)> = vec![(id.0, None)];
    let mut hidden_seen: rustc_hash::FxHashSet<usize> = Default::default();
    while let Some((i, shown)) = stack.pop() {
        let shown = shown.unwrap_or_else(|| visible_in(world, i));
        if !shown {
            if hidden_seen.insert(i)
                && let Some(by) = world.anchored_by.get(i)
            {
                stack.extend(by.iter().map(|&j| (j, None)));
            }
            continue;
        }
        match world.scene_marks.get_mut(i) {
            Some(m) if !*m => *m = true,
            _ => continue,
        }
        stack.extend(
            world.widgets[i]
                .children
                .iter()
                .map(|c| (c.0, Some(world.widgets[c.0].shown))),
        );
        if let Some(by) = world.anchored_by.get(i) {
            stack.extend(by.iter().map(|&j| (j, None)));
        }
    }
}

/// The widget alone changed (T545): it hides, so what is under it is no
/// longer drawn.
fn mark_alone(e: &mut Engine, id: WidgetId) {
    let world = &mut e.world;
    if world.scene_marks.len() < world.widgets.len() {
        world.scene_marks.resize(world.widgets.len(), true);
    }
    if let Some(m) = world.scene_marks.get_mut(id.0) {
        *m = true;
    }
}

/// [`visible`] of the world.
fn visible_in(world: &World, i: usize) -> bool {
    let mut cur = Some(i);
    while let Some(c) = cur {
        let w = &world.widgets[c];
        if !w.shown {
            return false;
        }
        cur = w.parent.map(|p| p.0);
    }
    true
}

/// `id` moves or changes with `target` (anchored to it, masked by it):
/// marking `target` marks it (decision T542).
pub(crate) fn depends_on(e: &mut Engine, id: WidgetId, target: WidgetId) {
    let by = &mut e.world.anchored_by;
    if by.len() <= target.0 {
        by.resize(target.0 + 1, Vec::new());
    }
    if !by[target.0].contains(&id.0) {
        by[target.0].push(id.0);
    }
}

/// A widget's anchors as they are now, for [`mark`] (T542).
fn note_anchors(vm: &mut Vm, id: WidgetId) {
    let relatives: Vec<Value> = engine(vm).world.widgets[id.0]
        .points
        .iter()
        .filter_map(|p| p.relative)
        .collect();
    for r in relatives {
        let target = match r {
            Value::Str(s) => {
                let name = String::from_utf8_lossy(vm.str_bytes(s)).into_owned();
                let g = vm.get_global(&name);
                widget_of(vm, g)
            }
            v => widget_of(vm, v),
        };
        if let Some(t) = target {
            depends_on(engine_mut(vm), id, t);
        }
    }
}

/// The frames' order changed: the scene orders them again (T543).
pub fn reorder(e: &mut Engine) {
    e.world.scene_order_valid = false;
}

/// The setters whose values order the frames (T543).
const ORDER_STEMS: [&str; 3] = ["FrameStrata", "FrameLevel", "UsingParentLevel"];

/// Whether a widget changed since the scene was last built (T542).
pub fn scene_changed(e: &Engine, i: usize) -> bool {
    e.world.scene_marks.get(i).copied().unwrap_or(true)
}

/// The scene was built: nothing changed since.
pub fn scene_built(e: &mut Engine) {
    let count = e.world.widgets.len();
    e.world.scene_marks.clear();
    e.world.scene_marks.resize(count, false);
}

/// A method that only reads its widget (`Get…`, `Is…`, `Has…`, `Can…`):
/// it leaves the scene as it is (T542).
fn reads_only(short: &str) -> bool {
    ["Get", "Is", "Has", "Can"]
        .iter()
        .any(|p| short.starts_with(p))
}

/// The CVars set, in the order they were (a session lists them so), found
/// by their name in any case at once (decision T538).
#[derive(Clone, Debug, Default)]
pub struct Cvars {
    list: Vec<(String, String)>,
    index: HashMap<String, usize>,
}

impl Cvars {
    /// A CVar's value, its name in any case.
    pub fn get(&self, name: &str) -> Option<&str> {
        self.index
            .get(&name.to_ascii_lowercase())
            .map(|&i| self.list[i].1.as_str())
    }

    pub fn has(&self, name: &str) -> bool {
        self.index.contains_key(&name.to_ascii_lowercase())
    }

    /// A CVar set (the name as first given kept).
    pub fn set(&mut self, name: &str, value: String) {
        let key = name.to_ascii_lowercase();
        match self.index.get(&key) {
            Some(&i) => self.list[i].1 = value,
            None => {
                self.index.insert(key, self.list.len());
                self.list.push((name.to_string(), value));
            }
        }
    }

    pub fn extend(&mut self, all: impl IntoIterator<Item = (String, String)>) {
        for (name, value) in all {
            self.set(&name, value);
        }
    }

    pub fn iter(&self) -> std::slice::Iter<'_, (String, String)> {
        self.list.iter()
    }
}

impl<'a> IntoIterator for &'a Cvars {
    type Item = &'a (String, String);
    type IntoIter = std::slice::Iter<'a, (String, String)>;
    fn into_iter(self) -> Self::IntoIter {
        self.list.iter()
    }
}

/// A frame's drag under way (the mouse held on it, moving): the frame,
/// what moves with the mouse (it, or for an overlay its parent, when it
/// started moving), where that stood and where the mouse began.
pub struct Dragging {
    pub frame: WidgetId,
    moved: Option<WidgetId>,
    start: Option<(f64, f64, f64, f64)>,
    from: (f64, f64),
}

/// The mouse pressed on a frame starts dragging it: `OnDragStart` with
/// the button.
pub fn drag_begin(vm: &mut Vm, id: WidgetId, button: &str) -> Dragging {
    crate::input::cursor_onto(vm, id);
    let from = engine(vm).world.input.cursor;
    let b = vm.str(button);
    run_script(vm, id, "OnDragStart", &[b]);
    let e = engine(vm);
    let moved = if e.world.widgets[id.0].moving {
        Some(id)
    } else {
        e.world.widgets[id.0]
            .parent
            .filter(|p| e.world.widgets[p.0].moving)
    };
    let start = moved.and_then(|m| bounds(vm, m, 0));
    Dragging {
        frame: id,
        moved,
        start,
        from,
    }
}

/// The drag's mouse now (dx, dy) screen units from where it began: what
/// moves follows it.
pub fn drag_move(vm: &mut Vm, d: &Dragging, dx: f64, dy: f64) {
    engine_mut(vm).world.input.cursor = (d.from.0 + dx, d.from.1 + dy);
    if let (Some(m), Some(b)) = (d.moved, d.start) {
        let s = layout_scale(engine(vm), m);
        // (the drag in screen units, rectangles in UIParent's)
        let ui = ui_parent_scale(engine(vm));
        let screen = vm.get_global("UIParent");
        mark(engine_mut(vm), m);
        let w = &mut engine_mut(vm).world.widgets[m.0];
        w.points = vec![Point {
            point: "BOTTOMLEFT".into(),
            relative: Some(screen),
            relative_point: "BOTTOMLEFT".into(),
            x: (b.0 + dx / ui) / s,
            y: (b.1 + dy / ui) / s,
        }];
        layout_changed(vm);
    }
}

/// The mouse let go: `OnDragStop`.
pub fn drag_end(vm: &mut Vm, d: &Dragging) {
    run_script(vm, d.frame, "OnDragStop", &[]);
}

/// The engine's controls for the mouse: a drag of a frame by (dx, dy)
/// screen units, if it (or, for an overlay, its parent) starts moving.
pub fn drag(vm: &mut Vm, id: WidgetId, dx: f64, dy: f64, frames: usize) {
    // The mouse pressed on the frame, then moved by (dx, dy) in `frames`
    // steps, a frame after each (none: at once).
    let d = drag_begin(vm, id, "LeftButton");
    let steps = frames.max(1);
    for k in 1..=steps {
        let part = k as f64 / steps as f64;
        drag_move(vm, &d, dx * part, dy * part);
        if frames > 0 {
            crate::clock::advance_frames(vm, 1, 1000.0 / 60.0);
        }
    }
    drag_end(vm, &d);
}

/// A drag of a resize grip by (dw, dh) screen units: its parent, if it
/// starts sizing, keeps its top left.
pub fn size_by(vm: &mut Vm, grip: WidgetId, dw: f64, dh: f64) {
    let left = vm.str("LeftButton");
    run_script(vm, grip, "OnMouseDown", &[left]);
    if let Some(f) = engine(vm).world.widgets[grip.0].parent
        && engine(vm).world.widgets[f.0].sizing
        && let Some(b) = bounds(vm, f, 0)
    {
        let s = layout_scale(engine(vm), f);
        // (the drag in screen units, the size in its own)
        let own = effective_scale(engine(vm), f);
        let screen = vm.get_global("UIParent");
        mark(engine_mut(vm), f);
        let w = &mut engine_mut(vm).world.widgets[f.0];
        let (min_w, min_h, max_w, max_h) = w.resize_bounds.unwrap_or_default();
        let limit = |v: f64, min: f64, max: f64| {
            let v = v.max(min);
            if max > 0.0 { v.min(max) } else { v }
        };
        w.width = limit(w.width + dw / own, min_w, max_w);
        w.height = limit(w.height + dh / own, min_h, max_h);
        w.points = vec![Point {
            point: "TOPLEFT".into(),
            relative: Some(screen),
            relative_point: "BOTTOMLEFT".into(),
            x: b.0 / s,
            y: (b.1 + b.3) / s,
        }];
        layout_changed(vm);
    }
    run_script(vm, grip, "OnMouseUp", &[left]);
}

// ----------------------------------------------------------- functions

/// `GetFramesRegisteredForEvent(event)`: the frames registered for it, in
/// dispatch order.
fn frames_registered_for_event(vm: &mut Vm, c: NativeCall) -> Ret {
    let e = vm.check_str(&c, 1)?;
    let e = String::from_utf8_lossy(vm.str_bytes(e)).into_owned();
    Ok(engine(vm)
        .world
        .widgets
        .iter()
        .filter(|w| w.events.contains_key(&e))
        .map(|w| Value::Table(w.table))
        .collect())
}

/// `C_EventUtils.IsEventValid(event)`: whether the client knows the event.
fn is_event_valid(vm: &mut Vm, c: NativeCall) -> Ret {
    let e = vm.check_str(&c, 1)?;
    let e = String::from_utf8_lossy(vm.str_bytes(e)).into_owned();
    Ok(vec![Value::Bool(engine(vm).api.events.contains(&e))])
}

/// `SetFontHeight(height)`: the font's height, kept as `SetFont` keeps it
/// (Blizzard's text size manager scales user-scalable fonts so).
fn set_font_height(vm: &mut Vm, id: WidgetId, c: NativeCall) -> Ret {
    let h = number_arg(vm, &c, 2)?;
    // The face as the font reports it now (its own, or its font object's).
    let table = Value::Table(engine(vm).world.widgets[id.0].table);
    let key = vm.str("GetFont");
    let getter = vm.index(table, key, None)?;
    let mut args = if getter.is_nil() {
        Vec::new()
    } else {
        vm.call(getter, &[table])?
    };
    args.resize(3, Value::Nil);
    if args[2].is_nil() {
        args[2] = vm.str("");
    }
    args[1] = Value::Num(h);
    store(vm, id, "Font", &args);
    Ok(vec![])
}

/// `SetScrollChild(child)`: stored, and the child placed by this frame.
fn set_scroll_child(vm: &mut Vm, id: WidgetId, c: NativeCall) -> Ret {
    let child = widget_of(vm, c.arg(2));
    // The previous child is no longer this frame's.
    let props = engine(vm).world.widgets[id.0].props;
    if let Some(old) = stored_args(vm, props, "ScrollChild")
        .and_then(|a| a.first().copied())
        .and_then(|v| widget_of(vm, v))
        && engine(vm).world.widgets[old.0].scroll_parent == Some(id)
    {
        engine_mut(vm).world.widgets[old.0].scroll_parent = None;
    }
    store(vm, id, "ScrollChild", &[c.arg(2)]);
    if let Some(child) = child {
        engine_mut(vm).world.widgets[child.0].scroll_parent = Some(id);
    }
    Ok(vec![])
}

/// A setter's argument as the client keeps it: colors in eight bits each
/// (text, shadow and vertex colors: `/stdtest engine` fontHeights), a
/// font's height as camelot keeps it (`kept_font_height`).
fn kept(vm: &Vm, stem: &str, i: usize, v: Value) -> Value {
    match (stem, i, v) {
        ("TextColor" | "ShadowColor" | "VertexColor", 0..=3, Value::Num(n)) => {
            Value::Num(byte_float(vm, n))
        }
        ("Font", 1, Value::Num(h)) if engine(vm).flavor == "camelot" => {
            Value::Num(kept_font_height(engine(vm).platform, h))
        }
        _ => v,
    }
}

/// The height camelot keeps for a font set to `h`, by machine (decision
/// T236): the Mac's recorded for 1 to 80 (13 reads back 12.999999046326);
/// Windows' 13 13.000000953674316 and 14 13.999999046325684 (the screen
/// probe's font objects, the same at five screens), its others as set
/// (unverified).
pub fn kept_font_height(platform: crate::Platform, h: f64) -> f64 {
    if platform == crate::Platform::Windows {
        return match h {
            13.0 => 13.000000953674316,
            14.0 => 13.999999046325684,
            _ => h,
        };
    }
    static TABLE: std::sync::OnceLock<HashMap<i64, f64>> = std::sync::OnceLock::new();
    let table = TABLE.get_or_init(|| {
        include_str!("../data/camelot-font-heights.txt")
            .lines()
            .filter(|l| !l.starts_with('#'))
            .filter_map(|l| l.split_once('\t'))
            .filter_map(|(a, b)| Some((a.parse().ok()?, b.parse().ok()?)))
            .collect()
    });
    if h.fract() == 0.0 {
        table.get(&(h as i64)).copied().unwrap_or(h)
    } else {
        h
    }
}

/// A color component as the client keeps it: in eight bits, read back as a
/// float (the byte times the float 1/255: `/stdtest engine` alpha 0.5 is
/// 0.50196081399918, `answers` 0.82 is 0.81960791349411).
pub fn byte_float(vm: &Vm, v: f64) -> f64 {
    let byte = (v.clamp(0.0, 1.0) * 255.0).round() as f32;
    crate::byte_channel(engine(vm).platform, byte)
}

/// A font object's color, shadow and user scaling given to a font that
/// inherits it (XML `inherits`).
pub fn inherit_font_style(vm: &mut Vm, from: WidgetId, to: WidgetId) {
    let (pf, pt) = (
        engine(vm).world.widgets[from.0].props,
        engine(vm).world.widgets[to.0].props,
    );
    for stem in ["TextColor", "ShadowColor", "ShadowOffset"] {
        if let Some(args) = stored_args(vm, pf, stem) {
            store(vm, to, stem, &args);
        }
    }
    let k = vm.str("canBeUserScaled");
    let v = vm.heap.table(pf).get(k);
    if !v.is_nil() {
        let _ = vm.heap.table_mut(pt).set(k, v);
    }
}

const FONT_INFO: &str = r#"return function(font, scalable, shadowed)
    local file, height, outline = font:GetFont()
    local r, g, b, a = font:GetTextColor()
    local function color(r, g, b, a)
        if CreateColor then return CreateColor(r, g, b, a) end
        return { r = r, g = g, b = b, a = a }
    end
    local shadow
    if shadowed then
        local sr, sg, sb, sa = font:GetShadowColor()
        local sx, sy = font:GetShadowOffset()
        shadow = { color = color(sr or 0, sg or 0, sb or 0, sa or 1), x = sx or 0, y = sy or 0 }
    end
    return {
        canBeUserScaled = scalable,
        color = color(r, g, b, a),
        fontObject = font,
        height = height and math.floor(height),
        outline = outline or "",
        shadow = shadow,
    }
end"#;

/// `GetFontInfo(font)`: a font object's (or a global font's, by name)
/// height, outline, color and shadow, colors made with Blizzard's
/// `CreateColor` (`/stdtest engine answers`).
fn get_font_info(vm: &mut Vm, c: NativeCall) -> Ret {
    let font = match c.arg(1) {
        Value::Str(s) => {
            let name = String::from_utf8_lossy(vm.str_bytes(s)).into_owned();
            vm.get_global(&name)
        }
        v => v,
    };
    let Some(id) = widget_of(vm, font) else {
        return Ok(vec![]);
    };
    let props = engine(vm).world.widgets[id.0].props;
    let k = vm.str("canBeUserScaled");
    let scalable = Value::Bool(vm.heap.table(props).get(k).truthy());
    // A shadow only when the font has one (recorded: no `shadow` else).
    let shadowed = stored_args(vm, props, "ShadowColor").is_some()
        || stored_args(vm, props, "ShadowOffset").is_some();
    let helper = crate::lua_helper(vm, "GetFontInfo", FONT_INFO)?;
    vm.call(helper, &[font, scalable, Value::Bool(shadowed)])
}

/// `CreateFont(name)`: a font object, global under its name.
fn create_font(vm: &mut Vm, c: NativeCall) -> Ret {
    let name = vm.check_str(&c, 1)?;
    let name = String::from_utf8_lossy(vm.str_bytes(name)).into_owned();
    Ok(vec![create(vm, "Font", Some(name), None)])
}

/// The frame system's API functions with behaviour.
/// `EnumerateFrames([frame])`: the first frame, or the one after `frame`
/// (regions, fonts and animations are no frames). Creation order here; the
/// client's is another (`/stdtest engine`: UIParent 212th of 3,331 named
/// frames, newer addons' frames first), its rule unknown.
fn enumerate_frames(vm: &mut Vm, c: NativeCall) -> Ret {
    let start = match c.arg(1) {
        Value::Nil => 0,
        v => match widget_of(vm, v) {
            Some(id) => id.0 + 1,
            None => return Ok(vec![Value::Nil]),
        },
    };
    // Addon code is not handed forbidden frames (unverified: ElvUI's
    // toolkit asks every enumerated frame's `GetObjectType` before
    // `IsForbidden`, error-free in the client, where that call on a
    // forbidden frame errors, `/stdtest engine`).
    let addon = crate::protection::tainted(vm);
    let e = engine(vm);
    let next = e.world.widgets[start.min(e.world.widgets.len())..]
        .iter()
        .find(|w| is_a(e, &w.class, "Frame") && !(addon && w.forbidden))
        .map(|w| Value::Table(w.table));
    Ok(vec![next.unwrap_or(Value::Nil)])
}

pub fn functions() -> Vec<(&'static str, wowlua_vm::NativeFn)> {
    vec![
        ("CreateFont", create_font),
        ("GetFontInfo", get_font_info),
        ("EnumerateFrames", enumerate_frames),
        ("GetFramesRegisteredForEvent", frames_registered_for_event),
        ("C_EventUtils.IsEventValid", is_event_valid),
    ]
}

/// A method's behaviour: the widget, the call (self first).
pub type MethodFn = fn(&mut Vm, WidgetId, NativeCall) -> Ret;

/// Stores `args` as setter `stem`'s on a widget's properties.
pub(crate) fn store(vm: &mut Vm, id: WidgetId, stem: &str, args: &[Value]) {
    if ORDER_STEMS.contains(&stem) {
        let props = engine(vm).world.widgets[id.0].props;
        let same = stored_ref(vm, props, stem).is_some_and(|old| {
            old.len() == args.len() && old.iter().zip(args).all(|(a, b)| a.raw_eq(*b))
        });
        if !same {
            reorder(engine_mut(vm));
        }
    }
    mark(engine_mut(vm), id);
    if stem == "KeyboardEnabled" {
        insert_sorted(&mut engine_mut(vm).world.keyboard, id.0);
    }
    // A texture shows one image: a plain color replaces its file or atlas,
    // a file its plain color, and an atlas its file or color, as in the client (an addon's
    // `SetTexture(WHITE8X8)` then `SetColorTexture(1, 1, 1, 0.04)` is a
    // faint line, not the white file).
    match stem {
        "ColorTexture" => {
            unstore(vm, id, "Texture");
            unstore(vm, id, "Atlas");
        }
        "Texture" => unstore(vm, id, "ColorTexture"),
        "Atlas" => {
            unstore(vm, id, "ColorTexture");
            unstore(vm, id, "Texture");
        }
        _ => {}
    }
    // A portrait (`SetPortraitTexture…`) gives way to any image set after.
    if matches!(stem, "ColorTexture" | "Texture" | "Atlas") {
        for k in ["\u{0}portrait", "\u{0}portraitlook", "\u{0}portraitdisplay"] {
            unstore(vm, id, k);
        }
    }
    let props = engine(vm).world.widgets[id.0].props;
    let list = vm.new_table();
    // A fixed-height font keeps its height as set.
    let fixed = stem == "Font"
        && matches!(args.get(2), Some(Value::Str(f)) if String::from_utf8_lossy(vm.str_bytes(*f)).contains("FIXEDHEIGHT"));
    for (k, v) in args.iter().enumerate() {
        let v = if fixed { *v } else { kept(vm, stem, k, *v) };
        let _ = vm.heap.table_mut(list).set(Value::Num(k as f64 + 1.0), v);
    }
    let n = vm.str("n");
    let _ = vm
        .heap
        .table_mut(list)
        .set(n, Value::Num(args.len() as f64));
    let key = vm.str(stem);
    let _ = vm.heap.table_mut(props).set(key, Value::Table(list));
}

/// A button's `kind` texture (`Normal`, `Highlight`): the one set, else a
/// new texture of the button's filling it (the highlight's in layer
/// HIGHLIGHT, the others' in ARTWORK; unverified: the layers).
fn button_texture(vm: &mut Vm, id: WidgetId, kind: &str) -> Value {
    mark(engine_mut(vm), id);
    let props = engine(vm).world.widgets[id.0].props;
    let stem = format!("{kind}Texture");
    if let Some(t) = stored_args(vm, props, &stem).and_then(|v| v.first().copied())
        && widget_of(vm, t).is_some()
    {
        return t;
    }
    let t = create(vm, "Texture", None, Some(id));
    if let Some(tid) = widget_of(vm, t) {
        let layer = vm.str(crate::buttons::state_layer(kind));
        store(vm, tid, "DrawLayer", &[layer, Value::Num(0.0)]);
        // All points on the button, as the client's state textures (the
        // default UI's micro buttons draw their icons so).
        let w = &mut engine_mut(vm).world.widgets[tid.0];
        for p in ["TOPLEFT", "BOTTOMRIGHT"] {
            w.points.push(Point {
                point: p.into(),
                relative: None,
                relative_point: p.into(),
                x: 0.0,
                y: 0.0,
            });
        }
        layout_changed(vm);
    }
    store(vm, id, &stem, &[t]);
    t
}

/// Forgets what a setter stored (`GetX` then answers as before any `SetX`).
pub(crate) fn unstore(vm: &mut Vm, id: WidgetId, stem: &str) {
    if ORDER_STEMS.contains(&stem) {
        reorder(engine_mut(vm));
    }
    mark(engine_mut(vm), id);
    let props = engine(vm).world.widgets[id.0].props;
    let key = vm.str(stem);
    let _ = vm.heap.table_mut(props).set(key, Value::Nil);
}

/// A texture's file (`SetTexture(file, …)`): kept, and the atlas it had
/// is gone (`GetAtlas` nil, as in the client).
pub(crate) fn set_texture_file(vm: &mut Vm, id: WidgetId, args: &[Value]) {
    store(vm, id, "Texture", args);
    unstore(vm, id, "Atlas");
}

/// `Texture:SetTexture(file, …)`: [`set_texture_file`]; it worked, but for
/// an empty path or file ID 0 (recorded: false for `""` and `0`, true for
/// `nil`, `{}` and `"NoSuchFile"`).
fn set_texture(vm: &mut Vm, id: WidgetId, c: NativeCall) -> Ret {
    let args: Vec<Value> = c.args.iter().skip(1).copied().collect();
    let worked = match vm.hidden(c.arg(2)) {
        Value::Num(n) => n != 0.0,
        Value::Str(s) => !vm.str_bytes(s).is_empty(),
        _ => true,
    };
    set_texture_file(vm, id, &args);
    Ok(vec![Value::Bool(worked)])
}

/// `SetXTexture(asset)`: a texture widget becomes the button's, a file its
/// texture's; `SetXAtlas(atlas, …)` the atlas of its texture.
fn set_button_texture(vm: &mut Vm, id: WidgetId, c: &NativeCall, kind: &str, atlas: bool) -> Ret {
    mark(engine_mut(vm), id);
    let asset = c.arg(2);
    if !atlas && let Some(new) = widget_of(vm, asset) {
        // The texture it replaces is hidden, back in ARTWORK (SuuUI probe
        // trees: the ammo slot's `<HighlightTexture>` replaces its
        // ItemButton's, which reports hidden in ARTWORK; one sample).
        let props = engine(vm).world.widgets[id.0].props;
        let old = stored_args(vm, props, &format!("{kind}Texture"))
            .and_then(|v| v.first().copied())
            .and_then(|v| widget_of(vm, v))
            .filter(|old| *old != new);
        if let Some(old) = old {
            engine_mut(vm).world.widgets[old.0].shown = false;
            let layer = vm.str("ARTWORK");
            store(vm, old, "DrawLayer", &[layer, Value::Num(0.0)]);
        }
        store(vm, id, &format!("{kind}Texture"), &[asset]);
        crate::buttons::refresh_states(vm, id);
        return Ok(vec![]);
    }
    let t = button_texture(vm, id, kind);
    if let Some(tid) = widget_of(vm, t) {
        let args: Vec<Value> = c.args.iter().skip(1).copied().collect();
        if atlas {
            store(vm, tid, "Atlas", &args);
            crate::measures::apply_atlas_size(vm, tid, &args);
        } else {
            set_texture_file(vm, tid, &args);
        }
    }
    if atlas {
        let args: Vec<Value> = c.args.iter().skip(1).copied().collect();
        store(vm, id, &format!("{kind}Atlas"), &args);
    } else {
        unstore(vm, id, &format!("{kind}Atlas"));
    }
    crate::buttons::refresh_states(vm, id);
    Ok(vec![])
}

macro_rules! button_textures {
    ($($kind:literal: $set:ident, $atlas:ident;)*) => {
        $(
            fn $set(vm: &mut Vm, id: WidgetId, c: NativeCall) -> Ret {
                set_button_texture(vm, id, &c, $kind, false)
            }
            fn $atlas(vm: &mut Vm, id: WidgetId, c: NativeCall) -> Ret {
                set_button_texture(vm, id, &c, $kind, true)
            }
        )*
        fn button_texture_methods() -> Vec<(&'static str, MethodFn)> {
            vec![$(
                (concat!("Set", $kind, "Texture"), $set as MethodFn),
                (concat!("Set", $kind, "Atlas"), $atlas as MethodFn),
            )*]
        }
    };
}

button_textures! {
    "Normal": set_normal_texture, set_normal_atlas;
    "Pushed": set_pushed_texture, set_pushed_atlas;
    "Disabled": set_disabled_texture, set_disabled_atlas;
    "Highlight": set_highlight_texture, set_highlight_atlas;
    "Checked": set_checked_texture, set_checked_atlas;
    "DisabledChecked": set_disabled_checked_texture, set_disabled_checked_atlas;
}

/// The methods with behaviour, by name (any class that has the method).
pub fn behaviours() -> Vec<(&'static str, MethodFn)> {
    fn font_string(vm: &mut Vm, id: WidgetId, c: NativeCall) -> Ret {
        create_region(vm, id, c, "FontString")
    }
    fn texture(vm: &mut Vm, id: WidgetId, c: NativeCall) -> Ret {
        create_region(vm, id, c, "Texture")
    }
    fn mask(vm: &mut Vm, id: WidgetId, c: NativeCall) -> Ret {
        create_region(vm, id, c, "MaskTexture")
    }
    fn line(vm: &mut Vm, id: WidgetId, c: NativeCall) -> Ret {
        create_region(vm, id, c, "Line")
    }
    vec![
        ("SetScript", set_script as MethodFn),
        ("GetScript", get_script),
        ("HookScript", hook_script),
        ("GetPropagateKeyboardInput", get_propagate_keyboard_input),
        ("HasScript", has_script_method),
        ("RegisterEvent", register_event),
        ("RegisterUnitEvent", register_unit_event),
        ("UnregisterEvent", unregister_event),
        ("UnregisterAllEvents", unregister_all_events),
        ("RegisterAllEvents", register_all_events),
        ("IsEventRegistered", is_event_registered),
        ("Show", show),
        ("Hide", hide),
        ("SetShown", set_shown_method),
        ("IsShown", is_shown),
        ("IsVisible", is_visible),
        ("GetName", get_name),
        ("GetDebugName", get_debug_name),
        ("GetObjectTable", get_object_table),
        ("GetObjectType", get_object_type),
        ("IsObjectType", is_object_type),
        ("GetParent", get_parent),
        ("SetParent", set_parent),
        ("GetChildren", get_children),
        ("GetRegions", get_regions),
        ("GetNumChildren", get_num_children),
        ("CreateFontString", font_string),
        ("AddMessage", add_message),
        ("SetColorRGB", set_color_rgb),
        ("GetColorRGB", get_color_rgb),
        ("SetColorHSV", set_color_hsv),
        ("GetColorHSV", get_color_hsv),
        ("SetColorAlpha", set_color_alpha),
        ("GetColorAlpha", get_color_alpha),
        ("GetFontStringByID", get_font_string_by_id),
        ("ResetMessageFadeByID", reset_message_fade),
        ("Clear", clear_messages),
        ("CreateTexture", texture),
        ("CreateActor", create_actor),
        ("GetNumActors", get_num_actors),
        ("GetActorAtIndex", get_actor_at_index),
        ("CreateMaskTexture", mask),
        ("CreateLine", line),
        ("SetPoint", set_point),
        ("GetPointByName", get_point_by_name),
        ("ClearAllPoints", clear_all_points),
        ("ClearPoint", clear_point),
        ("SetAllPoints", set_all_points),
        ("GetNumPoints", get_num_points),
        ("GetPoint", get_point),
        ("SetSize", set_size),
        ("SetWidth", set_width),
        ("SetHeight", set_height),
        ("GetWidth", get_width),
        ("GetHeight", get_height),
        ("GetSize", get_size),
        ("SetScale", set_scale),
        ("GetScale", get_scale),
        ("GetEffectiveScale", get_effective_scale),
        ("SetIgnoreParentScale", set_ignore_parent_scale),
        ("IsIgnoringParentScale", is_ignoring_parent_scale),
        ("IsIgnoringParentAlpha", is_ignoring_parent_alpha),
        ("SetAlpha", set_alpha),
        ("SetAlphaFromBoolean", set_alpha_from_boolean),
        ("SetTexture", set_texture),
        ("GetAlpha", get_alpha),
        ("GetLeft", get_left),
        ("GetRight", get_right),
        ("GetTop", get_top),
        ("GetBottom", get_bottom),
        ("GetCenter", get_center),
        ("GetRect", get_rect),
        ("SetText", set_text),
        ("SetTextToFit", set_text_to_fit),
        ("SetDrawLayer", set_draw_layer),
        ("SetNormalFontObject", set_normal_font_object),
        ("SetHighlightFontObject", set_highlight_font_object),
        ("SetDisabledFontObject", set_disabled_font_object),
        ("SetParentKey", set_parent_key),
        ("GetTextScale", get_text_scale),
        ("GetLineHeight", get_line_height),
        ("SetAttributeNoHandler", set_attribute_no_handler),
        ("GetStatusBarTexture", get_status_bar_texture),
        ("SetStatusBarTexture", set_status_bar_texture),
        ("SetFontString", set_font_string),
        ("GetText", get_text),
        ("GetNumber", get_number),
        ("SetNumber", set_number),
        ("SetFormattedText", set_formatted_text),
        ("SetAlphaGradient", set_alpha_gradient),
        ("SetAttribute", set_attribute),
        ("GetAttribute", get_attribute),
        ("ExecuteAttribute", execute_attribute),
        ("IsProtected", is_protected),
        ("Click", click),
        ("RegisterForClicks", register_for_clicks),
        ("IsForbidden", is_forbidden),
        ("SetFontObject", set_font_object),
        ("CopyFontObject", copy_font_object),
        ("GetFrameLevel", get_frame_level),
        ("SetFrameLevel", set_frame_level),
        ("SetGradient", set_gradient),
        ("SetUsingParentLevel", set_using_parent_level),
        ("SetToDefaults", set_to_defaults),
        ("GetID", get_id),
        ("GetStringWidth", get_string_width),
        ("GetTextWidth", get_text_width),
        ("GetUnboundedStringWidth", get_unbounded_string_width),
        ("GetFont", get_font),
        ("StartMoving", start_moving),
        ("StartSizing", start_sizing),
        ("StopMovingOrSizing", stop_moving_or_sizing),
        ("SetResizeBounds", set_resize_bounds),
        ("GetResizeBounds", get_resize_bounds),
        ("SetEnabled", set_enabled),
        ("Enable", enable),
        ("Disable", disable),
        ("IsEnabled", is_enabled),
        ("SetVertexColor", set_vertex_color),
        ("SetFontHeight", set_font_height),
        ("SetScrollChild", set_scroll_child),
        ("GetVertexColor", get_vertex_color),
    ]
    .into_iter()
    .chain(button_texture_methods())
    .collect()
}
