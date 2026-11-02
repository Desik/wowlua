//! The talent trees of the client's `Trait` tables (decision T435), against
//! the answers recorded in the client (`wowlua-engine/data/camelot-traits.txt`,
//! 11 characters): every node, entry, definition, cost, condition and tree
//! currency worked out again from the tables and the recorded ranks, on the
//! WoW installation of this machine; skipped where there is none.

/// What the builds changed since the recording (1.60.1.70425): the mage
/// tree's nodes moved, gained ranks or went; on 70707 more of them moved,
/// swapped their ranks and spells, or went (until the trees are recorded
/// again: tests/ingame/PROBES.md, "The Camelot update").
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
