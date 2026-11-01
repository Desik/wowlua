//! The engine's controls for tests (spec Tooling §16.1.4): what
//! `wowlua:mock` drives. A table of natives the test runner installs; addon
//! code never sees it.

use wowlua_vm::stdlib::Ret;
use wowlua_vm::{LuaError, NativeCall, Value, Vm};

use crate::{clock, engine, engine_mut, units, widgets};

/// `fireEvent(name, ...)`: fires an event the flavor knows, the payload
/// checked against its documentation (decision T202).
fn fire_event(vm: &mut Vm, c: NativeCall) -> Ret {
    let name = vm.check_str(&c, 1)?;
    let name = String::from_utf8_lossy(vm.str_bytes(name)).into_owned();
    if !engine(vm).api.events.contains(&name) {
        return Err(vm.error(&format!(
            "mock.fireEvent: \"{name}\" is not an event of this flavor"
        )));
    }
    let args: Vec<Value> = c.args.iter().skip(1).copied().collect();
    if let Some(doc) = engine(vm).api.event_docs.get(&name).cloned() {
        check_payload(vm, &name, &doc, &args)?;
    }
    // The payload as the client's: made by the client, not the test (a
    // plain table's fields written securely, decision T239).
    let args: Vec<Value> = args.into_iter().map(|v| client_made(vm, v, 0)).collect();
    widgets::fire_event(vm, &name, &args);
    Ok(vec![])
}

/// A payload value as the client makes it: a plain table (no metatable, so
/// not a frame or a mixin's object) copied with every field written
/// securely, as C code writes them; anything else as it is. A test's table
/// keeps the test's taint in its fields, which Blizzard's handlers would
/// read and be tainted by, where the client's never taints them.
fn client_made(vm: &mut Vm, v: Value, depth: usize) -> Value {
    let Value::Table(t) = v else {
        return v;
    };
    if depth > 8 || vm.heap.table(t).meta.is_some() {
        return v;
    }
    let out = vm.new_table();
    let entries: Vec<(Value, Value)> = vm.heap.table(t).entries().collect();
    for (k, field) in entries {
        let field = client_made(vm, field, depth + 1);
        let _ = vm.heap.table_mut(out).set(k, field);
    }
    Value::Table(out)
}

/// The payload's count and types against the event's documentation.
fn check_payload(
    vm: &mut Vm,
    name: &str,
    doc: &serde_json::Value,
    args: &[Value],
) -> Result<(), wowlua_vm::LuaError> {
    let payload = doc
        .get("Payload")
        .and_then(serde_json::Value::as_array)
        .cloned()
        .unwrap_or_default();
    if args.len() > payload.len() {
        return Err(vm.error(&format!(
            "mock.fireEvent: {name} has {} payload values, {} given",
            payload.len(),
            args.len()
        )));
    }
    for (i, p) in payload.iter().enumerate() {
        let field = p
            .get("Name")
            .and_then(serde_json::Value::as_str)
            .unwrap_or("?");
        let nilable = p
            .get("Nilable")
            .and_then(serde_json::Value::as_bool)
            .unwrap_or(false);
        let ty = p
            .get("Type")
            .and_then(serde_json::Value::as_str)
            .unwrap_or("");
        let v = args.get(i).copied().unwrap_or(Value::Nil);
        let h = vm.hidden(v);
        if h.is_nil() {
            if !nilable {
                return Err(vm.error(&format!("mock.fireEvent: {name} needs `{field}` ({ty})")));
            }
            continue;
        }
        let api = &engine(vm).api;
        let expected = match ty {
            "bool" => Some("boolean"),
            "cstring" | "string" | "UnitToken" | "WOWGUID" => Some("string"),
            "number" | "luaIndex" | "fileID" | "time_t" | "BigInteger" | "BigUInteger"
            | "WOWMONEY" => Some("number"),
            "table" => Some("table"),
            // An enumeration's value is its number (`BAG_UPDATE`'s
            // `BagIndex`); a structure is a table.
            _ if api.enum_names.contains(ty) => Some("number"),
            _ if api.structures.contains(ty) => Some("table"),
            _ => None,
        };
        if let Some(e) = expected
            && h.type_name() != e
        {
            return Err(vm.error(&format!(
                "mock.fireEvent: {name}'s `{field}` is {ty}, got {}",
                h.type_name()
            )));
        }
    }
    Ok(())
}

/// `sameSecret(a, b)`: whether two secrets hold the same value (the test
/// runner's matchers; addon code cannot look). Strings, numbers and booleans
/// by value, the rest by identity.
fn same_secret(vm: &mut Vm, c: NativeCall) -> Ret {
    let a = vm.hidden(c.arg(1));
    let b = vm.hidden(c.arg(2));
    Ok(vec![Value::Bool(a.raw_eq(b))])
}

/// `keepField(t, key)`: a function that puts `t[key]` back as it is now,
/// its taint included (`mock.stub` and `mock.fake` restore with it: the
/// test's own write would leave a global the client set tainted by the
/// addon, and Blizzard's code reading it then runs tainted).
fn keep_field(vm: &mut Vm, c: NativeCall) -> Ret {
    let Value::Table(t) = c.arg(1) else {
        return Err(vm.error("keepField: a table expected"));
    };
    let key = c.arg(2);
    let kept = vm.heap.table(t).get_slot(key);
    let restore = vm.native_slots(
        "restore",
        restore_field,
        vec![
            wowlua_vm::taint::Slot::secure(Value::Table(t)),
            wowlua_vm::taint::Slot::secure(key),
            kept,
        ],
    );
    Ok(vec![restore])
}

fn restore_field(vm: &mut Vm, c: NativeCall) -> Ret {
    let Value::Table(t) = c.upval_slot(0).v else {
        return Ok(vec![]);
    };
    let key = c.upval_slot(1).v;
    let kept = c.upval_slot(2);
    if vm.heap.table_mut(t).set_slot(key, kept).is_err() {
        return Err(vm.error("mock: cannot restore a field of a frozen table"));
    }
    Ok(vec![])
}

/// `fireUnitEvent(name, unit, ...)`: an event for a unit.
fn fire_unit_event(vm: &mut Vm, c: NativeCall) -> Ret {
    fire_event(vm, c)
}

fn advance_time(vm: &mut Vm, c: NativeCall) -> Ret {
    let s = vm.opt_number(&c, 1, 0.0)?;
    clock::advance(vm, s);
    Ok(vec![])
}

fn set_time_of_day(vm: &mut Vm, c: NativeCall) -> Ret {
    let hour = vm.check_number(&c, 1)?;
    let minute = vm.opt_number(&c, 2, 0.0)?;
    clock::set_time_of_day(vm, hour * 3600.0 + minute * 60.0);
    Ok(vec![])
}

fn next_frame(vm: &mut Vm, c: NativeCall) -> Ret {
    let ms = vm.opt_number(&c, 1, 1000.0 / 60.0)?;
    clock::advance_frames(vm, 1, ms);
    Ok(vec![])
}

fn advance_frames(vm: &mut Vm, c: NativeCall) -> Ret {
    let n = vm.check_int(&c, 1)?;
    let ms = vm.opt_number(&c, 2, 1000.0 / 60.0)?;
    clock::advance_frames(vm, n.max(0) as usize, ms);
    Ok(vec![])
}

/// `spend(ms)`: work that takes `ms` milliseconds (`debugprofilestop`
/// moves; no timers run).
fn spend(vm: &mut Vm, c: NativeCall) -> Ret {
    let ms = vm.check_number(&c, 1)?;
    vm.clock.elapsed_ms += ms.max(0.0);
    Ok(vec![])
}

/// `frames()`: every frame, in creation order (UIParent and the frames
/// `CreateFrame` made; not textures or font strings).
fn frames(vm: &mut Vm, _c: NativeCall) -> Ret {
    let e = engine(vm);
    let list: Vec<Value> = e
        .world
        .widgets
        .iter()
        .filter(|w| widgets::is_a(e, &w.class, "Frame"))
        .map(|w| Value::Table(w.table))
        .collect();
    let t = vm.new_table();
    for (i, f) in list.into_iter().enumerate() {
        let _ = vm.heap.table_mut(t).set(Value::Num(i as f64 + 1.0), f);
    }
    Ok(vec![Value::Table(t)])
}

fn widget_arg(vm: &mut Vm, c: &NativeCall, n: usize) -> Result<widgets::WidgetId, LuaError> {
    widgets::widget_of(vm, c.arg(n))
        .ok_or_else(|| vm.error(&format!("bad argument #{n} (a frame expected)")))
}

/// `script(frame, name, ...)`: runs a frame's script and hooks as the client
/// does (a user's action: a click, a key in an edit box).
fn script(vm: &mut Vm, c: NativeCall) -> Ret {
    let id = widget_arg(vm, &c, 1)?;
    let name = vm.check_str(&c, 2)?;
    let name = String::from_utf8_lossy(vm.str_bytes(name)).into_owned();
    let args: Vec<Value> = c.args.iter().skip(2).copied().collect();
    widgets::run_script(vm, id, &name, &args);
    Ok(vec![])
}

/// `drag(frame, dx, dy)`: drags a frame by (dx, dy) screen units.
fn drag(vm: &mut Vm, c: NativeCall) -> Ret {
    let id = widget_arg(vm, &c, 1)?;
    let dx = vm.check_number(&c, 2)?;
    let dy = vm.check_number(&c, 3)?;
    let frames = vm.opt_number(&c, 4, 0.0)?.max(0.0) as usize;
    widgets::drag(vm, id, dx, dy, frames);
    Ok(vec![])
}

/// `dragTo(source, target)`: the user drags `source` onto `target`: the
/// source's `OnDragStart("LeftButton")` (picking something up), then on
/// release its `OnDragStop` and, with something on the cursor, the
/// target's `OnReceiveDrag`. Unverified: the release's order.
pub fn drag_to(vm: &mut Vm, c: NativeCall) -> Ret {
    let from = widget_arg(vm, &c, 1)?;
    let to = widget_arg(vm, &c, 2)?;
    crate::input::as_user(vm, |vm| {
        let left = vm.str("LeftButton");
        widgets::run_script(vm, from, "OnDragStart", &[left]);
    });
    crate::input::as_user(vm, |vm| {
        widgets::run_script(vm, from, "OnDragStop", &[]);
        if !matches!(
            engine(vm).world.character.cursor,
            crate::cursor::Cursor::Empty
        ) {
            widgets::run_script(vm, to, "OnReceiveDrag", &[]);
        }
    });
    Ok(vec![])
}

/// `sizeBy(grip, dw, dh)`: drags a resize grip by (dw, dh) screen units.
fn size_by(vm: &mut Vm, c: NativeCall) -> Ret {
    let id = widget_arg(vm, &c, 1)?;
    let dw = vm.check_number(&c, 2)?;
    let dh = vm.check_number(&c, 3)?;
    widgets::size_by(vm, id, dw, dh);
    Ok(vec![])
}

/// `wheel(frame, delta)`: turns the mouse wheel over a frame.
/// `setLatency(seconds)`: the server's round trip from now on.
fn set_latency(vm: &mut Vm, c: NativeCall) -> Ret {
    let s = vm.check_number(&c, 1)?;
    crate::engine_mut(vm).world.latency = Some(s.max(0.0));
    Ok(vec![])
}

/// `snapshotTaken()`: the profile's snapshot was taken now (its
/// cooldowns' time left counts from here).
fn snapshot_taken(vm: &mut Vm, _c: NativeCall) -> Ret {
    let now = crate::clock::now(vm);
    crate::engine_mut(vm).world.snapshot_at = Some(now);
    Ok(vec![])
}

/// `loggedOut()`: whether the session ended (`ReloadUI`, the addon's or
/// Blizzard's): the test runner reloads.
fn logged_out(vm: &mut Vm, _c: NativeCall) -> Ret {
    Ok(vec![Value::Bool(crate::engine(vm).world.logged_out)])
}

fn wheel(vm: &mut Vm, c: NativeCall) -> Ret {
    let id = widget_arg(vm, &c, 1)?;
    let delta = vm.check_number(&c, 2)?;
    widgets::run_script(vm, id, "OnMouseWheel", &[Value::Num(delta)]);
    Ok(vec![])
}

/// `defineTemplate(name, fn)`: `CreateFrame(type, name, parent, template)`
/// calls `fn(frame)` (a stand-in for a template of Blizzard's UI code).
fn define_template(vm: &mut Vm, c: NativeCall) -> Ret {
    let name = vm.check_str(&c, 1)?;
    let name = String::from_utf8_lossy(vm.str_bytes(name)).into_owned();
    let f = c.arg(2);
    widgets::keep(vm, f);
    engine_mut(vm).world.templates.insert(name, f);
    Ok(vec![])
}

/// `defineFrameType(name, base, fn)`: `CreateFrame(name, …)` makes a `base`
/// frame and calls `fn(frame)` (a frame type the flavor's documentation
/// lacks); for a documented type, `fn` runs after it is made.
fn define_frame_type(vm: &mut Vm, c: NativeCall) -> Ret {
    let name = vm.check_str(&c, 1)?;
    let name = String::from_utf8_lossy(vm.str_bytes(name)).into_owned();
    let base = vm.check_str(&c, 2)?;
    let base = String::from_utf8_lossy(vm.str_bytes(base)).into_owned();
    let f = c.arg(3);
    widgets::keep(vm, f);
    engine_mut(vm).world.frame_types.insert(name, (base, f));
    Ok(vec![])
}

/// `protect(frame [, on])`: makes a frame protected (as a secure template
/// would).
fn protect(vm: &mut Vm, c: NativeCall) -> Ret {
    let id = widget_arg(vm, &c, 1)?;
    let on = !c.has(2) || c.arg(2).truthy();
    let w = &mut engine_mut(vm).world.widgets[id.0];
    w.protected = on;
    w.explicitly_protected = on;
    Ok(vec![])
}

fn warnings(vm: &mut Vm, _c: NativeCall) -> Ret {
    let list = engine(vm).world.warnings.clone();
    let t = vm.new_table();
    for (i, w) in list.iter().enumerate() {
        let s = vm.str(w);
        let _ = vm.heap.table_mut(t).set(Value::Num(i as f64 + 1.0), s);
    }
    Ok(vec![Value::Table(t)])
}

/// Installs the controls as global `name` (tests of the engine itself).
pub fn install(vm: &mut Vm, name: &str) {
    let t = table(vm);
    vm.set_global(name, Value::Table(t));
}

/// The controls as a table (the test runner gives it to `wowlua:mock`).
pub fn table(vm: &mut Vm) -> wowlua_vm::TableRef {
    let t = vm.new_table();
    for (key, f) in [
        ("fireEvent", fire_event as wowlua_vm::NativeFn),
        ("fireUnitEvent", fire_unit_event),
        ("sameSecret", same_secret),
        ("keepField", keep_field),
        ("advanceTime", advance_time),
        ("setTimeOfDay", set_time_of_day),
        ("setLatency", set_latency),
        ("snapshotTaken", snapshot_taken),
        ("loggedOut", logged_out),
        ("setFrameBatching", crate::batch::control_set),
        ("nextFrame", next_frame),
        ("advanceFrames", advance_frames),
        ("warnings", warnings),
        ("frames", frames),
        ("script", script),
        ("drag", drag),
        ("dragTo", drag_to),
        ("sizeBy", size_by),
        ("wheel", wheel),
        ("defineTemplate", define_template),
        ("defineFrameType", define_frame_type),
        ("protect", protect),
        ("refusals", crate::protection::refusals_control),
        ("slash", crate::chat::slash_control),
        ("receiveAddonMessage", crate::chat::receive_control),
        ("sentAddonMessages", crate::chat::sent_control),
        ("joinGuild", crate::guild::control_join),
        ("leaveGuild", crate::guild::control_leave),
        ("defineGuildMember", crate::guild::control_define_member),
        ("updateGuildMember", crate::guild::control_update_member),
        ("removeGuildMember", crate::guild::control_remove_member),
        ("systemMessage", crate::chat::system_message_control),
        ("defineBinding", crate::bindings::define_binding),
        ("pressBinding", crate::bindings::press_binding),
        ("pressKey", crate::bindings::press_key),
        ("defineAddon", crate::bindings::define_addon),
        ("defineSpell", crate::gamedata::define_spell),
        ("defineItem", crate::gamedata::define_item),
        ("defineQuest", crate::quests::control_define),
        ("defineQuestMap", crate::quests::control_define_map),
        ("setBagOrder", crate::cursor::control_set_bag_order),
        (
            "setItemDataDelay",
            crate::cursor::control_set_item_data_delay,
        ),
        ("setSortResult", crate::cursor::control_set_sort_result),
        ("defineGossip", crate::gossip::control_define),
        ("defineTrainer", crate::trainer::control_define),
        ("defineTaxi", crate::taxi::control_define),
        ("defineMailbox", crate::mail::control_define),
        ("setNewMail", crate::mail::control_set_new_mail),
        ("setEquipmentSets", crate::equipsets::control_set),
        ("defineBank", crate::bank::control_define),
        ("defineAuctionHouse", crate::auction::control_define),
        ("nearCorpse", crate::death::control_near_corpse),
        ("gameError", game_error),
        ("tellAuraStates", crate::aurastates::control_tell),
        ("tellTurns", crate::batch::control_tell_turns),
        ("beforeEvent", crate::batch::control_before_event),
        ("setCVar", crate::framescript::control_set_cvar),
        ("clickWorld", crate::inventory::control_click_world),
        (
            "setHiddenGroupBuffs",
            crate::character::control_set_hidden_group_buffs,
        ),
        ("setInventoryLoad", crate::login::control_set_inventory_load),
        ("defineLevel", crate::levels::control_define),
        ("setSpellTraits", crate::actions::control_set_spell_traits),
        ("setTraitRanks", crate::traits::control_set_trait_ranks),
        (
            "setTraitCharacter",
            crate::traits::control_set_trait_character,
        ),
        ("setRenown", crate::traits::control_set_renown),
        ("newWmoChunk", crate::character::control_new_wmo_chunk),
        ("setResting", crate::character::control_set_resting),
        ("setHardcore", crate::classic::control_set_hardcore),
        ("areaChanged", crate::character::control_area_changed),
        ("spend", spend),
        ("spawnUnit", units::spawn),
        ("unitOf", units::control_unit_of),
        ("unitAt", units::control_unit_at),
        ("setPlayer", units::set_player),
        ("setBag", crate::character::control_set_bag),
        (
            "setItemCooldown",
            crate::character::control_set_item_cooldown,
        ),
        ("defaultedCalls", defaulted_calls),
        ("openMerchant", crate::merchant::control_open),
        ("defineVendor", crate::merchant::control_define),
        ("setAction", crate::actions::control_set_action),
        ("setChannels", crate::channels::control_set_channels),
        ("setMacros", crate::macros::control_set_macros),
        ("setMounts", crate::collected::control_set_mounts),
        ("setToys", crate::collected::control_set_toys),
        ("setAppearances", crate::collected::control_set_appearances),
        ("defineLFGListing", crate::lfg::control_define),
        ("updateLFGListing", crate::lfg::control_update),
        ("removeLFGListing", crate::lfg::control_remove),
        ("lfgSearchResultID", crate::lfg::control_search_result_id),
        ("failLFGSearch", crate::lfg::control_fail_search),
        ("postLFGListing", crate::lfg::control_post),
        ("delistLFG", crate::lfg::control_delist),
        ("closeMerchant", crate::merchant::control_close),
        ("sell", crate::merchant::control_sell),
        ("moveTo", crate::character::control_move_to),
        ("hover", crate::input::control_hover),
        ("click", crate::input::control_click),
        ("slide", crate::input::control_slide),
        ("type", crate::input::control_type),
        ("setModifiers", crate::input::control_set_modifiers),
        (
            "setCursorPosition",
            crate::input::control_set_cursor_position,
        ),
        ("addToGroup", units::add_to_group),
        ("removeFromGroup", units::remove_from_group),
        ("leaveGroup", crate::party::control_leave_group),
        ("enterInstance", crate::encounters::control_enter_instance),
        ("leaveInstance", crate::encounters::control_leave_instance),
        ("startEncounter", crate::encounters::control_start),
        ("endEncounter", crate::encounters::control_end),
        ("setPet", crate::pets::control_set_pet),
        ("setRestriction", units::set_restriction_control),
        (
            "setDamageMeterSession",
            crate::damagemeter::control_set_session,
        ),
    ] {
        let native = vm.native(key, f);
        let k = vm.heap.intern(key.as_bytes());
        vm.heap.table_mut(t).set_str(k, native);
    }
    let player = units::handle(vm, units::PLAYER);
    let k = vm.heap.intern(b"player");
    vm.heap.table_mut(t).set_str(k, player);
    t
}

/// `defaultedCalls()`: the calls secure code made that the engine had no
/// answer for, sorted (`name(args)`).
fn defaulted_calls(vm: &mut Vm, _c: NativeCall) -> Ret {
    let list: Vec<String> = crate::engine(vm).defaulted_calls.iter().cloned().collect();
    let t = vm.new_table();
    for (i, call) in list.iter().enumerate() {
        let v = vm.str(call);
        let _ = vm.heap.table_mut(t).set(Value::Num(i as f64 + 1.0), v);
    }
    Ok(vec![Value::Table(t)])
}

/// `gameError(name)`: the client's error message `name` (`"BADATTACKFACING"`:
/// `UI_ERROR_MESSAGE(LE_GAME_ERR_BADATTACKFACING, ERR_BADATTACKFACING)`, as
/// recorded for an auto attack facing away).
fn game_error(vm: &mut Vm, c: NativeCall) -> Ret {
    let name = match c.arg(1) {
        Value::Str(s) => String::from_utf8_lossy(vm.str_bytes(s)).into_owned(),
        _ => return Err(vm.error("gameError: an error's name expected")),
    };
    let code = vm.get_global(&format!("LE_GAME_ERR_{name}"));
    let text = vm.get_global(&format!("ERR_{name}"));
    if code.is_nil() || text.is_nil() {
        return Err(vm.error(&format!("gameError: no client error {name}")));
    }
    crate::widgets::fire_event(vm, "UI_ERROR_MESSAGE", &[code, text]);
    Ok(vec![])
}
