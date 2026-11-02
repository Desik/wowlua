//! The client's C helpers for Lua code (the documentation's `FrameScript`
//! and neighbours): mixins, event callbacks, CVars, build information and
//! the login state, which Blizzard's UI code calls as it loads.
//!
//! Unverified (the documentation's signatures, behaviour inferred from
//! Blizzard's callers): event callbacks run after the frames' `OnEvent`,
//! global ones as `callback(nil, ...)` and a frame's as
//! `callback(frame, ...)`; `CreateSecureDelegate` and
//! `C_FunctionContainers.CreateCallback` return the function; an atlas's
//! information is a stand-in (atlases are game data); a CVar nobody
//! registered reads as nil.

use wowlua_vm::stdlib::Ret;
use wowlua_vm::{NativeCall, NativeFn, Value, Vm};

use crate::widgets::{self, WidgetId};
use crate::{engine, engine_mut};

/// An event callback: for an event (and unit), of a frame or global.
pub struct Callback {
    pub event: String,
    pub unit: Option<String>,
    pub owner: Option<WidgetId>,
    pub f: Value,
}

fn copy_fields(vm: &mut Vm, into: Value, from: Value) {
    let (Value::Table(dst), Value::Table(src)) = (into, from) else {
        return;
    };
    let mut k = Value::Nil;
    while let Some((key, v)) = vm.heap.table(src).next(k).ok().flatten() {
        let _ = vm.heap.table_mut(dst).set(key, v);
        k = key;
    }
}

/// `Mixin(object, ...)`: the mixins' fields copied into the object.
fn mixin(vm: &mut Vm, c: NativeCall) -> Ret {
    let object = c.arg(1);
    for m in c.args.iter().skip(1) {
        copy_fields(vm, object, *m);
    }
    Ok(vec![object])
}

/// `CreateFromMixins(...)`: a new table with the mixins' fields.
fn create_from_mixins(vm: &mut Vm, c: NativeCall) -> Ret {
    let object = Value::Table(vm.new_table());
    for m in c.args.iter() {
        copy_fields(vm, object, *m);
    }
    Ok(vec![object])
}

/// `GetCurrentEnvironment()`: the calling function's environment.
fn current_environment(vm: &mut Vm, _c: NativeCall) -> Ret {
    Ok(vec![Value::Table(vm.caller_env(1))])
}

/// `Get<Class>Metatable()`: the metatable widgets of the class share.
fn class_metatable(class: &'static str) -> impl Fn(&mut Vm, NativeCall) -> Ret {
    move |vm: &mut Vm, _c: NativeCall| Ok(vec![Value::Table(widgets::class_meta(vm, class))])
}

macro_rules! metatable_fns {
    ($($name:ident => $class:literal),*) => {
        $(fn $name(vm: &mut Vm, c: NativeCall) -> Ret {
            class_metatable($class)(vm, c)
        })*
    };
}

metatable_fns!(
    frame_metatable => "Frame",
    button_metatable => "Button",
    texture_metatable => "Texture",
    font_string_metatable => "FontString",
    edit_box_metatable => "EditBox"
);

/// Returns its first argument (a stand-in that keeps the function).
fn same(_vm: &mut Vm, c: NativeCall) -> Ret {
    Ok(vec![c.arg(1)])
}

fn nothing(_vm: &mut Vm, _c: NativeCall) -> Ret {
    Ok(vec![])
}

fn falsy(_vm: &mut Vm, _c: NativeCall) -> Ret {
    Ok(vec![Value::Bool(false)])
}

fn zero(_vm: &mut Vm, _c: NativeCall) -> Ret {
    Ok(vec![Value::Num(0.0)])
}

fn string_arg(vm: &mut Vm, c: &NativeCall, n: usize) -> Result<String, wowlua_vm::LuaError> {
    let s = vm.check_str(c, n)?;
    Ok(String::from_utf8_lossy(vm.str_bytes(s)).into_owned())
}

fn register(vm: &mut Vm, event: String, unit: Option<String>, owner: Option<WidgetId>, f: Value) {
    widgets::keep(vm, f);
    engine_mut(vm).world.callbacks.push(Callback {
        event,
        unit,
        owner,
        f,
    });
}

fn unregister(vm: &mut Vm, event: &str, owner: Option<WidgetId>, f: Option<Value>) {
    engine_mut(vm).world.callbacks.retain(|cb| {
        let same_f = match (f, cb.f) {
            (None, _) => true,
            (Some(Value::Func(a)), Value::Func(b)) => a == b,
            (Some(Value::Table(a)), Value::Table(b)) => a == b,
            _ => false,
        };
        !(cb.event == event && cb.owner == owner && same_f)
    });
}

/// The callback events the documentation marks `HasRestrictions`: only
/// Blizzard's untainted code may register a callback for them.
const RESTRICTED_CALLBACK_EVENTS: &[&str] = &[
    "MINIMAP_PING",
    "COMBAT_LOG_ENTRIES_CLEARED",
    "COMBAT_LOG_APPLY_FILTER_SETTINGS",
    "COMBAT_LOG_REFILTER_ENTRIES",
];

/// Whether tainted code's callback for `event` is forbidden: then
/// `ADDON_ACTION_FORBIDDEN(addon, action)` and nothing registers (probed
/// in WoW Forever, 2026-10-23: `Frame:RegisterEventCallback()` for
/// `MINIMAP_PING` and `COMBAT_LOG_REFILTER_ENTRIES` from `/run`, and the
/// global for `MINIMAP_PING`; `TOOLTIP_SHOW_ITEM_COMPARISON` registers).
fn forbidden_callback(vm: &mut Vm, event: &str, action: String) -> bool {
    if !RESTRICTED_CALLBACK_EVENTS.contains(&event) || !crate::protection::tainted(vm) {
        return false;
    }
    crate::protection::refuse(vm, "ADDON_ACTION_FORBIDDEN", action);
    true
}

fn register_event_callback(vm: &mut Vm, c: NativeCall) -> Ret {
    let event = string_arg(vm, &c, 1)?;
    // The global's action name is unverified.
    let action = crate::protection::action_name(vm);
    if !forbidden_callback(vm, &event, action) {
        register(vm, event, None, None, c.arg(2));
    }
    Ok(vec![])
}

fn register_unit_event_callback(vm: &mut Vm, c: NativeCall) -> Ret {
    let event = string_arg(vm, &c, 1)?;
    let unit = string_arg(vm, &c, 3)?;
    let action = crate::protection::action_name(vm);
    if !forbidden_callback(vm, &event, action) {
        register(vm, event, Some(unit), None, c.arg(2));
    }
    Ok(vec![])
}

fn unregister_event_callback(vm: &mut Vm, c: NativeCall) -> Ret {
    let event = string_arg(vm, &c, 1)?;
    unregister(vm, &event, None, Some(c.arg(2)));
    Ok(vec![])
}

/// `frame:RegisterEventCallback(event, fn)`: true once registered; false
/// for a restricted event from tainted code (probed).
fn frame_register_event_callback(vm: &mut Vm, id: WidgetId, c: NativeCall) -> Ret {
    let event = string_arg(vm, &c, 2)?;
    if forbidden_callback(vm, &event, "Frame:RegisterEventCallback()".into()) {
        return Ok(vec![Value::Bool(false)]);
    }
    register(vm, event, None, Some(id), c.arg(3));
    Ok(vec![Value::Bool(true)])
}

fn frame_unregister_event_callback(vm: &mut Vm, id: WidgetId, c: NativeCall) -> Ret {
    let event = string_arg(vm, &c, 2)?;
    unregister(vm, &event, Some(id), None);
    Ok(vec![])
}

/// Runs the callbacks registered for `event` (after the frames' handlers).
pub fn run_callbacks(vm: &mut Vm, event: &str, args: &[Value]) {
    let unit = match args.first() {
        Some(Value::Str(s)) => Some(String::from_utf8_lossy(vm.str_bytes(*s)).into_owned()),
        _ => None,
    };
    let due: Vec<(Option<WidgetId>, Value)> = engine(vm)
        .world
        .callbacks
        .iter()
        .filter(|cb| cb.event == event && (cb.unit.is_none() || cb.unit == unit))
        .map(|cb| (cb.owner, cb.f))
        .collect();
    for (owner, f) in due {
        let mut call = vec![owner.map_or(Value::Nil, |o| {
            Value::Table(engine(vm).world.widgets[o.0].table)
        })];
        call.extend_from_slice(args);
        vm.call_from_engine(f, &call);
    }
}

// ----------------------------------------------------------------- CVars

/// A CVar's value as set (none: not set, its default the client's).
pub fn cvar_get(vm: &mut Vm, name: &str) -> Option<String> {
    engine(vm).world.cvars.get(name).map(str::to_owned)
}

pub(crate) fn cvar_set(vm: &mut Vm, name: &str, value: String) {
    engine_mut(vm).world.cvars.set(name, value);
}

fn value_text(vm: &mut Vm, v: Value) -> String {
    match v {
        Value::Str(s) => String::from_utf8_lossy(vm.str_bytes(s)).into_owned(),
        Value::Num(n) if n.fract() == 0.0 => format!("{}", n as i64),
        Value::Num(n) => n.to_string(),
        Value::Bool(b) => if b { "1" } else { "0" }.into(),
        _ => String::new(),
    }
}

fn get_cvar(vm: &mut Vm, c: NativeCall) -> Ret {
    let name = string_arg(vm, &c, 1)?;
    Ok(vec![cvar_get(vm, &name).map_or(Value::Nil, |v| vm.str(&v))])
}

/// `C_CVar.GetCVarDefault(name)`: the default, whatever the value now.
fn get_cvar_default(vm: &mut Vm, c: NativeCall) -> Ret {
    let name = string_arg(vm, &c, 1)?;
    let default = engine(vm)
        .world
        .cvar_defaults
        .iter()
        .find(|(k, _)| k.eq_ignore_ascii_case(&name))
        .map(|(_, v)| v.clone());
    Ok(vec![default.map_or(Value::Nil, |v| vm.str(&v))])
}

/// `ConsoleGetAllCommands()`: the CVars, as console commands of type
/// `Enum.ConsoleCommandType.Cvar` (the client's other commands, 74 on
/// Forever, are unknown here).
fn console_get_all_commands(vm: &mut Vm, _c: NativeCall) -> Ret {
    let names: Vec<String> = engine(vm)
        .world
        .cvar_defaults
        .iter()
        .map(|(k, _)| k.clone())
        .collect();
    let mut cvar_type = vm.get_global("Enum");
    for key in ["ConsoleCommandType", "Cvar"] {
        cvar_type = match cvar_type {
            Value::Table(t) => {
                let k = vm.str(key);
                vm.heap.table(t).get(k)
            }
            _ => Value::Nil,
        };
    }
    let list = vm.new_table();
    for (i, name) in names.iter().enumerate() {
        let t = vm.new_table();
        let fields = [
            ("command", vm.str(name)),
            ("help", vm.str("")),
            ("category", Value::Num(0.0)),
            ("commandType", cvar_type),
            ("scriptContents", vm.str("")),
            ("scriptParameters", vm.str("")),
        ];
        for (k, v) in fields {
            let k = vm.str(k);
            let _ = vm.heap.table_mut(t).set(k, v);
        }
        let _ = vm
            .heap
            .table_mut(list)
            .set(Value::Num((i + 1) as f64), Value::Table(t));
    }
    Ok(vec![Value::Table(list)])
}

fn get_cvar_bool(vm: &mut Vm, c: NativeCall) -> Ret {
    let name = string_arg(vm, &c, 1)?;
    Ok(vec![cvar_get(vm, &name).map_or(Value::Nil, |v| {
        Value::Bool(v != "0" && !v.is_empty())
    })])
}

fn register_cvar(vm: &mut Vm, c: NativeCall) -> Ret {
    let name = string_arg(vm, &c, 1)?;
    if cvar_get(vm, &name).is_none() {
        let v = value_text(vm, c.arg(2));
        cvar_set(vm, &name, v);
    }
    Ok(vec![])
}

/// A CVar's flags on camelot (`/stdtest engine`): (locked from the user,
/// secure, read-only).
fn cvar_flags(vm: &Vm, name: &str) -> (bool, bool, bool) {
    if engine(vm).flavor != "camelot" {
        return (false, false, false);
    }
    let has = |kind: &str| {
        include_str!("../data/camelot-cvar-flags.txt")
            .lines()
            .filter(|l| !l.starts_with('#'))
            .filter_map(|l| l.split_once('\t'))
            .any(|(k, n)| k == kind && n.eq_ignore_ascii_case(name))
    };
    (has("locked"), has("secure"), has("readOnly"))
}

/// `C_CVar.GetCVarInfo(name)`: value, default, stored on the server for the
/// account and the character (unknown here: false), locked from the user,
/// secure, read-only.
fn get_cvar_info(vm: &mut Vm, c: NativeCall) -> Ret {
    let name = string_arg(vm, &c, 1)?;
    let Some(value) = cvar_get(vm, &name) else {
        return Ok(vec![]);
    };
    let default = engine(vm)
        .world
        .cvar_defaults
        .iter()
        .find(|(k, _)| k.eq_ignore_ascii_case(&name))
        .map(|(_, v)| v.clone())
        .unwrap_or_default();
    let (locked, secure, read_only) = cvar_flags(vm, &name);
    Ok(vec![
        vm.str(&value),
        vm.str(&default),
        Value::Bool(false),
        Value::Bool(false),
        Value::Bool(locked),
        Value::Bool(secure),
        Value::Bool(read_only),
    ])
}

/// `SetCVar(name, value)`: a secure CVar is refused to addons in combat
/// (`ADDON_ACTION_FORBIDDEN`, recorded for `SoftTargetEnemy` set from
/// `/run`; out of combat allowed, the user's word: SuuUI sets the
/// nameplates' secure CVars then without a refusal), a read-only one not
/// changed (unverified: its result), one the
/// client does not have neither (recorded: Blizzard's talents frame sets
/// `heroTalentsCollapse`, which camelot lacks, and no `CVAR_UPDATE` comes;
/// unverified: the result).
///
/// No value sets "0" (the full probe, 1.60.1: ten switches read "0" after
/// `SetCVar(name)`; unverified: a CVar whose range lacks 0, the camera's
/// zoom factor kept its 2).
fn set_cvar(vm: &mut Vm, c: NativeCall) -> Ret {
    let name = string_arg(vm, &c, 1)?;
    let text = match c.arg(2) {
        Value::Nil => "0".to_string(),
        v => value_text(vm, v),
    };
    set_cvar_text(vm, &name, text, true)
}

/// Sets a CVar as `C_CVar.SetCVar` does: an unknown one ignored, a secure
/// one refused to tainted code in combat, a read-only one not set, else
/// set and, when `tell`, told (`CVAR_UPDATE`).
pub(crate) fn set_cvar_text(vm: &mut Vm, name: &str, text: String, tell: bool) -> Ret {
    let name = name.to_string();
    // Camelot's CVars are the client's (recorded); other flavors' unknown.
    let world = &engine(vm).world;
    let unknown = engine(vm).flavor == "camelot"
        && !world.cvars.has(&name)
        && !world
            .cvar_defaults
            .iter()
            .any(|(k, _)| k.eq_ignore_ascii_case(&name));
    if unknown {
        return Ok(vec![]);
    }
    let (_, secure, read_only) = cvar_flags(vm, &name);
    if secure && engine(vm).units.lockdown && crate::protection::tainted(vm) {
        let action = crate::protection::action_name(vm);
        crate::protection::refuse(vm, "ADDON_ACTION_FORBIDDEN", action);
        return Ok(vec![]);
    }
    if read_only {
        return Ok(vec![Value::Bool(false)]);
    }
    // The camera's zoom factor keeps a value below 1 or no number, the set
    // answered as done (the full probe, 1.60.1: "0", "" and "abc" left 2,
    // "1" and "1.5" taken; the upper bound unverified).
    if name.eq_ignore_ascii_case("cameraDistanceMaxZoomFactor")
        && text.trim().parse::<f64>().map_or(true, |n| n < 1.0)
    {
        return Ok(vec![Value::Bool(true)]);
    }
    // Set to what it is: not told (recorded twice: the world map's first
    // showing after a reload sets `miniWorldMap` to its value, no
    // `CVAR_UPDATE`; scenarios tracking and mapnav).
    let unchanged = cvar_get(vm, &name).is_some_and(|v| v == text);
    cvar_set(vm, &name, text.clone());
    if !tell || unchanged {
        return Ok(vec![Value::Bool(true)]);
    }
    let n = vm.str(&name);
    let v = vm.str(&text);
    widgets::fire_event(vm, "CVAR_UPDATE", &[n, v]);
    // A Graphics setting sets what it stands for (decision T561).
    crate::graphics::follow(vm, &name, &text);
    // The UI scale's CVars scale UIParent (decision T482; unverified:
    // whether `CVAR_UPDATE` comes before `UI_SCALE_CHANGED`).
    if name.eq_ignore_ascii_case("uiScale") || name.eq_ignore_ascii_case("useUiScale") {
        let screen = crate::screen_from_cvars(vm);
        crate::change_screen(vm, screen);
    }
    Ok(vec![Value::Bool(true)])
}

/// `setCVar(name, value)`: a CVar the client changed without telling (the
/// camera's pitch as it turned): no `CVAR_UPDATE`.
pub(crate) fn control_set_cvar(vm: &mut Vm, c: NativeCall) -> Ret {
    let name = string_arg(vm, &c, 1)?;
    let text = value_text(vm, c.arg(2));
    cvar_set(vm, &name, text);
    Ok(vec![])
}

// ---------------------------------------------------------------- others

/// `C_Texture.GetAtlasInfo(atlas)`: the client's atlas (its tables,
/// `measures::atlas`, decision T238): its size, file, texture coordinates,
/// tiling and raw size (`rawSize`, a `Vector2D`), the name as asked; an
/// atlas the client lacks, or the empty name, nothing (the full probe,
/// 1.60.1: "" and "x").
fn atlas_info(vm: &mut Vm, c: NativeCall) -> Ret {
    let name = vm.check_str(&c, 1)?;
    let Some(known) = crate::measures::atlas(vm, &String::from_utf8_lossy(vm.str_bytes(name)))
    else {
        return Ok(vec![]);
    };
    let width = known.width.unwrap_or(0.0);
    let height = known.height.unwrap_or(0.0);
    let info = known.info.as_ref();
    let [left, right, top, bottom] = info.map_or([0.0, 1.0, 0.0, 1.0], |i| i.coords);
    let t = vm.new_table();
    let set = |vm: &mut Vm, k: &str, v: Value| {
        let key = vm.str(k);
        let _ = vm.heap.table_mut(t).set(key, v);
    };
    set(vm, "elementName", Value::Str(name));
    for (k, v) in [
        ("width", width),
        ("height", height),
        ("leftTexCoord", left),
        ("rightTexCoord", right),
        ("topTexCoord", top),
        ("bottomTexCoord", bottom),
        ("file", info.map_or(0.0, |i| i.file)),
    ] {
        set(vm, k, Value::Num(v));
    }
    set(
        vm,
        "tilesHorizontally",
        Value::Bool(info.is_some_and(|i| i.tiles_horizontally)),
    );
    set(
        vm,
        "tilesVertically",
        Value::Bool(info.is_some_and(|i| i.tiles_vertically)),
    );
    if let Some((w, h)) = info.and_then(|i| i.raw) {
        let create = vm.get_global("CreateVector2D");
        if !create.is_nil()
            && let Ok(r) = vm.call(create, &[Value::Num(w), Value::Num(h)])
            && let Some(v) = r.first().copied()
        {
            set(vm, "rawSize", v);
        }
    }
    Ok(vec![Value::Table(t)])
}

/// `C_Texture.GetAtlasExists(atlas)`: whether the client has it (its
/// tables, decision T238; the full probe, 1.60.1: "" false).
fn atlas_exists(vm: &mut Vm, c: NativeCall) -> Ret {
    let known = match vm.hidden(c.arg(1)) {
        Value::Str(s) => {
            crate::measures::atlas(vm, &String::from_utf8_lossy(vm.str_bytes(s))).is_some()
        }
        _ => false,
    };
    Ok(vec![Value::Bool(known)])
}

/// `GetBuildInfo()`: the flavor's client version and interface (the build
/// number and date are not known: "0", "").
fn build_info(vm: &mut Vm, _c: NativeCall) -> Ret {
    let flavor = engine(vm).flavor.clone();
    let table = wowlua_flavors::table();
    let client = table.clients.iter().find(|c| c.flavor == flavor);
    let (version, interface) = client.map_or(("0.0.0".to_string(), 0), |c| {
        (c.version.clone(), c.interface)
    });
    let v = vm.str(&version);
    // Camelot's: the recorded client's build and date, then an empty text
    // and a space (the full probe, 1.60.1: "70450", "Oct 15 2026", 16001,
    // "", " ").
    if flavor == "camelot" {
        let (build, date) = (vm.str("70450"), vm.str("Oct 15 2026"));
        let (empty, space) = (vm.str(""), vm.str(" "));
        return Ok(vec![
            v,
            build,
            date,
            Value::Num(f64::from(interface)),
            empty,
            space,
        ]);
    }
    let build = vm.str("0");
    let date = vm.str("");
    let info = vm.str(&format!("{version}.0"));
    Ok(vec![
        v,
        build,
        date,
        Value::Num(f64::from(interface)),
        v,
        info,
    ])
}

/// `C_AddOns.IsAddOnLoaded(name)`: (loaded or loading, loaded), once
/// `ADDON_LOADED` fired for it.
fn is_addon_loaded(vm: &mut Vm, c: NativeCall) -> Ret {
    let name = match vm.hidden(c.arg(1)) {
        Value::Str(s) => String::from_utf8_lossy(vm.str_bytes(s)).to_lowercase(),
        // An index into the addons the client knows (`GetNumAddOns`).
        Value::Num(n) if n >= 1.0 => match engine(vm).world.bindings.addons.get(n as usize - 1) {
            Some(a) => a.name.to_lowercase(),
            None => return Ok(vec![Value::Bool(false), Value::Bool(false)]),
        },
        _ => return Ok(vec![Value::Bool(false), Value::Bool(false)]),
    };
    let loaded = engine(vm).world.loaded_addons.contains(&name);
    Ok(vec![Value::Bool(loaded), Value::Bool(loaded)])
}

/// `C_AddOns.LoadAddOn(name)`: a Blizzard addon of the loaded UI code
/// loads (true); without it, or for another addon, (false, "MISSING")
/// (unverified reason).
fn load_addon(vm: &mut Vm, c: NativeCall) -> Ret {
    let name = match vm.hidden(c.arg(1)) {
        Value::Str(s) => String::from_utf8_lossy(vm.str_bytes(s)).into_owned(),
        _ => String::new(),
    };
    let loaded =
        crate::bindings::load_defined(vm, &name).or_else(|| crate::ui::load_addon(vm, &name));
    match loaded {
        Some(true) => Ok(vec![Value::Bool(true), Value::Nil]),
        _ => Ok(vec![Value::Bool(false), vm.str("MISSING")]),
    }
}

/// `C_LFGList.GetPremadeGroupFinderStyle()`: camelot's group finder is the
/// vanilla style (the client loads Blizzard_GroupFinder_VanillaStyle,
/// `/stdtest engine`); other flavors' is unverified (the enum's first).
fn premade_group_finder_style(vm: &mut Vm, _c: NativeCall) -> Ret {
    let api = engine(vm).api;
    let wanted = if engine(vm).flavor == "camelot" {
        "Vanilla"
    } else {
        ""
    };
    let style = api
        .enums
        .iter()
        .find(|(n, _)| *n == "PremadeGroupFinderStyle")
        .and_then(|(_, fields)| {
            fields
                .iter()
                .find(|(f, _)| f == wanted)
                .or_else(|| fields.first())
                .map(|(_, v)| *v)
        })
        .unwrap_or(0.0);
    Ok(vec![Value::Num(style)])
}

/// `C_LFGInfo.IsGroupFinderEnabled()`: on camelot, yes (the client loads
/// its group finder at login, `/stdtest engine`); elsewhere unverified.
fn group_finder_enabled(vm: &mut Vm, _c: NativeCall) -> Ret {
    Ok(vec![Value::Bool(engine(vm).flavor == "camelot")])
}

fn is_logged_in(vm: &mut Vm, _c: NativeCall) -> Ret {
    Ok(vec![Value::Bool(engine(vm).world.logged_in)])
}

/// `GetCallstackHeight()`: how deep the Lua call stack is.
fn callstack_height(vm: &mut Vm, _c: NativeCall) -> Ret {
    Ok(vec![Value::Num(vm.call_depth() as f64)])
}

/// The functions with behaviour here.
pub fn functions() -> Vec<(&'static str, NativeFn)> {
    vec![
        ("GetCurrentEnvironment", current_environment),
        ("GetFrameMetatable", frame_metatable),
        ("GetButtonMetatable", button_metatable),
        ("GetTextureMetatable", texture_metatable),
        ("GetFontStringMetatable", font_string_metatable),
        ("GetEditBoxMetatable", edit_box_metatable),
        ("IsGMClient", falsy),
        // No modifier is held (the test controls none yet).
        ("addframetext", nothing),
        ("Mixin", mixin),
        ("CreateFromMixins", create_from_mixins),
        ("C_FunctionContainers.CreateCallback", same),
        ("AddSourceLocationExclude", nothing),
        ("GetCallstackHeight", callstack_height),
        ("RegisterEventCallback", register_event_callback),
        ("RegisterUnitEventCallback", register_unit_event_callback),
        ("UnregisterEventCallback", unregister_event_callback),
        ("UnregisterUnitEventCallback", unregister_event_callback),
        ("C_CVar.GetCVar", get_cvar),
        ("C_CVar.GetCVarBool", get_cvar_bool),
        ("C_CVar.GetCVarDefault", get_cvar_default),
        ("C_CVar.GetCVarInfo", get_cvar_info),
        ("ConsoleGetAllCommands", console_get_all_commands),
        ("C_CVar.RegisterCVar", register_cvar),
        ("C_CVar.SetCVar", set_cvar),
        ("C_UI.Reload", crate::login::reload_ui),
        ("C_Texture.GetAtlasInfo", atlas_info),
        ("C_Texture.GetAtlasExists", atlas_exists),
        ("C_InputInterfaceStyle.GetCurrentStyle", zero),
        ("C_Glue.IsOnGlueScreen", falsy),
        ("GetBuildInfo", build_info),
        ("IsLoggedIn", is_logged_in),
        ("C_AddOns.IsAddOnLoaded", is_addon_loaded),
        ("C_AddOns.LoadAddOn", load_addon),
        (
            "C_LFGList.GetPremadeGroupFinderStyle",
            premade_group_finder_style,
        ),
        ("C_LFGInfo.IsGroupFinderEnabled", group_finder_enabled),
    ]
}

/// Widget methods with behaviour here.
pub fn methods() -> Vec<(&'static str, widgets::MethodFn)> {
    vec![
        ("RegisterEventCallback", frame_register_event_callback),
        ("UnregisterEventCallback", frame_unregister_event_callback),
    ]
}
