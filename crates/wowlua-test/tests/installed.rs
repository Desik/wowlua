//! Blizzard's UI code read from this machine's WoW installation (decision
//! T553), against the upstream copy of the flavor's code: skipped where
//! either is missing.
#![allow(clippy::disallowed_methods)] // (this machine's installation and the cache)

use std::collections::BTreeSet;
use std::path::PathBuf;

fn install() -> Option<PathBuf> {
    let p = std::env::var_os("WOWLUA_WOW_PATH")
        .map(PathBuf::from)
        .unwrap_or_else(|| "/Applications/World of Warcraft".into());
    p.join(".build.info").exists().then_some(p)
}

#[test]
fn each_installed_client_s_ui_code_is_read() {
    let Some(root) = install() else {
        eprintln!("no WoW installation: skipped");
        return;
    };
    let table = wowlua_flavors::table();
    for flavor in ["camelot", "vanilla", "tbc", "mists"] {
        let Some(upstream) = wowlua_test::cached_ui_source(flavor) else {
            eprintln!("{flavor}: no upstream copy cached: skipped");
            continue;
        };
        let Some(client) = table.clients_of(flavor).next() else {
            continue;
        };
        let storage = wowlua_casc::FsStorage { root: root.clone() };
        let Some(product) = client
            .folders
            .iter()
            .find_map(|f| wowlua_casc::Casc::product_of_folder(&storage, f))
        else {
            eprintln!("{flavor}: not installed: skipped");
            continue;
        };
        let casc = wowlua_casc::Casc::open(Box::new(storage), &product).expect("the client");
        let seeds: Vec<String> = std::fs::read_dir(upstream.interface.join("AddOns"))
            .expect("the upstream addons")
            .flatten()
            .filter_map(|e| e.file_name().to_str().map(str::to_owned))
            .collect();
        let mut games = vec![
            table
                .flavor(flavor)
                .map_or("Mainline", |f| f.family.name())
                .to_owned(),
        ];
        for g in client.game_types.iter().chain(&client.suffixes) {
            let mut cap = g.clone();
            cap[..1].make_ascii_uppercase();
            games.extend([g.clone(), cap]);
        }
        let out = tempfile::tempdir().unwrap();
        let got =
            wowlua_test::installed::extract(&casc, &seeds, &client.suffixes, &games, out.path())
                .expect("extracted");
        let new: BTreeSet<&String> = got.addons.iter().filter(|a| !seeds.contains(a)).collect();
        // (the upstream copy's addons this client loads: a TOC of its)
        let loaded: Vec<&String> = seeds
            .iter()
            .filter(|n| {
                let dir = upstream.interface.join("AddOns").join(n);
                client
                    .suffixes
                    .iter()
                    .map(|s| dir.join(format!("{n}_{s}.toc")))
                    .chain([dir.join(format!("{n}.toc"))])
                    .any(|p| p.exists())
            })
            .collect();
        let missing: Vec<&&String> = loaded
            .iter()
            .filter(|n| !got.addons.contains(**n))
            .collect();
        eprintln!(
            "{flavor} ({product}): {} addons, {} files from the installation; upstream loads {} here; new: {new:?}; missing: {missing:?}",
            got.addons.len(),
            got.files,
            loaded.len()
        );
        assert!(
            missing.len() * 20 <= loaded.len(),
            "{flavor}: missing {missing:?}"
        );
        assert!(got.files > 500, "{flavor}: {} files", got.files);
    }
}
