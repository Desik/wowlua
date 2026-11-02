//! The flavor table (spec §9.4.1).
//!
//! A *flavor* is a WoW ruleset, identified by Blizzard's game type
//! (`camelot`, `standard`, `vanilla`, `tbc`, `mists`). A *client* is one installable
//! game client that runs a flavor (Retail, Retail PTR, WoW Forever, …). The
//! table records each flavor's display name and how far wowlua supports it,
//! interface numbers, TOC suffixes, install folders and which TOC features
//! each client supports. It is data shipped with the compiler; the version
//! in use is pinned in `wowlua.lock`.
//!
//! It is the one list of flavors every tool reads (the CLI, Studio, the
//! registry): [`FlavorTable::flavors`] is in the order pickers show them,
//! the supported flavors first.
//!
//! It also records what each flavor's game has (decision T512): its
//! classes ([`Flavor::classes`], each with the powers it has, the first
//! the one it shows), its races ([`Flavor::races`], each with its faction:
//! decision T513) and its powers' defaults ([`Flavor::power_defaults`]),
//! and the power types' tokens and `Enum.PowerType` values
//! ([`FlavorTable::power_types`]). Each flavor's racial spells, its
//! client's race records with the classes each race can be
//! ([`Flavor::racials`], [`Flavor::race_records`], [`Flavor::allows`]) and
//! its classes' base mana ([`Flavor::base_mana`]) are read from the game's
//! tables by `cargo xtask flavor-data` into `data/races.txt` and
//! `data/base-mana.txt`.

use serde::Deserialize;
use std::collections::BTreeMap;
use std::fmt;
use std::sync::LazyLock;

static TABLE: LazyLock<FlavorTable> = LazyLock::new(|| {
    serde_json::from_str(include_str!("../data/flavors.json"))
        .expect("the embedded flavor table is valid")
});

/// The flavor table shipped with this compiler.
pub fn table() -> &'static FlavorTable {
    &TABLE
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub struct FlavorTable {
    pub version: String,
    /// Every flavor wowlua knows, in the order a picker lists them: the
    /// supported ones first, then those coming soon.
    pub flavors: Vec<Flavor>,
    pub clients: Vec<Client>,
    legacy_toc_suffixes: BTreeMap<String, String>,
    /// APIs that call a passed function later (deferred) or right away
    /// (immediate), for load-order analysis (spec §5.4.3).
    #[serde(default)]
    pub load_order_markers: LoadOrderMarkers,
    /// The power types by token (`MANA`, `RAGE`, …: `UnitPowerType`'s
    /// second result, the token of `UNIT_POWER_UPDATE`) and
    /// `Enum.PowerType` value, the same in every flavor.
    #[serde(default)]
    pub power_types: Vec<PowerType>,
}

/// A power type: its token and its `Enum.PowerType` value.
#[derive(Clone, Debug, PartialEq, Eq, Deserialize)]
pub struct PowerType {
    pub token: String,
    pub id: u32,
}

/// A class of a flavor: its file name (`UnitClass`'s second result:
/// `DRUID`) and the powers it has by token, the first the one it shows
/// unless a form or specialization shows another (`UnitPowerType`).
#[derive(Clone, Debug, PartialEq, Eq, Deserialize)]
pub struct Class {
    pub file: String,
    pub powers: Vec<String>,
    /// The power each specialization shows, by token, in specialization
    /// order (`GetSpecialization`'s 1, 2, …), where they differ (a shadow
    /// priest's insanity); empty when every one shows the first power.
    #[serde(default)]
    pub specs: Vec<String>,
}

impl Class {
    /// The power the class shows by default (`MANA` for a druid).
    pub fn shown_power(&self) -> Option<&str> {
        self.powers.first().map(String::as_str)
    }

    /// The power a specialization (`GetSpecialization`'s index, from 1)
    /// shows: its own where the table names one, else the class's first.
    pub fn shown_power_for_spec(&self, spec: usize) -> Option<&str> {
        spec.checked_sub(1)
            .and_then(|i| self.specs.get(i))
            .map(String::as_str)
            .or_else(|| self.shown_power())
    }
}

/// A faction: a race's (`Either` for one whose player picks it: the
/// Pandaren, the Skyborne) or a character's (`UnitFactionGroup`'s
/// `Alliance`, `Horde`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Faction {
    Alliance,
    Horde,
    Either,
}

impl Faction {
    /// The name as the client gives it (`UnitFactionGroup`: `Alliance`,
    /// `Horde`); `Either` for a race of either.
    pub fn name(self) -> &'static str {
        match self {
            Faction::Alliance => "Alliance",
            Faction::Horde => "Horde",
            Faction::Either => "Either",
        }
    }

    /// A character's faction by its name (`Alliance`, `horde`: any case);
    /// `Either` is no character's.
    pub fn parse(name: &str) -> Option<Faction> {
        if name.eq_ignore_ascii_case("Alliance") {
            Some(Faction::Alliance)
        } else if name.eq_ignore_ascii_case("Horde") {
            Some(Faction::Horde)
        } else {
            None
        }
    }
}

impl fmt::Display for Faction {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.name())
    }
}

/// A race of a flavor: its file name (`UnitRace`'s second result:
/// `Scourge`) and faction (`Either`: the player picks it).
#[derive(Clone, Debug, PartialEq, Eq, Deserialize)]
pub struct Race {
    pub file: String,
    pub faction: Faction,
}

impl Race {
    /// Whether the player picks the faction (the Pandaren, the Skyborne).
    pub fn is_either(&self) -> bool {
        self.faction == Faction::Either
    }
}

/// A race's record in the flavor's client (`ChrRaces`): its race ID and
/// name for a faction (`None`: neither, a Pandaren yet to choose one), and
/// the classes it can be (`CharBaseInfo`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RaceRecord {
    pub file: String,
    pub faction: Option<Faction>,
    pub id: u32,
    pub name: String,
    /// The class files it can be (empty: every class of the flavor).
    pub classes: Vec<String>,
}

impl RaceRecord {
    /// Whether it can be of this class (its file name, any case).
    pub fn allows(&self, class: &str) -> bool {
        self.classes.is_empty() || self.classes.iter().any(|c| c.eq_ignore_ascii_case(class))
    }
}

/// A racial spell: a spell of a race's racial skill line in the flavor's
/// client (`SkillLineAbility`), for the factions and classes it names.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Racial {
    /// The race's file name.
    pub race: String,
    /// The factions of the race it is for (empty: every one; the
    /// Skyborne's differ by faction; `None`: a character of none, a
    /// Pandaren yet to choose).
    pub factions: Vec<Option<Faction>>,
    /// The class files it is for (empty: every class).
    pub classes: Vec<String>,
    pub spell: u32,
    /// Its name in the client (enUS).
    pub name: String,
}

impl Racial {
    /// Whether a character of the race with this faction (`None`: none)
    /// and class (`None`: unknown, then only one for every class) has it.
    pub fn applies_to(&self, faction: Option<Faction>, class: Option<&str>) -> bool {
        let faction_ok = self.factions.is_empty() || self.factions.contains(&faction);
        let class_ok = self.classes.is_empty()
            || class.is_some_and(|c| self.classes.iter().any(|k| k.eq_ignore_ascii_case(c)));
        faction_ok && class_ok
    }
}

/// One flavor's section of `data/races.txt`.
struct FlavorRacials {
    build: String,
    races: Vec<RaceRecord>,
    spells: Vec<Racial>,
}

static RACIALS: LazyLock<BTreeMap<String, FlavorRacials>> =
    LazyLock::new(|| parse_racials(include_str!("../data/races.txt")));

fn racials_of(flavor: &str) -> Option<&'static FlavorRacials> {
    RACIALS.get(flavor)
}

/// One flavor's section of `data/base-mana.txt`: the classes' columns and
/// a row per level from 1.
struct BaseMana {
    classes: Vec<String>,
    levels: Vec<Vec<f64>>,
}

static BASE_MANA: LazyLock<BTreeMap<String, BaseMana>> =
    LazyLock::new(|| parse_base_mana(include_str!("../data/base-mana.txt")));

/// Parses `data/base-mana.txt` (written by `cargo xtask flavor-data`): a
/// section per flavor, `[flavor] build`, a `level` line naming the
/// classes, then a row per level.
fn parse_base_mana(text: &str) -> BTreeMap<String, BaseMana> {
    let mut out: BTreeMap<String, BaseMana> = BTreeMap::new();
    let mut current: Option<String> = None;
    for line in text.lines() {
        if line.starts_with('#') || line.trim().is_empty() {
            continue;
        }
        if let Some(rest) = line.strip_prefix('[') {
            let id = rest.split(']').next().unwrap_or(rest).to_string();
            out.insert(
                id.clone(),
                BaseMana {
                    classes: Vec::new(),
                    levels: Vec::new(),
                },
            );
            current = Some(id);
            continue;
        }
        let Some(section) = current.as_ref().and_then(|id| out.get_mut(id)) else {
            continue;
        };
        let mut cols = line.split('\t');
        match cols.next() {
            Some("level") => section.classes = cols.map(str::to_string).collect(),
            Some(_) => section
                .levels
                .push(cols.map(|v| v.parse().unwrap_or(0.0)).collect()),
            None => {}
        }
    }
    out
}

/// Parses `data/races.txt` (written by `cargo xtask flavor-data`): a
/// section per flavor, `[flavor] build`, then its `race` and `spell` lines
/// (tab-separated; `-` every faction or class; `Neutral` no faction).
fn parse_racials(text: &str) -> BTreeMap<String, FlavorRacials> {
    let mut out: BTreeMap<String, FlavorRacials> = BTreeMap::new();
    let mut current: Option<String> = None;
    let list = |text: &str| -> Vec<String> {
        if text == "-" {
            Vec::new()
        } else {
            text.split(',').map(str::to_string).collect()
        }
    };
    for line in text.lines() {
        if line.starts_with('#') || line.trim().is_empty() {
            continue;
        }
        if let Some(rest) = line.strip_prefix('[') {
            let (id, build) = rest.split_once(']').unwrap_or((rest, ""));
            out.insert(
                id.to_string(),
                FlavorRacials {
                    build: build.trim().to_string(),
                    races: Vec::new(),
                    spells: Vec::new(),
                },
            );
            current = Some(id.to_string());
            continue;
        }
        let Some(section) = current.as_ref().and_then(|id| out.get_mut(id)) else {
            continue;
        };
        let cols: Vec<&str> = line.split('\t').collect();
        match cols.as_slice() {
            ["race", file, faction, id, name, classes] => section.races.push(RaceRecord {
                file: file.to_string(),
                faction: Faction::parse(faction),
                id: id.parse().unwrap_or(0),
                name: name.to_string(),
                classes: list(classes),
            }),
            ["spell", race, factions, classes, spell, name] => section.spells.push(Racial {
                race: race.to_string(),
                factions: list(factions).iter().map(|f| Faction::parse(f)).collect(),
                classes: list(classes),
                spell: spell.parse().unwrap_or(0),
                name: name.to_string(),
            }),
            _ => {}
        }
    }
    out
}

/// A power's amount: how much there is (`UnitPower`) of how much at most
/// (`UnitPowerMax`).
#[derive(Clone, Copy, Debug, PartialEq, Deserialize)]
pub struct PowerAmount {
    pub current: f64,
    pub max: f64,
}

/// Load-order markers (decision M2: curated in the flavor table until the
/// declaration files mark parameters `@deferred`).
#[derive(Clone, Debug, Default, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub struct LoadOrderMarkers {
    /// Global functions by path (`C_Timer.After`).
    pub deferred_functions: Vec<String>,
    /// Method names on any object (`SetScript`).
    pub deferred_methods: Vec<String>,
    pub immediate_functions: Vec<String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Family {
    Mainline,
    Classic,
}

impl Family {
    /// The family name as used in TOC load directives (`mainline`, `classic`).
    pub fn name(self) -> &'static str {
        match self {
            Family::Mainline => "mainline",
            Family::Classic => "classic",
        }
    }

    /// The family TOC suffix (`_Mainline`, `_Classic`).
    pub fn suffix(self) -> &'static str {
        match self {
            Family::Mainline => "_Mainline",
            Family::Classic => "_Classic",
        }
    }
}

impl fmt::Display for Family {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.name())
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum AliasKind {
    /// Names a ruleset; permanent.
    Ruleset,
    /// Names a client; may move to another ruleset when the table is updated.
    Client,
}

/// How far wowlua supports a flavor.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Support {
    /// The toolchain runs it fully: the emulated client, the debugger,
    /// Blizzard's UI.
    Supported,
    /// Named and checked (manifests, types, TOCs), but not run yet.
    ComingSoon,
}

#[derive(Debug, Deserialize)]
pub struct Alias {
    pub name: String,
    pub kind: AliasKind,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub struct Flavor {
    pub id: String,
    /// The name people know the game by (`Retail`, `WoW Forever`).
    pub name: String,
    pub support: Support,
    /// The highest level a character reaches in this flavor
    /// (`GetMaxPlayerLevel`): the bound of a level picker and of a world
    /// profile's character.
    pub max_level: u32,
    pub family: Family,
    pub aliases: Vec<Alias>,
    pub project_id: u32,
    /// Current interface numbers of the flavor's live client(s).
    pub interface: Vec<u32>,
    /// Inclusive range of interface numbers that belong to this flavor.
    pub interface_range: [u32; 2],
    pub game_suffixes: Vec<String>,
    /// The classes a character can be, in the client's order
    /// (`CLASS_SORT_ORDER`), each with its powers.
    #[serde(default)]
    pub classes: Vec<Class>,
    /// The races a character can be, by file name (`UnitRace`'s second
    /// result: `Scourge`, `NightElf`), in race ID order, each with its
    /// faction (decision T513).
    #[serde(default)]
    pub races: Vec<Race>,
    /// What a unit has of each power, by token, when nothing says (the
    /// emulated client's units; a slider's start).
    #[serde(default)]
    pub power_defaults: BTreeMap<String, PowerAmount>,
}

impl PartialEq for Flavor {
    fn eq(&self, other: &Self) -> bool {
        self.id == other.id
    }
}

impl Eq for Flavor {}

impl Flavor {
    pub fn is_supported(&self) -> bool {
        self.support == Support::Supported
    }

    pub fn contains_interface(&self, interface: u32) -> bool {
        (self.interface_range[0]..=self.interface_range[1]).contains(&interface)
    }

    /// The class with this file name (any case: `druid`, `DRUID`).
    pub fn class(&self, file: &str) -> Option<&Class> {
        self.classes
            .iter()
            .find(|c| c.file.eq_ignore_ascii_case(file))
    }

    /// Whether a character can be of this race (its file name, `Scourge`).
    pub fn has_race(&self, race: &str) -> bool {
        self.race(race).is_some()
    }

    /// The race with this file name (`Scourge`).
    pub fn race(&self, file: &str) -> Option<&Race> {
        self.races.iter().find(|r| r.file == file)
    }

    /// The races' file names, in the table's order.
    pub fn race_files(&self) -> impl Iterator<Item = &str> {
        self.races.iter().map(|r| r.file.as_str())
    }

    /// The faction a character of this race has: the race's own, or for a
    /// race of either faction the one `chosen` (`None`: none chosen, or a
    /// race the flavor lacks).
    pub fn faction_of(&self, race: &str, chosen: Option<Faction>) -> Option<Faction> {
        match self.race(race)?.faction {
            Faction::Either => chosen.filter(|f| *f != Faction::Either),
            fixed => Some(fixed),
        }
    }

    /// The client's records of a race: its race IDs and names by faction
    /// (the Skyborne's two), as `cargo xtask flavor-data` read them from the
    /// flavor's client; none where it was not read.
    pub fn race_records<'a>(&'a self, race: &'a str) -> impl Iterator<Item = &'a RaceRecord> {
        racials_of(&self.id)
            .map(|r| r.races.as_slice())
            .unwrap_or_default()
            .iter()
            .filter(move |r| r.file == race)
    }

    /// The client's record of a race for a faction (`None`: the race's
    /// first record).
    pub fn race_record<'a>(
        &'a self,
        race: &'a str,
        faction: Option<Faction>,
    ) -> Option<&'a RaceRecord> {
        let mut records = self.race_records(race);
        match faction {
            Some(f) => records.find(|r| r.faction == Some(f)),
            None => records.next(),
        }
    }

    /// The racial spells a character of `race` knows without learning
    /// them (decision T513), for its faction (`None`: none, then those of
    /// every faction of the race and a factionless one's) and class file
    /// (`None`: only those of every class), in the data's order. None
    /// where the flavor's client was not read.
    pub fn racials<'a>(
        &'a self,
        race: &'a str,
        faction: Option<Faction>,
        class: Option<&'a str>,
    ) -> impl Iterator<Item = &'a Racial> + 'a {
        self.racial_spells()
            .iter()
            .filter(move |r| r.race == race && r.applies_to(faction, class))
    }

    /// Every racial spell of the flavor's races (`cargo xtask flavor-data`).
    pub fn racial_spells(&self) -> &[Racial] {
        racials_of(&self.id)
            .map(|r| r.spells.as_slice())
            .unwrap_or_default()
    }

    /// The client build the races and racial spells were read from
    /// (`wowf 1.60.1.70707`), if any.
    pub fn racials_build(&self) -> Option<&str> {
        racials_of(&self.id).map(|r| r.build.as_str())
    }

    /// The classes a character of `race` can be (decision T513), of the
    /// race's records of `faction` (`None`: of any), in the flavor's class
    /// order: the client's pairs (`CharBaseInfo`), or every class where
    /// the flavor's client was not read; none for a race the flavor lacks.
    pub fn classes_for(&self, race: &str, faction: Option<Faction>) -> Vec<&str> {
        if !self.has_race(race) {
            return Vec::new();
        }
        let records: Vec<&RaceRecord> = self
            .race_records(race)
            .filter(|r| faction.is_none() || r.faction == faction)
            .collect();
        self.classes
            .iter()
            .map(|c| c.file.as_str())
            .filter(|c| {
                if self.racials_build().is_none() {
                    return true;
                }
                records.iter().any(|r| r.allows(c))
            })
            .collect()
    }

    /// Whether a character of `race` can be of `class` (any case) in this
    /// flavor, of any faction (decision T513: a vanilla Orc is no priest).
    pub fn allows(&self, race: &str, class: &str) -> bool {
        self.allows_for(race, None, class)
    }

    /// [`Flavor::allows`] for a faction (`None`: any): a High Order
    /// Skyborne (Alliance) is no shaman, a Windshaper (Horde) no mage.
    pub fn allows_for(&self, race: &str, faction: Option<Faction>, class: &str) -> bool {
        self.classes_for(race, faction)
            .iter()
            .any(|c| c.eq_ignore_ascii_case(class))
    }

    /// A class's base mana at a level (decision T512: the client's
    /// `GameTables/BaseMp.txt`; a level past the table's its last), where
    /// the flavor's client was read and the class has some.
    pub fn base_mana(&self, class: &str, level: u32) -> Option<f64> {
        let table = BASE_MANA.get(&self.id)?;
        let column = table
            .classes
            .iter()
            .position(|c| c.eq_ignore_ascii_case(class))?;
        let row = (level.max(1) as usize - 1).min(table.levels.len().checked_sub(1)?);
        table.levels[row]
            .get(column)
            .copied()
            .filter(|mana| *mana > 0.0)
    }

    /// What a unit has of a power (its token) when nothing says.
    pub fn power_default(&self, token: &str) -> Option<PowerAmount> {
        self.power_defaults.get(token).copied()
    }
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub struct ClientFeatures {
    pub comma_interfaces: bool,
    pub inline_file_directives: bool,
    pub inline_directive_lines: bool,
    pub allow_load_text_locale: bool,
    /// `[Game]`, `[Family]` and `[TextLocale]` in TOC file lines.
    #[serde(default)]
    pub file_variables: bool,
    /// The `default` key attribute on addon keybindings (W8354 where not).
    #[serde(default)]
    pub binding_defaults: bool,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub struct Client {
    pub name: String,
    pub flavor: String,
    pub version: String,
    pub interface: u32,
    /// Folders inside the WoW installation (`_retail_`, `_classic_era_`, …).
    pub folders: Vec<String>,
    /// TOC suffixes this client looks for, in lookup order (before the plain name).
    pub suffixes: Vec<String>,
    pub features: ClientFeatures,
    /// Game-type names recognized in `[AllowLoadGameType …]`, besides the
    /// family names, which every client with inline directives recognizes.
    pub game_types: Vec<String>,
}

impl Client {
    /// Whether `name` (a game type or a family name) is recognized in load
    /// directives by this client.
    pub fn recognizes_game_type(&self, name: &str) -> bool {
        self.features.inline_file_directives
            && (name == Family::Mainline.name()
                || name == Family::Classic.name()
                || self.game_types.iter().any(|g| g == name))
    }

    pub fn recognizes_suffix(&self, suffix: &str) -> bool {
        self.suffixes.iter().any(|s| s == suffix)
    }
}

/// What a TOC file-name suffix refers to.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SuffixMeaning<'a> {
    Flavor(&'a Flavor),
    Family(Family),
}

impl FlavorTable {
    pub fn flavor(&self, id: &str) -> Option<&Flavor> {
        self.flavors.iter().find(|f| f.id == id)
    }

    /// Resolves a flavor ID or alias (case-insensitive).
    pub fn resolve(&self, name: &str) -> Option<&Flavor> {
        let name = name.to_ascii_lowercase();
        self.flavors
            .iter()
            .find(|f| f.id == name || f.aliases.iter().any(|a| a.name == name))
    }

    /// The alias entry for `name`, if `name` is an alias rather than an ID.
    pub fn alias(&self, name: &str) -> Option<(&Flavor, &Alias)> {
        let name = name.to_ascii_lowercase();
        self.flavors
            .iter()
            .find_map(|f| f.aliases.iter().find(|a| a.name == name).map(|a| (f, a)))
    }

    /// The flavor an interface number belongs to.
    pub fn by_interface(&self, interface: u32) -> Option<&Flavor> {
        self.flavors
            .iter()
            .find(|f| f.contains_interface(interface))
    }

    /// A power type's `Enum.PowerType` value by its token (`RAGE`: 1).
    pub fn power_type(&self, token: &str) -> Option<u32> {
        self.power_types
            .iter()
            .find(|p| p.token == token)
            .map(|p| p.id)
    }

    /// A power type's token by its `Enum.PowerType` value (1: `RAGE`).
    pub fn power_token(&self, id: u32) -> Option<&str> {
        self.power_types
            .iter()
            .find(|p| p.id == id)
            .map(|p| p.token.as_str())
    }

    pub fn clients_of<'a>(&'a self, flavor: &'a str) -> impl Iterator<Item = &'a Client> + 'a {
        self.clients.iter().filter(move |c| c.flavor == flavor)
    }

    /// Interprets a TOC file-name suffix such as `_TBC`, `_Classic`, `-BCC`.
    pub fn suffix_meaning(&self, suffix: &str) -> Option<SuffixMeaning<'_>> {
        let lower = suffix.to_ascii_lowercase();
        if lower == Family::Mainline.suffix().to_ascii_lowercase() {
            return Some(SuffixMeaning::Family(Family::Mainline));
        }
        if lower == Family::Classic.suffix().to_ascii_lowercase() {
            return Some(SuffixMeaning::Family(Family::Classic));
        }
        if let Some(flavor) = self.flavors.iter().find(|f| {
            f.game_suffixes
                .iter()
                .any(|s| s.eq_ignore_ascii_case(suffix))
        }) {
            return Some(SuffixMeaning::Flavor(flavor));
        }
        if let Some(meaning) = self
            .legacy_toc_suffixes
            .iter()
            .find(|(k, _)| k.eq_ignore_ascii_case(suffix))
            .and_then(|(_, id)| self.flavor(id))
        {
            return Some(SuffixMeaning::Flavor(meaning));
        }
        // The client (and the packager) take `-` in place of `_`.
        let rest = suffix.strip_prefix('-')?;
        self.suffix_meaning(&format!("_{rest}"))
    }
}

/// Formats an interface number as a game version: `120100` → `12.1.0`.
pub fn interface_to_version(interface: u32) -> String {
    format!(
        "{}.{}.{}",
        interface / 10000,
        (interface / 100) % 100,
        interface % 100
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn table_is_consistent() {
        let t = table();
        assert!(!t.version.is_empty());
        for client in &t.clients {
            assert!(t.flavor(&client.flavor).is_some(), "{}", client.name);
            let flavor = t.flavor(&client.flavor).unwrap();
            assert!(
                flavor.contains_interface(client.interface),
                "{}",
                client.name
            );
        }
        for flavor in &t.flavors {
            assert!(t.clients_of(&flavor.id).next().is_some(), "{}", flavor.id);
            for interface in &flavor.interface {
                assert_eq!(t.by_interface(*interface).unwrap().id, flavor.id);
            }
        }
    }

    #[test]
    fn resolves_ids_and_aliases() {
        let t = table();
        assert_eq!(t.resolve("Retail").unwrap().id, "standard");
        assert_eq!(t.resolve("classic-era").unwrap().id, "vanilla");
        assert_eq!(t.resolve("anniversary").unwrap().id, "tbc");
        assert!(t.resolve("bogus").is_none());
        assert_eq!(t.alias("forever").unwrap().1.kind, AliasKind::Client);
    }

    #[test]
    fn maps_interfaces_and_suffixes() {
        let t = table();
        assert_eq!(t.by_interface(110200).unwrap().id, "standard");
        assert_eq!(t.by_interface(11507).unwrap().id, "vanilla");
        assert_eq!(t.by_interface(16001).unwrap().id, "camelot");
        assert_eq!(
            t.suffix_meaning("_Classic"),
            Some(SuffixMeaning::Family(Family::Classic))
        );
        assert!(
            matches!(t.suffix_meaning("-BCC"), Some(SuffixMeaning::Flavor(f)) if f.id == "tbc")
        );
        assert!(
            matches!(t.suffix_meaning("-Forever"), Some(SuffixMeaning::Flavor(f)) if f.id == "camelot")
        );
        assert!(
            matches!(t.suffix_meaning("_Mists"), Some(SuffixMeaning::Flavor(f)) if f.id == "mists")
        );
        assert_eq!(t.by_interface(50504).unwrap().id, "mists");
        // Wrath and Cataclysm are no flavors (decisions T507, T509).
        for suffix in ["_Cata", "_Wrath", "-WOTLKC"] {
            assert_eq!(t.suffix_meaning(suffix), None, "{suffix}");
        }
        assert!(t.by_interface(40402).is_none() && t.by_interface(38002).is_none());
        assert_eq!(interface_to_version(120100), "12.1.0");
        assert_eq!(interface_to_version(11509), "1.15.9");
    }

    #[test]
    fn classic_clients_do_not_recognize_standard() {
        let t = table();
        let era = t.clients.iter().find(|c| c.name == "classic-era").unwrap();
        assert!(era.recognizes_game_type("mainline"));
        assert!(era.recognizes_game_type("vanilla"));
        assert!(!era.recognizes_game_type("standard"));
        let mists = t
            .clients
            .iter()
            .find(|c| c.name == "mists-classic")
            .unwrap();
        assert!(mists.recognizes_game_type("classic"));
        assert!(mists.recognizes_game_type("mists"));
        assert!(mists.features.file_variables);
    }

    /// The flavors, their names and support (decisions T507, T509): camelot
    /// is supported and listed first; the others are coming soon.
    #[test]
    fn names_and_support_in_picker_order() {
        let t = table();
        let listed: Vec<(&str, &str, Support)> = t
            .flavors
            .iter()
            .map(|f| (f.id.as_str(), f.name.as_str(), f.support))
            .collect();
        assert_eq!(
            listed,
            [
                ("camelot", "WoW Forever", Support::Supported),
                ("standard", "Retail", Support::ComingSoon),
                ("vanilla", "Classic Era", Support::ComingSoon),
                ("tbc", "TBC Anniversary", Support::ComingSoon),
                ("mists", "MoP Classic", Support::ComingSoon),
            ]
        );
        assert!(t.flavor("camelot").unwrap().is_supported());
        for gone in ["cata", "wrath", "titan"] {
            assert!(t.resolve(gone).is_none(), "{gone}");
        }
        // Clients name only flavors of the table.
        for client in &t.clients {
            for game_type in &client.game_types {
                assert!(
                    t.flavor(game_type).is_some(),
                    "{}: {game_type}",
                    client.name
                );
            }
        }
    }

    /// The classes each flavor has, in `CLASS_SORT_ORDER` (Blizzard's UI of
    /// each), with their powers; every power has a token and a default
    /// (decision T512).
    #[test]
    fn classes_powers_and_races() {
        let t = table();
        let files = |id: &str| -> Vec<&str> {
            t.flavor(id)
                .unwrap()
                .classes
                .iter()
                .map(|c| c.file.as_str())
                .collect()
        };
        let classic = [
            "WARRIOR", "PALADIN", "PRIEST", "SHAMAN", "DRUID", "ROGUE", "MAGE", "WARLOCK", "HUNTER",
        ];
        for id in ["camelot", "vanilla", "tbc"] {
            assert_eq!(files(id), classic, "{id}");
        }
        assert_eq!(
            files("mists"),
            [
                "WARRIOR",
                "DEATHKNIGHT",
                "PALADIN",
                "MONK",
                "PRIEST",
                "SHAMAN",
                "DRUID",
                "ROGUE",
                "MAGE",
                "WARLOCK",
                "HUNTER"
            ]
        );
        assert_eq!(files("standard").len(), 13);
        let powers = |id: &str, class: &str| -> Vec<&str> {
            let class = t.flavor(id).unwrap().class(class).unwrap();
            class.powers.iter().map(String::as_str).collect()
        };
        // Recorded on camelot: a warrior's rage, a paladin's, a druid's and
        // a shaman's mana shown; a paladin has no other power.
        assert_eq!(powers("camelot", "WARRIOR"), ["RAGE"]);
        assert_eq!(powers("camelot", "paladin"), ["MANA"]);
        assert_eq!(
            powers("camelot", "DRUID"),
            ["MANA", "RAGE", "ENERGY", "COMBO_POINTS"]
        );
        assert_eq!(
            t.flavor("camelot")
                .unwrap()
                .class("DRUID")
                .unwrap()
                .shown_power(),
            Some("MANA")
        );
        // Hunters use mana until Cataclysm, focus from then on.
        assert_eq!(powers("vanilla", "HUNTER"), ["MANA"]);
        assert_eq!(powers("tbc", "HUNTER"), ["MANA"]);
        assert_eq!(powers("mists", "HUNTER"), ["FOCUS"]);
        assert_eq!(powers("standard", "HUNTER"), ["FOCUS"]);
        assert_eq!(powers("standard", "DEMONHUNTER"), ["FURY"]);
        assert_eq!(powers("mists", "PALADIN"), ["MANA", "HOLY_POWER"]);
        assert_eq!(powers("standard", "DEATHKNIGHT"), ["RUNIC_POWER", "RUNES"]);
        // A specialization's shown power: a shadow priest's insanity, a
        // mistweaver's mana; without a list, the class's first.
        let priest = t.flavor("standard").unwrap().class("PRIEST").unwrap();
        assert_eq!(priest.shown_power_for_spec(3), Some("INSANITY"));
        assert_eq!(priest.shown_power_for_spec(1), Some("MANA"));
        let monk = t.flavor("mists").unwrap().class("MONK").unwrap();
        assert_eq!(monk.shown_power_for_spec(2), Some("MANA"));
        let rogue = t.flavor("camelot").unwrap().class("ROGUE").unwrap();
        assert_eq!(rogue.shown_power_for_spec(2), Some("ENERGY"));
        for flavor in &t.flavors {
            for class in &flavor.classes {
                for shown in &class.specs {
                    assert!(class.powers.contains(shown), "{} {}", flavor.id, class.file);
                }
            }
        }
        assert!(t.flavor("vanilla").unwrap().class("MONK").is_none());
        for flavor in &t.flavors {
            assert!(!flavor.races.is_empty(), "{}", flavor.id);
            for class in &flavor.classes {
                assert!(!class.powers.is_empty(), "{} {}", flavor.id, class.file);
                for power in &class.powers {
                    assert!(t.power_type(power).is_some(), "{power}");
                    let d = flavor.power_default(power);
                    assert!(
                        d.is_some_and(|d| d.current <= d.max),
                        "{} {power}",
                        flavor.id
                    );
                }
            }
            for token in flavor.power_defaults.keys() {
                assert!(t.power_type(token).is_some(), "{token}");
            }
        }
        // The races by file name.
        let camelot = t.flavor("camelot").unwrap();
        assert!(camelot.has_race("Scourge") && camelot.has_race("Skyborne"));
        assert!(!camelot.has_race("BloodElf") && !camelot.has_race("Undead"));
        assert!(t.flavor("tbc").unwrap().has_race("Draenei"));
        assert!(t.flavor("mists").unwrap().has_race("Pandaren"));
        assert_eq!(t.flavor("vanilla").unwrap().races.len(), 8);
        // Enum.PowerType's values, both ways.
        assert_eq!(t.power_type("MANA"), Some(0));
        assert_eq!(t.power_type("ESSENCE"), Some(19));
        assert_eq!(t.power_token(13), Some("INSANITY"));
        assert_eq!(t.power_token(28), Some("SHADOW_ORBS"));
        assert_eq!(t.power_token(30), None);
        assert_eq!(t.power_types.len(), 30);
    }

    /// Each race's faction (decision T513): the client's records agree
    /// (`ChrRaces`, read by `cargo xtask flavor-data`): a race of one faction
    /// has records of it only, one of either both.
    #[test]
    fn races_have_their_factions() {
        let t = table();
        let alliance = [
            "Human",
            "Dwarf",
            "NightElf",
            "Gnome",
            "Draenei",
            "Worgen",
            "VoidElf",
            "LightforgedDraenei",
            "DarkIronDwarf",
            "KulTiran",
            "Mechagnome",
        ];
        let horde = [
            "Orc",
            "Scourge",
            "Tauren",
            "Troll",
            "BloodElf",
            "Goblin",
            "Nightborne",
            "HighmountainTauren",
            "MagharOrc",
            "ZandalariTroll",
            "Vulpera",
        ];
        let either = [
            "Pandaren",
            "Dracthyr",
            "EarthenDwarf",
            "Harronir",
            "Skyborne",
        ];
        for flavor in &t.flavors {
            for race in &flavor.races {
                let expected = if alliance.contains(&race.file.as_str()) {
                    Faction::Alliance
                } else if horde.contains(&race.file.as_str()) {
                    Faction::Horde
                } else {
                    assert!(either.contains(&race.file.as_str()), "{}", race.file);
                    Faction::Either
                };
                assert_eq!(race.faction, expected, "{} {}", flavor.id, race.file);
                let records: Vec<Option<Faction>> =
                    flavor.race_records(&race.file).map(|r| r.faction).collect();
                if flavor.racials_build().is_none() {
                    continue;
                }
                match race.faction {
                    Faction::Either => {
                        assert!(records.contains(&Some(Faction::Alliance)), "{}", race.file);
                        assert!(records.contains(&Some(Faction::Horde)), "{}", race.file);
                    }
                    fixed => assert_eq!(records, [Some(fixed)], "{} {}", flavor.id, race.file),
                }
            }
        }
        let camelot = t.flavor("camelot").unwrap();
        assert!(camelot.race("Skyborne").unwrap().is_either());
        assert_eq!(camelot.faction_of("Skyborne", None), None);
        assert_eq!(
            camelot.faction_of("Skyborne", Some(Faction::Alliance)),
            Some(Faction::Alliance)
        );
        assert_eq!(
            camelot.faction_of("Orc", Some(Faction::Alliance)),
            Some(Faction::Horde)
        );
        assert_eq!(camelot.faction_of("BloodElf", None), None);
        assert_eq!(Faction::parse("horde"), Some(Faction::Horde));
        assert_eq!(Faction::parse("Either"), None);
        assert_eq!(
            camelot.race_files().collect::<Vec<_>>(),
            [
                "Human", "Orc", "Dwarf", "NightElf", "Scourge", "Tauren", "Gnome", "Troll",
                "Skyborne"
            ]
        );
        // The Skyborne's two records (camelot's client).
        let high_order = camelot
            .race_record("Skyborne", Some(Faction::Alliance))
            .unwrap();
        assert_eq!(
            (high_order.id, high_order.name.as_str()),
            (95, "High Order Skyborne")
        );
        let windshaper = camelot
            .race_record("Skyborne", Some(Faction::Horde))
            .unwrap();
        assert_eq!(
            (windshaper.id, windshaper.name.as_str()),
            (96, "Windshaper Skyborne")
        );
    }

    /// The racial spells (decision T513), read from each flavor's client
    /// (`cargo xtask flavor-data`): every flavor but standard (no retail client
    /// was installed); each of its races has some; they name the flavor's
    /// races and classes.
    #[test]
    fn racial_spells() {
        let t = table();
        for flavor in &t.flavors {
            if flavor.id == "standard" {
                assert!(flavor.racials_build().is_none());
                continue;
            }
            assert!(flavor.racials_build().is_some(), "{}", flavor.id);
            for race in flavor.race_files() {
                assert!(
                    flavor.racial_spells().iter().any(|r| r.race == race),
                    "{} {race}",
                    flavor.id
                );
            }
            for r in flavor.racial_spells() {
                assert!(flavor.has_race(&r.race), "{} {}", flavor.id, r.race);
                assert!(r.spell > 0);
                for class in &r.classes {
                    assert!(flavor.class(class).is_some(), "{} {class}", flavor.id);
                }
            }
        }
        let camelot = t.flavor("camelot").unwrap();
        let ids = |race: &str, faction: Option<Faction>, class: Option<&str>| -> Vec<u32> {
            camelot
                .racials(race, faction, class)
                .map(|r| r.spell)
                .collect()
        };
        // The Skyborne: three of either faction, a fourth of each.
        assert_eq!(
            ids("Skyborne", Some(Faction::Alliance), Some("MAGE")),
            [1259416, 1259707, 1259710, 1259705]
        );
        assert_eq!(
            ids("Skyborne", Some(Faction::Horde), Some("DRUID")),
            [1259416, 1259707, 1259710, 1259686]
        );
        assert_eq!(ids("Skyborne", None, None), [1259416, 1259707, 1259710]);
        let names: Vec<&str> = camelot
            .racials("Skyborne", Some(Faction::Alliance), None)
            .map(|r| r.name.as_str())
            .collect();
        assert_eq!(
            names,
            [
                "Walk on Air",
                "Elemental Insight",
                "Wind Blessed",
                "Read Leyline"
            ]
        );
        // An Undead's Touch of the Grave by class.
        assert!(ids("Scourge", Some(Faction::Horde), Some("WARRIOR")).contains(&1260189));
        assert!(!ids("Scourge", Some(Faction::Horde), Some("WARRIOR")).contains(&1260201));
        assert!(ids("Scourge", Some(Faction::Horde), Some("MAGE")).contains(&1260201));
        assert!(ids("Human", Some(Faction::Alliance), Some("WARRIOR")).contains(&20600));
        // Mists' Pandaren: their language by faction, none chosen too.
        let mists = t.flavor("mists").unwrap();
        let langs = |faction| -> Vec<u32> {
            mists
                .racials("Pandaren", faction, Some("MONK"))
                .filter(|r| r.name == "Languages")
                .map(|r| r.spell)
                .collect()
        };
        assert_eq!(langs(Some(Faction::Alliance)), [143368]);
        assert_eq!(langs(Some(Faction::Horde)), [143369]);
        assert_eq!(langs(None), [131701]);
    }

    /// The classes each race can be (decision T513), the clients'
    /// `CharBaseInfo` pairs.
    #[test]
    fn races_and_their_classes() {
        let t = table();
        let vanilla = t.flavor("vanilla").unwrap();
        assert!(!vanilla.allows("Orc", "PRIEST"));
        assert!(vanilla.allows("Orc", "warlock"));
        assert!(!vanilla.allows("Human", "HUNTER"));
        assert!(!vanilla.allows("BloodElf", "PALADIN"));
        assert_eq!(
            vanilla.classes_for("Tauren", None),
            ["WARRIOR", "SHAMAN", "DRUID", "HUNTER"]
        );
        assert!(t.flavor("tbc").unwrap().allows("BloodElf", "PALADIN"));
        assert!(t.flavor("mists").unwrap().allows("Pandaren", "MONK"));
        assert!(!t.flavor("mists").unwrap().allows("Pandaren", "PALADIN"));
        // WoW Forever's own: an Undead paladin, a Human hunter; the
        // Skyborne by faction.
        let camelot = t.flavor("camelot").unwrap();
        assert!(camelot.allows("Scourge", "PALADIN") && camelot.allows("Human", "HUNTER"));
        assert_eq!(
            camelot.classes_for("Skyborne", Some(Faction::Alliance)),
            ["WARRIOR", "DRUID", "ROGUE", "MAGE", "HUNTER"]
        );
        assert_eq!(
            camelot.classes_for("Skyborne", Some(Faction::Horde)),
            ["WARRIOR", "SHAMAN", "DRUID", "ROGUE", "HUNTER"]
        );
        assert_eq!(camelot.classes_for("Skyborne", None).len(), 6);
        assert!(!camelot.allows_for("Skyborne", Some(Faction::Alliance), "SHAMAN"));
        assert!(camelot.allows("Skyborne", "SHAMAN"));
        assert!(camelot.classes_for("BloodElf", None).is_empty());
        // Retail's pairs are not read: any class.
        let standard = t.flavor("standard").unwrap();
        assert_eq!(standard.classes_for("Orc", None).len(), 13);
        // Every read race can be some class, each the flavor's.
        for flavor in &t.flavors {
            for race in flavor.race_files() {
                assert!(!flavor.classes_for(race, None).is_empty(), "{race}");
            }
        }
    }

    /// Each class's base mana by level (decision T512), the clients'
    /// `BaseMp` table.
    #[test]
    fn base_mana() {
        let t = table();
        let camelot = t.flavor("camelot").unwrap();
        assert_eq!(camelot.base_mana("MAGE", 1), Some(100.0));
        assert_eq!(camelot.base_mana("priest", 1), Some(110.0));
        assert_eq!(camelot.base_mana("WARRIOR", 1), None);
        assert!(camelot.base_mana("MAGE", 60) > camelot.base_mana("MAGE", 1));
        assert_eq!(camelot.base_mana("MAGE", 99), camelot.base_mana("MAGE", 60));
        assert_eq!(camelot.base_mana("MAGE", 0), camelot.base_mana("MAGE", 1));
        assert_eq!(
            t.flavor("vanilla").unwrap().base_mana("MAGE", 1),
            Some(155.0)
        );
        assert_eq!(t.flavor("standard").unwrap().base_mana("MAGE", 1), None);
        for flavor in &t.flavors {
            assert_eq!(
                flavor.power_default("MANA"),
                Some(PowerAmount {
                    current: 100.0,
                    max: 100.0
                }),
                "{}",
                flavor.id
            );
        }
    }

    /// Each flavor's max level (camelot's recorded: `GetMaxPlayerLevel()`
    /// answers 60; the others are the games' caps).
    #[test]
    fn max_levels() {
        let t = table();
        let levels: Vec<(&str, u32)> = t
            .flavors
            .iter()
            .map(|f| (f.id.as_str(), f.max_level))
            .collect();
        assert_eq!(
            levels,
            [
                ("camelot", 60),
                ("standard", 90),
                ("vanilla", 60),
                ("tbc", 70),
                ("mists", 90)
            ]
        );
    }
}
