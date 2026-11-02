//! What a model frame shows, as the build's tables give it (spec debugger
//! §18.10): a creature display's model file, its skins and geosets; an
//! item's models.
//!
//! Display IDs resolve through `CreatureDisplayInfo` and
//! `CreatureModelData`; creatures (`SetCreature`) through `Creature`'s
//! first display; a model file is itself.

use std::collections::HashMap;

use crate::Casc;
use crate::tables::Table;

/// A texture a part is drawn with: a file, or a character's skin
/// composited from layers.
#[derive(Clone, Debug, PartialEq)]
pub enum Texture {
    File(u32),
    Composite(Composite),
}

/// A texture composited from files, each into its rectangle in order.
#[derive(Clone, Debug, PartialEq, Default)]
pub struct Composite {
    pub width: u32,
    pub height: u32,
    pub layers: Vec<Layer>,
}

/// A file drawn into a composite: its rectangle (x, y, width, height) and
/// blend mode (`ChrModelTextureLayer.BlendMode`: 0 and 1 copy, 4
/// multiply, 6 overlay, 7 screen, others blend by alpha).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Layer {
    pub file: u32,
    pub rect: [u32; 4],
    pub blend: u32,
}

/// A model file and what it is drawn with.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Part {
    /// The `.m2` file.
    pub file: u32,
    /// Textures by kind (11 to 13 a creature's skins, 2 an item's or a
    /// cape's, 1 the character's skin, 6 hair, …).
    pub textures: HashMap<u32, Texture>,
    /// Geoset groups the display decides: a group's submeshes shown
    /// (`group * 100 + variant`), the group's others hidden.
    pub groups: HashMap<u16, Vec<u16>>,
    /// Submeshes shown or hidden over the defaults (customizations).
    pub shown: Vec<u16>,
    pub hidden: Vec<u16>,
    /// Groups hidden whole (hair under a helm).
    pub hidden_groups: Vec<u16>,
    /// The attachment point of the first part this one sits at (an
    /// item's), else none.
    pub attachment: Option<u32>,
    /// Posed with the first part's bones (a model made for its skeleton:
    /// armor and customizations).
    pub skinned: bool,
}

impl Part {
    pub fn with_file(file: u32) -> Part {
        Part {
            file,
            ..Part::default()
        }
    }

    /// Whether a submesh shows.
    pub fn shows(&self, id: u16) -> bool {
        // The body always shows.
        if id == 0 {
            return true;
        }
        if self.hidden_groups.contains(&(id / 100)) {
            return false;
        }
        if let Some(ids) = self.groups.get(&(id / 100))
            && id >= 100
        {
            return ids.contains(&id);
        }
        if self.shown.contains(&id) {
            return true;
        }
        if self.hidden.contains(&id) {
            return false;
        }
        default_geoset(id)
    }

    /// Shows `group * 100 + variant` alone of its group.
    pub fn set_group(&mut self, group: u16, variant: u16) {
        self.groups.insert(group, vec![group * 100 + variant]);
    }
}

/// A display: its parts (the first the body), its scale and alpha.
#[derive(Clone, Debug, PartialEq)]
pub struct Display {
    pub parts: Vec<Part>,
    pub scale: f32,
    pub alpha: f32,
}

/// The tables, opened as first needed.
#[derive(Default)]
pub struct Library {
    pub(crate) tables: HashMap<&'static str, Option<Table>>,
    /// The client's creature cache: creatures' displays.
    creature_cache: Option<HashMap<u32, Vec<u32>>>,
    /// Each map's minimap tiles, read from its WDT on first use.
    minimap: HashMap<u32, Option<Vec<u32>>>,
    /// Each map's terrain tiles' files (decision T522).
    terrain_files: HashMap<u32, Option<crate::wdt::Tiles>>,
    /// The terrain tiles read, by map, column and row: the last
    /// [`TILES_KEPT`] asked for.
    terrain: HashMap<(u32, i32, i32), (u64, Option<SharedTile>)>,
    terrain_asked: u64,
    /// Each terrain tile's buildings and doodads (decision T523).
    objects: HashMap<(u32, i32, i32), Option<std::sync::Arc<crate::obj::Objects>>>,
    /// The buildings read, by their root file.
    buildings: HashMap<u32, Option<std::sync::Arc<Building>>>,
    /// The creatures the installation knows, read once (decision T531).
    creatures: Option<Vec<Creature>>,
    /// The props' collision meshes read, by model file (decision T532).
    pub(crate) props: HashMap<u32, Option<std::sync::Arc<crate::collide::Prop>>>,
    /// Each map tile's solids, gathered when last asked for.
    pub(crate) solids: HashMap<(u32, i32, i32), (u64, TileSolids)>,
    pub(crate) solids_asked: u64,
    /// The build's outdoor light, read on first use (decision T555).
    lights: Option<Option<std::sync::Arc<crate::light::Lights>>>,
    /// The build's liquids, read on first use (decision T557).
    liquids: Option<Option<std::sync::Arc<crate::liquid::Liquids>>>,
    /// The build's ground effects, read on first use (decision T560).
    clutter: Option<Option<std::sync::Arc<crate::clutter::Clutter>>>,
}

/// A creature the installation knows (decision T531): from the client's
/// `Creature` table (with its type, family and classification) or its
/// creature cache (what the server told it of the creatures it met).
#[derive(Clone, Debug, PartialEq)]
pub struct Creature {
    pub id: u32,
    pub name: String,
    pub display: Option<u32>,
    /// `CreatureType` (1 Beast, 2 Dragonkin, 3 Demon, 4 Elemental, 5
    /// Giant, 6 Undead, 7 Humanoid, 8 Critter, 9 Mechanical, …).
    pub kind: Option<u32>,
    /// `CreatureFamily`'s name ("Wolf").
    pub family: Option<String>,
    /// 0 normal, 1 elite, 2 rare elite, 3 world boss, 4 rare, 5
    /// trivial, 6 minus.
    pub classification: Option<u32>,
}

/// A terrain tile read, shared with whoever draws it.
type SharedTile = std::sync::Arc<crate::adt::Tile>;

/// A terrain tile's solids, listed (decision T532).
type TileSolids = std::sync::Arc<crate::collide::Solids>;

/// A building (a WMO) with its groups (decision T523).
#[derive(Clone, Debug, PartialEq)]
pub struct Building {
    pub root: crate::wmo::Root,
    pub groups: Vec<crate::wmo::Group>,
}

/// How many terrain tiles a library keeps read (a 7 × 7 around the player).
const TILES_KEPT: usize = 49;

impl Library {
    pub fn table(&mut self, casc: &Casc, name: &'static str) -> Option<&Table> {
        self.tables
            .entry(name)
            .or_insert_with(|| Table::open(casc, name))
            .as_ref()
    }

    /// A map's (`Map` ID, the instance) minimap texture of the terrain
    /// tile at column `x` and row `y` (0 to 63, from the north-west): its
    /// WDT's, the file the map names (`WdtFileDataID`), else the one at
    /// its directory's path (`world/maps/<dir>/<dir>.wdt`, the classic
    /// builds' tables naming none).
    pub fn minimap_tile(&mut self, casc: &Casc, map: u32, x: u32, y: u32) -> Option<u32> {
        if x >= 64 || y >= 64 {
            return None;
        }
        if !self.minimap.contains_key(&map) {
            let tiles = self
                .wdt(casc, map)
                .and_then(|b| crate::wdt::minimap_tiles(&b));
            self.minimap.insert(map, tiles);
        }
        let t = self.minimap.get(&map)?.as_ref()?[(y * 64 + x) as usize];
        (t != 0).then_some(t)
    }

    /// A map's WDT: the file the map names, else the one at its
    /// directory's path.
    fn wdt(&mut self, casc: &Casc, map: u32) -> Option<Vec<u8>> {
        let t = self.table(casc, "Map")?;
        let r = t.row(map)?;
        let named = t.has("WdtFileDataID").then(|| r.int("WdtFileDataID"));
        match named.filter(|f| *f != 0) {
            Some(f) => casc.file_by_id(f),
            None => {
                let dir = r.string("Directory")?;
                casc.file_by_path(&format!("world/maps/{dir}/{dir}.wdt"))
            }
        }
    }

    /// The build's outdoor light (decision T555), read on first use; none
    /// where its tables cannot be read.
    pub fn lights(&mut self, casc: &Casc) -> Option<std::sync::Arc<crate::light::Lights>> {
        self.lights
            .get_or_insert_with(|| crate::light::Lights::read(casc).map(std::sync::Arc::new))
            .clone()
    }

    /// The build's ground effects (decision T560), read on first use; none
    /// where its tables cannot be read.
    pub fn clutter(&mut self, casc: &Casc) -> Option<std::sync::Arc<crate::clutter::Clutter>> {
        self.clutter
            .get_or_insert_with(|| crate::clutter::Clutter::read(casc).map(std::sync::Arc::new))
            .clone()
    }

    /// The build's liquids (decision T557), read on first use; none where
    /// its tables cannot be read.
    pub fn liquids(&mut self, casc: &Casc) -> Option<std::sync::Arc<crate::liquid::Liquids>> {
        self.liquids
            .get_or_insert_with(|| crate::liquid::Liquids::read(casc).map(std::sync::Arc::new))
            .clone()
    }

    /// A map's terrain tile at a column and row (0 to 63, from the
    /// north-west), read from its root and textures files on first use
    /// (decision T522); none where the map has no tile.
    pub fn terrain_tile(
        &mut self,
        casc: &Casc,
        map: u32,
        x: i32,
        y: i32,
    ) -> Option<std::sync::Arc<crate::adt::Tile>> {
        if !(0..64).contains(&x) || !(0..64).contains(&y) {
            return None;
        }
        self.terrain_asked += 1;
        let asked = self.terrain_asked;
        if let Some((when, tile)) = self.terrain.get_mut(&(map, x, y)) {
            *when = asked;
            return tile.clone();
        }
        if !self.terrain_files.contains_key(&map) {
            let tiles = self.wdt(casc, map).and_then(|b| crate::wdt::tiles(&b));
            self.terrain_files.insert(map, tiles);
        }
        let tile = self.terrain_files.get(&map)?.as_ref().and_then(|t| {
            let files = t.tiles[(y * 64 + x) as usize];
            let big = t.flags & (0x4 | 0x80) != 0;
            let root = (files.root != 0).then(|| casc.file_by_id(files.root))??;
            let tex = (files.tex0 != 0)
                .then(|| casc.file_by_id(files.tex0))
                .flatten();
            crate::adt::read(&root, tex.as_deref(), big).map(std::sync::Arc::new)
        });
        // (the tile asked for longest ago goes)
        if self.terrain.len() >= TILES_KEPT
            && let Some(old) = self
                .terrain
                .iter()
                .min_by_key(|(_, (when, _))| *when)
                .map(|(k, _)| *k)
        {
            self.terrain.remove(&old);
        }
        self.terrain.insert((map, x, y), (asked, tile.clone()));
        tile
    }

    /// The ground's height at a place of a map (north and west yards), from
    /// its terrain (decision T522); none off its tiles or over a hole.
    pub fn ground(&mut self, casc: &Casc, map: u32, north: f64, west: f64) -> Option<f64> {
        let (x, y) = crate::adt::tile_of(north, west);
        let tile = self.terrain_tile(casc, map, x, y)?;
        let (n, w) = crate::adt::corner(x, y);
        tile.height_at(n - north, w - west)
    }

    /// A map tile's buildings and doodads, from its objects file
    /// (decision T523), read once.
    pub fn objects(
        &mut self,
        casc: &Casc,
        map: u32,
        x: i32,
        y: i32,
    ) -> Option<std::sync::Arc<crate::obj::Objects>> {
        if !(0..64).contains(&x) || !(0..64).contains(&y) {
            return None;
        }
        if let Some(o) = self.objects.get(&(map, x, y)) {
            return o.clone();
        }
        if !self.terrain_files.contains_key(&map) {
            let tiles = self.wdt(casc, map).and_then(|b| crate::wdt::tiles(&b));
            self.terrain_files.insert(map, tiles);
        }
        let objects = self.terrain_files.get(&map)?.as_ref().and_then(|t| {
            let files = t.tiles[(y * 64 + x) as usize];
            let data = (files.obj0 != 0).then(|| casc.file_by_id(files.obj0))??;
            let by_path = |p: &str| casc.id_of(p);
            Some(std::sync::Arc::new(crate::obj::read(&data, &by_path)))
        });
        self.objects.insert((map, x, y), objects.clone());
        objects
    }

    /// A building (a WMO root file's ID) with its groups, read once
    /// (decision T523).
    pub fn building(&mut self, casc: &Casc, file: u32) -> Option<std::sync::Arc<Building>> {
        if let Some(b) = self.buildings.get(&file) {
            return b.clone();
        }
        let by_path = |p: &str| casc.id_of(p);
        let building = casc
            .file_by_id(file)
            .and_then(|data| crate::wmo::Root::read(&data, &by_path))
            .map(|root| {
                let groups = root
                    .groups
                    .iter()
                    .filter_map(|g| crate::wmo::Group::read(&casc.file_by_id(*g)?))
                    .collect();
                std::sync::Arc::new(Building { root, groups })
            });
        self.buildings.insert(file, building.clone());
        building
    }

    /// The creatures the installation knows (decision T531): its
    /// `Creature` table's, then its creature caches' it lacks, by ID.
    pub fn creatures(&mut self, casc: &Casc) -> &[Creature] {
        if self.creatures.is_none() {
            let mut all: Vec<Creature> = Vec::new();
            let families: HashMap<u32, String> = self
                .table(casc, "CreatureFamily")
                .map(|t| {
                    t.rows()
                        .filter_map(|r| Some((r.id(), r.string("Name_lang")?)))
                        .collect()
                })
                .unwrap_or_default();
            if let Some(t) = self.table(casc, "Creature") {
                for r in t.rows() {
                    let Some(name) = r.string("Name_lang").filter(|n| !n.is_empty()) else {
                        continue;
                    };
                    all.push(Creature {
                        id: r.id(),
                        name,
                        display: r.ints("DisplayID").into_iter().find(|&d| d != 0),
                        kind: Some(r.int("CreatureType")).filter(|k| *k != 0),
                        family: families.get(&r.int("CreatureFamily")).cloned(),
                        classification: Some(r.int("Classification")),
                    });
                }
            }
            let known: std::collections::HashSet<u32> = all.iter().map(|c| c.id).collect();
            let mut cached: Vec<Creature> = casc
                .cache_files("creaturecache.wdb")
                .iter()
                .flat_map(|f| crate::wdb::creatures(f))
                .filter(|(id, _, _)| !known.contains(id))
                .map(|(id, name, displays)| Creature {
                    id,
                    name,
                    display: displays.first().copied(),
                    kind: None,
                    family: None,
                    classification: None,
                })
                .collect();
            cached.sort_by_key(|c| c.id);
            cached.dedup_by_key(|c| c.id);
            all.extend(cached);
            self.creatures = Some(all);
        }
        self.creatures.as_deref().unwrap_or_default()
    }

    /// A creature display (`CreatureDisplayInfo` ID).
    pub fn creature_display(&mut self, casc: &Casc, id: u32) -> Option<Display> {
        let (model_id, scale, alpha, extended, textures) = {
            let t = self.table(casc, "CreatureDisplayInfo")?;
            let r = t.row(id)?;
            let mut textures = r.ints("TextureVariationFileDataID");
            textures.resize(4, 0);
            (
                r.int("ModelID"),
                r.float("CreatureModelScale"),
                r.int("CreatureModelAlpha"),
                r.int("ExtendedDisplayInfoID"),
                textures,
            )
        };
        let (file, model_scale, has_geosets) = {
            let t = self.table(casc, "CreatureModelData")?;
            let r = t.row(model_id)?;
            (
                r.int("FileDataID"),
                r.float("ModelScale"),
                r.int("CreatureGeosetDataID") != 0,
            )
        };
        let scale = if scale > 0.0 { scale } else { 1.0 };
        let model_scale = if model_scale > 0.0 { model_scale } else { 1.0 };
        // A humanoid: the character its extra info describes.
        if extended != 0
            && let Some(mut d) = self.npc(casc, extended, file)
        {
            d.scale *= scale;
            return Some(d);
        }
        let mut part = Part::with_file(file);
        // Skins: kinds 11 to 13 the display's first three; kinds 2 to 4
        // (object skins) the same, as model viewers take them.
        for (k, &f) in textures.iter().enumerate().take(3) {
            if f != 0 {
                part.textures.insert(11 + k as u32, Texture::File(f));
                part.textures.insert(2 + k as u32, Texture::File(f));
            }
        }
        if has_geosets && let Some(t) = self.table(casc, "CreatureDisplayInfoGeosetData") {
            for g in t.rows_where("CreatureDisplayInfoID", id) {
                let group = (g.int("GeosetIndex") + 1) as u16;
                let geoset = group * 100 + g.int("GeosetValue") as u16;
                part.groups.entry(group).or_default().push(geoset);
            }
        }
        Some(Display {
            parts: vec![part],
            scale: scale * model_scale,
            alpha: if alpha == 0 {
                1.0
            } else {
                alpha as f32 / 255.0
            },
        })
    }

    /// A creature's (`Creature` ID) first display ID: the client's table's,
    /// else its creature cache's (what the server told it).
    pub fn creature_display_id(&mut self, casc: &Casc, id: u32) -> Option<u32> {
        let own = self
            .table(casc, "Creature")
            .and_then(|t| t.row(id))
            .and_then(|r| r.ints("DisplayID").into_iter().find(|&d| d != 0));
        if own.is_some() {
            return own;
        }
        let cache = self.creature_cache.get_or_insert_with(|| {
            let mut all = HashMap::new();
            for f in casc.cache_files("creaturecache.wdb") {
                all.extend(crate::wdb::creature_displays(&f));
            }
            all
        });
        cache.get(&id)?.first().copied()
    }

    /// A creature's (`Creature` ID) first display: the client's table's,
    /// else its creature cache's (what the server told it).
    pub fn creature(&mut self, casc: &Casc, id: u32) -> Option<Display> {
        let display = self.creature_display_id(casc, id)?;
        self.creature_display(casc, display)
    }

    /// How wide a creature display's body is, a radius in yards (decision
    /// T532): half its model data's collision width (`CollisionWidth`; a
    /// human male's 0.611, the 0.306 radius the game's servers know) at
    /// the display's scale.
    pub fn body_radius(&mut self, casc: &Casc, display: u32) -> Option<f64> {
        let (model, scale) = {
            let r = self.table(casc, "CreatureDisplayInfo")?.row(display)?;
            (r.int("ModelID"), r.float("CreatureModelScale"))
        };
        let width = self
            .table(casc, "CreatureModelData")?
            .row(model)?
            .float("CollisionWidth");
        let scale = if scale > 0.0 { scale } else { 1.0 };
        (width > 0.0).then(|| f64::from(width * scale) / 2.0)
    }

    /// A race's and sex's character display (`ChrModel`'s).
    pub fn character_display(&mut self, casc: &Casc, race: u32, sex: u32) -> Option<u32> {
        let chr_model = self
            .table(casc, "ChrRaceXChrModel")?
            .rows_where("ChrRacesID", race)
            .iter()
            .find(|r| r.int("Sex") == sex)?
            .int("ChrModelID");
        let display = self
            .table(casc, "ChrModel")?
            .row(chr_model)?
            .int("DisplayID");
        (display != 0).then_some(display)
    }

    /// A model file by itself.
    pub fn file(file: u32) -> Display {
        Display {
            parts: vec![Part::with_file(file)],
            scale: 1.0,
            alpha: 1.0,
        }
    }
}

/// Whether a submesh shows by default (no display naming its geosets):
/// the body (0), every group's first variant (`x01`) and group 32; not
/// the eye glows (17) nor group 35.
pub fn default_geoset(id: u16) -> bool {
    if id < 100 {
        return id == 0;
    }
    let group = id / 100;
    if group == 17 || group == 35 {
        return false;
    }
    id % 100 == 1 || group == 32
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A group the display decides shows its named submeshes alone; the
    /// customizations' over the defaults; the body always.
    #[test]
    fn parts_show_their_geosets() {
        let mut p = Part::with_file(1);
        assert!(p.shows(0) && p.shows(401) && !p.shows(402) && !p.shows(1701));
        p.set_group(4, 3);
        assert!(p.shows(403) && !p.shows(401));
        p.shown.push(5);
        p.hidden.push(0);
        p.hidden.push(501);
        assert!(p.shows(5) && p.shows(0) && !p.shows(501));
    }
}
