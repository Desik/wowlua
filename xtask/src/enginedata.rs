//! The engine's `Enum`, `Constants` and number globals for the flavors
//! without a recording of their own client (decision T554): what the
//! client's documentation lacks, from Ketho's dump of the client
//! (BlizzardInterfaceResources' `LuaEnum.lua`, at the commit the
//! declarations name: older than an installed client, so it only adds),
//! and the tables its own `EnvironmentCleanup.lua` removes before addons
//! load (secure). Written as camelot's recordings are
//! (`crates/wowlua-engine/data/<flavor>-enums.txt`, `<flavor>-globals.txt`).

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::Path;

use anyhow::Result;
use serde_json::Value as Json;

/// A table's fields, each as a recording writes its value (`number 1`).
type Tables = BTreeMap<String, BTreeMap<String, String>>;

/// Ketho's `LuaEnum.lua`: `Enum`, `Constants`, and the number globals.
#[derive(Default)]
struct Dump {
    enums: Tables,
    constants: Tables,
    globals: BTreeMap<String, String>,
}

fn value(text: &str) -> Option<String> {
    let text = text.trim();
    if let Some(s) = text.strip_prefix('"').and_then(|s| s.strip_suffix('"')) {
        return Some(format!("string {s}"));
    }
    if text == "true" || text == "false" {
        return Some(format!("boolean {text}"));
    }
    let n = match text.strip_prefix("0x") {
        Some(hex) => u64::from_str_radix(hex, 16).ok()? as f64,
        None => text.parse::<f64>().ok()?,
    };
    Some(format!("number {n}"))
}

fn parse(text: &str) -> Dump {
    let mut dump = Dump::default();
    let mut section: Option<bool> = None; // Some(true): Enum, Some(false): Constants
    let mut table: Option<String> = None;
    for line in text.lines() {
        match (line, section, &table) {
            ("Enum = {", _, _) => section = Some(true),
            ("Constants = {", _, _) => section = Some(false),
            ("}", _, _) => section = None,
            (l, Some(_), None) if l.starts_with('\t') && l.ends_with(" = {") => {
                table = Some(l.trim().trim_end_matches(" = {").to_string());
            }
            (l, Some(_), Some(_)) if l.trim() == "}," || l.trim() == "}" => table = None,
            (l, Some(enums), Some(t)) => {
                let Some((name, v)) = l.trim().trim_end_matches(',').split_once(" = ") else {
                    continue;
                };
                let Some(v) = value(v) else {
                    continue;
                };
                let tables = if enums {
                    &mut dump.enums
                } else {
                    &mut dump.constants
                };
                tables
                    .entry(t.clone())
                    .or_default()
                    .insert(name.to_string(), v);
            }
            (l, None, _) => {
                if let Some((name, v)) = l.split_once(" = ")
                    && name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
                    && let Some(n) =
                        value(v).and_then(|v| v.strip_prefix("number ").map(str::to_string))
                {
                    dump.globals.insert(name.to_string(), n);
                }
            }
            _ => {}
        }
    }
    dump
}

/// The documentation's enums and constants: table → field names.
fn documented(
    api: &Json,
) -> (
    BTreeMap<String, BTreeSet<String>>,
    BTreeMap<String, BTreeSet<String>>,
) {
    let mut enums: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    let mut constants: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    let list = |v: &Json, k: &str| {
        v.get(k)
            .and_then(Json::as_array)
            .cloned()
            .unwrap_or_default()
    };
    let name = |v: &Json| v.get("Name").and_then(Json::as_str).map(str::to_string);
    for system in list(api, "systems") {
        for t in list(&system, "Tables") {
            let Some(tname) = name(&t) else {
                continue;
            };
            match t.get("Type").and_then(Json::as_str) {
                Some("Enumeration") => {
                    let fields: BTreeSet<String> =
                        list(&t, "Fields").iter().filter_map(name).collect();
                    enums.entry(tname.clone()).or_default().extend(fields);
                    // (and its `Meta` table)
                    enums
                        .entry(format!("{tname}Meta"))
                        .or_default()
                        .extend(["NumValues", "MinValue", "MaxValue"].map(str::to_string));
                }
                Some("Constants") => {
                    let fields: BTreeSet<String> =
                        list(&t, "Values").iter().filter_map(name).collect();
                    constants.entry(tname).or_default().extend(fields);
                }
                _ => {}
            }
        }
    }
    (enums, constants)
}

/// The tables an `EnvironmentCleanup.lua` removes (`Enum.StoreError = nil;`).
fn removed(cleanup: &str) -> BTreeSet<String> {
    cleanup
        .lines()
        .filter_map(|l| {
            let rest = l.trim().strip_suffix("= nil;")?.trim();
            let (root, table) = rest.split_once('.')?;
            (["Enum", "Constants"].contains(&root) && !table.contains('.'))
                .then(|| format!("{root}.{table}"))
        })
        .collect()
}

/// Writes the flavor's enums from the dump `dump_text` (`source`), its
/// documentation `api` and its UI code `interface` (the installed client's,
/// or wow-ui-source's).
pub fn write_enums(
    root: &Path,
    flavor: &str,
    source: &str,
    dump_text: &str,
    api: &Json,
    interface: &Path,
) -> Result<()> {
    let dump = parse(dump_text);
    let (enums, constants) = documented(api);
    let mut secure = BTreeSet::new();
    let cleanup_dir = interface.join("AddOns/Blizzard_EnvironmentCleanup");
    let mut files = vec![cleanup_dir.join("EnvironmentCleanup.lua")];
    if let Ok(dirs) = fs::read_dir(&cleanup_dir) {
        files.extend(
            dirs.flatten()
                .map(|d| d.path().join("EnvironmentCleanup.lua")),
        );
    }
    for f in files {
        if let Ok(text) = fs::read_to_string(&f) {
            secure.extend(removed(&text));
        }
    }
    let mut out = format!(
        "# The client's `Enum` and `Constants` where its documentation lacks them\n\
         # (decision T554; `cargo xtask types gen`): the tables its\n\
         # EnvironmentCleanup.lua removes (`secure`), then the rest from\n\
         # {source}.\n\
         # `name<TAB>value` as camelot-enums.txt.\n"
    );
    for t in &secure {
        out.push_str(&format!("{t}\tsecure\n"));
    }
    for (root_name, tables, docs) in [
        ("Enum", &dump.enums, &enums),
        ("Constants", &dump.constants, &constants),
    ] {
        for (t, fields) in tables {
            if secure.contains(&format!("{root_name}.{t}")) {
                continue;
            }
            let known = docs.get(t);
            for (field, v) in fields {
                if !known.is_some_and(|k| k.contains(field)) {
                    out.push_str(&format!("{root_name}.{t}.{field}\t{v}\n"));
                }
            }
        }
    }
    crate::write_if_changed(
        &root.join(format!("crates/wowlua-engine/data/{flavor}-enums.txt")),
        &out,
    )
}

/// Writes the flavor's number globals from the dump `dump_text` (`source`).
pub fn write_globals(root: &Path, flavor: &str, source: &str, dump_text: &str) -> Result<()> {
    let dump = parse(dump_text);
    let mut globals = format!(
        "# The number globals the client defines itself (decision T554; `cargo xtask\n\
         # types gen`), `LE_*` and `NUM_LE_*`, from\n\
         # {source}.\n\
         # `name<TAB>value`.\n"
    );
    for (name, n) in &dump.globals {
        globals.push_str(&format!("{name}\t{n}\n"));
    }
    crate::write_if_changed(
        &root.join(format!("crates/wowlua-engine/data/{flavor}-globals.txt")),
        &globals,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_dump_is_read() {
        let dump = parse(
            "Enum = {\n\tGameMode = {\n\t\tStandard = 0,\n\t\tFlags = 0x10,\n\t},\n}\n\
             Constants = {\n\tC = {\n\t\tOn = false,\n\t\tName = \"x\",\n\t\tStep = 1.5,\n\t},\n}\n\
             NUM_LE_X = 2\nLE_X_A = 1\n",
        );
        assert_eq!(dump.enums["GameMode"]["Standard"], "number 0");
        assert_eq!(dump.enums["GameMode"]["Flags"], "number 16");
        assert_eq!(dump.constants["C"]["On"], "boolean false");
        assert_eq!(dump.constants["C"]["Name"], "string x");
        assert_eq!(dump.constants["C"]["Step"], "number 1.5");
        assert_eq!(dump.globals["NUM_LE_X"], "2");
        assert_eq!(
            removed("Enum.StoreError = nil;\nC_StoreSecure = nil;\n"),
            BTreeSet::from(["Enum.StoreError".to_string()])
        );
    }
}
