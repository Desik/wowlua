//! The client's tables by name: each build's layout (which field is
//! which) known by its layout hash, from [`crate::schema`]; a layout not
//! listed is taken as a listed one of the same field count.

use std::collections::HashMap;
use std::sync::Mutex;

use crate::Casc;
use crate::db2::Db2;

/// The tables' file IDs (the same in every build).
const FILES: &[(&str, u32)] = &[
    ("Achievement", 1260179),
    ("Achievement_Category", 1324299),
    ("AreaTable", 1353545),
    ("CharBaseInfo", 1343386),
    ("CharComponentTextureLayouts", 1360262),
    ("CharComponentTextureSections", 1360263),
    ("ChrCustomizationChoice", 3450554),
    ("ChrCustomizationElement", 3512765),
    ("ChrCustomizationGeoset", 3456171),
    ("ChrCustomizationMaterial", 3459652),
    ("ChrCustomizationOption", 3384247),
    ("ChrCustomizationReq", 3450453),
    ("ChrCustomizationSkinnedModel", 3460183),
    ("ChrModel", 3384313),
    ("ChrModelMaterial", 3566562),
    ("ChrModelTextureLayer", 3548976),
    ("ChrRaceXChrModel", 3490304),
    ("ChrRaces", 1305311),
    ("ComponentModelFileData", 1349053),
    ("ComponentTextureFileData", 1278239),
    ("Creature", 841631),
    ("CreatureDisplayInfo", 1108759),
    ("CreatureDisplayInfoExtra", 1264997),
    ("CreatureDisplayInfoGeosetData", 1720141),
    ("CreatureDisplayInfoOption", 3692043),
    ("CreatureModelData", 1365368),
    ("Criteria", 1263817),
    ("CriteriaTree", 1263818),
    ("GlobalColor", 4328468),
    ("GroundEffectDoodad", 1308057),
    ("GroundEffectTexture", 1308499),
    ("GlobalStrings", 1394440),
    ("HelmetGeosetData", 2821752),
    ("Item", 841626),
    ("ItemAppearance", 982462),
    ("ItemClass", 1140189),
    ("ItemDisplayInfo", 1266429),
    ("ItemDisplayInfoMaterialRes", 1280614),
    ("ItemDisplayInfoModelMatRes", 4050937),
    ("ItemModifiedAppearance", 982457),
    ("ItemSearchName", 1273408),
    ("ItemSparse", 1572924),
    ("ItemSubClass", 1261604),
    ("Light", 1375579),
    ("LightData", 1375580),
    ("LightParams", 1334669),
    ("LightSkybox", 1308501),
    ("LiquidMaterial", 1132538),
    ("LiquidObject", 1308058),
    ("LiquidType", 1371380),
    ("Weather", 1343311),
    ("SpellXSpellVisual", 1101657),
    ("SpellVisual", 897952),
    ("SpellVisualEvent", 1685317),
    ("SpellVisualKitEffect", 1140480),
    ("SpellVisualKitModelAttach", 897953),
    ("SpellVisualEffectName", 897948),
    ("SpellVisualMissile", 897954),
    ("LiquidTypeXTexture", 2261065),
    ("Map", 1349477),
    ("ModelFileData", 1337833),
    ("NPCModelItemSlotDisplayInfo", 1340661),
    ("SharedString", 4620073),
    ("SkillLine", 1240935),
    ("SkillLineAbility", 1266278),
    ("SkillRaceClassInfo", 1240406),
    ("Spell", 1140089),
    ("SpellAuraOptions", 1139952),
    ("SpellCastTimes", 1134089),
    ("SpellCastingRequirements", 1002166),
    ("SpellCooldowns", 1139924),
    ("SpellDuration", 1137828),
    ("SpellEffect", 1140088),
    ("SpellEquippedItems", 1140011),
    ("SpellFocusObject", 1280620),
    ("SpellLevels", 1140079),
    ("SpellMisc", 1003144),
    ("SpellName", 1990283),
    ("SpellPower", 982806),
    ("SpellRadius", 1134584),
    ("SpellRange", 1146820),
    ("SpellReagents", 841946),
    ("SpellShapeshift", 1139929),
    ("SpellShapeshiftForm", 1280618),
    ("SpellTotems", 1002162),
    ("TaxiNodes", 1068100),
    ("TextureFileData", 982459),
    ("TotemCategory", 1134586),
    ("TradeSkillCategory", 1301101),
    ("TraitCond", 4543085),
    ("TraitCost", 4420295),
    ("TraitCurrency", 4524216),
    ("TraitCurrencySource", 4539393),
    ("TraitDefinition", 4420327),
    ("TraitEdge", 4420308),
    ("TraitNode", 4420297),
    ("TraitNodeEntry", 4420298),
    ("TraitNodeEntryXTraitCond", 4543088),
    ("TraitNodeEntryXTraitCost", 4420299),
    ("TraitNodeGroup", 4420300),
    ("TraitNodeGroupXTraitCond", 4543090),
    ("TraitNodeGroupXTraitCost", 4420301),
    ("TraitNodeGroupXTraitNode", 4420302),
    ("TraitNodeXTraitCond", 4543092),
    ("TraitNodeXTraitCost", 4420303),
    ("TraitNodeXTraitNodeEntry", 4420304),
    ("TraitTree", 4420305),
    ("TraitTreeXTraitCost", 4420306),
    ("TraitTreeXTraitCurrency", 4524218),
    ("UiCamFbackTransmogChrRace", 1261406),
    ("UiCamFbackTransmogWeapon", 1261407),
    ("UiCamera", 1120287),
    ("UiCanvas", 1989363),
    ("UiMap", 1957206),
    ("UiMapArt", 1957202),
    ("UiMapArtStyleLayer", 1957208),
    ("UiMapArtTile", 1957210),
    ("UiMapAssignment", 1957219),
    ("UiMapXMapArt", 1957217),
    ("UiModelScene", 1548215),
    ("UiModelSceneActor", 1548214),
    ("UiModelSceneActorDisplay", 1548216),
    ("UiModelSceneCamera", 1548213),
    ("UiTextureAtlas", 897470),
    ("UiTextureAtlasElement", 1989276),
    ("UiTextureAtlasMember", 897532),
    ("WorldMapOverlay", 1134579),
    ("WorldMapOverlayTile", 1957212),
    ("ZoneLight", 1310253),
    ("ZoneLightPoint", 1310256),
];

/// The file IDs of the tables the engine reads (`wowlua data pull`).
pub(crate) fn file_ids() -> impl Iterator<Item = u32> {
    FILES.iter().map(|(_, id)| *id)
}

/// A column: a field of the records, their ID, or the parent ID of the
/// relationship map.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Col {
    Field(usize),
    Id,
    Relation,
}

/// Records by a column's value.
type Index = std::sync::Arc<HashMap<u32, Vec<usize>>>;

/// A table of the build, its fields by name.
pub struct Table {
    db: Db2,
    names: &'static [&'static str],
    /// Records by a column's value, built on first use.
    by: Mutex<HashMap<&'static str, Index>>,
}

/// A record of a table.
#[derive(Clone, Copy)]
pub struct Row<'a> {
    table: &'a Table,
    index: usize,
}

impl Table {
    /// The build's table `name`, if it can be read and its layout is
    /// known.
    pub fn open(casc: &Casc, name: &str) -> Option<Table> {
        let file = FILES.iter().find(|(n, _)| *n == name)?.1;
        let mut db = Db2::parse(casc.file_by_id(file)?)?;
        let layouts = crate::schema::LAYOUTS.iter().find(|(n, _)| *n == name)?.1;
        let fields = |names: &[&str]| names.iter().filter(|n| !n.starts_with('@')).count();
        let names = layouts
            .iter()
            .find(|(h, _)| *h == db.layout_hash)
            .or_else(|| layouts.iter().find(|(_, n)| fields(n) == db.field_count()))?
            .1;
        // A sparse table's strings are inline, as are a hotfix's: which
        // fields are (`$`).
        let strings = names
            .iter()
            .filter(|n| !n.starts_with('@'))
            .map(|n| n.starts_with('$'))
            .collect();
        db.set_string_fields(strings);
        // what the server changed since the build (decision T391), where
        // the layout gives the fields' sizes (`~bytes`)
        let sizes: Vec<Option<usize>> = names
            .iter()
            .filter(|n| !n.starts_with('@'))
            .map(|n| n.split_once('~').and_then(|(_, b)| b.parse().ok()))
            .collect();
        db.set_field_sizes(sizes.clone());
        db.apply_hotfixes(casc.hotfixes(db.table_hash), &sizes);
        Some(Table {
            db,
            names,
            by: Mutex::new(HashMap::new()),
        })
    }

    fn col(&self, name: &str) -> Option<Col> {
        let mut i = 0;
        for n in self.names {
            if let Some(rel) = n.strip_prefix('@') {
                if rel == name {
                    return Some(Col::Relation);
                }
                continue;
            }
            let n = n.split_once('~').map_or(*n, |(n, _)| n);
            if n.strip_prefix('$').unwrap_or(n) == name {
                return Some(Col::Field(i));
            }
            i += 1;
        }
        (name == "ID").then_some(Col::Id)
    }

    /// Whether the layout has a column.
    pub fn has(&self, name: &str) -> bool {
        self.col(name).is_some()
    }

    pub fn len(&self) -> usize {
        self.db.len()
    }

    pub fn is_empty(&self) -> bool {
        self.db.is_empty()
    }

    /// The record with ID `id`.
    pub fn row(&self, id: u32) -> Option<Row<'_>> {
        self.db.find(id).map(|index| Row { table: self, index })
    }

    /// Every record.
    pub fn rows(&self) -> impl Iterator<Item = Row<'_>> {
        (0..self.db.len()).map(move |index| Row { table: self, index })
    }

    /// The records whose column `name` is `value` (a parent's children).
    pub fn rows_where(&self, name: &'static str, value: u32) -> Vec<Row<'_>> {
        let map = {
            let Ok(mut by) = self.by.lock() else {
                return Vec::new();
            };
            by.entry(name)
                .or_insert_with(|| {
                    let mut m: HashMap<u32, Vec<usize>> = HashMap::new();
                    for r in self.rows() {
                        m.entry(r.int(name)).or_default().push(r.index);
                    }
                    std::sync::Arc::new(m)
                })
                .clone()
        };
        map.get(&value)
            .map(|v| v.iter().map(|&index| Row { table: self, index }).collect())
            .unwrap_or_default()
    }
}

impl Row<'_> {
    pub fn id(&self) -> u32 {
        self.table.db.id(self.index)
    }

    /// Whether the server changed or added the record (a hotfix): the
    /// client lists those after its file's (recorded: `TaxiNodes`).
    pub fn hotfixed(&self) -> bool {
        self.table.db.is_hot(self.index)
    }

    /// A column as an integer (0 when the layout lacks it).
    pub fn int(&self, name: &str) -> u32 {
        match self.table.col(name) {
            Some(Col::Field(f)) => self.table.db.int(self.index, f).unwrap_or(0),
            Some(Col::Id) => self.id(),
            Some(Col::Relation) => self.table.db.relation(self.index),
            None => 0,
        }
    }

    /// A column as a signed integer.
    pub fn signed(&self, name: &str) -> i32 {
        match self.table.col(name) {
            Some(Col::Field(f)) => self.table.db.signed(self.index, f).unwrap_or(0),
            _ => self.int(name) as i32,
        }
    }

    /// A float column (0 when the layout lacks it).
    pub fn float(&self, name: &str) -> f32 {
        match self.table.col(name) {
            Some(Col::Field(f)) => self.table.db.float(self.index, f).unwrap_or(0.0),
            _ => 0.0,
        }
    }

    /// An array column's elements.
    pub fn ints(&self, name: &str) -> Vec<u32> {
        match self.table.col(name) {
            Some(Col::Field(f)) => self.table.db.ints(self.index, f),
            Some(_) => vec![self.int(name)],
            None => Vec::new(),
        }
    }

    /// An array of floats.
    pub fn floats(&self, name: &str) -> Vec<f32> {
        self.ints(name).into_iter().map(f32::from_bits).collect()
    }

    pub fn string(&self, name: &str) -> Option<String> {
        match self.table.col(name)? {
            Col::Field(f) => self.table.db.string(self.index, f),
            _ => None,
        }
    }
}
