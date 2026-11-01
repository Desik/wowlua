//! Blizzard's UI code read from the WoW installation (decision T553): the
//! installed build's own files, not a copy of an older one. The client
//! names its interface files by hashes only, so the code is found as the
//! client finds it: from Blizzard's addons' names, each addon's TOC (the
//! client's suffixes in order, then the plain name), the files it lists,
//! the files their XML includes and the addons they load or depend on. The
//! files land in a folder laid out as wow-ui-source's (`Interface/AddOns`).

use std::collections::{BTreeSet, VecDeque};
use std::path::Path;

use wowlua_casc::Casc;

/// What a crawl found.
#[derive(Debug, Default)]
pub struct Extracted {
    /// The addons with a TOC, by name.
    pub addons: BTreeSet<String>,
    /// The files written (their paths under the folder).
    pub files: usize,
}

/// Writes the build's Blizzard UI code into `out` (its `Interface/AddOns`),
/// starting from `seeds` (addon names); `suffixes` are the client's TOC
/// suffixes in lookup order; `games` what a TOC's `[Family]` and `[Game]`
/// may stand for (its family, its game types: `Mainline`, `Camelot`), each
/// tried.
#[allow(clippy::disallowed_methods)] // (a cache folder of the computer's)
pub fn extract(
    casc: &Casc,
    seeds: &[String],
    suffixes: &[String],
    games: &[String],
    out: &Path,
) -> Result<Extracted, String> {
    let script = regex::Regex::new(r#"(?i)<(?:Script|Include)\b[^>]*?\bfile\s*=\s*"([^"]+)""#)
        .expect("a valid pattern");
    let loads =
        regex::Regex::new(r#"LoadAddOn\(\s*"([A-Za-z0-9_]+)"\s*\)"#).expect("a valid pattern");
    let mut done = Extracted::default();
    let mut queue: VecDeque<String> = seeds.iter().cloned().collect();
    let mut seen_addons: BTreeSet<String> = BTreeSet::new();
    let mut files: VecDeque<String> = VecDeque::new();
    let mut seen_files: BTreeSet<String> = BTreeSet::new();
    let write = |rel: &str, bytes: &[u8]| -> Result<(), String> {
        let path = out.join(rel);
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
        }
        std::fs::write(&path, bytes).map_err(|e| format!("{}: {e}", path.display()))
    };
    loop {
        // the addons first: their TOCs name more files
        if let Some(name) = queue.pop_front() {
            if !seen_addons.insert(name.to_ascii_lowercase()) {
                continue;
            }
            let dir = format!("Interface/AddOns/{name}");
            let toc = suffixes
                .iter()
                .map(|s| format!("{dir}/{name}_{s}.toc"))
                .chain([format!("{dir}/{name}.toc")])
                .find_map(|p| casc.file_by_path(&p).map(|b| (p, b)));
            let Some((path, bytes)) = toc else {
                continue;
            };
            write(&path, &bytes)?;
            done.files += 1;
            done.addons.insert(name.clone());
            let parsed = wowlua_toc::parse(&String::from_utf8_lossy(&bytes));
            for d in &parsed.directives {
                let key = d.key.to_ascii_lowercase();
                if [
                    "dependencies",
                    "requireddeps",
                    "dep",
                    "optionaldeps",
                    "loadwith",
                ]
                .iter()
                .any(|k| key == *k || key.starts_with(&format!("{k}-")))
                {
                    queue.extend(
                        d.value
                            .split(',')
                            .map(str::trim)
                            .filter(|n| !n.is_empty())
                            .map(str::to_owned),
                    );
                }
            }
            for f in &parsed.files {
                let rel = f.path.replace("[TextLocale]", "enUS");
                if rel.contains("[Family]") || rel.contains("[Game]") {
                    // (each value tried: the files that are there are kept)
                    for g in games {
                        let one = rel.replace("[Family]", g).replace("[Game]", g);
                        files.push_back(normalize(&format!("{dir}/{one}")));
                    }
                } else {
                    files.push_back(normalize(&format!("{dir}/{rel}")));
                }
            }
            continue;
        }
        let Some(path) = files.pop_front() else {
            break;
        };
        if !seen_files.insert(path.to_ascii_lowercase()) {
            continue;
        }
        let Some(bytes) = casc.file_by_path(&path) else {
            continue;
        };
        write(&path, &bytes)?;
        done.files += 1;
        let text = String::from_utf8_lossy(&bytes);
        let lower = path.to_ascii_lowercase();
        if lower.ends_with(".xml") {
            let here = path.rsplit_once('/').map_or("", |(d, _)| d);
            for c in script.captures_iter(&text) {
                files.push_back(normalize(&format!("{here}/{}", c[1].replace('\\', "/"))));
            }
        }
        if lower.ends_with(".lua") || lower.ends_with(".xml") {
            queue.extend(loads.captures_iter(&text).map(|c| c[1].to_string()));
        }
    }
    Ok(done)
}

/// `a/b/../c` as `a/c`, `\` as `/`.
fn normalize(path: &str) -> String {
    let mut parts: Vec<&str> = Vec::new();
    for p in path.split(['/', '\\']) {
        match p {
            "" | "." => {}
            ".." => {
                parts.pop();
            }
            p => parts.push(p),
        }
    }
    parts.join("/")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn paths_are_normalized() {
        assert_eq!(
            normalize("Interface/AddOns/A/../B/x.lua"),
            "Interface/AddOns/B/x.lua"
        );
        assert_eq!(
            normalize("Interface\\AddOns\\A\\.\\x.xml"),
            "Interface/AddOns/A/x.xml"
        );
    }
}
