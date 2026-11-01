//! Blizzard's UI code for `test.blizzard-ui` (spec emulator §17.7, decision
//! T204): the installed client's own when the flavor is installed
//! (decision T553: read from the installation once per build, into
//! `$WOWLUA_HOME/cache/blizzard-ui/<flavor>-installed-<version>/`), else
//! wow-ui-source at the commit the flavor's API declarations were
//! generated from; with the global strings of the declarations' dump,
//! fetched on first use into `$WOWLUA_HOME/cache/blizzard-ui/`.
//! `WOWLUA_UI_SOURCE` names a folder laid out as `cargo xtask types fetch`
//! makes it (`<flavor>/Interface`, `strings/<commit>.lua`) to use instead;
//! `WOWLUA_UI_FROM=upstream` leaves the installation out.

use std::path::{Path, PathBuf};
use std::process::Command;

const UI_REPO: &str = "https://github.com/Gethe/wow-ui-source.git";

/// Where a flavor's UI code is: its `Interface` folder and its global
/// strings.
#[derive(Clone, Debug)]
pub struct UiSource {
    pub interface: PathBuf,
    pub strings: Option<PathBuf>,
}

/// The commits the flavor's declarations name: wow-ui-source's and the
/// global strings dump's.
pub fn commits(flavor: &str) -> Option<(String, Option<String>)> {
    let file = wowlua_api::own(flavor)?;
    let header: String = file.text.lines().take(3).collect::<Vec<_>>().join(" ");
    let ui = regex::Regex::new(r"wow-ui-source [^,]*, commit ([0-9a-f]{40})")
        .expect("a valid pattern")
        .captures(&header)?[1]
        .to_string();
    let strings = regex::Regex::new(r"BlizzardInterfaceResources \S+ ([0-9a-f]{40})")
        .expect("a valid pattern")
        .captures(&header)
        .map(|c| c[1].to_string());
    Some((ui, strings))
}

fn run(cmd: &mut Command, what: &str) -> Result<(), String> {
    let out = cmd
        .output()
        .map_err(|e| format!("{what}: cannot run {:?}: {e}", cmd.get_program()))?;
    if !out.status.success() {
        return Err(format!(
            "{what} failed: {}",
            String::from_utf8_lossy(&out.stderr).trim()
        ));
    }
    Ok(())
}

/// The flavor's UI code, fetched when the cache lacks it.
/// The UI code in a `cargo xtask types fetch` directory (`target/types-src`):
/// the flavor's `Interface` and its global strings, when there.
pub fn ui_source_in(dir: &Path, flavor: &str) -> Option<UiSource> {
    let (_, strings_commit) = commits(flavor)?;
    let interface = dir.join(flavor).join("Interface");
    if !wowlua_host::exists(&interface) {
        return None;
    }
    Some(UiSource {
        interface,
        strings: strings_commit
            .map(|c| dir.join("strings").join(format!("{c}.lua")))
            .filter(|p| wowlua_host::exists(p)),
    })
}

/// The flavor's UI code when it is there already: in `WOWLUA_UI_SOURCE`,
/// read from the installation, or fetched into the cache with its global
/// strings. Never fetches (wowlua studio's start page shows whether a test
/// run will have to).
pub fn cached_ui_source(flavor: &str) -> Option<UiSource> {
    let (commit, strings_commit) = commits(flavor)?;
    if let Some(dir) = std::env::var_os("WOWLUA_UI_SOURCE") {
        return ui_source_in(&PathBuf::from(dir), flavor);
    }
    // (an installed flavor's is the installation's, once read)
    #[cfg(feature = "native")]
    if let Some((_, dir)) = installed_dir(flavor) {
        if !wowlua_host::exists(dir.join(".wowlua-complete")) {
            return None;
        }
        let strings = strings_commit
            .as_ref()
            .and_then(|c| Some(cache_dir()?.join("strings").join(format!("{c}.lua"))))
            .filter(|p| wowlua_host::exists(p));
        return Some(UiSource {
            interface: dir.join("Interface"),
            strings,
        });
    }
    let cache = wowlua_driver::cache_root()?.join("blizzard-ui");
    let dir = cache.join(format!("{flavor}-{}", &commit[..12]));
    if !wowlua_host::exists(dir.join(".wowlua-complete")) {
        return None;
    }
    let strings = match strings_commit {
        Some(c) => {
            let path = cache.join("strings").join(format!("{c}.lua"));
            if !wowlua_host::exists(&path) {
                return None;
            }
            Some(path)
        }
        None => None,
    };
    Some(UiSource {
        interface: dir.join("Interface"),
        strings,
    })
}

/// Forgets the flavor's fetched UI code, so the next [`ui_source`] fetches
/// it again (T505); none fetched is no error.
#[allow(clippy::disallowed_methods)] // (the cache is the computer's)
pub fn forget_ui_source(flavor: &str) -> Result<(), String> {
    let (commit, _) = commits(flavor).ok_or_else(|| {
        format!("the `{flavor}` declarations do not name their wow-ui-source commit")
    })?;
    let Some(cache) = wowlua_driver::cache_root() else {
        return Ok(());
    };
    let dir = cache
        .join("blizzard-ui")
        .join(format!("{flavor}-{}", &commit[..12]));
    match std::fs::remove_dir_all(&dir) {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(format!("cannot remove `{}`: {e}", dir.display())),
    }
}

pub fn ui_source(flavor: &str, status: &dyn Fn(&str)) -> Result<UiSource, String> {
    let (commit, strings_commit) = commits(flavor).ok_or_else(|| {
        format!("the `{flavor}` declarations do not name their wow-ui-source commit")
    })?;
    if let Some(dir) = std::env::var_os("WOWLUA_UI_SOURCE") {
        return ui_source_in(&PathBuf::from(dir), flavor)
            .ok_or_else(|| format!("no `{flavor}` UI code in WOWLUA_UI_SOURCE"));
    }
    if let Some(cached) = cached_ui_source(flavor) {
        return Ok(cached);
    }
    let cache = wowlua_driver::cache_root()
        .ok_or("cannot find the cache (set `WOWLUA_HOME`)")?
        .join("blizzard-ui");
    // The installed client's own code, read once per build (T553).
    #[cfg(feature = "native")]
    if let Some((casc, dir)) = installed_dir(flavor) {
        status(&format!(
            "Blizzard's UI code for {flavor} from the installation ({})",
            casc.version
        ));
        let names = addon_names(&cache, &commit)?;
        let table = wowlua_flavors::table();
        let client = table.clients_of(flavor).next().ok_or("no client")?;
        let part = dir.with_extension("part");
        if wowlua_host::exists(&part) {
            wowlua_host::remove_dir_all(&part).map_err(|e| e.to_string())?;
        }
        crate::installed::extract(&casc, &names, &client.suffixes, &games(flavor), &part)?;
        wowlua_host::write(part.join(".wowlua-complete"), &casc.version)
            .map_err(|e| e.to_string())?;
        if wowlua_host::exists(&dir) {
            wowlua_host::remove_dir_all(&dir).map_err(|e| e.to_string())?;
        }
        wowlua_host::rename(&part, &dir).map_err(|e| e.to_string())?;
        let strings = match strings_commit {
            Some(c) => {
                let path = cache.join("strings").join(format!("{c}.lua"));
                if !wowlua_host::exists(&path) {
                    fetch_strings(&path, &c)?;
                }
                Some(path)
            }
            None => None,
        };
        return Ok(UiSource {
            interface: dir.join("Interface"),
            strings,
        });
    }
    let dir = cache.join(format!("{flavor}-{}", &commit[..12]));
    if !wowlua_host::exists(dir.join(".wowlua-complete")) {
        status(&format!(
            "Blizzard's UI code for {flavor} (wow-ui-source {})",
            &commit[..12]
        ));
        fetch_ui(&dir, &commit)?;
    }
    let strings = match strings_commit {
        Some(c) => {
            let path = cache.join("strings").join(format!("{c}.lua"));
            if !wowlua_host::exists(&path) {
                fetch_strings(&path, &c)?;
            }
            Some(path)
        }
        None => None,
    };
    Ok(UiSource {
        interface: dir.join("Interface"),
        strings,
    })
}

fn fetch_ui(dir: &Path, commit: &str) -> Result<(), String> {
    if wowlua_host::exists(dir) {
        wowlua_host::remove_dir_all(dir).map_err(|e| e.to_string())?;
    }
    wowlua_host::create_dir_all(dir).map_err(|e| e.to_string())?;
    let git = |args: &[&str]| -> Result<(), String> {
        run(
            Command::new("git").arg("-C").arg(dir).args(args),
            "fetching Blizzard's UI code with git",
        )
    };
    git(&["init", "-q"])?;
    git(&["remote", "add", "origin", UI_REPO])?;
    git(&["sparse-checkout", "set", "Interface/AddOns"])?;
    git(&[
        "fetch",
        "-q",
        "--depth",
        "1",
        "--filter=blob:none",
        "origin",
        commit,
    ])?;
    git(&["checkout", "-q", "FETCH_HEAD"])?;
    // Only the files are needed.
    let _ = wowlua_host::remove_dir_all(dir.join(".git"));
    wowlua_host::write(dir.join(".wowlua-complete"), commit).map_err(|e| e.to_string())
}

/// What an installed client's TOCs' `[Family]` and `[Game]` may stand for:
/// its family, its game types and TOC suffixes, as written and capitalized.
#[cfg(feature = "native")]
pub(crate) fn games(flavor: &str) -> Vec<String> {
    let table = wowlua_flavors::table();
    let mut out: Vec<String> = Vec::new();
    if let Some(f) = table.flavor(flavor) {
        out.push(f.family.name().to_owned());
    }
    for c in table.clients_of(flavor) {
        for g in c.game_types.iter().chain(&c.suffixes) {
            let cap: String = g
                .chars()
                .take(1)
                .flat_map(char::to_uppercase)
                .chain(g.chars().skip(1))
                .collect();
            for v in [g.clone(), cap] {
                if !out.contains(&v) {
                    out.push(v);
                }
            }
        }
    }
    out
}

fn cache_dir() -> Option<PathBuf> {
    Some(wowlua_driver::cache_root()?.join("blizzard-ui"))
}

/// The flavor's client in this machine's WoW installation, and where its
/// UI code is kept: none when it is not installed, or
/// `WOWLUA_UI_FROM=upstream`.
#[cfg(feature = "native")]
fn installed_dir(flavor: &str) -> Option<(wowlua_casc::Casc, PathBuf)> {
    if std::env::var("WOWLUA_UI_FROM").is_ok_and(|v| v == "upstream") {
        return None;
    }
    let root = wowlua_driver::install::find_wow(None).ok()?;
    let table = wowlua_flavors::table();
    let client = table.clients_of(flavor).next()?;
    let storage = wowlua_casc::FsStorage { root: root.clone() };
    let product = client
        .folders
        .iter()
        .find_map(|f| wowlua_casc::Casc::product_of_folder(&storage, f))?;
    let casc = wowlua_casc::Casc::open(Box::new(storage), &product).ok()?;
    let dir = cache_dir()?.join(format!("{flavor}-installed-{}", casc.version));
    Some((casc, dir))
}

/// Blizzard's addons' names at a wow-ui-source commit: where the crawl of
/// an installed client starts (T553). Listed by git without the files'
/// contents, kept per commit.
#[cfg(feature = "native")]
fn addon_names(cache: &Path, commit: &str) -> Result<Vec<String>, String> {
    let kept = cache.join("names").join(format!("{commit}.txt"));
    if let Ok(text) = wowlua_host::read_to_string(&kept) {
        return Ok(text.lines().map(str::to_owned).collect());
    }
    let dir = cache.join("names").join(format!("{commit}.git"));
    if wowlua_host::exists(&dir) {
        wowlua_host::remove_dir_all(&dir).map_err(|e| e.to_string())?;
    }
    wowlua_host::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let git = |args: &[&str]| -> Result<(), String> {
        run(
            Command::new("git").arg("-C").arg(&dir).args(args),
            "listing Blizzard's addons with git",
        )
    };
    git(&["init", "-q", "--bare"])?;
    git(&["remote", "add", "origin", UI_REPO])?;
    git(&[
        "fetch",
        "-q",
        "--depth",
        "1",
        "--filter=blob:none",
        "origin",
        commit,
    ])?;
    let out = Command::new("git")
        .arg("-C")
        .arg(&dir)
        .args(["ls-tree", "--name-only", commit, "Interface/AddOns/"])
        .output()
        .map_err(|e| format!("listing Blizzard's addons with git: {e}"))?;
    let names: Vec<String> = String::from_utf8_lossy(&out.stdout)
        .lines()
        .filter_map(|l| l.rsplit('/').next().map(str::to_owned))
        .filter(|n| !n.is_empty())
        .collect();
    if names.is_empty() {
        return Err("listing Blizzard's addons with git: none".into());
    }
    wowlua_host::write(&kept, names.join("\n")).map_err(|e| e.to_string())?;
    let _ = wowlua_host::remove_dir_all(&dir);
    Ok(names)
}

fn fetch_strings(path: &Path, commit: &str) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        wowlua_host::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    let url = format!(
        "https://raw.githubusercontent.com/Ketho/BlizzardInterfaceResources/{commit}/Resources/GlobalStrings/enUS.lua"
    );
    let part = path.with_extension("part");
    run(
        Command::new("curl")
            .args(["-fsSL", "-o"])
            .arg(&part)
            .arg(&url),
        "downloading the global strings with curl",
    )?;
    wowlua_host::rename(&part, path).map_err(|e| e.to_string())
}
