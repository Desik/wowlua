//! The talent trees of the client's `Trait` tables (decision T435), against
//! the answers recorded in the client (`wowlua-engine/data/camelot-traits.txt`,
//! 11 characters): every node, entry, definition, cost, condition and tree
//! currency worked out again from the tables and the recorded ranks, on the
//! WoW installation of this machine; skipped where there is none.

/// What the builds changed since the recording (1.60.1.70425): the mage
/// tree's nodes moved, gained ranks or went; on 70707 more of them moved,
/// swapped their ranks and spells, or went; and 70707's server (its
/// hotfixes, in the client's cache once it reached the login screen)
/// reworked the paladin's, the druid's and the hunter's trees: new talents
/// (the paladin's Shieldsworn), others gone, moved or changed (until the
/// trees are recorded again: tests/ingame/PROBES.md, "The Camelot update").
const CHANGED: &[&str] = &[
    "GetTreeNodes(1112)",
    ", 105771)",
    ", 105772)",
    ", 105773)",
    ", 105778)",
    ", 130507)",
    // 70707
    "GetDefinitionInfo(135332)",
    "GetDefinitionInfo(135339)",
    ", 130513)",
    ", 130525)",
    ", 105784)",
    ", 105796)",
    ", 105797)",
    ", 105802)",
    ", 105813)",
    ", 105814)",
    // 70707's hotfixes
    "GetDefinitionInfo(134378)",
    "GetDefinitionInfo(134379)",
    "GetDefinitionInfo(134381)",
    "GetDefinitionInfo(134400)",
    "GetDefinitionInfo(134410)",
    "GetDefinitionInfo(134412)",
    "GetDefinitionInfo(134417)",
    "GetDefinitionInfo(134419)",
    "GetDefinitionInfo(134825)",
    "GetDefinitionInfo(135164)",
    "GetDefinitionInfo(135386)",
    "GetDefinitionInfo(142627)",
    "GetDefinitionInfo(145859)",
    "GetEntryInfo(14972990, 130531)",
    "GetEntryInfo(14974071, 130018)",
    "GetEntryInfo(14974071, 130028)",
    "GetEntryInfo(14974071, 130029)",
    "GetEntryInfo(14974071, 130363)",
    "GetEntryInfo(14974071, 130432)",
    "GetEntryInfo(14974071, 137869)",
    "GetEntryInfo(14974071, 137872)",
    "GetEntryInfo(14974071, 137877)",
    "GetEntryInfo(14974071, 137880)",
    "GetEntryInfo(22147, 141188)",
    "GetEntryInfo(2839888, 141188)",
    "GetNodeCost(14974071, 105629)",
    "GetNodeCost(14974071, 110880)",
    "GetNodeInfo(14974071, 105323)",
    "GetNodeInfo(14974071, 105324)",
    "GetNodeInfo(14974071, 105325)",
    "GetNodeInfo(14974071, 105327)",
    "GetNodeInfo(14974071, 105329)",
    "GetNodeInfo(14974071, 105330)",
    "GetNodeInfo(14974071, 105331)",
    "GetNodeInfo(14974071, 105332)",
    "GetNodeInfo(14974071, 105334)",
    "GetNodeInfo(14974071, 105335)",
    "GetNodeInfo(14974071, 105625)",
    "GetNodeInfo(14974071, 105628)",
    "GetNodeInfo(14974071, 105629)",
    "GetNodeInfo(14974071, 105630)",
    "GetNodeInfo(14974071, 105632)",
    "GetNodeInfo(14974071, 105633)",
    "GetNodeInfo(14974071, 105634)",
    "GetNodeInfo(14974071, 105636)",
    "GetNodeInfo(14974071, 105639)",
    "GetNodeInfo(14974071, 105692)",
    "GetNodeInfo(14974071, 105693)",
    "GetNodeInfo(14974071, 105694)",
    "GetNodeInfo(14974071, 105696)",
    "GetNodeInfo(14974071, 105697)",
    "GetNodeInfo(14974071, 105700)",
    "GetNodeInfo(14974071, 105701)",
    "GetNodeInfo(14974071, 105702)",
    "GetNodeInfo(14974071, 105703)",
    "GetNodeInfo(14974071, 105704)",
    "GetNodeInfo(14974071, 110871)",
    "GetNodeInfo(14974071, 110872)",
    "GetNodeInfo(14974071, 110873)",
    "GetNodeInfo(14974071, 110874)",
    "GetNodeInfo(14974071, 110875)",
    "GetNodeInfo(14974071, 110878)",
    "GetNodeInfo(14974071, 110879)",
    "GetNodeInfo(14974071, 110880)",
    "GetNodeInfo(14974071, 110882)",
    "GetNodeInfo(22147, 104945)",
    "GetNodeInfo(22147, 104951)",
    "GetNodeInfo(22147, 104952)",
    "GetNodeInfo(22147, 104953)",
    "GetNodeInfo(22147, 104954)",
    "GetNodeInfo(22147, 113563)",
    "GetNodeInfo(2839888, 104945)",
    "GetNodeInfo(2839888, 104951)",
    "GetNodeInfo(2839888, 104952)",
    "GetNodeInfo(2839888, 104953)",
    "GetNodeInfo(2839888, 104954)",
    "GetNodeInfo(2839888, 113563)",
    "GetNodeInfo(4226427, 104972)",
    "GetNodeInfo(4226427, 104973)",
    "GetTreeNodes(1100)",
];

#[test]
fn the_client_trees_answer_as_recorded() {
    let root = std::env::var_os("WOWLUA_WOW_PATH")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| "/Applications/World of Warcraft".into());
    let storage = wowlua_casc::FsStorage { root };
    let Some(product) = wowlua_casc::Casc::product_of_folder(&storage, "_forever_") else {
        eprintln!("no camelot client: skipped");
        return;
    };
    let casc = wowlua_casc::Casc::open(Box::new(storage), &product).unwrap();
    let tables = wowlua_render::client::trait_tables(&casc).expect("the trait tables");
    assert!(tables.trees.len() > 10, "{} trees", tables.trees.len());
    let (checked, wrong) =
        wowlua_engine::traits::compare_with_recording(std::sync::Arc::new(tables));
    let unexpected: Vec<&String> = wrong
        .iter()
        .filter(|w| !CHANGED.iter().any(|c| w.contains(c)))
        .collect();
    eprintln!("{checked} answers, {} differ", wrong.len());
    assert!(checked > 3000, "{checked}");
    assert!(
        unexpected.is_empty(),
        "{} of {checked}:\n{}",
        unexpected.len(),
        unexpected
            .iter()
            .take(40)
            .map(|s| s.as_str())
            .collect::<Vec<_>>()
            .join("\n")
    );
}
