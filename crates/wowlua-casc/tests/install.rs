//! The reader on the WoW installation of this machine (`WOWLUA_WOW_PATH`,
//! else the default place); skipped where there is none.

use wowlua_casc::{Casc, FsStorage, Storage};

fn install() -> Option<std::path::PathBuf> {
    let p = std::env::var_os("WOWLUA_WOW_PATH")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| "/Applications/World of Warcraft".into());
    p.join(".build.info").exists().then_some(p)
}

#[test]
fn reads_every_products_textures_and_fonts() {
    let Some(root) = install() else {
        eprintln!("no WoW installation: skipped");
        return;
    };
    let storage = FsStorage { root: root.clone() };
    let camelot = Casc::product_of_folder(&storage, "_forever_");
    eprintln!("_forever_ is {camelot:?}");
    let products = Casc::products(&storage);
    assert!(!products.is_empty());
    for (product, version) in products {
        let started = std::time::Instant::now();
        let casc = match Casc::open(Box::new(FsStorage { root: root.clone() }), &product) {
            Ok(c) => c,
            Err(e) => panic!("{product} {version}: {e}"),
        };
        let opened = started.elapsed();
        let white = casc
            .file_by_path("Interface\\Buttons\\WHITE8X8")
            .unwrap_or_else(|| panic!("{product}: WHITE8X8"));
        let img = wowlua_casc::decode_image(&white).expect("a BLP");
        assert_eq!((img.width, img.height), (8, 8));
        // Near white: a DXT1 file stores 565 colors (era's green is 251).
        assert!(img.pixels.iter().all(|&c| c >= 248), "{product}: white");
        let font = casc
            .file_by_path("Fonts\\FRIZQT__.TTF")
            .unwrap_or_else(|| panic!("{product}: FRIZQT__.TTF"));
        assert_eq!(&font[..4], &[0, 1, 0, 0], "{product}: a TrueType font");
        let button = casc
            .file_by_path("Interface/Buttons/UI-Panel-Button-Up.blp")
            .and_then(|b| wowlua_casc::decode_image(&b));
        eprintln!(
            "{product} {version}: {} files, opened in {opened:?}, button {:?}",
            casc.file_count(),
            button.map(|b| (b.width, b.height))
        );
    }
    let _ = storage.list("Data");
}

#[test]
fn reads_the_builds_own_atlases() {
    let Some(root) = install() else {
        eprintln!("no WoW installation: skipped");
        return;
    };
    let storage = FsStorage { root: root.clone() };
    for (product, version) in Casc::products(&storage) {
        let casc = Casc::open(Box::new(FsStorage { root: root.clone() }), &product).expect("opens");
        let started = std::time::Instant::now();
        let atlases = wowlua_casc::atlas::read(&casc);
        eprintln!(
            "{product} {version}: {} atlases in {:?}",
            atlases.len(),
            started.elapsed()
        );
        // Some builds encrypt the tables (classic's): none then.
        if product == "wow_classic_beta" {
            assert!(atlases.len() > 1000, "{product}: {}", atlases.len());
            // An element's camelot variant at 1x: the level circle.
            let c = atlases["ui-hud-unitframe-smallcircle"];
            assert_eq!(c.file, 8036204, "{c:?}");
            assert_eq!((c.width, c.height), (39, 39));
            assert_eq!(
                c.coords,
                [979.0 / 1024.0, 1018.0 / 1024.0, 87.0 / 512.0, 126.0 / 512.0]
            );
            // A member's own name.
            assert_eq!(atlases["ui-hud-unitframe-smallcircle-c60-2x"].file, 8036209);
        }
    }
}

/// Every product's creature displays resolve to models that read: the
/// tables (encrypted ones with the client's keys), the model and its
/// skin, posed in its first sequence.
#[test]
fn reads_every_products_creature_models() {
    let Some(root) = install() else {
        eprintln!("no WoW installation: skipped");
        return;
    };
    let storage = FsStorage { root: root.clone() };
    for (product, version) in Casc::products(&storage) {
        let Ok(casc) = Casc::open(Box::new(FsStorage { root: root.clone() }), &product) else {
            continue;
        };
        let mut lib = wowlua_casc::display::Library::default();
        let rows = lib
            .table(&casc, "CreatureDisplayInfo")
            .map_or(0, wowlua_casc::tables::Table::len);
        assert!(rows > 1000, "{product}: {rows} displays");
        // The first displays that name a model file.
        let mut read = 0;
        for id in 1..3000 {
            let Some(d) = lib.creature_display(&casc, id) else {
                continue;
            };
            let part = &d.parts[0];
            let Some(bytes) = casc.file_by_id(part.file) else {
                continue;
            };
            let m = wowlua_casc::m2::Model::parse(&bytes)
                .unwrap_or_else(|| panic!("{product}: display {id}'s model {}", part.file));
            let skin = casc
                .file_by_id(m.skin_files[0])
                .and_then(|b| wowlua_casc::m2::Skin::parse(&b))
                .unwrap_or_else(|| panic!("{product}: model {}'s skin", part.file));
            assert!(!skin.batches.is_empty() && !m.vertices.is_empty());
            let pose = m.pose(m.sequence(0), 0);
            assert_eq!(pose.len(), m.bones.len());
            read += 1;
            if read == 20 {
                break;
            }
        }
        eprintln!(
            "{product} {version}: {rows} displays, {} keys, {read} models read",
            casc.key_count()
        );
        assert_eq!(read, 20, "{product}");
    }
}

/// Every product's characters resolve: a body with its customizations'
/// skin composited and textures, what it wears as parts; and a humanoid
/// creature with its baked skin.
#[test]
fn resolves_every_products_characters() {
    use wowlua_casc::character::{Look, Worn};
    use wowlua_casc::display::Texture;
    let Some(root) = install() else {
        eprintln!("no WoW installation: skipped");
        return;
    };
    let storage = FsStorage { root: root.clone() };
    for (product, _) in Casc::products(&storage) {
        let Ok(casc) = Casc::open(Box::new(FsStorage { root: root.clone() }), &product) else {
            continue;
        };
        let mut lib = wowlua_casc::display::Library::default();
        // A human warrior in a recruit's shirt with a sword (the starting
        // items of every classic client).
        let look = Look {
            race: 1,
            sex: 0,
            class: 1,
            choices: Vec::new(),
            worn: vec![(4, Worn::Item(38)), (16, Worn::Item(25))],
            bake: None,
        };
        let d = lib
            .character(&casc, &look)
            .unwrap_or_else(|| panic!("{product}: the human"));
        let body = &d.parts[0];
        let Some(Texture::Composite(skin)) = body.textures.get(&1) else {
            panic!("{product}: no composited skin: {:?}", body.textures.keys());
        };
        assert!(skin.layers.len() > 2, "{product}: {skin:?}");
        assert!(
            d.parts.iter().any(|p| p.attachment == Some(1)),
            "{product}: the sword"
        );
        assert!(casc.file_by_id(body.file).is_some());
        // A humanoid creature: its baked skin.
        let humanoid = lib
            .table(&casc, "CreatureDisplayInfo")
            .and_then(|t| {
                t.rows()
                    .find(|r| r.int("ExtendedDisplayInfoID") != 0)
                    .map(|r| r.id())
            })
            .expect("a humanoid");
        let d = lib
            .creature_display(&casc, humanoid)
            .unwrap_or_else(|| panic!("{product}: display {humanoid}"));
        assert!(
            matches!(d.parts[0].textures.get(&1), Some(Texture::File(_))),
            "{product}: display {humanoid}'s bake"
        );
    }
}

/// Every product's maps have WDTs (by file ID or path), which list each
/// terrain tile's minimap texture: Eastern Kingdoms has Northshire's
/// (column 32, row 48), and the texture reads.
#[test]
fn reads_the_minimap_tiles() {
    let Some(root) = install() else {
        eprintln!("no WoW installation: skipped");
        return;
    };
    let storage = FsStorage { root: root.clone() };
    for (product, version) in Casc::products(&storage) {
        let casc = Casc::open(Box::new(FsStorage { root: root.clone() }), &product).expect("opens");
        let mut lib = wowlua_casc::display::Library::default();
        let tile = lib.minimap_tile(&casc, 0, 32, 48);
        eprintln!("{product} {version}: Northshire's minimap tile {tile:?}");
        {
            let f = tile.expect("the tile");
            let img = wowlua_casc::decode_image(&casc.file_by_id(f).expect("the file"))
                .expect("an image");
            // 512 pixels on camelot, 256 on the older builds.
            assert!(img.width == img.height && img.width >= 256, "{product}");
            assert_eq!(lib.minimap_tile(&casc, 0, 0, 0), None);
            assert_eq!(lib.minimap_tile(&casc, 0, 64, 48), None);
        }
    }
}

/// Every product's terrain (decision T522): the ground where a new human
/// starts, at Northshire Abbey (-8949.95, -132.49), is where the game puts
/// their feet (83.53), and its tile has its textures and their blends.
#[test]
fn reads_the_terrain() {
    let Some(root) = install() else {
        eprintln!("no WoW installation: skipped");
        return;
    };
    let storage = FsStorage { root: root.clone() };
    for (product, version) in Casc::products(&storage) {
        let casc = Casc::open(Box::new(FsStorage { root: root.clone() }), &product).expect("opens");
        let mut lib = wowlua_casc::display::Library::default();
        let started = std::time::Instant::now();
        let ground = lib.ground(&casc, 0, -8949.95, -132.493);
        let tile = lib.terrain_tile(&casc, 0, 32, 48).expect("the tile");
        let layers = tile
            .chunks
            .iter()
            .map(|c| c.layers.len())
            .max()
            .unwrap_or(0);
        let blends = tile
            .chunks
            .iter()
            .flat_map(|c| &c.layers)
            .filter(|l| l.alpha.is_some())
            .count();
        eprintln!(
            "{product} {version}: ground {ground:?} in {:?}, {} textures, up to {layers} layers, {blends} blends",
            started.elapsed(),
            tile.textures.len(),
        );
        let ground = ground.expect("the ground");
        assert!((ground - 83.53).abs() < 1.5, "{product}: {ground}");
        assert!(
            !tile.textures.is_empty() && layers >= 2 && blends > 0,
            "{product}"
        );
    }
}

/// Every product's `ItemSparse` (a sparse table: records of their own
/// sizes, strings inline) reads: the Hearthstone's name and quality, and
/// the `Item` table's class for it.
#[test]
fn reads_the_sparse_item_table() {
    let Some(root) = install() else {
        eprintln!("no WoW installation: skipped");
        return;
    };
    let storage = FsStorage { root: root.clone() };
    for (product, version) in Casc::products(&storage) {
        let casc = Casc::open(Box::new(FsStorage { root: root.clone() }), &product).expect("opens");
        let sparse = wowlua_casc::tables::Table::open(&casc, "ItemSparse").expect("ItemSparse");
        let r = sparse.row(6948).expect("the Hearthstone");
        eprintln!("{product} {version}: {} items", sparse.len());
        assert_eq!(
            r.string("Display_lang").as_deref(),
            Some("Hearthstone"),
            "{product}"
        );
        assert_eq!(r.int("OverallQualityID"), 1, "{product}");
        let item = wowlua_casc::tables::Table::open(&casc, "Item").expect("Item");
        assert_eq!(
            item.row(6948).expect("its row").int("ClassID"),
            15,
            "{product}"
        );
        if product == "wow_classic_beta" {
            let cub = sparse.row(277714).expect("the cub");
            assert_eq!(
                cub.string("Display_lang").as_deref(),
                Some("Spectral Bear Cub")
            );
        }
    }
}

/// Ribbon emitters read: camelot's fire totem (display 4589) has three,
/// laying edges down 23 times a second for 0.53 seconds, each 0.22 yards
/// above and below its bone's point, additive.
#[test]
fn reads_ribbon_emitters() {
    let Some(root) = install() else {
        eprintln!("no WoW installation: skipped");
        return;
    };
    let Ok(casc) = Casc::open(Box::new(FsStorage { root }), "wow_classic_beta") else {
        eprintln!("no camelot client: skipped");
        return;
    };
    let mut lib = wowlua_casc::display::Library::default();
    let d = lib.creature_display(&casc, 4589).expect("the totem");
    let m = wowlua_casc::m2::Model::parse(&casc.file_by_id(d.parts[0].file).expect("its file"))
        .expect("its model");
    assert_eq!(m.ribbons.len(), 3);
    let r = &m.ribbons[0];
    assert_eq!((r.edges_per_second, r.bone), (23.0, 27));
    assert!((r.edge_lifetime - 0.53).abs() < 1e-4);
    let look = m.ribbon_look(0, Some(0), 0).expect("its look");
    assert!((look.above - 0.2222).abs() < 1e-3 && (look.below - 0.2222).abs() < 1e-3);
    let blend = r.materials.first().map(|i| m.materials[*i as usize].blend);
    assert_eq!(blend, Some(4));
}

/// WoW Forever's wardrobe cameras: an Undead male's head is UI camera 542
/// (as the client answered `GetAppearanceCameraID` for him), a Skyborne
/// female's 1957; its weapons' table reads.
#[test]
fn reads_the_wardrobe_camera_tables() {
    let Some(root) = install() else {
        eprintln!("no WoW installation: skipped");
        return;
    };
    let storage = FsStorage { root: root.clone() };
    if !Casc::products(&storage)
        .iter()
        .any(|(p, _)| p == "wow_classic_beta")
    {
        return;
    }
    let casc = Casc::open(Box::new(FsStorage { root }), "wow_classic_beta").expect("opens");
    let t = wowlua_casc::tables::Table::open(&casc, "UiCamFbackTransmogChrRace").expect("races");
    let cam = |race: u32, gender: u32| {
        t.rows()
            .find(|r| {
                r.int("ChrRaceID") == race
                    && r.int("Gender") == gender
                    && r.int("InventoryType") == 1
                    && r.int("Variation") == 0
            })
            .map(|r| r.int("UiCameraID"))
    };
    assert_eq!((cam(5, 0), cam(96, 1)), (Some(542), Some(1957)));
    let w = wowlua_casc::tables::Table::open(&casc, "UiCamFbackTransmogWeapon").expect("weapons");
    assert!(
        w.rows()
            .any(|r| r.int("ItemClass") == 2 && r.int("UiCameraID") > 0)
    );
}

/// WoW Forever's areas: 2437 is Ragefire Chasm (AtlasLoot names its
/// dungeons by `C_Map.GetAreaInfo`), 1637 Orgrimmar (as the client
/// answered).
#[test]
fn reads_the_area_table() {
    let Some(root) = install() else {
        eprintln!("no WoW installation: skipped");
        return;
    };
    let storage = FsStorage { root: root.clone() };
    if !Casc::products(&storage)
        .iter()
        .any(|(p, _)| p == "wow_classic_beta")
    {
        return;
    }
    let casc = Casc::open(Box::new(FsStorage { root }), "wow_classic_beta").expect("opens");
    let t = wowlua_casc::tables::Table::open(&casc, "AreaTable").expect("areas");
    let name = |id: u32| t.row(id).and_then(|r| r.string("AreaName_lang"));
    assert_eq!(name(2437).as_deref(), Some("Ragefire Chasm"));
    assert_eq!(name(1637).as_deref(), Some("Orgrimmar"));
}

/// camelot's spells as the client's tables and hotfixes have them
/// (decision T391): the descriptions filled in for the character a
/// recording was taken with (`wowlua-engine/tests/client`, its level, no
/// gear) are the client's, except where the character's power counts
/// (the recordings lack it); a description that cannot be filled exactly
/// is left unknown, never wrong. The facts of a spell: its name, rank,
/// cast time, range and icon.
#[test]
fn spells_read_as_the_client_shows_them() {
    use wowlua_casc::spells::SpellTables;
    use wowlua_casc::spelltext::Caster;
    let Some(root) = install() else {
        eprintln!("no WoW installation: skipped");
        return;
    };
    let storage = FsStorage { root: root.clone() };
    let Some(product) = Casc::product_of_folder(&storage, "_forever_") else {
        eprintln!("no camelot client: skipped");
        return;
    };
    let casc = Casc::open(Box::new(FsStorage { root }), &product).unwrap();
    let tables = SpellTables::open(&casc).expect("the spell tables");
    let fireball = tables.info(133).expect("Fireball");
    assert_eq!(fireball.name, "Fireball");
    assert_eq!(fireball.subtext, "Rank 1");
    assert!(
        fireball.cast_ms > 0 && fireball.max_range > 0.0,
        "{fireball:?}"
    );
    assert!(
        fireball.icon.is_some() && !fireball.costs.is_empty(),
        "{fireball:?}"
    );
    let dir =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../wowlua-engine/tests/client");
    let (mut same, mut by_power, mut unknown) = (0, 0, 0);
    let mut differ = Vec::new();
    for name in [
        "forever-camelot-trainer-druid-3033.lua",
        "forever-camelot-trainer-druid-3060.lua",
        "forever-camelot-trainer-druid-3064.lua",
        "forever-camelot-trainer-warrior-911.lua",
        "forever-camelot-trainer-warrior-913.lua",
    ] {
        let text = std::fs::read_to_string(dir.join(name)).unwrap();
        let level: u32 = text
            .lines()
            .find_map(|l| {
                l.trim()
                    .strip_prefix("[\"UnitLevel(\\\"player\\\")\"] = \"1: number ")
            })
            .and_then(|r| r.trim_end_matches("\",").parse().ok())
            .expect("the recording's level");
        // the level-21 druid's damage is 2 % more (something of the
        // character's the recording does not show)
        let damage_done = if level == 21 { 1.02 } else { 1.0 };
        let caster = Caster {
            level,
            damage_done,
            ..Default::default()
        };
        let more = Caster {
            spell_power: 1000.0,
            healing_power: 1000.0,
            attack_power: 1000.0,
            ..caster
        };
        for line in text.lines() {
            let Some(rest) = line.trim().strip_prefix("[\"C_Spell.GetSpellDescription(") else {
                continue;
            };
            let Some((id, answer)) = rest.split_once(")\"] = \"1: string ") else {
                continue;
            };
            let id: u32 = id.parse().unwrap();
            let recorded = answer
                .trim_end_matches("\",")
                .replace("\\\"", "\"")
                .replace("\\r", "\r")
                .replace("\\n", "\n");
            match tables.describe(id, &caster) {
                Some(d) if d == recorded => same += 1,
                Some(_) if tables.describe(id, &more) != tables.describe(id, &caster) => {
                    by_power += 1
                }
                Some(d) => differ.push(format!("{id} at {level}: {d}\n  client: {recorded}")),
                None => unknown += 1,
            }
        }
    }
    eprintln!("{same} the same, {by_power} where power counts, {unknown} unknown");
    assert!(differ.is_empty(), "{}", differ.join("\n"));
    assert!(same >= 200, "{same}");
}

/// camelot's spells' power costs as the client's `GetSpellPowerCost`
/// answers them in the trainers' recordings: each cost's power type and
/// amount (rage and runic power in whole points, the tables' tenths;
/// a finisher's the most it takes); costs in percent of the base power,
/// and costs a recorded character's talents lowered, depend on the
/// character.
///
/// `NEWER_COSTS`: what 70707's server changed (its hotfixes) since the
/// recordings (70425): the paladin's blessings cost nothing, Holy Shield
/// less (until the trainers are recorded again: tests/ingame/PROBES.md,
/// "The Camelot update").
const NEWER_COSTS: &[u32] = &[1038, 19977, 19978, 19979, 20927, 20928, 25890];

#[test]
fn spell_costs_read_as_the_client_answers_them() {
    use wowlua_casc::spells::SpellTables;
    let Some(root) = install() else {
        eprintln!("no WoW installation: skipped");
        return;
    };
    let storage = FsStorage { root: root.clone() };
    let Some(product) = Casc::product_of_folder(&storage, "_forever_") else {
        eprintln!("no camelot client: skipped");
        return;
    };
    let casc = Casc::open(Box::new(FsStorage { root }), &product).unwrap();
    let tables = SpellTables::open(&casc).expect("the spell tables");
    let dir =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../wowlua-engine/tests/client");
    let (mut same, mut by_character, mut differ) = (0, 0, Vec::new());
    for entry in std::fs::read_dir(&dir).unwrap().flatten() {
        let name = entry.file_name().to_string_lossy().into_owned();
        if !name.starts_with("forever-camelot-trainer") {
            continue;
        }
        let text = std::fs::read_to_string(entry.path()).unwrap();
        for line in text.lines() {
            let Some(rest) = line.trim().strip_prefix("[\"C_Spell.GetSpellPowerCost(") else {
                continue;
            };
            let Some((id, answer)) = rest.split_once(")\"] = \"") else {
                continue;
            };
            let id: u32 = id.parse().unwrap();
            // (type, cost) of each recorded cost
            let recorded: Vec<(i32, u32)> = answer
                .split("},")
                .filter_map(|c| {
                    let num = |key: &str| -> Option<i64> {
                        let at = c.find(&format!("{key}=number "))? + key.len() + 8;
                        c[at..]
                            .split(|ch: char| !ch.is_ascii_digit() && ch != '-')
                            .next()?
                            .parse()
                            .ok()
                    };
                    Some((num("type")? as i32, num("cost")? as u32))
                })
                .filter(|_| {
                    !answer.contains("costPercent=number 1")
                        && !answer.contains("costPercent=number 2")
                })
                .collect();
            if recorded.is_empty() {
                continue;
            }
            let info = tables.info(id);
            // (a share of the base power: the character's)
            if info
                .as_ref()
                .is_some_and(|s| s.costs.iter().any(|c| c.percent > 0.0))
            {
                continue;
            }
            let ours: Vec<(i32, u32)> = info
                .map(|s| s.costs.iter().map(|c| (c.power_type, c.shown())).collect())
                .unwrap_or_default();
            // (less of the same power: the character's talents, which the
            // tables do not show: Ferocity, Improved Thunder Clap)
            let reduced = ours.len() == recorded.len()
                && ours
                    .iter()
                    .zip(&recorded)
                    .all(|(o, r)| o.0 == r.0 && r.1 <= o.1);
            if ours == recorded {
                same += 1;
            } else if reduced {
                by_character += 1;
            } else if NEWER_COSTS.contains(&id) {
                continue;
            } else {
                differ.push(format!("{id}: ours {ours:?}, client {recorded:?}"));
            }
        }
    }
    eprintln!(
        "{same} the same, {by_character} less by the character's talents, {} different",
        differ.len()
    );
    assert!(differ.is_empty(), "{}", differ.join("\n"));
    assert!(
        same >= 1000 && by_character < same / 20,
        "{same}, {by_character}"
    );
}

/// Pulled game data (`wowlua data pull`, decision T203) reads as the
/// installation does: the same tables with their hotfixes, the named
/// files it was given, the client's caches.
#[test]
fn pulled_game_data_reads_as_the_installation() {
    use std::collections::HashMap;
    use std::sync::Mutex;
    let Some(root) = install() else {
        eprintln!("no WoW installation: skipped");
        return;
    };
    #[derive(Default)]
    struct Memory(Mutex<HashMap<String, Vec<u8>>>);
    impl Storage for Memory {
        fn read(&self, path: &str) -> Option<Vec<u8>> {
            self.0.lock().unwrap().get(path).cloned()
        }
        fn read_at(&self, path: &str, offset: u64, len: usize) -> Option<Vec<u8>> {
            let all = self.read(path)?;
            all.get(offset as usize..offset as usize + len)
                .map(<[u8]>::to_vec)
        }
        fn list(&self, dir: &str) -> Vec<String> {
            let prefix = if dir.is_empty() {
                String::new()
            } else {
                format!("{dir}/")
            };
            let mut out: Vec<String> = self
                .0
                .lock()
                .unwrap()
                .keys()
                .filter_map(|k| {
                    k.strip_prefix(&prefix)?
                        .split('/')
                        .next()
                        .map(str::to_string)
                })
                .collect();
            out.sort();
            out.dedup();
            out
        }
    }
    let storage = FsStorage { root: root.clone() };
    let product = Casc::product_of_folder(&storage, "_forever_").unwrap_or("wow".into());
    let live = Casc::open(Box::new(FsStorage { root }), &product).expect("opens");
    let memory = Memory::default();
    let files = live.pull(&["Fonts\\FRIZQT__.TTF"]);
    let bytes: usize = files.iter().map(|(_, b)| b.len()).sum();
    eprintln!("{product}: {} files, {} MiB", files.len(), bytes >> 20);
    memory.0.lock().unwrap().extend(files);
    let pulled = Casc::open_pulled(Box::new(memory)).expect("opens the pulled data");
    assert_eq!(
        (&pulled.product, &pulled.version),
        (&live.product, &live.version)
    );
    let open = |c: &Casc| wowlua_casc::tables::Table::open(c, "ItemSparse").expect("ItemSparse");
    let (a, b) = (open(&live), open(&pulled));
    assert_eq!(a.len(), b.len());
    let name = |t: &wowlua_casc::tables::Table| t.row(6948).and_then(|r| r.string("Display_lang"));
    assert_eq!(name(&a), name(&b));
    assert_eq!(name(&b).as_deref(), Some("Hearthstone"));
    // a record only the hotfixes have on camelot
    let cub = |t: &wowlua_casc::tables::Table| t.row(277714).and_then(|r| r.string("Display_lang"));
    assert_eq!(cub(&a), cub(&b));
    assert_eq!(
        pulled.file_by_path("Fonts\\FRIZQT__.TTF"),
        live.file_by_path("Fonts\\FRIZQT__.TTF")
    );
    // (one copy per locale; the installation may have a folder more)
    assert_eq!(
        pulled.cache_files("creaturecache.wdb").is_empty(),
        live.cache_files("creaturecache.wdb").is_empty()
    );
    let spells = |c: &Casc| wowlua_casc::spells::SpellTables::open(c).is_some();
    assert_eq!(spells(&live), spells(&pulled));
}

/// Every product's buildings at Northshire (decision T523): the tile's
/// objects file places them, each one's root and groups read, and turned
/// and moved by its placement its bounds are where the placement says.
#[test]
fn reads_the_buildings() {
    let Some(root) = install() else {
        eprintln!("no WoW installation: skipped");
        return;
    };
    let storage = FsStorage { root: root.clone() };
    for (product, version) in Casc::products(&storage) {
        let casc = Casc::open(Box::new(FsStorage { root: root.clone() }), &product).expect("opens");
        let mut lib = wowlua_casc::display::Library::default();
        let objects = lib.objects(&casc, 0, 32, 48).expect("the tile's objects");
        let (mut checked, mut groups, mut triangles) = (0, 0, 0);
        for b in &objects.buildings {
            let Some(wmo) = lib.building(&casc, b.file) else {
                continue;
            };
            let place = wowlua_casc::obj::placed(b.at, b.rotation, b.scale);
            let (lo, hi) = (wmo.root.bounds[0], wmo.root.bounds[1]);
            let mut min = [f64::MAX; 3];
            let mut max = [f64::MIN; 3];
            for i in 0..8 {
                let c = [
                    if i & 1 == 0 { lo[0] } else { hi[0] },
                    if i & 2 == 0 { lo[1] } else { hi[1] },
                    if i & 4 == 0 { lo[2] } else { hi[2] },
                ];
                let w = place(c);
                for k in 0..3 {
                    min[k] = min[k].min(w[k]);
                    max[k] = max[k].max(w[k]);
                }
            }
            let off = (0..3)
                .map(|k| {
                    (min[k] - b.bounds[0][k])
                        .abs()
                        .max((max[k] - b.bounds[1][k]).abs())
                })
                .fold(0.0, f64::max);
            eprintln!("{product}: building {} off by {off:.2}", b.file);
            assert!(off < 2.0, "{product}: building {} off by {off}", b.file);
            checked += 1;
            groups += wmo.groups.len();
            triangles += wmo
                .groups
                .iter()
                .map(|g| g.indices.len() / 3)
                .sum::<usize>();
        }
        eprintln!(
            "{product} {version}: {} buildings ({checked} checked, {groups} groups, {triangles} triangles), {} doodads",
            objects.buildings.len(),
            objects.doodads.len()
        );
        assert!(
            checked > 0 && triangles > 0 && !objects.doodads.is_empty(),
            "{product}"
        );
    }
}

/// The buildings are solid (decision T524), on camelot's client: walking
/// toward Northshire Abbey's middle from each side meets a wall on most
/// (its doors may be open), there is a floor in it, and a line through it
/// is not clear.
#[test]
fn the_buildings_are_solid() {
    let Some(root) = install() else {
        eprintln!("no WoW installation: skipped");
        return;
    };
    let storage = FsStorage { root: root.clone() };
    let Some(product) = Casc::product_of_folder(&storage, "_forever_") else {
        eprintln!("no camelot client: skipped");
        return;
    };
    let casc = Casc::open(Box::new(storage), &product).expect("opens");
    let mut lib = wowlua_casc::display::Library::default();
    let objects = lib.objects(&casc, 0, 32, 48).expect("the tile's objects");
    // (the building nearest where a new human starts, of a building's
    // size: the tile also places the valley's cliffs as one)
    let from_start = |x: &wowlua_casc::obj::Building| (x.at[0] + 8949.95).hypot(x.at[1] + 132.493);
    let abbey = objects
        .buildings
        .iter()
        .filter(|b| b.bounds[1][0] - b.bounds[0][0] < 300.0)
        .min_by(|a, b| from_start(a).total_cmp(&from_start(b)))
        .expect("a building");
    let mid = [
        (abbey.bounds[0][0] + abbey.bounds[1][0]) / 2.0,
        (abbey.bounds[0][1] + abbey.bounds[1][1]) / 2.0,
    ];
    let inside_ground = lib.ground(&casc, 0, mid[0], mid[1]).expect("the ground");
    let started = std::time::Instant::now();
    let floor = lib.floor(&casc, 0, [mid[0], mid[1], inside_ground]);
    eprintln!(
        "abbey {}: ground {inside_ground:.2}, floor {floor:?} in {:?}",
        abbey.file,
        started.elapsed()
    );
    assert!(floor.is_some_and(|f| f >= inside_ground - 0.5), "{floor:?}");
    let reach =
        (abbey.bounds[1][0] - abbey.bounds[0][0]).max(abbey.bounds[1][1] - abbey.bounds[0][1]);
    let mut blocked = 0;
    for (dn, dw) in [(1.0, 0.0), (-1.0, 0.0), (0.0, 1.0), (0.0, -1.0)] {
        let (n, w) = (mid[0] + dn * reach, mid[1] + dw * reach);
        let z = lib.ground(&casc, 0, n, w).expect("the ground");
        let started = std::time::Instant::now();
        let f = lib.walk(&casc, 0, [n, w, z], [mid[0], mid[1], z], 0.3);
        eprintln!(
            "from ({dn}, {dw}): walks {f:.2} of the way, in {:?}",
            started.elapsed()
        );
        if f < 1.0 {
            blocked += 1;
        }
    }
    assert!(blocked >= 2, "{blocked}");
    let high = inside_ground + 5.0;
    let through = lib.hit(
        &casc,
        0,
        [mid[0] + reach, mid[1], high],
        [mid[0] - reach, mid[1], high],
        false,
    );
    assert!(through.is_some(), "a line through the abbey");
}

/// The creatures each client knows (decision T531): its `Creature`
/// table's and its creature cache's, by name; the classic client's cache
/// knows the Black Hawkstrider it met.
#[test]
fn knows_the_creatures_met() {
    let Some(root) = install() else {
        eprintln!("no WoW installation: skipped");
        return;
    };
    let storage = FsStorage { root: root.clone() };
    for (product, version) in Casc::products(&storage) {
        let casc = Casc::open(Box::new(FsStorage { root: root.clone() }), &product).expect("opens");
        let mut lib = wowlua_casc::display::Library::default();
        let all = lib.creatures(&casc);
        let named = all.iter().filter(|c| !c.name.is_empty()).count();
        eprintln!(
            "{product} {version}: {} creatures known, {named} named, first {:?}",
            all.len(),
            all.first().map(|c| &c.name)
        );
        if product == "wow_classic" {
            assert!(
                all.iter()
                    .any(|c| c.name == "Black Hawkstrider" && c.display.is_some()),
                "{product}"
            );
        }
    }
}

/// One standing on a building stays there (decision T524), on camelot's
/// client: every colliding floor of Northshire Abbey, each 3 yards, is
/// the floor under one standing on it, though a wall or a steep roof is
/// over it within a step.
#[test]
fn one_stands_on_every_floor() {
    let Some(root) = install() else {
        eprintln!("no WoW installation: skipped");
        return;
    };
    let storage = FsStorage { root: root.clone() };
    let Some(product) = Casc::product_of_folder(&storage, "_forever_") else {
        eprintln!("no camelot client: skipped");
        return;
    };
    let casc = Casc::open(Box::new(storage), &product).expect("opens");
    let mut lib = wowlua_casc::display::Library::default();
    let objects = lib.objects(&casc, 0, 32, 48).expect("the tile's objects");
    let from_start = |x: &wowlua_casc::obj::Building| (x.at[0] + 8949.95).hypot(x.at[1] + 132.493);
    let abbey = objects
        .buildings
        .iter()
        .filter(|b| b.bounds[1][0] - b.bounds[0][0] < 300.0)
        .min_by(|a, b| from_start(a).total_cmp(&from_start(b)))
        .expect("a building")
        .clone();
    let wmo = lib.building(&casc, abbey.file).expect("the abbey");
    let place = wowlua_casc::obj::placed(abbey.at, abbey.rotation, abbey.scale);
    // (its colliding triangles facing up, on the map)
    let mut floors = Vec::new();
    for g in &wmo.groups {
        for (t, tri) in g.indices.as_chunks::<3>().0.iter().enumerate() {
            let v = tri.map(|i| place(g.positions[i as usize]));
            let (e1, e2) = (
                [v[1][0] - v[0][0], v[1][1] - v[0][1], v[1][2] - v[0][2]],
                [v[2][0] - v[0][0], v[2][1] - v[0][1], v[2][2] - v[0][2]],
            );
            let n = [
                e1[1] * e2[2] - e1[2] * e2[1],
                e1[2] * e2[0] - e1[0] * e2[2],
                e1[0] * e2[1] - e1[1] * e2[0],
            ];
            let l = (n[0] * n[0] + n[1] * n[1] + n[2] * n[2]).sqrt();
            if g.collides(t) && l > 1e-9 && (n[2] / l).abs() >= wowlua_casc::collide::WALKABLE {
                floors.push(v);
            }
        }
    }
    // (the height where a vertical line crosses a triangle)
    let height = |v: &[[f64; 3]; 3], n: f64, w: f64| {
        let d =
            (v[1][0] - v[0][0]) * (v[2][1] - v[0][1]) - (v[2][0] - v[0][0]) * (v[1][1] - v[0][1]);
        let s = ((n - v[0][0]) * (v[2][1] - v[0][1]) - (v[2][0] - v[0][0]) * (w - v[0][1])) / d;
        let t = ((v[1][0] - v[0][0]) * (w - v[0][1]) - (n - v[0][0]) * (v[1][1] - v[0][1])) / d;
        (d.abs() > 1e-9 && s >= 0.0 && t >= 0.0 && s + t <= 1.0)
            .then(|| v[0][2] + s * (v[1][2] - v[0][2]) + t * (v[2][2] - v[0][2]))
    };
    let (mut stood, mut fell) = (0, Vec::new());
    let mut n = abbey.bounds[0][0];
    while n <= abbey.bounds[1][0] {
        let mut w = abbey.bounds[0][1];
        while w <= abbey.bounds[1][1] {
            for z in floors.iter().filter_map(|v| height(v, n, w)) {
                stood += 1;
                let floor = lib.floor(&casc, 0, [n, w, z]);
                if floor.is_none_or(|f| f < z - 0.3) {
                    fell.push((n, w, z, floor));
                }
            }
            w += 3.0;
        }
        n += 3.0;
    }
    eprintln!("abbey {}: {stood} places, {} fell", abbey.file, fell.len());
    assert!(stood > 500, "{stood}");
    assert!(fell.is_empty(), "{:?}", &fell[..fell.len().min(5)]);
}

/// Camelot's client and its Northshire Abbey (the building nearest where
/// a new human starts, of a building's size); none without them.
fn northshire() -> Option<(
    Casc,
    wowlua_casc::display::Library,
    wowlua_casc::obj::Building,
)> {
    let root = install()?;
    let storage = FsStorage { root };
    let product = Casc::product_of_folder(&storage, "_forever_")?;
    let casc = Casc::open(Box::new(storage), &product).expect("opens");
    let mut lib = wowlua_casc::display::Library::default();
    let objects = lib.objects(&casc, 0, 32, 48).expect("the tile's objects");
    let from_start = |x: &wowlua_casc::obj::Building| (x.at[0] + 8949.95).hypot(x.at[1] + 132.493);
    let abbey = objects
        .buildings
        .iter()
        .filter(|b| b.bounds[1][0] - b.bounds[0][0] < 300.0)
        .min_by(|a, b| from_start(a).total_cmp(&from_start(b)))?
        .clone();
    Some((casc, lib, abbey))
}

/// One walking in Northshire Abbey as the engine moves one (decision
/// T532: a step at a time, sliding along a wall) does not pass through a
/// wall from the knee to the head: from places all over it on the ground,
/// each way, the abbey's walls (a beam, a sill) stop one.
#[test]
fn one_walks_through_no_wall() {
    let Some((casc, mut lib, abbey)) = northshire() else {
        eprintln!("no camelot client: skipped");
        return;
    };
    let wmo = lib.building(&casc, abbey.file).expect("the abbey");
    let walls = wowlua_casc::collide::Placed::new(wmo, &abbey);
    let steep = |_: f64, n: [f64; 3]| n[2].abs() < wowlua_casc::collide::WALKABLE;
    let (mut runs, mut through) = (0, Vec::new());
    let mut n0 = abbey.bounds[0][0] + 2.0;
    while n0 < abbey.bounds[1][0] {
        let mut w0 = abbey.bounds[0][1] + 2.0;
        while w0 < abbey.bounds[1][1] {
            let Some(ground) = lib.ground(&casc, 0, n0, w0) else {
                w0 += 12.0;
                continue;
            };
            let z0 = lib.floor(&casc, 0, [n0, w0, ground]).unwrap_or(ground);
            for k in 0..8 {
                let a = f64::from(k) * std::f64::consts::TAU / 8.0;
                let (dn, dw) = (a.cos() * 0.12, a.sin() * 0.12);
                let (mut y, mut x, mut z) = (n0, w0, z0);
                runs += 1;
                for _ in 0..60 {
                    let (ty, tx) = (y + dn, x + dw);
                    let (ny, nx) = if lib.walk(&casc, 0, [y, x, z], [ty, tx, z], 0.306) >= 1.0 {
                        (ty, tx)
                    } else {
                        let fy = lib.walk(&casc, 0, [y, x, z], [ty, x, z], 0.306);
                        let ny = y + (ty - y) * fy;
                        let fx = lib.walk(&casc, 0, [ny, x, z], [ny, tx, z], 0.306);
                        (ny, x + (tx - x) * fx)
                    };
                    let nz = lib.floor(&casc, 0, [ny, nx, z]).unwrap_or(z);
                    // (a step down from a ledge passes its side: not a wall)
                    if nz > z - 0.3
                        && [1.0, 1.4].iter().any(|up| {
                            walls
                                .first([y, x, z + up], [ny, nx, nz + up], steep)
                                .is_some()
                        })
                    {
                        through.push((y, x, z));
                        break;
                    }
                    (y, x, z) = (ny, nx, nz);
                }
            }
            w0 += 12.0;
        }
        n0 += 12.0;
    }
    eprintln!(
        "abbey {}: {runs} walks, {} through a wall",
        abbey.file,
        through.len()
    );
    assert!(runs > 100, "{runs}");
    assert!(through.is_empty(), "{:?}", &through[..through.len().min(5)]);
}

/// The props are solid (decision T532), on camelot's client: Northshire's
/// props with a collision mesh (fences, posts, crates, trees) stop one
/// walking at them; a body's width keeps one that far from a wall; a
/// human's body is the width the game's servers know (0.306 yards).
#[test]
fn the_props_are_solid() {
    let Some((casc, mut lib, abbey)) = northshire() else {
        eprintln!("no camelot client: skipped");
        return;
    };
    let started = std::time::Instant::now();
    let solids = lib.placed_near(&casc, 0, -8949.95, -132.493, 80.0);
    eprintln!("the solids within 80 yards read in {:?}", started.elapsed());
    let props: Vec<_> = solids
        .iter()
        .filter(|p| {
            let size = |k: usize| p.bounds[1][k] - p.bounds[0][k];
            p.is_prop()
                && size(2) > 1.5
                && size(0) > 0.8
                && size(0) < 6.0
                && size(1) > 0.8
                && size(1) < 6.0
        })
        .take(20)
        .collect();
    let mut stopped = 0;
    for p in &props {
        let mid = [
            (p.bounds[0][0] + p.bounds[1][0]) / 2.0,
            (p.bounds[0][1] + p.bounds[1][1]) / 2.0,
        ];
        let away = (p.bounds[1][1] - p.bounds[0][1]) / 2.0 + 3.0;
        let z = p.bounds[0][2];
        if lib.walk(
            &casc,
            0,
            [mid[0], mid[1] - away, z],
            [mid[0], mid[1], z],
            0.306,
        ) < 1.0
        {
            stopped += 1;
        }
    }
    eprintln!(
        "{} solids within 80 yards, {} props of a person's size: {stopped} stop one",
        solids.len(),
        props.len()
    );
    assert!(
        props.len() >= 10 && stopped * 4 >= props.len() * 3,
        "{stopped} of {}",
        props.len()
    );
    // (a wall of the abbey, walked at from outside: a wider body stops
    // further from it)
    let mid = [
        (abbey.bounds[0][0] + abbey.bounds[1][0]) / 2.0,
        (abbey.bounds[0][1] + abbey.bounds[1][1]) / 2.0,
    ];
    let reach = abbey.bounds[1][0] - abbey.bounds[0][0];
    let from = [mid[0] + reach, mid[1]];
    let z = lib.ground(&casc, 0, from[0], from[1]).expect("the ground");
    let (a, b) = ([from[0], from[1], z], [mid[0], mid[1], z]);
    let thin = lib.walk(&casc, 0, a, b, 0.0);
    let wide = lib.walk(&casc, 0, a, b, 0.5);
    eprintln!("toward the abbey: {thin:.4} of the way, {wide:.4} a body 0.5 wide");
    assert!(thin < 1.0, "{thin}");
    assert!(((thin - wide) * reach - 0.5).abs() < 0.05, "{thin} {wide}");
    let human = lib
        .character_display(&casc, 1, 0)
        .and_then(|d| lib.body_radius(&casc, d));
    eprintln!("a human male's body: {human:?}");
    assert!(human.is_some_and(|r| (r - 0.306).abs() < 0.01), "{human:?}");
}

/// Each product's global strings read from its `GlobalStrings` table
/// (decision T553): `OKAY` among the strings set in the game.
#[test]
fn reads_the_global_strings() {
    let Some(root) = install() else {
        eprintln!("no WoW installation: skipped");
        return;
    };
    let storage = FsStorage { root: root.clone() };
    for (product, version) in Casc::products(&storage) {
        let casc = Casc::open(Box::new(FsStorage { root: root.clone() }), &product).expect("opens");
        let strings = wowlua_casc::globalstrings::read(&casc).expect("GlobalStrings");
        let in_game = strings
            .iter()
            .filter(|(_, _, f)| f & wowlua_casc::globalstrings::IN_GAME != 0)
            .count();
        eprintln!(
            "{product} {version}: {} strings, {in_game} in the game",
            strings.len()
        );
        let okay = strings.iter().find(|(n, _, _)| n == "OKAY").expect("OKAY");
        assert_eq!(okay.1, "Okay", "{product}");
        // (written `%s says:\32` in the table)
        let say = strings.iter().find(|(n, _, _)| n == "CHAT_SAY_GET");
        assert_eq!(say.map(|s| s.1.as_str()), Some("%s says: "), "{product}");
        assert!(
            okay.2 & wowlua_casc::globalstrings::IN_GAME != 0,
            "{product}"
        );
        assert!(in_game > 10_000, "{product}: {in_game}");
    }
}

/// Each product's light (decision T555): Goldshire's at noon and at
/// midnight, as each client's own tables give it (Classic Era's night is
/// a bright blue, MoP Classic's direct light dim beside its ambient).
#[test]
fn reads_the_light_of_a_place_and_time() {
    let Some(root) = install() else {
        eprintln!("no WoW installation: skipped");
        return;
    };
    let storage = FsStorage { root: root.clone() };
    for (product, version) in Casc::products(&storage) {
        let casc = Casc::open(Box::new(FsStorage { root: root.clone() }), &product).expect("opens");
        let lights = wowlua_casc::light::Lights::read(&casc).expect("the light tables");
        let at = [-9465.0, 62.0, 56.0];
        let kind = wowlua_casc::light::Kind::Clear;
        let noon = lights.at(0, at, 1440.0, kind).expect("noon");
        let night = lights.at(0, at, 0.0, kind).expect("midnight");
        let sum = |c: [f32; 3]| c.iter().sum::<f32>();
        eprintln!(
            "{product} {version}: noon direct {:?} ambient {:?} sky top {:?} fog {:?} {}..{} skyboxes {:?}; midnight direct {:?}",
            noon.direct,
            noon.ambient,
            noon.sky[0],
            noon.fog,
            noon.fog_start,
            noon.fog_end,
            noon.skyboxes,
            night.direct
        );
        assert!(sum(noon.direct) + sum(noon.ambient) > 0.5, "{product}");
        assert_ne!(noon.direct, night.direct, "{product}");
        assert!(noon.fog_end > noon.fog_start, "{product}");
        assert!((100.0..2000.0).contains(&noon.fog_end), "{product}");
    }
}

/// Each product's liquids (decision T557): Elwynn's tiles about Northshire
/// have water, its heights about the water's level.
#[test]
fn reads_the_liquids() {
    let Some(root) = install() else {
        eprintln!("no WoW installation: skipped");
        return;
    };
    let storage = FsStorage { root: root.clone() };
    for (product, version) in Casc::products(&storage) {
        let casc = Casc::open(Box::new(FsStorage { root: root.clone() }), &product).expect("opens");
        let mut lib = wowlua_casc::display::Library::default();
        let mut kinds = std::collections::BTreeMap::new();
        let mut count = 0;
        for x in 31..34 {
            for y in 47..50 {
                let Some(tile) = lib.terrain_tile(&casc, 0, x, y) else {
                    continue;
                };
                for l in &tile.liquids {
                    count += 1;
                    *kinds.entry((l.kind, l.object)).or_insert(0) += 1;
                    let (heights, _) = l.heights(if l.object < 42 { l.object } else { 0 });
                    assert!(
                        heights
                            .iter()
                            .all(|h| h.is_finite() && *h > -500.0 && *h < 2000.0),
                        "{product}: {heights:?}"
                    );
                }
            }
        }
        eprintln!("{product} {version}: {count} liquids, (type, object): {kinds:?}");
        let tables = wowlua_casc::liquid::Liquids::read(&casc).expect("the liquid tables");
        let tile = lib
            .terrain_tile(&casc, 0, 32, 48)
            .expect("Northshire's tile");
        let seen: std::collections::BTreeSet<String> = tile
            .liquids
            .iter()
            .filter_map(|l| tables.resolve(l))
            .map(|r| {
                format!(
                    "{} {:?} format {} {} frames",
                    r.kind.name,
                    r.kind.look,
                    r.format,
                    r.kind.frames.len()
                )
            })
            .collect();
        eprintln!("  Northshire's: {seen:?}");
        assert!(!seen.is_empty(), "{product}");
        assert!(count > 0, "{product}");
    }
}
