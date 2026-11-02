//! What the client answers to calls about itself rather than the world
//! (`/stdtest engine`, `loadCalls`: functions Blizzard's code calls at
//! load, without arguments): camelot's recorded answers
//! (`data/camelot-load-calls.txt`), and neutral ones where the client
//! answered for the probed character, place or session (decision: a
//! fresh, plain world; tests set what they need).
//!
//! Unverified: the neutral answers' exact shape (`GetRestState` when not
//! rested, `GetInstanceInfo` outside any map, `GetZonePVPInfo` without a
//! zone). `GetFonts`' order: creation order here; the client's changes
//! between sessions (two runs of `/stdtest engine`).

use std::collections::HashMap;
use std::sync::OnceLock;

use wowlua_vm::stdlib::Ret;
use wowlua_vm::{NativeCall, NativeFn, Value, Vm};

use crate::{engine, engine_mut};

/// A recorded value: the probe's text of it.
#[derive(Clone, Debug, PartialEq)]
pub enum Rec {
    Nil,
    Bool(bool),
    Num(f64),
    Str(String),
    /// A method of the mixin the table was made with (`ColorMixin`).
    Func,
    /// Keys as the probe shows them (`1`, `r`), sorted as strings.
    Table(Vec<(String, Rec)>),
}

/// Parses one value at `s`, ending (for a string) before `stop`: `, ` at
/// the top, `,` or `}` in a table.
fn value(s: &str, top: bool) -> Option<(Rec, &str)> {
    if let Some(mut rest) = s.strip_prefix('{') {
        let mut entries = Vec::new();
        if let Some(r) = rest.strip_prefix('}') {
            return Some((Rec::Table(entries), r));
        }
        loop {
            let (key, r) = rest.split_once('=')?;
            let (v, r) = value(r, false)?;
            entries.push((key.to_string(), v));
            if let Some(r) = r.strip_prefix(',') {
                rest = r;
            } else {
                return Some((Rec::Table(entries), r.strip_prefix('}')?));
            }
        }
    }
    if let Some(r) = s.strip_prefix("function") {
        return Some((Rec::Func, r));
    }
    if let Some(r) = s.strip_prefix("nil nil") {
        return Some((Rec::Nil, r));
    }
    let (ty, r) = s.split_once(' ')?;
    let end = if top {
        r.find(", ").unwrap_or(r.len())
    } else {
        r.find([',', '}']).unwrap_or(r.len())
    };
    let (text, rest) = r.split_at(end);
    let v = match ty {
        "boolean" => Rec::Bool(text == "true"),
        "number" => Rec::Num(text.parse().ok()?),
        "string" => Rec::Str(text.to_string()),
        _ => return None,
    };
    Some((v, rest))
}

/// A call's results as the probe shows them: `2: number 0, boolean true`.
pub fn parse_results(text: &str) -> Option<Vec<Rec>> {
    let (count, mut rest) = text.split_once(": ")?;
    let count: usize = count.parse().ok()?;
    let mut out = Vec::new();
    while out.len() < count {
        let (v, r) = value(rest, true)?;
        out.push(v);
        rest = r.strip_prefix(", ").unwrap_or(r);
    }
    rest.is_empty().then_some(out)
}

/// The probe's text of results (the inverse of `parse_results`).
pub fn show_results(values: &[Rec]) -> String {
    fn show(v: &Rec, out: &mut String) {
        match v {
            Rec::Nil => out.push_str("nil nil"),
            Rec::Bool(b) => out.push_str(&format!("boolean {b}")),
            Rec::Num(n) => out.push_str(&format!("number {}", wowlua_vm::number::fmt_num(*n))),
            Rec::Str(s) => out.push_str(&format!("string {s}")),
            Rec::Func => out.push_str("function"),
            Rec::Table(entries) => {
                out.push('{');
                for (i, (k, v)) in entries.iter().enumerate() {
                    if i > 0 {
                        out.push(',');
                    }
                    out.push_str(k);
                    out.push('=');
                    show(v, out);
                }
                out.push('}');
            }
        }
    }
    let parts: Vec<String> = values
        .iter()
        .map(|v| {
            let mut s = String::new();
            show(v, &mut s);
            s
        })
        .collect();
    format!("{}: {}", values.len(), parts.join(", "))
}

fn recorded() -> &'static HashMap<String, Vec<Rec>> {
    static DATA: OnceLock<HashMap<String, Vec<Rec>>> = OnceLock::new();
    DATA.get_or_init(|| {
        include_str!("../data/camelot-load-calls.txt")
            .lines()
            .filter(|l| !l.starts_with('#'))
            .filter_map(|l| {
                let (name, results) = l.split_once('\t')?;
                Some((name.to_string(), parse_results(results)?))
            })
            .collect()
    })
}

/// The functions with recorded answers (camelot).
pub fn recorded_names(flavor: &str) -> Vec<&'static str> {
    if flavor != "camelot" {
        return Vec::new();
    }
    let mut names: Vec<&'static str> = recorded().keys().map(String::as_str).collect();
    names.sort_unstable();
    names
}

/// A recorded value in the VM: list keys as numbers; a table with methods
/// gets them from its mixin (`ColorMixin`, when Blizzard's code defined it).
fn build(vm: &mut Vm, v: &Rec) -> Value {
    match v {
        Rec::Nil | Rec::Func => Value::Nil,
        Rec::Bool(b) => Value::Bool(*b),
        Rec::Num(n) => Value::Num(*n),
        Rec::Str(s) => vm.str(s),
        Rec::Table(entries) => {
            let t = vm.new_table();
            let mut mixin = false;
            for (k, inner) in entries {
                if *inner == Rec::Func {
                    mixin = true;
                    continue;
                }
                let key = match k.parse::<u32>() {
                    Ok(i) if i.to_string() == *k => Value::Num(f64::from(i)),
                    _ => vm.str(k),
                };
                let value = build(vm, inner);
                let _ = vm.heap.table_mut(t).set(key, value);
            }
            if mixin && let Value::Table(m) = vm.get_global("ColorMixin") {
                let methods: Vec<(Value, Value)> = vm.heap.table(m).entries().collect();
                for (k, f) in methods {
                    let _ = vm.heap.table_mut(t).set(k, f);
                }
            }
            Value::Table(t)
        }
    }
}

/// A recorded function: its answers, whatever the arguments.
pub fn recorded_call(vm: &mut Vm, c: NativeCall) -> Ret {
    // Its name, or (documented) its index in the API.
    let name = match c.upvals.first().copied() {
        Some(Value::Str(s)) => String::from_utf8_lossy(vm.str_bytes(s)).into_owned(),
        Some(Value::Num(i)) => engine(vm).api.functions[i as usize].name.to_string(),
        _ => return Err(vm.error("a recorded function without its name")),
    };
    Ok(recorded_answer(vm, &name, &c))
}

/// The recorded answer to a call of `name`: this call's own (`/stdtest
/// engine answers`), else the answer recorded at load.
pub fn recorded_answer(vm: &mut Vm, name: &str, c: &NativeCall) -> Vec<Value> {
    if let Some(values) = crate::recorded::lookup(vm, name, c) {
        return values;
    }
    let values = recorded().get(name).cloned().unwrap_or_default();
    values.iter().map(|v| build(vm, v)).collect()
}

// ------------------------------------------------- the neutral world

fn no(_vm: &mut Vm, _c: NativeCall) -> Ret {
    Ok(vec![Value::Bool(false)])
}

fn empty_list(vm: &mut Vm, _c: NativeCall) -> Ret {
    Ok(vec![Value::Table(vm.new_table())])
}

fn zero(_vm: &mut Vm, _c: NativeCall) -> Ret {
    Ok(vec![Value::Num(0.0)])
}

/// No group: no members of any role.
fn group_member_counts(vm: &mut Vm, _c: NativeCall) -> Ret {
    let t = vm.new_table();
    // By role and by class, the flavor's classes in ID order (the full
    // probe: camelot's nine).
    let mut classes = crate::units::flavor_class_files(vm);
    classes.sort_by(|a, b| crate::units::class_id(a).total_cmp(&crate::units::class_id(b)));
    let roles = ["TANK", "HEALER", "DAMAGER", "NOROLE"].map(String::from);
    for role in roles.iter().chain(&classes) {
        let k = vm.str(role);
        let _ = vm.heap.table_mut(t).set(k, Value::Num(0.0));
    }
    Ok(vec![Value::Table(t)])
}

/// No role chosen for the group finder.
fn saved_roles(vm: &mut Vm, _c: NativeCall) -> Ret {
    let t = vm.new_table();
    for role in ["tank", "healer", "dps"] {
        let k = vm.str(role);
        let _ = vm.heap.table_mut(t).set(k, Value::Bool(false));
    }
    Ok(vec![Value::Table(t)])
}

fn four_zeros(_vm: &mut Vm, _c: NativeCall) -> Ret {
    Ok(vec![Value::Num(0.0); 4])
}

/// No traffic, no latency.
fn net_stats(_vm: &mut Vm, _c: NativeCall) -> Ret {
    Ok(vec![Value::Num(0.0); 4])
}

/// `GetMirrorTimerInfo(index)`: no timer runs (`"UNKNOWN"`, which
/// Blizzard's MirrorTimer.lua skips): seven values, the pause a number
/// (the full probe: "UNKNOWN", 0, 0, 0, 0, "", 0).
fn mirror_timer_info(vm: &mut Vm, _c: NativeCall) -> Ret {
    let unknown = vm.str("UNKNOWN");
    let empty = vm.str("");
    let zero = Value::Num(0.0);
    Ok(vec![unknown, zero, zero, zero, zero, empty, zero])
}

fn zeros(_vm: &mut Vm, _c: NativeCall) -> Ret {
    Ok(vec![Value::Num(0.0), Value::Num(0.0)])
}

/// The free slots of the backpack and the four bags (the full probe,
/// 1.60.1: 2, the backpack's two, its other bag full; unverified: the
/// reagent bag left out, which was full).
fn free_bag_slots(vm: &mut Vm, _c: NativeCall) -> Ret {
    let free: usize = engine(vm)
        .world
        .character
        .bags
        .range(0..=4)
        .map(|(_, b)| b.slots.saturating_sub(b.items.len()))
        .sum();
    Ok(vec![Value::Num(free as f64)])
}

/// `GetGameTime()`: the realm's hour and minute, the clock's local time
/// (decision T555; noon when a session starts).
fn game_time(vm: &mut Vm, _c: NativeCall) -> Ret {
    let minutes = crate::clock::time_of_day(vm) / 60.0;
    Ok(vec![
        Value::Num((minutes / 60.0).floor()),
        Value::Num((minutes % 60.0).floor()),
    ])
}

/// `C_Weather.GetCurrentWeather()`: the world's weather (decision T565),
/// its kind (`Enum.WeatherType`) and intensity; clear: kind 0 (unverified:
/// the game may leave the kind out).
fn current_weather(vm: &mut Vm, _c: NativeCall) -> Ret {
    let (kind, intensity) = engine(vm).world.weather;
    let t = vm.new_table();
    let (k, v) = (vm.str("type"), Value::Num(f64::from(kind)));
    let _ = vm.heap.table_mut(t).set(k, v);
    let (k, v) = (vm.str("intensity"), Value::Num(intensity));
    let _ = vm.heap.table_mut(t).set(k, v);
    Ok(vec![Value::Table(t)])
}

/// No zone: no PvP type, not a PvP subzone, no faction.
/// `C_Sound.PlaySound(kit, …)`: played, a handle (the engine plays no
/// sound; unverified: the handles' numbers).
fn play_sound(vm: &mut Vm, _c: NativeCall) -> Ret {
    let w = &mut engine_mut(vm).world;
    w.sound_handles += 1.0;
    Ok(vec![Value::Bool(true), Value::Num(w.sound_handles)])
}

/// The zone's PvP status (a city's: "friendly", false, its faction),
/// else nothing (recorded: Durotar's).
fn zone_pvp_info(vm: &mut Vm, _c: NativeCall) -> Ret {
    match engine(vm).world.character.position.pvp.clone() {
        Some((kind, ffa, faction)) => Ok(vec![vm.str(&kind), Value::Bool(ffa), vm.str(&faction)]),
        None => Ok(vec![]),
    }
}

/// The instance the player is in (`encounters.rs`); else the continent
/// the player is on (its name and ID; recorded after a Hearthstone:
/// "Kalimdor", 1), else outside any: the 11 results empty.
fn instance_info(vm: &mut Vm, _c: NativeCall) -> Ret {
    if let Some(info) = crate::encounters::instance_info(vm) {
        return Ok(info);
    }
    let (empty, none) = (vm.str(""), vm.str("none"));
    let zero = Value::Num(0.0);
    // Outside an instance: the continent the map lies on (the full probe:
    // "Kalimdor", instance 1, in the Barrens; Eastern Kingdoms' 0
    // unverified).
    let position = engine(vm).world.character.position.clone();
    let continent = position
        .map_id
        .and_then(|m| crate::maps::world_rect(vm, m as i64))
        .and_then(|(instance, ..)| match instance as i64 {
            0 => Some(("Eastern Kingdoms".to_string(), 0.0)),
            1 => Some(("Kalimdor".to_string(), 1.0)),
            _ => None,
        });
    let (name, id) = match position.instance.or(continent) {
        Some((n, id)) => (vm.str(&n), Value::Num(id)),
        None => (empty, zero),
    };
    Ok(vec![
        name,
        none,
        zero,
        empty,
        zero,
        zero,
        Value::Bool(false),
        id,
        zero,
        Value::Nil,
        Value::Bool(false),
    ])
}

/// `GetRealmName()`: the player's realm, as the world gives it (the
/// character's `realm`), else "Realm" (decision T537).
fn realm_name(vm: &mut Vm, _c: NativeCall) -> Ret {
    let realm = crate::social::player_realm_name(vm);
    Ok(vec![vm.str(&realm)])
}

/// `GetNormalizedRealmName()`: the player's realm without spaces and hyphens
/// (decision T537).
fn normalized_realm_name(vm: &mut Vm, _c: NativeCall) -> Ret {
    let realm = crate::social::player_realm(vm);
    Ok(vec![vm.str(&realm)])
}

/// `GetPhysicalScreenSize()`: the screen's pixels (decision T237).
fn physical_screen_size(vm: &mut Vm, _c: NativeCall) -> Ret {
    Ok(vec![
        Value::Num(engine(vm).screen.width),
        Value::Num(engine(vm).screen.height),
    ])
}

fn screen_height(vm: &mut Vm, _c: NativeCall) -> Ret {
    Ok(vec![Value::Num(engine(vm).screen.ui_parent_size().1)])
}

/// `SetPortraitTexture(texture, unit [, disableMasking])`,
/// `C_NamePlate.SetNamePlateSize(width, height)`: what the client draws,
/// which the engine does not (nothing to read back: unverified that the
/// portrait leaves the texture's file as it was).
fn not_drawn(_vm: &mut Vm, _c: NativeCall) -> Ret {
    Ok(vec![])
}

/// `SetPortraitTexture(texture, unit [, disableMasking])`: the client
/// draws the unit's head into the texture (a 3D render, not the
/// engine's). Screenshots draw the client's flat portrait of the unit's
/// race and sex (`Interface\CharacterFrame\TemporaryPortrait-<Sex>-<Race>`,
/// the client's own stand-in) in its place; what code reads of the
/// texture is unchanged (unverified in the client).
fn set_portrait_texture(vm: &mut Vm, c: NativeCall) -> Ret {
    let Some(texture) = crate::widgets::widget_of(vm, c.arg(1)) else {
        return Ok(vec![]);
    };
    let unit = c.arg(2);
    let race = vm.get_global("UnitRace");
    let sex = vm.get_global("UnitSex");
    let race_file = vm
        .pcall(race, &[unit])
        .ok()
        .and_then(|r| r.get(1).copied())
        .and_then(|v| match v {
            Value::Str(s) => Some(String::from_utf8_lossy(vm.str_bytes(s)).into_owned()),
            _ => None,
        });
    let female = vm
        .pcall(sex, &[unit])
        .ok()
        .and_then(|r| r.first().copied())
        .is_some_and(|v| matches!(v, Value::Num(n) if n == 3.0));
    let props = engine(vm).world.widgets[texture.0].props;
    let key = vm.str("\u{0}portrait");
    let value = match race_file {
        Some(race) => {
            let path = format!(
                "Interface\\CharacterFrame\\TemporaryPortrait-{}-{race}",
                if female { "Female" } else { "Male" }
            );
            vm.str(&path)
        }
        None => Value::Nil,
    };
    let _ = vm.heap.table_mut(props).set(key, value);
    // The unit's look now, for the renderer's 3D portrait.
    let look = match unit {
        Value::Str(s) => {
            let unit = String::from_utf8_lossy(vm.str_bytes(s)).into_owned();
            let l = crate::scene::look_of(vm, &unit);
            (l.race > 0).then(|| crate::scene::encode_look(&l))
        }
        _ => None,
    };
    let key = vm.str("\u{0}portraitdisplay");
    let _ = vm.heap.table_mut(props).set(key, Value::Nil);
    let key = vm.str("\u{0}portraitlook");
    let value = match look {
        Some(l) => vm.str(&l),
        None => Value::Nil,
    };
    let _ = vm.heap.table_mut(props).set(key, value);
    Ok(vec![])
}

/// `SetPortraitTextureFromCreatureDisplayID(texture, displayID)`: the
/// texture shows a portrait of that creature display, as the client draws
/// one (screenshots: its 3D model's face; without the installation a
/// question mark).
pub(crate) fn set_portrait_from_display(vm: &mut Vm, c: NativeCall) -> Ret {
    let Some(texture) = crate::widgets::widget_of(vm, c.arg(1)) else {
        return Ok(vec![]);
    };
    let Value::Num(display) = c.arg(2) else {
        return Ok(vec![]);
    };
    let props = engine(vm).world.widgets[texture.0].props;
    for (k, v) in [
        (
            "\u{0}portrait",
            vm.str("Interface\\Icons\\INV_Misc_QuestionMark"),
        ),
        ("\u{0}portraitlook", Value::Nil),
        ("\u{0}portraitdisplay", Value::Num(display)),
    ] {
        let key = vm.str(k);
        let _ = vm.heap.table_mut(props).set(key, v);
    }
    Ok(vec![])
}

/// `C_PlayerInteractionManager.IsInteractingWithNpcOfType(type)`: that
/// interaction is open (its `PLAYER_INTERACTION_MANAGER_FRAME_SHOW` told,
/// its hide not yet).
fn interacting_with(vm: &mut Vm, c: NativeCall) -> Ret {
    // An enumeration's value: a non-number reads as 0, `None`, which is
    // the type while no interaction is open (the full probe, 1.60.1: 0 and
    // "x" true, 1 to 57 false, nothing open; unverified with one open).
    let n = vm.tonumber(vm.hidden(c.arg(1))).unwrap_or(0.0).trunc();
    let interactions = &engine(vm).world.interactions;
    let open = if n == 0.0 {
        interactions.is_empty()
    } else {
        interactions.contains(&n.to_bits())
    };
    Ok(vec![Value::Bool(open)])
}

/// `C_VideoOptions.GetCurrentGameWindowSize(monitor, fullscreen)`: the
/// screen's size, a `Vector2DMixin` (decision T482; recorded 1920 by 1080
/// on the Mac, the screen's default).
fn current_game_window_size(vm: &mut Vm, _c: NativeCall) -> Ret {
    let screen = crate::engine(vm).screen;
    let size = crate::curves::mixin_table(
        vm,
        "Vector2DMixin",
        &[
            ("x", Value::Num(screen.width)),
            ("y", Value::Num(screen.height)),
        ],
    );
    Ok(vec![size])
}

/// `C_VideoOptions.SetGameWindowSize(x, y)`: the window takes that size,
/// as the Graphics settings' window size does (decision T482); 0 by 0 the
/// default (the recorded 1920 by 1080). Unverified: what the client does
/// with a size its monitor does not list.
fn set_game_window_size(vm: &mut Vm, c: NativeCall) -> Ret {
    let (Value::Num(x), Value::Num(y)) = (c.arg(1), c.arg(2)) else {
        return Ok(vec![]);
    };
    let mut screen = crate::engine(vm).screen;
    let default = crate::Screen::default();
    (screen.width, screen.height) = if x <= 0.0 || y <= 0.0 {
        (default.width, default.height)
    } else {
        (x.floor(), y.floor())
    };
    crate::change_screen(vm, screen);
    Ok(vec![])
}

/// `GetScreenWidth()`: UIParent's width, as `GetScreenHeight` its height.
fn screen_width(vm: &mut Vm, _c: NativeCall) -> Ret {
    Ok(vec![Value::Num(engine(vm).screen.ui_parent_size().0)])
}

/// The role `C_SpecializationInfo.GetSpecializationInfo(index)` answers
/// for the player's specialization at that index (its fifth value).
fn role_at(vm: &mut Vm, index: f64) -> Result<Value, wowlua_vm::LuaError> {
    let ns = vm.get_global("C_SpecializationInfo");
    let Value::Table(ns) = ns else {
        return Ok(Value::Nil);
    };
    let k = vm.str("GetSpecializationInfo");
    let f = vm.heap.table(ns).get(k);
    let info = vm.call(f, &[Value::Num(index)])?;
    Ok(info.get(4).copied().unwrap_or(Value::Nil))
}

/// The role of the player's (first) specialization, if it has one.
pub(crate) fn own_specialization_role(vm: &mut Vm) -> Option<String> {
    match role_at(vm, 1.0) {
        Ok(Value::Str(s)) => Some(String::from_utf8_lossy(vm.str_bytes(s)).into_owned()),
        _ => None,
    }
}

/// What `GetSpecializationRole(specIndex [, isInspect, isPet])` asks
/// about: nothing for an index past the class's specializations (the
/// full probe: -1, 0 and 2 nothing, 1 "DAMAGER"), an inspected unit's
/// role 0, a number (`(1, 1)`, `GetSpecializationRoleEnum` alike;
/// unverified why), a pet's none (unverified), else the player's.
enum SpecRole {
    Nothing,
    Inspected,
    Pet,
    Own(Value),
}

fn spec_role(vm: &mut Vm, c: &NativeCall) -> Result<SpecRole, wowlua_vm::LuaError> {
    let index = vm.tonumber(vm.hidden(c.arg(1))).unwrap_or(0.0).trunc();
    // The class's one specialization (`GetNumSpecializations()`, the full
    // probe: 1).
    if index != 1.0 {
        return Ok(SpecRole::Nothing);
    }
    if vm.hidden(c.arg(2)).truthy() {
        return Ok(SpecRole::Inspected);
    }
    if vm.hidden(c.arg(3)).truthy() {
        return Ok(SpecRole::Pet);
    }
    Ok(SpecRole::Own(role_at(vm, index)?))
}

/// `GetSpecializationRole(specIndex [, isInspect, isPet])`: the role
/// `C_SpecializationInfo.GetSpecializationInfo(specIndex)` answers (its
/// fifth value, recorded "DAMAGER" for the default warrior), as
/// [`spec_role`] tells. Unverified: that the client's answers agree for
/// every class.
fn specialization_role(vm: &mut Vm, c: NativeCall) -> Ret {
    Ok(match spec_role(vm, &c)? {
        SpecRole::Nothing => vec![],
        SpecRole::Inspected => vec![Value::Num(0.0)],
        SpecRole::Pet => vec![Value::Nil],
        SpecRole::Own(role) => vec![role],
    })
}

/// `GetSpecializationRoleEnum(specIndex [, isInspect, isPet])`: the role
/// as `Enum.LFGRole` (recorded 2 for 1), as `GetSpecializationRole`.
fn specialization_role_enum(vm: &mut Vm, c: NativeCall) -> Ret {
    Ok(match spec_role(vm, &c)? {
        SpecRole::Nothing => vec![],
        SpecRole::Inspected => vec![Value::Num(0.0)],
        SpecRole::Pet => vec![Value::Nil],
        SpecRole::Own(Value::Str(s)) => {
            let n = match vm.str_bytes(s) {
                b"TANK" => 0.0,
                b"HEALER" => 1.0,
                b"DAMAGER" => 2.0,
                _ => -1.0,
            };
            vec![Value::Num(n)]
        }
        SpecRole::Own(_) => vec![Value::Nil],
    })
}

/// The font objects made, in order: by name, else as `tostring` gives
/// them (the client lists unnamed ones so).
fn fonts(vm: &mut Vm, _c: NativeCall) -> Ret {
    let made: Vec<(Option<String>, Value)> = engine(vm)
        .world
        .widgets
        .iter()
        .filter(|w| w.class == "Font")
        .map(|w| (w.name.clone(), Value::Table(w.table)))
        .collect();
    let t = vm.new_table();
    for (i, (name, table)) in made.into_iter().enumerate() {
        let v = match name {
            Some(n) => vm.str(&n),
            None => vm.tostring(table).unwrap_or(Value::Nil),
        };
        let _ = vm.heap.table_mut(t).set(Value::Num((i + 1) as f64), v);
    }
    Ok(vec![Value::Table(t)])
}

/// Each item quality's color (`/stdtest engine`, `qualityColors`): red,
/// green, blue, alpha.
pub(crate) const QUALITY_COLORS: [[f64; 4]; 9] = [
    [0.61568629741669, 0.61568629741669, 0.61568629741669, 1.0],
    [1.0, 1.0, 1.0, 1.0],
    [0.11764706671238, 1.0, 0.0, 1.0],
    [0.0, 0.43921571969986, 0.8666667342186, 1.0],
    [0.63921570777893, 0.20784315466881, 0.93333339691162, 1.0],
    [1.0, 0.50196081399918, 0.0, 1.0],
    [0.90196084976196, 0.80000007152557, 0.50196081399918, 1.0],
    [0.0, 0.80000007152557, 1.0, 1.0],
    [0.0, 0.80000007152557, 1.0, 1.0],
];

/// `C_ColorOverrides.GetColorForQuality(quality)`: a `ColorMixin` color
/// (camelot's; an unknown quality gives nothing, unverified).
///
/// A string reads as quality 0 (the full probe, 1.60.1: "x" the poor gray).
fn color_for_quality(vm: &mut Vm, c: NativeCall) -> Ret {
    let q = match vm.hidden(c.arg(1)) {
        Value::Str(_) => 0,
        _ => vm.check_int(&c, 1)?,
    };
    let Some(rgba) = usize::try_from(q).ok().and_then(|q| QUALITY_COLORS.get(q)) else {
        return Ok(vec![Value::Nil]);
    };
    let fields: Vec<(String, Rec)> = ["a", "b", "g", "r"]
        .iter()
        .zip([rgba[3], rgba[2], rgba[1], rgba[0]])
        .map(|(k, v)| (k.to_string(), Rec::Num(v)))
        .chain(std::iter::once(("GetRGB".to_string(), Rec::Func)))
        .collect();
    Ok(vec![build(vm, &Rec::Table(fields))])
}

/// The flavor's colors, `(name, 0xAARRGGBB)` by name: its installed
/// client's `GlobalColor` table (`cargo xtask types gen`, decision T554).
fn colors_of(flavor: &str) -> Option<Vec<(&'static str, u32)>> {
    let text = match flavor {
        "camelot" => include_str!("../data/camelot-colors.txt"),
        "vanilla" => include_str!("../data/vanilla-colors.txt"),
        "tbc" => include_str!("../data/tbc-colors.txt"),
        "mists" => include_str!("../data/mists-colors.txt"),
        _ => return None,
    };
    Some(
        text.lines()
            .filter(|l| !l.starts_with('#'))
            .filter_map(|l| {
                let (name, argb) = l.split_once('\t')?;
                Some((name, u32::from_str_radix(argb, 16).ok()?))
            })
            .collect(),
    )
}

/// `C_UIColor.GetColors()`: the client's colors, `{ baseTag, color }` by
/// name, each a `ColorMixin` color of its bytes (as the client makes them
/// floats); a flavor without them answers as recorded.
fn ui_colors(vm: &mut Vm, c: NativeCall) -> Ret {
    let Some(entries) = color_entries(&engine(vm).flavor) else {
        return Ok(recorded_answer(vm, "C_UIColor.GetColors", &c));
    };
    let list = vm.new_table();
    for (i, entry) in entries.iter().enumerate() {
        let value = build(vm, entry);
        let _ = vm
            .heap
            .table_mut(list)
            .set(Value::Num((i + 1) as f64), value);
    }
    Ok(vec![Value::Table(list)])
}

/// `C_UIColor.GetColors`' entries for the flavor, as a recording shows them.
fn color_entries(flavor: &str) -> Option<Vec<Rec>> {
    let mut out = Vec::new();
    for (name, argb) in colors_of(flavor)? {
        // (the client's: the byte times 1/255, in floats)
        let byte = |shift: u32| {
            Rec::Num(f64::from(
                ((argb >> shift) & 0xFF) as f32 * (1.0f32 / 255.0),
            ))
        };
        let color = Rec::Table(vec![
            ("GetRGB".to_string(), Rec::Func),
            ("a".to_string(), byte(24)),
            ("b".to_string(), byte(0)),
            ("g".to_string(), byte(8)),
            ("r".to_string(), byte(16)),
        ]);
        out.push(Rec::Table(vec![
            ("baseTag".to_string(), Rec::Str(name.to_string())),
            ("color".to_string(), color),
        ]));
    }
    Some(out)
}

/// `GetGlobalEnvironment()`: the addons' globals, from secure code too.
fn global_environment(vm: &mut Vm, _c: NativeCall) -> Ret {
    Ok(vec![Value::Table(engine(vm).main_globals)])
}

/// `SwapToGlobalEnvironment()`: the calling code (a secure file) goes on
/// with the addons' globals (Blizzard_StoreUI's toggle, Blizzard_PingUI).
fn swap_to_global_environment(vm: &mut Vm, _c: NativeCall) -> Ret {
    let main = engine(vm).main_globals;
    vm.set_caller_env(1, main);
    Ok(vec![])
}

/// Inventory slot names: ID, empty slot texture, whether it checks relics
/// (`/stdtest engine`, `slotInfo`).
const INVENTORY_SLOTS: &[(&str, f64, f64, bool)] = &[
    ("AMMOSLOT", 0.0, 136520.0, false),
    ("HEADSLOT", 1.0, 136516.0, false),
    ("NECKSLOT", 2.0, 136519.0, false),
    ("SHOULDERSLOT", 3.0, 136526.0, false),
    ("SHIRTSLOT", 4.0, 136525.0, false),
    ("CHESTSLOT", 5.0, 136512.0, false),
    ("WAISTSLOT", 6.0, 136529.0, false),
    ("LEGSSLOT", 7.0, 136517.0, false),
    ("FEETSLOT", 8.0, 136513.0, false),
    ("WRISTSLOT", 9.0, 136530.0, false),
    ("HANDSSLOT", 10.0, 136515.0, false),
    ("FINGER0SLOT", 11.0, 136514.0, false),
    ("FINGER1SLOT", 12.0, 136514.0, false),
    ("TRINKET0SLOT", 13.0, 136528.0, false),
    ("TRINKET1SLOT", 14.0, 136528.0, false),
    ("BACKSLOT", 15.0, 136512.0, false),
    ("MAINHANDSLOT", 16.0, 136518.0, false),
    ("SECONDARYHANDSLOT", 17.0, 136524.0, false),
    ("RANGEDSLOT", 18.0, 136520.0, true),
    ("TABARDSLOT", 19.0, 136527.0, false),
    ("BAG0SLOT", 31.0, 4760248.0, false),
    ("BAG1SLOT", 32.0, 4760248.0, false),
    ("BAG2SLOT", 33.0, 4760248.0, false),
    ("BAG3SLOT", 34.0, 4760248.0, false),
    ("REAGENTBAG0SLOT", 35.0, 4760248.0, false),
];

/// `GetInventorySlotInfo(slotName)`: the slot's ID, its empty texture,
/// whether it checks relics; another name (the profession slots) as the
/// client answered it.
fn inventory_slot_info(vm: &mut Vm, c: NativeCall) -> Ret {
    let name = match c.arg(1) {
        Value::Str(s) => String::from_utf8_lossy(vm.str_bytes(s)).to_uppercase(),
        _ => return Ok(vec![]),
    };
    Ok(INVENTORY_SLOTS
        .iter()
        .find(|(n, ..)| *n == name)
        .map(|(_, id, texture, relic)| {
            vec![Value::Num(*id), Value::Num(*texture), Value::Bool(*relic)]
        })
        .unwrap_or_else(|| crate::recorded::client_answer(vm, &c)))
}

/// The client's transmog slot locations (`C_TransmogOutfitInfo.
/// GetAllSlotLocationInfo`, recorded): each `{slot, slotName, isSecondary,
/// …}`.
fn slot_locations() -> Vec<&'static Vec<(String, Rec)>> {
    match recorded()
        .get("C_TransmogOutfitInfo.GetAllSlotLocationInfo")
        .and_then(|r| r.first())
    {
        Some(Rec::Table(list)) => list
            .iter()
            .filter_map(|(_, v)| match v {
                Rec::Table(fields) => Some(fields),
                _ => None,
            })
            .collect(),
        _ => Vec::new(),
    }
}

fn rec_field<'a>(fields: &'a [(String, Rec)], key: &str) -> Option<&'a Rec> {
    fields.iter().find(|(k, _)| k == key).map(|(_, v)| v)
}

/// `C_TransmogOutfitInfo.GetTransmogOutfitSlotFromInventorySlot(slot)`
/// (0-based inventory slot): the primary transmog slot of that inventory
/// slot's name, else nothing. The client checks no argument (`/stdtest
/// argcheck`: a number for `'x'`, `{}`): a non-number counts as 0.
///
/// The slot taken from 1 past a 32-bit integer is refused with the usage
/// (the full probe, 1.60.1: 2147483647 refused, -1 nothing; unverified
/// why).
fn outfit_slot_from_inventory_slot(vm: &mut Vm, c: NativeCall) -> Ret {
    let inv = match vm.hidden(c.arg(1)) {
        Value::Num(n) => n.trunc(),
        _ => 0.0,
    } + 1.0;
    if inv > 2_147_483_647.0 {
        return Err(crate::args::refuse(
            vm,
            "C_TransmogOutfitInfo.GetTransmogOutfitSlotFromInventorySlot",
            1,
        ));
    }
    let Some((name, ..)) = INVENTORY_SLOTS.iter().find(|(_, id, ..)| *id == inv) else {
        return Ok(vec![]);
    };
    for loc in slot_locations() {
        let named = matches!(rec_field(loc, "slotName"), Some(Rec::Str(s)) if s == name);
        let primary = matches!(rec_field(loc, "isSecondary"), Some(Rec::Bool(false)));
        if named
            && primary
            && let Some(Rec::Num(slot)) = rec_field(loc, "slot")
        {
            return Ok(vec![Value::Num(*slot)]);
        }
    }
    Ok(vec![])
}

/// `C_TransmogOutfitInfo.GetLinkedSlotInfo(slot)`: a slot with a secondary
/// of the same name (shoulders): `{primarySlotInfo, secondarySlotInfo}`,
/// the same for either of the two, else nothing (the full probe, 1.60.1:
/// slots 1 and 2 alike; a string as slot 0, nothing).
fn linked_slot_info(vm: &mut Vm, c: NativeCall) -> Ret {
    let slot = match vm.hidden(c.arg(1)) {
        Value::Num(n) => n.trunc(),
        _ => 0.0,
    };
    let locs = slot_locations();
    let Some(at) = locs
        .iter()
        .find(|l| matches!(rec_field(l, "slot"), Some(Rec::Num(n)) if *n == slot))
    else {
        return Ok(vec![]);
    };
    let name = rec_field(at, "slotName").cloned();
    let Some(primary) = locs.iter().find(|l| {
        rec_field(l, "slotName").cloned() == name
            && matches!(rec_field(l, "isSecondary"), Some(Rec::Bool(false)))
    }) else {
        return Ok(vec![]);
    };
    let Some(secondary) = locs.iter().find(|l| {
        rec_field(l, "slotName").cloned() == name
            && matches!(rec_field(l, "isSecondary"), Some(Rec::Bool(true)))
    }) else {
        return Ok(vec![]);
    };
    let info = Rec::Table(vec![
        ("primarySlotInfo".into(), Rec::Table((*primary).clone())),
        ("secondarySlotInfo".into(), Rec::Table((*secondary).clone())),
    ]);
    Ok(vec![build(vm, &info)])
}

/// `C_TransmogOutfitInfo.GetSlotGroupInfo()`: the transmogrifier's three
/// columns of slots, each its primary slots' locations in slot order (the
/// head to the wrists, the hands to the feet, the weapons) and no illusion
/// slots (the full probe, 1.60.1).
fn slot_group_info(vm: &mut Vm, _c: NativeCall) -> Ret {
    let mut locs: Vec<(f64, &'static Vec<(String, Rec)>)> = slot_locations()
        .into_iter()
        .filter(|l| matches!(rec_field(l, "isSecondary"), Some(Rec::Bool(false))))
        .filter_map(|l| match rec_field(l, "slot") {
            Some(Rec::Num(n)) => Some((*n, l)),
            _ => None,
        })
        .collect();
    locs.sort_by(|a, b| a.0.total_cmp(&b.0));
    let mut groups: Vec<Rec> = Vec::new();
    for position in 0..3 {
        let in_group = |slot: f64| match position {
            0 => slot <= 7.0,
            1 => (8.0..=11.0).contains(&slot),
            _ => slot >= 12.0,
        };
        let slots: Vec<(String, Rec)> = locs
            .iter()
            .filter(|(slot, _)| in_group(*slot))
            .enumerate()
            .map(|(i, (_, l))| ((i + 1).to_string(), Rec::Table((*l).clone())))
            .collect();
        groups.push(Rec::Table(vec![
            ("appearanceSlotInfo".into(), Rec::Table(slots)),
            ("illusionSlotInfo".into(), Rec::Table(Vec::new())),
            ("position".into(), Rec::Num(f64::from(position))),
        ]));
    }
    let list = Rec::Table(
        groups
            .into_iter()
            .enumerate()
            .map(|(i, g)| ((i + 1).to_string(), g))
            .collect(),
    );
    Ok(vec![build(vm, &list)])
}

/// `C_Log.LogErrorMessage(message)`: the client's log of Lua errors
/// (Blizzard's error handler writes every error there): the engine's list
/// of Blizzard's errors, first line.
fn log_error_message(vm: &mut Vm, c: NativeCall) -> Ret {
    if let Value::Str(s) = vm.hidden(c.arg(1)) {
        let text = String::from_utf8_lossy(vm.str_bytes(s)).into_owned();
        let first = text
            .lines()
            .next()
            .unwrap_or("")
            .trim_start_matches("Lua Error: ");
        let ui = &mut crate::engine_mut(vm).world.ui;
        ui.errors.push(first.to_string());
        ui.error_stacks.push(text);
    }
    Ok(vec![])
}

/// The functions with behaviour here.
pub fn behaviours() -> Vec<(&'static str, NativeFn)> {
    vec![
        ("C_Log.LogErrorMessage", log_error_message),
        (
            "C_TransmogOutfitInfo.GetTransmogOutfitSlotFromInventorySlot",
            outfit_slot_from_inventory_slot,
        ),
        ("C_TransmogOutfitInfo.GetLinkedSlotInfo", linked_slot_info),
        ("C_TransmogOutfitInfo.GetSlotGroupInfo", slot_group_info),
        ("C_PaperDollInfo.GetInventorySlotInfo", inventory_slot_info),
        ("GetInventorySlotInfo", inventory_slot_info),
        ("GetGlobalEnvironment", global_environment),
        ("SwapToGlobalEnvironment", swap_to_global_environment),
        ("C_ColorOverrides.GetColorForQuality", color_for_quality),
        ("C_UIColor.GetColors", ui_colors),
        (
            "C_Container.CalculateTotalNumberOfFreeBagSlots",
            free_bag_slots,
        ),
        ("C_PvP.GetZonePVPInfo", zone_pvp_info),
        ("C_PetJournal.GetNumPets", zeros),
        ("C_VoiceChat.GetTtsVoices", empty_list),
        ("GetFonts", fonts),
        ("GetGameTime", game_time),
        ("C_Weather.GetCurrentWeather", current_weather),
        ("C_Sound.PlaySound", play_sound),
        ("GetInstanceInfo", instance_info),
        ("GetPhysicalScreenSize", physical_screen_size),
        ("GetRealmName", realm_name),
        // The same without spaces: the placeholder has none.
        ("GetNormalizedRealmName", normalized_realm_name),
        ("GetScreenHeight", screen_height),
        ("GetScreenWidth", screen_width),
        ("C_VideoOptions.SetGameWindowSize", set_game_window_size),
        (
            "C_VideoOptions.GetCurrentGameWindowSize",
            current_game_window_size,
        ),
        ("SetPortraitTexture", set_portrait_texture),
        ("C_NamePlate.SetNamePlateSize", not_drawn),
        ("GetSpecializationRole", specialization_role),
        ("GetSpecializationRoleEnum", specialization_role_enum),
        // The neutral world: no follower dungeon, keystone, house editor,
        // garrison, friend online or warband bank gold.
        ("C_LFGInfo.IsInLFGFollowerDungeon", no),
        ("C_ChallengeMode.IsChallengeModeActive", no),
        ("C_HouseEditor.IsHouseEditorActive", no),
        ("C_Garrison.IsOnGarrisonMap", no),
        ("C_FriendList.GetNumOnlineFriends", zero),
        ("C_Bank.FetchDepositedMoney", zero),
        // No item is new (`C_NewItems.IsNewItem`): nothing to clear. No
        // cast has a target the engine knows: no target's class.
        ("C_NewItems.ClearAll", not_drawn),
        // The bags' search: nothing the engine filters by (unverified:
        // the client's `INVENTORY_SEARCH_UPDATE`).
        ("C_Container.SetItemSearch", not_drawn),
        // The engine measures no addon's time (unverified: the profiler
        // disabled answers 0 too).
        ("C_AddOnProfiler.GetAddOnMetric", zero),
        ("UnitSpellTargetClass", not_drawn),
        // Keyboard and mouse.
        ("IsUsingGamepad", no),
        (
            "C_PlayerInteractionManager.IsInteractingWithNpcOfType",
            interacting_with,
        ),
        ("GetActiveLootRollIDs", empty_list),
        ("C_LFGList.GetApplications", empty_list),
        // No pet battle, no nameplates.
        ("C_PetBattles.GetNumPets", zero),
        // (Health, level and experience are the full probe's,
        // `leftovers2.rs`.)
        ("C_PetBattles.GetPower", zero),
        ("C_PetBattles.GetSpeed", zero),
        ("C_LFGListRoles.GetSavedRoles", saved_roles),
        ("GetGroupMemberCounts", group_member_counts),
        ("GetNetStats", net_stats),
        ("GetMirrorTimerInfo", mirror_timer_info),
        // The probed character's (`loadCalls`); the neutral world's 0.
        ("GetMaxBattlefieldID", zero),
        ("GetNumLanguages", crate::globalsinfo::get_num_languages),
        // The shapeshift forms are the character's stats (`stats.rs`,
        // `shapeshiftForms`: the paladin's two auras, as the client's
        // `probe.loadCalls` answered), none without a snapshot.
        ("BNGetNumFriends", four_zeros),
        ("C_MountJournal.GetNumMountsNeedingFanfare", zero),
        ("C_PetJournal.GetNumPetsNeedingFanfare", zero),
        ("GetSpellConfirmationPromptsInfo", empty_list),
        // Not a convention kiosk.
        ("Kiosk.IsEnabled", no),
        // No trade: Blizzard's money frames subtract it from GetMoney()
        // (without an error in the client).
        ("GetPlayerTradeMoney", zero),
        ("GetTargetTradeMoney", zero),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every recorded answer parses, and shows as the probe showed it.
    #[test]
    fn the_recorded_answers_read_back_as_recorded() {
        let mut lines = 0;
        for line in include_str!("../data/camelot-load-calls.txt")
            .lines()
            .filter(|l| !l.starts_with('#'))
        {
            let (name, text) = line.split_once('\t').unwrap();
            let values = parse_results(text).unwrap_or_else(|| panic!("{name} parses"));
            assert_eq!(show_results(&values), text, "{name}");
            lines += 1;
        }
        assert_eq!(recorded().len(), lines);
    }

    /// Camelot's colors from its table are the ones the client answered
    /// (recorded on 70425): names, order, and values to the float.
    #[test]
    fn the_table_s_colors_are_the_recorded_ones() {
        fn plain(r: &Rec) -> Rec {
            match r {
                Rec::Table(entries) => Rec::Table(
                    entries
                        .iter()
                        .filter(|(_, v)| *v != Rec::Func)
                        .map(|(k, v)| (k.clone(), plain(v)))
                        .collect(),
                ),
                // (the probe shows 14 digits)
                Rec::Num(n) => Rec::Str(format!("{n:.13e}")),
                other => other.clone(),
            }
        }
        let Some(Rec::Table(list)) = recorded()["C_UIColor.GetColors"].first().cloned() else {
            panic!("the recorded colors");
        };
        let mut theirs: Vec<(u32, Rec)> = list
            .into_iter()
            .map(|(k, v)| (k.parse().unwrap(), v))
            .collect();
        theirs.sort_by_key(|(k, _)| *k);
        // A color 70707's table added (until the battery is recorded again).
        let added = [Rec::Str("EMPTY_BAG_SLOT_EFFECT_COLOR".into())];
        let ours: Vec<Rec> = color_entries("camelot")
            .unwrap()
            .into_iter()
            .filter(|c| !matches!(c, Rec::Table(f) if added.contains(&f[0].1)))
            .collect();
        assert_eq!(ours.len(), theirs.len());
        // Its alpha: 0 in 70658's and 70707's table, 1 as recorded on 70425
        // (the build's change or a hotfix: tests/ingame/PROBES.md, "The
        // Camelot update").
        let changed = Rec::Str("COMPACT_UNIT_FRAME_FRIENDLY_HEALTH_COLOR".into());
        let mut differ = Vec::new();
        for (mine, (i, recorded)) in ours.iter().zip(&theirs) {
            if plain(mine) != plain(recorded) {
                differ.push(*i);
                let Rec::Table(fields) = mine else { panic!() };
                assert_eq!(fields[0].1, changed, "color {i}");
            }
        }
        assert_eq!(differ.len(), 1, "{differ:?}");
        assert!(color_entries("vanilla").is_some_and(|c| c.len() > 150));
    }

    #[test]
    fn results_text() {
        let r = parse_results("3: string , number 0, {1={a=boolean true,b=function}}").unwrap();
        assert_eq!(r[0], Rec::Str(String::new()));
        assert_eq!(r[1], Rec::Num(0.0));
        assert_eq!(
            r[2],
            Rec::Table(vec![(
                "1".into(),
                Rec::Table(vec![("a".into(), Rec::Bool(true)), ("b".into(), Rec::Func)])
            )])
        );
        assert_eq!(parse_results("0: ").unwrap(), vec![]);
    }
}
