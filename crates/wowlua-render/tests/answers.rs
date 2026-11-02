//! Answers from the client's tables (decision T464), against the answers
//! the client recorded (`wowlua-engine/data/camelot-answers.txt`), on the
//! WoW installation of this machine; skipped where there is none. Each
//! kind of call the tables answer has its test.

fn client() -> Option<wowlua_render::client::CascClientData> {
    let root = std::env::var_os("WOWLUA_WOW_PATH")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| "/Applications/World of Warcraft".into());
    let storage = wowlua_casc::FsStorage { root };
    let product = wowlua_casc::Casc::product_of_folder(&storage, "_forever_")?;
    let casc = std::sync::Arc::new(wowlua_casc::Casc::open(Box::new(storage), &product).ok()?);
    Some(wowlua_render::client::CascClientData::new(Box::new(
        move || Some(casc.clone()),
    )))
}

/// What 70707's server changed (its hotfixes) since the recordings
/// (70425): those spells' base cooldowns, the achievement's trait currency
/// (until they are recorded again: tests/ingame/PROBES.md, "The Camelot
/// update").
const NEWER: &[&str] = &[
    "GetSpellBaseCooldown(1240044)",
    "GetSpellBaseCooldown(1240045)",
    "GetSpellBaseCooldown(1240046)",
    "GetSpellBaseCooldown(1240047)",
    "GetSpellBaseCooldown(400640)",
    "C_Traits.GetTraitCurrencyForAchievement(4225, 64283)",
];

fn check(prefix: &str, at_least: usize) {
    let Some(data) = client() else {
        eprintln!("no camelot client: skipped");
        return;
    };
    let (checked, wrong) = wowlua_engine::recorded::compare_with_client(&data, prefix);
    let wrong: Vec<String> = wrong
        .into_iter()
        .filter(|w| !NEWER.iter().any(|n| w.starts_with(n)))
        .collect();
    eprintln!("{prefix}: {checked} answers, {} differ", wrong.len());
    assert!(checked >= at_least, "{checked}");
    assert!(
        wrong.is_empty(),
        "{} of {checked}:\n{}",
        wrong.len(),
        wrong
            .iter()
            .take(12)
            .cloned()
            .collect::<Vec<_>>()
            .join("\n")
    );
}

/// The world maps' static data.
#[test]
fn maps() {
    check("C_Map.", 700);
}

/// The achievements' own data and their categories.
#[test]
fn achievements() {
    for (prefix, at_least) in [
        ("GetAchievementInfo(", 311),
        ("GetAchievementCategory(", 311),
        ("GetCategoryInfo(", 55),
        ("GetCategoryList(", 2),
        ("GetStatisticsCategoryList(", 2),
        ("GetPreviousAchievement(", 311),
        ("GetNextAchievement(", 311),
        ("C_AchievementInfo.", 622),
        ("C_Traits.GetTraitCurrencyForAchievement(", 311),
    ] {
        check(prefix, at_least);
    }
}

/// The item classes' and subclasses' names.
#[test]
fn item_kinds() {
    check("C_Item.GetItemClassInfo(", 20);
    check("C_Item.GetItemSubClassInfo(", 109);
}

/// Spells' facts the API answers alone (the trainers' spells among them).
#[test]
fn spells() {
    check("C_Spell.IsSpellPassive(", 1300);
    check("GetSpellBaseCooldown(", 1300);
}
