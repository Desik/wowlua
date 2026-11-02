//! Units casting spells (spec Tooling §16.1.4): `unit:castSpell(id, opts)`
//! and `unit:interruptCast()` drive `UnitCastingInfo`, `UnitChannelInfo`
//! and the `UNIT_SPELLCAST_*` events on the virtual clock.
//!
//! Verified against the client (`/stdtest engine record cast`, Forever
//! 70425): the player's `SENT(unit, target name | nil, castGUID, spellID)`;
//! `START`; then `SUCCEEDED` and `STOP`, or `INTERRUPTED(unit, castGUID,
//! spellID, nil, castBarID)` and `STOP`; each with the cast's
//! `CastBar-<16 hex>` id last; `CURRENT_SPELL_CAST_CHANGED(false)` after the
//! player's start and stop; an instant cast's `SUCCEEDED(…, nil)` between
//! two of those, without `START` / `STOP`; a player's cast while casting `FAILED_QUIET` with
//! `UI_ERROR_MESSAGE(280, "Another action is in progress")`, the running
//! cast kept; channels' `CHANNEL_START(unit, nil, spellID, castBarID)` and
//! `CHANNEL_STOP(unit, nil, spellID, nil, castBarID)`; the cast GUID's
//! shape; other units' cast GUIDs and spell IDs secret, the player's
//! `SENT` target in combat. Unverified: the default cast time (1.5 s), an interrupted
//! channel's `INTERRUPTED`, other units' casts replacing a running one.
//! The player's interrupted cast is answered 0.4 s later (recorded for an
//! interrupt by moving; unverified for other interrupts).

use std::collections::HashMap;

use wowlua_vm::stdlib::Ret;
use wowlua_vm::{NativeCall, NativeFn, TableRef, Value, Vm};

use crate::units::{self, UnitId};
use crate::{engine, engine_mut};

#[derive(Clone, Debug)]
pub struct Cast {
    pub spell_id: f64,
    pub name: Value,
    pub icon: Value,
    /// Start and end, in the clock's seconds.
    pub start: f64,
    pub end: f64,
    pub channel: bool,
    pub not_interruptible: bool,
    /// A craft from a trade skill window (`UnitCastingInfo`'s sixth).
    pub trade_skill: bool,
    pub guid: String,
    pub bar: String,
    /// What follows its success (a gathered object's loot).
    pub done: Option<Value>,
    /// Whom it is cast on (the 3D world's spell effects: decision T582).
    pub target: Option<UnitId>,
}

/// A spell gone off (decision T582): the spell, its caster, whom it is
/// cast on, when (the clock's).
#[derive(Clone, Debug, PartialEq)]
pub struct Gone {
    pub spell_id: f64,
    pub caster: UnitId,
    pub target: Option<UnitId>,
    pub at: f64,
}

/// How long a spell gone off is kept for the 3D world (seconds): its cast
/// flash, missile and impact played within.
const GONE_KEPT: f64 = 10.0;

#[derive(Default)]
pub struct Casting {
    pub casts: HashMap<UnitId, Cast>,
    pub next: u64,
    /// The next cast's success this long after its start, not at its end
    /// (a recording's: the server's answer).
    pub succeeds_after: Option<f64>,
    /// The next cast a craft's (`mark_craft`).
    pub crafting: bool,
    /// The player's spell for the next swing (Heroic Strike), taken by the
    /// server: its ID and cast GUID.
    pub next_swing: Option<(f64, String)>,
    /// The player's spells on cooldown: when each is ready again (the
    /// clock's), from its data's `cooldown` (seconds).
    pub ready_at: HashMap<i64, f64>,
    /// A food's cooldown category on its short cooldown since its use:
    /// the category, the start (the clock's), the seconds.
    pub category_cooldown: Option<(f64, f64, f64)>,
    /// The player's presses the server has not answered yet, in order:
    /// the spell and the client's cast GUID for it.
    pub pending: Vec<(f64, String)>,
    /// Whether the player is moving (`player:setMoving`).
    pub moving: bool,
    /// The spells gone off in the last seconds (decision T582).
    pub gone: Vec<Gone>,
}

/// Whom a unit's spell is cast on (decision T582): the unit named, else
/// the caster's target, else the caster; the caster for a spell without a
/// target (its data's `targeted = false`).
fn aim(vm: &mut Vm, me: UnitId, spell_id: f64, on: Option<UnitId>) -> Option<UnitId> {
    if matches!(spell_field(vm, spell_id, "targeted"), Value::Bool(false)) {
        return Some(me);
    }
    let u = &engine(vm).units;
    let target = if me == units::PLAYER {
        u.target
    } else {
        u.list.get(me.0).and_then(|x| x.target)
    };
    on.or(target).or(Some(me))
}

/// A unit's spell gone off, for the 3D world's effects (decision T582).
fn went_off(vm: &mut Vm, me: UnitId, spell_id: f64, target: Option<UnitId>) {
    let now = crate::clock::now(vm);
    let gone = &mut engine_mut(vm).units.casting.gone;
    gone.retain(|g| now - g.at <= GONE_KEPT);
    gone.push(Gone {
        spell_id,
        caster: me,
        target,
        at: now,
    });
}

fn field(vm: &mut Vm, t: TableRef, key: &str) -> Value {
    let k = vm.str(key);
    vm.heap.table(t).get(k)
}

/// The player's `CURRENT_SPELL_CAST_CHANGED(false)`.
pub(crate) fn cast_changed(vm: &mut Vm, unit: UnitId) {
    if unit == units::PLAYER {
        crate::widgets::fire_event(vm, "CURRENT_SPELL_CAST_CHANGED", &[Value::Bool(false)]);
    }
}

/// The player's `UNIT_SPELLCAST_SENT(unit, target name, castGUID, spellID)`:
/// the target's full name (with a last name), or nil.
fn sent(vm: &mut Vm, unit: UnitId, guid: Value, spell_id: f64, on: Option<UnitId>) {
    if unit != units::PLAYER {
        return;
    }
    // A spell without a target (its data's `targeted = false`) names none
    // (recorded: an instant self spell's nil).
    let untargeted = matches!(spell_field(vm, spell_id, "targeted"), Value::Bool(false));
    let target = {
        let u = &engine(vm).units;
        on.or(u.target).map(|t| {
            let t = &u.list[t.0];
            match &t.last_name {
                Some(last) => format!("{} {last}", t.name),
                None => t.name.clone(),
            }
        })
    };
    let mut target = match target {
        Some(t) if !untargeted => vm.str(&t),
        _ => Value::Nil,
    };
    // Secret where units' identities are (`ConditionalSecret`): a
    // paladin's Judgement in Ragefire Chasm (its cast GUIDs' map 389)
    // secret, out of combat too (`/stdtest engine record combat`); a
    // warrior's Heroic Strike in the open world plain, in combat (scenarios
    // combat and quest). Unverified: which instance restrictions count.
    let restricted = units::active(
        &engine(vm).units,
        &[
            units::MAP,
            units::ENCOUNTER,
            units::CHALLENGE_MODE,
            units::PVP_MATCH,
        ],
    );
    if !target.is_nil() && restricted {
        target = vm.secret_wrap(target);
    }
    units::fire_unit(
        vm,
        "UNIT_SPELLCAST_SENT",
        unit,
        &[target, guid, Value::Num(spell_id)],
    );
    slot_changes(vm, spell_id);
}

/// A spell's slots told changed as it is sent (its data's `slotChanges`:
/// the seconds after, one per time; recorded: Seal of Fury's slot twice at
/// once, Consecration's at once and 7.937 s later; unverified: which
/// spells, and why).
fn slot_changes(vm: &mut Vm, spell_id: f64) {
    let Value::Table(t) = spell_field(vm, spell_id, "slotChanges") else {
        return;
    };
    let n = vm.heap.table(t).border();
    let times: Vec<f64> = (1..=n)
        .filter_map(|i| match vm.heap.table(t).get(Value::Num(i as f64)) {
            Value::Num(x) => Some(x),
            _ => None,
        })
        .collect();
    for after in times {
        if after <= 0.0 {
            slots_changed(vm, spell_id);
        } else {
            let later = vm.native_with(
                "spellSlotsChanged",
                slots_changed_later,
                vec![Value::Num(spell_id)],
            );
            crate::clock::after_engine(vm, after, later);
        }
    }
}

fn slots_changed_later(vm: &mut Vm, c: NativeCall) -> Ret {
    if let Value::Num(id) = c.upvals[0] {
        slots_changed(vm, id);
    }
    Ok(vec![])
}

/// `ACTIONBAR_SLOT_CHANGED(slot)` for each slot holding the spell.
fn slots_changed(vm: &mut Vm, spell_id: f64) {
    let mut slots: Vec<i64> = engine(vm)
        .world
        .character
        .actions
        .iter()
        .filter(|(_, a)| a.kind == "spell" && matches!(a.id, Value::Num(n) if n == spell_id))
        .map(|(s, _)| *s)
        .collect();
    slots.sort_unstable();
    for s in slots {
        crate::widgets::fire_event(vm, "ACTIONBAR_SLOT_CHANGED", &[Value::Num(s as f64)]);
    }
}

/// A cast event for each of the unit's tokens: another unit's cast GUID
/// and spell ID secret (`SecretWhenUnitSpellCastRestricted`; recorded for
/// nameplates out of combat), its cast bar id never.
fn fire_cast(vm: &mut Vm, event: &str, unit: UnitId, args: &[Value]) {
    let mut args = args.to_vec();
    if unit != units::PLAYER {
        for v in args.iter_mut().take(2) {
            if !v.is_nil() {
                *v = vm.secret_wrap(*v);
            }
        }
    }
    units::fire_unit(vm, event, unit, &args);
}

fn events(channel: bool) -> (&'static str, &'static str) {
    if channel {
        (
            "UNIT_SPELLCAST_CHANNEL_START",
            "UNIT_SPELLCAST_CHANNEL_STOP",
        )
    } else {
        ("UNIT_SPELLCAST_START", "UNIT_SPELLCAST_STOP")
    }
}

/// `unit:castSpell(spellID, { castTime?, channel?, notInterruptible? })`.
pub fn control_cast(vm: &mut Vm, c: NativeCall) -> Ret {
    let me = units::this(vm, &c)?;
    let spell_id = vm.check_number(&c, 2)?;
    let opts = match c.arg(3) {
        Value::Table(t) => Some(t),
        _ => None,
    };
    let opt = |vm: &mut Vm, k: &str| opts.map(|t| field(vm, t, k)).unwrap_or(Value::Nil);
    let seconds = match opt(vm, "castTime") {
        Value::Num(s) => Some(s),
        _ => None,
    };
    let look = spell_look(vm, spell_id, seconds);
    let channel = opt(vm, "channel").truthy();
    let not_interruptible = opt(vm, "notInterruptible").truthy();
    cast_at(
        vm,
        me,
        spell_id,
        look,
        channel,
        not_interruptible,
        None,
        None,
    )
}

/// A unit's cast of `spell_id` (its name, icon and seconds): on another
/// unit or object (`on`: the player's `SENT` names it), `done` called at
/// its success.
#[allow(clippy::too_many_arguments)]
pub fn cast_at(
    vm: &mut Vm,
    me: UnitId,
    spell_id: f64,
    (name, icon, seconds): (Value, Value, f64),
    channel: bool,
    not_interruptible: bool,
    on: Option<UnitId>,
    done: Option<Value>,
) -> Ret {
    start(
        vm,
        me,
        spell_id,
        (name, icon, seconds),
        (channel, not_interruptible),
        on,
        done,
        true,
    )
}

/// The player's cast the server started after the client had given it up
/// (moved meanwhile): no `SENT` (recorded: a smelting's next cast).
pub fn cast_unsent(
    vm: &mut Vm,
    spell_id: f64,
    look: (Value, Value, f64),
    done: Option<Value>,
) -> Ret {
    start(
        vm,
        units::PLAYER,
        spell_id,
        look,
        (false, false),
        None,
        done,
        false,
    )
}

#[allow(clippy::too_many_arguments)]
fn start(
    vm: &mut Vm,
    me: UnitId,
    spell_id: f64,
    (name, icon, seconds): (Value, Value, f64),
    (channel, not_interruptible): (bool, bool),
    on: Option<UnitId>,
    done: Option<Value>,
    announce: bool,
) -> Ret {
    // Its name and icon read later (`UnitCastingInfo`) and what its end
    // does: kept alive from the start (a made-up name is no data's; the
    // events told before the cast is stored run scripts that may collect).
    for v in [name, icon].into_iter().chain(done) {
        crate::widgets::keep(vm, v);
    }
    let craft = std::mem::take(&mut engine_mut(vm).units.casting.crafting);
    let now = crate::clock::now(vm);
    let n = {
        let c = &mut engine_mut(vm).units.casting;
        c.next += 1;
        c.next
    };
    let guid = format!("Cast-3-4621-0-1-{}-{:010X}", spell_id as i64, n);
    let bar = format!("CastBar-{:016X}", 0x5EED_0000_0000_0000u64 + n);
    let g = vm.str(&guid);
    let id = Value::Num(spell_id);
    if engine(vm).units.casting.casts.contains_key(&me) {
        if me == units::PLAYER {
            // The player's running cast stays; the new one fails.
            sent(vm, me, g, spell_id, on);
            fire_cast(vm, "UNIT_SPELLCAST_FAILED_QUIET", me, &[g, id, Value::Nil]);
            cast_changed(vm, me);
            let message = vm.str("Another action is in progress");
            crate::widgets::fire_event(vm, "UI_ERROR_MESSAGE", &[Value::Num(280.0), message]);
            return Ok(vec![]);
        }
        // Another unit's new cast replaces the one running (unverified).
        stop(vm, me, true);
    }
    if announce {
        sent(vm, me, g, spell_id, on);
    }
    let target = aim(vm, me, spell_id, on);
    if seconds <= 0.0 && !channel {
        went_off(vm, me, spell_id, target);
        cast_changed(vm, me);
        fire_cast(vm, "UNIT_SPELLCAST_SUCCEEDED", me, &[g, id, Value::Nil]);
        cast_changed(vm, me);
        return Ok(vec![]);
    }
    // A craft's flags with its start (its `UnitCastingInfo` asked then).
    let not_interruptible = if craft {
        !crate::actions::has_trait(vm, Value::Num(spell_id), "interruptible")
    } else {
        not_interruptible
    };
    let cast = Cast {
        spell_id,
        name,
        icon,
        start: now,
        end: now + seconds,
        channel,
        not_interruptible,
        trade_skill: craft,
        guid: guid.clone(),
        bar: bar.clone(),
        done,
        target,
    };
    engine_mut(vm).units.casting.casts.insert(me, cast);
    let (start_event, _) = events(channel);
    let b = vm.str(&bar);
    // A channel has no cast GUID.
    let shown = if channel { Value::Nil } else { g };
    fire_cast(vm, start_event, me, &[shown, id, b]);
    cast_changed(vm, me);
    let native = vm.native_with(
        "castDone",
        cast_done as NativeFn,
        vec![Value::Num(me.0 as f64), g],
    );
    let after = engine_mut(vm)
        .units
        .casting
        .succeeds_after
        .take()
        .unwrap_or(seconds);
    crate::clock::after_engine(vm, after, native);
    Ok(vec![])
}

/// A cast of the player's another module runs (an item's): known to
/// `UnitCastingInfo` until `end_cast`.
pub fn begin_cast(
    vm: &mut Vm,
    spell_id: f64,
    name: Value,
    icon: Value,
    seconds: f64,
    guid: &str,
    bar: &str,
) {
    let now = crate::clock::now(vm);
    let cast = Cast {
        spell_id,
        name,
        icon,
        start: now,
        end: now + seconds,
        channel: false,
        not_interruptible: false,
        trade_skill: false,
        guid: guid.to_string(),
        bar: bar.to_string(),
        done: None,
        target: None,
    };
    engine_mut(vm)
        .units
        .casting
        .casts
        .insert(units::PLAYER, cast);
}

/// A spell's name, icon and cast seconds as its data has them (else
/// "Spell <id>", the default icon, `seconds`).
pub fn spell_look(vm: &mut Vm, spell_id: f64, seconds: Option<f64>) -> (Value, Value, f64) {
    let data = engine(vm)
        .world
        .data
        .spells
        .get(&(spell_id as i64))
        .copied();
    let (name, icon, data_time) = match data {
        Some(t) => (
            field(vm, t, "name"),
            field(vm, t, "iconID"),
            // In milliseconds.
            match field(vm, t, "castTime") {
                Value::Num(ms) => Some(ms / 1000.0),
                _ => None,
            },
        ),
        None => (
            vm.str(&format!("Spell {spell_id}")),
            Value::Num(136243.0),
            None,
        ),
    };
    (name, icon, seconds.or(data_time).unwrap_or(1.5))
}

/// The player's cast another module ran, over.
pub fn end_cast(vm: &mut Vm) {
    engine_mut(vm).units.casting.casts.remove(&units::PLAYER);
}

/// The global cooldown's category (`SPELL_UPDATE_COOLDOWN`'s fourth value
/// for a spell on it: recorded 133).
const GCD_CATEGORY: f64 = 133.0;

/// A spell's data field (`defineSpell`'s, the profile's).
pub(crate) fn spell_field(vm: &mut Vm, id: f64, key: &str) -> Value {
    match engine(vm).world.data.spells.get(&(id as i64)).copied() {
        Some(t) => field(vm, t, key),
        None => Value::Nil,
    }
}

/// The cooldown events after a spell's use: the action bars', the bags',
/// `SPELL_UPDATE_COOLDOWN(id, nil, 0, category, nil)` (the global
/// cooldown's category, or 0 off it), the stance bar's.
fn cooldown_events(vm: &mut Vm, id: f64, category: f64, bags: bool) {
    let fire = |vm: &mut Vm, e: &str, args: &[Value]| crate::widgets::fire_event(vm, e, args);
    // The spell's own cooldown category (data `category`; recorded:
    // Consecration's 932, Hammer of Justice's 32), else 0.
    let own = match spell_field(vm, id, "category") {
        Value::Num(n) => n,
        _ => 0.0,
    };
    fire(vm, "ACTIONBAR_UPDATE_COOLDOWN", &[]);
    fire(vm, "ACTIONBAR_UPDATE_STATE", &[]);
    if bags {
        fire(vm, "BAG_UPDATE_COOLDOWN", &[]);
    }
    fire(
        vm,
        "SPELL_UPDATE_COOLDOWN",
        &[
            Value::Num(id),
            Value::Nil,
            Value::Num(own),
            Value::Num(category),
            Value::Nil,
        ],
    );
    fire(vm, "UPDATE_SHAPESHIFT_COOLDOWN", &[]);
}

/// `player:useSpell(spellID)`: the player pressing a spell, as recorded at a
/// ping near 0.3 s (`/stdtest engine scenario combat`). Without the power
/// its data's `powerCost` needs: `UNIT_SPELLCAST_FAILED` with a client
/// cast GUID, `CURRENT_SPELL_CAST_CHANGED`, `ACTIONBAR_UPDATE_STATE`,
/// `UI_ERROR_MESSAGE(359, "Not enough rage")`. A spell on the next swing
/// (data `nextSwing`): `CURRENT_SPELL_CAST_CHANGED`,
/// `ACTIONBAR_UPDATE_STATE`; a round trip later `SENT`, again both; the
/// next swing lands it (`units::swing`). Else the global cooldown starts
/// (`CURRENT_SPELL_CAST_CHANGED`, the cooldown events, category 133 unless
/// data `offGCD`) and a round trip later the cast (`castSpell`), then the
/// cooldown events again.
pub fn control_use_spell(vm: &mut Vm, c: NativeCall) -> Ret {
    let me = units::this(vm, &c)?;
    if me != units::PLAYER {
        return Err(vm.error("useSpell: the player's"));
    }
    let id = vm.check_number(&c, 2)?;
    use_spell(vm, id)
}

/// The player pressing spell `id` (`control_use_spell`).
pub fn use_spell(vm: &mut Vm, id: f64) -> Ret {
    let me = units::PLAYER;
    let fire = |vm: &mut Vm, e: &str, args: &[Value]| crate::widgets::fire_event(vm, e, args);
    // On its cooldown: refused (recorded: Hammer of Justice pressed again,
    // "Spell is not ready yet.", 61).
    let ready = engine(vm).units.casting.ready_at.get(&(id as i64)).copied();
    if ready.is_some_and(|at| crate::clock::now(vm) < at) {
        refuse(vm, id, 61.0, "Spell is not ready yet.");
        return Ok(vec![]);
    }
    // Moving: a spell with a cast time refused (recorded, `profession`:
    // Holy Light, 2.5 s, pressed on the walk; unverified: before a power
    // check).
    if engine(vm).units.casting.moving && cast_time(vm, id) > 0.0 {
        refuse(vm, id, 57.0, "Can't do that while moving");
        return Ok(vec![]);
    }
    if let Some((code, text)) = target_refusal(vm, id) {
        refuse(vm, id, code, &text);
        return Ok(vec![]);
    }
    if let Value::Num(cost) = spell_field(vm, id, "powerCost")
        && engine(vm).units.list[units::PLAYER.0].power() < cost
    {
        let token = engine(vm).units.list[units::PLAYER.0].power_type;
        let (code, text) = match token as i64 {
            1 => (359.0, "Not enough rage"),
            3 => (360.0, "Not enough energy"),
            _ => (358.0, "Not enough mana"),
        };
        refuse(vm, id, code, text);
        return Ok(vec![]);
    }
    let delay = crate::clock::server_delay(vm);
    if spell_field(vm, id, "nextSwing").truthy() {
        cast_changed(vm, me);
        fire(vm, "ACTIONBAR_UPDATE_STATE", &[]);
        let later = vm.native_with(
            "nextSwingSent",
            next_swing_sent as NativeFn,
            vec![Value::Num(id)],
        );
        crate::clock::after_engine(vm, delay, later);
        return Ok(vec![]);
    }
    // Off the global cooldown: by data or trait (Judgement's, recorded).
    let off = spell_field(vm, id, "offGCD").truthy()
        || crate::actions::has_trait(vm, Value::Num(id), "offGlobalCooldown");
    let category = if off { 0.0 } else { GCD_CATEGORY };
    cast_changed(vm, me);
    if off {
        // Its press tells only that (recorded: Judgement pressed).
        fire(vm, "ACTIONBAR_UPDATE_STATE", &[]);
    } else {
        // The global cooldown from the key press (recorded: the bar's
        // spells on it in the first cooldown events).
        crate::actions::start_global_cooldown(vm);
        cooldown_events(vm, id, category, false);
    }
    // Waiting for the server's answer, with the client's own cast GUID.
    let guid = client_guid(vm, id);
    engine_mut(vm)
        .units
        .casting
        .pending
        .push((id, guid.clone()));
    let g = vm.str(&guid);
    let later = vm.native_with(
        "spellUsed",
        spell_used as NativeFn,
        vec![Value::Num(id), Value::Num(category), g],
    );
    crate::clock::after_engine(vm, delay, later);
    Ok(vec![])
}

/// Why the client refuses a spell at the target (decision T519): out of
/// its range ("Out of range.", 402; recorded: Judgement pressed at a mob
/// that ran off), or behind the player for a spell that needs it in front
/// (its data's `facing`, from the client's `SpellCastingRequirements`;
/// `SPELL_FAILED_UNIT_NOT_INFRONT`, 57, unverified). None for a spell
/// without a target (data `targeted = false`), a helpful one (data
/// `helpful`: the game casts it on the player instead, unverified), no
/// target, or a target at no known place.
fn target_refusal(vm: &mut Vm, id: f64) -> Option<(f64, String)> {
    if matches!(spell_field(vm, id, "targeted"), Value::Bool(false))
        || spell_field(vm, id, "helpful").truthy()
    {
        return None;
    }
    let target = engine(vm).units.target.filter(|t| *t != units::PLAYER)?;
    if matches!(
        crate::spellinfo::in_range(vm, id, Some(target)),
        Value::Bool(false)
    ) {
        return Some((402.0, "Out of range.".into()));
    }
    if spell_field(vm, id, "facing").truthy() && crate::character::behind(vm, target) {
        let text = match vm.get_global("SPELL_FAILED_UNIT_NOT_INFRONT") {
            Value::Str(s) => String::from_utf8_lossy(vm.str_bytes(s)).into_owned(),
            _ => "Target needs to be in front of you.".into(),
        };
        return Some((57.0, text));
    }
    // (a hill or a building between, while the 3D world is on, T524;
    // unverified: the code)
    if !crate::character::in_sight(vm, units::PLAYER, target) {
        let text = match vm.get_global("SPELL_FAILED_LINE_OF_SIGHT") {
            Value::Str(s) => String::from_utf8_lossy(vm.str_bytes(s)).into_owned(),
            _ => "Target not in line of sight".into(),
        };
        return Some((57.0, text));
    }
    None
}

/// A spell's cast time in ms (its data's, else the probed spell info's), 0
/// for an instant one.
fn cast_time(vm: &mut Vm, id: f64) -> f64 {
    let Some(t) = crate::gamedata::spell_data(vm, id as i64) else {
        return 0.0;
    };
    match field(vm, t, "castTime") {
        Value::Num(ms) => ms,
        _ => 0.0,
    }
}

/// `player:setMoving(moving)`: the player starts or stops moving
/// (`PLAYER_STARTED_MOVING`, `PLAYER_STOPPED_MOVING`, recorded); spells
/// with a cast time are refused meanwhile. Unverified: a running cast is
/// left to `interruptCast`.
pub fn control_set_moving(vm: &mut Vm, c: NativeCall) -> Ret {
    let me = units::this(vm, &c)?;
    if me != units::PLAYER {
        return Err(vm.error("setMoving: the player's"));
    }
    let moving = c.arg(2).truthy();
    if engine(vm).units.casting.moving == moving {
        return Ok(vec![]);
    }
    engine_mut(vm).units.casting.moving = moving;
    let event = if moving {
        "PLAYER_STARTED_MOVING"
    } else {
        "PLAYER_STOPPED_MOVING"
    };
    crate::widgets::fire_event(vm, event, &[]);
    Ok(vec![])
}

/// A cast GUID of the client's own (`Cast-2-0-0-0-…`: a press it refused
/// or one it held, recorded).
fn client_guid(vm: &mut Vm, id: f64) -> String {
    let n = {
        let c = &mut engine_mut(vm).units.casting;
        c.next += 1;
        c.next
    };
    format!("Cast-2-0-0-0-{}-{:010X}", id as i64, n)
}

/// The player's target died: the presses on it still waiting for the
/// server fail, in order (`UNIT_SPELLCAST_FAILED` with each one's client
/// cast GUID, `CURRENT_SPELL_CAST_CHANGED`; recorded: two Judgements and
/// two of another spell pressed during the fight's last swing).
pub fn target_died(vm: &mut Vm) {
    let pending = std::mem::take(&mut engine_mut(vm).units.casting.pending);
    let mut kept = Vec::new();
    for (id, guid) in pending {
        let untargeted = matches!(spell_field(vm, id, "targeted"), Value::Bool(false));
        if untargeted {
            kept.push((id, guid));
            continue;
        }
        let g = vm.str(&guid);
        units::fire_unit(
            vm,
            "UNIT_SPELLCAST_FAILED",
            units::PLAYER,
            &[g, Value::Num(id), Value::Nil],
        );
        cast_changed(vm, units::PLAYER);
    }
    engine_mut(vm).units.casting.pending = kept;
}

/// The client refusing the player's spell: `UNIT_SPELLCAST_FAILED` with
/// its own cast GUID (`Cast-2-0-0-0-…`), `CURRENT_SPELL_CAST_CHANGED`,
/// `ACTIONBAR_UPDATE_STATE`, `UI_ERROR_MESSAGE(code, text)` (recorded).
fn refuse(vm: &mut Vm, id: f64, code: f64, text: &str) {
    let me = units::PLAYER;
    let g = client_guid(vm, id);
    let g = vm.str(&g);
    units::fire_unit(
        vm,
        "UNIT_SPELLCAST_FAILED",
        me,
        &[g, Value::Num(id), Value::Nil],
    );
    cast_changed(vm, me);
    crate::widgets::fire_event(vm, "ACTIONBAR_UPDATE_STATE", &[]);
    let text = vm.str(text);
    crate::widgets::fire_event(vm, "UI_ERROR_MESSAGE", &[Value::Num(code), text]);
}

/// The server's answer to an instant spell: the cast, the cooldown events;
/// its cooldown running (data `cooldown`).
fn spell_used(vm: &mut Vm, c: NativeCall) -> Ret {
    let (Value::Num(id), Value::Num(category)) = (c.upvals[0], c.upvals[1]) else {
        return Ok(vec![]);
    };
    // A press failed meanwhile (its target died) has no answer.
    if let Some(Value::Str(g)) = c.upvals.get(2).copied() {
        let guid = String::from_utf8_lossy(vm.str_bytes(g)).into_owned();
        let pending = &mut engine_mut(vm).units.casting.pending;
        let Some(i) = pending.iter().position(|(_, p)| *p == guid) else {
            return Ok(vec![]);
        };
        pending.remove(i);
    }
    if let Value::Num(cd) = spell_field(vm, id, "cooldown") {
        let at = crate::clock::now(vm) + cd;
        engine_mut(vm).units.casting.ready_at.insert(id as i64, at);
    }
    instant(vm, id);
    // What it does as it lands (data `onCast`, a test's function: the
    // damage, the kill, the auras), in the same packet, before its
    // cooldowns (recorded: Judgement's kill, then its cooldowns).
    if let f @ Value::Func(_) = spell_field(vm, id, "onCast") {
        vm.call(f, &[])?;
    }
    // Its cooldowns: the data's list of spells told (`cooldowns = { {
    // spell, a?, b? } }`; recorded: Judgement's three), else its own.
    let listed = spell_field(vm, id, "cooldowns");
    if matches!(listed, Value::Table(_)) {
        crate::widgets::fire_event(vm, "ACTIONBAR_UPDATE_COOLDOWN", &[]);
        crate::widgets::fire_event(vm, "ACTIONBAR_UPDATE_STATE", &[]);
        if let Value::Table(list) = listed {
            crate::widgets::fire_event(vm, "BAG_UPDATE_COOLDOWN", &[]);
            let n = vm.heap.table(list).border();
            for i in 1..=n {
                let Value::Table(e) = vm.heap.table(list).get(Value::Num(i as f64)) else {
                    continue;
                };
                let t = vm.heap.table(e);
                let (spell, a, b) = (
                    t.get(Value::Num(1.0)),
                    t.get(Value::Num(2.0)),
                    t.get(Value::Num(3.0)),
                );
                crate::widgets::fire_event(
                    vm,
                    "SPELL_UPDATE_COOLDOWN",
                    &[spell, Value::Nil, a, b, Value::Nil],
                );
            }
            crate::widgets::fire_event(vm, "UPDATE_SHAPESHIFT_COOLDOWN", &[]);
        }
    } else {
        cooldown_events(vm, id, category, true);
    }
    Ok(vec![])
}

/// An instant cast: `SENT`, `CURRENT_SPELL_CAST_CHANGED`, `SUCCEEDED`,
/// `CURRENT_SPELL_CAST_CHANGED`.
fn instant(vm: &mut Vm, id: f64) {
    let me = units::PLAYER;
    let g = new_guid(vm, id);
    let g = vm.str(&g);
    sent(vm, me, g, id, None);
    let target = aim(vm, me, id, None);
    went_off(vm, me, id, target);
    cast_changed(vm, me);
    fire_cast(
        vm,
        "UNIT_SPELLCAST_SUCCEEDED",
        me,
        &[g, Value::Num(id), Value::Nil],
    );
    cast_changed(vm, me);
}

/// The player's cast running a craft: a trade skill's, not to be
/// interrupted unless it has the trait `interruptible` (recorded: Smelt
/// Copper, Smelt Bronze, a Blacksmithing craft `true, true`; the
/// Sharpening Wheel `true, false`; a gathering neither): the next cast
/// started, its flags with its start (recorded: the answers asked at its
/// `UNIT_SPELLCAST_START`).
pub fn mark_craft(vm: &mut Vm) {
    engine_mut(vm).units.casting.crafting = true;
}

/// A new cast GUID of a kind (3 a spell, 4 an item's, 15 a triggered one,
/// recorded) for the player's spell `id`.
pub fn cast_guid(vm: &mut Vm, kind: u8, id: f64) -> String {
    let n = {
        let c = &mut engine_mut(vm).units.casting;
        c.next += 1;
        c.next
    };
    format!("Cast-{kind}-4621-0-1-{}-{:010X}", id as i64, n)
}

/// `player:spellFailedQuiet(spellID, { client? })`: a spell the server
/// cast for the player (a proc) failing without a word:
/// `UNIT_SPELLCAST_FAILED_QUIET` with its own GUID,
/// `CURRENT_SPELL_CAST_CHANGED(true)`, `ACTIONBAR_UPDATE_STATE()`
/// (recorded: auradeath's 1310927 after the seal's procs; unverified: what
/// fails it). `client`: a press of the player's the server dropped so,
/// told with the client's cast GUID (recorded: auradeath's spell 1866
/// pressed twice while Judgement flew, the second's answer).
pub fn control_spell_failed_quiet(vm: &mut Vm, c: NativeCall) -> Ret {
    let me = units::this(vm, &c)?;
    let id = vm.check_number(&c, 2)?;
    let client = match c.arg(3) {
        Value::Table(t) => field(vm, t, "client").truthy(),
        _ => false,
    };
    // The spell queued for the next swing failing so: no longer queued,
    // told with its own GUID (recorded once, `quest` 82.530: Heroic Strike
    // queued, the target changed to a corpse).
    let queued = match &engine(vm).units.casting.next_swing {
        Some((spell, g)) if *spell == id && !client => Some(g.clone()),
        _ => None,
    };
    if queued.is_some() {
        engine_mut(vm).units.casting.next_swing = None;
    }
    let g = if let Some(g) = queued {
        g
    } else if client {
        client_guid(vm, id)
    } else {
        new_guid(vm, id)
    };
    let g = vm.str(&g);
    fire_cast(
        vm,
        "UNIT_SPELLCAST_FAILED_QUIET",
        me,
        &[g, Value::Num(id), Value::Nil],
    );
    if me == units::PLAYER {
        crate::widgets::fire_event(vm, "CURRENT_SPELL_CAST_CHANGED", &[Value::Bool(true)]);
        crate::widgets::fire_event(vm, "ACTIONBAR_UPDATE_STATE", &[]);
    }
    Ok(vec![])
}

/// `player:castFailed(spellID, { error, client? })`: the player's cast of
/// `spellID` refused, the reason `error` told: a client error's name
/// (`"SPELL_COOLDOWN"`: `LE_GAME_ERR_SPELL_COOLDOWN`, `ERR_SPELL_COOLDOWN`)
/// or a spell failure's (`"TARGETS_DEAD"`: `LE_GAME_ERR_SPELL_FAILED_S`,
/// `SPELL_FAILED_TARGETS_DEAD`). By the server (a cast GUID of its own):
/// `UNIT_SPELLCAST_FAILED`, `CURRENT_SPELL_CAST_CHANGED`, a spell on the
/// global cooldown's cooldown events again, `ACTIONBAR_UPDATE_STATE`,
/// `UI_ERROR_MESSAGE` (recorded: auradeath's presses while Judgement
/// flew, answered after the kill; the cooldown events from one sample,
/// unverified). `client`: by the client (its own cast GUID, no cooldown
/// events; recorded: the presses after the kill, out of range).
pub fn control_cast_failed(vm: &mut Vm, c: NativeCall) -> Ret {
    let me = units::this(vm, &c)?;
    if me != units::PLAYER {
        return Err(vm.error("castFailed: the player's"));
    }
    let id = vm.check_number(&c, 2)?;
    let Value::Table(opts) = c.arg(3) else {
        return Err(vm.error("castFailed: { error = name } expected"));
    };
    let client = field(vm, opts, "client").truthy();
    let name = match field(vm, opts, "error") {
        Value::Str(s) => String::from_utf8_lossy(vm.str_bytes(s)).into_owned(),
        _ => return Err(vm.error("castFailed: an error's name expected")),
    };
    let (mut code, mut text) = (
        vm.get_global(&format!("LE_GAME_ERR_{name}")),
        vm.get_global(&format!("ERR_{name}")),
    );
    if code.is_nil() || text.is_nil() {
        code = vm.get_global("LE_GAME_ERR_SPELL_FAILED_S");
        text = vm.get_global(&format!("SPELL_FAILED_{name}"));
    }
    if code.is_nil() || text.is_nil() {
        return Err(vm.error(&format!("castFailed: no client error {name}")));
    }
    let g = if client {
        client_guid(vm, id)
    } else {
        new_guid(vm, id)
    };
    let g = vm.str(&g);
    units::fire_unit(
        vm,
        "UNIT_SPELLCAST_FAILED",
        me,
        &[g, Value::Num(id), Value::Nil],
    );
    cast_changed(vm, me);
    let off = spell_field(vm, id, "offGCD").truthy()
        || crate::actions::has_trait(vm, Value::Num(id), "offGlobalCooldown");
    if !client && !off {
        // The server's refusal ends the global cooldown the press began
        // (recorded: `auradeath` 17.005, the slots' cooldowns inactive
        // after a refusal 0.48 s into it; unverified elsewhere).
        engine_mut(vm).world.global_cooldown = None;
        cooldown_events(vm, id, GCD_CATEGORY, true);
    }
    crate::widgets::fire_event(vm, "ACTIONBAR_UPDATE_STATE", &[]);
    crate::widgets::fire_event(vm, "UI_ERROR_MESSAGE", &[code, text]);
    Ok(vec![])
}

/// A new cast GUID for the player's spell `id`.
fn new_guid(vm: &mut Vm, id: f64) -> String {
    let n = {
        let c = &mut engine_mut(vm).units.casting;
        c.next += 1;
        c.next
    };
    format!("Cast-3-4621-0-1-{}-{:010X}", id as i64, n)
}

/// The server took a spell for the next swing: `SENT`,
/// `CURRENT_SPELL_CAST_CHANGED`, `ACTIONBAR_UPDATE_STATE`; queued.
fn next_swing_sent(vm: &mut Vm, c: NativeCall) -> Ret {
    let Value::Num(id) = c.upvals[0] else {
        return Ok(vec![]);
    };
    let g = new_guid(vm, id);
    let gv = vm.str(&g);
    sent(vm, units::PLAYER, gv, id, None);
    cast_changed(vm, units::PLAYER);
    crate::widgets::fire_event(vm, "ACTIONBAR_UPDATE_STATE", &[]);
    engine_mut(vm).units.casting.next_swing = Some((id, g));
    Ok(vec![])
}

/// The spell queued for the next swing, taken: its `SUCCEEDED` (and
/// `CURRENT_SPELL_CAST_CHANGED`); its ID.
pub fn land_next_swing(vm: &mut Vm) -> Option<f64> {
    let (id, g) = engine_mut(vm).units.casting.next_swing.take()?;
    let g = vm.str(&g);
    fire_cast(
        vm,
        "UNIT_SPELLCAST_SUCCEEDED",
        units::PLAYER,
        &[g, Value::Num(id), Value::Nil],
    );
    cast_changed(vm, units::PLAYER);
    Some(id)
}

/// After a spell on the next swing landed: the cooldown events (off the
/// global cooldown).
pub fn next_swing_cooldowns(vm: &mut Vm, id: f64) {
    cooldown_events(vm, id, 0.0, true);
}

/// Ends `unit`'s cast: interrupted, or done.
pub(crate) fn stop(vm: &mut Vm, unit: UnitId, interrupted: bool) {
    let Some(cast) = engine_mut(vm).units.casting.casts.remove(&unit) else {
        return;
    };
    let g = if cast.channel {
        Value::Nil
    } else {
        vm.str(&cast.guid)
    };
    let id = Value::Num(cast.spell_id);
    let b = vm.str(&cast.bar);
    if interrupted {
        fire_cast(
            vm,
            "UNIT_SPELLCAST_INTERRUPTED",
            unit,
            &[g, id, Value::Nil, b],
        );
    } else if !cast.channel {
        went_off(vm, unit, cast.spell_id, cast.target);
        fire_cast(vm, "UNIT_SPELLCAST_SUCCEEDED", unit, &[g, id, b]);
    }
    let (_, stop_event) = events(cast.channel);
    if cast.channel {
        fire_cast(vm, stop_event, unit, &[g, id, Value::Nil, b]);
    } else {
        fire_cast(vm, stop_event, unit, &[g, id, b]);
    }
    // A gathering's end tells no change of the current cast (recorded:
    // mining's success, its loot at once).
    match cast.done {
        Some(f) if !interrupted => {
            // What its end does (the loot, the craft) failing is an error
            // the error handler hears, as a script's would be.
            if let Err(e) = vm.call(f, &[]) {
                let handler = vm.error_handler;
                vm.call_from_engine(handler, &[e.value]);
            }
        }
        _ => cast_changed(vm, unit),
    }
}

/// The cast's end, if it is still the unit's cast.
fn cast_done(vm: &mut Vm, c: NativeCall) -> Ret {
    let (Some(Value::Num(u)), Some(Value::Str(g))) = (c.upvals.first(), c.upvals.get(1)) else {
        return Ok(vec![]);
    };
    let unit = UnitId(*u as usize);
    let guid = String::from_utf8_lossy(vm.str_bytes(*g)).into_owned();
    if engine(vm)
        .units
        .casting
        .casts
        .get(&unit)
        .is_some_and(|c| c.guid == guid)
    {
        stop(vm, unit, false);
    }
    Ok(vec![])
}

// The server answers the player's interrupted cast a round trip later
// (`/stdtest engine record cast`, an interrupt by moving: 0.35–0.54 s).

/// `unit:interruptCast()`; the player's is answered by the server later:
/// three `INTERRUPTED(unit, castGUID, spellID, nil, nil)`, each with
/// `CURRENT_SPELL_CAST_CHANGED(false)`, then `UI_ERROR_MESSAGE(57,
/// "Interrupted")`. The player's with no cast: a craft's next cast asked,
/// given up (`tradeskill.rs`).
pub fn control_interrupt(vm: &mut Vm, c: NativeCall) -> Ret {
    let me = units::this(vm, &c)?;
    let cast = engine(vm).units.casting.casts.get(&me).cloned();
    // No cast yet: a craft's next cast asked of the server, given up.
    if me == units::PLAYER && cast.is_none() {
        crate::tradeskill::give_up_asked(vm);
        return Ok(vec![]);
    }
    stop(vm, me, true);
    if me == units::PLAYER
        && let Some(cast) = cast
        && !cast.channel
    {
        let g = vm.str(&cast.guid);
        let echo = vm.native_with(
            "interruptEcho",
            interrupt_echo,
            vec![g, Value::Num(cast.spell_id)],
        );
        let delay = crate::clock::server_delay(vm);
        crate::clock::after_engine(vm, delay, echo);
    }
    Ok(vec![])
}

fn interrupt_echo(vm: &mut Vm, c: NativeCall) -> Ret {
    let (Some(&g), Some(&id)) = (c.upvals.first(), c.upvals.get(1)) else {
        return Ok(vec![]);
    };
    for _ in 0..3 {
        fire_cast(
            vm,
            "UNIT_SPELLCAST_INTERRUPTED",
            units::PLAYER,
            &[g, id, Value::Nil, Value::Nil],
        );
        cast_changed(vm, units::PLAYER);
    }
    let message = vm.str("Interrupted");
    crate::widgets::fire_event(vm, "UI_ERROR_MESSAGE", &[Value::Num(57.0), message]);
    Ok(vec![])
}

fn info(vm: &mut Vm, c: &NativeCall, channel: bool) -> Ret {
    let Some(unit) = units::unit_arg(vm, c, 1) else {
        return Ok(vec![]);
    };
    let Some(cast) = engine(vm).units.casting.casts.get(&unit).cloned() else {
        return Ok(vec![]);
    };
    if cast.channel != channel {
        return Ok(vec![]);
    }
    let mut out = vec![
        cast.name,
        cast.name,
        cast.icon,
        Value::Num((cast.start * 1000.0).floor()),
        Value::Num((cast.end * 1000.0).floor()),
        Value::Bool(cast.trade_skill),
    ];
    if !channel {
        out.push(vm.str(&cast.guid));
    }
    out.push(Value::Bool(cast.not_interruptible));
    out.push(Value::Num(cast.spell_id));
    // The cast bar's ID and the delay (`castBarID`, `delayTimeMs`:
    // recorded for an item's cast, its bar's and 0).
    if !channel && !cast.bar.is_empty() {
        out.push(vm.str(&cast.bar));
        out.push(Value::Num(0.0));
    }
    Ok(out)
}

fn unit_casting_info(vm: &mut Vm, c: NativeCall) -> Ret {
    info(vm, &c, false)
}

fn unit_channel_info(vm: &mut Vm, c: NativeCall) -> Ret {
    info(vm, &c, true)
}

pub fn behaviours() -> Vec<(&'static str, NativeFn)> {
    vec![
        ("UnitCastingInfo", unit_casting_info),
        ("UnitChannelInfo", unit_channel_info),
    ]
}
