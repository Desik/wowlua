//! Development automation for the wowlua workspace.
//!
//! - `cargo xtask codes`: regenerate the diagnostic code table from
//!   `spec/appendix/diagnostics.md`.
//! - `cargo xtask diagnostics`: the codes' explanations as JSON for the
//!   website (`docs/reference/diagnostics.json`).
//! - `cargo xtask schema`: regenerate the manifest's and the world
//!   profiles' JSON schemas in `schema/`.
//! - `cargo xtask runtime`: compile the runtime's wowlua sources into the
//!   Lua the compiler embeds (`crates/wowlua-runtime/lua`).
//! - `cargo xtask check`: fail if generated files are out of date (used in CI).
//! - `cargo xtask corpus fetch|run [dir]`: the conformance corpus.
//! - `cargo xtask types fetch|gen|wiki [dir]`: the embedded wow-api declarations.
//! - `cargo xtask packager-diff <release dir> <wowlua project>`: the M6
//!   exit check against the BigWigs packager (every AddOn folder).
//! - `cargo xtask parse-stats <dir>`: what the syntax trees of every Lua
//!   file below a directory hold, and the heap they take (#19).
//! - `cargo xtask trainer-levels <SuuUI>/src/Data/Trainer`: the recorded
//!   class trainer services' levels from SuuUI's trainer data.
//! - `cargo xtask flavor-data [<WoW installation>]`: each flavor's races,
//!   the classes they can be, racial spells and base mana from its
//!   installed client's tables (`crates/wowlua-flavors/data/`).
//! - `cargo xtask ingest [--unstamped] [--account <n>] [--client <folder>] [<StdTest.lua>...]`:
//!   StdTest's recordings into the fixtures and data, each part only when
//!   newer than the repository's; without files, every account's in the
//!   WoW installation.
//! - `cargo xtask fullprobe-plan [--stats]`: the plan of `/stdtest engine
//!   fullprobe` from camelot's documentation (`tests/ingame/src/fullprobeplan.lua`).
//! - `cargo xtask methodprobe-plan [--stats]`: the plan of `/stdtest engine
//!   methodprobe`, every widget method on fresh widgets
//!   (`tests/ingame/src/methodprobeplan.lua`).
//! - `cargo xtask coverage-report`: the emulator's coverage report
//!   (`docs/emulator-coverage.md`).
//! - `cargo xtask api-reference`: the website's WoW API reference data
//!   for camelot (`docs/reference/api/camelot.json.gz`).
//! - `cargo xtask web-ui`: the Blizzard UI files the playground fetches
//!   per flavor (`crates/wowlua-web/ui/`).

use anyhow::{Context, Result, bail};
use std::fs;
use std::path::{Path, PathBuf};

mod api_reference;
mod atlases;
mod corpus;
mod coverage;
mod diagnostics;
mod enginedata;
mod fixture;
mod flavor_data;
mod fullprobe;
mod ingest;
mod lua51;
mod methodprobe;
mod packager_diff;
mod parse_stats;
mod profile;
mod runtime;
mod trainer_levels;
mod types;
mod webui;

// Counts live heap bytes for `parse-stats`; forwards to the system allocator.
#[global_allocator]
static ALLOC: parse_stats::Counting = parse_stats::Counting;

fn main() -> Result<()> {
    let task = std::env::args().nth(1).unwrap_or_default();
    let root = workspace_root();
    match task.as_str() {
        "codes" => write_if_changed(&codes_path(&root), &generate_codes(&root)?),
        "diagnostics" => {
            write_if_changed(&root.join(diagnostics::PATH), &diagnostics::generate(&root))
        }
        "fixture" => {
            let (Some(saved), Some(name)) = (std::env::args().nth(2), std::env::args().nth(3))
            else {
                bail!("usage: cargo xtask fixture <SavedVariables/StdTest.lua> <name>");
            };
            fixture::run(&root, Path::new(&saved), &name)
        }
        "atlases" => {
            let Some(dir) = std::env::args().nth(2) else {
                bail!("usage: cargo xtask atlases <dir of the client's atlas tables as CSV>");
            };
            atlases::run(&root, Path::new(&dir))
        }
        "trainer-levels" => {
            let Some(dir) = std::env::args().nth(2) else {
                bail!("usage: cargo xtask trainer-levels <SuuUI>/src/Data/Trainer");
            };
            trainer_levels::run(&root, Path::new(&dir))
        }
        "flavor-data" => flavor_data::run(&root, std::env::args().nth(2).as_deref()),
        "ingest" => {
            let args: Vec<String> = std::env::args().skip(2).collect();
            ingest::run_all(&root, &args)
        }
        "profile" => {
            let (Some(saved), Some(out)) = (std::env::args().nth(2), std::env::args().nth(3))
            else {
                bail!("usage: cargo xtask profile <SavedVariables/StdTest.lua> <out.json>");
            };
            profile::run(Path::new(&saved), Path::new(&out))
        }
        "schema" => {
            write_if_changed(&schema_path(&root), &wowlua_manifest::schema_json())?;
            write_if_changed(
                &world_schema_path(&root),
                &wowlua_manifest::world::schema_json(),
            )
        }
        "fullprobe-plan" => {
            if std::env::args().nth(2).as_deref() == Some("--stats") {
                fullprobe::stats(&root)
            } else {
                write_if_changed(&root.join(fullprobe::PLAN), &fullprobe::generate(&root)?)
            }
        }
        "methodprobe-plan" => {
            if std::env::args().nth(2).as_deref() == Some("--stats") {
                methodprobe::stats(&root)
            } else {
                write_if_changed(
                    &root.join(methodprobe::PLAN),
                    &methodprobe::generate(&root)?,
                )
            }
        }
        "coverage-report" => {
            write_if_changed(&root.join(coverage::REPORT), &coverage::generate(&root)?)
        }
        "api-reference" => write_bytes_if_changed(
            &root.join(api_reference::PATH),
            &api_reference::generate(&root)?,
        ),
        "web-ui" => {
            for flavor in webui::FLAVORS {
                write_if_changed(&webui::path(&root, flavor), &webui::generate(flavor)?)?;
            }
            Ok(())
        }
        "runtime" => {
            let slices = runtime::generate(&root)?;
            for stale in stale_runtime_files(&root, &slices)? {
                fs::remove_file(&stale)?;
                println!("removed {}", stale.display());
            }
            for (path, lua) in &slices {
                write_if_changed(path, lua)?;
            }
            Ok(())
        }
        "check" => {
            check_current(&codes_path(&root), &generate_codes(&root)?, "codes")?;
            check_current(
                &root.join(diagnostics::PATH),
                &diagnostics::generate(&root),
                "diagnostics",
            )?;
            check_current(
                &schema_path(&root),
                &wowlua_manifest::schema_json(),
                "schema",
            )?;
            check_current(
                &world_schema_path(&root),
                &wowlua_manifest::world::schema_json(),
                "schema",
            )?;
            check_current(
                &root.join(fullprobe::PLAN),
                &fullprobe::generate(&root)?,
                "fullprobe-plan",
            )?;
            check_current(
                &root.join(methodprobe::PLAN),
                &methodprobe::generate(&root)?,
                "methodprobe-plan",
            )?;
            check_current(
                &root.join(coverage::REPORT),
                &coverage::generate(&root)?,
                "coverage-report",
            )?;
            check_bytes_current(
                &root.join(api_reference::PATH),
                &api_reference::generate(&root)?,
                "api-reference",
            )?;
            let slices = runtime::generate(&root)?;
            if let Some(stale) = stale_runtime_files(&root, &slices)?.first() {
                bail!(
                    "{} is not generated: run `cargo xtask runtime`",
                    stale.display()
                );
            }
            for (path, lua) in &slices {
                check_current(path, lua, "runtime")?;
            }
            webui::check(&root)?;
            check_web_version(&root)
        }
        "parse-stats" => {
            let Some(dir) = std::env::args().nth(2) else {
                bail!("usage: cargo xtask parse-stats <dir>");
            };
            parse_stats::run(Path::new(&dir))
        }
        "corpus" => {
            let dir = std::env::args()
                .nth(3)
                .map(PathBuf::from)
                .unwrap_or_else(|| root.join("target/corpus"));
            match std::env::args().nth(2).as_deref() {
                Some("fetch") => corpus::fetch(&root, &dir),
                Some("run") => corpus::run(&dir),
                Some("addons") => corpus::addons(&dir),
                Some("vm") => corpus::vm(&dir),
                Some("lua51") => lua51::run(&dir),
                _ => bail!("usage: cargo xtask corpus <fetch|run|addons|vm|lua51> [dir]"),
            }
        }
        "types" => {
            let dir = std::env::args()
                .nth(3)
                .map(PathBuf::from)
                .unwrap_or_else(|| root.join("target/types-src"));
            match std::env::args().nth(2).as_deref() {
                Some("fetch") => types::fetch(&dir),
                Some("gen") => types::generate(&root, &dir),
                Some("wiki") => types::wiki(&root, &dir),
                _ => bail!("usage: cargo xtask types <fetch|gen|wiki> [dir]"),
            }
        }
        "packager-runs" => {
            // The single-game packager runs of a project's CI workflows:
            // `<game> <flavor> <project file or ->` per line.
            let dir = PathBuf::from(std::env::args().nth(2).unwrap_or_else(|| ".".into()));
            // `split` alone when the runs make TOCs per game (`-S`).
            let single = wowlua_driver::pkgmeta::single_game_runs(&dir);
            if single.is_empty()
                && wowlua_driver::pkgmeta::workflow_runs(&dir)
                    .iter()
                    .any(|r| r.split)
            {
                println!("split");
                return Ok(());
            }
            let mut seen = std::collections::BTreeSet::new();
            for run in single {
                let Some(game) = run.game else { continue };
                let Some(flavor) = wowlua_driver::keywords::game_flavor(&game) else {
                    continue;
                };
                if seen.insert(flavor) {
                    println!("{game} {flavor} {}", run.file.as_deref().unwrap_or("-"));
                }
            }
            Ok(())
        }
        "packager-diff" => {
            let mut raw: Vec<String> = std::env::args().skip(2).collect();
            let only = match raw.iter().position(|a| a == "--flavor") {
                Some(i) if i + 1 < raw.len() => {
                    let flavor = raw.remove(i + 1);
                    raw.remove(i);
                    Some(flavor)
                }
                _ => None,
            };
            let args: Vec<PathBuf> = raw.into_iter().map(PathBuf::from).collect();
            match args.as_slice() {
                [packager, wowlua, manifest] => {
                    packager_diff::run(packager, wowlua, manifest, only.as_deref())
                }
                [release, project] => packager_diff::run_all(release, project, only.as_deref()),
                _ => bail!(
                    "usage: cargo xtask packager-diff [--flavor <id>] <release dir> <wowlua project> | <packager dir> <wowlua dir> <wowlua.json>"
                ),
            }
        }
        _ => bail!(
            "usage: cargo xtask <codes|diagnostics|schema|check|coverage-report|api-reference|corpus|types|packager-diff|packager-runs>"
        ),
    }
}

fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("xtask lives in the workspace root")
        .to_path_buf()
}

fn codes_path(root: &Path) -> PathBuf {
    root.join("crates/wowlua-diagnostics/src/codes.rs")
}

fn schema_path(root: &Path) -> PathBuf {
    root.join(format!(
        "schema/wowlua-{}.json",
        wowlua_manifest::SCHEMA_VERSION
    ))
}

/// The world profiles' schema (decision T401).
fn world_schema_path(root: &Path) -> PathBuf {
    root.join(format!(
        "schema/world-profile-{}.json",
        wowlua_manifest::world::SCHEMA_VERSION
    ))
}

fn check_current(path: &Path, expected: &str, task: &str) -> Result<()> {
    check_bytes_current(path, expected.as_bytes(), task)
}

fn check_bytes_current(path: &Path, expected: &[u8], task: &str) -> Result<()> {
    if fs::read(path).ok().as_deref() != Some(expected) {
        bail!(
            "{} is out of date: run `cargo xtask {task}`",
            path.display()
        );
    }
    Ok(())
}

/// Lua files of the runtime that no wowlua source generates.
fn stale_runtime_files(root: &Path, slices: &[(PathBuf, String)]) -> Result<Vec<PathBuf>> {
    let mut stale = Vec::new();
    for entry in fs::read_dir(root.join("crates/wowlua-runtime/lua"))? {
        let path = entry?.path().canonicalize()?;
        if path.extension().is_some_and(|e| e == "lua") && !slices.iter().any(|(p, _)| *p == path) {
            stale.push(path);
        }
    }
    Ok(stale)
}

fn write_if_changed(path: &Path, content: &str) -> Result<()> {
    write_bytes_if_changed(path, content.as_bytes())
}

fn write_bytes_if_changed(path: &Path, content: &[u8]) -> Result<()> {
    if fs::read(path).ok().as_deref() != Some(content) {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::write(path, content).with_context(|| format!("writing {}", path.display()))?;
        println!("updated {}", path.display());
    }
    Ok(())
}

/// Builds `codes.rs` from the `| E0101 | summary |` rows of Appendix B.
fn generate_codes(root: &Path) -> Result<String> {
    let appendix = root.join("spec/appendix/diagnostics.md");
    let text =
        fs::read_to_string(&appendix).with_context(|| format!("reading {}", appendix.display()))?;
    let mut rows: Vec<(String, String)> = text.lines().filter_map(parse_row).collect();
    rows.sort_by(|a, b| a.0[1..].cmp(&b.0[1..]).then(a.0.cmp(&b.0)));
    rows.dedup_by(|a, b| a.0 == b.0);

    let mut out = String::from(
        "// @generated by `cargo xtask codes` from spec/appendix/diagnostics.md. Do not edit.\n\n\
         use crate::CodeInfo;\n\n\
         #[rustfmt::skip]\n\
         pub(crate) static CODES: &[CodeInfo] = &[\n",
    );
    for (code, summary) in rows {
        out.push_str(&format!(
            "    CodeInfo {{ code: {code:?}, summary: {summary:?} }},\n"
        ));
    }
    out.push_str("];\n");
    Ok(out)
}

fn parse_row(line: &str) -> Option<(String, String)> {
    let mut cells = line.strip_prefix('|')?.split('|').map(str::trim);
    let code = cells.next()?;
    let summary = cells.next()?;
    let valid = code.len() == 5
        && matches!(code.as_bytes()[0], b'E' | b'W' | b'P')
        && code[1..].bytes().all(|b| b.is_ascii_digit());
    valid.then(|| (code.to_string(), summary.replace("\\|", "|")))
}

/// `web/package.json` (the playground's package, released with wowlua) has
/// the workspace's version.
fn check_web_version(root: &Path) -> Result<()> {
    let manifest = fs::read_to_string(root.join("web/package.json"))?;
    let value: serde_json::Value = serde_json::from_str(&manifest)?;
    let cargo = fs::read_to_string(root.join("Cargo.toml"))?;
    let want = regex::Regex::new(r#"(?m)^\[workspace\.package\][^\[]*?^version = "([^"]+)""#)?
        .captures(&cargo)
        .map(|c| c[1].to_string())
        .context("the workspace's version in Cargo.toml")?;
    if value["version"].as_str() != Some(want.as_str()) {
        bail!(
            "web/package.json has version {}, the workspace {want}: set it (CLAUDE.md, Releasing)",
            value["version"]
        );
    }
    Ok(())
}
