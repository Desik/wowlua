//! The client's global functions no other module answers: the game's
//! systems Forever lacks or the engine does not model (the encounter
//! journal, guild banks, looking for group, archaeology, war games,
//! vehicles, the camera), answered in their "nothing here" state; the
//! client's settings an addon sets and reads back (kept per engine); and
//! the few that follow from the engine's world (the gear the player wears,
//! the group, the pet, the player's faction).
//!
//! Each answer names its source: a recording (`/stdtest engine answers`,
//! `tests/client/forever-camelot-answers.lua`), Blizzard's documentation
//! and UI code, warcraft.wiki.gg, or the function's name. Everything not
//! from a recording is unverified.

use std::rc::Rc;

use wowlua_vm::stdlib::Ret;
use wowlua_vm::{NativeCall, NativeFn, TableRef, Value, Vm};

use crate::{engine, units, widgets};

// ------------------------------------------------------------ helpers

/// Another behaviour's answer to a call made from here (as
/// `conditionals.rs` asks), its values unwrapped; none without one.
fn ask(vm: &mut Vm, name: &'static str, args: &[Value]) -> Vec<Value> {
    let Some(f) = engine(vm).behaviour(name) else {
        return vec![];
    };
    let full = vm.str(name);
    let call = NativeCall {
        args: args.iter().copied().collect(),
        taints: args.iter().map(|_| Default::default()).collect(),
        origins: args.iter().map(|_| Default::default()).collect(),
        upvals: Rc::from([full]),
        upval_taints: Rc::from([Default::default()]),
    };
    match f(vm, call) {
        Ok(values) => values.into_iter().map(|v| vm.hidden(v)).collect(),
        Err(_) => vec![],
    }
}

/// An argument as text, if it is a string.
fn text_arg(vm: &Vm, c: &NativeCall, n: usize) -> Option<String> {
    match vm.hidden(c.arg(n)) {
        Value::Str(s) => Some(String::from_utf8_lossy(vm.str_bytes(s)).into_owned()),
        _ => None,
    }
}

/// An argument as a number, if it is one.
fn num_arg(vm: &Vm, c: &NativeCall, n: usize) -> Option<f64> {
    match vm.hidden(c.arg(n)) {
        Value::Num(n) => Some(n),
        _ => None,
    }
}

/// The settings this module keeps (a table in the registry, one per
/// engine): what a setter set, for its getter.
fn state(vm: &mut Vm) -> TableRef {
    let registry = vm.registry;
    let key = vm.heap.intern(b"wowlua.engine.globalsinfo");
    match vm.heap.table(registry).get_str(key) {
        Value::Table(t) => t,
        _ => {
            let t = vm.new_table();
            vm.heap.table_mut(registry).set_str(key, Value::Table(t));
            t
        }
    }
}

fn get(vm: &mut Vm, key: &str) -> Value {
    let t = state(vm);
    let k = vm.str(key);
    vm.heap.table(t).get(k)
}

pub(crate) fn put(vm: &mut Vm, key: &str, v: Value) {
    let t = state(vm);
    let k = vm.str(key);
    let _ = vm.heap.table_mut(t).set(k, v);
}

/// A kept setting, or `default` before any setter ran.
pub(crate) fn get_or(vm: &mut Vm, key: &str, default: Value) -> Value {
    match get(vm, key) {
        Value::Nil => default,
        v => v,
    }
}

// ------------------------------------------------------------ generic answers

fn nothing(_vm: &mut Vm, _c: NativeCall) -> Ret {
    Ok(vec![])
}

fn no(_vm: &mut Vm, _c: NativeCall) -> Ret {
    Ok(vec![Value::Bool(false)])
}

fn yes(_vm: &mut Vm, _c: NativeCall) -> Ret {
    Ok(vec![Value::Bool(true)])
}

fn zero(_vm: &mut Vm, _c: NativeCall) -> Ret {
    Ok(vec![Value::Num(0.0)])
}

fn two_zeros(_vm: &mut Vm, _c: NativeCall) -> Ret {
    Ok(vec![Value::Num(0.0), Value::Num(0.0)])
}

fn three_zeros(_vm: &mut Vm, _c: NativeCall) -> Ret {
    Ok(vec![Value::Num(0.0); 3])
}

fn six_zeros(_vm: &mut Vm, _c: NativeCall) -> Ret {
    Ok(vec![Value::Num(0.0); 6])
}

fn two_falses(_vm: &mut Vm, _c: NativeCall) -> Ret {
    Ok(vec![Value::Bool(false), Value::Bool(false)])
}

fn nil(_vm: &mut Vm, _c: NativeCall) -> Ret {
    Ok(vec![Value::Nil])
}

fn empty_string(vm: &mut Vm, _c: NativeCall) -> Ret {
    Ok(vec![vm.str("")])
}

fn empty_table(vm: &mut Vm, _c: NativeCall) -> Ret {
    Ok(vec![Value::Table(vm.new_table())])
}

/// The table the caller passed (filled with nothing), else a new empty
/// one: the LFG lists' order and state functions fill the caller's table.
fn passed_table(vm: &mut Vm, c: NativeCall) -> Ret {
    match c.arg(1) {
        Value::Table(t) => Ok(vec![Value::Table(t)]),
        _ => Ok(vec![Value::Table(vm.new_table())]),
    }
}

// ------------------------------------------------------------ table aliases

/// Blizzard's `TableUtil.lua` makes these globals the table library's
/// functions (`TableIsEmpty = table.isempty`); before it loads, the
/// client's C functions of the same names answer as those do.
fn table_alias(vm: &mut Vm, c: &NativeCall, field: &str) -> Ret {
    let table = vm.get_global("table");
    let Value::Table(t) = table else {
        return Ok(vec![]);
    };
    let k = vm.str(field);
    let f = vm.heap.table(t).get(k);
    if f.is_nil() {
        return Ok(vec![]);
    }
    let args: Vec<Value> = (1..=c.len()).map(|i| c.arg(i)).collect();
    vm.call(f, &args)
}

fn table_is_empty(vm: &mut Vm, c: NativeCall) -> Ret {
    table_alias(vm, &c, "isempty")
}

fn count_table(vm: &mut Vm, c: NativeCall) -> Ret {
    table_alias(vm, &c, "count")
}

fn get_keys_array(vm: &mut Vm, c: NativeCall) -> Ret {
    table_alias(vm, &c, "keys")
}

fn get_values_array(vm: &mut Vm, c: NativeCall) -> Ret {
    table_alias(vm, &c, "values")
}

fn t_contains(vm: &mut Vm, c: NativeCall) -> Ret {
    table_alias(vm, &c, "contains")
}

fn t_index_of(vm: &mut Vm, c: NativeCall) -> Ret {
    table_alias(vm, &c, "indexof")
}

fn t_delete_item(vm: &mut Vm, c: NativeCall) -> Ret {
    table_alias(vm, &c, "removevalue")
}

fn t_unordered_remove(vm: &mut Vm, c: NativeCall) -> Ret {
    table_alias(vm, &c, "removeunordered")
}

// ------------------------------------------------------------ the player's gear

/// The class and subclass of the item worn in an inventory slot
/// (`C_Item.GetItemInfoInstant`'s sixth and seventh results).
fn worn_class(vm: &mut Vm, slot: i64) -> Option<(f64, f64)> {
    let id = engine(vm).world.character.equipment.get(&slot)?.id;
    let info = ask(vm, "C_Item.GetItemInfoInstant", &[Value::Num(id as f64)]);
    match (info.get(5), info.get(6)) {
        (Some(Value::Num(class)), Some(Value::Num(sub))) => Some((*class, *sub)),
        _ => None,
    }
}

const WEAPON: f64 = 2.0;
const OFF_HAND: i64 = 17;
const RANGED: i64 = 18;

/// `IsDualWielding()`: a weapon in the off hand (a shield or a held item
/// is armor). Recorded false for a character without one; unverified for
/// one with.
fn is_dual_wielding(vm: &mut Vm, _c: NativeCall) -> Ret {
    let dual = worn_class(vm, OFF_HAND).is_some_and(|(class, _)| class == WEAPON);
    Ok(vec![Value::Bool(dual)])
}

/// `IsRangedWeapon()`: a bow, gun, crossbow or thrown weapon in the ranged
/// slot (recorded false without one; unverified which weapons count:
/// warcraft.wiki.gg does not say, wands left out).
fn is_ranged_weapon(vm: &mut Vm, _c: NativeCall) -> Ret {
    let ranged = worn_class(vm, RANGED)
        .is_some_and(|(class, sub)| class == WEAPON && matches!(sub as i64, 2 | 3 | 16 | 18));
    Ok(vec![Value::Bool(ranged)])
}

/// `HasWandEquipped()`: a wand (weapon subclass 19) in the ranged slot
/// (from the name; unverified).
fn has_wand_equipped(vm: &mut Vm, _c: NativeCall) -> Ret {
    let wand = worn_class(vm, RANGED).is_some_and(|(class, sub)| class == WEAPON && sub == 19.0);
    Ok(vec![Value::Bool(wand)])
}

/// `CanDualWield()`: the player knows Dual Wield (spell 674), as the
/// spellbook answers (unverified).
fn can_dual_wield(vm: &mut Vm, _c: NativeCall) -> Ret {
    let known = ask(vm, "C_SpellBook.IsSpellKnown", &[Value::Num(674.0)])
        .first()
        .is_some_and(|v| v.truthy());
    Ok(vec![Value::Bool(known)])
}

// ------------------------------------------------------------ the world

/// `IsUnitModelReadyForUI(unit)`: a unit that exists (recorded true for
/// the player; unverified for other units and for none).
fn is_unit_model_ready(vm: &mut Vm, c: NativeCall) -> Ret {
    Ok(vec![Value::Bool(units::unit_arg(vm, &c, 1).is_some())])
}

/// `PlayerIsInCombat()`: the player in combat, as `UnitAffectingCombat`
/// answers (from the name; unverified).
fn player_is_in_combat(vm: &mut Vm, _c: NativeCall) -> Ret {
    let player = vm.str("player");
    let combat = ask(vm, "UnitAffectingCombat", &[player])
        .first()
        .is_some_and(|v| v.truthy());
    Ok(vec![Value::Bool(combat)])
}

/// `IsOutdoors()`: not under a roof, as `IsIndoors` answers (the macro
/// conditionals' rule; unverified: caves, instances).
fn is_outdoors(vm: &mut Vm, _c: NativeCall) -> Ret {
    let indoors = ask(vm, "IsIndoors", &[])
        .first()
        .is_some_and(|v| v.truthy());
    Ok(vec![Value::Bool(!indoors)])
}

/// `GroupHasOfflineMember()`: a member of the player's group offline
/// (from the name; unverified).
fn group_has_offline_member(vm: &mut Vm, _c: NativeCall) -> Ret {
    let u = &engine(vm).units;
    let offline = u.group.iter().any(|id| !u.list[id.0].online);
    Ok(vec![Value::Bool(offline)])
}

/// `PetHasSpellbook()`, `PetCanBeDismissed()`: the player has a pet (from
/// the names; unverified: a pet without abilities, a charmed one).
fn has_pet(vm: &mut Vm, _c: NativeCall) -> Ret {
    Ok(vec![Value::Bool(engine(vm).units.pet.is_some())])
}

/// `CanLootUnit(guid)`: whether the unit has loot and the player may take
/// it: a dead unit with loot left (unverified: tapping, a group's loot
/// rules).
fn can_loot_unit(vm: &mut Vm, c: NativeCall) -> Ret {
    let Some(guid) = text_arg(vm, &c, 1) else {
        return Ok(vec![Value::Bool(false), Value::Bool(false)]);
    };
    // A GUID naming no unit: nothing (the full probe).
    let Some(unit) = engine(vm).units.list.iter().find(|u| u.guid == guid) else {
        return Ok(vec![]);
    };
    let loot = unit.dead && !unit.loot.is_empty();
    // A living unit leaves nothing to forbid: may loot, nothing there (the
    // full probe: the player's own GUID answers false, true; unverified for
    // other units).
    let may = loot || !unit.dead;
    Ok(vec![Value::Bool(loot), Value::Bool(may)])
}

/// `GetDefaultLanguage()`, `GetAlternativeDefaultLanguage()`: the
/// player's faction's language and its ID, the faction as
/// `UnitFactionGroup` answers (recorded "Orcish", 1 for a Horde
/// character; the Alliance's Common, 7, unverified).
fn get_default_language(vm: &mut Vm, _c: NativeCall) -> Ret {
    let player = vm.str("player");
    let alliance = match ask(vm, "UnitFactionGroup", &[player]).first() {
        Some(Value::Str(s)) => vm.str_bytes(*s) == b"Alliance",
        _ => false,
    };
    let (name, id) = if alliance {
        ("Common", 7.0)
    } else {
        ("Orcish", 1.0)
    };
    Ok(vec![vm.str(name), Value::Num(id)])
}

/// The player's languages: its faction's, then its race's own (the full
/// probe: the Undead's "Orcish", 1 and "Gutterspeak", 33; the other
/// races' unverified, the client's language IDs).
fn languages(vm: &Vm) -> Vec<(&'static str, f64)> {
    let race = engine(vm).units.list[units::PLAYER.0].race.clone();
    let (file, _) = units::race_file_and_id(&race);
    let own: Option<(&str, f64)> = match file.as_str() {
        "Scourge" => Some(("Gutterspeak", 33.0)),
        "Tauren" => Some(("Taurahe", 3.0)),
        "Troll" => Some(("Zandali", 14.0)),
        "BloodElf" => Some(("Thalassian", 10.0)),
        "Goblin" => Some(("Goblin", 40.0)),
        "Dwarf" => Some(("Dwarvish", 6.0)),
        "NightElf" => Some(("Darnassian", 2.0)),
        "Gnome" => Some(("Gnomish", 13.0)),
        "Draenei" => Some(("Draenei", 35.0)),
        "Worgen" => Some(("Gilnean", 39.0)),
        _ => None,
    };
    let alliance = matches!(
        file.as_str(),
        "Human" | "Dwarf" | "NightElf" | "Gnome" | "Draenei" | "Worgen"
    );
    let mut out = vec![if alliance {
        ("Common", 7.0)
    } else {
        ("Orcish", 1.0)
    }];
    out.extend(own);
    out
}

/// `GetNumLanguages()`: how many the player speaks.
pub(crate) fn get_num_languages(vm: &mut Vm, _c: NativeCall) -> Ret {
    Ok(vec![Value::Num(languages(vm).len() as f64)])
}

/// `GetLanguageByIndex(index)`: a language's name and ID; nothing past
/// them (the full probe).
fn get_language_by_index(vm: &mut Vm, c: NativeCall) -> Ret {
    let Value::Num(i) = vm.hidden(c.arg(1)) else {
        return Ok(vec![]);
    };
    let list = languages(vm);
    Ok(
        match (i as usize).checked_sub(1).and_then(|i| list.get(i)) {
            Some((name, id)) if i >= 1.0 => vec![vm.str(name), Value::Num(*id)],
            _ => vec![],
        },
    )
}

/// `SelectedRealmName()`: the realm the player is on, as `GetRealmName`
/// answers (from the name; unverified).
fn selected_realm_name(vm: &mut Vm, _c: NativeCall) -> Ret {
    match ask(vm, "GetRealmName", &[]).first().copied() {
        Some(v @ Value::Str(_)) => Ok(vec![v]),
        _ => Ok(vec![vm.str("")]),
    }
}

/// `GetLocalGameTime()`: the game's time of day, as `GetGameTime` answers
/// (unverified: the client's local and server times are the same here).
fn get_local_game_time(vm: &mut Vm, _c: NativeCall) -> Ret {
    let t = ask(vm, "GetGameTime", &[]);
    let hour = t.first().copied().unwrap_or(Value::Num(0.0));
    let minute = t.get(1).copied().unwrap_or(Value::Num(0.0));
    Ok(vec![hour, minute])
}

/// `GetResSicknessDuration()`: how long resurrection sickness lasts at the
/// player's level, unformatted: classic's minute a level above 10, ten
/// at most (the full probe: "10 |4minute:minutes;" at level 21; the
/// other levels unverified), none below level 11.
fn get_res_sickness_duration(vm: &mut Vm, _c: NativeCall) -> Ret {
    let level = engine(vm).units.list[units::PLAYER.0].level;
    let minutes = (level - 10.0).min(10.0).floor();
    if minutes < 1.0 {
        return Ok(vec![Value::Nil]);
    }
    Ok(vec![vm.str(&format!("{minutes} |4minute:minutes;"))])
}

/// `GetMouseButtonName(button)`: a mouse button's own name; another
/// "UNKNOWN" (the full probe: the five buttons, "x"; Button6 and above
/// unverified).
fn get_mouse_button_name(vm: &mut Vm, c: NativeCall) -> Ret {
    let name = match vm.hidden(c.arg(1)) {
        Value::Str(s) => String::from_utf8_lossy(vm.str_bytes(s)).into_owned(),
        _ => String::new(),
    };
    let known = matches!(name.as_str(), "LeftButton" | "RightButton" | "MiddleButton")
        || name
            .strip_prefix("Button")
            .and_then(|n| n.parse::<u32>().ok())
            .is_some_and(|n| (4..=31).contains(&n));
    Ok(vec![vm.str(if known { &name } else { "UNKNOWN" })])
}

/// `GetDefaultScale()`: the UI scale that makes the screen 768 units high
/// at its physical height (the screen's, decision T237), Blizzard's long-standing
/// rule, as a C float (recorded 0.71111112833023 at 1080).
fn get_default_scale(vm: &mut Vm, _c: NativeCall) -> Ret {
    let scale = (768.0 / crate::engine(vm).screen.height) as f32;
    Ok(vec![Value::Num(f64::from(scale))])
}

/// `C_Spell.GetSchoolString`'s global twin `GetSchoolString(mask)`: its
/// answer (recorded "Frost" for 16, "Fire" for 4), else a single school's
/// name (unverified).
fn get_school_string(vm: &mut Vm, c: NativeCall) -> Ret {
    let mask = c.arg(1);
    let asked = ask(vm, "C_Spell.GetSchoolString", &[mask]);
    if let Some(v @ Value::Str(_)) = asked.first().copied() {
        return Ok(vec![v]);
    }
    let name = match num_arg(vm, &c, 1).map(|n| n as i64) {
        Some(1) => "Physical",
        Some(2) => "Holy",
        Some(4) => "Fire",
        Some(8) => "Nature",
        Some(16) => "Frost",
        Some(32) => "Shadow",
        Some(64) => "Arcane",
        _ => "",
    };
    Ok(vec![vm.str(name)])
}

/// The global twin of `C_VideoOptions.IsOutlineModeSupported()`: its
/// answer (recorded false).
fn is_outline_mode_supported(vm: &mut Vm, _c: NativeCall) -> Ret {
    let supported = ask(vm, "C_VideoOptions.IsOutlineModeSupported", &[])
        .first()
        .is_some_and(|v| v.truthy());
    Ok(vec![Value::Bool(supported)])
}

/// `DoesTemplateExist(name)`: a template the loaded XML defined, as
/// `C_XMLUtil.GetTemplateInfo` knows it (from the name; unverified).
fn does_template_exist(vm: &mut Vm, c: NativeCall) -> Ret {
    let name = c.arg(1);
    let found = ask(vm, "C_XMLUtil.GetTemplateInfo", &[name])
        .first()
        .is_some_and(|v| !v.is_nil());
    Ok(vec![Value::Bool(found)])
}

/// `GetClickFrame(name)`: the frame a global of that name holds, else nil
/// (warcraft.wiki.gg; unverified).
fn get_click_frame(vm: &mut Vm, c: NativeCall) -> Ret {
    let Some(name) = text_arg(vm, &c, 1) else {
        return Ok(vec![Value::Nil]);
    };
    let v = vm.get_global(&name);
    if widgets::widget_of(vm, v).is_some() {
        Ok(vec![v])
    } else {
        Ok(vec![Value::Nil])
    }
}

/// `RunBinding(command [, "up"])`: runs the binding's code as a key press
/// would, down unless "up" (warcraft.wiki.gg; unverified).
fn run_binding(vm: &mut Vm, c: NativeCall) -> Ret {
    let Some(command) = text_arg(vm, &c, 1) else {
        return Ok(vec![]);
    };
    let state = match text_arg(vm, &c, 2).as_deref() {
        Some("up") => "up",
        _ => "down",
    };
    crate::bindings::run_command(vm, &command, state);
    Ok(vec![])
}

/// `CreateUnitHealPredictionCalculator()`: a new calculator object of the
/// documented class (its methods are the documentation's; unverified).
fn create_heal_prediction_calculator(vm: &mut Vm, _c: NativeCall) -> Ret {
    Ok(vec![widgets::create(
        vm,
        "UnitHealPredictionCalculator",
        None,
        None,
    )])
}

/// `CreateFontFamily(name, members)`: a named font object with the first
/// member's file, height and flags (the documentation's
/// `CreateFontFamilyMemberInfo`; unverified: the other alphabets' members
/// are not kept).
fn create_font_family(vm: &mut Vm, c: NativeCall) -> Ret {
    let f = crate::lua_helper(
        vm,
        "CreateFontFamily",
        "return function(name, members)
            local font = CreateFont(name)
            local m = type(members) == 'table' and members[1]
            if type(m) == 'table' and m.file then
                font:SetFont(m.file, m.height or 12, m.flags or '')
            end
            return font
        end",
    )?;
    vm.call(f, &[c.arg(1), c.arg(2)])
}

/// `CopyToClipboard(text [, removeMarkup])`: the length copied, the text's
/// in bytes (the documentation's `length`; the engine has no clipboard;
/// unverified with markup removed).
fn copy_to_clipboard(vm: &mut Vm, c: NativeCall) -> Ret {
    let n = match vm.hidden(c.arg(1)) {
        Value::Str(s) => vm.str_bytes(s).len(),
        _ => 0,
    };
    Ok(vec![Value::Num(n as f64)])
}

/// `CalculateStringEditDistance(a, b)`: the Levenshtein distance between
/// the two strings' characters (from the name; unverified).
fn calculate_string_edit_distance(vm: &mut Vm, c: NativeCall) -> Ret {
    let a: Vec<char> = text_arg(vm, &c, 1).unwrap_or_default().chars().collect();
    let b: Vec<char> = text_arg(vm, &c, 2).unwrap_or_default().chars().collect();
    Ok(vec![Value::Num(edit_distance(&a, &b) as f64)])
}

fn edit_distance(a: &[char], b: &[char]) -> usize {
    let mut row: Vec<usize> = (0..=b.len()).collect();
    for (i, ca) in a.iter().enumerate() {
        let mut prev = row[0];
        row[0] = i + 1;
        for (j, cb) in b.iter().enumerate() {
            let cost = usize::from(ca != cb);
            let next = (row[j + 1] + 1).min(row[j] + 1).min(prev + cost);
            prev = row[j + 1];
            row[j + 1] = next;
        }
    }
    row[b.len()]
}

/// `CaseAccentInsensitiveParse(name)`: the name in lower case with the
/// Latin accents taken off, a form searches compare (from the name;
/// unverified).
fn case_accent_insensitive_parse(vm: &mut Vm, c: NativeCall) -> Ret {
    let name = text_arg(vm, &c, 1).unwrap_or_default();
    let out: String = name.to_lowercase().chars().map(fold_accent).collect();
    Ok(vec![vm.str(&out)])
}

fn fold_accent(c: char) -> char {
    match c {
        'à' | 'á' | 'â' | 'ã' | 'ä' | 'å' => 'a',
        'ç' => 'c',
        'è' | 'é' | 'ê' | 'ë' => 'e',
        'ì' | 'í' | 'î' | 'ï' => 'i',
        'ñ' => 'n',
        'ò' | 'ó' | 'ô' | 'õ' | 'ö' | 'ø' => 'o',
        'ù' | 'ú' | 'û' | 'ü' => 'u',
        'ý' | 'ÿ' => 'y',
        other => other,
    }
}

/// `GetGameMessageInfo(index)`: the client's answer where recorded (26:
/// `ERR_ITEM_MAX_COUNT`, no sound kit, voice 30); else the message's name,
/// from the client's `LE_GAME_ERR_*` globals (`LE_GAME_ERR_X` = index
/// gives `ERR_X`), no sound kit or voice (unverified); an unknown index
/// nothing.
fn get_game_message_info(vm: &mut Vm, c: NativeCall) -> Ret {
    if let Some(v) = crate::recorded::lookup(vm, "GetGameMessageInfo", &c) {
        return Ok(v);
    }
    let Some(index) = num_arg(vm, &c, 1) else {
        return Ok(vec![]);
    };
    let name = include_str!("../data/camelot-globals.txt")
        .lines()
        .filter_map(|l| l.split_once('\t'))
        .find(|(name, n)| name.starts_with("LE_GAME_ERR_") && n.parse::<f64>().ok() == Some(index))
        .map(|(name, _)| name.trim_start_matches("LE_GAME_"));
    match name {
        Some(name) => Ok(vec![vm.str(name), Value::Nil, Value::Nil]),
        None => Ok(vec![]),
    }
}

/// `GetMaxLevelForExpansionLevel(expansion)`: 60 for the classic game,
/// 0 for every later expansion, which this client lacks (recorded for 0
/// to 11).
fn get_max_level_for_expansion_level(vm: &mut Vm, c: NativeCall) -> Ret {
    let level = match num_arg(vm, &c, 1) {
        Some(0.0) => 60.0,
        _ => 0.0,
    };
    Ok(vec![Value::Num(level)])
}

/// `GetMaxLevelForLatestExpansion()`, `GetMaxPlayerLevel()` and
/// `GetMaxLevelForPlayerExpansion()`: the flavor's max level from the
/// flavor table (camelot's 60, as the last two recorded; the first
/// unverified, as the classic flavors' 60, 70 and 90).
fn max_level_for_latest_expansion(vm: &mut Vm, _c: NativeCall) -> Ret {
    let level = wowlua_flavors::table()
        .flavor(&crate::engine(vm).flavor)
        .map_or(60, |f| f.max_level);
    Ok(vec![Value::Num(f64::from(level))])
}

/// `GetNumClasses()`: Classic's nine classes (warcraft.wiki.gg: on
/// Classic it is not the highest class ID; unverified on Forever).
fn get_num_classes(_vm: &mut Vm, _c: NativeCall) -> Ret {
    Ok(vec![Value::Num(9.0)])
}

/// `GetRealmID()`, `GetNativeRealmID()`: recorded 4618 (the native realm's
/// unverified).
fn get_realm_id(_vm: &mut Vm, _c: NativeCall) -> Ret {
    Ok(vec![Value::Num(4618.0)])
}

/// `GetWorldElapsedTime(timerID)`: no timer (recorded `"", 0, 0` for 0;
/// unverified for other IDs).
fn get_world_elapsed_time(vm: &mut Vm, _c: NativeCall) -> Ret {
    Ok(vec![vm.str(""), Value::Num(0.0), Value::Num(0.0)])
}

/// `GetNextAvailableRaidTargetMarkerIndex(start, …)`: with no marker in
/// use, the start (recorded 1 from 1, 8 from 8 backwards; unverified with
/// markers placed); a start that is no marker, 0 (the full probe: 0 and
/// 2147483647).
fn next_raid_target_marker(vm: &mut Vm, c: NativeCall) -> Ret {
    let start = num_arg(vm, &c, 1).unwrap_or(1.0).trunc();
    let next = if (1.0..=8.0).contains(&start) {
        start
    } else {
        0.0
    };
    Ok(vec![Value::Num(next)])
}

/// `GetBaseDifficultyID(difficultyID)`: the difficulty itself, taken whole
/// (no difficulty derives from another in the classic game; unverified);
/// 0 for 0, and for a difficulty the game lacks 4294967295, the C code's
/// -1 as its unsigned result (the full probe: 1.5 answers 1, 2147483647
/// that).
fn get_base_difficulty_id(vm: &mut Vm, c: NativeCall) -> Ret {
    let id = num_arg(vm, &c, 1).unwrap_or(0.0).trunc();
    let base = if id == 0.0 || crate::content::difficulty(id).is_some() {
        id
    } else {
        4_294_967_295.0
    };
    Ok(vec![Value::Num(base)])
}

/// `GetTaskInfo(questID)`: not a task quest: not in its area, not on the
/// map, no objectives (from the declaration; unverified).
fn get_task_info(_vm: &mut Vm, _c: NativeCall) -> Ret {
    Ok(vec![
        Value::Bool(false),
        Value::Bool(false),
        Value::Num(0.0),
    ])
}

/// `InGuildParty()`: not a guild group, no guild members, none needed,
/// no bonus (from the declaration; unverified).
fn in_guild_party(_vm: &mut Vm, _c: NativeCall) -> Ret {
    Ok(vec![
        Value::Bool(false),
        Value::Num(0.0),
        Value::Num(0.0),
        Value::Num(0.0),
    ])
}

/// The movies the client has, by ID, and their sizes in bytes: the intro
/// (the full probe: `GetMovieDownloadProgress(1)` false, 5298374 of
/// 5298374, `IsMovieLocal(1)` true; 0, -1 and 2147483647 none; the other
/// IDs unverified).
const MOVIES: &[(f64, f64)] = &[(1.0, 5_298_374.0)];

/// The movie a call names (its first argument, taken whole), if the client
/// has it: its size.
fn movie(vm: &Vm, c: &NativeCall) -> Option<f64> {
    let id = num_arg(vm, c, 1)?.trunc();
    MOVIES.iter().find(|(m, _)| *m == id).map(|(_, size)| *size)
}

/// `GetMovieDownloadProgress(movieID)`: not downloading; a movie at hand
/// all of its size, another nothing of nothing (`MOVIES`).
fn get_movie_download_progress(vm: &mut Vm, c: NativeCall) -> Ret {
    let size = movie(vm, &c).unwrap_or(0.0);
    Ok(vec![Value::Bool(false), Value::Num(size), Value::Num(size)])
}

/// `IsMovieLocal(movieID)`, `IsMoviePlayable`, `IsMovieReadable`: whether
/// the client has the movie (`MOVIES`).
fn is_movie_at_hand(vm: &mut Vm, c: NativeCall) -> Ret {
    Ok(vec![Value::Bool(movie(vm, &c).is_some())])
}

// ------------------------------------------------------------ chat colors

/// `ChangeChatColor(chatType, r, g, b)`: tells `UPDATE_CHAT_COLOR(chatType,
/// r, g, b)`, the event Blizzard's chat frames set `ChatTypeInfo` from
/// (their handler; unverified: when the client sends it).
fn change_chat_color(vm: &mut Vm, c: NativeCall) -> Ret {
    let args = [c.arg(1), c.arg(2), c.arg(3), c.arg(4)];
    widgets::fire_event(vm, "UPDATE_CHAT_COLOR", &args);
    Ok(vec![])
}

/// `SetChatColorNameByClass(chatType, colorNameByClass)`: tells
/// `UPDATE_CHAT_COLOR_NAME_BY_CLASS(chatType, colorNameByClass)` (the chat
/// frames' handler; unverified as above).
fn set_chat_color_name_by_class(vm: &mut Vm, c: NativeCall) -> Ret {
    let args = [c.arg(1), c.arg(2)];
    widgets::fire_event(vm, "UPDATE_CHAT_COLOR_NAME_BY_CLASS", &args);
    Ok(vec![])
}

// ------------------------------------------------------------ kept settings

/// A getter and setter pair over one kept value: the setter's first
/// argument kept, the getter's answer it or the default. Defaults are
/// from the names and warcraft.wiki.gg (unverified).
macro_rules! setting {
    ($getter:ident, $setter:ident, $key:literal, $default:expr) => {
        fn $getter(vm: &mut Vm, _c: NativeCall) -> Ret {
            Ok(vec![get_or(vm, $key, $default)])
        }

        fn $setter(vm: &mut Vm, c: NativeCall) -> Ret {
            let v = vm.hidden(c.arg(1));
            put(vm, $key, v);
            Ok(vec![])
        }
    };
    // A flag: the setter keeps its argument's truth (the full probe:
    // `SetOptOutOfLoot("x")` and `(0)` read back true, `()` false).
    (truth $getter:ident, $setter:ident, $key:literal, $default:expr) => {
        fn $getter(vm: &mut Vm, _c: NativeCall) -> Ret {
            Ok(vec![get_or(vm, $key, $default)])
        }

        fn $setter(vm: &mut Vm, c: NativeCall) -> Ret {
            let v = Value::Bool(vm.hidden(c.arg(1)).truthy());
            put(vm, $key, v);
            Ok(vec![])
        }
    };
    // A setting the server keeps: the setter asks it, the getter reads
    // the same until it answers (the full probe: read back unchanged right
    // after; unverified when the change lands).
    (server $getter:ident, $setter:ident, $key:literal, $default:expr) => {
        fn $getter(vm: &mut Vm, _c: NativeCall) -> Ret {
            Ok(vec![get_or(vm, $key, $default)])
        }

        fn $setter(_vm: &mut Vm, _c: NativeCall) -> Ret {
            Ok(vec![])
        }
    };
}

// The guild bank's open tab, the first (warcraft.wiki.gg).
setting!(
    get_current_guild_bank_tab,
    set_current_guild_bank_tab,
    "guildBankTab",
    Value::Num(1.0)
);
// The encounter journal's tier: none, 0 (recorded, with
// `EJ_GetNumTiers()` 0).
setting!(
    ej_get_current_tier,
    ej_select_tier,
    "ejTier",
    Value::Num(0.0)
);
// The encounter journal's difficulty: none chosen, 0 (the full probe; the
// newest answer, over normal dungeons' 1).
setting!(
    ej_get_difficulty,
    ej_set_difficulty,
    "ejDifficulty",
    Value::Num(0.0)
);
setting!(
    truth get_opt_out_of_loot,
    set_opt_out_of_loot,
    "optOutOfLoot",
    Value::Bool(false)
);
// Unchanged right after `SetAllowLowLevelRaid(true)` (the full probe).
setting!(
    server get_allow_low_level_raid,
    set_allow_low_level_raid,
    "allowLowLevelRaid",
    Value::Bool(false)
);
setting!(
    get_auto_decline_neighborhood_invites,
    set_auto_decline_neighborhood_invites,
    "autoDeclineNeighborhood",
    Value::Bool(false)
);
setting!(
    is_european_numbers,
    set_european_numbers,
    "europeanNumbers",
    Value::Bool(false)
);
setting!(
    get_error_callstack_height,
    set_error_callstack_height,
    "errorCallstackHeight",
    Value::Nil
);
// Allowed: the account's (the full probe; an account that never allowed
// them answers false, unverified).
setting!(
    are_dangerous_scripts_allowed,
    set_allow_dangerous_scripts,
    "dangerousScripts",
    Value::Bool(true)
);
setting!(
    get_selected_display_channel,
    set_selected_display_channel,
    "displayChannel",
    Value::Nil
);
// The guild roster shows offline members: at first (the full probe).
setting!(
    truth get_guild_roster_show_offline,
    set_guild_roster_show_offline,
    "guildRosterShowOffline",
    Value::Bool(true)
);
// None selected: 0 (the full probe).
setting!(
    get_selected_war_game_type,
    set_selected_war_game_type,
    "warGameType",
    Value::Num(0.0)
);
// The helmet and the cloak: not shown, and unchanged right after
// `ShowHelm(true)` (the full probe; unverified whether another character
// shows them).
setting!(
    server showing_helm,
    show_helm,
    "showHelm",
    Value::Bool(false)
);
setting!(
    server showing_cloak,
    show_cloak,
    "showCloak",
    Value::Bool(false)
);

/// `EJ_GetLootFilter()`: the class and specialization the journal's loot
/// is filtered by: the class the `EJLootClass` CVar keeps (the full probe:
/// 2, 0 with it "2"), until `EJ_SetLootFilter`.
fn ej_get_loot_filter(vm: &mut Vm, _c: NativeCall) -> Ret {
    let saved = crate::framescript::cvar_get(vm, "EJLootClass")
        .and_then(|v| v.parse::<f64>().ok())
        .unwrap_or(0.0);
    let class = get_or(vm, "ejLootClass", Value::Num(saved));
    let spec = get_or(vm, "ejLootSpec", Value::Num(0.0));
    Ok(vec![class, spec])
}

fn ej_set_loot_filter(vm: &mut Vm, c: NativeCall) -> Ret {
    let class = vm.hidden(c.arg(1));
    let spec = vm.hidden(c.arg(2));
    put(vm, "ejLootClass", class);
    put(vm, "ejLootSpec", spec);
    Ok(vec![])
}

fn ej_reset_loot_filter(vm: &mut Vm, _c: NativeCall) -> Ret {
    put(vm, "ejLootClass", Value::Nil);
    put(vm, "ejLootSpec", Value::Nil);
    Ok(vec![])
}

/// `LoggingCombat([newState])`, `LoggingChat([newState])`: a boolean given
/// turns logging on or off; the answer is whether it is on (off at first;
/// warcraft.wiki.gg; unverified).
fn logging(vm: &mut Vm, c: &NativeCall, key: &str) -> Ret {
    if let Value::Bool(b) = vm.hidden(c.arg(1)) {
        put(vm, key, Value::Bool(b));
    }
    Ok(vec![get_or(vm, key, Value::Bool(false))])
}

fn logging_combat(vm: &mut Vm, c: NativeCall) -> Ret {
    logging(vm, &c, "loggingCombat")
}

fn logging_chat(vm: &mut Vm, c: NativeCall) -> Ret {
    logging(vm, &c, "loggingChat")
}

/// `GetSheathState()`: 1 with the weapons sheathed, as a character stands
/// out of combat, 2 drawn after `ToggleSheath()` (Blizzard's
/// `ModelSceneUtil` reads 1 as sheathed; unverified: the client draws them
/// in combat).
fn get_sheath_state(vm: &mut Vm, _c: NativeCall) -> Ret {
    Ok(vec![get_or(vm, "sheath", Value::Num(1.0))])
}

fn toggle_sheath(vm: &mut Vm, _c: NativeCall) -> Ret {
    let drawn = matches!(get(vm, "sheath"), Value::Num(n) if n == 2.0);
    put(vm, "sheath", Value::Num(if drawn { 1.0 } else { 2.0 }));
    Ok(vec![])
}

/// `ToggleSelfHighlight()`: the player's highlight switched; whether it is
/// now on (off at first; the documentation's `enabled`; unverified).
fn toggle_self_highlight(vm: &mut Vm, _c: NativeCall) -> Ret {
    let on = !get(vm, "selfHighlight").truthy();
    put(vm, "selfHighlight", Value::Bool(on));
    Ok(vec![Value::Bool(on)])
}

// ------------------------------------------------------------ the lists

/// Actions and requests the engine has nothing to do for: no visible
/// state of its own changes (camera moves, requests to the server whose
/// answers it does not model, systems Forever lacks, windows' settings
/// kept by the client), so they accept and do nothing (from their names
/// and declarations; unverified that no event follows).
const NOTHING: &[&str] = &[
    // Loot, groups, duels, trade.
    "SetLootSpecialization",
    "AcceptGroup",
    "DeclineGroup",
    "ConfirmBNRequestInviteFriend",
    "RespondToInviteConfirmation",
    "GiveMasterLoot",
    "DoMasterLootRoll",
    "CancelMasterLootRoll",
    "LootMoneyNotify",
    "SetLootPortrait",
    "StartDuel",
    "AcceptDuel",
    "CancelDuel",
    "ForfeitDuel",
    "BeginTrade",
    "CancelTrade",
    "AcceptTrade",
    "CancelTradeAccept",
    "ClickTradeButton",
    "ClickTargetTradeButton",
    "SetTradeCurrency",
    "PlaceRaidMarker",
    "ClearRaidMarker",
    "RemoveRaidTargets",
    "RespondInstanceLock",
    "SetSavedInstanceExtend",
    // The client window, the console, profiling.
    "FlashClientIcon",
    "LaunchURL",
    "LoadURLIndex",
    "ResetCPUUsage",
    "UpdateAddOnCPUUsage",
    "ConsolePrintAllMatchingCommands",
    "SetConsoleKey",
    "RestartGx",
    "UpdateWindow",
    "ToggleWindowed",
    "AutoChooseCurrentGraphicsSetting",
    "SetCurrentGraphicsSetting",
    "SetSelectedScreenResolutionIndex",
    "SetGamePadCursorControl",
    "SetGamePadFreeLook",
    "SetPreferredGamepadInteractTarget",
    "DetectWowMouse",
    "SetInWorldUIVisibility",
    "ProcessExceptionClient",
    "ReportBug",
    "ReportSuggestion",
    "ForceLogout",
    "Stuck",
    "SendSubscriptionInterstitialResponse",
    "HandleAtlasMemberCommand",
    "ClearOutage",
    "SetPOIIconOverlapDistance",
    "SetPOIIconOverlapPushDistance",
    "SetTaxiBenchmarkMode",
    "RequestBottomLeftActionBar",
    "NeutralPlayerSelectFaction",
    // The world map's action button (none in the classic game).
    "ClickWorldMapActionButton",
    "UseWorldMapActionButtonSpellOnQuest",
    // Chat windows and channels (their settings kept: `chatwindows.rs`).
    "RedockChatWindows",
    "ResetChatWindows",
    "ResetChatColors",
    "ChannelToggleAnnouncements",
    "ChannelSetAllSilent",
    "ChannelSetPartyMemberSilent",
    "DeclineChannelInvite",
    // Tutorials, cinematics, movies, scenes.
    "TriggerTutorial",
    "FlagTutorial",
    "ResetTutorials",
    "ClearTutorials",
    "StopCinematic",
    "CinematicStarted",
    "CinematicFinished",
    "OpeningCinematic",
    "MouseOverrideCinematicDisable",
    "PreloadMovie",
    "CancelPreloadingMovie",
    "CancelScene",
    // The encounter journal (not in the classic game).
    "EJ_SelectInstance",
    "EJ_SelectEncounter",
    "EJ_SetSearch",
    "EJ_ClearSearch",
    "EJ_EndSearch",
    // Guild banks, guild management, petitions (the engine's guild has
    // no bank, ranks or roster).
    "QueryGuildBankTab",
    "QueryGuildBankLog",
    "QueryGuildBankText",
    "PickupGuildBankItem",
    "SplitGuildBankItem",
    "AutoStoreGuildBankItem",
    "DepositGuildBankMoney",
    "WithdrawGuildBankMoney",
    "PickupGuildBankMoney",
    "BuyGuildBankTab",
    "CloseGuildBankFrame",
    "SetGuildBankTabInfo",
    "SetGuildBankTabItemWithdraw",
    "SetGuildBankTabPermissions",
    "SetGuildBankText",
    "SetGuildBankWithdrawGoldLimit",
    "QueryGuildNews",
    "QueryGuildRecipes",
    "QueryGuildEventLog",
    "RequestGuildChallengeInfo",
    "RequestGuildPartyState",
    "RequestGuildRewards",
    "SetGuildNewsFilter",
    "SetGuildInfoText",
    "SetGuildMemberRank",
    "SetGuildTradeSkillCategoryFilter",
    "SetGuildTradeSkillItemNameFilter",
    "SortGuildTradeSkill",
    "CollapseGuildTradeSkillHeader",
    "ExpandGuildTradeSkillHeader",
    "CloseGuildRoster",
    "CloseGuildRegistrar",
    "ViewGuildRecipes",
    "ReplaceGuildMaster",
    "SubmitRequiredGuildRename",
    "TurnInGuildCharter",
    "BuyGuildCharter",
    "AcceptGuild",
    "DeclineGuild",
    "SetAutoDeclineGuildInvites",
    "SetAllowRecentAlliesSeeLocation",
    "RenamePetition",
    "SignPetition",
    "OfferPetition",
    "ClosePetition",
    // Looking for group, battlegrounds, arenas, war games.
    "JoinLFG",
    "LeaveLFG",
    "JoinSingleLFG",
    "LeaveSingleLFG",
    "ClearAllLFGDungeons",
    "CompleteLFGRoleCheck",
    "CompleteLFGReadyCheck",
    "AcceptProposal",
    "RejectProposal",
    "PartyLFGStartBackfill",
    "RefreshLFGList",
    "RequestLFDPartyLockInfo",
    "RequestLFDPlayerLockInfo",
    "SearchLFGJoin",
    "SearchLFGLeave",
    "SearchLFGSort",
    "JoinArena",
    "JoinRatedBattlefield",
    "JoinRatedSoloShuffle",
    "JoinSkirmish",
    "RequeueSkirmish",
    "SurrenderArena",
    "StartWarGame",
    "StartWarGameByName",
    "StartSoloShuffleWarGameByName",
    "StartSpectatorWarGame",
    "StartSpectatorSoloShuffleWarGame",
    "WarGameRespond",
    "UpdateWarGamesList",
    "CollapseWarGameHeader",
    "ExpandWarGameHeader",
    "BattlefieldMgrQueueInviteResponse",
    "BattlefieldMgrEntryInviteResponse",
    "BattlefieldMgrExitRequest",
    "BattlefieldMgrQueueRequest",
    "RequestRatedInfo",
    "RequestPVPRewards",
    "RequestPVPOptionsEnabled",
    "RequestRandomBattlegroundInstanceInfo",
    "RequestBattlegroundInstanceInfo",
    "RequestBattlefieldScoreData",
    "SetBattlefieldScoreFaction",
    "SortBattlefieldScoreData",
    "ReportPlayerIsPVPAFK",
    "SetPVPRoles",
    "ClearBattlemaster",
    // Achievements' comparison.
    "ClearAchievementComparisonUnit",
    "SetFocusedAchievement",
    "SwitchAchievementSearchTab",
    "ClearAchievementSearchString",
    // Archaeology, artifacts, glyphs, PvP talents, specializations
    // (not in the classic game).
    "SolveArtifact",
    "CloseResearch",
    "SetSelectedArtifact",
    "AttachGlyphToSpell",
    "LearnPvpTalent",
    "LearnPvpTalents",
    "RemovePvpTalent",
    "ClearFailedPVPTalentIDs",
    "ClearFailedTalentIDs",
    "UnlearnSpecialization",
    "ConfirmTalentWipe",
    // Quests' pop-ups, sounds and map icons, the greeting's choices.
    "AddAutoQuestPopUp",
    "RemoveAutoQuestPopUp",
    "AcknowledgeAutoAcceptQuest",
    "PlayAutoAcceptQuestSound",
    "ClearAutoAcceptQuestSound",
    "QuestMapUpdateAllQuests",
    "QuestPOIUpdateIcons",
    "ProcessQuestLogRewardFactions",
    "SortQuests",
    "SortQuestSortTypes",
    "QuestChooseRewardError",
    "SelectActiveQuest",
    "SelectAvailableQuest",
    // Death, spirit healers, graveyards.
    "AcceptAreaSpiritHeal",
    "CancelAreaSpiritHeal",
    "HearthAndResurrectFromArea",
    "SetCemeteryPreference",
    "PortGraveyard",
    // Vehicles (not in the classic game).
    "VehicleExit",
    "EjectPassengerFromSeat",
    "VehicleNextSeat",
    "VehiclePrevSeat",
    "VehicleAimDecrement",
    "VehicleAimIncrement",
    "VehicleAimDownStart",
    "VehicleAimDownStop",
    "VehicleAimUpStart",
    "VehicleAimUpStop",
    "VehicleAimRequestAngle",
    "VehicleAimSetNormPower",
    "CancelPetPossess",
    // Items: pending equips and bind conversions, sockets, item text.
    "EquipPendingItem",
    "CancelPendingEquip",
    "ClearPendingBindConversionItem",
    "ConvertItemToBindToAccount",
    "SocketInventoryItem",
    "UpdateInventoryAlertStatus",
    "CloseItemText",
    "ItemTextNextPage",
    "ItemTextPrevPage",
    // Macros' icons, spell confirmation prompts, surveys and tickets.
    "SetMacroItem",
    "SetMacroSpell",
    "AcceptSpellConfirmationPrompt",
    "DeclineSpellConfirmationPrompt",
    "AcknowledgeSurvey",
    "GMSurveyAnswerSubmit",
    "GMSurveyCommentSubmit",
    "GMSurveySubmit",
    "GMReportLag",
    "GMRequestPlayerInfo",
    "GMResponseResolve",
    "DeleteGMTicket",
];

/// "No" in the classic game or with nothing modeled: no vehicle, no
/// pending glyph (recorded false), no new boost (recorded false), not PvP
/// inactive (recorded false), no timerunning, no flying, no gamepad, no
/// guild rank to edit with, no LFG or war games, no archaeology, no
/// cinematic (from the names and Blizzard's documentation; the rest
/// unverified).
const NO: &[&str] = &[
    "PlayerIsTimerunning",
    "PlayerVehicleHasComboPoints",
    "PlayerIsPVPInactive",
    "PlayerIsSpellTarget",
    "IsCharacterNewlyBoosted",
    "HasPendingGlyphCast",
    "IsPendingGlyphRemoval",
    "IsCastingGlyph",
    "IsFlyableArea",
    "IsAdvancedFlyableArea",
    "IsDrivableArea",
    "IsSubmerged",
    "IsOutOfBounds",
    "IsUsingGamepad",
    "CanGamePadControlCursor",
    "IsGamePadCursorControlEnabled",
    "IsGamePadFreelookEnabled",
    "IsLeftMetaKeyDown",
    "IsRightMetaKeyDown",
    "IsUsingFixedTimeStep",
    "IsDebugBuild",
    "IsPublicTestClient",
    "IsOnTournamentRealm",
    "IsExpansionTrial",
    "IsDemonHunterAvailable",
    "CanUpgradeToCurrentExpansion",
    "ShouldShowExpansionUpgradeBanner",
    "ShouldShowSpecialSplashScreen",
    "SplashFrameCanBeShown",
    "ShouldShowIslandsWeeklyPOI",
    "ShouldDisplayStoryModeTooltipInSocialFrame",
    "RegionalUniqueNamesEnabled",
    "ConsoleIsActive",
    "IsChatAFK",
    "IsChatDND",
    "IsRaidMarkerActive",
    "IsRaidMarkerSystemEnabled",
    "IsArenaSkirmish",
    "IsWargame",
    "IsPartyWorldPVP",
    "IsServerControlledBackfill",
    "CanPartyLFGBackfill",
    "CanJoinBattlefieldAsGroup",
    "DungeonAppearsInRandomLFD",
    "InitiateRolePoll",
    "SortBGList",
    "IsInJailersTower",
    "IsInsane",
    "IsInAuthenticatedRank",
    "CanEditGuildEvent",
    "CanReplaceGuildMaster",
    "CanViewGuildRecipes",
    "SetAchievementComparisonUnit",
    "IsMasterLooter",
    "CanMapChangeDifficulty",
    "EJ_IsValidInstanceDifficulty",
    "EJ_InstanceIsRaid",
    "FlyoutHasSpell",
    "TargetSpellReplacesBonusTree",
    "HasLootSpecializations",
    "AreTalentsLocked",
    "CheckTalentMasterDist",
    "CanResetTutorials",
    "AreClassRolesSoftSuggestions",
    "CanShowSetRoleButton",
    "CanSolveArtifact",
    "CanScanResearchSite",
    "ItemAddedToArtifact",
    "RemoveItemFromArtifact",
    "CanItemBeSocketedToArtifact",
    "IsInCinematicScene",
    "CanCancelScene",
    "IsUsingVehicleControls",
    "CanSwitchVehicleSeat",
    "CanSwitchVehicleSeats",
    "CanEjectPassengerFromSeat",
    "IsVehicleAimAngleAdjustable",
    "IsVehicleAimPowerAdjustable",
    "HasDualWieldPenalty",
    "HasIgnoreDualWieldWeapon",
    "HasAPEffectsSpellPower",
    "HasSPEffectsAttackPower",
    "HasKey",
    "ReagentsFromBankAllowed",
    "DoesCurrentLocaleSellExpansionLevels",
    "ItemCanTargetGarrisonFollowerAbility",
    "CannotBeResurrected",
    "IsCemeterySelectionAvailable",
    "CanPortGraveyard",
    "IsBreadcrumbQuest",
    "IsStoryQuest",
    "QuestIsDaily",
    "QuestIsWeekly",
    "QuestGetAutoLaunched",
    "IsActiveQuestTrivial",
    "IsAvailableQuestTrivial",
    "ItemTextHasNextPage",
    "ItemTextIsFullPage",
    "GMEuropaBugsEnabled",
    "GMEuropaComplaintsEnabled",
    "GMEuropaSuggestionsEnabled",
    "GMEuropaTicketsEnabled",
    "GMItemRestorationButtonEnabled",
    "GMQuickTicketSystemEnabled",
    "GMQuickTicketSystemThrottled",
];

/// "Yes": the cursor shown (recorded true), a 64-bit client, the mouse in
/// use, quest data at hand (the engine's quests carry theirs; from the
/// names, unverified but the cursor).
const YES: &[&str] = &["IsCursorVisible", "Is64BitClient", "IsUsingMouse"];

/// Counts and amounts of nothing: no LFG dungeons, scenarios, war games
/// or battleground types listed; no archaeology; no unspent PvP talents;
/// no CPU profiled; no corruption, PvP power, bandwidth, billing time,
/// eviction or boot timers; the classic game's expansion levels (0, as
/// `GetClassicExpansionLevel` and `GetExpansionLevel` recorded); no
/// declension sets for enUS (from the names and documentation; the rest
/// unverified).
const ZERO: &[&str] = &[
    "GetNumRFDungeons",
    "GetNumRandomScenarios",
    "GetNumScenarios",
    "GetNumFlexRaidDungeons",
    "GetNumDungeonForRandomSlot",
    "GetLFDLockPlayerCount",
    "GetNumWarGameTypes",
    "GetNumBattlegroundTypes",
    "GetNumTreasurePickerItems",
    "GetNumArchaeologyRaces",
    "GetNumArtifactsByRace",
    "ArchaeologyMapUpdateAll",
    "ArcheologyGetVisibleBlobID",
    "GetNumUnspentPvpTalents",
    "GetNumPetitionNames",
    "GetNumDeclensionSets",
    "GetAddOnCPUUsage",
    "GetScriptCPUUsage",
    "GetCollapsingStarCost",
    "GetCorruption",
    "GetCorruptionResistance",
    "GetPvpPowerDamage",
    "GetPvpPowerHealing",
    "GetAvailableBandwidth",
    "GetBillingTimeRested",
    "GetEvictionTimeRemaining",
    "GetInstanceBootTimeRemaining",
    "GetAreaSpiritHealerTime",
    "GetReadyCheckTimeLeft",
    "GetJailersTowerLevel",
    "GetPreviousArenaSeason",
    "GetDailyQuestsCompleted",
    "GetMaxRewardCurrencies",
    "GetNumActiveQuests",
    "GetNumAvailableQuests",
    "EJ_GetNumTiers",
    "EJ_GetNumSearchResults",
    "EJ_GetSearchSize",
    "EJ_GetSearchProgress",
    "EJ_GetNumLoot",
    "EJ_GetNumEncountersForLootByIndex",
    "EJ_GetContentTuningID",
    "EJ_GetInstanceForMap",
    "GMSurveyNumAnswers",
    "VehicleAimGetNormPower",
    "ItemTextGetPage",
];

/// No such thing: an unknown index's or ID's info, no running macro, no
/// glyph, no tutorial next or before, no invite to confirm, no best
/// choice of an LFG list, no window made, no timerunning season (nil, as
/// the declarations' nilable results or warcraft.wiki.gg have it for
/// none; unverified).
const NIL: &[&str] = &[
    "CreateWindow",
    "PlayerGetTimerunningSeasonID",
    "GetRFDungeonInfo",
    "GetRandomScenarioInfo",
    "GetFlexRaidDungeonInfo",
    "GetDungeonForRandomSlot",
    "GetLFDLockInfo",
    "GetLFDRoleLockInfo",
    "GetWarGameTypeInfo",
    "GetWarGameQueueStatus",
    "GetTreasurePickerItemInfo",
    "SearchLFGGetResults",
    "SearchLFGGetPartyResults",
    "SearchLFGGetEncounterResults",
    "SearchLFGGetJoinedID",
    "GetPartyLFGBackfillInfo",
    "GetBestRFChoice",
    "GetRandomScenarioBestChoice",
    "GetBestFlexRaidChoice",
    "GetSavedWorldBossInfo",
    "GetInviteConfirmationInfo",
    "GetNextPendingInviteConfirmation",
    "GetCallPetSpellInfo",
    "GetPvpTalentInfoByID",
    "GetPvpTalentInfoBySpecialization",
    "GetPvpTalentLink",
    "GetCurrentGlyphNameForSpell",
    "GetPendingGlyphName",
    "GetArchaeologyRaceInfo",
    "GetArchaeologyRaceInfoByID",
    "GetArchaeologyInfo",
    "GetArtifactInfoByRace",
    "GetActiveArtifactByRace",
    "GetSelectedArtifactInfo",
    "ArchaeologyGetIconInfo",
    "GetAutoQuestPopUp",
    "GetActiveTitle",
    "GetAvailableTitle",
    "GetActiveLevel",
    "GetAvailableLevel",
    "GetActiveQuestID",
    "GetAvailableQuestInfo",
    "GetRunningMacro",
    "GetRunningMacroButton",
    "GetTargetTradeCurrency",
    "GetCurrentLevelFeatures",
    "GetCurrentEventID",
    "GetSecondsUntilParentalControlsKick",
    "GetMirrorTimerProgress",
    "GetSoundEntryCount",
    "GetCurrentCombatTextEventInfo",
    "ItemTextGetItem",
    "ItemTextGetText",
    "ItemTextGetCreator",
    "ItemTextGetMaterial",
    "GMSurveyQuestion",
    // No world map action button, no garrison followers, no
    // specializations.
    "GetWorldMapActionButtonSpellInfo",
    "GetFollowerTypeIDFromSpell",
    "GetJournalInfoForSpellConfirmation",
    "EJ_GetInstanceByIndex",
    "EJ_GetInstanceInfo",
    "EJ_GetEncounterInfo",
    "EJ_GetEncounterInfoByIndex",
    "EJ_GetCreatureInfo",
    "EJ_GetMapEncounter",
    "EJ_GetSearchResult",
    "EJ_GetSectionPath",
    "EJ_HandleLinkPath",
];

/// Results that do not apply say nothing (the documentation marks them
/// `MayReturnNothing`: no event profiled, no source location, no vehicle
/// indicator, no unit of that creature near; unverified).
const RETURNS_NOTHING: &[&str] = &[
    "GetEventTime",
    "GetVehicleUIIndicator",
    "GetVehicleUIIndicatorSeat",
    "ClosestUnitPosition",
    "ClosestGameObjectPosition",
];

/// Two zeros: no CPU used and no calls profiled; no comparison
/// achievements; no LFG search results; no battleground points (the
/// declarations' shapes; unverified).
const TWO_ZEROS: &[&str] = &[
    "GetEventCPUUsage",
    "GetFrameCPUUsage",
    "SearchLFGGetNumResults",
    "GetBattlegroundPoints",
];

/// Empty lists: no failed talents, no credits, no corruption effects, no
/// pending invite confirmations (the documentation's table results;
/// unverified).
const EMPTY_TABLE: &[&str] = &[
    "GetFailedPVPTalentIDs",
    "GetFailedTalentIDs",
    "GetCreditsDisplayInfo",
    "GetNegativeCorruptionEffectInfo",
    "GetPendingInviteConfirmations",
];

/// The LFG lists' order and state functions: the caller's table, nothing
/// added (no dungeons listed; Blizzard's code passes the table to fill;
/// unverified).
const PASSED_TABLE: &[&str] = &[
    "GetLFDChoiceOrder",
    "GetLFDChoiceCollapseState",
    "GetLFDChoiceEnabledState",
    "GetLFRChoiceOrder",
    "GetScenariosChoiceOrder",
    // The macro icons from loose files: none (the full probe: `{}`).
    "GetLooseMacroIcons",
    "GetLooseMacroItemIcons",
    "GetHomePartyInfo",
];

/// `SetLegacyRaidDifficultyID(difficultyID)`: the legacy raid difficulty
/// `GetLegacyRaidDifficultyID` tells (`content`; unverified: no event).
fn set_legacy_raid_difficulty_id(vm: &mut Vm, c: NativeCall) -> Ret {
    if let Value::Num(id) = c.arg(1) {
        crate::content::set_legacy_raid_difficulty(vm, id);
    }
    Ok(vec![])
}

pub fn behaviours() -> Vec<(&'static str, NativeFn)> {
    let mut out: Vec<(&'static str, NativeFn)> = vec![
        ("TableIsEmpty", table_is_empty),
        ("GetExpansionLevel", get_expansion_level),
        ("GetServerExpansionLevel", expansion_info),
        ("GetClassicExpansionLevel", expansion_info),
        ("GetAccountExpansionLevel", expansion_info),
        ("GetMaximumExpansionLevel", expansion_info),
        ("GetMinimumExpansionLevel", expansion_info),
        ("C_ClassColor.GetClassColor", get_class_color),
        ("SetLegacyRaidDifficultyID", set_legacy_raid_difficulty_id),
        ("CountTable", count_table),
        ("GetKeysArray", get_keys_array),
        ("GetValuesArray", get_values_array),
        ("tContains", t_contains),
        ("tIndexOf", t_index_of),
        ("tDeleteItem", t_delete_item),
        ("tUnorderedRemove", t_unordered_remove),
        ("IsDualWielding", is_dual_wielding),
        ("IsRangedWeapon", is_ranged_weapon),
        ("HasWandEquipped", has_wand_equipped),
        ("CanDualWield", can_dual_wield),
        ("IsUnitModelReadyForUI", is_unit_model_ready),
        ("PlayerIsInCombat", player_is_in_combat),
        ("IsOutdoors", is_outdoors),
        ("GroupHasOfflineMember", group_has_offline_member),
        ("PetHasSpellbook", has_pet),
        ("PetCanBeDismissed", has_pet),
        ("CanLootUnit", can_loot_unit),
        ("GetDefaultLanguage", get_default_language),
        ("GetLanguageByIndex", get_language_by_index),
        ("GetAlternativeDefaultLanguage", get_default_language),
        ("SelectedRealmName", selected_realm_name),
        ("GetLocalGameTime", get_local_game_time),
        ("GetResSicknessDuration", get_res_sickness_duration),
        ("GetGuildRosterShowOffline", get_guild_roster_show_offline),
        ("SetGuildRosterShowOffline", set_guild_roster_show_offline),
        ("GetMouseButtonName", get_mouse_button_name),
        ("GetDefaultScale", get_default_scale),
        ("GetSchoolString", get_school_string),
        ("IsOutlineModeSupported", is_outline_mode_supported),
        ("DoesTemplateExist", does_template_exist),
        ("GetClickFrame", get_click_frame),
        ("RunBinding", run_binding),
        (
            "CreateUnitHealPredictionCalculator",
            create_heal_prediction_calculator,
        ),
        ("CreateFontFamily", create_font_family),
        ("CopyToClipboard", copy_to_clipboard),
        (
            "CalculateStringEditDistance",
            calculate_string_edit_distance,
        ),
        ("CaseAccentInsensitiveParse", case_accent_insensitive_parse),
        ("GetGameMessageInfo", get_game_message_info),
        (
            "GetMaxLevelForExpansionLevel",
            get_max_level_for_expansion_level,
        ),
        (
            "GetMaxLevelForLatestExpansion",
            max_level_for_latest_expansion,
        ),
        ("GetMaxPlayerLevel", max_level_for_latest_expansion),
        (
            "GetMaxLevelForPlayerExpansion",
            max_level_for_latest_expansion,
        ),
        ("GetNumClasses", get_num_classes),
        ("GetRealmID", get_realm_id),
        ("GetNativeRealmID", get_realm_id),
        ("GetWorldElapsedTime", get_world_elapsed_time),
        (
            "GetNextAvailableRaidTargetMarkerIndex",
            next_raid_target_marker,
        ),
        ("GetBaseDifficultyID", get_base_difficulty_id),
        ("GetTaskInfo", get_task_info),
        ("InGuildParty", in_guild_party),
        ("GetMovieDownloadProgress", get_movie_download_progress),
        ("ChangeChatColor", change_chat_color),
        ("SetChatColorNameByClass", set_chat_color_name_by_class),
        ("GetCurrentGuildBankTab", get_current_guild_bank_tab),
        ("SetCurrentGuildBankTab", set_current_guild_bank_tab),
        ("EJ_GetCurrentTier", ej_get_current_tier),
        ("EJ_SelectTier", ej_select_tier),
        ("EJ_GetDifficulty", ej_get_difficulty),
        ("EJ_SetDifficulty", ej_set_difficulty),
        ("EJ_GetLootFilter", ej_get_loot_filter),
        ("EJ_SetLootFilter", ej_set_loot_filter),
        ("EJ_ResetLootFilter", ej_reset_loot_filter),
        ("GetOptOutOfLoot", get_opt_out_of_loot),
        ("SetOptOutOfLoot", set_opt_out_of_loot),
        ("GetAllowLowLevelRaid", get_allow_low_level_raid),
        ("SetAllowLowLevelRaid", set_allow_low_level_raid),
        (
            "GetAutoDeclineNeighborhoodInvites",
            get_auto_decline_neighborhood_invites,
        ),
        (
            "SetAutoDeclineNeighborhoodInvites",
            set_auto_decline_neighborhood_invites,
        ),
        ("IsEuropeanNumbers", is_european_numbers),
        ("SetEuropeanNumbers", set_european_numbers),
        ("GetErrorCallstackHeight", get_error_callstack_height),
        ("SetErrorCallstackHeight", set_error_callstack_height),
        ("AreDangerousScriptsAllowed", are_dangerous_scripts_allowed),
        ("SetAllowDangerousScripts", set_allow_dangerous_scripts),
        ("GetSelectedDisplayChannel", get_selected_display_channel),
        ("SetSelectedDisplayChannel", set_selected_display_channel),
        ("SetSelectedWarGameType", set_selected_war_game_type),
        ("ShowingHelm", showing_helm),
        ("ShowHelm", show_helm),
        ("ShowingCloak", showing_cloak),
        ("ShowCloak", show_cloak),
        ("LoggingCombat", logging_combat),
        ("LoggingChat", logging_chat),
        ("GetSheathState", get_sheath_state),
        ("ToggleSheath", toggle_sheath),
        ("ToggleSelfHighlight", toggle_self_highlight),
        // The quest greeting's text: none (unverified).
        ("GetGreetingText", empty_string),
        // No cinematic playing: no summary (unverified).
        ("GetCurrentCinematicSummary", empty_string),
        // The engine's locale (`GetLocale`): enUS (unverified).
        ("GetOSLocale", os_locale),
        // A comparison of nothing, an inspected player's honor of none
        // (the declarations' shapes; unverified).
        ("GetInspectHonorData", six_zeros),
        ("GetArtifactProgress", three_zeros),
        ("CanChangePlayerDifficulty", two_falses),
        ("EJ_IsSearchFinished", yes),
        ("GetNumTotemSlots", get_num_totem_slots),
        ("GetNumFlyouts", get_num_flyouts),
        ("GetSelectedWarGameType", get_selected_war_game_type),
    ];
    let groups: [(&[&str], NativeFn); 9] = [
        (NOTHING, nothing),
        (NO, no),
        (YES, yes),
        (ZERO, zero),
        (NIL, nil),
        (RETURNS_NOTHING, nothing),
        (TWO_ZEROS, two_zeros),
        (EMPTY_TABLE, empty_table),
        (PASSED_TABLE, passed_table),
    ];
    for (names, f) in groups {
        out.extend(names.iter().map(|name| (*name, f)));
    }
    // The rules the full probe settled, over the lists above.
    out.extend(probed());
    out
}

/// `GetNumTotemSlots()`: 8 (recorded for a paladin, a class without
/// totems: the client's slot count, not the class's; Blizzard's cooldown
/// viewer loops over them).
pub(crate) const TOTEM_SLOTS: f64 = 8.0;

fn get_num_totem_slots(_vm: &mut Vm, _c: NativeCall) -> Ret {
    Ok(vec![Value::Num(TOTEM_SLOTS)])
}

/// The global twin of `C_Flyout.GetNumFlyouts()`: its answer, else none
/// (unverified).
fn get_num_flyouts(vm: &mut Vm, _c: NativeCall) -> Ret {
    match ask(vm, "C_Flyout.GetNumFlyouts", &[]).first().copied() {
        Some(v @ Value::Num(_)) => Ok(vec![v]),
        _ => Ok(vec![Value::Num(0.0)]),
    }
}

/// `GetOSLocale()`: enUS, the engine's locale (unverified).
fn os_locale(vm: &mut Vm, _c: NativeCall) -> Ret {
    Ok(vec![vm.str("enUS")])
}

// ------------------------------------------------------- the full probe

/// Whether the first argument is a number of at least 1: legacy functions
/// answer nothing for an ID or index below (the full probe: -1 and 0).
fn positive(vm: &Vm, c: &NativeCall) -> bool {
    num_arg(vm, c, 1).is_some_and(|n| n >= 1.0)
}

/// `GetLFDRoleRestrictions(dungeonID)`: no role restricted for a dungeon,
/// three trues (the full probe, IDs 1 and 2; unverified for a dungeon
/// that restricts one); nothing for no ID.
fn lfd_role_restrictions(vm: &mut Vm, c: NativeCall) -> Ret {
    if !positive(vm, &c) {
        return Ok(vec![]);
    }
    Ok(vec![Value::Bool(true); 3])
}

/// `EJ_GetInvTypeSortOrder(invType)`: nothing for a type below 1 (the full
/// probe; the types' orders, 1's 0 and 2's 10, unverified for the others:
/// 0).
fn ej_inv_type_sort_order(vm: &mut Vm, c: NativeCall) -> Ret {
    if !positive(vm, &c) {
        return Ok(vec![]);
    }
    let order = if num_arg(vm, &c, 1) == Some(2.0) {
        10.0
    } else {
        0.0
    };
    Ok(vec![Value::Num(order)])
}

/// `IsChatChannelRaid(index)`, `IsSelectedSpellBookItem(slot, bank)`'s like:
/// false for a number, nothing for another argument or none (the full
/// probe).
fn false_for_number(vm: &mut Vm, c: NativeCall) -> Ret {
    Ok(match vm.hidden(c.arg(1)) {
        Value::Num(_) => vec![Value::Bool(false)],
        _ => vec![],
    })
}

/// `IsSelectedSpellBookItem(slot, bank)`: none selected, false; with no
/// bank nothing (the full probe: `(1)` nothing, `(1, 1)` and `(1,
/// "spell")` false).
fn is_selected_spell_book_item(vm: &mut Vm, c: NativeCall) -> Ret {
    if c.arg(2).is_nil() {
        return Ok(vec![]);
    }
    false_for_number(vm, c)
}

/// `IsSpellClassOrSpec(slot, bank)`: two nils for a slot and bank, nothing
/// without the bank (the full probe; the spellbook's class spells
/// unverified).
fn is_spell_class_or_spec(_vm: &mut Vm, c: NativeCall) -> Ret {
    if c.arg(2).is_nil() {
        return Ok(vec![]);
    }
    Ok(vec![Value::Nil, Value::Nil])
}

/// `GetAchievementGuildRep(achievementID)`: no guild reputation needed,
/// false alone; nothing for an argument not a number (the full probe; the
/// newest answer, over three values).
fn achievement_guild_rep(vm: &mut Vm, c: NativeCall) -> Ret {
    false_for_number(vm, c)
}

/// `HaveQuestData(questID)`, `HaveQuestRewardData(questID)`: a quest of
/// the player's log has its data, another not yet (the full probe: the
/// log's 1063 true; 1, 2 and the invalid IDs false).
fn have_quest_data(vm: &mut Vm, c: NativeCall) -> Ret {
    let id = c.arg(1);
    let known = num_arg(vm, &c, 1).is_some()
        && ask(vm, "C_QuestLog.GetLogIndexForQuestID", &[id])
            .first()
            .is_some_and(|v| matches!(v, Value::Num(_)));
    Ok(vec![Value::Bool(known)])
}

/// `GetNextCompleatedTutorial(index)` and the previous: none (the full
/// probe: nothing for -1, 0 and 1; 2's 27 unverified, the tutorials'
/// order).
fn no_tutorial(_vm: &mut Vm, _c: NativeCall) -> Ret {
    Ok(vec![])
}

/// `GetPromotionRank(rank)`: none (the full probe: nil, but 2's 3).
fn get_promotion_rank(_vm: &mut Vm, _c: NativeCall) -> Ret {
    Ok(vec![Value::Nil])
}

/// Each class's colour, as Retail 12.1.5's `C_ClassColor.GetClassColor`
/// gives it (`tostring`'s digits, recorded 2026-10-28).
const CLASS_COLORS: &[(&str, [f64; 3])] = &[
    (
        "ADVENTURER",
        [0.66666668653488, 0.82745105028152, 0.44705885648727],
    ),
    (
        "DEATHKNIGHT",
        [0.76862752437592, 0.11764706671238, 0.22745099663734],
    ),
    (
        "DEMONHUNTER",
        [0.63921570777893, 0.18823531270027, 0.78823536634445],
    ),
    ("DRUID", [1.0, 0.48627454042435, 0.039215687662363]),
    (
        "EVOKER",
        [0.20000001788139, 0.57647061347961, 0.49803924560547],
    ),
    (
        "HUNTER",
        [0.66666668653488, 0.82745105028152, 0.44705885648727],
    ),
    (
        "MAGE",
        [0.24705883860588, 0.78039222955704, 0.9215686917305],
    ),
    ("MONK", [0.0, 1.0, 0.59607845544815]),
    (
        "PALADIN",
        [0.95686280727386, 0.54901963472366, 0.7294117808342],
    ),
    ("PRIEST", [1.0, 1.0, 1.0]),
    ("ROGUE", [1.0, 0.95686280727386, 0.4078431725502]),
    ("SHAMAN", [0.0, 0.43921571969986, 0.8666667342186]),
    (
        "TRAVELER",
        [0.40000003576279, 0.60000002384186, 0.60000002384186],
    ),
    (
        "WARLOCK",
        [0.52941179275513, 0.53333336114883, 0.93333339691162],
    ),
    (
        "WARRIOR",
        [0.77647066116333, 0.60784316062927, 0.42745101451874],
    ),
];

/// `C_ClassColor.GetClassColor(className)`: the class's colour (a
/// `ColorMixin`: r, g, b), nothing for a class the client lacks. Recorded
/// the same on Mists 5.5.4, TBC 2.5.6 and Classic Era 1.15.9 but for those
/// classes, and Classic Era's Shaman in the Paladin's pink.
// ponytail: the classes after Mists (Evoker, Adventurer, Traveler) were not
// asked on the classic clients, taken as lacking.
fn get_class_color(vm: &mut Vm, c: NativeCall) -> Ret {
    let Value::Str(s) = c.arg(1) else {
        return Ok(vec![]);
    };
    let class = String::from_utf8_lossy(vm.str_bytes(s)).into_owned();
    let flavor = engine(vm).flavor.clone();
    let lacking: &[&str] = match flavor.as_str() {
        "standard" => &[],
        "mists" => &["DEMONHUNTER", "EVOKER", "ADVENTURER", "TRAVELER"],
        _ => &[
            "DEATHKNIGHT",
            "MONK",
            "DEMONHUNTER",
            "EVOKER",
            "ADVENTURER",
            "TRAVELER",
        ],
    };
    let shown = if flavor == "vanilla" && class == "SHAMAN" {
        "PALADIN"
    } else {
        class.as_str()
    };
    let Some((_, rgb)) = CLASS_COLORS.iter().find(|(name, _)| *name == shown) else {
        return Ok(vec![]);
    };
    if lacking.contains(&class.as_str()) {
        return Ok(vec![]);
    }
    let fields: Vec<(&str, Value)> = ["r", "g", "b"]
        .into_iter()
        .zip(rgb.map(Value::Num))
        .collect();
    Ok(vec![crate::curves::mixin_table(vm, "ColorMixin", &fields)])
}

/// `GetExpansionLevel()`: the flavor's expansion, as an account that has
/// it, as `GetServerExpansionLevel` (recorded: Retail 12.1.5 11, Mists 5.5.4
/// 4, TBC 2.5.6 1, Classic Era 1.15.9 0; camelot's recorded 0 takes this
/// place).
fn get_expansion_level(vm: &mut Vm, _c: NativeCall) -> Ret {
    let level = match engine(vm).flavor.as_str() {
        "tbc" => 1.0,
        "mists" => 4.0,
        "standard" => 11.0,
        _ => 0.0,
    };
    Ok(vec![Value::Num(level)])
}

/// The other expansion getters, as recorded on each client 2026-10-28
/// (Retail 12.1.5, Mists 5.5.4, TBC 2.5.6, Classic Era 1.15.9): the
/// classic clients' account level is 0, their minimum the client's own.
/// `GetServerExpansionLevel` is 0 until the player logs in (the wiki:
/// until after `PLAYER_ENTERING_WORLD`).
// camelot's recordings take this place.
fn expansion_info(vm: &mut Vm, c: NativeCall) -> Ret {
    let name = crate::recorded::called_name(vm, &c).unwrap_or_default();
    let flavor = engine(vm).flavor.as_str();
    // (expansion, account, minimum)
    let (level, account, minimum) = match flavor {
        "standard" => (11.0, 11.0, 10.0),
        "mists" => (4.0, 0.0, 3.0),
        "tbc" => (1.0, 0.0, 5.0),
        "vanilla" => (0.0, 0.0, 5.0),
        _ => (0.0, 0.0, 0.0),
    };
    let v = match name.as_str() {
        "GetServerExpansionLevel" if !engine(vm).world.logged_in => 0.0,
        "GetAccountExpansionLevel" => account,
        "GetMinimumExpansionLevel" => minimum,
        _ => level,
    };
    Ok(vec![Value::Num(v)])
}

/// `GetNumSpecializations([isInspect, isPet])`: the class's one; an
/// inspected unit's or pet's none (the full probe: 1 without arguments, 0
/// with any).
fn get_num_specializations(_vm: &mut Vm, c: NativeCall) -> Ret {
    let n = if c.arg(1).is_nil() { 1.0 } else { 0.0 };
    Ok(vec![Value::Num(n)])
}

/// `GetNumComparisonCompletedAchievements(category)`: with no comparison,
/// the player's own completed count and 0 (the full probe: 112, 0 for
/// any category, the player's `GetNumCompletedAchievements()` 112).
fn comparison_completed_achievements(vm: &mut Vm, _c: NativeCall) -> Ret {
    let own = crate::recorded::lookup_text(vm, "GetNumCompletedAchievements()")
        .and_then(|v| v.first().copied())
        .unwrap_or(Value::Num(0.0));
    Ok(vec![own, Value::Num(0.0)])
}

/// `EJ_IsLootListOutOfDate(...)`: false asked with nothing, true with any
/// argument (the full probe: `()` false, `("x")`, `(-1)`, `(1)` true; why
/// is not known, unverified).
fn ej_is_loot_list_out_of_date(_vm: &mut Vm, c: NativeCall) -> Ret {
    Ok(vec![Value::Bool(c.has(1))])
}

/// `EJ_GetTierInfo(index)`: no tier listed (`EJ_GetNumTiers` 0), so every
/// index refused, its number in the error (the full probe: "Usage:
/// EJ_GetTierInfo(index) Invalid index -1", 0, 1 and 2 alike; a value not
/// a number refused before, `camelot-usages.txt`).
fn ej_get_tier_info(vm: &mut Vm, c: NativeCall) -> Ret {
    let n = vm.tonumber(vm.hidden(c.arg(1))).unwrap_or(0.0).trunc();
    Err(vm.error(&format!(
        "Usage: EJ_GetTierInfo(index) Invalid index {}",
        n as i64
    )))
}

/// `IsLegacyDifficulty(difficultyID)`: false for a difficulty of the game,
/// nil for another, 0 too (the full probe: 1 and 1.5 false, 0 and
/// 2147483647 nil).
fn is_legacy_difficulty(vm: &mut Vm, c: NativeCall) -> Ret {
    let id = num_arg(vm, &c, 1).unwrap_or(0.0).trunc();
    Ok(vec![if crate::content::difficulty(id).is_some() {
        Value::Bool(false)
    } else {
        Value::Nil
    }])
}

/// `IsSpecializationActivateSpell(spellID)`: no spell activates a
/// specialization, but 0 answers true (the full probe: 0 true; -1, 1, 2,
/// 6603 false; why unverified, an empty entry matched).
fn is_specialization_activate_spell(vm: &mut Vm, c: NativeCall) -> Ret {
    Ok(vec![Value::Bool(num_arg(vm, &c, 1) == Some(0.0))])
}

/// `QuestHasPOIInfo(questID)`: a quest of the player's log has its map
/// point, 0 too; another not (the full probe: 1063 of the log and 0 true;
/// -1, 1, 2 false).
fn quest_has_poi_info(vm: &mut Vm, c: NativeCall) -> Ret {
    if num_arg(vm, &c, 1) == Some(0.0) {
        return Ok(vec![Value::Bool(true)]);
    }
    have_quest_data(vm, c)
}

/// `CanInitiateWarGame(index)`: no war game to start, with the war game
/// type's player counts (the full probe: 1 answers false, 1, 10; 2 false,
/// 1, 15; other types unverified: 0, 0 as for 0, -1 and no number).
fn can_initiate_war_game(vm: &mut Vm, c: NativeCall) -> Ret {
    let (min, max) = match num_arg(vm, &c, 1).map(f64::trunc) {
        Some(1.0) => (1.0, 10.0),
        Some(2.0) => (1.0, 15.0),
        _ => (0.0, 0.0),
    };
    Ok(vec![Value::Bool(false), Value::Num(min), Value::Num(max)])
}

/// `GetSourceLocation()`: where the caller is, its chunk's name (a file's
/// path, or the code of a string chunk) and line (the full probe: `return
/// GetSourceLocation()` loaded from a string answers "return
/// GetSourceLocation():1"; a file's form unverified: the path without
/// `@`).
fn get_source_location(vm: &mut Vm, _c: NativeCall) -> Ret {
    let Some(source) = vm.caller_source(1) else {
        return Ok(vec![]);
    };
    let name = match source.first() {
        Some(b'@' | b'=') => &source[1..],
        _ => &source[..],
    };
    // The line, from `chunk:line: `.
    let w = vm.where_(1);
    let text = String::from_utf8_lossy(&w);
    let line = text
        .trim_end_matches(": ")
        .rsplit(':')
        .next()
        .and_then(|l| l.parse::<u32>().ok())
        .unwrap_or(0);
    let mut out = name.to_vec();
    out.extend_from_slice(format!(":{line}").as_bytes());
    Ok(vec![vm.bytes(&out)])
}

/// `GetFunctionCPUUsage(func [, includeSubroutines])`: no CPU used, no
/// calls profiled; asked without a function, the C code's usage error
/// (the full probe: `()`; a value not a function unverified, refused as
/// well).
fn get_function_cpu_usage(vm: &mut Vm, c: NativeCall) -> Ret {
    if !matches!(vm.hidden(c.arg(1)), Value::Func(_)) {
        return Err(vm.error("Usage: GetFunctionCPUUsage(function[, includeSubroutines])"));
    }
    Ok(vec![Value::Num(0.0), Value::Num(0.0)])
}

/// `GetInspectArenaData(bracketIndex)`: an inspected player's arena
/// rating, games and wins of none, five zeros; nothing for a bracket below
/// 1 (the full probe: 1 and 2 zeros, 0 and -1 nothing).
fn get_inspect_arena_data(vm: &mut Vm, c: NativeCall) -> Ret {
    if !positive(vm, &c) {
        return Ok(vec![]);
    }
    Ok(vec![Value::Num(0.0); 5])
}

/// `GetStringFromModifiers(modifiers)`: the keys a modifier mask holds,
/// as a binding writes them (the full probe: 0 "", 1 "SHIFT"; the other
/// bits, a left and a right key each for shift, control and alt as
/// `Enum.ModifierKey` orders them, and their order, unverified).
fn get_string_from_modifiers(vm: &mut Vm, c: NativeCall) -> Ret {
    let mask = num_arg(vm, &c, 1).unwrap_or(0.0) as u64;
    let mut keys = Vec::new();
    for (bits, name) in [
        (0b11_0000, "ALT"),
        (0b00_1100, "CTRL"),
        (0b00_0011, "SHIFT"),
    ] {
        if mask & bits != 0 {
            keys.push(name);
        }
    }
    Ok(vec![vm.str(&keys.join("-"))])
}

/// `GetMonitorName(index)`: the recorded monitors' (1 the system's
/// default, nil and true; 2 a named one; 3 none, nil and false); a
/// monitor index below 1 none, nil and false (the full probe: -1 and 0);
/// no number, nothing.
fn get_monitor_name(vm: &mut Vm, c: NativeCall) -> Ret {
    if let Some(v) = crate::recorded::lookup(vm, "GetMonitorName", &c) {
        return Ok(v);
    }
    if num_arg(vm, &c, 1).is_none() {
        return Ok(vec![]);
    }
    Ok(vec![Value::Nil, Value::Bool(false)])
}

/// The rules the full probe settled (`/stdtest engine fullprobe`).
fn probed() -> Vec<(&'static str, NativeFn)> {
    vec![
        ("GetNumSpecializations", get_num_specializations),
        (
            "GetNumComparisonCompletedAchievements",
            comparison_completed_achievements,
        ),
        ("GetLFDRoleRestrictions", lfd_role_restrictions),
        ("EJ_GetInvTypeSortOrder", ej_inv_type_sort_order),
        ("IsChatChannelRaid", false_for_number),
        ("IsSelectedSpellBookItem", is_selected_spell_book_item),
        ("IsSpellClassOrSpec", is_spell_class_or_spec),
        ("GetAchievementGuildRep", achievement_guild_rep),
        ("HaveQuestData", have_quest_data),
        ("HaveQuestRewardData", have_quest_data),
        ("GetNextCompleatedTutorial", no_tutorial),
        ("GetPrevCompleatedTutorial", no_tutorial),
        ("GetPromotionRank", get_promotion_rank),
        ("EJ_IsLootListOutOfDate", ej_is_loot_list_out_of_date),
        ("EJ_GetTierInfo", ej_get_tier_info),
        ("IsLegacyDifficulty", is_legacy_difficulty),
        (
            "IsSpecializationActivateSpell",
            is_specialization_activate_spell,
        ),
        ("QuestHasPOIInfo", quest_has_poi_info),
        ("CanInitiateWarGame", can_initiate_war_game),
        ("GetSourceLocation", get_source_location),
        ("GetFunctionCPUUsage", get_function_cpu_usage),
        ("GetInspectArenaData", get_inspect_arena_data),
        ("GetStringFromModifiers", get_string_from_modifiers),
        ("GetMonitorName", get_monitor_name),
        ("IsMovieLocal", is_movie_at_hand),
        ("IsMoviePlayable", is_movie_at_hand),
        ("IsMovieReadable", is_movie_at_hand),
    ]
}

#[cfg(test)]
mod tests {
    #[test]
    fn edit_distance() {
        let d = |a: &str, b: &str| {
            let a: Vec<char> = a.chars().collect();
            let b: Vec<char> = b.chars().collect();
            super::edit_distance(&a, &b)
        };
        assert_eq!(d("kitten", "sitting"), 3);
        assert_eq!(d("", "abc"), 3);
        assert_eq!(d("same", "same"), 0);
    }

    #[test]
    fn no_name_twice() {
        let list = super::behaviours();
        let mut names: Vec<&str> = list.iter().map(|(n, _)| *n).collect();
        names.sort_unstable();
        let before = names.len();
        names.dedup();
        assert_eq!(before, names.len());
        for name in names {
            assert!(
                !crate::protection::PROTECTED.contains(&name),
                "{name} is protected"
            );
        }
    }
}
