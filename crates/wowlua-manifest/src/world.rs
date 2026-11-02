//! World profiles (spec Tooling §16.1.4, decisions T213, T401): the JSON
//! files under `tests/profiles/` a test or a run starts in. Their model
//! here gives the schema (`schema/world-profile-0.1.json`, completion in
//! editors) and the checks (E4400, E4401): an unknown part at the top is an
//! error (the world ignores it); within a part the known fields are typed
//! and others pass to the engine's controls as they are.

use crate::load::{json_syntax_error, schema_error, span_of};
use crate::locate::JsonPath;
use schemars::JsonSchema;
use serde::Deserialize;
use serde_json::Value;
use std::collections::BTreeMap;
use std::path::Path;
use wowlua_diagnostics::{Diagnostic, Diagnostics};
use wowlua_span::{FileId, SourceDb, Span};

/// The schema's version (its file name and `$id`).
pub const SCHEMA_VERSION: &str = "0.1";

/// The schema's URL, for a profile's `$schema`.
pub fn schema_url() -> String {
    format!(
        "https://raw.githubusercontent.com/DomenikIrrgang/wowlua/main/schema/world-profile-{SCHEMA_VERSION}.json"
    )
}

/// Where a package's world profiles are, relative to its root.
pub const PROFILES_DIR: &str = "tests/profiles";

/// The world of a test that names no profile: the flavor's fresh
/// character as the client recorded it (camelot: a level 1 Human Warrior
/// in Elwynn Forest, `/stdtest snapshot`; `cargo xtask profile` writes it).
pub fn fresh_character(flavor: &str) -> Option<&'static str> {
    match flavor {
        "camelot" => Some(include_str!(
            "../../wowlua-engine/data/camelot-default-character.json"
        )),
        _ => None,
    }
}

/// The world profiles wowlua has built in (decision T533), by name: a
/// package names one as its own (`test.profile`, `config({ profile })`)
/// where it has none of that name. `northshire`: the fresh character at
/// Northshire's start among test NPCs of every kind (vendors of gear,
/// repairs, the class trainers, a banker, an auctioneer, a postmaster, a
/// flight master, gossip, things to fight).
pub const BUILT_IN: &[&str] = &["northshire"];

/// A built-in world profile's text for a flavor (decision T533): its parts
/// over the fresh character's (an object's entries over its); none where
/// the flavor has none.
pub fn built_in(flavor: &str, name: &str) -> Option<String> {
    let over = match (flavor, name) {
        ("camelot", "northshire") => {
            include_str!("../../wowlua-engine/data/camelot-northshire.json")
        }
        _ => return None,
    };
    let mut profile: serde_json::Map<String, Value> =
        serde_json::from_str(fresh_character(flavor)?).ok()?;
    let over: serde_json::Map<String, Value> = serde_json::from_str(over).ok()?;
    // (a part both have as an object, the CVars: the fresh character's
    // with the built-in's over them)
    for (key, value) in over {
        match (profile.get_mut(&key), value) {
            (Some(Value::Object(mine)), Value::Object(theirs)) => mine.extend(theirs),
            (_, value) => {
                profile.insert(key, value);
            }
        }
    }
    serde_json::to_string_pretty(&Value::Object(profile)).ok()
}

/// Other fields, passed to the engine's controls as they are.
type Rest = BTreeMap<String, Value>;

/// The situation a test or a run starts in, applied through `wowlua:mock`'s
/// controls before the addon loads.
#[derive(Debug, Default, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
#[schemars(
    title = "World profile",
    description = "The world a wowlua test or run starts in (tests/profiles/<name>.json)"
)]
pub struct WorldProfile {
    /// This schema's URL, for editors.
    #[serde(rename = "$schema")]
    pub schema: Option<String>,
    /// The client a recorded profile comes from (`/stdtest snapshot`).
    pub client: Option<String>,
    /// The leader boards a snapshot recorded (kept, not applied).
    pub leader_boards: Option<Value>,
    /// The player's character (`mock.setPlayer`).
    pub character: Option<UnitData>,
    /// Item data by item ID (`mock.defineItem`).
    pub items: Option<List<Item>>,
    /// Spell data by spell ID (`mock.defineSpell`): to override what the
    /// client's tables answer.
    pub spells: Option<List<Spell>>,
    /// The spells the character has trained, by spell ID
    /// (`mock.player:learn`; decision T500).
    pub trained: Option<List<f64>>,
    /// The character's money, in copper.
    pub money: Option<f64>,
    /// The character's stats by key (`mock.player:setStats`).
    pub stats: Option<Rest>,
    /// What is worn, by slot ID or the client's slot name
    /// (`MainHandSlot`): an item ID or an item.
    pub equipment: Option<List<ItemRef>>,
    /// The bags by bag ID (0 the backpack).
    pub bags: Option<List<Bag>>,
    /// The action bars' actions by slot (`mock.setAction`).
    pub actions: Option<List<Action>>,
    /// The account's and the character's macros (`mock.setMacros`).
    pub macros: Option<Macros>,
    /// The mount journal by mount ID (`mock.setMounts`).
    pub mounts: Option<List<Mount>>,
    /// The toy box by item ID (`mock.setToys`).
    pub toys: Option<List<Toy>>,
    /// The wardrobe (`mock.setAppearances`).
    pub appearances: Option<Appearances>,
    /// The chat channels the character is in (`mock.setChannels`).
    pub channels: Option<Channels>,
    /// Where the character is (`mock.moveTo`).
    pub position: Option<Position>,
    /// The realm's time of day, `"HH:MM"` (`mock.setTimeOfDay`; 12:00
    /// without it).
    pub time: Option<String>,
    /// The weather (`mock.setWeather`; clear without it).
    pub weather: Option<Weather>,
    /// The character's auras (`mock.player:applyAura`).
    pub auras: Option<List<Aura>>,
    /// Major factions' renown levels by faction ID (`mock.setRenown`).
    pub renown: Option<BTreeMap<String, f64>>,
    /// The units around: creatures and other players (`mock.spawnUnit`).
    pub units: Option<List<WorldUnit>>,
    /// CVars by name.
    pub cvars: Option<BTreeMap<String, Scalar>>,
    /// The screen the client draws on, over the manifest's `test.screen`
    /// (decision T482): its size, DPI scale and UI scale.
    pub screen: Option<crate::model::TestScreen>,
    /// Key bindings: key to action (`SetBinding`).
    pub bindings: Option<BTreeMap<String, String>>,
    /// SavedVariables by global name, as the addon finds them at load.
    pub saved_variables: Option<BTreeMap<String, Value>>,
}

/// An object of items by key, or a list of them (recorded from Lua, a
/// table keyed 1, 2, 3 is a list and an empty one `{}`; the world reads
/// either with `pairs`).
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(untagged)]
pub enum List<T> {
    Items(Vec<T>),
    Keyed(BTreeMap<String, T>),
}

/// A string, number or boolean.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(untagged)]
pub enum Scalar {
    String(String),
    Number(f64),
    Bool(bool),
}

/// A unit's data (the character's, or a unit's around).
#[derive(Debug, Default, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct UnitData {
    pub name: Option<String>,
    /// A WoW Forever character's last name.
    pub last_name: Option<String>,
    pub realm: Option<String>,
    pub guid: Option<String>,
    /// The level (`UnitLevel`); -1: a boss's ?? level. The character's is
    /// 1 to the flavor's max level (the flavor table's `max-level`: 60 on
    /// camelot, 60 on vanilla, 70 on tbc, 90 on standard).
    pub level: Option<f64>,
    /// The class by file name (`WARRIOR`).
    pub class: Option<String>,
    pub class_id: Option<f64>,
    /// The race by file name (`Human`). Its faction follows it, and the
    /// character knows its racial spells (decision T513).
    pub race: Option<String>,
    /// 1 unknown, 2 male, 3 female.
    pub sex: Option<f64>,
    pub health: Option<f64>,
    pub health_max: Option<f64>,
    pub max_health: Option<f64>,
    /// The shown power's amount (`UnitPower(unit)`).
    pub power: Option<f64>,
    /// The shown power's maximum (`UnitPowerMax(unit)`); `maxPower` the
    /// same.
    pub power_max: Option<f64>,
    pub max_power: Option<f64>,
    /// The power it shows (`Enum.PowerType`: 0 mana, 1 rage, 3 energy, …);
    /// by default its class's (a druid's mana).
    pub power_type: Option<f64>,
    /// Any of its powers by token (`MANA`, `RAGE`, `ENERGY`,
    /// `COMBO_POINTS`, …): how much it has and the most it can have. A
    /// unit has every power its class has (the flavor table's), each at
    /// the flavor's default unless named here (decision T512).
    pub powers: Option<BTreeMap<String, PowerAmount>>,
    /// `hostile`, `neutral` or `friendly`.
    pub reaction: Option<String>,
    /// A player character (not a creature).
    pub player: Option<bool>,
    pub dead: Option<bool>,
    /// Yards from the player. Without `world` the unit stands this far in
    /// front of the player; with it, its distance follows the two places
    /// (decision T515).
    pub distance: Option<f64>,
    /// Where it stands, as `UnitPosition` answers it (the player's
    /// instance): its distance from the player follows (decision T515).
    pub world: Option<WorldPosition>,
    /// Radians counter-clockwise from north; facing the player when not
    /// given.
    pub facing: Option<f64>,
    /// How it moves on its own (decision T517).
    pub movement: Option<UnitMovement>,
    /// What it does on its own (decision T518), each action when and what.
    pub actions: Option<Vec<UnitAction>>,
    /// `Alliance` or `Horde`: the one a race of either picked (the
    /// Pandaren, the Skyborne), or a creature's. A race of one faction has
    /// its own (decision T513): another is an error.
    pub faction: Option<String>,
    pub npc_id: Option<f64>,
    /// The creature display it shows in the 3D world (decision T526).
    pub display_id: Option<f64>,
    pub classification: Option<String>,
    /// An NPC's title, under its name in the 3D world (`Cloth Armor
    /// Merchant`; decision T534).
    pub title: Option<String>,
    pub creature_type: Option<String>,
    pub creature_family: Option<String>,
    pub zone: Option<String>,
    pub role: Option<String>,
    pub pvp: Option<bool>,
    /// What it sells (decision T530): its window opens when the player
    /// talks to it.
    pub vendor: Option<UnitVendor>,
    /// A trainer (decision T530): `{ class }`, the class's services as the
    /// client lists them, or `{ services }`, its own.
    pub trainer: Option<Value>,
    /// Its gossip (decision T530): `{ text, options = { { name, opens } },
    /// availableQuests }`; an option opens `vendor`, `trainer` or `taxi`.
    pub gossip: Option<Value>,
    /// A service it gives (decision T530).
    pub service: Option<UnitService>,
    /// Its swings' damage and speed where its attack actions say none
    /// (decision T530).
    pub attack: Option<UnitAttack>,
    /// The player's threat on it, in percent.
    pub threat: Option<f64>,
    /// What its corpse holds when a kill names none (decision T530).
    pub loot: Option<Vec<LootItem>>,
    #[serde(flatten)]
    pub other: Rest,
}

/// A vendor's goods (decision T530).
#[derive(Debug, Default, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct UnitVendor {
    pub items: Option<Vec<VendorItem>>,
    /// It repairs the player's gear.
    pub repairs: Option<bool>,
}

/// An item a vendor sells: its price (copper, by default the item's) for
/// `stack` of it, `available` left (-1: unlimited).
#[derive(Debug, Default, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct VendorItem {
    pub id: f64,
    pub price: Option<f64>,
    pub stack: Option<f64>,
    pub available: Option<f64>,
}

/// A service an NPC gives (decision T530).
#[derive(Debug, Default, Deserialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum UnitService {
    #[default]
    Banker,
    Mailbox,
    Auctioneer,
    Flightmaster,
}

/// A unit's swings (decision T530).
#[derive(Debug, Default, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct UnitAttack {
    /// Damage a swing (10).
    pub damage: Option<f64>,
    /// Seconds between swings (2).
    pub speed: Option<f64>,
}

/// An item a corpse holds.
#[derive(Debug, Default, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct LootItem {
    pub id: f64,
    pub count: Option<f64>,
}

/// One of a unit's powers: how much it has (`UnitPower(unit, type)`) and
/// the most it can have (`UnitPowerMax(unit, type)`).
#[derive(Debug, Default, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct PowerAmount {
    /// How much it has (`UnitPower(unit, type)`).
    pub current: Option<f64>,
    /// The most it can have (`UnitPowerMax(unit, type)`).
    pub max: Option<f64>,
}

/// How a unit moves on its own (decision T517), facing its way, its
/// distance from the player following.
#[derive(Debug, Default, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct UnitMovement {
    pub kind: MovementKind,
    /// A patrol's waypoints, in order.
    pub path: Option<Vec<WorldPosition>>,
    /// A patrol starts again at its first waypoint after the last (true by
    /// default); else it stands at the last.
    #[serde(rename = "loop")]
    pub looping: Option<bool>,
    /// Yards a second: 2.5 (walking) for a patrol, 7 (running) otherwise.
    pub speed: Option<f64>,
    /// Yards it keeps from the player following or coming to them (2:
    /// melee reach).
    pub keep: Option<f64>,
}

#[derive(Debug, Default, Deserialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum MovementKind {
    /// It stands.
    #[default]
    Stand,
    /// Along its `path`.
    Patrol,
    /// After the player, `keep` yards behind.
    Follow,
    /// To the player, stopping `keep` yards away.
    Approach,
    /// Away from the player.
    Flee,
}

/// A unit's action (decision T518): when (one of `at`, `every`,
/// `within`) and what (`do`).
#[derive(Debug, Default, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct UnitAction {
    /// Once, this many seconds after the unit appears.
    pub at: Option<f64>,
    /// Every so many seconds (the first after that long).
    pub every: Option<f64>,
    /// Each time the player comes within so many yards.
    pub within: Option<f64>,
    /// What it does.
    #[serde(rename = "do")]
    pub what: ActionKind,
    /// An attack's damage a swing (10).
    pub damage: Option<f64>,
    /// An attack's seconds between swings (2).
    pub speed: Option<f64>,
    /// A cast's spell ID.
    pub spell: Option<f64>,
    /// A cast's time in seconds (the spell's own by default).
    pub cast_time: Option<f64>,
}

#[derive(Debug, Default, Deserialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum ActionKind {
    /// Swings at the player while within melee reach (5 yards), from then
    /// on.
    #[default]
    Attack,
    /// Casts `spell`.
    Cast,
    /// Turns to the player.
    Face,
}

/// A unit around the player, and how it relates to them.
#[derive(Debug, Default, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct WorldUnit {
    #[serde(flatten)]
    pub data: UnitData,
    /// The player's target.
    pub target: Option<bool>,
    /// The player's focus.
    pub focus: Option<bool>,
    /// In the player's group: `party` or `raid`.
    pub group: Option<GroupKind>,
    /// Its nameplate shows.
    pub nameplate: Option<bool>,
    /// Another player accepts the player's invitations (true by default).
    pub accepts_invites: Option<bool>,
    pub auras: Option<List<Aura>>,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum GroupKind {
    Party,
    Raid,
}

/// Item data (`GetItemInfo`'s fields by name).
#[derive(Debug, Default, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct Item {
    pub name: Option<String>,
    pub link: Option<String>,
    pub quality: Option<f64>,
    pub item_level: Option<f64>,
    pub min_level: Option<f64>,
    #[serde(rename = "type")]
    pub kind: Option<String>,
    pub sub_type: Option<String>,
    pub stack_count: Option<f64>,
    pub equip_loc: Option<String>,
    pub icon: Option<Scalar>,
    pub sell_price: Option<f64>,
    pub class_id: Option<f64>,
    pub subclass_id: Option<f64>,
    pub bind_type: Option<f64>,
    pub expansion_id: Option<f64>,
    /// In the client's cache (true unless false).
    pub cached: Option<bool>,
    /// Seconds until its data loads.
    pub delay: Option<f64>,
    /// The server does not know it.
    pub missing: Option<bool>,
    #[serde(flatten)]
    pub other: Rest,
}

/// Spell data.
#[derive(Debug, Default, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct Spell {
    pub name: Option<String>,
    #[serde(rename = "iconID")]
    pub icon_id: Option<f64>,
    pub description: Option<String>,
    /// The player's pet knows it.
    pub pet_known: Option<bool>,
    pub cooldown: Option<Value>,
    pub charges: Option<Value>,
    pub cast_time: Option<f64>,
    pub min_range: Option<f64>,
    pub max_range: Option<f64>,
    pub cached: Option<bool>,
    #[serde(flatten)]
    pub other: Rest,
}

/// An item ID, or an item stack.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(untagged)]
pub enum ItemRef {
    Id(f64),
    Stack(ItemStack),
}

#[derive(Debug, Default, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ItemStack {
    pub id: f64,
    pub count: Option<f64>,
    pub link: Option<String>,
    pub bound: Option<bool>,
    pub locked: Option<bool>,
    /// `{ remaining, duration }` in seconds.
    pub cooldown: Option<Value>,
    #[serde(flatten)]
    pub other: Rest,
}

/// A bag: its size and what is in it by slot.
#[derive(Debug, Default, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct Bag {
    pub slots: Option<f64>,
    pub items: Option<List<ItemRef>>,
    #[serde(flatten)]
    pub other: Rest,
}

/// An action bar slot's action.
#[derive(Debug, Default, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct Action {
    /// `spell`, `item`, `macro`, …
    #[serde(rename = "type")]
    pub kind: Option<String>,
    pub id: Option<Scalar>,
    pub sub_type: Option<String>,
    pub texture: Option<Scalar>,
    /// A number, or `""` as the client reports none.
    pub count: Option<Scalar>,
    pub text: Option<String>,
    /// A boolean, or `{ usable, noPower }`.
    pub usable: Option<Value>,
    #[serde(flatten)]
    pub other: Rest,
}

#[derive(Debug, Default, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct Macros {
    pub account: Option<List<Macro>>,
    pub character: Option<List<Macro>>,
}

#[derive(Debug, Default, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct Macro {
    pub name: Option<String>,
    pub icon: Option<Scalar>,
    pub body: Option<String>,
    #[serde(flatten)]
    pub other: Rest,
}

#[derive(Debug, Default, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct Mount {
    #[serde(rename = "spellID")]
    pub spell_id: Option<f64>,
    pub name: Option<String>,
    pub icon: Option<Scalar>,
    pub collected: Option<bool>,
    pub usable: Option<bool>,
    pub favorite: Option<bool>,
    #[serde(flatten)]
    pub other: Rest,
}

#[derive(Debug, Default, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct Toy {
    pub collected: Option<bool>,
    pub usable: Option<bool>,
    pub favorite: Option<bool>,
    pub fanfare: Option<bool>,
    #[serde(flatten)]
    pub other: Rest,
}

#[derive(Debug, Default, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct Appearances {
    /// Collected source IDs.
    pub collected: Option<List<f64>>,
    /// Favourite appearance IDs.
    pub favorites: Option<List<f64>>,
}

/// The chat channels the character is in (`mock.setChannels`).
#[derive(Debug, Default, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct Channels {
    /// The channels, numbered in order.
    pub joined: Option<List<Channel>>,
    /// How many group channels the character is in
    /// (`GetNumGroupChannels`).
    pub group_channels: Option<f64>,
    /// Whether the regional chat service is up
    /// (`C_ChatInfo.IsRegionalServiceAvailable`).
    pub regional_service: Option<bool>,
}

/// A chat channel the character is in.
#[derive(Debug, Default, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct Channel {
    pub name: String,
    /// With its zone (`General - Elwynn Forest`); by default the name.
    pub full_name: Option<String>,
    /// `CHANNEL_CATEGORY_WORLD`, `CHANNEL_CATEGORY_CUSTOM` (the default) or
    /// `CHANNEL_CATEGORY_GROUP`.
    pub category: Option<String>,
    pub disabled: Option<bool>,
    /// By default the name.
    pub shortcut: Option<String>,
    #[serde(rename = "instanceID")]
    pub instance_id: Option<f64>,
    #[serde(rename = "zoneChannelID")]
    pub zone_channel_id: Option<f64>,
    pub channel_type: Option<f64>,
    /// `C_ChatInfo.IsChannelRegional`.
    pub regional: Option<bool>,
    /// Its number when higher than the one after the channel before (a
    /// channel left keeps its number free).
    pub number: Option<f64>,
    #[serde(flatten)]
    pub other: Rest,
}

/// The world's weather (decision T565).
#[derive(Debug, Deserialize, JsonSchema)]
pub struct Weather {
    pub kind: WeatherKind,
    /// 0 to 1 (1 without it).
    pub intensity: Option<f64>,
}

/// A kind of weather.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum WeatherKind {
    Clear,
    Rain,
    Snow,
    Sandstorm,
}

/// Where the character is.
#[derive(Debug, Default, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct Position {
    #[serde(rename = "mapID")]
    pub map_id: Option<f64>,
    pub zone: Option<String>,
    pub sub_zone: Option<String>,
    /// On the map, 0 to 1.
    pub x: Option<f64>,
    pub y: Option<f64>,
    /// `UnitPosition`'s world coordinates.
    pub world: Option<WorldPosition>,
    /// Radians counter-clockwise from north.
    pub facing: Option<f64>,
    #[serde(flatten)]
    pub other: Rest,
}

/// A place in the world, as `UnitPosition` answers it.
#[derive(Debug, Default, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct WorldPosition {
    /// North, in yards.
    pub y: f64,
    /// West, in yards.
    pub x: f64,
    /// Up, in yards (0 by default).
    pub z: Option<f64>,
    /// The continent's instance ID (0 by default).
    #[serde(rename = "instanceID")]
    pub instance_id: Option<f64>,
}

/// An aura (`applyAura`'s data).
#[derive(Debug, Default, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct Aura {
    pub spell_id: Option<f64>,
    pub name: Option<String>,
    pub icon: Option<Scalar>,
    /// Seconds.
    pub duration: Option<f64>,
    pub applications: Option<f64>,
    pub harmful: Option<bool>,
    pub source: Option<String>,
    pub dispel_name: Option<String>,
    pub stealable: Option<bool>,
    #[serde(flatten)]
    pub other: Rest,
}

/// The world profile schema (`schema/world-profile-0.1.json`).
pub fn schema_json() -> String {
    let mut schema = serde_json::to_value(schemars::schema_for!(WorldProfile))
        .expect("the world profile schema serializes");
    schema["$id"] = schema_url().into();
    let mut text = serde_json::to_string_pretty(&schema).expect("schema serializes");
    text.push('\n');
    text
}

/// The parts whose keys are IDs (whole numbers).
const ID_KEYS: &[(&str, &str)] = &[
    ("items", "item IDs"),
    ("spells", "spell IDs"),
    ("bags", "bag IDs"),
    ("actions", "action slots"),
    ("mounts", "mount IDs"),
    ("toys", "item IDs"),
];

/// Checks a world profile's text (`path` names it): E4400 when it is no
/// JSON object, E4401 when a part does not match the schema. The
/// character's level may be up to the highest max level of any flavor.
pub fn check(db: &mut SourceDb, path: &Path, text: String) -> (FileId, Diagnostics) {
    check_for(db, path, text, &[])
}

/// [`check`] for a package of `flavors` (flavor IDs; none: every flavor):
/// the character's level is 1 to the highest of their max levels.
pub fn check_for(
    db: &mut SourceDb,
    path: &Path,
    text: String,
    flavors: &[String],
) -> (FileId, Diagnostics) {
    let file = db.add(path, text);
    let text = db.get(file).text();
    let mut diagnostics = Diagnostics::new();
    let raw: Value = match serde_json::from_str(text) {
        Ok(value) => value,
        Err(error) => {
            diagnostics.push(json_syntax_error(
                db,
                file,
                &error,
                "E4400",
                "a world profile",
            ));
            return (file, diagnostics);
        }
    };
    if !raw.is_object() {
        diagnostics.push(
            Diagnostic::error("E4400", "a world profile is a JSON object")
                .with_label(Span::new(file, 0..text.len().min(1)), "expected `{`"),
        );
        return (file, diagnostics);
    }
    let mut deserializer = serde_json::Deserializer::from_str(text);
    let profile = match serde_path_to_error::deserialize::<_, WorldProfile>(&mut deserializer) {
        Ok(profile) => profile,
        Err(error) => {
            diagnostics.push(schema_error(text, file, &error, "E4401"));
            return (file, diagnostics);
        }
    };
    let mut wrong = |at: JsonPath, message: String, label: &str| {
        diagnostics.push(
            Diagnostic::error("E4401", message).with_label(span_of(text, file, &at, false), label),
        );
    };
    if let Some(problem) = profile.screen.as_ref().and_then(|s| s.ui_scale_problem()) {
        wrong(
            JsonPath::root().key("screen").key("uiScale"),
            problem,
            "not a UI scale",
        );
    }
    if let Some(level) = profile.character.as_ref().and_then(|c| c.level) {
        let at = JsonPath::root().key("character").key("level");
        let table = wowlua_flavors::table();
        let caps: Vec<(&str, u32)> = table
            .flavors
            .iter()
            .filter(|f| flavors.is_empty() || flavors.contains(&f.id))
            .map(|f| (f.id.as_str(), f.max_level))
            .collect();
        let highest = caps.iter().map(|(_, cap)| *cap).max().unwrap_or(u32::MAX);
        if level < 1.0 {
            wrong(
                at,
                format!("the character's level is at least 1, not {level}"),
                "below level 1",
            );
        } else if level > f64::from(highest) {
            let named: Vec<String> = caps.iter().map(|(id, cap)| format!("{id} {cap}")).collect();
            wrong(
                at,
                format!(
                    "the character's level {level} is above the flavors' max level ({})",
                    named.join(", ")
                ),
                "above the max level",
            );
        }
    }
    // A unit's `powers` are keyed by power tokens (decision T512).
    let mut with_powers: Vec<(JsonPath, &UnitData)> = Vec::new();
    if let Some(c) = &profile.character {
        with_powers.push((JsonPath::root().key("character"), c));
    }
    match &profile.units {
        Some(List::Items(units)) => {
            for (i, u) in units.iter().enumerate() {
                with_powers.push((JsonPath::root().key("units").index(i), &u.data));
            }
        }
        Some(List::Keyed(units)) => {
            for (k, u) in units {
                with_powers.push((JsonPath::root().key("units").key(k), &u.data));
            }
        }
        None => {}
    }
    let table = wowlua_flavors::table();
    // A race of one faction decides the unit's (decision T513): a faction
    // given that is not it is an error.
    for (at, unit) in &with_powers {
        let Some(race) = unit
            .race
            .as_deref()
            .and_then(|r| table.flavors.iter().find_map(|f| f.race(r)))
        else {
            continue;
        };
        if race.is_either() {
            continue;
        }
        let given = match (&unit.faction, unit.other.get("factionGroup")) {
            (Some(f), _) => Some(("faction", f.as_str())),
            (None, Some(Value::String(f))) => Some(("factionGroup", f.as_str())),
            _ => None,
        };
        if let Some((key, faction)) = given
            && wowlua_flavors::Faction::parse(faction) != Some(race.faction)
        {
            wrong(
                at.clone().key(key),
                format!(
                    "the race `{}` is of the {}: its faction follows its race, not `{faction}`",
                    race.file, race.faction
                ),
                "not the race's faction",
            );
        }
    }
    // A race can be only the classes its flavor's game pairs it with
    // (decision T513): in none of the package's flavors that have the
    // race is an error.
    for (at, unit) in &with_powers {
        let (Some(race), Some(class)) = (unit.race.as_deref(), unit.class.as_deref()) else {
            continue;
        };
        let faction = unit
            .faction
            .as_deref()
            .or_else(|| match unit.other.get("factionGroup") {
                Some(Value::String(f)) => Some(f.as_str()),
                _ => None,
            })
            .and_then(wowlua_flavors::Faction::parse);
        let having: Vec<&wowlua_flavors::Flavor> = table
            .flavors
            .iter()
            .filter(|f| (flavors.is_empty() || flavors.contains(&f.id)) && f.has_race(race))
            .collect();
        if having.is_empty() || having.iter().any(|f| f.allows_for(race, faction, class)) {
            continue;
        }
        let named: Vec<String> = having
            .iter()
            .map(|f| {
                format!(
                    "{}: {}",
                    f.id,
                    f.classes_for(race, faction)
                        .iter()
                        .map(|c| format!("`{c}`"))
                        .collect::<Vec<_>>()
                        .join(", ")
                )
            })
            .collect();
        let who = match faction {
            Some(f)
                if having
                    .iter()
                    .all(|fl| fl.race(race).is_some_and(|r| r.is_either())) =>
            {
                format!("a `{race}` of the {f}")
            }
            _ => format!("a `{race}`"),
        };
        wrong(
            at.clone().key("class"),
            format!(
                "{who} can't be a `{class}` (its classes: {})",
                named.join("; ")
            ),
            "not a class of the race",
        );
    }
    for (at, unit) in with_powers {
        for token in unit.powers.iter().flat_map(|p| p.keys()) {
            if table.power_type(token).is_none() {
                wrong(
                    at.clone().key("powers").key(token),
                    format!(
                        "`powers` is keyed by power tokens (`MANA`, `RAGE`, `ENERGY`, `COMBO_POINTS`, …), not `{token}`"
                    ),
                    "not a power token",
                );
            }
        }
    }
    for (part, what) in ID_KEYS {
        let Some(map) = raw.get(*part).and_then(Value::as_object) else {
            continue;
        };
        for key in map.keys() {
            if key.parse::<i64>().is_err() {
                let at = JsonPath::root().key(part).key(key);
                diagnostics.push(
                    Diagnostic::error("E4401", format!("`{part}` is keyed by {what}, not `{key}`"))
                        .with_label(span_of(text, file, &at, false), "not a whole number"),
                );
            }
        }
    }
    (file, diagnostics)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn codes(text: &str) -> Vec<(String, String)> {
        let mut db = SourceDb::new();
        let (_, found) = check(
            &mut db,
            Path::new("tests/profiles/p.json"),
            text.to_string(),
        );
        found
            .into_vec()
            .into_iter()
            .map(|d| (d.code.unwrap_or_default().to_string(), d.message))
            .collect()
    }

    #[test]
    fn the_spec_s_example_is_a_profile() {
        let example = r#"{
  "character": { "name": "Thrall", "realm": "Realm", "class": "SHAMAN", "race": "Orc", "level": 60 },
  "money": 123456,
  "items": { "19019": { "name": "Thunderfury, Blessed Blade of the Windseeker", "quality": 5 } },
  "spells": { "403": { "name": "Lightning Bolt" } },
  "trained": [403],
  "equipment": { "MainHandSlot": 19019 },
  "bags": { "0": { "slots": 16, "items": { "1": { "id": 159, "count": 5 } } } },
  "position": { "mapID": 1453, "zone": "Stormwind City", "subZone": "Trade District", "x": 0.5, "y": 0.6 },
  "auras": [ { "spellId": 546, "name": "Water Walking", "duration": 600 } ],
  "units": [ { "name": "Hogger", "level": 11, "reaction": "hostile", "distance": 12, "target": true },
             { "name": "Jaina", "lastName": "Proudmoore", "player": true, "class": "MAGE", "race": "Human", "level": 60 } ],
  "cvars": { "nameplateShowEnemies": "1" },
  "bindings": { "SHIFT-1": "ACTIONBUTTON1" },
  "mounts": { "125": { "name": "Riding Turtle", "spellID": 30174, "icon": 132199, "favorite": true } },
  "toys": { "54452": { "favorite": true } },
  "appearances": { "collected": [21598], "favorites": [11257] },
  "savedVariables": { "MyAddonDB": { "logins": 41 } }
}"#;
        assert_eq!(codes(example), Vec::<(String, String)>::new());
    }

    #[test]
    fn profiles_accept_numeric_renown_by_faction() {
        let text = r#"{ "renown": { "2802": 16, "2507": 0 } }"#;
        assert!(codes(text).is_empty());
        let profile: WorldProfile = serde_json::from_str(text).unwrap();
        let renown = profile.renown.unwrap();
        assert_eq!(renown["2802"], 16.0);
        assert_eq!(renown["2507"], 0.0);
        assert_eq!(
            codes(r#"{ "renown": { "2802": "sixteen" } }"#)[0].0,
            "E4401"
        );
    }

    #[test]
    fn mistakes_are_located() {
        let found = codes(r#"{ "monies": 5 }"#);
        assert_eq!(found[0].0, "E4401");
        assert!(found[0].1.contains("unknown key `monies`"), "{found:?}");
        assert_eq!(codes(r#"{ "money": "lots" }"#)[0].0, "E4401");
        assert_eq!(
            codes(r#"{ "units": [ { "group": "guild" } ] }"#)[0].0,
            "E4401"
        );
        assert_eq!(codes(r#"{ "items": { "sword": {} } }"#)[0].0, "E4401");
        assert_eq!(codes("{ \"money\": 5, }")[0].0, "E4400");
        assert_eq!(codes("[1]")[0].0, "E4400");
    }

    /// Every profile of the repository: the recorded fresh character, the
    /// examples' and the client's snapshots.
    #[test]
    fn the_repository_s_profiles_are_profiles() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        let mut files = vec![root.join("crates/wowlua-engine/data/camelot-default-character.json")];
        for example in ["typed-demo", "widget-demo"] {
            let dir = root.join("examples").join(example).join(PROFILES_DIR);
            #[allow(clippy::disallowed_methods)]
            for e in std::fs::read_dir(dir).unwrap().flatten() {
                files.push(e.path());
            }
        }
        // the client's snapshots (`/stdtest snapshot`), as `xtask profile`
        // writes them
        fn snapshots(dir: &Path, out: &mut Vec<std::path::PathBuf>) {
            #[allow(clippy::disallowed_methods)]
            for e in std::fs::read_dir(dir).unwrap().flatten() {
                let p = e.path();
                if p.is_dir() {
                    snapshots(&p, out);
                } else if p.extension().is_some_and(|x| x == "json")
                    && std::fs::read_to_string(&p).is_ok_and(|t| t.contains("\"character\""))
                {
                    out.push(p);
                }
            }
        }
        snapshots(&root.join("crates/wowlua-engine/tests/client"), &mut files);
        assert!(files.len() > 50, "{}", files.len());
        for file in files {
            #[allow(clippy::disallowed_methods)]
            let text = std::fs::read_to_string(&file).unwrap();
            let mut db = SourceDb::new();
            let (_, found) = check(&mut db, &file, text);
            let found: Vec<String> = found.into_vec().into_iter().map(|d| d.message).collect();
            assert!(found.is_empty(), "{}: {found:?}", file.display());
        }
    }

    /// A profile's `screen.uiScale` is a value of the game's UI Scale
    /// slider; the character's level is within the flavors' max level; a
    /// unit's may be -1 (a boss's ??).
    #[test]
    fn ui_scale_and_levels() {
        assert_eq!(
            codes(r#"{ "screen": { "width": 1920, "height": 1080, "uiScale": 0.8 } }"#),
            Vec::<(String, String)>::new()
        );
        let found = codes(r#"{ "screen": { "width": 1920, "height": 1080, "uiScale": 0.5 } }"#);
        assert_eq!(found[0].0, "E4401");
        assert!(found[0].1.contains("UI Scale slider"), "{found:?}");
        assert_eq!(
            codes(
                r#"{ "units": [ { "name": "Onyxia", "level": -1, "classification": "worldboss" } ] }"#
            ),
            Vec::<(String, String)>::new()
        );
        assert!(codes(r#"{ "character": { "level": 90 } }"#).is_empty());
        assert_eq!(codes(r#"{ "character": { "level": 91 } }"#)[0].0, "E4401");
        assert_eq!(codes(r#"{ "character": { "level": -1 } }"#)[0].0, "E4401");
        let camelot = |text: &str| {
            let mut db = SourceDb::new();
            let (_, found) = check_for(
                &mut db,
                Path::new("tests/profiles/p.json"),
                text.to_string(),
                &["camelot".to_string()],
            );
            found
                .into_vec()
                .into_iter()
                .map(|d| d.message)
                .collect::<Vec<_>>()
        };
        assert!(camelot(r#"{ "character": { "level": 60 } }"#).is_empty());
        assert_eq!(
            camelot(r#"{ "character": { "level": 70 } }"#),
            ["the character's level 70 is above the flavors' max level (camelot 60)"]
        );
    }

    /// `position.world` and the channels' fields are typed as the engine
    /// reads them.
    #[test]
    fn powers_are_keyed_by_token_with_current_and_max() {
        let good = r#"{ "character": { "class": "DRUID", "powerType": 0,
            "powers": { "MANA": { "current": 300, "max": 900 }, "RAGE": { "current": 20 } } },
            "units": [ { "name": "Rogue", "powers": { "COMBO_POINTS": { "current": 3, "max": 5 } } } ] }"#;
        assert!(codes(good).is_empty(), "{:?}", codes(good));
        for wrong in [
            r#"{ "character": { "powers": { "MAGIC": { "current": 1 } } } }"#,
            r#"{ "units": [ { "powers": { "mana": { "current": 1 } } } ] }"#,
            r#"{ "character": { "powers": { "MANA": { "now": 1 } } } }"#,
            r#"{ "character": { "powers": { "MANA": 5 } } }"#,
        ] {
            assert_eq!(codes(wrong)[0].0, "E4401", "{wrong}");
        }
        let found = codes(r#"{ "character": { "powers": { "MAGIC": {} } } }"#);
        assert!(found[0].1.contains("not `MAGIC`"), "{found:?}");
    }

    #[test]
    fn world_positions_and_channels_are_typed() {
        assert!(
            codes(
                r#"{ "position": { "world": { "y": -8913.2, "x": 554.6, "z": 93.1, "instanceID": 0 } },
                     "channels": { "joined": [ { "name": "General", "zoneChannelID": 1 } ], "groupChannels": 2, "regionalService": true } }"#
            )
            .is_empty()
        );
        for wrong in [
            r#"{ "position": { "world": [1, 2] } }"#,
            r#"{ "position": { "world": { "y": 1 } } }"#,
            r#"{ "channels": { "groupChannels": "2" } }"#,
            r#"{ "channels": { "regionalService": 1 } }"#,
            r#"{ "channels": { "joined": [ { "fullName": "General" } ] } }"#,
        ] {
            assert_eq!(codes(wrong)[0].0, "E4401", "{wrong}");
        }
    }

    /// A race of one faction decides it (decision T513): another faction
    /// is E4401; a race of either takes either, a creature any.
    #[test]
    fn a_faction_follows_a_fixed_race() {
        for good in [
            r#"{ "character": { "race": "Orc", "faction": "Horde" } }"#,
            r#"{ "character": { "race": "Orc" } }"#,
            r#"{ "character": { "race": "Skyborne", "faction": "Alliance" } }"#,
            r#"{ "character": { "race": "Pandaren", "faction": "Horde" } }"#,
            r#"{ "units": [ { "name": "Guard", "faction": "Alliance" } ] }"#,
            r#"{ "units": [ { "name": "Jaina", "player": true, "race": "Human", "factionGroup": "Alliance" } ] }"#,
        ] {
            assert!(codes(good).is_empty(), "{good}: {:?}", codes(good));
        }
        let found = codes(r#"{ "character": { "race": "Orc", "faction": "Alliance" } }"#);
        assert_eq!(found.len(), 1, "{found:?}");
        assert_eq!(found[0].0, "E4401");
        assert_eq!(
            found[0].1,
            "the race `Orc` is of the Horde: its faction follows its race, not `Alliance`"
        );
        let found = codes(
            r#"{ "units": [ { "name": "Thrall", "race": "Orc", "factionGroup": "Alliance" } ] }"#,
        );
        assert_eq!(found[0].0, "E4401", "{found:?}");
        assert_eq!(
            codes(r#"{ "character": { "race": "Human", "faction": "Neutral" } }"#)[0].0,
            "E4401"
        );
    }

    /// A race can be only the classes its flavor's game pairs it with
    /// (decision T513; the clients' `CharBaseInfo`).
    #[test]
    fn a_race_can_be_only_its_classes() {
        let for_flavor = |flavor: &str, text: &str| -> Vec<String> {
            let mut db = SourceDb::new();
            let (_, found) = check_for(
                &mut db,
                Path::new("tests/profiles/p.json"),
                text.to_string(),
                &[flavor.to_string()],
            );
            found.into_vec().into_iter().map(|d| d.message).collect()
        };
        let orc_priest = r#"{ "character": { "race": "Orc", "class": "PRIEST" } }"#;
        assert_eq!(
            for_flavor("vanilla", orc_priest),
            [
                "a `Orc` can't be a `PRIEST` (its classes: vanilla: `WARRIOR`, `SHAMAN`, `ROGUE`, `WARLOCK`, `HUNTER`)"
            ]
        );
        // Nor MoP Classic's, though they can be mages there; a package of
        // every flavor has Retail, whose pairs are not read (any class).
        assert_eq!(for_flavor("mists", orc_priest).len(), 1);
        assert!(
            for_flavor(
                "mists",
                r#"{ "character": { "race": "Orc", "class": "MAGE" } }"#
            )
            .is_empty()
        );
        assert!(codes(orc_priest).is_empty());
        // WoW Forever's own pairs: an Undead paladin; the Skyborne by
        // faction.
        assert!(
            for_flavor(
                "camelot",
                r#"{ "character": { "race": "Scourge", "class": "PALADIN" } }"#
            )
            .is_empty()
        );
        assert!(
            for_flavor(
                "camelot",
                r#"{ "character": { "race": "Skyborne", "faction": "Horde", "class": "SHAMAN" } }"#
            )
            .is_empty()
        );
        assert_eq!(
            for_flavor(
                "camelot",
                r#"{ "character": { "race": "Skyborne", "faction": "Alliance", "class": "SHAMAN" } }"#
            ),
            [
                "a `Skyborne` of the Alliance can't be a `SHAMAN` (its classes: camelot: `WARRIOR`, `DRUID`, `ROGUE`, `MAGE`, `HUNTER`)"
            ]
        );
        // A Blood Elf paladin from TBC on; a unit's pair too.
        assert!(
            for_flavor(
                "tbc",
                r#"{ "units": [ { "name": "Liadrin", "player": true, "race": "BloodElf", "class": "PALADIN" } ] }"#
            )
            .is_empty()
        );
        assert_eq!(
            for_flavor(
                "tbc",
                r#"{ "units": [ { "name": "X", "race": "Tauren", "class": "MAGE" } ] }"#
            )
            .len(),
            1
        );
    }

    #[test]
    fn a_part_keeps_fields_the_schema_does_not_name() {
        assert_eq!(
            codes(r#"{ "character": { "name": "A", "honorLevel": 3 } }"#),
            Vec::<(String, String)>::new()
        );
    }

    /// The built-in profiles (decision T533) are profiles the check finds
    /// nothing in: Northshire's test NPCs over camelot's fresh character.
    #[test]
    fn the_built_in_profiles_are_sound() {
        for name in BUILT_IN {
            let text = built_in("camelot", name).expect("camelot has it");
            let mut db = SourceDb::new();
            let (_, found) = check_for(
                &mut db,
                Path::new("northshire.json"),
                text.clone(),
                &["camelot".to_string()],
            );
            let found: Vec<String> = found.into_vec().into_iter().map(|d| d.message).collect();
            assert!(found.is_empty(), "{name}: {found:?}");
            let json: Value = serde_json::from_str(&text).expect("JSON");
            // (the fresh character's parts, and its own)
            assert_eq!(json["character"]["class"], "WARRIOR");
            assert!(json["units"].as_array().is_some_and(|u| u.len() > 20));
            // (an object part's entries over the fresh character's: its
            // CVars kept, the names of the NPCs met shown)
            assert!(json["cvars"].as_object().is_some_and(|c| c.len() > 100));
            assert_eq!(json["cvars"]["UnitNameInteractiveNPC"], "1");
        }
        assert!(built_in("vanilla", "northshire").is_none());
        assert!(built_in("camelot", "nowhere").is_none());
    }
}
