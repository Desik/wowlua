//! The classic clients' functions that standard's and camelot's lack
//! (Ketho's client dumps: Mists 5.5.4, TBC Anniversary 2.5.6, Classic Era
//! 1.15.9), for the flavors that document them.

use wowlua_vm::stdlib::Ret;
use wowlua_vm::{NativeCall, NativeFn, Value, Vm};

/// Keeps a combat log event's payload as the current one (`fire_now`).
pub fn note_combat_log_event(vm: &mut Vm, args: &[Value]) {
    let t = vm.new_table();
    for (i, v) in args.iter().enumerate() {
        let _ = vm.heap.table_mut(t).set(Value::Num(i as f64 + 1.0), *v);
    }
    crate::widgets::keep_as(vm, "combatLogEvent", Value::Table(t));
}

/// `C_CombatLog.GetCurrentEventInfo()`: the payload of the combat log
/// event last fired (`COMBAT_LOG_EVENT_UNFILTERED`'s), nothing before one.
fn get_current_event_info(vm: &mut Vm, _c: NativeCall) -> Ret {
    let registry = vm.registry;
    let key = vm.heap.intern(b"wowlua.engine.named");
    let Value::Table(named) = vm.heap.table(registry).get_str(key) else {
        return Ok(vec![]);
    };
    let k = vm.str("combatLogEvent");
    let Value::Table(t) = vm.heap.table(named).get(k) else {
        return Ok(vec![]);
    };
    let n = vm.heap.table(t).border();
    Ok((1..=n)
        .map(|i| vm.heap.table(t).get(Value::Num(i as f64)))
        .collect())
}

/// `C_GameRules.IsHardcoreActive()`: whether the realm has the hardcore
/// rules (permanent death); not by default (recorded false on the three
/// classic clients, off a hardcore realm).
fn is_hardcore_active(vm: &mut Vm, _c: NativeCall) -> Ret {
    Ok(vec![Value::Bool(crate::engine(vm).world.hardcore)])
}

/// `setHardcore(active)`: the realm has the hardcore rules or not, on
/// vanilla alone (Classic Era's hardcore realms; Mists and TBC have none).
pub fn control_set_hardcore(vm: &mut Vm, c: NativeCall) -> Ret {
    if crate::engine(vm).flavor != "vanilla" {
        return Err(vm.error("mock.setHardcore: this flavor has no hardcore realms"));
    }
    crate::engine_mut(vm).world.hardcore = c.arg(1).truthy();
    Ok(vec![])
}

/// `GetWeaponEnchantInfo()`: no temporary enchant (as
/// `C_PaperDollInfo.GetTemporaryEnchantmentInfo`), for the main hand, the
/// off hand and, before Mists removed the slot, the ranged weapon: has, then
/// the expiration, charges and enchant ID (8 values on Mists 5.5.4, 12 on
/// TBC 2.5.6 and Classic Era 1.15.9, recorded; the wiki's ranged values
/// "only on cataclysm/4.x").
fn get_weapon_enchant_info(vm: &mut Vm, _c: NativeCall) -> Ret {
    let slots = if crate::engine(vm).flavor == "mists" {
        2
    } else {
        3
    };
    Ok((0..slots)
        .flat_map(|_| [Value::Bool(false), Value::Nil, Value::Nil, Value::Nil])
        .collect())
}

/// The runes' types, two of each (1 blood, 3 frost, 2 unholy), on every
/// class (recorded on Mists' Death Knight and TBC's and Classic Era's
/// others); no value at all for another index (rune 7: `select("#", ...)` 0). ponytail: no death runes, a test
/// that needs them mocks it.
fn get_rune_type(vm: &mut Vm, c: NativeCall) -> Ret {
    let Value::Num(rune) = vm.hidden(c.arg(1)) else {
        return Ok(vec![Value::Nil]);
    };
    Ok(match rune {
        1.0 | 2.0 => vec![Value::Num(1.0)],
        3.0 | 4.0 => vec![Value::Num(3.0)],
        5.0 | 6.0 => vec![Value::Num(2.0)],
        _ => vec![],
    })
}

/// A function the client has but refuses: "Script_<name>: API unsupported
/// in this version of World of Warcraft." (recorded).
fn unsupported(vm: &mut Vm, c: NativeCall) -> Ret {
    let name = crate::recorded::called_name(vm, &c).unwrap_or_default();
    Err(crate::widgets::c_error(
        vm,
        &format!("Script_{name}: API unsupported in this version of World of Warcraft."),
        false,
    ))
}

/// `C_LootHistory.GetNumItems()`: no loot rolls (the group's are not
/// modelled; unverified).
fn none_counted(_vm: &mut Vm, _c: NativeCall) -> Ret {
    Ok(vec![Value::Num(0.0)])
}

/// `C_ProductChoice.GetChoices()`: no product choices (unverified).
fn no_choices(vm: &mut Vm, _c: NativeCall) -> Ret {
    Ok(vec![Value::Table(vm.new_table())])
}

/// The behaviours, on the classic flavors alone.
pub fn behaviours(flavor: &str) -> Vec<(&'static str, NativeFn)> {
    if !matches!(flavor, "mists" | "tbc" | "vanilla") {
        return Vec::new();
    }
    let mut out: Vec<(&'static str, NativeFn)> = vec![
        ("C_CombatLog.GetCurrentEventInfo", get_current_event_info),
        ("C_LootHistory.GetNumItems", none_counted),
        ("C_ProductChoice.GetChoices", no_choices),
        ("C_GameRules.IsHardcoreActive", is_hardcore_active),
        ("GetTalentGroupRole", unsupported),
        ("GetWeaponEnchantInfo", get_weapon_enchant_info),
    ];
    // Mists has its tiers and specs; TBC and Classic Era their tabs.
    let refused: &[&'static str] = if flavor == "mists" {
        &[
            "GetNumTalentTabs",
            "GetNumTalentGroups",
            "GetUnspentTalentPoints",
        ]
    } else {
        &["GetNumSpecGroups", "GetTalentTierInfo"]
    };
    out.extend(refused.iter().map(|&n| (n, unsupported as NativeFn)));
    out.push(("GetRuneType", get_rune_type));
    out
}
