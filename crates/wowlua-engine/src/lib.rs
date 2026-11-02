//! The WoW client's engine for the wowlua VM (spec emulator §17.6): the API
//! a flavor's documentation describes, as natives of the VM (decision T198),
//! with behaviour where tests need it; a function without behaviour fails
//! with "`<name>` is not mocked", so gaps are visible.

/// The files and clock the engine reads through (`wowlua-host`).
pub use wowlua_host as host;

pub mod achievements;
pub mod actions;
pub mod actions2;
pub mod animations;
pub mod api;
pub mod args;
pub mod auction;
pub mod auras;
pub mod aurastates;
pub mod bank;
pub mod bars;
pub mod batch;
pub mod bindings;
pub mod blanks;
pub mod buttons;
pub mod camera;
pub mod casting;
pub mod channels;
pub mod character;
pub mod chat;
pub mod chatwindows;
pub mod classic;
pub mod clientdata;
pub mod clock;
pub mod collected;
pub mod collections;
pub mod conditionals;
pub mod content;
pub mod controls;
pub mod coverage;
pub mod cursor;
pub mod curves;
pub mod damagemeter;
pub mod death;
pub mod editmode;
pub mod encounters;
pub mod equipsets;
pub mod eventlog;
pub mod extras;
pub mod facts;
pub mod formatters;
pub mod framescript;
pub mod gamedata;
pub mod globalsinfo;
pub mod gossip;
pub mod graphics;
pub mod guild;
pub mod healcalc;
pub mod input;
pub mod inspect;
pub mod inventory;
pub mod iteminfo;
pub mod itemlocation;
pub mod leftovers2;
pub mod leftovers3;
pub mod levels;
pub mod lfg;
pub mod login;
pub mod loot;
pub mod macros;
pub mod mail;
pub mod maps;
pub mod measures;
pub mod merchant;
pub mod methods2;
pub mod methods3;
pub mod misc;
pub mod models;
pub mod movement;
pub mod objects2;
pub mod party;
pub mod pets;
pub mod play;
pub mod protection;
pub mod queries;
pub mod questinfo;
pub mod quests;
pub mod recorded;
pub mod replies;
pub mod scene;
pub mod social;
pub mod spellinfo;
pub mod standins;
pub mod stats;
pub mod systeminfo;
pub mod targeting;
pub mod taxi;
pub mod text;
pub mod timeline;
pub mod tooltip;
pub mod tradeskill;
pub mod trainer;
pub mod traits;
pub mod ui;
pub mod unitacts;
pub mod unitinfo;
pub mod unitnames;
pub mod units;
pub mod widgetdata;
pub mod widgetops;
pub mod widgets;

use std::collections::HashMap;

use wowlua_vm::stdlib::Ret;
use wowlua_vm::{NativeCall, NativeFn, Value, Vm};

/// The engine's state, kept in the VM's host slot.
pub struct Engine {
    pub flavor: String,
    /// The machine the client runs on (decision T236): its machine-specific
    /// answers and `IsMacClient`.
    pub platform: Platform,
    /// The screen it draws on (decision T237).
    pub screen: Screen,
    /// The 3D world is on (decision T522): the player and the units stand
    /// on the terrain of the installation, and the world is drawn behind
    /// the UI. Off unless turned on ([`set_world`]).
    pub world_on: bool,
    pub api: &'static api::Api,
    /// Set by a behaviour whose answer has nothing to hide this call.
    pub plain_results: bool,
    /// Behaviour by function name (`C_Timer.After`).
    behaviours: HashMap<&'static str, NativeFn>,
    /// The functions answered by a fill-in (`fill_ins`): secure code's
    /// calls of them are still noted for the probes, as defaulted ones.
    fill_ins: std::collections::HashSet<&'static str>,
    /// Widget methods' behaviour by name (`Show`), or by class and name
    /// (`Button:Click`) where a class differs. Looked up, never iterated:
    /// its hasher is not observable.
    methods: rustc_hash::FxHashMap<&'static str, widgets::MethodFn>,
    /// Each class's lineage (`widgets::lineage`), worked out once: the
    /// documentation's classes do not change.
    pub(crate) lineages: std::cell::RefCell<rustc_hash::FxHashMap<String, std::rc::Rc<[String]>>>,
    /// The rectangles of the scene being built (decision T538).
    pub(crate) scene_bounds: std::cell::RefCell<Option<widgets::KnownBounds>>,
    /// The frames' items of the last scene, kept (decision T542).
    pub(crate) scene_kept: std::cell::RefCell<scene::Kept>,
    pub world: widgets::World,
    pub timers: clock::Timers,
    /// The events owed at the frame's end (`batch`).
    pub batch: batch::Batch,
    /// The damage meter's sessions (`damagemeter`).
    pub damage_meter: damagemeter::DamageMeter,
    pub units: units::Units,
    /// The action bar page (`conditionals`).
    pub bars: conditionals::Bars,
    /// Animation groups playing.
    pub animations: animations::Animations,
    /// Protected actions refused so far.
    pub refusals: Vec<protection::Refusal>,
    /// Functions without behaviour that secure code called (they returned
    /// their documented types' empty values), by name.
    pub defaulted: std::collections::BTreeSet<&'static str>,
    /// Those calls with their arguments (`name(1, "x")`), and the client's
    /// undocumented functions secure code called: what a probe asks the
    /// client (`defaultedCalls`).
    pub defaulted_calls: std::collections::BTreeSet<String>,
    /// Lua helpers the engine compiled once (`lua_helper`).
    pub lua_helpers: HashMap<&'static str, Value>,
    /// Tables Blizzard's code alone sees while it loads.
    pub secure_tables: Vec<SecureTable>,
    /// The addons' globals (`GetGlobalEnvironment`), whatever environment
    /// Blizzard's secure code runs in.
    pub main_globals: wowlua_vm::TableRef,
    /// Documented namespaces only Blizzard's secure code has (none of their
    /// functions in the client's addon environment: `C_PingSecure`).
    pub secure_namespaces: Vec<(wowlua_vm::StrRef, Value)>,
    /// The behaviours of undocumented functions that check their arguments
    /// first (`args::install_usages`), by name.
    pub(crate) usage_checked: HashMap<&'static str, NativeFn>,
    /// The client's own tables, where the host has the installation.
    pub client_data: Option<std::sync::Arc<dyn clientdata::ClientData>>,
}

impl Engine {
    /// The engine a VM runs, if it has one.
    pub fn of(vm: &Vm) -> Option<&Engine> {
        vm.host.as_ref()?.downcast_ref()
    }

    /// An API function's behaviour, by name (`C_Timer.After`).
    pub fn behaviour(&self, name: &str) -> Option<NativeFn> {
        self.behaviours.get(name).copied()
    }

    /// A widget method's behaviour, by its full name or its name.
    pub fn method_behaviour(&self, full: &str, short: &str) -> Option<widgets::MethodFn> {
        self.methods
            .get(full)
            .or_else(|| self.methods.get(short))
            .copied()
    }
}

/// The engine of a VM made with [`install`].
pub fn engine(vm: &Vm) -> &Engine {
    Engine::of(vm).expect("the engine is installed")
}

pub fn engine_mut(vm: &mut Vm) -> &mut Engine {
    vm.host
        .as_mut()
        .and_then(|h| h.downcast_mut())
        .expect("the engine is installed")
}

/// The machine a test's client runs on (decision T236): the recorded
/// answers that differ by machine (`data/camelot-answers-<platform>.txt`:
/// key names, graphics, sound, colours' last digits) and `IsMacClient`,
/// `IsWindowsClient`. The Mac by default, where the first recordings were
/// made.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Platform {
    #[default]
    Mac,
    Windows,
}

impl Platform {
    /// `"mac"` or `"windows"`, as the manifest and `config` name it.
    pub fn parse(name: &str) -> Option<Platform> {
        match name {
            "mac" => Some(Platform::Mac),
            "windows" => Some(Platform::Windows),
            _ => None,
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Platform::Mac => "mac",
            Platform::Windows => "windows",
        }
    }
}

/// The screen the client draws on (decision T237): its size in pixels, its
/// DPI scale, and the UI scale when "Use UI Scale" is on. The recorded
/// Mac's by default: 1920 x 1080, DPI scale 1.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Screen {
    pub width: f64,
    pub height: f64,
    pub dpi_scale: f64,
    pub ui_scale: Option<f64>,
}

impl Default for Screen {
    fn default() -> Self {
        Screen {
            width: 1920.0,
            height: 1080.0,
            dpi_scale: 1.0,
            ui_scale: None,
        }
    }
}

impl Screen {
    /// UIParent's scale: the UI scale when it is used, else 768 over the
    /// screen's height times its DPI scale, at most 1; single floats
    /// (recorded at 1280x720, 1600x900, 1920x1080 and 2560x1440 on Windows,
    /// 1920x1080 on the Mac, and at UI scale 1.15).
    pub fn ui_parent_scale(&self) -> f64 {
        let s = match self.ui_scale {
            Some(s) => s as f32,
            // 768 times the DPI scale, over the height (recorded 0.80000001192
            // at 2560x1440, DPI scale 1.5: dividing first gives a float more).
            None => ((768.0f32 * self.dpi_scale as f32) / self.height as f32).min(1.0),
        };
        f64::from(s)
    }

    /// UIParent's size in its units: the screen at 768 units high, over
    /// UIParent's scale, in single floats (recorded: the Mac's
    /// 1920.0001220703 by 1080, Windows' 1365.3333740234 by 768).
    pub fn ui_parent_size(&self) -> (f64, f64) {
        let wide = (self.width * 768.0 / self.height) as f32;
        // The UI scale as set (a CVar's text) divides in double precision
        // (recorded at 1.15: 1187.246337890625); the client's own scale in
        // single.
        if let Some(ui) = self.ui_scale {
            return (
                f64::from((self.width * 768.0 / self.height / ui) as f32),
                f64::from((768.0 / ui) as f32),
            );
        }
        let s = self.ui_parent_scale() as f32;
        (f64::from(wide / s), f64::from(768.0f32 / s))
    }

    /// WorldFrame's size: 768 units high at any resolution and UI scale, its
    /// width following the screen's aspect ratio (recorded on five 16:9
    /// screens: 1365.3333740234 by 768).
    pub fn world_frame_size(&self) -> (f64, f64) {
        (f64::from((self.width * 768.0 / self.height) as f32), 768.0)
    }
}

/// Sets the screen the client draws on (after `install`): UIParent's size
/// follows, and with a UI scale "Use UI Scale" and its slider's CVars
/// (`useUiScale`, `uiScale`) read as it (decision T482).
/// Turns the 3D world on or off (decision T522).
pub fn set_world(vm: &mut Vm, on: bool) {
    engine_mut(vm).world_on = on;
}

pub fn set_screen(vm: &mut Vm, screen: Screen) {
    place_screen(vm, screen);
    if let Some(ui) = screen.ui_scale {
        framescript::cvar_set(vm, "useUiScale", "1".into());
        framescript::cvar_set(vm, "uiScale", format!("{ui:.6}"));
    }
}

/// The screen changed while the client runs, as the game's settings change
/// it (decision T482: the window's size, `C_VideoOptions.SetGameWindowSize`;
/// the UI scale, the `useUiScale` and `uiScale` CVars): UIParent follows,
/// then `DISPLAY_SIZE_CHANGED` when the size changed and `UI_SCALE_CHANGED`
/// when UIParent's scale did (unverified: their order, and whether a size
/// that changes UIParent's scale tells both).
pub fn change_screen(vm: &mut Vm, screen: Screen) {
    let before = engine(vm).screen;
    if before == screen {
        return;
    }
    place_screen(vm, screen);
    if (before.width, before.height) != (screen.width, screen.height) {
        widgets::fire_event(vm, "DISPLAY_SIZE_CHANGED", &[]);
    }
    if before.ui_parent_scale() != screen.ui_parent_scale() {
        widgets::fire_event(vm, "UI_SCALE_CHANGED", &[]);
    }
}

/// A host changes the game's settings as the Graphics settings do
/// (decision T482, Studio's toolbar): the window's size, then "Use UI
/// Scale" and its slider (none: off), set untainted as the client's own.
pub fn apply_settings(vm: &mut Vm, width: f64, height: f64, ui_scale: Option<f64>) {
    let mut screen = engine(vm).screen;
    (screen.width, screen.height) = (width.floor(), height.floor());
    change_screen(vm, screen);
    let saved = *vm.taint();
    {
        let t = vm.taint_mut();
        t.stack = wowlua_vm::Taint::SECURE;
        t.fixed = wowlua_vm::Taint::SECURE;
    }
    if let Some(ui) = ui_scale {
        let _ = framescript::set_cvar_text(vm, "uiScale", format!("{ui}"), true);
    }
    let on = if ui_scale.is_some() { "1" } else { "0" };
    let _ = framescript::set_cvar_text(vm, "useUiScale", on.into(), true);
    *vm.taint_mut() = saved;
}

/// The screen with the UI scale its CVars set: `uiScale` when `useUiScale`
/// is on, else none.
pub(crate) fn screen_from_cvars(vm: &mut Vm) -> Screen {
    let mut screen = engine(vm).screen;
    let on = framescript::cvar_get(vm, "useUiScale").is_some_and(|v| v.trim() == "1");
    // (a flavor without the client's CVars: the slider's default, 1)
    let scale =
        framescript::cvar_get(vm, "uiScale").map_or(Some(1.0), |v| v.trim().parse::<f64>().ok());
    screen.ui_scale = match (on, scale) {
        (true, Some(s)) if s > 0.0 => Some(s),
        _ => None,
    };
    screen
}

fn place_screen(vm: &mut Vm, screen: Screen) {
    engine_mut(vm).world.scene_epoch += 1;
    engine_mut(vm).screen = screen;
    let ui = vm.get_global("UIParent");
    if let Some(id) = widgets::widget_of(vm, ui) {
        let (w, h) = screen.ui_parent_size();
        let widget = &mut engine_mut(vm).world.widgets[id.0];
        widget.width = w;
        widget.height = h;
        widget.scale = screen.ui_parent_scale();
        widgets::layout_changed(vm);
    }
    let world = vm.get_global("WorldFrame");
    if let Some(id) = widgets::widget_of(vm, world) {
        let widget = &mut engine_mut(vm).world.widgets[id.0];
        (widget.width, widget.height) = screen.world_frame_size();
        widgets::layout_changed(vm);
    }
}

/// A colour channel kept in eight bits, read back as the machine does it
/// (decision T236): the Mac multiplies the byte by the float 1/255 (209 is
/// 0.81960791349411), Windows divides it by 255 (0.81960785388947; both
/// recorded, the answers battery's colours and `GetFontInfo`).
pub(crate) fn byte_channel(platform: Platform, byte: f32) -> f64 {
    match platform {
        Platform::Mac => f64::from(byte * (1.0f32 / 255.0)),
        Platform::Windows => f64::from(byte / 255.0f32),
    }
}

/// Sets the machine the client runs on (after `install`).
pub fn set_platform(vm: &mut Vm, platform: Platform) {
    engine_mut(vm).platform = platform;
}

/// A CVar of a flavor's client: its name, default and flags.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CVar {
    pub name: String,
    pub default: String,
    /// Addons may not set it.
    pub secure: bool,
    pub read_only: bool,
    /// Locked from the user.
    pub locked: bool,
}

/// The CVars a flavor's client has, with their defaults and flags, in the
/// recorded order: camelot's recorded by `/stdtest engine`; other flavors
/// only the deprecation fallbacks, which the client has.
pub fn cvars(flavor: &str) -> Vec<CVar> {
    if flavor != "camelot" {
        return vec![CVar {
            name: "loadDeprecationFallbacks".into(),
            default: "1".into(),
            secure: false,
            read_only: false,
            locked: false,
        }];
    }
    let flags: Vec<(&str, &str)> = include_str!("../data/camelot-cvar-flags.txt")
        .lines()
        .filter(|l| !l.starts_with('#'))
        .filter_map(|l| l.split_once('\t'))
        .collect();
    let has = |kind: &str, name: &str| {
        flags
            .iter()
            .any(|(k, n)| *k == kind && n.eq_ignore_ascii_case(name))
    };
    include_str!("../data/camelot-cvars.txt")
        .lines()
        .filter(|l| !l.starts_with('#'))
        .filter_map(|l| l.split_once('\t'))
        .map(|(name, default)| CVar {
            name: name.to_owned(),
            default: default.to_owned(),
            secure: has("secure", name),
            read_only: has("readOnly", name),
            locked: has("locked", name),
        })
        .collect()
}

/// Installs a flavor's engine into a VM made with `Vm::wow`: every
/// documented global and namespaced function (those the VM's runtime does
/// not already define), and the `Enum` and `Constants` tables.
pub fn install(vm: &mut Vm, flavor: &str) -> Result<(), String> {
    let api = api::api(flavor).ok_or_else(|| format!("no API documentation for `{flavor}`"))?;
    for (i, f) in api.functions.iter().enumerate() {
        let index = Value::Num(i as f64);
        match f.namespace {
            None => {
                if vm.get_global(f.name).is_nil() {
                    let native = vm.native_with(f.short, api_function, vec![index]);
                    vm.set_global(f.name, native);
                }
            }
            Some(ns) => {
                let table = namespace(vm, ns);
                let key = vm.heap.intern(f.short.as_bytes());
                if vm.heap.table(table).get_str(key).is_nil() {
                    let native = vm.native_with(f.short, api_function, vec![index]);
                    vm.heap.table_mut(table).set_str(key, native);
                }
            }
        }
    }
    let enums = namespace(vm, "Enum");
    for (name, fields) in &api.enums {
        let t = vm.new_table();
        for (field, value) in fields {
            let k = vm.heap.intern(field.as_bytes());
            vm.heap.table_mut(t).set_str(k, Value::Num(*value));
        }
        let k = vm.heap.intern(name.as_bytes());
        vm.heap.table_mut(enums).set_str(k, Value::Table(t));
    }
    for (name, [n, min, max]) in &api.enum_meta {
        let t = vm.new_table();
        for (field, value) in [("NumValues", n), ("MinValue", min), ("MaxValue", max)] {
            let k = vm.heap.intern(field.as_bytes());
            vm.heap.table_mut(t).set_str(k, Value::Num(*value));
        }
        let k = vm.heap.intern(name.as_bytes());
        vm.heap.table_mut(enums).set_str(k, Value::Table(t));
    }
    let constants = namespace(vm, "Constants");
    for (name, values) in &api.constants {
        let t = vm.new_table();
        for (field, value) in values {
            let v = match value {
                serde_json::Value::Number(n) => Value::Num(n.as_f64().unwrap_or(0.0)),
                serde_json::Value::String(s) => vm.str(s),
                serde_json::Value::Bool(b) => Value::Bool(*b),
                _ => continue,
            };
            let k = vm.heap.intern(field.as_bytes());
            vm.heap.table_mut(t).set_str(k, v);
        }
        let k = vm.heap.intern(name.as_bytes());
        vm.heap.table_mut(constants).set_str(k, Value::Table(t));
    }
    // The number globals the client defines (`LE_*`).
    for line in client_data(flavor).map_or("", |d| d.globals).lines() {
        if let Some((name, n)) = line.split_once('\t')
            && let Ok(n) = n.parse::<f64>()
            && vm.get_global(name).is_nil()
        {
            vm.set_global(name, Value::Num(n));
        }
    }
    let secure_tables = recorded_tables(vm, client_data(flavor).map_or("", |d| d.enums));
    // The engine's own behaviours, the recorded answers first: any
    // behaviour of the engine's own takes their place (and a fill-in's that
    // a setter of its changes, `OVER_RECORDED`).
    let own: HashMap<&'static str, NativeFn> = own_behaviours(flavor)
        .into_iter()
        .flat_map(|m| {
            let recorded = m.recorded;
            m.list
                .into_iter()
                .filter(move |(name, _)| !recorded || !OVER_RECORDED.contains(name))
        })
        .collect();
    // The fill-ins answer only what nothing else does.
    let fill: Vec<(&'static str, NativeFn)> = fill_ins().into_iter().flat_map(|m| m.list).collect();
    let fill_ins: std::collections::HashSet<&'static str> = fill
        .iter()
        .map(|(name, _)| *name)
        .filter(|name| !own.contains_key(name))
        .collect();
    vm.host = Some(Box::new(Engine {
        flavor: flavor.to_string(),
        platform: Platform::Mac,
        screen: Screen::default(),
        world_on: false,
        api,
        behaviours: fill.into_iter().chain(own).collect(),
        fill_ins,
        // The fill-in methods first: the engine's own take their place.
        methods: method_behaviours()
            .into_iter()
            .flat_map(|m| m.list)
            .collect(),
        lineages: Default::default(),
        scene_bounds: Default::default(),
        scene_kept: Default::default(),
        world: widgets::World::default(),
        timers: clock::Timers::default(),
        batch: batch::Batch::default(),
        damage_meter: Default::default(),
        units: units::Units::default(),
        bars: Default::default(),
        refusals: Vec::new(),
        animations: Default::default(),
        defaulted: Default::default(),
        defaulted_calls: Default::default(),
        plain_results: false,
        lua_helpers: Default::default(),
        secure_tables,
        main_globals: vm.globals,
        secure_namespaces: Vec::new(),
        usage_checked: HashMap::new(),
        client_data: None,
    }));
    vm.untrusted_layout_forbidden = Some(protection::untrusted_layout_forbidden);
    // Protected functions the documentation lacks.
    for &name in protection::PROTECTED {
        if flavor == "camelot" && CAMELOT_ABSENT.contains(&name) {
            continue;
        }
        let (ns, short) = match name.split_once('.') {
            Some((ns, short)) => (Some(ns), short),
            None => (None, name),
        };
        let upval = vm.str(name);
        match ns {
            None if vm.get_global(short).is_nil() => {
                let native = vm.native_with(short, protection::protected_global, vec![upval]);
                vm.set_global(short, native);
            }
            Some(ns) => {
                let t = namespace(vm, ns);
                let key = vm.heap.intern(short.as_bytes());
                if vm.heap.table(t).get_str(key).is_nil() {
                    let native = vm.native_with(short, protection::protected_global, vec![upval]);
                    vm.heap.table_mut(t).set_str(key, native);
                }
            }
            None => {}
        }
    }
    // Functions needing the user's action the documentation lacks.
    for &name in protection::HARDWARE {
        if !name.contains('.') && vm.get_global(name).is_nil() {
            let upval = vm.str(name);
            let native = vm.native_with(name, protection::hardware_global, vec![upval]);
            vm.set_global(name, native);
        }
    }
    // Functions with behaviour the documentation lacks (`IsInGroup`): the
    // client has them (camelot's inventory; other flavors unverified).
    // By name: the globals made in the same order every run (a map's
    // order is random; `pairs(_G)` and the handles followed it).
    let mut names: Vec<(&'static str, NativeFn)> = engine(vm)
        .behaviours
        .iter()
        .map(|(&n, &f)| (n, f))
        .collect();
    names.sort_by_key(|(n, _)| *n);
    for (name, f) in names {
        if flavor == "camelot" && CAMELOT_ABSENT.contains(&name) {
            continue;
        }
        let (ns, short) = match name.split_once('.') {
            Some((ns, short)) => (Some(ns), short),
            None => (None, name),
        };
        let exists = match ns {
            None => !vm.get_global(short).is_nil(),
            Some(ns) => {
                let t = namespace(vm, ns);
                let key = vm.heap.intern(short.as_bytes());
                !vm.heap.table(t).get_str(key).is_nil()
            }
        };
        if !exists {
            // Its full name as the upvalue (behaviours shared by several
            // functions tell them apart by it).
            let full = vm.str(name);
            let native = vm.native_with(short, f, vec![full]);
            match ns {
                None => vm.set_global(short, native),
                Some(ns) => {
                    let t = namespace(vm, ns);
                    let key = vm.heap.intern(short.as_bytes());
                    vm.heap.table_mut(t).set_str(key, native);
                }
            }
        }
    }
    // The client's recorded answers about itself: documented functions get
    // them as behaviour, the others as natives of their own.
    for name in facts::recorded_names(flavor) {
        let (ns, short) = match name.split_once('.') {
            Some((ns, short)) => (Some(ns), short),
            None => (None, name),
        };
        let table = match ns {
            None => vm.globals,
            Some(ns) => namespace(vm, ns),
        };
        let key = vm.heap.intern(short.as_bytes());
        if !vm.heap.table(table).get_str(key).is_nil() {
            // A recorded answer takes a fill-in's place.
            let engine = engine_mut(vm);
            if engine.fill_ins.contains(name) && !OVER_RECORDED.contains(&name) {
                engine.behaviours.insert(name, facts::recorded_call);
            } else {
                engine
                    .behaviours
                    .entry(name)
                    .or_insert(facts::recorded_call);
            }
            continue;
        }
        let upval = vm.str(name);
        let native = vm.native_with(short, facts::recorded_call, vec![upval]);
        vm.heap.table_mut(table).set_str(key, native);
    }
    // The documentation's secure-only functions (`C_PingSecure`): for the
    // secure environment, without behaviour.
    let mut secure_only: std::collections::BTreeMap<&str, Vec<&'static str>> = Default::default();
    for (ns, f) in &api.secure_only {
        secure_only.entry(ns).or_default().push(f);
    }
    for (ns, functions) in secure_only {
        let t = vm.new_table();
        for f in functions {
            let name = vm.str(&format!("{ns}.{f}"));
            let native = vm.native_with(f, undocumented_function, vec![name]);
            let k = vm.heap.intern(f.as_bytes());
            vm.heap.table_mut(t).set_str(k, native);
        }
        let k = vm.heap.intern(ns.as_bytes());
        widgets::keep(vm, Value::Table(t));
        widgets::keep(vm, Value::Str(k));
        engine_mut(vm).secure_namespaces.push((k, Value::Table(t)));
    }
    // Documented namespaces none of whose functions the client's addons
    // have: Blizzard's secure code's alone.
    if flavor == "camelot" {
        // A documented global the addons lack (recorded): the secure
        // environment has it (`ui::secure_env`).
        let k = vm.heap.intern(b"secretunwrap");
        vm.heap.table_mut(vm.globals).set_str(k, Value::Nil);
        let client: std::collections::HashSet<&str> = include_str!("../data/camelot-functions.txt")
            .lines()
            .filter_map(|l| l.split_once('.').map(|(ns, _)| ns))
            .collect();
        let documented: std::collections::BTreeSet<&str> =
            api.functions.iter().filter_map(|f| f.namespace).collect();
        for ns in documented {
            if client.contains(ns) || !ns.starts_with("C_") {
                continue;
            }
            let k = vm.heap.intern(ns.as_bytes());
            let v = vm.heap.table(vm.globals).get_str(k);
            if v.is_nil() {
                continue;
            }
            widgets::keep(vm, v);
            widgets::keep(vm, Value::Str(k));
            engine_mut(vm).secure_namespaces.push((k, v));
            vm.heap.table_mut(vm.globals).set_str(k, Value::Nil);
        }
    }
    // The client's undocumented C functions (its inventory): they exist,
    // without behaviour.
    if let Some(inventory) = inventory(flavor) {
        for line in inventory.lines() {
            if line.starts_with('#') || line.is_empty() {
                continue;
            }
            let (ns, short) = match line.split_once('.') {
                Some((ns, short)) => (Some(ns), short),
                None => (None, line),
            };
            let name = vm.str(line);
            match ns {
                None => {
                    if vm.get_global(short).is_nil() {
                        let native = vm.native_with(short, undocumented_function, vec![name]);
                        vm.set_global(short, native);
                    }
                }
                Some(ns) => {
                    let t = namespace(vm, ns);
                    let key = vm.heap.intern(short.as_bytes());
                    if vm.heap.table(t).get_str(key).is_nil() {
                        let native = vm.native_with(short, undocumented_function, vec![name]);
                        vm.heap.table_mut(t).set_str(key, native);
                    }
                }
            }
        }
    }
    // The undocumented functions' own argument checks (camelot's).
    if flavor == "camelot" {
        args::install_usages(vm);
    }
    units::install(vm);
    // The camelot character leads a guild, as the recorded one does
    // (`guild.rs`; decision: the default character is in a guild).
    if flavor == "camelot" {
        engine_mut(vm).world.chat.guild = Some(guild::Guild::recorded());
    }
    // The CVars' defaults.
    let cvars: Vec<(String, String)> = self::cvars(flavor)
        .into_iter()
        .map(|c| (c.name, c.default))
        .collect();
    engine_mut(vm).world.cvar_defaults = cvars.clone();
    engine_mut(vm).world.cvars.extend(cvars);
    // The Graphics settings' CVars as their levels are (decision T561).
    graphics::settle(vm);
    // Undocumented C functions with behaviour.
    let create_frame = vm.native("CreateFrame", widgets::create_frame);
    vm.set_global("CreateFrame", create_frame);
    // The client's project ID, and the project constants (WoW Forever has
    // these, `/stdtest engine`; other flavors unverified). Blizzard's UI
    // code sets its own again when it loads.
    let project = wowlua_flavors::table()
        .flavor(flavor)
        .map_or(1, |f| f.project_id);
    vm.set_global("WOW_PROJECT_ID", Value::Num(f64::from(project)));
    vm.set_global("WOW_PROJECT_MAINLINE", Value::Num(1.0));
    vm.set_global("WOW_PROJECT_CLASSIC", Value::Num(2.0));
    if flavor == "camelot" {
        vm.set_global("WOW_PROJECT_CAMELOT", Value::Num(18.0));
    }
    // Blizzard's slash command registry (its chat code makes it; addons
    // and the engine's slash control use it without the UI code too).
    let slash = vm.new_table();
    vm.set_global("SlashCmdList", Value::Table(slash));
    // The frames the client starts with.
    let ui = widgets::create(vm, "Frame", Some("UIParent".into()), None);
    if let Some(id) = widgets::widget_of(vm, ui) {
        engine_mut(vm).world.ui_parent = Some(id);
        let w = &mut engine_mut(vm).world.widgets[id.0];
        let (width, height) = Screen::default().ui_parent_size();
        w.width = width;
        w.height = height;
        w.scale = Screen::default().ui_parent_scale();
        // Anchored to the screen at both corners, and protected
        // (`/stdtest engine`).
        w.protected = true;
        w.explicitly_protected = true;
        for p in ["TOPLEFT", "BOTTOMRIGHT"] {
            w.points.push(widgets::Point {
                point: p.into(),
                relative: None,
                relative_point: p.into(),
                x: 0.0,
                y: 0.0,
            });
        }
    }
    // WorldFrame covers the screen too (`widgets::bounds`), unscaled.
    let world = widgets::create(vm, "Frame", Some("WorldFrame".into()), None);
    if let Some(id) = widgets::widget_of(vm, world) {
        let w = &mut engine_mut(vm).world.widgets[id.0];
        (w.width, w.height) = Screen::default().world_frame_size();
    }
    // What Blizzard's UI code defines that addons use as the client's
    // (until that code loads).
    standins::install(vm);
    // The errors the client drops without a report are the engine's
    // errors too (`ui::DROPPED`).
    vm.on_dropped_error = Some(ui::dropped_error);
    Ok(())
}

/// `Enum` and `Constants` as the client has them where its documentation
/// differs (`data/<flavor>-enums.txt`, from `/stdtest engine`): a line
/// `Enum.X.Field<TAB>number 5` sets a field, `…<TAB>-` removes a field or a
/// whole table, `…<TAB>secure` takes a table out for Blizzard's code
/// alone (returned: put back while it loads, `ui::load`).
/// The client's own `Enum` and `Constants` changes and number globals:
/// camelot's recorded (`/stdtest engine`), the other flavors' from Ketho's
/// dumps and their EnvironmentCleanup.lua (decision T554).
struct ClientData {
    enums: &'static str,
    globals: &'static str,
}

fn client_data(flavor: &str) -> Option<ClientData> {
    macro_rules! data {
        ($f:literal) => {
            ClientData {
                enums: include_str!(concat!("../data/", $f, "-enums.txt")),
                globals: include_str!(concat!("../data/", $f, "-globals.txt")),
            }
        };
    }
    Some(match flavor {
        "camelot" => data!("camelot"),
        "standard" => data!("standard"),
        "mists" => data!("mists"),
        "tbc" => data!("tbc"),
        "vanilla" => data!("vanilla"),
        _ => return None,
    })
}

fn recorded_tables(vm: &mut Vm, data: &str) -> Vec<SecureTable> {
    let mut secure = Vec::new();
    for line in data.lines().filter(|l| !l.starts_with('#')) {
        let Some((name, value)) = line.split_once('\t') else {
            continue;
        };
        let mut parts = name.splitn(3, '.');
        let (Some(root), Some(table)) = (parts.next(), parts.next()) else {
            continue;
        };
        let root = namespace(vm, root);
        let tkey = vm.heap.intern(table.as_bytes());
        let Some(field) = parts.next() else {
            if value == "secure" {
                let table = match vm.heap.table(root).get_str(tkey) {
                    t @ Value::Table(_) => t,
                    _ => stand_in_enum(vm),
                };
                widgets::keep(vm, table);
                widgets::keep(vm, Value::Str(tkey));
                secure.push(SecureTable {
                    root,
                    name: tkey,
                    table,
                });
            }
            if value == "-" || value == "secure" {
                vm.heap.table_mut(root).set_str(tkey, Value::Nil);
            }
            continue;
        };
        let t = match vm.heap.table(root).get_str(tkey) {
            Value::Table(t) => t,
            _ if value == "-" => continue,
            _ => {
                let t = vm.new_table();
                vm.heap.table_mut(root).set_str(tkey, Value::Table(t));
                t
            }
        };
        let v = match value.split_once(' ') {
            Some(("number", n)) => n.parse().map_or(Value::Nil, Value::Num),
            Some(("string", s)) => vm.str(s),
            Some(("boolean", b)) => Value::Bool(b == "true"),
            _ => Value::Nil,
        };
        let fkey = vm.heap.intern(field.as_bytes());
        vm.heap.table_mut(t).set_str(fkey, v);
    }
    secure
}

/// A table only Blizzard's code sees while it loads (`Enum.StoreError`).
#[derive(Clone, Copy)]
pub struct SecureTable {
    pub root: wowlua_vm::TableRef,
    pub name: wowlua_vm::StrRef,
    pub table: Value,
}

/// An enumeration whose values are unknown: each field read gets the next
/// number (0, 1, …) and keeps it.
fn stand_in_enum(vm: &mut Vm) -> Value {
    fn next_value(vm: &mut Vm, c: NativeCall) -> Ret {
        let Value::Table(t) = c.arg(1) else {
            return Ok(vec![Value::Nil]);
        };
        let n = vm.heap.table(t).entries().count() as f64;
        let _ = vm.heap.table_mut(t).set(c.arg(2), Value::Num(n));
        Ok(vec![Value::Num(n)])
    }
    let (t, meta) = (vm.new_table(), vm.new_table());
    let index = vm.native("__index", next_value);
    let k = vm.names.index;
    vm.heap.table_mut(meta).set_str(k, index);
    vm.heap.table_mut(t).meta = Some(meta);
    Value::Table(t)
}

/// The global table `name`, made if missing.
fn namespace(vm: &mut Vm, name: &str) -> wowlua_vm::TableRef {
    if let Value::Table(t) = vm.get_global(name) {
        return t;
    }
    let t = vm.new_table();
    vm.set_global(name, Value::Table(t));
    t
}

/// A function's documented results as empty values, even for one that may
/// return nothing.
pub fn documented_values(vm: &mut Vm, api: &api::Api, doc: &serde_json::Value) -> Vec<Value> {
    doc.get("Returns")
        .and_then(serde_json::Value::as_array)
        .cloned()
        .unwrap_or_default()
        .iter()
        .filter(|r| r.get("StrideIndex").is_none())
        .map(|r| default_value(vm, api, r, 0))
        .collect()
}

/// A function's documented results as empty values: nil where nilable,
/// false, 0, "", an empty table, none of a list (unverified stand-ins).
pub fn defaults(vm: &mut Vm, api: &api::Api, doc: &serde_json::Value) -> Vec<Value> {
    let returns = doc
        .get("Returns")
        .and_then(serde_json::Value::as_array)
        .cloned()
        .unwrap_or_default();
    if doc
        .get("MayReturnNothing")
        .and_then(serde_json::Value::as_bool)
        == Some(true)
    {
        return vec![];
    }
    // A list of values (`StrideIndex`): none.
    returns
        .iter()
        .filter(|r| r.get("StrideIndex").is_none())
        .map(|r| default_value(vm, api, r, 0))
        .collect()
}

/// A documented value's empty value: a structure with its fields' (to a
/// depth), a number 0, a string "", false, an empty table.
pub(crate) fn default_value(
    vm: &mut Vm,
    api: &api::Api,
    r: &serde_json::Value,
    depth: usize,
) -> Value {
    let nilable = r.get("Nilable").and_then(serde_json::Value::as_bool) == Some(true);
    let ty = r
        .get("Type")
        .and_then(serde_json::Value::as_str)
        .unwrap_or("");
    if nilable {
        return Value::Nil;
    }
    // `Seconds` (declared `any`) a number: SuuUI probe timeline,
    // `C_EncounterTimeline` answered 0.
    let ty = match ty {
        "Seconds" => "number",
        _ => api.aliases.get(ty).map(String::as_str).unwrap_or(ty),
    };
    match ty {
        "bool" | "boolean" => Value::Bool(false),
        "string" | "cstring" => vm.str(""),
        "table" => Value::Table(vm.new_table()),
        _ if api.structures.contains(ty) => {
            let t = vm.new_table();
            if depth < 4
                && let Some(fields) = api.structure_fields.get(ty)
            {
                for f in fields {
                    let Some(name) = f.get("Name").and_then(serde_json::Value::as_str) else {
                        continue;
                    };
                    let v = default_value(vm, api, f, depth + 1);
                    if !v.is_nil() {
                        let k = vm.str(name);
                        let _ = vm.heap.table_mut(t).set(k, v);
                    }
                }
            }
            Value::Table(t)
        }
        _ if api.enum_names.contains(ty) => api
            .enums
            .iter()
            .find(|(n, _)| *n == ty)
            .and_then(|(_, f)| f.first().map(|(_, v)| Value::Num(*v)))
            .unwrap_or(Value::Num(0.0)),
        _ if ty.contains("string") => vm.str(""),
        // Numbers by their alias; other types are objects. By their names
        // `normalizedValue` and `FrameTime` too (unverified).
        "number" | "luaIndex" | "uiUnit" | "fileID" | "time_t" | "BigInteger" | "BigUInteger"
        | "WOWMONEY" | "SingleColorValue" | "uiFontHeight" | "normalizedValue" | "FrameTime" => {
            Value::Num(0.0)
        }
        // An object of its mixin (`ColorMixin`), when Blizzard's code
        // defined it.
        _ if r.get("Mixin").is_some() => {
            let t = vm.new_table();
            let mixin = r.get("Mixin").and_then(serde_json::Value::as_str);
            if let Some(Value::Table(m)) = mixin.map(|m| vm.get_global(m)) {
                let methods: Vec<(Value, Value)> = vm.heap.table(m).entries().collect();
                for (k, f) in methods {
                    let _ = vm.heap.table_mut(t).set(k, f);
                }
            }
            // A vector at the origin (its coordinates are fields).
            let axes: &[&str] = match ty {
                "vector2" | "Vector2DMixin" => &["x", "y"],
                "vector3" | "Vector3DMixin" => &["x", "y", "z"],
                _ => &[],
            };
            for axis in axes {
                let k = vm.str(axis);
                let _ = vm.heap.table_mut(t).set(k, Value::Num(0.0));
            }
            Value::Table(t)
        }
        _ if ty.ends_with("ID") || ty.ends_with("Id") || ty.ends_with("Index") => Value::Num(0.0),
        _ => Value::Table(vm.new_table()),
    }
}

/// An undocumented C function without behaviour: nothing for Blizzard's
/// code, "not mocked" for the addon's.
pub(crate) fn undocumented_function(vm: &mut Vm, c: NativeCall) -> Ret {
    let name = match c.upvals.first().copied() {
        Some(Value::Str(s)) => String::from_utf8_lossy(vm.str_bytes(s)).into_owned(),
        _ => "?".into(),
    };
    // Some have behaviour (the engine's world answers them).
    if let Some(f) = engine(vm).behaviours.get(name.as_str()).copied() {
        return f(vm, c);
    }
    if protection::blizzard_caller(vm) {
        note_default(vm, &name, &c, 1);
        return Ok(vec![]);
    }
    Err(vm.error(&format!("{name} is not mocked")))
}

/// A function the engine writes in Lua (`source` returns it), compiled
/// once and kept; secure.
pub fn lua_helper(
    vm: &mut Vm,
    name: &'static str,
    source: &str,
) -> Result<Value, wowlua_vm::LuaError> {
    if let Some(v) = engine(vm).lua_helpers.get(name).copied() {
        return Ok(v);
    }
    let f = vm
        .load(source.as_bytes(), &format!("=[engine {name}]"))
        .map_err(|e| vm.error(&e))?;
    let v = vm.call(f, &[])?.first().copied().unwrap_or_default();
    widgets::keep(vm, v);
    engine_mut(vm).lua_helpers.insert(name, v);
    Ok(v)
}

/// Notes a call secure code made without an answer: `name(args)`, the
/// arguments from `first` on (literals; tables as `{}`).
pub(crate) fn note_default(vm: &mut Vm, name: &str, c: &NativeCall, first: usize) {
    let call = call_text(vm, name, c, first);
    engine_mut(vm).defaulted_calls.insert(call);
}

/// A call as text, `name(1, "x", nil)`: its arguments from `first` on
/// (literals; tables as `{}`). Probes ask the client these calls, and
/// their answers are looked up by them.
pub(crate) fn call_text(vm: &Vm, name: &str, c: &NativeCall, first: usize) -> String {
    let args: Vec<String> = (first..=c.len())
        .map(|i| match vm.hidden(c.arg(i)) {
            Value::Nil => "nil".to_string(),
            Value::Bool(b) => b.to_string(),
            Value::Num(n) if n.fract() == 0.0 && n.abs() < 1e15 => format!("{}", n as i64),
            Value::Num(n) => format!("{n}"),
            Value::Str(s) => format!("{:?}", String::from_utf8_lossy(vm.str_bytes(s))),
            Value::Table(_) => "{}".to_string(),
            _ => "?".to_string(),
        })
        .collect();
    format!("{name}({})", args.join(", "))
}

/// Globals camelot's client lacks though the engine has their behaviour:
/// the `C_` functions' old global names (`C_Container.PickupContainerItem`
/// keeps the behaviour) and retail's removed `GetMouseFocus`. The client's
/// inventory (`/stdtest inventory`, forever-camelot-functions.tsv: every
/// function of the environment, C or Lua, with Blizzard's UI loaded) has
/// none of them; camelot's documentation names them only in their `C_`
/// namespaces and its UI code defines none (its deprecation files define
/// `HasAction` and the other action functions, which the engine keeps).
/// The cursor probe (forever-camelot-cursor.lua) called the `C_` ones.
/// Other flavors keep them (unverified: their inventories are not
/// recorded; their UI code defines `GetItemCount` and `UseItemByName`,
/// cata's `InviteUnit`).
pub(crate) const CAMELOT_ABSENT: &[&str] = &[
    "GetItemCount",
    "GetMouseFocus",
    "InviteUnit",
    "PickupContainerItem",
    "PickupSpell",
    "PickupSpellBookItem",
    "RunMacroText",
    "SplitContainerItem",
    "UseContainerItem",
    "UseItemByName",
];

/// Fill-ins whose answer a setter of theirs changes: they take the place
/// of the recorded answer, which they give until set (the dungeon
/// difficulty recorded 1, the action bar toggles, the heirloom and wardrobe filters recorded
/// checked, nothing super-tracked, no map pin).
const OVER_RECORDED: &[&str] = &[
    // The log's quests at their recorded places (`questinfo.rs`): the recorded
    // answer was the recording character's log.
    "C_QuestLog.GetQuestsOnMap",
    // The tracked filters' rule (`systeminfo.rs`), over the one recorded
    // filter.
    "C_Minimap.IsFilteredOut",
    // A rule over the recorded calls (the full probe: any sex, numbers
    // taken whole, the pets' class 0).
    "GetSpecializationInfoForClassID",
    // An inspected unit's none (the full probe).
    "GetNumSpecializations",
    // The full probe's constants (`leftovers2.rs`), the recorded calls'
    // answers for every argument.
    "C_PetBattles.CanPetSwapIn",
    "C_KeyBindings.GetBindingContextForAction",
    "C_GameRules.GetGameRuleAsFrameStrata",
    "C_SpecializationInfo.GetNumSpecializationsForClassID",
    "C_SpecializationInfo.GetPvpTalentSlotInfo",
    "C_ContentTracking.GetTrackablesOnMap",
    "C_Club.ShouldAllowClubType",
    "C_Club.GetAvatarIdList",
    "GetDungeonDifficultyID",
    // The full probe's answers over the load's (an older build's): no
    // error callstack height set (nil), no expansion trial (false, nil).
    "GetErrorCallstackHeight",
    "GetExpansionTrialInfo",
    // Rules over the recorded calls, asking them first: a game message's
    // name from its index (the full probe: 1 `ERR_SYSTEM`), a monitor of
    // none.
    "GetGameMessageInfo",
    "GetMonitorName",
    "GetActionBarToggles",
    "C_Heirloom.GetCollectedHeirloomFilter",
    "C_Heirloom.GetUncollectedHeirloomFilter",
    "C_Heirloom.GetClassAndSpecFilters",
    "C_HeirloomInfo.IsUsingDefaultFilters",
    "C_HeirloomInfo.SetDefaultFilters",
    "C_TransmogCollection.IsUsingDefaultFilters",
    "C_SuperTrack.GetSuperTrackedQuestID",
    "C_SuperTrack.GetSuperTrackedMapPin",
    "C_SuperTrack.GetSuperTrackedContent",
    "C_SuperTrack.GetSuperTrackedVignette",
    "C_Map.GetUserWaypointPositionForMap",
    // The journal's mounts: the recorded ones' info and the full probe's
    // (`collections.rs`).
    "C_MountJournal.GetMountInfoByID",
    "C_MountJournal.GetMountInfoExtraByID",
    // A rule over the one recorded call (the full probe: the joined
    // channels' shortcuts, nil past them).
    "C_ChatInfo.GetChannelShortcutForChannelID",
    // Nothing at load (recorded), nil once logged in (the full probe).
    "C_VoiceChat.GetActiveChannelID",
    "C_QuestSession.GetSessionBeginDetails",
    // The recorded first four slots' empty totem for every slot
    // `GetNumTotemSlots` counts (`spellinfo.rs`).
    "GetTotemInfo",
];

/// The C functions of a flavor's client, one per line: camelot's recorded
/// (`/stdtest inventory`), standard's, mists', tbc's and vanilla's from
/// Ketho's client dumps (`cargo xtask types gen`), checked against the
/// clients (Retail 12.1.5, Mists 5.5.4, TBC 2.5.6, Classic Era 1.15.9):
/// wowlua lacks only `Kiosk.GetCharacterTemplateSetIndex` of their C
/// functions (and Retail's `C_SocialRestrictions.IsAgeVerificationRestricted`).
pub(crate) fn inventory(flavor: &str) -> Option<&'static str> {
    Some(match flavor {
        "camelot" => include_str!("../data/camelot-functions.txt"),
        "standard" => include_str!("../data/standard-functions.txt"),
        "mists" => include_str!("../data/mists-functions.txt"),
        "tbc" => include_str!("../data/tbc-functions.txt"),
        "vanilla" => include_str!("../data/vanilla-functions.txt"),
        _ => return None,
    })
}

/// The fill-in modules' behaviours: answers for functions no recording
/// or model of the engine answers, from Blizzard's documentation and UI
/// code, warcraft.wiki.gg and the functions' names (unverified).
pub(crate) fn fill_ins() -> Vec<Behaviours<NativeFn>> {
    vec![
        Behaviours::new("unitinfo", unitinfo::behaviours()),
        Behaviours::new("spellinfo", spellinfo::behaviours()),
        Behaviours::new("iteminfo", iteminfo::behaviours()),
        Behaviours::new("social", social::behaviours()),
        Behaviours::new("content", content::behaviours()),
        Behaviours::new("questinfo", questinfo::behaviours()),
        Behaviours::new("collections", collections::behaviours()),
        Behaviours::new("extras", extras::behaviours()),
        Behaviours::new("globalsinfo", globalsinfo::behaviours()),
        Behaviours::new("systeminfo", systeminfo::behaviours()),
        Behaviours::new("actions2", actions2::behaviours()),
        Behaviours::new("leftovers2", leftovers2::behaviours()),
        // Last: what the client answers with nothing there (the full
        // probe) takes the place of the guesses above.
        Behaviours::new("blanks", blanks::behaviours()),
    ]
}

/// A module's behaviours: its name, whether they answer from the client's
/// recordings (`data/camelot-answers.txt`, the recorded talent trees), and
/// the functions or methods with their behaviour.
pub struct Behaviours<F> {
    pub module: &'static str,
    pub recorded: bool,
    pub list: Vec<(&'static str, F)>,
}

impl<F> Behaviours<F> {
    fn new(module: &'static str, list: Vec<(&'static str, F)>) -> Self {
        Behaviours {
            module,
            recorded: false,
            list,
        }
    }

    fn recorded(module: &'static str, list: Vec<(&'static str, F)>) -> Self {
        Behaviours {
            module,
            recorded: true,
            list,
        }
    }
}

/// The engine's own behaviours by module, in the order they take each
/// other's place (a later module's over an earlier one's): the recorded
/// answers first.
pub(crate) fn own_behaviours(flavor: &str) -> Vec<Behaviours<NativeFn>> {
    vec![
        Behaviours::recorded("recorded", recorded::behaviours(flavor)),
        Behaviours::recorded("traits", traits::recorded_behaviours(flavor)),
        // The client's tables over the recordings, where the host has them.
        Behaviours::new("clientdata", clientdata::behaviours()),
        Behaviours::new("clock", clock::behaviours()),
        Behaviours::new("units", units::behaviours()),
        Behaviours::new("units", units::more_behaviours()),
        Behaviours::new("targeting", targeting::behaviours()),
        Behaviours::new("leftovers3", leftovers3::behaviours()),
        Behaviours::new("curves", curves::behaviours()),
        Behaviours::new("healcalc", healcalc::behaviours()),
        Behaviours::new("damagemeter", damagemeter::behaviours()),
        Behaviours::new("formatters", formatters::behaviours()),
        Behaviours::new("widgets", widgets::functions()),
        Behaviours::new("auras", auras::behaviours()),
        Behaviours::new("gamedata", gamedata::behaviours()),
        Behaviours::new("character", character::behaviours()),
        Behaviours::new("stats", stats::behaviours()),
        Behaviours::new("actions", actions::behaviours()),
        Behaviours::new("editmode", editmode::behaviours()),
        Behaviours::new("channels", channels::behaviours()),
        Behaviours::new("macros", macros::behaviours()),
        Behaviours::new("cursor", cursor::behaviours()),
        Behaviours::new("input", input::behaviours()),
        Behaviours::new("casting", casting::behaviours()),
        Behaviours::new("merchant", merchant::behaviours()),
        Behaviours::new("loot", loot::behaviours()),
        Behaviours::new("quests", quests::behaviours()),
        Behaviours::new("gossip", gossip::behaviours()),
        Behaviours::new("trainer", trainer::behaviours()),
        Behaviours::new("taxi", taxi::behaviours()),
        Behaviours::new("maps", maps::behaviours()),
        Behaviours::new("misc", misc::behaviours()),
        Behaviours::new("mail", mail::behaviours()),
        Behaviours::new("bank", bank::behaviours()),
        Behaviours::new("auction", auction::behaviours()),
        Behaviours::new("itemlocation", itemlocation::behaviours()),
        Behaviours::new("death", death::behaviours()),
        Behaviours::new("inventory", inventory::behaviours()),
        Behaviours::new("queries", queries::behaviours()),
        Behaviours::new("objects2", objects2::behaviours()),
        Behaviours::new("party", party::behaviours()),
        Behaviours::new("inspect", inspect::behaviours()),
        Behaviours::new("traits", traits::behaviours(flavor)),
        Behaviours::new("classic", classic::behaviours(flavor)),
        Behaviours::new("achievements", achievements::behaviours()),
        Behaviours::new("tradeskill", tradeskill::behaviours(flavor)),
        Behaviours::new("tooltip", tooltip::behaviours()),
        Behaviours::new("chat", chat::behaviours()),
        Behaviours::new("movement", movement::behaviours()),
        Behaviours::new("camera", camera::behaviours()),
        Behaviours::new("replies", replies::behaviours()),
        Behaviours::new("bindings", bindings::behaviours()),
        Behaviours::new("facts", facts::behaviours()),
        Behaviours::new("equipsets", equipsets::behaviours()),
        Behaviours::new("encounters", encounters::behaviours()),
        Behaviours::new("pets", pets::behaviours()),
        Behaviours::new("conditionals", conditionals::behaviours()),
        Behaviours::new("lfg", lfg::behaviours()),
        Behaviours::new("timeline", timeline::behaviours()),
        Behaviours::new("chatwindows", chatwindows::behaviours()),
        Behaviours::new("guild", guild::behaviours()),
        Behaviours::new("collected", collected::behaviours()),
        Behaviours::new(
            "ui",
            vec![
                ("import", ui::build::import as NativeFn),
                (
                    "C_XMLUtil.GetTemplateInfo",
                    ui::build::template_info as NativeFn,
                ),
            ],
        ),
        Behaviours::new("framescript", framescript::functions()),
        Behaviours::new("standins", standins::behaviours()),
    ]
}

/// The widget methods' behaviours by module, in the order they take each
/// other's place: the fill-ins (`methods2`, `methods3`) first.
pub(crate) fn method_behaviours() -> Vec<Behaviours<widgets::MethodFn>> {
    vec![
        Behaviours::new("methods2", methods2::methods()),
        Behaviours::new("methods3", methods3::methods()),
        Behaviours::new("widgets", widgets::behaviours()),
        Behaviours::new("curves", curves::methods()),
        Behaviours::new("objects2", objects2::methods()),
        Behaviours::new("clock", clock::methods()),
        Behaviours::new("healcalc", healcalc::methods()),
        Behaviours::new("formatters", formatters::methods()),
        Behaviours::new("framescript", framescript::methods()),
        Behaviours::new("animations", animations::methods()),
        Behaviours::new("tooltip", tooltip::methods()),
        Behaviours::new("input", input::methods()),
        Behaviours::new("bars", bars::methods()),
        Behaviours::new("buttons", buttons::methods()),
        Behaviours::new("measures", measures::methods()),
        Behaviours::new("models", models::methods()),
        Behaviours::new("widgetops", widgetops::methods()),
    ]
}

/// Every API function: its behaviour, or the "not mocked" error.
fn api_function(vm: &mut Vm, c: NativeCall) -> Ret {
    let Some(Value::Num(i)) = c.upvals.first().copied() else {
        return Err(vm.error("an API function without its index"));
    };
    let Some(engine) = Engine::of(vm) else {
        return Err(vm.error("the engine is not installed"));
    };
    let api = engine.api;
    let f = &api.functions[i as usize];
    let behaviour = engine.behaviours.get(f.name).copied();
    // Refused before the arguments are checked (unverified).
    if protection::is_protected(f.name, Some(&f.doc)) && protection::tainted(vm) {
        let action = protection::action_name(vm);
        protection::refuse(vm, "ADDON_ACTION_FORBIDDEN", action);
        return Ok(defaults(vm, api, &f.doc));
    }
    if protection::refuses_without_user(vm, f.name) {
        return Ok(defaults(vm, api, &f.doc));
    }
    // Nothing, by the documentation's requirements (`extras`).
    if extras::silent_unchecked(&f.doc) {
        return Ok(vec![]);
    }
    args::check(vm, api, f, &c)?;
    if extras::silent_checked(&f.doc) && !timeline::valid(vm, &c) {
        return Ok(vec![]);
    }
    let Some(behaviour) = behaviour else {
        // Blizzard's code gets empty values; the addon's code an error.
        if protection::blizzard_caller(vm) {
            engine_mut(vm).defaulted.insert(f.name);
            note_default(vm, f.name, &c, 1);
            return Ok(defaults(vm, api, &f.doc));
        }
        return Err(vm.error(&format!("{} is not mocked", f.name)));
    };
    // The client's secrecy: results secret as the function's documentation
    // and the restrictions active say; a return marked never secret stays.
    let flags = *f.secret_flags.get_or_init(|| units::secret_flags(&f.doc));
    let secret = units::secret_results_flagged(vm, flags, &c);
    if engine_mut(vm).fill_ins.contains(f.name) && protection::blizzard_caller(vm) {
        note_default(vm, f.name, &c, 1);
    }
    let results = behaviour(vm, c)?;
    // A behaviour may say its answer has nothing to hide (an empty action
    // slot's cooldown, recorded plain in combat).
    let plain = std::mem::take(&mut engine_mut(vm).plain_results);
    if !secret || plain {
        return Ok(results);
    }
    let returns = f
        .doc
        .get("Returns")
        .and_then(serde_json::Value::as_array)
        .map_or(&[][..], Vec::as_slice);
    Ok(results
        .into_iter()
        .enumerate()
        .map(|(i, v)| {
            let never = returns
                .get(i)
                .and_then(|r| r.get("NeverSecret"))
                .and_then(serde_json::Value::as_bool)
                == Some(true);
            if never || v.is_nil() {
                return v;
            }
            // A structure stays readable, its fields secret but those
            // documented `NeverSecret` (`SpellCooldownInfo`'s `isActive`,
            // `isEnabled`, `isOnGCD`: recorded in combat).
            let ty = returns
                .get(i)
                .and_then(|r| r.get("Type"))
                .and_then(serde_json::Value::as_str);
            if let (Some(ty), Value::Table(t)) = (ty, v)
                && let Some(fields) = api.structure_fields.get(ty)
            {
                for field in fields {
                    let Some(name) = field.get("Name").and_then(serde_json::Value::as_str) else {
                        continue;
                    };
                    if field
                        .get("NeverSecret")
                        .and_then(serde_json::Value::as_bool)
                        == Some(true)
                    {
                        continue;
                    }
                    let k = vm.str(name);
                    let fv = vm.heap.table(t).get(k);
                    if !fv.is_nil() {
                        let w = vm.secret_wrap(fv);
                        let _ = vm.heap.table_mut(t).set(k, w);
                    }
                }
                return v;
            }
            // A curve's color stays a readable color, its channels secret
            // (`UnitHealthPercent(unit, true, colorCurve)`: ElvUI's oUF
            // health bars call `GetRGB` on it, error-free in the client;
            // unverified by a probe).
            if ty == Some("LuaCurveEvaluatedResult")
                && let Value::Table(t) = v
            {
                for name in ["r", "g", "b", "a"] {
                    let k = vm.str(name);
                    let fv = vm.heap.table(t).get(k);
                    if !fv.is_nil() {
                        let w = vm.secret_wrap(fv);
                        let _ = vm.heap.table_mut(t).set(k, w);
                    }
                }
                return v;
            }
            vm.secret_wrap(v)
        })
        .collect())
}
