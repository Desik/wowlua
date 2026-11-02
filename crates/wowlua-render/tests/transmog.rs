//! Transmog sources from the client's tables (decision T462), against the
//! sources the client answered (`wowlua-engine/data/camelot-answers.txt`,
//! `C_TransmogCollection.GetAppearanceSources`): item, appearance, slot,
//! category and source type, on the WoW installation of this machine;
//! skipped where there is none.

/// What the build changed since the recording (1.60.1.70425): on 70707
/// two sources show another appearance (until the sources are recorded
/// again: tests/ingame/PROBES.md, "The Camelot update").
const CHANGED: &[&str] = &["source 299176:", "source 304856:"];

#[test]
fn the_client_sources_answer_as_recorded() {
    let root = std::env::var_os("WOWLUA_WOW_PATH")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| "/Applications/World of Warcraft".into());
    let storage = wowlua_casc::FsStorage { root };
    let Some(product) = wowlua_casc::Casc::product_of_folder(&storage, "_forever_") else {
        eprintln!("no camelot client: skipped");
        return;
    };
    let casc = std::sync::Arc::new(wowlua_casc::Casc::open(Box::new(storage), &product).unwrap());
    let data = wowlua_render::client::CascClientData::new(Box::new(move || Some(casc.clone())));
    let (checked, wrong) = wowlua_engine::collections::compare_sources_with_recording(&data);
    eprintln!("{checked} sources, {} differ", wrong.len());
    let wrong: Vec<&String> = wrong
        .iter()
        .filter(|w| !CHANGED.iter().any(|c| w.starts_with(c)))
        .collect();
    assert!(checked > 5000, "{checked}");
    assert!(
        wrong.is_empty(),
        "{} of {checked}:\n{}",
        wrong.len(),
        wrong
            .iter()
            .take(20)
            .map(|s| s.as_str())
            .collect::<Vec<_>>()
            .join("\n")
    );
}
