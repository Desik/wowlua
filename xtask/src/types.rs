//! `cargo xtask types fetch|gen [dir]`: the embedded wow-api declarations
//! (spec package manager §10.9, decisions T83, T84). `fetch` checks out
//! Blizzard's UI code with its API documentation per flavor (the
//! `wow-ui-source` mirror, at the tag of the flavor's newest client
//! version), Ketho's annotations and the names of the global strings
//! (pinned); `gen` writes `crates/wowlua-api/data/`.

use anyhow::{Context, Result, bail};
use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

const UI_REPO: &str = "https://github.com/Gethe/wow-ui-source.git";
const DOCS_DIR: &str = "Interface/AddOns/Blizzard_APIDocumentationGenerated";
const KETHO_REPO: &str = "https://github.com/Ketho/vscode-wow-api.git";
/// Ketho's vscode-wow-api (MIT) at a reviewed commit.
const KETHO_COMMIT: &str = "d0b5b51fac4c52c493371b9b18e66ce604ea4326";
/// The annotations used: widget classes, script objects, type aliases,
/// FrameXML, and the undocumented globals (Lua and enum data come from
/// elsewhere).
const KETHO_DIRS: &[&str] = &["Widget", "ScriptObject", "Type", "FrameXML"];
const KETHO_FILES: &[&str] = &[
    "Data/Wiki.lua",
    "Data/Classic.lua",
    "Data/Event.lua",
    "Data/CVar.lua",
];
/// Global string names (client dumps of Ketho's BlizzardInterfaceResources,
/// names only): flavor → (branch, pinned commit).
const STRINGS: &[(&str, &str, &str)] = &[
    (
        "standard",
        "live",
        "36dd01db2d8fa5086dffda5cbfb3d55f4a70e526",
    ),
    (
        "camelot",
        "forever",
        "c5a49d394564f20c508c71014d6f60d9cd240c71",
    ),
    (
        "mists",
        "classic",
        "877c67e9e0a64b5daae348a7b72b7a64bfaab90f",
    ),
    (
        "tbc",
        "classic_anniversary",
        "d6d4a8f445f198c5c73ff5f8f5002ad8f04451e4",
    ),
    (
        "vanilla",
        "classic_era",
        "1f1dec331d23927fefe3d50c3d7f6e0915cda96f",
    ),
];

fn git(args: &[&str], dir: Option<&Path>) -> Result<String> {
    let mut cmd = Command::new("git");
    if let Some(d) = dir {
        cmd.current_dir(d);
    }
    let out = cmd.args(args).output().context("running git")?;
    if !out.status.success() {
        bail!(
            "git {}: {}",
            args.join(" "),
            String::from_utf8_lossy(&out.stderr)
        );
    }
    Ok(String::from_utf8_lossy(&out.stdout).into_owned())
}

/// The flavors, the documentation tag of each (the newest version of the
/// flavor's live clients that the mirror has a tag for) and the commit it
/// points to now: the mirror moves a version's tag to each new build.
fn flavor_tags() -> Result<Vec<(String, String, String)>> {
    let tags_out = git(&["ls-remote", "--tags", UI_REPO], None)?;
    // Tag → commit (an annotated tag's peeled line, `^{}`, wins).
    let mut commits: HashMap<String, String> = HashMap::new();
    for line in tags_out.lines() {
        let Some((sha, name)) = line.split_once('\t') else {
            continue;
        };
        let Some(name) = name.strip_prefix("refs/tags/") else {
            continue;
        };
        match name.strip_suffix("^{}") {
            Some(tag) => {
                commits.insert(tag.to_string(), sha.to_string());
            }
            None => {
                commits
                    .entry(name.to_string())
                    .or_insert_with(|| sha.to_string());
            }
        }
    }
    let tags: HashSet<String> = commits.keys().cloned().collect();
    let table = wowlua_flavors::table();
    let mut out = Vec::new();
    for flavor in &table.flavors {
        let mut versions: Vec<&str> = table
            .clients
            .iter()
            .filter(|c| c.flavor == flavor.id)
            // Test clients do not define the flavor's API.
            .filter(|c| {
                !["-ptr", "-xptr", "-beta"]
                    .iter()
                    .any(|t| c.name.ends_with(t))
            })
            .map(|c| c.version.as_str())
            .collect();
        versions.sort_by_key(|v| std::cmp::Reverse(version_key(v)));
        match versions.iter().find(|v| tags.contains(**v)) {
            Some(v) => out.push((flavor.id.clone(), v.to_string(), commits[*v].clone())),
            None => bail!(
                "no wow-ui-source tag for flavor {} (clients {:?})",
                flavor.id,
                versions
            ),
        }
    }
    Ok(out)
}

fn version_key(v: &str) -> Vec<u32> {
    v.split('.').filter_map(|p| p.parse().ok()).collect()
}

/// A flavor's UI code at the commit its declarations name
/// (`-- … commit <sha>;` in `crates/wowlua-api/data/<flavor>.d.wowlua`).
fn fetch_pinned(dir: &Path, flavor: &str) -> Result<()> {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("..");
    let decl = fs::read_to_string(root.join(format!("crates/wowlua-api/data/{flavor}.d.wowlua")))?;
    let commit = decl
        .lines()
        .take(5)
        .find_map(|l| l.split("commit ").nth(1))
        .and_then(|rest| rest.get(..40))
        .ok_or_else(|| anyhow::anyhow!("{flavor}'s declarations name no commit"))?
        .to_string();
    let target = dir.join(flavor);
    let marker = target.join(".wowlua-tag");
    let stamp = format!("pinned {commit} addons");
    if fs::read_to_string(&marker).ok().as_deref() == Some(stamp.as_str()) {
        println!("{flavor}: {commit} (cached)");
        return Ok(());
    }
    if target.exists() {
        fs::remove_dir_all(&target)?;
    }
    println!("{flavor}: fetching wow-ui-source {commit}");
    fs::create_dir_all(&target)?;
    git(&["init", "-q"], Some(&target))?;
    git(&["remote", "add", "origin", UI_REPO], Some(&target))?;
    git(
        &["sparse-checkout", "set", "Interface/AddOns"],
        Some(&target),
    )?;
    git(
        &[
            "fetch",
            "-q",
            "--depth",
            "1",
            "--filter=blob:none",
            "origin",
            &commit,
        ],
        Some(&target),
    )?;
    git(&["checkout", "-q", "FETCH_HEAD"], Some(&target))?;
    fs::write(&marker, &stamp)?;
    Ok(())
}

pub fn fetch(dir: &Path) -> Result<()> {
    fs::create_dir_all(dir)?;
    // `WOWLUA_FETCH_ONLY=camelot`: that flavor's UI code and strings alone,
    // at the commit its declarations name (CI's Blizzard UI tests: the
    // code the engine's recordings match), without Ketho's declarations.
    let only = std::env::var("WOWLUA_FETCH_ONLY").ok();
    let wanted = |flavor: &str| only.as_deref().is_none_or(|o| o == flavor);
    if let Some(flavor) = &only {
        fetch_pinned(dir, flavor)?;
    }
    for (flavor, tag, commit) in flavor_tags()?
        .into_iter()
        .filter(|(f, _, _)| only.is_none() && wanted(f))
    {
        let target = dir.join(&flavor);
        let marker = target.join(".wowlua-tag");
        // The commit too: the tag moves with each build.
        let stamp = format!("{tag} {commit} addons");
        if fs::read_to_string(&marker).ok().as_deref() == Some(stamp.as_str()) {
            println!("{flavor}: {tag} (cached)");
            continue;
        }
        if target.exists() {
            fs::remove_dir_all(&target)?;
        }
        println!("{flavor}: fetching wow-ui-source {tag}");
        let t = target.to_string_lossy().into_owned();
        git(
            &[
                "clone",
                "-q",
                "--depth",
                "1",
                "--branch",
                &tag,
                "--filter=blob:none",
                "--sparse",
                UI_REPO,
                &t,
            ],
            None,
        )?;
        git(
            &["sparse-checkout", "set", "Interface/AddOns"],
            Some(&target),
        )?;
        fs::write(&marker, &stamp)?;
    }
    // The installed clients' own UI code in place of the mirror's (decision
    // T553): the mirror stays the copy CI fetches.
    for (flavor, _, _) in flavor_tags()?
        .into_iter()
        .filter(|(f, _, _)| only.is_none() && wanted(f))
    {
        let Some(casc) = wowlua_test::installed_client(&flavor) else {
            continue;
        };
        let target = dir.join(&flavor);
        let marker = target.join(".wowlua-installed");
        if fs::read_to_string(&marker).ok().as_deref() == Some(casc.version.as_str()) {
            println!("{flavor}: the installed client {} (cached)", casc.version);
            continue;
        }
        println!("{flavor}: reading the installed client {}", casc.version);
        let addons = target.join("Interface/AddOns");
        // (the mirror's addons, as committed: the folder may be a crawl's)
        let seeds: Vec<String> = git(
            &["ls-tree", "--name-only", "HEAD", "Interface/AddOns/"],
            Some(&target),
        )?
        .lines()
        .filter_map(|l| l.rsplit('/').next())
        .filter(|n| !n.is_empty())
        .map(str::to_owned)
        .collect();
        let part = target.join("installed.part");
        if part.exists() {
            fs::remove_dir_all(&part)?;
        }
        let got = wowlua_test::extract_installed(&casc, &flavor, &seeds, &part)
            .map_err(|e| anyhow::anyhow!("{flavor}: {e}"))?;
        println!("{flavor}: {} addons, {} files", got.addons.len(), got.files);
        fs::remove_dir_all(&addons)?;
        fs::rename(part.join("Interface/AddOns"), &addons)?;
        fs::rename(
            part.join(wowlua_test::INSTALLED_STRINGS),
            target.join(wowlua_test::INSTALLED_STRINGS),
        )?;
        fs::remove_dir_all(&part)?;
        fs::write(&marker, &casc.version)?;
    }
    let dumps = [
        ("strings", "GlobalStrings/enUS.lua"),
        ("api", "GlobalAPI.lua"),
        ("events", "Events.lua"),
    ];
    for ((_, branch, commit), (kind, file)) in STRINGS
        .iter()
        .filter(|(flavor, _, _)| wanted(flavor))
        .flat_map(|s| dumps.iter().map(move |d| (s, *d)))
    {
        let path = dir.join(kind).join(format!("{commit}.lua"));
        if path.exists() {
            continue;
        }
        fs::create_dir_all(path.parent().expect("a directory"))?;
        println!("{kind}: fetching {branch} {commit}");
        let url = format!(
            "https://raw.githubusercontent.com/Ketho/BlizzardInterfaceResources/{commit}/Resources/{file}"
        );
        let status = Command::new("curl")
            .args(["-fsSL", "-o"])
            .arg(&path)
            .arg(&url)
            .status()
            .context("running curl")?;
        if !status.success() {
            bail!("downloading {url} failed");
        }
    }
    if only.is_some() {
        return Ok(());
    }
    let ketho = dir.join("ketho");
    let marker = ketho.join(".wowlua-commit");
    if fs::read_to_string(&marker).ok().as_deref() != Some(KETHO_COMMIT) {
        if ketho.exists() {
            fs::remove_dir_all(&ketho)?;
        }
        println!("ketho: fetching vscode-wow-api {KETHO_COMMIT}");
        let k = ketho.to_string_lossy().into_owned();
        git(
            &[
                "clone",
                "-q",
                "--filter=blob:none",
                "--no-checkout",
                "--sparse",
                KETHO_REPO,
                &k,
            ],
            None,
        )?;
        git(
            &["sparse-checkout", "set", "Annotations/Core"],
            Some(&ketho),
        )?;
        git(&["checkout", "-q", KETHO_COMMIT], Some(&ketho))?;
        fs::write(&marker, KETHO_COMMIT)?;
    }
    Ok(())
}

/// The Lua and XML files of Blizzard's UI code, without the documentation.
fn ui_files(
    dir: &Path,
    lua: &mut Vec<(String, String)>,
    xml: &mut Vec<(String, String)>,
) -> Result<()> {
    let mut entries: Vec<PathBuf> = fs::read_dir(dir)?
        .filter_map(|e| e.ok().map(|e| e.path()))
        .collect();
    entries.sort();
    for p in entries {
        if p.ends_with("Blizzard_APIDocumentationGenerated")
            || p.ends_with("Blizzard_APIDocumentation")
        {
            continue;
        }
        if p.is_dir() {
            ui_files(&p, lua, xml)?;
            continue;
        }
        let target = match p.extension().and_then(|e| e.to_str()) {
            Some("lua") => &mut *lua,
            Some("xml") => &mut *xml,
            _ => continue,
        };
        let text = String::from_utf8_lossy(&fs::read(&p)?).into_owned();
        target.push((p.to_string_lossy().into_owned(), text));
    }
    Ok(())
}

/// The files only Blizzard's load-on-demand addons load (from their TOCs
/// and the XML files those include; `[Bootstrap]` files load at startup):
/// path (lowercase, `/`) → addon.
fn lod_files(dir: &Path) -> Result<HashMap<String, String>> {
    let mut lod: HashMap<String, String> = HashMap::new();
    let mut always: HashSet<String> = HashSet::new();
    let mut addons: Vec<PathBuf> = fs::read_dir(dir)?
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.is_dir())
        .collect();
    addons.sort();
    for folder in addons {
        let Some(addon) = folder
            .file_name()
            .and_then(|n| n.to_str())
            .map(str::to_string)
        else {
            continue;
        };
        let mut tocs: Vec<PathBuf> = fs::read_dir(&folder)?
            .filter_map(|e| e.ok().map(|e| e.path()))
            .filter(|p| p.extension().is_some_and(|e| e == "toc"))
            .collect();
        tocs.sort();
        for toc in tocs {
            let text = String::from_utf8_lossy(&fs::read(&toc)?).into_owned();
            let is_lod = text
                .lines()
                .any(|l| l.replace(' ', "").eq_ignore_ascii_case("##LoadOnDemand:1"));
            for line in text.lines().map(str::trim) {
                if line.is_empty() || line.starts_with('#') {
                    continue;
                }
                // `path [Attribute …]`: variables like `[Family]` follow no space.
                let (path, attrs) = match line.find(" [") {
                    Some(i) => (&line[..i], &line[i..]),
                    None => (line, ""),
                };
                let bootstrap = attrs.contains("[Bootstrap]");
                let mut files = Vec::new();
                for f in resolve(&folder, path) {
                    loaded(&f, &mut files, 0);
                }
                for f in files {
                    let key = key_of(&f);
                    if is_lod && !bootstrap {
                        lod.entry(key).or_insert_with(|| addon.clone());
                    } else {
                        always.insert(key);
                    }
                }
            }
        }
    }
    lod.retain(|k, _| !always.contains(k));
    Ok(lod)
}

/// A path as `lod_files` keys it.
fn key_of(p: &Path) -> String {
    p.to_string_lossy().replace('\\', "/").to_lowercase()
}

/// The existing files a TOC or XML path names (`\\` separated, `..`
/// allowed, `[Variable]` segments matching any name).
fn resolve(base: &Path, rel: &str) -> Vec<PathBuf> {
    let mut current = vec![base.to_path_buf()];
    for seg in rel
        .split(['\\', '/'])
        .filter(|s| !s.is_empty() && *s != ".")
    {
        let mut next = Vec::new();
        for dir in &current {
            if seg == ".." {
                if let Some(p) = dir.parent() {
                    next.push(p.to_path_buf());
                }
            } else if seg.contains('[') {
                let pattern = seg.to_lowercase();
                for e in fs::read_dir(dir).into_iter().flatten().flatten() {
                    let name = e.file_name().to_string_lossy().to_lowercase();
                    if matches_variables(&pattern, &name) {
                        next.push(e.path());
                    }
                }
            } else {
                // Case-insensitive, as the client.
                let lower = seg.to_lowercase();
                let found = fs::read_dir(dir)
                    .into_iter()
                    .flatten()
                    .flatten()
                    .find(|e| e.file_name().to_string_lossy().to_lowercase() == lower);
                if let Some(e) = found {
                    next.push(e.path());
                }
            }
        }
        current = next;
    }
    current.into_iter().filter(|p| p.is_file()).collect()
}

/// `name` matches `pattern`, where `[X]` parts match anything.
fn matches_variables(pattern: &str, name: &str) -> bool {
    let mut parts: Vec<&str> = Vec::new();
    let mut rest = pattern;
    while let Some(open) = rest.find('[') {
        parts.push(&rest[..open]);
        match rest[open..].find(']') {
            Some(close) => rest = &rest[open + close + 1..],
            None => return false,
        }
    }
    parts.push(rest);
    let mut pos = 0;
    for (i, part) in parts.iter().enumerate() {
        match name[pos..].find(part) {
            Some(at) if i > 0 || at == 0 => pos += at + part.len(),
            _ => return false,
        }
    }
    parts.last().is_some_and(|p| p.is_empty()) || pos == name.len()
}

/// A loaded file and, for XML, the files it includes.
fn loaded(file: &Path, out: &mut Vec<PathBuf>, depth: u32) {
    out.push(file.to_path_buf());
    if depth > 16
        || !file
            .extension()
            .is_some_and(|e| e.eq_ignore_ascii_case("xml"))
    {
        return;
    }
    let Ok(text) = fs::read(file) else { return };
    let text = String::from_utf8_lossy(&text);
    let lower = text.to_lowercase();
    let base = file.parent().unwrap_or(Path::new("."));
    for tag in ["<script", "<include"] {
        let mut from = 0;
        while let Some(at) = lower[from..].find(tag) {
            let start = from + at;
            let end = lower[start..].find('>').map_or(lower.len(), |e| start + e);
            if let Some(f) = lower[start..end].find("file=\"") {
                let value_start = start + f + "file=\"".len();
                if let Some(len) = text[value_start..end].find('"') {
                    for inc in resolve(base, &text[value_start..value_start + len]) {
                        loaded(&inc, out, depth + 1);
                    }
                }
            }
            from = end;
        }
    }
}

fn lua_files(dir: &Path, out: &mut Vec<(String, String)>) -> Result<()> {
    let mut entries: Vec<PathBuf> = fs::read_dir(dir)?
        .filter_map(|e| e.ok().map(|e| e.path()))
        .collect();
    entries.sort();
    for p in entries {
        if p.is_dir() {
            lua_files(&p, out)?;
        } else if p.extension().is_some_and(|e| e == "lua") {
            let text = String::from_utf8_lossy(&fs::read(&p)?).into_owned();
            out.push((p.to_string_lossy().into_owned(), text));
        }
    }
    Ok(())
}

/// Global names the compiler's own declarations define
/// (`crates/wowlua-stdlib/decl/`).
fn compiler_globals(root: &Path) -> Result<HashSet<String>> {
    let mut text = String::new();
    for entry in fs::read_dir(root.join("crates/wowlua-stdlib/decl"))? {
        text.push_str(&fs::read_to_string(entry?.path())?);
        text.push('\n');
    }
    let mut out = HashSet::new();
    for line in text.lines() {
        for prefix in ["declare function ", "declare namespace ", "declare global "] {
            if let Some(rest) = line.strip_prefix(prefix) {
                let name: String = rest
                    .chars()
                    .take_while(|c| c.is_ascii_alphanumeric() || *c == '_')
                    .collect();
                out.insert(name);
            }
        }
    }
    Ok(out)
}

/// The type names the compiler's declaration files define (classes,
/// interfaces, aliases).
fn compiler_type_names(root: &Path) -> Result<HashSet<String>> {
    let mut out = HashSet::new();
    for entry in fs::read_dir(root.join("crates/wowlua-stdlib/decl"))? {
        for line in fs::read_to_string(entry?.path())?.lines() {
            // `@widget("Frame") declare class Frame …`: after a decorator.
            let line = match line.strip_prefix('@') {
                Some(rest) => rest.split_once(") ").map_or("", |(_, r)| r),
                None => line,
            };
            for prefix in [
                "declare class ",
                "declare interface ",
                "interface ",
                "type ",
            ] {
                if let Some(rest) = line.strip_prefix(prefix) {
                    let name: String = rest
                        .chars()
                        .take_while(|c| c.is_ascii_alphanumeric() || *c == '_')
                        .collect();
                    if !name.is_empty() {
                        out.insert(name);
                    }
                }
            }
        }
    }
    Ok(out)
}

/// What the camelot client told of its undocumented C functions, as the
/// engine recorded it (decision T294): its own argument checks
/// (`camelot-usages.txt`, the first line of each function: the usage's
/// argument names, `[x]` or a `?` check optional) and its answers to calls
/// without arguments (`blanks.rs`).
fn client_facts(root: &Path) -> Result<wowlua_types_gen::ClientFacts> {
    let path = root.join("crates/wowlua-engine/data/camelot-usages.txt");
    let text = fs::read_to_string(&path).with_context(|| path.display().to_string())?;
    let mut usages: Vec<(String, Vec<wowlua_types_gen::UsageArg>)> = Vec::new();
    for line in text.lines().filter(|l| !l.starts_with('#')) {
        let mut cols = line.split('\t');
        let (Some(name), Some(checks), Some(message)) = (cols.next(), cols.next(), cols.next())
        else {
            continue;
        };
        if usages.iter().any(|(n, _)| n == name) {
            continue;
        }
        // `Usage: Name(a, [b])`: the names, bracketed ones optional
        let inner = message
            .split_once('(')
            .and_then(|(_, r)| r.rsplit_once(')'))
            .map_or("", |(a, _)| a);
        let names: Vec<(String, bool)> = inner
            .split(',')
            .map(|a| {
                let bracketed = a.contains('[');
                let n = a.trim().trim_matches(|c| c == '[' || c == ']').trim();
                (n.to_string(), bracketed)
            })
            .filter(|(n, _)| !n.is_empty())
            .collect();
        let checks: Vec<&str> = checks.split(',').filter(|c| !c.is_empty()).collect();
        let args = checks
            .iter()
            .enumerate()
            .map(|(i, check)| {
                let optional = check.ends_with('?') || *check == "optnumber";
                let (name, bracketed) = names
                    .get(i)
                    .cloned()
                    .filter(|(n, _)| n.chars().all(|c| c.is_ascii_alphanumeric() || c == '_'))
                    .unwrap_or_else(|| (format!("arg{}", i + 1), false));
                (
                    name,
                    check.trim_end_matches('?').to_string(),
                    optional || bracketed,
                )
            })
            .collect();
        usages.push((name.to_string(), args));
    }
    let answers = wowlua_engine::blanks::all()
        .iter()
        .map(|(n, a)| (n.to_string(), a.to_string()))
        .collect();
    Ok(wowlua_types_gen::ClientFacts {
        flavor: "camelot".into(),
        usages,
        answers,
    })
}

/// The flavors whose pinned dump (`STRINGS`) is their own client's.
const CLIENT_DUMPS: &[&str] = &["standard", "mists", "tbc", "vanilla"];

/// C functions the clients have and their dumps lack (recorded 2026-10-28 on
/// Retail 12.1.5, Mists 5.5.4, TBC 2.5.6 and Classic Era 1.15.9).
const NOT_IN_DUMPS: &[(&str, &str)] = &[
    (
        "standard",
        "C_SocialRestrictions.IsAgeVerificationRestricted",
    ),
    ("standard", "Kiosk.GetCharacterTemplateSetIndex"),
    ("mists", "Kiosk.GetCharacterTemplateSetIndex"),
    ("tbc", "Kiosk.GetCharacterTemplateSetIndex"),
    ("vanilla", "Kiosk.GetCharacterTemplateSetIndex"),
];

/// Writes `crates/wowlua-engine/data/<flavor>-functions.txt`: the dump's C
/// functions (its `GlobalAPI` table, not `LuaAPI`) and those it lacks
/// ([`NOT_IN_DUMPS`]), one per line.
fn write_inventory(root: &Path, flavor: &str, branch: &str, commit: &str, api: &str) -> Result<()> {
    let c_functions = api.split("local LuaAPI").next().unwrap_or_default();
    let mut names: Vec<String> = wowlua_types_gen::framexml::quoted_names(c_functions)
        .into_iter()
        .chain(
            NOT_IN_DUMPS
                .iter()
                .filter(|(f, _)| *f == flavor)
                .map(|(_, n)| n.to_string()),
        )
        .collect();
    names.sort();
    names.dedup();
    let mut out = format!(
        "# The C functions of the {flavor} client (Ketho's BlizzardInterfaceResources {branch}\n# {commit}, GlobalAPI.lua; `cargo xtask types gen`), one per line.\n"
    );
    for name in names {
        out.push_str(&name);
        out.push('\n');
    }
    let path = root.join(format!("crates/wowlua-engine/data/{flavor}-functions.txt"));
    fs::write(&path, out).with_context(|| path.display().to_string())
}

pub fn generate(root: &Path, dir: &Path) -> Result<()> {
    let mut flavors = Vec::new();
    for (flavor, _, _) in flavor_tags()? {
        let docs = dir.join(&flavor).join(DOCS_DIR);
        if !docs.exists() {
            bail!(
                "{} is missing: run `cargo xtask types fetch` first",
                docs.display()
            );
        }
        let mut files = Vec::new();
        lua_files(&docs, &mut files)?;
        // Blizzard's UI code (the documentation excluded).
        let mut ui_lua = Vec::new();
        let mut ui_xml = Vec::new();
        ui_files(
            &dir.join(&flavor).join("Interface/AddOns"),
            &mut ui_lua,
            &mut ui_xml,
        )?;
        let (branch, commit) = STRINGS
            .iter()
            .find(|(f, _, _)| *f == flavor)
            .map(|(_, b, c)| (*b, *c))
            .with_context(|| format!("no global strings for {flavor}"))?;
        // (an installed client's own, `fetch` read them: T553)
        let own = dir.join(&flavor).join(wowlua_test::INSTALLED_STRINGS);
        let strings_text = fs::read_to_string(&own)
            .or_else(|_| fs::read_to_string(dir.join("strings").join(format!("{commit}.lua"))))
            .context("global strings missing: run `cargo xtask types fetch`")?;
        // The engine's inventory of the client's C functions, where the
        // dump is the flavor's own client and `/stdtest inventory` has not
        // recorded it (camelot's).
        if CLIENT_DUMPS.contains(&flavor.as_str()) {
            let api = fs::read_to_string(dir.join("api").join(format!("{commit}.lua")))
                .context("global API names missing: run `cargo xtask types fetch`")?;
            write_inventory(root, &flavor, branch, commit, &api)?;
        }
        let lod_files = lod_files(&dir.join(&flavor).join("Interface/AddOns"))?;
        // The build on disk, as the mirror's commit names it:
        // `1.60.1 (70425)`.
        let build = git(&["log", "-1", "--format=%s"], Some(&dir.join(&flavor)))?;
        let build = build.trim();
        let ui_sha = git(&["rev-parse", "HEAD"], Some(&dir.join(&flavor)))?;
        // In full: the test runner fetches this commit (`test.blizzard-ui`).
        let ui_commit = ui_sha.trim();
        // The installed client's code in its place (`fetch`, T553): its
        // version named, and the commit CI fetches.
        let installed = fs::read_to_string(dir.join(&flavor).join(".wowlua-installed")).ok();
        let ui = match &installed {
            Some(version) => format!(
                "the installed client {version}, its UI code and global strings; in CI wow-ui-source {build}, commit {ui_commit}"
            ),
            None => format!("wow-ui-source {build}, commit {ui_commit}"),
        };
        // The engine's record of the number globals the client defines
        // itself (`/stdtest engine`, `scalars`), where there is one.
        let number_globals: Vec<String> = fs::read_to_string(
            root.join(format!("crates/wowlua-engine/data/{flavor}-globals.txt")),
        )
        .map(|text| {
            text.lines()
                .filter(|l| !l.starts_with('#'))
                .filter_map(|l| l.split('\t').next())
                .filter(|n| !n.is_empty())
                .map(str::to_string)
                .collect()
        })
        .unwrap_or_default();
        flavors.push(wowlua_types_gen::FlavorDocs {
            id: flavor,
            source: format!("{ui}; global string and API names from Ketho's BlizzardInterfaceResources {branch} {commit}"),
            files,
            ui_lua,
            ui_xml,
            strings: wowlua_types_gen::framexml::global_strings(&strings_text),
            api_names: Some(wowlua_types_gen::framexml::quoted_names(
                &fs::read_to_string(dir.join("api").join(format!("{commit}.lua")))
                    .context("global API names missing: run `cargo xtask types fetch`")?,
            )),
            event_names: Some(wowlua_types_gen::framexml::quoted_names(
                &fs::read_to_string(dir.join("events").join(format!("{commit}.lua")))
                    .context("event names missing: run `cargo xtask types fetch`")?,
            )),
            lod_files,
            number_globals,
        });
    }
    let mut annotations = Vec::new();
    let core = dir.join("ketho/Annotations/Core");
    for d in KETHO_DIRS {
        lua_files(&core.join(d), &mut annotations)?;
    }
    for f in KETHO_FILES {
        let path = core.join(f);
        let text =
            String::from_utf8_lossy(&fs::read(&path).with_context(|| path.display().to_string())?)
                .into_owned();
        annotations.push((path.to_string_lossy().into_owned(), text));
    }
    let skip = compiler_globals(root)?;
    let compiler_types = compiler_type_names(root)?;
    let source = format!("Ketho's vscode-wow-api annotations (MIT License, commit {KETHO_COMMIT})");
    // warcraft.wiki.gg's signatures, as `cargo xtask types wiki` reviewed
    // them (decision T288).
    let wiki_path = root.join(WIKI_DATA);
    let wiki = vec![(
        wiki_path.to_string_lossy().into_owned(),
        fs::read_to_string(&wiki_path).unwrap_or_default(),
    )];
    let client = client_facts(root)?;
    // what the functions do (`cargo xtask types wiki`), and wowlua's own
    // descriptions where nothing documents one
    let sources = wowlua_types_gen::docs::Sources {
        wiki: fs::read_to_string(root.join(WIKI_DOCS))
            .ok()
            .and_then(|t| serde_json::from_str(&t).ok())
            .unwrap_or_default(),
        written: fs::read_to_string(root.join(WRITTEN_DOCS))
            .ok()
            .and_then(|t| serde_json::from_str(&t).ok())
            .unwrap_or_default(),
    };
    let output = wowlua_types_gen::generate(
        &flavors,
        &annotations,
        &source,
        &skip,
        &compiler_types,
        &wiki,
        &client,
        &sources,
    )
    .map_err(anyhow::Error::msg)?;
    let data = root.join("crates/wowlua-api/data");
    crate::write_if_changed(
        &data.join("LICENSE-warcraft-wiki.txt"),
        "The declarations in this directory include function signatures, records and\nthe descriptions of functions and their values (doc comments) adapted from\nwarcraft.wiki.gg's API pages (https://warcraft.wiki.gg), by its contributors,\nas `xtask/data/warcraft-wiki-api.lua` and `xtask/data/warcraft-wiki-docs.json`\nhold them. That content is licensed under the Creative Commons\nAttribution-ShareAlike 4.0 International License\n(https://creativecommons.org/licenses/by-sa/4.0/); the adapted declarations are\nshared under the same license.\n",
    )?;
    fs::create_dir_all(&data)?;
    // The MIT notice of the annotations goes with the generated files.
    let license = git(
        &["show", &format!("{KETHO_COMMIT}:LICENSE")],
        Some(&dir.join("ketho")),
    )?;
    crate::write_if_changed(
        &data.join("LICENSE-vscode-wow-api.txt"),
        &format!(
            "The declarations in this directory include information converted from\nKetho's vscode-wow-api (https://github.com/Ketho/vscode-wow-api), commit\n{KETHO_COMMIT}, under this license:\n\n{license}"
        ),
    )?;
    // The documentation as the engine reads it: compact JSON, gzipped
    // without a timestamp, so unchanged documentation writes the same bytes.
    for (flavor, json) in &output.api {
        let text = serde_json::to_vec(json)?;
        let mut gz = flate2::GzBuilder::new()
            .mtime(0)
            .write(Vec::new(), flate2::Compression::best());
        std::io::Write::write_all(&mut gz, &text)?;
        let bytes = gz.finish()?;
        let path = data.join(format!("{flavor}.api.json.gz"));
        if fs::read(&path).ok().as_deref() != Some(bytes.as_slice()) {
            fs::write(&path, &bytes).with_context(|| format!("writing {}", path.display()))?;
            println!("updated {}", path.display());
        }
    }
    let mut written = vec![("shared".to_string(), output.shared)];
    written.extend(output.flavors);
    for (name, text) in &written {
        // The output must parse.
        let mut db = wowlua_span::SourceDb::new();
        let file = db.add(format!("{name}.d.wowlua"), text.clone());
        let parse = wowlua_syntax::parse(text.as_bytes(), file);
        if parse.has_errors() {
            let first = &parse.diagnostics[0];
            let at = first
                .labels
                .first()
                .map(|l| l.span.start as usize)
                .unwrap_or(0);
            let line = text[..at.min(text.len())].matches('\n').count() + 1;
            let context: Vec<&str> = text.lines().skip(line.saturating_sub(3)).take(5).collect();
            bail!(
                "{name}.d.wowlua:{line}: {} ({} errors)\n{}",
                first.message,
                parse.diagnostics.len(),
                context.join("\n")
            );
        }
        let path = data.join(format!("{name}.d.wowlua"));
        crate::write_if_changed(&path, text)?;
        println!("{name}: {} KB", text.len() / 1024);
    }
    // Every flavor's declarations resolve with the compiler's own.
    let lua = fs::read_to_string(root.join("crates/wowlua-stdlib/decl/lua.d.wowlua"))?;
    let wow = fs::read_to_string(root.join("crates/wowlua-stdlib/decl/wow.d.wowlua"))?;
    for (name, text) in written.iter().skip(1) {
        let diagnostics = validate(&[
            ("lua", &lua),
            ("wow", &wow),
            ("shared", &written[0].1),
            (name, text),
        ]);
        if !diagnostics.is_empty() {
            for d in diagnostics.iter().take(10) {
                println!("  {name}: {d}");
            }
            bail!(
                "{name}: {} problems in the generated declarations",
                diagnostics.len()
            );
        }
    }
    Ok(())
}

/// Builds the type environment from declaration files; its diagnostics.
fn validate(files: &[(&str, &String)]) -> Vec<String> {
    let mut db = wowlua_span::SourceDb::new();
    let parses: Vec<(String, wowlua_syntax::Parse)> = files
        .iter()
        .map(|(n, t)| {
            let file = db.add(format!("{n}.d.wowlua"), (*t).clone());
            (
                format!("{n}.d.wowlua"),
                wowlua_syntax::parse(t.as_bytes(), file),
            )
        })
        .collect();
    let inputs: Vec<wowlua_sema::FileInput> = parses
        .iter()
        .map(|(n, p)| wowlua_sema::FileInput {
            src_rel: n.clone(),
            kind: wowlua_sema::FileKind::Declaration,
            parse: Some(p),
            xml: None,
            meta: None,
            main_of: None,
            // The texts here are complete: nothing is taken out.
            globals: Default::default(),
            usage: None,
            prepared: None,
        })
        .collect();
    let analysis = wowlua_sema::analyze(&inputs, None);
    let settings = vec![None; inputs.len()];
    wowlua_sema::check::check_package(&inputs, &analysis, &settings, &Default::default(), None)
        .into_iter()
        .map(|d| {
            let at = d.labels.first().map(|l| {
                let (path, text) = files[l.span.file.index()];
                let line = text[..(l.span.start as usize).min(text.len())]
                    .matches('\n')
                    .count()
                    + 1;
                format!("{path}:{line}: ")
            });
            format!("{}{}", at.unwrap_or_default(), d.message)
        })
        .collect()
}

const WIKI_API: &str = "https://warcraft.wiki.gg/api.php";
const WIKI_AGENT: &str =
    "wowlua-types-gen (https://github.com/DomenikIrrgang/wowlua; API types for addon authors)";
/// The wiki's API namespace.
const WIKI_API_NAMESPACE: &str = "3000";
/// The reviewed wiki data the generator reads (decision T288).
pub const WIKI_DATA: &str = "xtask/data/warcraft-wiki-api.lua";

/// What warcraft.wiki.gg's API pages say the functions do (CC BY-SA 4.0),
/// by function, for the declarations' doc comments.
pub const WIKI_DOCS: &str = "xtask/data/warcraft-wiki-docs.json";

/// wowlua's own one-line descriptions of the functions no documentation
/// describes (written from their names, signatures and the emulator's
/// behaviour), marked as such in the doc comments.
pub const WRITTEN_DOCS: &str = "xtask/data/written-docs.json";

/// One request to the wiki's API (form-encoded), paced: a second apart.
fn wiki_query(params: &[(&str, &str)]) -> Result<serde_json::Value> {
    let mut cmd = Command::new("curl");
    cmd.args(["-fsS", "--retry", "3", "-A", WIKI_AGENT]);
    for (k, v) in params {
        cmd.arg("--data-urlencode").arg(format!("{k}={v}"));
    }
    cmd.arg(WIKI_API);
    let out = cmd.output().context("running curl")?;
    std::thread::sleep(std::time::Duration::from_secs(1));
    if !out.status.success() {
        bail!(
            "the wiki's API failed: {}",
            String::from_utf8_lossy(&out.stderr)
        );
    }
    Ok(serde_json::from_slice(&out.stdout)?)
}

/// The pages' texts by title (and the newest revision among them), from
/// `cache` or the wiki, 50 a request.
fn wiki_pages(cache: &Path, titles: &[String]) -> Result<(HashMap<String, String>, u64)> {
    let mut known: serde_json::Map<String, serde_json::Value> = fs::read(cache)
        .ok()
        .and_then(|b| serde_json::from_slice(&b).ok())
        .unwrap_or_default();
    let missing: Vec<&String> = titles.iter().filter(|t| !known.contains_key(*t)).collect();
    for (i, batch) in missing.chunks(50).enumerate() {
        println!(
            "wiki: pages {}-{} of {}",
            i * 50 + 1,
            i * 50 + batch.len(),
            missing.len()
        );
        let joined = batch
            .iter()
            .map(|t| t.as_str())
            .collect::<Vec<_>>()
            .join("|");
        let d = wiki_query(&[
            ("action", "query"),
            ("prop", "revisions"),
            ("rvprop", "content|ids"),
            ("rvslots", "main"),
            ("format", "json"),
            ("formatversion", "2"),
            ("redirects", "1"),
            ("titles", &joined),
        ])?;
        // The titles asked for, by the page that answers them (through
        // normalization and redirects).
        let mut asked: HashMap<String, Vec<String>> = HashMap::new();
        for kind in ["normalized", "redirects"] {
            for r in d["query"][kind].as_array().into_iter().flatten() {
                let (Some(from), Some(to)) = (r["from"].as_str(), r["to"].as_str()) else {
                    continue;
                };
                let mut froms = asked.remove(from).unwrap_or_default();
                froms.push(from.to_string());
                asked.entry(to.to_string()).or_default().extend(froms);
            }
        }
        for p in d["query"]["pages"].as_array().into_iter().flatten() {
            let title = p["title"].as_str().unwrap_or_default().to_string();
            for from in asked.get(&title).into_iter().flatten() {
                if let Some(r) = p["revisions"].get(0) {
                    known.insert(
                        from.clone(),
                        serde_json::json!({
                            "rev": r["revid"],
                            "text": r["slots"]["main"]["content"],
                        }),
                    );
                }
            }
            let entry = match p["revisions"].get(0) {
                Some(r) => serde_json::json!({
                    "rev": r["revid"],
                    "text": r["slots"]["main"]["content"],
                }),
                // no such page: remembered, not asked again
                None => serde_json::json!({ "rev": 0, "text": "" }),
            };
            known.insert(title, entry);
        }
        // asked for but not answered (normalized titles): none
        for t in batch {
            known
                .entry(t.to_string())
                .or_insert_with(|| serde_json::json!({ "rev": 0, "text": "" }));
        }
        fs::write(cache, serde_json::to_vec(&known)?)?;
    }
    let mut rev = 0;
    let texts = titles
        .iter()
        .filter_map(|t| {
            let e = known.get(t)?;
            rev = rev.max(e["rev"].as_u64().unwrap_or(0));
            Some((t.clone(), e["text"].as_str()?.to_string()))
        })
        .filter(|(_, text)| !text.is_empty())
        .collect();
    Ok((texts, rev))
}

/// The titles of the wiki's API pages (redirects left out).
fn wiki_api_titles(cache: &Path) -> Result<Vec<String>> {
    if let Some(t) = fs::read(cache)
        .ok()
        .and_then(|b| serde_json::from_slice::<Vec<String>>(&b).ok())
    {
        return Ok(t);
    }
    let mut titles = Vec::new();
    let mut from = String::new();
    loop {
        let mut q = vec![
            ("action", "query"),
            ("list", "allpages"),
            ("apnamespace", WIKI_API_NAMESPACE),
            ("aplimit", "max"),
            ("apfilterredir", "nonredirects"),
            ("format", "json"),
            ("formatversion", "2"),
        ];
        if !from.is_empty() {
            q.push(("apcontinue", &from));
        }
        let d = wiki_query(&q)?;
        for p in d["query"]["allpages"].as_array().into_iter().flatten() {
            if let Some(t) = p["title"].as_str() {
                titles.push(t.to_string());
            }
        }
        match d["continue"]["apcontinue"].as_str() {
            Some(c) => from = c.to_string(),
            None => break,
        }
    }
    fs::write(cache, serde_json::to_vec(&titles)?)?;
    Ok(titles)
}

/// The functions Blizzard's documentation of any flavor types: the wiki
/// does not stand in for them.
fn documented(dir: &Path) -> Result<HashSet<String>> {
    let ns = regex::Regex::new(r#"Namespace\s*=\s*"(\w+)""#)?;
    let func = regex::Regex::new(r#"Name\s*=\s*"(\w+)",\s*\n\s*Type\s*=\s*"Function""#)?;
    let mut out = HashSet::new();
    for (flavor, _, _) in flavor_tags()? {
        let mut files = Vec::new();
        lua_files(&dir.join(&flavor).join(DOCS_DIR), &mut files)?;
        for (_, text) in files {
            let prefix = ns
                .captures(&text)
                .map(|c| format!("{}.", &c[1]))
                .unwrap_or_default();
            for c in func.captures_iter(&text) {
                out.insert(format!("{prefix}{}", &c[1]));
            }
        }
    }
    Ok(out)
}

/// `cargo xtask types wiki [dir]`: the signatures of warcraft.wiki.gg's API
/// pages (CC BY-SA 4.0) for the functions Blizzard's documentation leaves
/// out, with the records they name, as LuaLS annotations in [`WIKI_DATA`]
/// (decision T288). The pages are cached under `dir/wiki`; delete it to
/// read the wiki again.
pub fn wiki(root: &Path, dir: &Path) -> Result<()> {
    let cache = dir.join("wiki");
    fs::create_dir_all(&cache)?;
    let titles = wiki_api_titles(&cache.join("api_titles.json"))?;
    let (pages, mut rev) = wiki_pages(&cache.join("api_pages.json"), &titles)?;
    let documented = documented(dir)?;
    let mut functions = Vec::new();
    let mut refs = std::collections::BTreeSet::new();
    let mut ordered: Vec<(&String, &String)> = pages.iter().collect();
    ordered.sort();
    for (title, text) in ordered {
        let Some(f) = wowlua_types_gen::wiki::function(title, text) else {
            continue;
        };
        if documented.contains(&f.name) {
            continue;
        }
        refs.extend(wowlua_types_gen::wiki::struct_refs(text));
        functions.push(f);
    }
    // The records, and the records they show in turn.
    let mut struct_pages: HashMap<String, String> = HashMap::new();
    let mut wanted: Vec<String> = refs.into_iter().collect();
    while !wanted.is_empty() {
        let (got, struct_rev) = wiki_pages(&cache.join("struct_pages.json"), &wanted)?;
        rev = rev.max(struct_rev);
        let mut next = std::collections::BTreeSet::new();
        for (title, text) in got {
            for r in wowlua_types_gen::wiki::struct_refs(&text) {
                if !struct_pages.contains_key(&r) && r != title {
                    next.insert(r);
                }
            }
            struct_pages.insert(title, text);
        }
        wanted = next
            .into_iter()
            .filter(|r| !struct_pages.contains_key(r))
            .collect();
    }
    let mut structs: Vec<(String, Vec<wowlua_types_gen::wiki::Value>)> = Vec::new();
    let mut ordered: Vec<(&String, &String)> = struct_pages.iter().collect();
    ordered.sort();
    for (title, text) in ordered {
        let Some(name) = wowlua_types_gen::wiki::struct_name(title) else {
            continue;
        };
        let fields = wowlua_types_gen::wiki::struct_fields(text);
        if !fields.is_empty() && !structs.iter().any(|(n, _)| *n == name) {
            structs.push((name, fields));
        }
    }
    // What each function does, for the doc comments.
    let mut docs = wowlua_types_gen::docs::Docs::new();
    for (title, text) in &pages {
        if let Some((key, doc)) = wowlua_types_gen::docs::from_wiki(title, text) {
            docs.insert(key, doc);
        }
    }
    let docs_path = root.join(WIKI_DOCS);
    let mut docs_text = serde_json::to_string(&docs)?;
    // (one function a line: a change shows as the lines it touches)
    docs_text = docs_text.replace("},\"", "},\n\"");
    docs_text.push('\n');
    crate::write_if_changed(&docs_path, &docs_text)?;
    println!(
        "wiki: {} documented functions -> {}",
        docs.len(),
        docs_path.display()
    );
    let body = wowlua_types_gen::wiki::to_luals(&functions, &structs);
    let text = format!(
        "---@meta\n-- Generated by `cargo xtask types wiki` from warcraft.wiki.gg's API pages\n-- (newest revision {rev}): the functions Blizzard's documentation leaves\n-- out, and the records they name. The wiki's content is licensed under the\n-- Creative Commons Attribution-ShareAlike 4.0 License\n-- (https://creativecommons.org/licenses/by-sa/4.0/); so is this file, an\n-- adaptation of it. Authors: the contributors of https://warcraft.wiki.gg.\n\n{body}"
    );
    let path = root.join(WIKI_DATA);
    fs::create_dir_all(path.parent().expect("a directory"))?;
    crate::write_if_changed(&path, &text)?;
    println!(
        "wiki: {} functions, {} records -> {}",
        functions.len(),
        structs.len(),
        path.display()
    );
    Ok(())
}
