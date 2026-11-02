//! Every function of WoW Forever's in-game environment (`/stdtest
//! inventory`, fixture `wowlua-vm/tests/client/forever-camelot-functions.tsv`):
//! with camelot's engine and Blizzard UI code loaded, each exists, as a C
//! function or a Lua one as in the client.

#![allow(clippy::disallowed_methods)] // (tests set up their files on the disk)

use std::path::{Path, PathBuf};

use wowlua_vm::function::Function;
use wowlua_vm::{Value, Vm};

fn repo() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

/// The client: camelot's engine with its UI code, or none when that is
/// not on disk (`cargo xtask types`).
fn client() -> Option<Vm> {
    let interface = repo().join("target/types-src/camelot/Interface");
    if !interface.exists() {
        return None;
    }
    let xtask = std::fs::read_to_string(repo().join("xtask/src/types.rs")).unwrap_or_default();
    let commit = xtask
        .split("\"camelot\",")
        .nth(1)
        .and_then(|rest| rest.split('"').nth(3))
        .unwrap_or("");
    let strings = repo().join(format!("target/types-src/strings/{commit}.lua"));
    let mut vm = Vm::wow();
    wowlua_engine::install(&mut vm, "camelot").expect("installs");
    wowlua_engine::ui::load(
        &mut vm,
        &interface,
        strings.exists().then_some(Path::new(&strings)),
    );
    // Errors after loading, for the report.
    let f = vm
        .load(
            b"InventoryErrors = {} seterrorhandler(function(e) InventoryErrors[#InventoryErrors + 1] = tostring(e) end)",
            "=errors",
        )
        .expect("compiles");
    vm.call(f, &[]).expect("runs");
    // Logged in and in the world, as when the inventory was taken
    // (Blizzard's code loads some addons then).
    wowlua_engine::widgets::fire_event(&mut vm, "PLAYER_LOGIN", &[]);
    wowlua_engine::widgets::fire_event(
        &mut vm,
        "PLAYER_ENTERING_WORLD",
        &[Value::Bool(true), Value::Bool(false)],
    );
    wowlua_engine::clock::advance_frames(&mut vm, 30, 1000.0 / 60.0);
    Some(vm)
}

/// What a name holds: "C", "Lua", another type's name, or "absent".
fn kind(vm: &mut Vm, name: &str) -> String {
    let mut v = Value::Table(vm.globals);
    for part in name.split('.') {
        let Value::Table(t) = v else {
            return "absent".into();
        };
        let k = vm.str(part);
        v = vm.heap.table(t).get(k);
    }
    match v {
        Value::Nil => "absent".into(),
        Value::Func(f) => match vm.heap.func(f) {
            Function::Native(_) => "C".into(),
            Function::Lua(_) => "Lua".into(),
        },
        other => other.type_name().into(),
    }
}

/// A global as the probe shows it: `number 5`, `boolean true`.
fn shown(vm: &mut Vm, name: &str) -> String {
    let v = vm.get_global(name);
    match v {
        Value::Nil => "absent".into(),
        Value::Num(n) => format!("number {}", wowlua_vm::number::fmt_num(n)),
        Value::Bool(b) => format!("boolean {b}"),
        other => other.type_name().into(),
    }
}

/// How many of the client's number and boolean globals (`/stdtest engine`,
/// `scalars`) the engine with Blizzard's code has otherwise. It may only go
/// down.
const MAX_SCALARS_DIFFERENT: usize = 1;

/// What the installed build's UI code no longer defines since the
/// inventory was taken (Forever 70425), until it is taken again
/// (tests/ingame/PROBES.md, "The Camelot update"): 70707's smart
/// navigation dropped a function.
const NEWER_BUILD: &[&str] = &["SmartNavigation_SetReturnFrameFocusButton"];

#[test]
fn every_function_of_the_client_exists_with_its_kind() {
    let Some(mut vm) = client() else {
        eprintln!("skipped: camelot's UI code is not on disk");
        return;
    };
    let fixture = std::fs::read_to_string(
        repo().join("crates/wowlua-vm/tests/client/forever-camelot-functions.tsv"),
    )
    .expect("the inventory");
    let mut missing = Vec::new();
    let mut wrong = Vec::new();
    let mut total = 0;
    for line in fixture.lines().filter(|l| !l.starts_with('#')) {
        let Some((name, expected)) = line.split_once('\t') else {
            continue;
        };
        total += 1;
        let ours = kind(&mut vm, name);
        if ours == "absent" && NEWER_BUILD.contains(&name) {
            continue;
        }
        if ours == "absent" {
            missing.push(format!("{name} ({expected})"));
        } else if ours != expected {
            wrong.push(format!("{name}: client {expected}, ours {ours}"));
        }
    }
    if let Ok(path) = std::env::var("WOWLUA_INVENTORY_REPORT") {
        let f = vm
            .load(b"return table.concat(InventoryErrors, '\\n')", "=report")
            .expect("compiles");
        let errors = match vm.call(f, &[]).ok().and_then(|r| r.first().copied()) {
            Some(Value::Str(s)) => String::from_utf8_lossy(vm.str_bytes(s)).into_owned(),
            _ => String::new(),
        };
        let defaulted: Vec<&str> = wowlua_engine::engine(&vm)
            .defaulted
            .iter()
            .copied()
            .collect();
        let load_errors = wowlua_engine::engine(&vm).world.ui.errors.join("\n");
        std::fs::write(
            path,
            format!(
                "missing:\n{}\n\nwrong kind:\n{}\n\nerrors while loading:\n{load_errors}\n\nerrors after loading:\n{errors}\n\ndefaulted:\n{}\n",
                missing.join("\n"),
                wrong.join("\n"),
                defaulted.join("\n")
            ),
        )
        .expect("writes");
    }
    // Blizzard's errors, distinct: while loading (its handler logs them,
    // `C_Log.LogErrorMessage`) and once logged in.
    let f = vm
        .load(b"return table.concat(InventoryErrors, '\\0')", "=errors")
        .expect("compiles");
    let after = match vm.call(f, &[]).ok().and_then(|r| r.first().copied()) {
        Some(Value::Str(s)) => String::from_utf8_lossy(vm.str_bytes(s)).into_owned(),
        _ => String::new(),
    };
    let distinct: std::collections::BTreeSet<String> = wowlua_engine::engine(&vm)
        .world
        .ui
        .errors
        .iter()
        .cloned()
        .chain(
            after
                .split('\0')
                .filter(|e| !e.is_empty())
                .map(str::to_string),
        )
        .collect();
    eprintln!("{} distinct errors", distinct.len());
    // The client raises none (`/stdtest engine`: Blizzard's ScriptErrorsFrame
    // kept no error), and neither may the engine.
    assert!(
        distinct.is_empty(),
        "Blizzard's code raised {} distinct errors:\n{}",
        distinct.len(),
        distinct.iter().cloned().collect::<Vec<_>>().join("\n")
    );
    // The number and boolean globals.
    let engine_fixture = std::fs::read_to_string(
        repo().join("crates/wowlua-engine/tests/client/forever-camelot-engine.lua"),
    )
    .expect("the engine fixture");
    // The addons the client's session had loaded (the battery's
    // `blizzardNow`: one may open a panel first, its addon's globals then
    // defined), loaded here too.
    if let Some(now) = engine_fixture
        .lines()
        .find_map(|l| l.trim().strip_prefix("[\"probe.blizzardNow\"] = \""))
    {
        for name in now.trim_end_matches("\",").split_whitespace() {
            let src = format!(
                "if not C_AddOns.IsAddOnLoaded({name:?}) then C_AddOns.LoadAddOn({name:?}) end"
            );
            let f = vm.load(src.as_bytes(), "=load").expect("compiles");
            let _ = vm.call(f, &[]);
        }
    }
    let mut scalars = Vec::new();
    let mut scalar_total = 0;
    for line in engine_fixture.lines() {
        let Some(rest) = line.trim().strip_prefix("[\"probe.scalars.") else {
            continue;
        };
        let Some((name, expected)) = rest.split_once("\"] = \"") else {
            continue;
        };
        let expected = expected.trim_end_matches("\",");
        scalar_total += 1;
        let ours = shown(&mut vm, name);
        if ours != expected {
            scalars.push(format!("{name}: client {expected}, ours {ours}"));
        }
    }
    eprintln!(
        "{} of {total} missing; {} of {scalar_total} globals differ",
        missing.len(),
        scalars.len()
    );
    if let Ok(path) = std::env::var("WOWLUA_INVENTORY_REPORT") {
        let mut report = std::fs::read_to_string(&path).unwrap_or_default();
        report.push_str(&format!("\nglobals:\n{}\n", scalars.join("\n")));
        std::fs::write(path, report).expect("writes");
    }
    assert!(
        scalars.len() <= MAX_SCALARS_DIFFERENT,
        "of {scalar_total} globals, {} differ (at most {MAX_SCALARS_DIFFERENT}):\n{}",
        scalars.len(),
        scalars.join("\n")
    );
    assert!(
        missing.is_empty() && wrong.is_empty(),
        "of {total}: {} missing, {} of another kind\nmissing: {}\nwrong kind: {}",
        missing.len(),
        wrong.len(),
        missing
            .iter()
            .take(40)
            .cloned()
            .collect::<Vec<_>>()
            .join(", "),
        wrong
            .iter()
            .take(40)
            .cloned()
            .collect::<Vec<_>>()
            .join(", ")
    );
}
