//! `cargo xtask ingest [<SavedVariables/StdTest.lua>...]`: a client's StdTest
//! recordings into the repository's fixtures and data, scrubbed of the
//! player (name, last name and GUID; any player's GUID; the social systems'
//! texts), so a new client is checked by running the tests and reading the
//! diff:
//!
//! - `/stdtest engine` (and order, protected, blocked):
//!   `crates/wowlua-engine/tests/client/forever-camelot-engine.lua`;
//! - `/stdtest engine input`: `…/forever-camelot-input.lua`;
//! - `/stdtest engine answers`: `…/forever-camelot-answers.lua` (its checks
//!   `…/forever-camelot-checks.lua`);
//! - `/stdtest engine maps`: `…/forever-camelot-maps.lua` (and the data);
//! - `/stdtest engine cursor`: `…/forever-camelot-cursor.lua`;
//! - `/stdtest engine collections`: `…/forever-camelot-collections.lua`
//!   (and the data);
//! - `/stdtest engine achievements`: `…/forever-camelot-achievements.lua`
//!   (and the data);
//! - `/stdtest engine traits`: each class's `…/forever-camelot-traits-<class>.lua`
//!   (all of them: `crates/wowlua-engine/data/camelot-traits.txt`);
//! - `/stdtest engine tradeskill`: each profession's
//!   `…/forever-camelot-tradeskill-<profession>.lua` (all of them:
//!   `crates/wowlua-engine/data/camelot-tradeskill.txt`);
//! - `/stdtest engine trainer`: each trainer's `…/forever-camelot-trainer-<key>.lua`
//!   (all of them: `crates/wowlua-engine/data/camelot-trainers.txt`);
//! - `/stdtest engine group`: `…/forever-camelot-group-party.lua`,
//!   `…/forever-camelot-group-raid.lua`;
//! - the default character (a fresh level 1 Human Warrior's `/stdtest
//!   snapshot`): `cargo xtask profile <its StdTest.lua>
//!   crates/wowlua-engine/data/camelot-default-character.json`;
//! - `/stdtest snapshot`: the character's world as a test profile,
//!   `…/forever-camelot-character.json`;
//! - the CVar flags: `crates/wowlua-engine/data/camelot-cvar-flags.txt`;
//! - the font heights: `crates/wowlua-engine/data/camelot-font-heights.txt`;
//! - the static functions' answers: `crates/wowlua-engine/data/camelot-answers.txt`,
//!   those that differ by machine `…/camelot-answers-<platform>.txt` (decision T236);
//! - the login: `crates/wowlua-engine/data/camelot-login.txt` (and reload);
//! - `/stdtest engine fullprobe`: `…/forever-camelot-fullprobe.lua`;
//! - `/stdtest engine methodprobe`: `…/forever-camelot-methodprobe.lua`.
//!
//! Each part is written only when the file has it, and only when StdTest
//! stamped it later than the repository's (`tests/client/recorded.txt`;
//! results recorded before stamps only with `--unstamped`): a leftover in
//! a SavedVariables file never overwrites a newer recording. Answers merge
//! (one account's run never drops another's), the engine battery's parts
//! by their prefix; a file whose content is unchanged keeps its header.
//! Without a path, every account's SavedVariables in the WoW installation.
//! The engine's default world (the account's settings, the character:
//! the battery, answers, maps, collections, achievements, snapshot,
//! cursor, keys, input, login, the full probe) comes from one account (`#1`, or
//! `--account <n>`); a class's traits, a trainer, a window, a scenario
//! from any.

use anyhow::{Context, Result, anyhow};
use std::fmt::Write;
use std::path::Path;

/// The player as the file names them.
struct Identity {
    name: String,
    last_name: Option<String>,
    guid: String,
}

/// Functions whose answers are the player's social life: their strings are
/// blanked (the structure stays).
const SOCIAL: &[&str] = &[
    "C_Club.",
    "C_ClubFinder.",
    "C_GuildInfo.",
    "GetGuild",
    "BNGet",
    "C_BattleNet.",
    "C_FriendList.",
    "C_DamageMeter.",
    "C_RecentAllies.",
    "C_SocialQueue.",
    "GetFriendInfo",
    "GetWhoInfo",
];

impl Identity {
    fn scrub(&self, text: &str) -> String {
        // The session's addresses (`function: 0x…` on a Mac, `function:
        // 00000145B10B9B50` on Windows).
        let address = regex::Regex::new(
            r"(function|table|userdata|thread): (?:0x[0-9a-f]+|[0-9A-F]{8,16}\b)",
        )
        .expect("a valid pattern");
        let text = address.replace_all(text, "$1");
        let mut out = text.replace(&self.guid, "Player-0000-00000000");
        // Any other player's GUID.
        let guid = regex::Regex::new(r"Player-\d+-[0-9A-F]{8}").expect("a valid pattern");
        out = guid.replace_all(&out, "Player-0000-00000000").into_owned();
        out = out.replace(&self.name, "Player");
        if let Some(last) = &self.last_name {
            out = out.replace(last, "Lastname");
        }
        out
    }
}

/// A social function's answer with its strings blanked.
fn blank_strings(text: &str) -> String {
    let s = regex::Regex::new(r"string [^,}]*").expect("a valid pattern");
    s.replace_all(text, "string <scrubbed>").into_owned()
}

/// A Lua string literal (ASCII; others as `\ddd`).
fn quote(text: &str) -> String {
    let mut out = String::from("\"");
    for b in text.bytes() {
        match b {
            b'"' => out.push_str("\\\""),
            b'\\' => out.push_str("\\\\"),
            b'\n' => out.push_str("\\n"),
            b'\r' => out.push_str("\\r"),
            32..=126 => out.push(b as char),
            _ => {
                let _ = write!(out, "\\{b:03}");
            }
        }
    }
    out.push('"');
    out
}

/// `StdTestState`'s tables flattened as the fixtures are (`a.b.c` keys,
/// `tostring` values), by a Lua function given the state.
fn flatten(lua: &mlua::Lua, state: &mlua::Table, script: &str) -> Result<Vec<(String, String)>> {
    let f: mlua::Function = lua.load(script).eval().map_err(|e| anyhow!("{e}"))?;
    let t: mlua::Table = f.call(state.clone()).map_err(|e| anyhow!("{e}"))?;
    let mut out = Vec::new();
    for pair in t.pairs::<String, String>() {
        out.push(pair.map_err(|e| anyhow!("{e}"))?);
    }
    out.sort();
    Ok(out)
}

const FLATTEN: &str = r#"return function(s)
    local out = {}
    local function dump(v, prefix)
        if type(v) ~= "table" then out[prefix] = tostring(v) return end
        for k, inner in pairs(v) do dump(inner, prefix == "" and tostring(k) or (prefix .. "." .. tostring(k))) end
    end
    if s.engineProbe then dump(s.engineProbe, "probe") end
    if s.engineOrder then dump(s.engineOrder, "order") end
    if s.engineProtected then dump(s.engineProtected, "protected") end
    if s.engineBlocked then dump(s.engineBlocked, "blocked") end
    return out
end"#;

fn write_table(path: &Path, header: &str, entries: &[(String, String)]) -> Result<()> {
    let mut out = String::new();
    for line in header.lines() {
        let _ = writeln!(out, "-- {line}");
    }
    out.push_str("return {\n");
    for (k, v) in entries {
        let _ = writeln!(out, "    [{}] = {},", quote(k), quote(v));
    }
    out.push_str("}\n");
    write_file(path, &out)
}

/// A text without its header (its leading comment lines, `--` or `#`).
fn body(text: &str) -> &str {
    let mut rest = text;
    while rest.starts_with("--") || rest.starts_with('#') {
        rest = rest.split_once('\n').map_or("", |(_, r)| r);
    }
    rest
}

/// Writes `text` to `path` unless only its header would change (the
/// client's version on the same recording: no churn).
fn write_file(path: &Path, text: &str) -> Result<()> {
    if let Ok(old) = std::fs::read_to_string(path)
        && body(&old) == body(text)
    {
        println!("unchanged {}", path.display());
        return Ok(());
    }
    std::fs::write(path, text).with_context(|| path.display().to_string())?;
    println!("wrote {}", path.display());
    Ok(())
}

/// A fixture table's entries (`write_table`'s), to merge into.
fn read_table(path: &Path) -> Vec<(String, String)> {
    let Ok(src) = std::fs::read(path) else {
        return Vec::new();
    };
    let lua = mlua::Lua::new();
    let Ok(t) = lua.load(&src[..]).eval::<mlua::Table>() else {
        return Vec::new();
    };
    let mut out: Vec<(String, String)> = t.pairs::<String, String>().flatten().collect();
    out.sort();
    out
}

/// The entries of `old` and `new`, `new`'s winning: a run's answers added
/// to the repository's, none lost (an account's run may lack another's).
fn merged(old: Vec<(String, String)>, new: &[(String, String)]) -> Vec<(String, String)> {
    let mut all: std::collections::BTreeMap<String, String> = old.into_iter().collect();
    for (k, v) in new {
        all.insert(k.clone(), v.clone());
    }
    all.into_iter().collect()
}

/// What the repository's fixtures and data hold: each part's recording
/// (when, on which client), `tests/client/recorded.txt`. A file's part is
/// taken only when StdTest stamped it later (`engineStamps`); one without
/// a stamp (recorded before stamps) only with `--unstamped`.
pub struct Gate {
    path: std::path::PathBuf,
    recorded: std::collections::BTreeMap<String, (String, String)>,
    unstamped: bool,
    /// The file's WoW account (`#1`'s 1), when its path names one.
    account: Option<String>,
    /// The account the engine's default world is (`--account`, 1).
    world_account: String,
}

/// The parts that are the engine's default world (the account's settings,
/// the character): taken only from its account. A class's traits, a
/// trainer, a window, a scenario come from any.
const WORLD_PARTS: &[&str] = &[
    "probe",
    "order",
    "protected",
    "blocked",
    "answers",
    "maps",
    "collections",
    "achievements",
    "snapshot",
    "cursor",
    "keys",
    "input",
    "login",
    "reload",
    "fullprobe",
    "methodprobe",
];

/// The WoW account a SavedVariables path is in (`…/Account/<id>#2/…`: 2).
fn account_of(path: &Path) -> Option<String> {
    let mut parts = path
        .components()
        .map(|c| c.as_os_str().to_string_lossy().into_owned());
    parts.by_ref().find(|c| c == "Account")?;
    parts.next()?.rsplit_once('#').map(|(_, n)| n.to_string())
}

/// The time an unstamped part is recorded as: before any stamp.
const UNSTAMPED: &str = "0000-00-00 00:00:00";

impl Gate {
    pub fn load(root: &Path, unstamped: bool) -> Result<Gate> {
        let path = root.join("crates/wowlua-engine/tests/client/recorded.txt");
        let mut recorded = std::collections::BTreeMap::new();
        if let Ok(text) = std::fs::read_to_string(&path) {
            for line in text.lines().filter(|l| !l.starts_with('#')) {
                let mut parts = line.splitn(3, '\t');
                if let (Some(part), Some(at), Some(client)) =
                    (parts.next(), parts.next(), parts.next())
                {
                    recorded.insert(part.to_string(), (at.to_string(), client.to_string()));
                }
            }
        }
        Ok(Gate {
            path,
            recorded,
            unstamped,
            account: None,
            world_account: "1".to_string(),
        })
    }

    /// Whether to take `part` of a file with these stamps: its client if so
    /// (recorded as the repository's from now on).
    fn take(&mut self, stamps: &Stamps, part: &str, fallback: &str) -> Option<String> {
        if WORLD_PARTS.contains(&part)
            && let Some(account) = &self.account
            && *account != self.world_account
        {
            println!(
                "skipped {part}: account #{account}'s (the default world is account #{}'s; --account {account} takes it)",
                self.world_account
            );
            return None;
        }
        let (at, client) = match stamps.get(part) {
            Some(s) => s.clone(),
            None if self.unstamped => (UNSTAMPED.to_string(), fallback.to_string()),
            None => {
                println!(
                    "skipped {part}: not stamped (recorded before stamps; --unstamped takes it)"
                );
                return None;
            }
        };
        if let Some((have, _)) = self.recorded.get(part)
            && *have >= at
        {
            println!("skipped {part}: the repository's is as new ({have})");
            return None;
        }
        self.recorded.insert(part.to_string(), (at, client.clone()));
        Some(client)
    }

    pub fn save(&self) -> Result<()> {
        let mut out = String::from(
            "# Each part's recording the repository's fixtures and data hold: the\n\
             # part, when StdTest recorded it, the client (cargo xtask ingest takes a\n\
             # part only when a SavedVariables file has it recorded later).\n",
        );
        for (part, (at, client)) in &self.recorded {
            let _ = writeln!(out, "{part}\t{at}\t{client}");
        }
        write_file(&self.path, &out)
    }
}

/// A file's stamps: part -> (when, client).
type Stamps = std::collections::HashMap<String, (String, String)>;

fn stamps_of(state: &mlua::Table) -> Stamps {
    let mut out = Stamps::new();
    if let Ok(t) = state.get::<mlua::Table>("engineStamps") {
        for (part, stamp) in t.pairs::<String, mlua::Table>().flatten() {
            if let (Ok(at), Ok(client)) = (stamp.get::<String>("at"), stamp.get::<String>("client"))
            {
                out.insert(part, (at, client));
            }
        }
    }
    out
}

/// Functions that answer from the client's own data or the account's
/// settings (the same whatever the character): their recorded answers are
/// the engine's.
const STATIC: &[&str] = &[
    // Game data.
    "GetChatTypeIndex",
    "GetUICameraInfo",
    "C_PaperDollInfo.GetInventorySlotInfoForInvSlot",
    "C_TradeSkillUI.GetProfessionInfoBySkillLineID",
    "C_TradeSkillUI.CanTradeSkillShowCraftingUI",
    // What a map holds (the world's).
    "C_AreaPoiInfo.GetAreaPOIForMap",
    "C_AreaPoiInfo.GetQuestHubsForMap",
    "C_EncounterJournal.GetDungeonEntrancesForMap",
    "C_TaxiMap.ShouldMapShowTaxiNodes",
    "C_TaskQuest.DoesMapShowTaskQuestObjectives",
    "C_DeathInfo.GetGraveyardsForMap",
    // The character's explored map areas.
    "C_MapExplorationInfo.GetExploredMapTextures",
    // The account's transmog collection.
    "C_TransmogCollection.GetFilteredCategoryCollectedCount",
    "C_TransmogCollection.GetFilteredCategoryTotal",
    "C_TransmogCollection.GetAppearanceSourceInfo",
    "C_TransmogCollection.HasAnyAppearance",
    "C_TransmogCollection.GetNumTransmogSources",
    "C_Map.",
    "C_GameRules.",
    "GetBuildOption",
    "C_ClassColor.GetClassColor",
    "C_CreatureInfo.GetClassInfo",
    "LocalizedClassList",
    "GetSpecializationInfoForClassID",
    "C_SpecializationInfo.GetNumSpecializationsForClassID",
    "C_TradeSkillUI.GetProfessionSkillLineID",
    "C_Spell.GetSchoolString",
    // Spell data (the spells Blizzard's UI names; a test's own spells are
    // defined by the test).
    "C_Spell.GetSpellInfo",
    "C_Spell.GetSpellName",
    "C_Spell.GetSpellTexture",
    "C_Spell.GetSpellDescription",
    "GetInventorySlotInfo",
    "C_PaperDollInfo.GetInventorySlotInfo",
    "C_ColorOverrides.GetColorOverrideInfo",
    "C_UIWidgetManager.GetWidgetSetInfo",
    "C_UIWidgetManager.GetObjectiveTrackerWidgetSetID",
    "C_MountJournal.GetDynamicFlightModeSpellID",
    "C_MountJournal.GetNumMounts",
    "C_PetBattles.GetAllEffectNames",
    "C_PetJournal.GetNumPetSources",
    "C_QuestLog.GetMaxNumQuests",
    "C_GamepadUI.GetFirstGamepadActionStorageSlotIndex",
    "C_GamepadUI.GetFirstGamepadPetActionStorageSlotIndex",
    "C_Ping.GetDefaultPingOptions",
    "GetText",
    "GetMaxPlayerLevel",
    "GetTotalAchievementPoints",
    "GetMaxLevelForPlayerExpansion",
    "GetClassicExpansionLevel",
    "GetSendMailPrice",
    "C_StableInfo.GetNextStableSlotCost",
    "C_Bank.FetchMaxNumBankTabs",
    // The item classes' and inventory types' names; the auction house's
    // subclasses, filters and settings.
    "C_Item.GetItemClassInfo",
    "C_Item.GetItemSubClassInfo",
    "C_Item.GetItemInventorySlotInfo",
    "C_AuctionHouse.GetAuctionItemSubClasses",
    "C_AuctionHouse.GetFilterGroups",
    "C_AuctionHouse.SupportsCopperValues",
    "C_AuctionHouse.ShouldAutoPopulatePrice",
    "C_WowTokenPublic.GetCurrentMarketPrice",
    // Key bindings' data.
    "C_KeyBindings.GetSearchTagsForAction",
    "C_KeyBindings.GetBindingIndex",
    "GetBindingText",
    "IsBindingForGamePad",
    "C_KeyBindings.GetCustomBindingType",
    // The client's systems, as this build has them.
    "C_AccountServices.",
    "C_AdventureJournal.CanBeShown",
    "BNFeaturesEnabled",
    "C_BattleNet.IsBattleNetFriendsListSupported",
    "C_BattleNet.IsFriendTagSupportedForCurrentGameType",
    "C_CatalogShop.IsShop2Enabled",
    "C_ChatInfo.AreOutgoingAddonChatMessagesRestricted",
    "C_Club.IsEnabled",
    "C_Club.ShouldAllowClubType",
    "C_ClubFinder.IsCommunityFinderEnabled",
    "C_ClubFinder.GetClubFinderDisableReason",
    "C_Discord.IsVoiceEnabled",
    "C_EventScheduler.CanShowEvents",
    "C_ExternalEventURL.",
    "C_Housing.IsHousingServiceEnabled",
    "C_MythicPlus.IsMythicPlusActive",
    "C_PartyInfo.IsRaidListSupported",
    "C_PartyInfo.IsRaidListEnabled",
    "C_PhotoSharing.IsEnabled",
    "C_Ping.IsPingSystemEnabled",
    "C_RecentAllies.IsSystemEnabled",
    "C_RecentAllies.IsSystemSupported",
    "C_RecentAllies.IsInteractionCategoryFilterSupportedForCurrentGameType",
    "C_RecruitAFriend.IsSystemSupported",
    "C_RecruitAFriend.GetRAFSystemInfo",
    "C_SocialQueue.IsSystemSupported",
    "C_SocialRestrictions.IsFriendsDisabled",
    "C_SocialUI.IsSystemEnabled",
    "C_StorePublic.IsEnabled",
    "C_TradeSkillUI.IsGuildTradeSkillsEnabled",
    "C_VoiceChat.IsEnabled",
    "C_VoiceChat.GetActiveVoiceProviderID",
    "C_WowTokenPublic.GetCommerceSystemStatus",
    "CanShowAchievementUI",
    "GetLFGInfoServer",
    "IsTestBuild",
    "IsAccountSecured",
    "GetRestrictedAccountData",
    "NoPlayTime",
    "PartialPlayTime",
    // The machine's (the tests' screen is the recording's).
    "GetMonitorCount",
    "GetMonitorName",
    "IsGraphicsSettingValueSupported",
    "IsGraphicsCVarValueSupported",
    "MultiSampleAntiAliasingSupported",
    "GetAvailableLocales",
    "Sound_GameSystem_GetNumOutputDrivers",
    "Sound_GameSystem_GetOutputDriverNameByIndex",
    "Sound_GameSystem_GetNumInputDrivers",
    "Sound_GameSystem_GetInputDriverNameByIndex",
    "C_UI.ShouldUIParentAvoidNotch",
    "C_VideoOptions.",
    "GetCameraFOVDefaults",
    "GetGraphicsAPIs",
    "GetMaxRenderScale",
    "GetMinRenderScale",
    "IsCpuBound",
    "PlatformIsHandheld",
    "SupportsClipCursor",
    "CanAutoSetGamePadCursorControl",
    // The account's and the character's settings.
    "C_CombatAudioAlert.GetThrottle",
    "C_TTSSettings.",
    "GetChatWindowInfo",
    "GetChatWindowSavedDimensions",
    "GetChatWindowSavedPosition",
    "GetChatWindowChannels",
    "GetChatWindowMessages",
    "GetActionBarToggles",
    "IsThreatWarningEnabled",
    "GetTutorialsEnabled",
    "IsTutorialFlagged",
    "C_Spell.IsSpellPassive",
    "C_Spell.IsAutoAttackSpell",
    "C_Spell.IsRangedAutoAttackSpell",
    "C_ClickBindings.CanSpellBeClickBound",
    "C_KeyBindings.GetBindingContextForAction",
    "C_LevelLink.IsSpellLocked",
    "C_Minimap.IsFilteredOut",
    // The machine (graphics, display).
    "GetCurrentGraphicsAPI",
    "GetGraphicsAPIs",
    "IsGraphicsCVarValueSupported",
    "GetGraphicsCVarValueForQualityLevel",
    "GetMinRenderScale",
    "GetMaxRenderScale",
    "AntiAliasingSupported",
    "C_VideoOptions.",
    "C_UI.DoesAnyDisplayHaveNotch",
    // The account's settings and state.
    "C_CombatAudioAlert.",
    "C_SocialRestrictions.",
    "C_VoiceChat.IsSpeakForMeAllowed",
    "C_VoiceChat.GetPushToTalkBinding",
    "C_ClickBindings.GetProfileInfo",
    "C_ClickBindings.GetTutorialShown",
    "C_ClickBindings.GetBindingType",
    "C_SplashScreen.CanViewSplashScreen",
    "C_TransmogOutfitInfo.IsTransmogEnabled",
    // The account's outfits (Blizzard's spellbook counts them), the one
    // worn.
    "C_TransmogOutfitInfo.GetOutfitsInfo",
    "C_TransmogOutfitInfo.GetActiveOutfitID",
    // The account's mounts (their data and whether collected), the club
    // avatars, the game messages' sounds.
    "C_MountJournal.GetMountInfoByID",
    "C_MountJournal.GetMountInfoExtraByID",
    "C_Club.GetAvatarIdList",
    "GetGameMessageInfo",
    "GetAllowRecentAlliesSeeLocation",
    "C_EditMode.GetEditModeDefaultLayout",
    "C_EditMode.GetAccountSettings",
    "GetAutoDeclineGuildInvites",
    "GetCurrentBindingSet",
];

/// Functions of systems the engine does not model yet, answered as the
/// recording's world had them: nothing going on (no pet battle, scenario,
/// vehicle, summons, queue, totem, pet…). The engine's models take over
/// as they come.
const BASELINE: &[&str] = &[
    "C_ChatInfo.GetChannelRuleset",
    "C_UIWidgetManager.GetAllWidgetsBySetID",
    "C_TradeSkillUI.IsAnyRecipeFromSource",
    "C_Item.GetWeaponEnchantInfo",
    "C_AreaPoiInfo.GetDelvesForMap",
    "C_AreaPoiInfo.GetEventsForMap",
    "C_ContributionCollector.GetContributionCollectorsForMap",
    "C_DeathInfo.GetCorpseMapPosition",
    "C_Garrison.GetGarrisonPlotsInstancesForMap",
    "C_GossipInfo.GetPoiForUiMapID",
    "C_InvasionInfo.GetInvasionForUiMapID",
    "C_Navigation.GetNextWaypointForMap",
    "C_ResearchInfo.GetDigSitesForMap",
    "C_ActionBar.HasAssistedCombatActionButtons",
    "C_ActionBar.HasBonusActionBar",
    "C_ActionBar.HasExtraActionBar",
    "C_ActionBar.HasOverrideActionBar",
    "C_ActionBar.HasTempShapeshiftActionBar",
    "C_ActionBar.HasVehicleActionBar",
    "C_ArtifactUI.IsAtForge",
    "C_AssistedCombat.",
    "C_AzeriteItem.",
    "C_Bank.CanPurchaseBankTab",
    "C_Bank.FetchBankLockedReason",
    "C_Bank.FetchNumPurchasedBankTabs",
    "C_Bank.FetchPurchasedBankTabData",
    "C_CatalogShop.HasNewProducts",
    "C_ChatInfo.GetChannelShortcutForChannelID",
    "C_ClientScene.IsSceneTypeActive",
    "C_Club.GetInvitationsForSelf",
    "C_ClubFinder.GetPlayerApplicantLocaleFlags",
    "C_ClubFinder.GetPlayerApplicantSettings",
    "C_Container.GetBackpackAutosortDisabled",
    "C_Container.GetBackpackSellJunkDisabled",
    "C_Container.IsContainerFiltered",
    "C_ContentTracking.",
    "C_CurrencyInfo.DoesCurrentFilterRequireAccountCurrencyData",
    "C_DeathInfo.GetSelfResurrectOptions",
    "C_DelvesUI.",
    "C_EventScheduler.HasSavedReminders",
    "C_FogOfWar.GetFogOfWarForMap",
    "C_FrameManager.GetFrameVisibilityState",
    "C_GamePad.GetDeviceMappedState",
    "C_Heirloom.",
    "C_HeirloomInfo.",
    "C_Housing.GetTrackedHouseGuid",
    "C_Housing.IsInsideHouseOrPlot",
    "C_LFGList.",
    "C_LFGInfo.CanPlayerUseLFD",
    "C_LFGInfo.CanPlayerUsePremadeGroup",
    "C_LobbyMatchmakerInfo.IsInQueue",
    "C_MountJournal.GetDisplayedMountInfo",
    "C_MountJournal.GetNumDisplayedMounts",
    "C_MountJournal.HasDisplayableMount",
    "C_MountJournal.IsDragonridingUnlocked",
    "C_MountJournal.MountEquipmentAvailable",
    "C_NeighborhoodInitiative.GetTrackedInitiativeTasks",
    "C_PartyInfo.AllowedToDoPartyConversion",
    "C_PerksActivities.GetTrackedPerksActivities",
    "C_PetBattles.",
    "C_PetJournal.GetPetInfoByIndex",
    "C_PetJournal.GetSummonBattlePetCooldown",
    "C_PlayerChoice.",
    "C_PlayerInfo.GetAlternateFormInfo",
    "C_PlayerInfo.IsPlayerInRPE",
    "C_PlayerInfo.IsPlayerNPERestricted",
    "C_PlayerInfo.ShouldDisplaySurname",
    "C_PvP.",
    "C_QuestLog.CanAbandonQuest",
    "C_QuestLog.GetBountiesForMapID",
    "C_QuestLog.GetBountySetInfoForMapID",
    "C_QuestLog.GetNumWorldQuestWatches",
    "C_QuestLog.GetQuestWatchType",
    "C_QuestLog.GetQuestsOnMap",
    "C_QuestLog.GetZoneStoryInfo",
    "C_QuestLog.HasActiveThreats",
    "C_QuestLog.IsPushableQuest",
    "C_QuestLog.IsQuestDisabledForSession",
    "C_QuestSession.",
    "C_RecruitAFriend.GetRAFInfo",
    "C_Scenario.GetStepInfo",
    "C_Scenario.IsInScenario",
    "C_Scenario.ShouldShowCriteria",
    "C_ScenarioInfo.",
    "C_SocialQueue.GetAllGroups",
    "C_SpectatingUI.",
    "C_Spell.GetMawPowerRarityInfoBySpellID",
    "C_StableInfo.GetStablePetInfo",
    "C_StableInfo.GetNumStableSlots",
    "C_SummonInfo.",
    "C_TaskQuest.GetQuestsOnMap",
    "C_ToyBox.GetNumFilteredToys",
    "C_ToyBox.HasFavorites",
    "C_ToyBoxInfo.HasAnyToy",
    "C_TradeSkillUI.AnyRecipeCategoriesFiltered",
    "C_TradeSkillUI.AreAnyInventorySlotsFiltered",
    "C_TradeSkillUI.GetAllFilterableInventorySlotsCount",
    "C_TradeSkillUI.GetBaseProfessionInfo",
    "C_TradeSkillUI.GetChildProfessionInfo",
    "C_TradeSkillUI.GetChildProfessionInfos",
    "C_TradeSkillUI.GetOnlyShowFirstCraftRecipes",
    "C_TradeSkillUI.GetOnlyShowMakeableRecipes",
    "C_TradeSkillUI.GetOnlyShowSkillUpRecipes",
    "C_TradeSkillUI.GetRecipeItemNameFilter",
    "C_TradeSkillUI.IsNPCCrafting",
    "C_TradeSkillUI.IsTradeSkillGuild",
    "C_TradeSkillUI.IsTradeSkillGuildMember",
    "C_TradeSkillUI.IsTradeSkillLinked",
    "C_Traits.GetConfigIDByTreeID",
    "C_TransmogCollection.GetLatestAppearance",
    "C_TransmogCollection.IsSearchInProgress",
    "C_TransmogCollection.IsUsingDefaultFilters",
    "C_VoiceChat.GetActiveChannelType",
    "C_VoiceChat.IsMuted",
    "C_VoiceChat.IsSpeakForMeActive",
    "CanExitVehicle",
    "CanHearthAndResurrectFromArea",
    "GetArenaOpponentSpec",
    "GetBackgroundLoadingStatus",
    "GetCorpseRecoveryDelay",
    "GetCurrentArenaSeason",
    "GetLFGProposal",
    "GetLFGReadyCheckUpdate",
    "GetLFGRoleUpdate",
    "GetLootSpecialization",
    "GetNumArenaOpponentSpecs",
    "GetNumSavedWorldBosses",
    "GetPartyLFGID",
    "GetPetActionCooldown",
    "GetPetActionInfo",
    "GetPetExperience",
    "GetPetIcon",
    "GetPossessInfo",
    "GetReadyCheckStatus",
    "GetReleaseTimeRemaining",
    "GetSpecializationRoleEnum",
    "GetTotemInfo",
    "GetWorldElapsedTimers",
    "HasArtifactEquipped",
    "HasPetUI",
    "IsActiveBattlefieldArena",
    "IsEveryoneAssistant",
    "IsInActiveWorldPVP",
    "IsInInstance",
    "IsOnGroundFloorInJailersTower",
    "IsPVPTimerRunning",
    "IsPartyLFG",
    "IsTargetLoose",
    "PetHasActionBar",
    "ResurrectGetOfferer",
    "ResurrectHasSickness",
    "ResurrectHasTimer",
    "ShouldDisplayRaidRolesInSocialFrame",
    "C_LossOfControl.GetActiveLossOfControlDataByUnit",
    "C_NamePlateManager.IsNamePlateUnitBehindCamera",
    "C_PvP.IsPVPMap",
    "C_SpecializationInfo.GetPvpTalentSlotInfo",
    "C_SpecializationInfo.IsSpecSelectionEnabled",
    "InCinematic",
    "GetRaidTargetIndex",
    "UnitClassification",
    "UnitDetailedThreatSituation",
    "UnitIsMinion",
    "UnitIsTapDenied",
    "UnitNameplateShowsWidgetsOnly",
    "UnitPlayerControlled",
    "UnitSelectionColor",
    "UnitShouldDisplayName",
    "UnitWidgetSet",
];

/// Events of the player's session, not the client's startup: left out of
/// the login data.
const SESSION: &[&str] = &[
    "CONSOLE_MESSAGE",
    "SPELL_DATA_LOAD_RESULT",
    "QUEST_DATA_LOAD_RESULT",
    "ITEM_DATA_LOAD_RESULT",
    "CHAT_MSG_",
    "NAME_PLATE_",
    "UNIT_SPELLCAST_",
    "GUILD",
    "BN_",
    "CLUB_",
    "RAF_",
    "DISCORD",
    "RECENT_ALLIES",
    "TOKEN_",
    "TABARD_",
    "PLAYER_GUILD_UPDATE",
    "CRITERIA_UPDATE",
    "ACTION_RANGE_CHECK_UPDATE",
    "SPELL_ACTIVATION_OVERLAY",
    "UPDATE_MOUSEOVER_UNIT",
    "CURSOR_CHANGED",
    "WORLD_CURSOR_TOOLTIP_UPDATE",
    "GLOBAL_MOUSE",
    "GLOBAL_REGION_MOUSE",
    "MODIFIER_STATE_CHANGED",
    "PLAYER_SOFT_",
    "CURRENT_SPELL_CAST_CHANGED",
    // The recorder's own marks (when timers and OnUpdate run).
    "TIMER_TICK",
    "ON_UPDATE",
];

/// Events the test's world makes (its items, auras and action bars), not
/// replayed as recorded.
const WORLD: &[&str] = &[
    "PLAYER_EQUIPED_SPELLS_CHANGED",
    "BAG_UPDATE",
    "UNIT_INVENTORY_CHANGED",
    "UNIT_AURA",
    "ACTION_USABLE_CHANGED",
    "PLAYERBANKSLOTS_CHANGED",
];

/// The login's events as the engine replays them: one per line, the
/// recorded time, the event and its arguments; `$ADDON`, `$BINDINGS`,
/// `$CVAR` (the world's value of the CVar named before it), `$INVENTORY`
/// (the inventory's load, made from the world's items) in place of what
/// the test's world gives.
fn login_data(me: &Identity, version: &str, lines: &[String]) -> String {
    let mut out = format!(
        "# WoW Forever {version}'s login (/stdtest engine startup), from the addon's\n\
         # ADDON_LOADED to three seconds after PLAYER_ENTERING_WORLD: time, event and\n\
         # arguments (as recorded; $-names the test's world fills in). The player's\n\
         # session (chat, guild, Battle.net, others' casts, data loads) is left out\n\
         # (cargo xtask ingest).\n"
    );
    let mut inventory = false;
    for line in lines {
        let Some((time, rest)) = line.split_once(' ') else {
            continue;
        };
        let (event, args) = rest.split_once('(').unwrap_or((rest, ")"));
        let args = args.strip_suffix(')').unwrap_or(args);
        if SESSION.iter().any(|p| event.starts_with(p)) {
            continue;
        }
        // Units seen around the player (`nameplate5`): the world, which
        // differs each time.
        if args
            .strip_prefix("string nameplate")
            .is_some_and(|n| n.starts_with(|c: char| c.is_ascii_digit()))
        {
            continue;
        }
        // Only the addon's own ADDON_LOADED: Blizzard's load-on-demand
        // addons fire theirs as Blizzard's code loads them.
        if event == "ADDON_LOADED" {
            if args.starts_with("string StdTest;") {
                let _ = writeln!(out, "{time}\tADDON_LOADED\t$ADDON; $BINDINGS");
            }
            continue;
        }
        if WORLD.contains(&event) {
            if !inventory && event == "PLAYER_EQUIPED_SPELLS_CHANGED" {
                inventory = true;
                let _ = writeln!(out, "{time}\t$INVENTORY\t");
            }
            continue;
        }
        if event == "CVAR_UPDATE" {
            let name = args.split("; ").next().unwrap_or("");
            let _ = writeln!(out, "{time}\tCVAR_UPDATE\t{name}; $CVAR");
            continue;
        }
        // Edit Mode's layouts: the test's world's (the account's own
        // layouts, named by the player, are not kept).
        if event == "EDIT_MODE_LAYOUTS_UPDATED" {
            let reconcile = args.rsplit("; ").next().unwrap_or("boolean false");
            let _ = writeln!(
                out,
                "{time}	EDIT_MODE_LAYOUTS_UPDATED	$EDIT_MODE_LAYOUTS; {reconcile}"
            );
            continue;
        }
        // Other tables' contents are not replayed (the startup's other
        // events with one are the session's: group buffs, covenant
        // callings).
        if args.contains('{') {
            continue;
        }
        let _ = writeln!(out, "{time}\t{event}\t{}", me.scrub(args));
    }
    out
}

/// The player of a file with scenarios only: the name its snapshots
/// give, the last name its own loot messages' sender carries ("Name Last"),
/// the GUID its logs show most (the player's own messages).
fn identity_from_scenarios(state: &mlua::Table) -> Result<Identity> {
    let scenarios: mlua::Table = state
        .get("engineScenarios")
        .map_err(|_| anyhow!("no /stdtest engine results or scenarios to name the player by"))?;
    let mut name = None;
    let mut logs = String::new();
    for pair in scenarios.pairs::<String, mlua::Table>() {
        let (_, scenario) = pair.map_err(|e| anyhow!("{e}"))?;
        if name.is_none()
            && let Ok(before) = scenario.get::<mlua::Table>("before")
            && let Ok(character) = before.get::<mlua::Table>("character")
            && let Ok(n) = character.get::<String>("name")
        {
            name = Some(n);
        }
        if let Ok(log) = scenario.get::<mlua::Table>("log") {
            for line in log.sequence_values::<String>().flatten() {
                logs.push_str(&line);
                logs.push('\n');
            }
        }
    }
    let name = name.ok_or_else(|| anyhow!("no scenario snapshot names the player"))?;
    let count = |re: &regex::Regex| {
        let mut seen: std::collections::BTreeMap<String, usize> = Default::default();
        for c in re.captures_iter(&logs) {
            *seen.entry(c[1].to_string()).or_default() += 1;
        }
        seen.into_iter().max_by_key(|(_, n)| *n).map(|(k, _)| k)
    };
    let sender = regex::Regex::new(&format!(r"string {} ([^;\s]+);", regex::escape(&name)))
        .expect("a valid pattern");
    let guid = regex::Regex::new(r"(Player-\d+-[0-9A-F]{8})").expect("a valid pattern");
    Ok(Identity {
        last_name: count(&sender),
        guid: count(&guid).unwrap_or_default(),
        name,
    })
}

/// The player of a file with the full probe or the method probe only:
/// their answers to `UnitName("player")` and `UnitGUID("player")`.
fn identity_from_fullprobe(state: &mlua::Table) -> Option<Identity> {
    ["engineFullProbe", "engineMethodProbe"]
        .iter()
        .find_map(|key| identity_from_probe(state, key))
}

fn identity_from_probe(state: &mlua::Table, key: &str) -> Option<Identity> {
    let probe: mlua::Table = state.get(key).ok()?;
    let strings = |v: &str| -> Vec<String> {
        let re = regex::Regex::new(r"string ([^,]+)").expect("a valid pattern");
        re.captures_iter(v)
            .map(|c| c[1].trim().to_string())
            .collect()
    };
    let names = strings(&probe.get::<String>("UnitName(\"player\")").ok()?);
    let guid = strings(&probe.get::<String>("UnitGUID(\"player\")").ok()?)
        .into_iter()
        .next()?;
    Some(Identity {
        name: names.first().cloned()?,
        last_name: names.get(1).cloned(),
        guid,
    })
}

/// The full probe's answers scrubbed: the player (name, last name, GUID),
/// the realm and the guild (as the run's own answers name them), the
/// social systems' strings blanked.
fn fullprobe_entries(me: &Identity, probe: &mlua::Table) -> Result<Vec<(String, String)>> {
    let first_string = |call: &str| -> Option<String> {
        let answer: String = probe.get(call).ok()?;
        let re = regex::Regex::new(r"^\d+: string ([^,]+)").expect("a valid pattern");
        re.captures(&answer)
            .map(|c| c[1].trim().to_string())
            .filter(|s| s.len() >= 3)
    };
    let mut names: Vec<(String, &str)> = Vec::new();
    for (call, with) in [
        ("GetRealmName()", "Realm"),
        ("GetNormalizedRealmName()", "Realm"),
        ("GetGuildInfo(\"player\")", "Guild"),
    ] {
        if let Some(name) = first_string(call) {
            names.push((name, with));
        }
    }
    // The longest first (a realm's name in its normalized one).
    names.sort_by_key(|(n, _)| std::cmp::Reverse(n.len()));
    let scrub = |text: &str| -> String {
        let mut out = me.scrub(text);
        for (name, with) in &names {
            out = out.replace(name.as_str(), with);
        }
        out
    };
    // A player link's name and text (`|Hplayer:Otherplayer:…|h[Otherplayer]|h`),
    // but the placeholders'.
    let player_links = regex::Regex::new(r"(\|H(?:player):)([^|:\]]*)([^|]*\|h\[)?([^\]|]*)")
        .expect("a valid pattern");
    let placeholder = regex::Regex::new(r"^(Player|Lastname|Realm)([ -](Player|Lastname|Realm))*$")
        .expect("a valid pattern");
    let mut entries = Vec::new();
    for pair in probe.pairs::<mlua::LuaString, mlua::LuaString>() {
        let (call, answer) = pair.map_err(|e| anyhow!("{e}"))?;
        let call = escaped_bytes(&call.as_bytes());
        let answer = escaped_bytes(&answer.as_bytes());
        let answer = if names_others(&call) {
            blank_strings(&answer)
        } else {
            answer
        };
        let scrubbed = scrub(&answer);
        let answer = player_links.replace_all(&scrubbed, |c: &regex::Captures| {
            let own = |text: &str| text.is_empty() || placeholder.is_match(text);
            match c.get(3) {
                Some(rest) => format!(
                    "{}{}{}{}",
                    &c[1],
                    if own(&c[2]) { &c[2] } else { "Otherplayer" },
                    rest.as_str(),
                    if own(&c[4]) { &c[4] } else { "Otherplayer" }
                ),
                None => format!(
                    "{}{}{}",
                    &c[1],
                    if own(&c[2]) { &c[2] } else { "Otherplayer" },
                    &c[4]
                ),
            }
        });
        entries.push((scrub(&call), answer.into_owned()));
    }
    entries.sort();
    Ok(entries)
}

/// Functions whose answers may name other players (the social systems',
/// the group's, the guild's, chat, mail, auctions, the group finder,
/// calendar invites, petitions): the full probe's strings of them blanked.
const OTHERS: &[&str] = &[
    "C_ChatInfo.",
    "GetChannelRoster",
    "GetChatLine",
    "C_Calendar.",
    "CalendarEvent",
    "GetRaidRosterInfo",
    "GetGuildRoster",
    "GetGuildMember",
    "GetGuildNews",
    "GetGuildEvent",
    "GetGuildBank",
    "GuildControl",
    "C_LFGList.",
    "C_PartyInfo.",
    "C_RecentAllies.",
    "C_AuctionHouse.",
    "GetInbox",
    "GetPetition",
    "GetTrade",
    "GetMasterLoot",
    "GetLootRoll",
    "GetIgnore",
    "GetBattlefieldScore",
    "C_PvP.",
    "C_Commentator.",
    "GetReadyCheck",
    "GetPartyAssignment",
    "GetLFG",
    "C_LFGInfo.",
    "GetNumWho",
    "GetWho",
    "C_Housing",
    "C_PlayerInfo.GetPlayerMythicPlusRatingSummary",
    "C_TradeSkillUI.GetRecipeCrafter",
    "C_CraftingOrders.",
    "C_Item.GetItemCreator",
    "GetItemCreator",
    "ItemTextGetCreator",
    "GetPlayerInfoByGUID",
    "C_SocialRestrictions.",
    "C_Texture.GetTitleIconTexture",
];

/// Functions that name a unit: other players' names unless asked of the
/// player.
const UNIT_NAMES: &[&str] = &[
    "UnitName",
    "UnitFullName",
    "UnitNameUnmodified",
    "UnitNameFromGUID",
    "UnitPVPName",
    "GetUnitName",
    "UnitSpellTargetName",
    "UnitSpellTargetClass",
    "C_PlayerInfo.GetName",
];

/// Whether a full probe call's strings may name other players.
fn names_others(call: &str) -> bool {
    if SOCIAL.iter().chain(OTHERS).any(|p| call.starts_with(p)) {
        return true;
    }
    let Some((name, args)) = call.split_once('(') else {
        return false;
    };
    UNIT_NAMES.contains(&name)
        && !["\"player\")", "UnitGUID(\"player\"))", "\"player\", "]
            .iter()
            .any(|own| args.starts_with(own))
}

/// An auction's owners (`owners={1=string <name>, …}`): other players'
/// names, but the player's own ("player").
fn scrub_owners(line: &str) -> String {
    let list = regex::Regex::new(r"owners=\{([^}]*)\}").expect("a valid pattern");
    let name = regex::Regex::new(r"(\d+=string )([^,}]+)").expect("a valid pattern");
    list.replace_all(line, |c: &regex::Captures| {
        let body = name.replace_all(&c[1], |n: &regex::Captures| {
            if &n[2] == "player" {
                n[0].to_string()
            } else {
                format!("{}Otherplayer", &n[1])
            }
        });
        format!("owners={{{body}}}")
    })
    .into_owned()
}

/// A call or answer as a data file's field: one line, no tab (a line
/// break, a tab, a carriage return as the symbols for them, ␤ ␉ ␍, which
/// the client's texts do not hold; the engine reads them back).
fn data_text(text: &str) -> String {
    text.replace('\n', "\u{2424}")
        .replace('\t', "\u{2409}")
        .replace('\r', "\u{240D}")
}

/// The engine's trait data (`data/camelot-traits.txt`): every class's
/// recorded trait answers (`forever-camelot-traits-<class>.lua`, from any
/// account's ingest): the class, a tab, the call, a tab, the answer.
fn write_traits_data(root: &Path, client: &Path, version: &str) -> Result<()> {
    let (lines, classes) = fixtures_by(client, "forever-camelot-traits-")?;
    let lines = owned_traits(lines);
    let mut data = format!(
        "# WoW Forever {version}'s trait configs as characters answered them\n\
         # (/stdtest engine traits: {}): the class, the call, the answer, a tab\n\
         # between (cargo xtask ingest). The engine's trees, entries, conditions\n\
         # and costs; the character's ranks and currencies as recorded.\n",
        classes.join(", ")
    );
    for ((class, call), answer) in &lines {
        let _ = writeln!(data, "{class}\t{}\t{}", data_text(call), data_text(answer));
    }
    let path = root.join("crates/wowlua-engine/data/camelot-traits.txt");
    write_file(&path, &data)?;
    Ok(())
}

/// The numbers of a list field of an answer (`treeIDs={1=number 1089}`).
fn list_numbers(answer: &str, field: &str) -> Vec<i64> {
    let Some(at) = answer.find(&format!("{field}={{")) else {
        return Vec::new();
    };
    let rest = &answer[at + field.len() + 2..];
    let mut depth = 1;
    let end = rest
        .char_indices()
        .find(|(_, c)| {
            match c {
                '{' => depth += 1,
                '}' => depth -= 1,
                _ => {}
            }
            depth == 0
        })
        .map_or(rest.len(), |(i, _)| i);
    static NUMBER: std::sync::OnceLock<regex::Regex> = std::sync::OnceLock::new();
    NUMBER
        .get_or_init(|| regex::Regex::new(r"number (-?\d+)").expect("a valid pattern"))
        .captures_iter(&rest[..end])
        .filter_map(|c| c[1].parse().ok())
        .collect()
}

/// A call's integer arguments (`C_Traits.GetNodeInfo(22147, 110301)`).
fn call_args(call: &str) -> Vec<i64> {
    call.split_once('(')
        .map(|(_, a)| a.trim_end_matches(')'))
        .unwrap_or("")
        .split(", ")
        .filter_map(|a| a.parse().ok())
        .collect()
}

/// The trait answers the engine needs: each node's, entry's, definition's
/// and spell's from the class whose config holds its tree (every class
/// asks every class's trees; another's are worked out from the owner's,
/// the fixtures keep them all), an answer every class gives alike once.
fn owned_traits(
    lines: std::collections::BTreeMap<(String, String), String>,
) -> std::collections::BTreeMap<(String, String), String> {
    use std::collections::{BTreeMap, BTreeSet};
    let name = |call: &str| call.split('(').next().unwrap_or("").to_string();
    let gate = regex::Regex::new(r"conditionID=number (\d+)").expect("a valid pattern");
    let def = regex::Regex::new(r"definitionID=number (\d+)").expect("a valid pattern");
    let spell =
        regex::Regex::new(r"(?:spellID|overriddenSpellID)=number (\d+)").expect("a valid pattern");
    // The trees of each class's configs; each tree's nodes.
    let mut config_trees: BTreeMap<(String, i64), BTreeSet<i64>> = BTreeMap::new();
    let mut tree_of: BTreeMap<i64, i64> = BTreeMap::new();
    for ((class, call), answer) in &lines {
        let args = call_args(call);
        match name(call).as_str() {
            "C_Traits.GetConfigInfo" if args.len() == 1 => {
                let trees = list_numbers(answer, "treeIDs");
                config_trees.insert((class.clone(), args[0]), trees.into_iter().collect());
            }
            "C_Traits.GetTreeNodes" if args.len() == 1 => {
                let nodes = answer.split_once(": ").map_or("", |(_, a)| a);
                for n in list_numbers(&format!("x={nodes}"), "x") {
                    tree_of.insert(n, args[0]);
                }
            }
            _ => {}
        }
    }
    // A tree no class's config holds (one more class tree than classes):
    // each node's answers kept from the first character that recorded it.
    let held: BTreeSet<i64> = config_trees.values().flatten().copied().collect();
    let mut first: BTreeMap<i64, String> = BTreeMap::new();
    for (class, call) in lines.keys() {
        let args = call_args(call);
        if name(call) == "C_Traits.GetNodeInfo" && args.len() == 2 {
            first.entry(args[1]).or_insert_with(|| class.clone());
        }
    }
    let holds = |class: &str, config: i64, node: i64| {
        tree_of.get(&node).is_some_and(|t| {
            config_trees
                .get(&(class.to_string(), config))
                .is_some_and(|ts| ts.contains(t))
                || (!held.contains(t) && first.get(&node).is_some_and(|c| c == class))
        })
    };
    // What each class owns: its configs' nodes' entries and conditions,
    // their definitions and spells.
    let mut entries: BTreeSet<(String, i64)> = BTreeSet::new();
    let mut conditions: BTreeSet<(String, i64, i64)> = BTreeSet::new();
    for ((class, call), answer) in &lines {
        let args = call_args(call);
        if name(call) == "C_Traits.GetNodeInfo" && args.len() == 2 && holds(class, args[0], args[1])
        {
            for e in list_numbers(answer, "entryIDs") {
                entries.insert((class.clone(), e));
            }
            for c in list_numbers(answer, "conditionIDs") {
                conditions.insert((class.clone(), args[0], c));
            }
        }
        if name(call) == "C_Traits.GetTreeInfo" && args.len() == 2 {
            for c in gate.captures_iter(answer) {
                if let Ok(c) = c[1].parse() {
                    conditions.insert((class.clone(), args[0], c));
                }
            }
        }
    }
    let mut definitions: BTreeSet<(String, i64)> = BTreeSet::new();
    for ((class, call), answer) in &lines {
        let args = call_args(call);
        if name(call) == "C_Traits.GetEntryInfo"
            && args.len() == 2
            && entries.contains(&(class.clone(), args[1]))
        {
            if let Some(d) = def.captures(answer).and_then(|c| c[1].parse().ok()) {
                definitions.insert((class.clone(), d));
            }
            for c in list_numbers(answer, "conditionIDs") {
                conditions.insert((class.clone(), args[0], c));
            }
        }
    }
    let mut spells: BTreeSet<(String, i64)> = BTreeSet::new();
    for ((class, call), answer) in &lines {
        let args = call_args(call);
        if name(call) == "C_Traits.GetDefinitionInfo"
            && args.len() == 1
            && definitions.contains(&(class.clone(), args[0]))
        {
            for c in spell.captures_iter(answer) {
                if let Ok(id) = c[1].parse() {
                    spells.insert((class.clone(), id));
                }
            }
        }
    }
    let kept = lines.into_iter().filter(|((class, call), _)| {
        let args = call_args(call);
        let c = class.clone();
        match name(call).as_str() {
            "C_Traits.GetNodeInfo"
            | "C_Traits.GetNodeCost"
            | "C_Traits.CanRefundRank"
            | "C_Traits.CanPurchaseRank"
                if args.len() >= 2 =>
            {
                holds(class, args[0], args[1])
            }
            "C_Traits.GetEntryInfo" if args.len() == 2 => entries.contains(&(c, args[1])),
            "C_Traits.GetConditionInfo" if args.len() == 2 => {
                conditions.contains(&(c, args[0], args[1]))
            }
            "C_Traits.GetTreeInfo" | "C_Traits.GetTreeCurrencyInfo" if args.len() >= 2 => {
                config_trees
                    .get(&(c, args[0]))
                    .is_some_and(|ts| ts.contains(&args[1]))
            }
            "C_Traits.GetTraitDescription" if !args.is_empty() => entries.contains(&(c, args[0])),
            "C_Traits.GetIncreasedTraitData" if args.len() == 2 => entries.contains(&(c, args[1])),
            "C_Traits.GetDefinitionInfo" if args.len() == 1 => definitions.contains(&(c, args[0])),
            n if (n.starts_with("C_Spell.") || n == "GetSpellBaseCooldown") && args.len() == 1 => {
                spells.contains(&(c, args[0]))
            }
            _ => true,
        }
    });
    // An answer every class that has it gives alike: the first's only.
    let mut by_call: BTreeMap<String, Vec<(String, String)>> = BTreeMap::new();
    for ((class, call), answer) in kept {
        by_call.entry(call).or_default().push((class, answer));
    }
    let mut out = BTreeMap::new();
    for (call, answers) in by_call {
        let alike = answers.windows(2).all(|w| w[0].1 == w[1].1);
        if alike && answers.len() > 1 {
            let (class, answer) = answers.into_iter().next().expect("one");
            out.insert((class, call), answer);
        } else {
            for (class, answer) in answers {
                out.insert((class, call.clone()), answer);
            }
        }
    }
    out
}

/// The engine's professions' data (`data/camelot-tradeskill.txt`): every
/// profession window's recorded answers (`forever-camelot-tradeskill-<p>.lua`,
/// from any account's ingest): the profession, the call, the answer.
fn write_tradeskill_data(root: &Path, client: &Path, version: &str) -> Result<()> {
    let (lines, professions) = fixtures_by(client, "forever-camelot-tradeskill-")?;
    let mut data = format!(
        "# WoW Forever {version}'s profession windows as a character answered them\n\
         # (/stdtest engine tradeskill: {}): the profession, the call, the answer,\n\
         # a tab between (cargo xtask ingest). The engine's recipes; the skill and\n\
         # the bags' counts as recorded.\n",
        professions.join(", ")
    );
    for ((profession, call), answer) in &lines {
        let _ = writeln!(
            data,
            "{profession}\t{}\t{}",
            data_text(call),
            data_text(answer)
        );
    }
    let path = root.join("crates/wowlua-engine/data/camelot-tradeskill.txt");
    write_file(&path, &data)?;
    Ok(())
}

/// The answers of the fixtures named `<prefix><key>.lua`: by (KEY, call),
/// errors left out; the keys.
#[allow(clippy::type_complexity)]
fn fixtures_by(
    client: &Path,
    prefix: &str,
) -> Result<(
    std::collections::BTreeMap<(String, String), String>,
    Vec<String>,
)> {
    let mut lines = std::collections::BTreeMap::new();
    let mut classes = Vec::new();
    for entry in std::fs::read_dir(client)? {
        let path = entry?.path();
        let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
        let Some(class) = name
            .strip_prefix(prefix)
            .and_then(|n| n.strip_suffix(".lua"))
        else {
            continue;
        };
        classes.push(class.to_string());
        let lua = mlua::Lua::new();
        let table: mlua::Table = lua
            .load(std::fs::read(&path)?)
            .eval()
            .map_err(|e| anyhow!("{}: {e}", path.display()))?;
        for pair in table.pairs::<String, String>() {
            let (call, answer) = pair.map_err(|e| anyhow!("{e}"))?;
            if !answer.starts_with("error") {
                lines.insert((class.to_uppercase(), call), answer);
            }
        }
    }
    classes.sort();
    Ok((lines, classes))
}

/// An answer's values (`n: v1, v2, …`): where each starts (after a
/// top-level ", " followed by a type or a table).
fn answer_values(answer: &str) -> Option<(&str, Vec<&str>)> {
    const TYPES: &[&str] = &["string ", "number ", "boolean ", "nil ", "secret ", "{"];
    let (count, rest) = answer.split_once(": ")?;
    let bytes = rest.as_bytes();
    let (mut depth, mut start) = (0i32, 0);
    let mut values = Vec::new();
    for i in 0..bytes.len() {
        match bytes[i] {
            b'{' => depth += 1,
            b'}' => depth -= 1,
            b',' if depth == 0
                && bytes.get(i + 1) == Some(&b' ')
                && TYPES.iter().any(|t| rest[i + 2..].starts_with(t)) =>
            {
                values.push(&rest[start..i]);
                start = i + 2;
            }
            _ => {}
        }
    }
    values.push(&rest[start..]);
    Some((count, values))
}

/// The characters who earned achievements and criteria, in their answers:
/// `GetAchievementInfo`'s `earnedBy` (14th), `GetAchievementCriteriaInfo`'s
/// `charName` (6th), "Player" when a name.
fn scrub_earners(call: &str, answer: &str) -> String {
    let at = if call.starts_with("GetAchievementInfo(") {
        13
    } else if call.starts_with("GetAchievementCriteriaInfo(") {
        5
    } else {
        return answer.to_string();
    };
    let Some((count, mut values)) = answer_values(answer) else {
        return answer.to_string();
    };
    if values
        .get(at)
        .is_some_and(|v| v.starts_with("string ") && v.len() > 7)
    {
        values[at] = "string Player";
    }
    format!("{count}: {}", values.join(", "))
}

/// A profile as `cargo xtask profile` writes it: pretty, keys in order,
/// a line break at the end.
fn pretty(v: &serde_json::Value) -> Option<String> {
    serde_json::to_string_pretty(v).ok().map(|t| t + "\n")
}

/// A profile's character with its last name (`"lastName": "Lastname"`).
fn with_last_name(profile: &str) -> String {
    match serde_json::from_str::<serde_json::Value>(profile) {
        Ok(mut v) => {
            if let Some(c) = v.get_mut("character").and_then(|c| c.as_object_mut()) {
                c.insert("lastName".into(), "Lastname".into());
                c.sort_keys();
            }
            pretty(&v).unwrap_or_else(|| profile.to_string())
        }
        Err(_) => profile.to_string(),
    }
}

/// A profile's items marked readable (`"readable": true`).
fn with_books(profile: &str, books: &std::collections::BTreeSet<String>) -> String {
    match serde_json::from_str::<serde_json::Value>(profile) {
        Ok(mut v) => {
            if let Some(items) = v.get_mut("items").and_then(|i| i.as_object_mut()) {
                for id in books {
                    if let Some(item) = items.get_mut(id).and_then(|i| i.as_object_mut()) {
                        item.insert("readable".into(), true.into());
                        item.sort_keys();
                    }
                }
            }
            pretty(&v).unwrap_or_else(|| profile.to_string())
        }
        Err(_) => profile.to_string(),
    }
}

/// `cargo xtask ingest [--unstamped] [--client <folder>] [<StdTest.lua>...]`:
/// each SavedVariables file's parts newer than the repository's; without
/// files, every account's in the WoW installation (`wowlua install`'s:
/// `--wow`'s environment `WOWLUA_WOW_PATH`, the config, Battle.net's
/// default folder on macOS, Windows and under Wine on Linux) client
/// folder (`_forever_` unless `--client`).
pub fn run_all(root: &Path, args: &[String]) -> Result<()> {
    let mut unstamped = false;
    let mut world_account = "1".to_string();
    let mut client = "_forever_".to_string();
    let mut paths: Vec<std::path::PathBuf> = Vec::new();
    let mut it = args.iter();
    while let Some(arg) = it.next() {
        match arg.as_str() {
            "--unstamped" => unstamped = true,
            "--account" => {
                world_account = it
                    .next()
                    .ok_or_else(|| anyhow!("--account needs a number (2 for `<id>#2`)"))?
                    .clone();
            }
            "--client" => {
                client = it
                    .next()
                    .ok_or_else(|| anyhow!("--client needs a client folder (_forever_)"))?
                    .clone();
            }
            other if other.starts_with("--") => {
                return Err(anyhow!(
                    "usage: cargo xtask ingest [--unstamped] [--account <n>] [--client <folder>] [<StdTest.lua>...]"
                ));
            }
            path => paths.push(path.into()),
        }
    }
    if paths.is_empty() {
        let wow = wowlua_driver::install::find_wow(None).map_err(|d| anyhow!("{}", d.message))?;
        paths = saved_variables(&wow.join(&client));
        if paths.is_empty() {
            return Err(anyhow!(
                "no StdTest SavedVariables under {}",
                wow.join(&client).display()
            ));
        }
    }
    let mut gate = Gate::load(root, unstamped)?;
    gate.world_account = world_account;
    for path in &paths {
        println!("ingesting {}", path.display());
        gate.account = account_of(path);
        run(root, path, &mut gate)?;
    }
    gate.save()
}

/// Every account's StdTest SavedVariables in a client folder.
fn saved_variables(client: &Path) -> Vec<std::path::PathBuf> {
    let mut out: Vec<std::path::PathBuf> = std::fs::read_dir(client.join("WTF").join("Account"))
        .into_iter()
        .flatten()
        .flatten()
        .map(|account| account.path().join("SavedVariables").join("StdTest.lua"))
        .filter(|p| p.is_file())
        .collect();
    out.sort();
    out
}

pub fn run(root: &Path, saved: &Path, gate: &mut Gate) -> Result<()> {
    let src = std::fs::read(saved).with_context(|| saved.display().to_string())?;
    let lua = mlua::Lua::new();
    lua.load(&src[..])
        .exec()
        .map_err(|e| anyhow!("{}: {e}", saved.display()))?;
    let state: mlua::Table = lua
        .globals()
        .get("StdTestState")
        .map_err(|_| anyhow!("no StdTestState in {}", saved.display()))?;
    let client = root.join("crates/wowlua-engine/tests/client");
    let battery = flatten(&lua, &state, FLATTEN)?;
    let get = |key: &str| {
        battery
            .iter()
            .find(|(k, _)| k == key)
            .map(|(_, v)| v.clone())
    };
    // The player, from the battery's unit calls.
    let strings = |v: &str| -> Vec<String> {
        let re = regex::Regex::new(r"string ([^,]+)").expect("a valid pattern");
        re.captures_iter(v)
            .map(|c| c[1].trim().to_string())
            .collect()
    };
    // The player: from the /stdtest engine results; a file with
    // scenarios only, from their snapshots and logs.
    let has_battery = get("probe.units.UnitName(player)").is_some();
    let me = if has_battery {
        let names = get("probe.units.UnitName(player)")
            .map(|v| strings(&v))
            .unwrap_or_default();
        let guid = get("probe.units.UnitGUID(player)")
            .and_then(|v| strings(&v).into_iter().next())
            .ok_or_else(|| anyhow!("no player GUID in the /stdtest engine results"))?;
        Identity {
            name: names.first().cloned().unwrap_or_default(),
            last_name: names.get(1).cloned(),
            guid,
        }
    } else {
        match identity_from_scenarios(&state) {
            Ok(me) => me,
            Err(e) => identity_from_fullprobe(&state).ok_or(e)?,
        }
    };
    let stamps = stamps_of(&state);
    // The battery's client, else the one the parts were stamped on.
    let version = get("probe.client")
        .or_else(|| stamps.values().next().map(|(_, client)| client.clone()))
        .unwrap_or_else(|| "an unknown client".into());

    let scrubbed: Vec<(String, String)> = battery
        .iter()
        .map(|(k, v)| (me.scrub(k), me.scrub(v)))
        .collect();
    // The battery's parts taken, each merged in by its prefix.
    let of = |part: &str, key: &str| key.starts_with(&format!("{part}."));
    let mut battery_parts: Vec<&str> = Vec::new();
    for part in ["probe", "order", "protected", "blocked"] {
        if battery.iter().any(|(k, _)| of(part, k)) && gate.take(&stamps, part, &version).is_some()
        {
            battery_parts.push(part);
        }
    }
    if !battery_parts.is_empty() {
        let path = client.join("forever-camelot-engine.lua");
        let ours = |k: &str| battery_parts.iter().any(|p| of(p, k));
        let mut entries: Vec<(String, String)> = read_table(&path)
            .into_iter()
            .filter(|(k, _)| !ours(k))
            .collect();
        entries.extend(scrubbed.iter().filter(|(k, _)| ours(k)).cloned());
        entries.sort();
        write_table(
            &path,
            &format!(
                "WoW Forever {version}: /stdtest engine, order, protected and blocked\n\
             (tests/ingame/src/engineprobe.wowlua), flattened; the player's name, realm\n\
             and GUID replaced (cargo xtask ingest)."
            ),
            &entries,
        )?;
    }
    let probed = battery_parts.contains(&"probe");

    // The CVar flags, as the engine's data.
    let mut flags = String::from(&format!(
        "# The CVars C_CVar.GetCVarInfo flags on WoW Forever {version}\n\
         # (/stdtest engine, probe.cvarFlags): secure (addons may not set them),\n\
         # readOnly, locked (from the user); one per line, the flag then the name.\n"
    ));
    let mut any_flags = false;
    for kind in ["secure", "readOnly", "locked"] {
        if !probed {
            break;
        }
        if let Some(list) = get(&format!("probe.cvarFlags.{kind}")) {
            any_flags = true;
            for name in list.split_whitespace() {
                let _ = writeln!(flags, "{kind}\t{name}");
            }
        }
    }
    if any_flags {
        let path = root.join("crates/wowlua-engine/data/camelot-cvar-flags.txt");
        write_file(&path, &flags)?;
    }

    // The heights fonts keep after SetFont (the same for every file).
    if let Some(list) = get("probe.fontHeights.Fonts\\FRIZQT__.TTF").filter(|_| probed) {
        let mut out = format!(
            "# The heights a font keeps after SetFont on WoW Forever {version}, the same\n\
             # for every font file (/stdtest engine, probe.fontHeights; GetFont's\n\
             # height, GetFontInfo floors it); one per line: the height set, then kept.\n"
        );
        for entry in list.split_whitespace() {
            if let Some((set, rest)) = entry.split_once('=') {
                let kept = rest.split('/').next().unwrap_or(rest);
                let _ = writeln!(out, "{set}\t{kept}");
            }
        }
        let path = root.join("crates/wowlua-engine/data/camelot-font-heights.txt");
        write_file(&path, &out)?;
    }

    // The client's login, as engine data (`/stdtest engine startup`).
    for (key, file, part) in [
        ("engineStartupLogin", "camelot-login.txt", "login"),
        ("engineStartup", "camelot-reload.txt", "reload"),
    ] {
        let Ok(list) = state.get::<mlua::Table>(key) else {
            continue;
        };
        if gate.take(&stamps, part, &version).is_none() {
            continue;
        }
        let lines: Vec<String> = list
            .sequence_values::<String>()
            .collect::<Result<_, _>>()
            .map_err(|e| anyhow!("{e}"))?;
        // A recording without full payloads (an older StdTest) is skipped.
        if lines
            .iter()
            .any(|l| l.contains("ACTION_USABLE_CHANGED({1,"))
        {
            println!("skipped {key}: recorded without full payloads (record it again)");
            continue;
        }
        let text = login_data(&me, &version, &lines);
        let path = root.join("crates/wowlua-engine/data").join(file);
        write_file(&path, &text)?;
    }

    // The Graphics settings (`/stdtest engine graphics`, decision T561):
    // what each setting's levels and the overall quality set, as engine
    // data.
    // (each client's own: its flavor and version on the first line)
    if let Ok(list) = state.get::<mlua::Table>("engineGraphics") {
        let lines: Vec<String> = list
            .sequence_values::<String>()
            .collect::<Result<_, _>>()
            .map_err(|e| anyhow!("{e}"))?;
        let mut first = lines
            .first()
            .map(|l| l.split_whitespace())
            .into_iter()
            .flatten()
            .skip(1);
        let flavor = first.next().unwrap_or("camelot").to_string();
        let client = first.next().unwrap_or("").to_string();
        if ["camelot", "vanilla", "tbc", "mists"].contains(&flavor.as_str())
            && gate
                .take(&stamps, &format!("graphics-{flavor}"), &version)
                .is_some()
        {
            let mut text = format!(
                "# The {flavor} client {client}: what the Graphics settings set (decision\n\
                 # T561; `/stdtest engine graphics`, cargo xtask ingest): `setting=level`\n\
                 # then the CVars it set.\n"
            );
            for l in lines.iter().filter(|l| !l.starts_with("start ")) {
                text.push_str(l);
                text.push('\n');
            }
            write_file(
                &root.join(format!("crates/wowlua-engine/data/{flavor}-graphics.txt")),
                &text,
            )?;
        }
    }

    // The input recording, in order.
    if let Ok(input) = state.get::<mlua::Table>("engineInput")
        && gate.take(&stamps, "input", &version).is_some()
    {
        let mut out = format!(
            "-- WoW Forever {version}: /stdtest engine input\n\
             -- (tests/ingame/src/engineprobe.wowlua): hover the button, click it,\n\
             -- Shift-click it, click the box, type ab, press Enter.\nreturn {{\n"
        );
        for line in input.sequence_values::<String>() {
            let line = line.map_err(|e| anyhow!("{e}"))?;
            let _ = writeln!(out, "    {},", quote(&me.scrub(&line)));
        }
        out.push_str("}\n");
        let path = client.join("forever-camelot-input.lua");
        write_file(&path, &out)?;
    }

    // The character's world, as a test profile (`/stdtest snapshot`).
    if state.get::<mlua::Table>("snapshot").is_ok()
        && gate.take(&stamps, "snapshot", &version).is_some()
    {
        let tmp = tempfile::NamedTempFile::new()?;
        crate::profile::run(saved, tmp.path())?;
        let text = std::fs::read_to_string(tmp.path())?;
        let path = client.join("forever-camelot-character.json");
        // The character's last name, which the snapshot lacks (the full
        // probe's `UnitName("player")` gives one).
        let mut text = me.scrub(&text);
        if me.last_name.is_some() {
            text = with_last_name(&text);
        }
        write_file(&path, &text)?;
    }

    // The answers to the calls the engine could not answer (`/stdtest
    // engine answers`, its checks with them), the cursor's and keyboard's
    // steps, the map data, the account's collections and achievements;
    // the static functions' answers merged into the engine's data.
    let mut data_entries: Vec<(String, String)> = Vec::new();
    let mut answered: Vec<String> = Vec::new();
    if let Ok(answers) = state.get::<mlua::Table>("engineAnswers")
        && gate.take(&stamps, "answers", &version).is_some()
    {
        let mut entries = Vec::new();
        for pair in answers.pairs::<mlua::LuaString, mlua::LuaString>() {
            let (call, answer) = pair.map_err(|e| anyhow!("{e}"))?;
            // Text the client gives in the system's own code page (a sound
            // device's name on Windows) or binary, as `\xNN`.
            let call = escaped_bytes(&call.as_bytes());
            let answer = escaped_bytes(&answer.as_bytes());
            let answer = if SOCIAL.iter().any(|p| call.starts_with(p)) {
                blank_strings(&answer)
            } else {
                answer
            };
            entries.push((me.scrub(&call), me.scrub(&answer)));
        }
        entries.sort();
        for (call, answer) in &entries {
            let name = call.split('(').next().unwrap_or("");
            let listed = |list: &[&str]| {
                list.iter()
                    .any(|p| name == *p || (p.ends_with('.') && name.starts_with(p)))
            };
            if (listed(STATIC) || listed(BASELINE)) && !answer.starts_with("error") {
                data_entries.push((call.clone(), answer.clone()));
            }
        }
        answered = entries.iter().map(|(c, _)| c.clone()).collect();
        let path = client.join("forever-camelot-answers.lua");
        let all = merged(read_table(&path), &entries);
        write_table(
            &path,
            &format!(
                "WoW Forever {version}: /stdtest engine answers, the client's answers to the\n\
                 calls Blizzard's UI made in the engine's sweep that the engine could not\n\
                 answer (tests/ingame/src/answers.wowlua); the player scrubbed, the social\n\
                 systems' texts blanked (cargo xtask ingest)."
            ),
            &all,
        )?;
        // The checks of rules the call form cannot ask (its CHECKS), for
        // the engine's tests.
        if let Ok(checks) = state.get::<mlua::Table>("engineChecks") {
            let mut list = Vec::new();
            for pair in checks.pairs::<String, mlua::LuaString>() {
                let (label, answer) = pair.map_err(|e| anyhow!("{e}"))?;
                list.push((label, me.scrub(&escaped_bytes(&answer.as_bytes()))));
            }
            list.sort();
            write_table(
                &client.join("forever-camelot-checks.lua"),
                &format!(
                    "WoW Forever {version}: /stdtest engine answers' checks, the client's\n\
                     answers to rules the call form cannot ask (cargo xtask ingest)."
                ),
                &list,
            )?;
        }
    }
    // Every function called with many arguments (`/stdtest engine
    // fullprobe`), merged in.
    if let Ok(probe) = state.get::<mlua::Table>("engineFullProbe")
        && let Some(client_version) = gate.take(&stamps, "fullprobe", &version)
    {
        let path = client.join("forever-camelot-fullprobe.lua");
        let entries = fullprobe_entries(&me, &probe)?;
        let all = merged(read_table(&path), &entries);
        write_table(
            &path,
            &format!(
                "WoW Forever {client_version}: /stdtest engine fullprobe, every C function called\n\
                 with many arguments (tests/ingame/src/fullprobe.wowlua, its plan\n\
                 fullprobeplan.lua); the player, realm and guild scrubbed, the social\n\
                 systems' texts blanked (cargo xtask ingest)."
            ),
            &all,
        )?;
    }
    // Every widget method called on fresh widgets (`/stdtest engine
    // methodprobe`), merged in; scrubbed as the full probe (its own
    // answers name the player, the realm and the guild).
    if let Ok(probe) = state.get::<mlua::Table>("engineMethodProbe")
        && let Some(client_version) = gate.take(&stamps, "methodprobe", &version)
    {
        let path = client.join("forever-camelot-methodprobe.lua");
        let entries = fullprobe_entries(&me, &probe)?;
        let all = merged(read_table(&path), &entries);
        write_table(
            &path,
            &format!(
                "WoW Forever {client_version}: /stdtest engine methodprobe, every widget method\n\
                 called on fresh hidden widgets, setters read back\n\
                 (tests/ingame/src/methodprobe.wowlua, its plan methodprobeplan.lua); the\n\
                 player, realm and guild scrubbed (cargo xtask ingest)."
            ),
            &all,
        )?;
    }
    // The cursor (`/stdtest engine cursor`): each step's state and the
    // events between; the keyboard (`/stdtest engine keys`): which frames
    // each key reached, and the binding's clicks; in order.
    for (key, part, fixture, about) in [
        (
            "engineCursor",
            "cursor",
            "forever-camelot-cursor.lua",
            "/stdtest engine cursor, the cursor's steps and\nevents in order",
        ),
        (
            "engineKeys",
            "keys",
            "forever-camelot-keys.lua",
            "/stdtest engine keys, the keyboard frames each\nkey reached and the binding's clicks, in order",
        ),
    ] {
        if let Ok(log) = state.get::<mlua::Table>(key)
            && gate.take(&stamps, part, &version).is_some()
        {
            let mut list = Vec::new();
            for (i, line) in log.sequence_values::<String>().enumerate() {
                let line = line.map_err(|e| anyhow!("{e}"))?;
                list.push((format!("{:03}", i + 1), me.scrub(&line)));
            }
            write_table(
                &client.join(fixture),
                &format!("WoW Forever {version}: {about} (cargo xtask ingest)."),
                &list,
            )?;
            // The cursor's character, its own snapshot (another's would
            // have other spells and items).
            if part == "cursor"
                && let Ok(snapshot) = state.get::<mlua::Value>("engineCursorCharacter")
                && !snapshot.is_nil()
            {
                let (text, names) = crate::profile::profile_text(&state, &snapshot)?;
                let text = names
                    .iter()
                    .filter(|n| n.len() > 2)
                    .fold(me.scrub(&text), |t, n| t.replace(n.as_str(), "Player"));
                write_file(&client.join("forever-camelot-cursor-character.json"), &text)?;
            }
        }
    }
    // The map data (`/stdtest engine maps`, all static) and the account's
    // collections (`/stdtest engine collections`, the default world's)
    // and achievements.
    for (key, part, fixture, about) in [
        (
            "engineMapAnswers",
            "maps",
            "forever-camelot-maps.lua",
            "/stdtest engine maps, the client's map data (every map from\n\
             the world map and the player's map down) as Blizzard's UI asks it",
        ),
        (
            "engineCollectionAnswers",
            "collections",
            "forever-camelot-collections.lua",
            "/stdtest engine collections, the account's collections\n\
             (transmog appearances by category; mounts, pets, toys, heirlooms)",
        ),
        (
            "engineAchievementAnswers",
            "achievements",
            "forever-camelot-achievements.lua",
            "/stdtest engine achievements, the achievements (every category,\n\
             achievement, criterion, legacy points; the legacy reward track) as\n\
             Blizzard's UI asks them, who earned them scrubbed",
        ),
    ] {
        if let Ok(maps) = state.get::<mlua::Table>(key)
            && gate.take(&stamps, part, &version).is_some()
        {
            let mut map_entries = Vec::new();
            for pair in maps.pairs::<String, String>() {
                let (call, answer) = pair.map_err(|e| anyhow!("{e}"))?;
                let answer = scrub_earners(&call, &me.scrub(&answer));
                map_entries.push((call, answer));
            }
            map_entries.sort();
            for (call, answer) in &map_entries {
                if !answer.starts_with("error") && !answered.contains(call) {
                    data_entries.push((call.clone(), answer.clone()));
                }
            }
            write_table(
                &client.join(fixture),
                &format!("WoW Forever {version}: {about} (cargo xtask ingest)."),
                &map_entries,
            )?;
        }
    }
    // The engine's data: the repository's static answers, these added (a
    // run's answers win).
    if !data_entries.is_empty() {
        let data_dir = root.join("crates/wowlua-engine/data");
        let read = |path: &Path| -> std::collections::BTreeMap<String, String> {
            std::fs::read_to_string(path)
                .unwrap_or_default()
                .lines()
                .filter(|l| !l.starts_with('#'))
                .filter_map(|l| l.split_once('\t'))
                .map(|(c, a)| (c.to_string(), a.to_string()))
                .collect()
        };
        let path = data_dir.join("camelot-answers.txt");
        let mut all = read(&path);
        // The answers that differ by machine go to the recording machine's
        // file (decision T236): a call either platform's file holds.
        let platform = recorded_platform(&state);
        let machine_path = |p: &str| data_dir.join(format!("camelot-answers-{p}.txt"));
        let mac = read(&machine_path("mac"));
        let windows = read(&machine_path("windows"));
        let mut own = match platform {
            Some("mac") => mac.clone(),
            Some(_) => windows.clone(),
            None => Default::default(),
        };
        let mut unplaced = 0;
        for (call, answer) in &data_entries {
            let (call, answer) = (data_text(call), data_text(answer));
            if mac.contains_key(&call) || windows.contains_key(&call) {
                match platform {
                    Some(_) => {
                        own.insert(call, answer);
                    }
                    None => unplaced += 1,
                }
            } else {
                all.insert(call, answer);
            }
        }
        if unplaced > 0 {
            println!(
                "left out {unplaced} answers that differ by machine: the recording does not say its machine (IsMacClient)"
            );
        }
        if let Some(p) = platform {
            let mut data = format!(
                "# WoW Forever {version} on a {p} machine: the answers that differ by\n\
                 # machine (key names, graphics, sound, colours' last digits), consulted\n\
                 # before camelot-answers.txt when a test runs on this platform (decision\n\
                 # T236; cargo xtask ingest files a recording's under its platform).\n"
            );
            for (call, answer) in &own {
                let _ = writeln!(data, "{call}\t{answer}");
            }
            write_file(&machine_path(p), &data)?;
        }
        let mut data = format!(
            "# WoW Forever {version}'s answers to calls of functions that answer from the\n\
             # client's own data (/stdtest engine answers), as the engine gives them:\n\
             # the call, a tab, the answer (cargo xtask ingest).\n"
        );
        for (call, answer) in &all {
            let _ = writeln!(data, "{call}\t{answer}");
        }
        write_file(&path, &data)?;
    }
    // The trait configs (`/stdtest engine traits`), each class's as its
    // character answered them: its ranks, currencies and conditions.
    if let Ok(classes) = state.get::<mlua::Table>("engineTraitAnswers") {
        let mut any = false;
        for pair in classes.pairs::<String, mlua::Table>() {
            let (class, calls) = pair.map_err(|e| anyhow!("{e}"))?;
            if gate
                .take(&stamps, &format!("traits:{class}"), &version)
                .is_none()
            {
                continue;
            }
            any = true;
            let mut list = Vec::new();
            for pair in calls.pairs::<String, String>() {
                let (call, answer) = pair.map_err(|e| anyhow!("{e}"))?;
                list.push((call, me.scrub(&answer)));
            }
            list.sort();
            // A character of the class: its class config's ID names it (a
            // class may have several characters recorded; no name kept).
            let config = list
                .iter()
                .find(|(c, _)| c == "C_ClassTalents.GetActiveConfigID()")
                .and_then(|(_, a)| a.strip_prefix("1: number "))
                .unwrap_or("0")
                .to_string();
            write_table(
                &client.join(format!(
                    "forever-camelot-traits-{}-{config}.lua",
                    class.to_lowercase()
                )),
                &format!(
                    "WoW Forever {version}: /stdtest engine traits, a {class}'s trait configs\n\
                     (class talents, legacy trees) as Blizzard's UI asks them (cargo xtask ingest)."
                ),
                &list,
            )?;
        }
        if any {
            write_traits_data(root, &client, &version)?;
        }
    }
    // The professions' windows (`/stdtest engine tradeskill`), each one's
    // recipes as the character's skill and bags answered them.
    if let Ok(windows) = state.get::<mlua::Table>("engineTradeSkillAnswers") {
        let mut any = false;
        for pair in windows.pairs::<String, mlua::Value>() {
            let (profession, calls) = pair.map_err(|e| anyhow!("{e}"))?;
            // A run before the windows were kept apart: a call's answer.
            let mlua::Value::Table(calls) = calls else {
                continue;
            };
            if gate
                .take(&stamps, &format!("tradeskill:{profession}"), &version)
                .is_none()
            {
                continue;
            }
            any = true;
            let mut list = Vec::new();
            for pair in calls.pairs::<String, String>() {
                let (call, answer) = pair.map_err(|e| anyhow!("{e}"))?;
                list.push((call, me.scrub(&answer)));
            }
            list.sort();
            let file: String = profession
                .chars()
                .map(|c| {
                    if c.is_ascii_alphanumeric() {
                        c.to_ascii_lowercase()
                    } else {
                        '-'
                    }
                })
                .collect();
            write_table(
                &client.join(format!("forever-camelot-tradeskill-{file}.lua")),
                &format!(
                    "WoW Forever {version}: /stdtest engine tradeskill, the {profession} window\n\
                     (its recipes, categories, schematics) as Blizzard's UI asks it (cargo xtask ingest)."
                ),
                &list,
            )?;
        }
        if any {
            write_tradeskill_data(root, &client, &version)?;
        }
    }
    // The trainers (`/stdtest engine trainer`), each class's (or
    // profession's) services as the character's level and spells
    // answered them.
    if let Ok(trainers) = state.get::<mlua::Table>("engineTrainerAnswers") {
        let mut any = false;
        for pair in trainers.pairs::<String, mlua::Table>() {
            let (key, calls) = pair.map_err(|e| anyhow!("{e}"))?;
            if gate
                .take(&stamps, &format!("trainer:{key}"), &version)
                .is_none()
            {
                continue;
            }
            any = true;
            let mut list = Vec::new();
            for pair in calls.pairs::<String, String>() {
                let (call, answer) = pair.map_err(|e| anyhow!("{e}"))?;
                list.push((call, me.scrub(&answer)));
            }
            list.sort();
            let file: String = key
                .chars()
                .map(|c| {
                    if c.is_ascii_alphanumeric() {
                        c.to_ascii_lowercase()
                    } else {
                        '-'
                    }
                })
                .collect();
            write_table(
                &client.join(format!("forever-camelot-trainer-{file}.lua")),
                &format!(
                    "WoW Forever {version}: /stdtest engine trainer, the {key} trainer's services\n\
                     (every one shown) and the spells they teach (cargo xtask ingest)."
                ),
                &list,
            )?;
        }
        if any {
            let (lines, keys) = fixtures_by(&client, "forever-camelot-trainer-")?;
            let mut data = format!(
                "# WoW Forever {version}'s trainers as a character answered them\n\
                 # (/stdtest engine trainer: {}): the trainer, the call, the answer,\n\
                 # a tab between (cargo xtask ingest). Each class's services by level\n\
                 # and cost, the spells they teach.\n",
                keys.join(", ")
            );
            for ((key, call), answer) in &lines {
                let _ = writeln!(data, "{key}\t{}\t{}", data_text(call), data_text(answer));
            }
            let path = root.join("crates/wowlua-engine/data/camelot-trainers.txt");
            write_file(&path, &data)?;
        }
    }
    // The player's group (`/stdtest engine group`), a party's and a raid's
    // answers (the probe names other players Member1, …).
    if let Ok(groups) = state.get::<mlua::Table>("engineGroupAnswers") {
        let named =
            regex::Regex::new(r"string (Member\d+), string ([^,]+)$").expect("a valid pattern");
        for pair in groups.pairs::<String, mlua::Table>() {
            let (label, calls) = pair.map_err(|e| anyhow!("{e}"))?;
            if !matches!(label.as_str(), "party" | "raid")
                || gate
                    .take(&stamps, &format!("group:{label}"), &version)
                    .is_none()
            {
                continue;
            }
            let mut list = Vec::new();
            for pair in calls.pairs::<String, String>() {
                let (call, answer) = pair.map_err(|e| anyhow!("{e}"))?;
                list.push((call, me.scrub(&answer)));
            }
            // A member's last name the probe left (`Member1, <last>`):
            // `Member1Last`, wherever it appears.
            let lasts: Vec<(String, String)> = list
                .iter()
                .filter(|(call, _)| call.starts_with("UnitName("))
                .filter_map(|(_, answer)| named.captures(answer))
                .map(|c| (c[2].to_string(), format!("{}Last", &c[1])))
                .filter(|(last, hidden)| last != hidden)
                .collect();
            for (_, answer) in &mut list {
                for (last, hidden) in &lasts {
                    *answer = answer.replace(last.as_str(), hidden);
                }
            }
            list.sort();
            write_table(
                &client.join(format!("forever-camelot-group-{label}.lua")),
                &format!(
                    "WoW Forever {version}: /stdtest engine group, the player's {label}\n\
                     (the counts, every group token, the roster, roles, leader, nameplates,\n\
                     Blizzard's group frames; other players Member1, …) (cargo xtask ingest)."
                ),
                &list,
            )?;
        }
    }
    // Gameplay (`/stdtest engine scenario <name>`): each scenario's
    // events and answers, and the character's world before and after
    // as test profiles, the character's names scrubbed.
    if let Ok(scenarios) = state.get::<mlua::Table>("engineScenarios") {
        let dir = client.join("scenarios");
        std::fs::create_dir_all(&dir)?;
        let two_names =
            regex::Regex::new(r#"UnitName\(\\?"[a-z]+\\?"\) -> 2: string ([^,]+), string (.+)$"#)
                .expect("a valid pattern");
        let book = regex::Regex::new(r"isReadable=boolean true,[^}]*?itemID=number (\d+)")
            .expect("a valid pattern");
        // Other players' crafts in the combat log ("<name> creates <item>.").
        let crafter = regex::Regex::new(r"^string (.+) creates ").expect("a valid pattern");
        // Others' pets in the combat log tab ("<name>'s pet begins eating
        // ..."): the owner's name goes too (2026-10-20: one leaked).
        let pet_owner = regex::Regex::new(r"^string [^ ']+'s pet ").expect("a valid pattern");
        // Mail's players: an auction invoice's buyer or seller, the sender
        // of a letter one can answer (`canReply`, the header's twelfth).
        let invoice = regex::Regex::new(
            r"GetInboxInvoiceInfo\(\d+\) -> \d+: string [a-z]+, string [^,]*, string ([^,]+),",
        )
        .expect("a valid pattern");
        let letter = regex::Regex::new(
            r"GetInboxHeaderInfo\(\d+\) -> \d+: [^,]+, [^,]+, string ([^,]+), (?:[^,]+, ){8}boolean true,",
        )
        .expect("a valid pattern");
        // A friend's coming online or going offline names them (as a link
        // or plain).
        let friend = regex::Regex::new(
            r"string (?:\|Hplayer:[^|]*\|h\[)?([^\[\]|;]+?)(?:\]\|h)? has (?:gone offline|come online)\.",
        )
        .expect("a valid pattern");
        // Another player's invitation names them (its chat line too, as a
        // link).
        let inviter = regex::Regex::new(r"^[0-9.]+ PARTY_INVITE_REQUEST\(string ([^;]+);")
            .expect("a valid pattern");
        // First every recording's names (a name one recording shows
        // may be in another's chat), then each recording scrubbed of
        // them all.
        let mut all: Vec<(String, mlua::Table)> = Vec::new();
        for pair in scenarios.pairs::<String, mlua::Table>() {
            all.push(pair.map_err(|e| anyhow!("{e}"))?);
        }
        let mut secrets: Vec<String> = Vec::new();
        let mut profiles_of: Vec<Vec<(&str, String)>> = Vec::new();
        for (_, scenario) in &all {
            let mut profiles = Vec::new();
            for part in ["before", "after"] {
                let snapshot: mlua::Value = scenario.get(part).map_err(|e| anyhow!("{e}"))?;
                if snapshot.is_nil() {
                    continue;
                }
                let (text, names) = crate::profile::profile_text(&state, &snapshot)?;
                secrets.extend(names);
                profiles.push((part, text));
            }
            profiles_of.push(profiles);
        }
        // Players' names: a player's `UnitName` answers with two names
        // (first and last), an NPC's with one. The character's last
        // name, and others' names, as whole words.
        let mut words: Vec<(String, &str)> = Vec::new();
        // Friends, from the lines as recorded: their names may hold the
        // character's ("<name>tina"), which the scrub would cut apart.
        let mut friends: Vec<String> = Vec::new();
        for (_, scenario) in &all {
            let log: mlua::Table = scenario.get("log").map_err(|e| anyhow!("{e}"))?;
            for line in log.sequence_values::<String>() {
                let line = line.map_err(|e| anyhow!("{e}"))?;
                if let Some(c) = friend.captures(&line).or_else(|| inviter.captures(&line)) {
                    let other = c[1].trim().to_string();
                    let first = other.split(' ').next().unwrap_or("");
                    let own = first == me.name || secrets.iter().any(|s| s == first);
                    if !own && other.len() > 1 && !friends.contains(&other) {
                        friends.push(other);
                    }
                }
            }
        }
        // Longest first: a name inside another's goes after it.
        friends.sort_by_key(|f| std::cmp::Reverse(f.len()));
        for (_, scenario) in &all {
            let log: mlua::Table = scenario.get("log").map_err(|e| anyhow!("{e}"))?;
            for line in log.sequence_values::<String>() {
                let line = line.map_err(|e| anyhow!("{e}"))?;
                let line = me.scrub(&line);
                for re in [&invoice, &letter] {
                    if let Some(c) = re.captures(&line) {
                        let other = c[1].trim().to_string();
                        if other.len() > 1 && other != "Player" && !secrets.contains(&other) {
                            words.push((other, "Otherplayer"));
                        }
                    }
                }
                if let Some(c) = two_names.captures(&line) {
                    let (first, last) = (c[1].trim().to_string(), c[2].trim().to_string());
                    let own = first == "Player" || secrets.contains(&first);
                    if !own && first.len() > 1 {
                        words.push((first, "Otherplayer"));
                    }
                    if last.len() > 1 {
                        words.push((last, if own { "Lastname" } else { "Othername" }));
                    }
                }
            }
        }
        words.sort();
        words.dedup();
        for ((name, scenario), profiles) in all.iter().zip(profiles_of) {
            // Every recording's names gathered; the ones taken written.
            if gate
                .take(&stamps, &format!("scenario:{name}"), &version)
                .is_none()
            {
                continue;
            }
            let file: String = name
                .chars()
                .map(|c| {
                    if c.is_ascii_alphanumeric() {
                        c.to_ascii_lowercase()
                    } else {
                        '-'
                    }
                })
                .collect();
            let scrub = |text: &str| {
                let text = friends.iter().fold(text.to_string(), |t, f| {
                    let re = regex::Regex::new(&format!(r"\b{}\b", regex::escape(f)))
                        .expect("a valid pattern");
                    re.replace_all(&t, "Otherplayer").into_owned()
                });
                let text = secrets
                    .iter()
                    .filter(|s| s.len() > 2)
                    .fold(me.scrub(&text), |t, s| t.replace(s.as_str(), "Player"));
                words.iter().fold(text, |t, (w, as_)| {
                    // The character's last name may be a word of the
                    // client's texts ("Lua Taint"): only as a name.
                    let pattern = if *as_ == "Lastname" {
                        format!(r"(Player |string ){}\b", regex::escape(w))
                    } else {
                        format!(r"()\b{}\b", regex::escape(w))
                    };
                    let re = regex::Regex::new(&pattern).expect("a valid pattern");
                    re.replace_all(&t, format!("${{1}}{as_}")).into_owned()
                })
            };
            // The character's last name, as its chat shows it (the
            // snapshot has none to read).
            let log: mlua::Table = scenario.get("log").map_err(|e| anyhow!("{e}"))?;
            let last_name = log
                .sequence_values::<String>()
                .flatten()
                .any(|line| scrub(&line).contains("string Player Lastname;"));
            // Books (a bag item's `isReadable`), for snapshots before they
            // recorded it.
            let books: std::collections::BTreeSet<String> = log
                .sequence_values::<String>()
                .flatten()
                .flat_map(|line| {
                    book.captures_iter(&line)
                        .map(|c| c[1].to_string())
                        .collect::<Vec<_>>()
                })
                .collect();
            for (part, text) in profiles {
                let path = dir.join(format!("{file}-{part}.json"));
                let mut text = scrub(&text);
                if last_name {
                    text = with_last_name(&text);
                }
                if !books.is_empty() {
                    text = with_books(&text, &books);
                }
                write_file(&path, &text)?;
            }
            let mut list = Vec::new();
            for (i, line) in log.sequence_values::<String>().enumerate() {
                let line = line.map_err(|e| anyhow!("{e}"))?;
                list.push((format!("{:05}", i + 1), scrub_owners(&scrub(&line))));
            }
            // The ping at the start and the end (`GetNetStats()`:
            // bandwidth in, out, latency home, world, in ms).
            for key in ["latency", "latencyAfter"] {
                if let Ok(stats) = scenario.get::<mlua::Table>(key) {
                    let values: Vec<String> = stats
                        .sequence_values::<f64>()
                        .filter_map(|v| v.ok())
                        .map(|v| format!("{v}"))
                        .collect();
                    list.push((key.to_string(), values.join(", ")));
                }
            }
            // The session's clock as the recording began (`GetTime()`): the
            // answers' times are the session's.
            if let Ok(start) = scenario.get::<f64>("startTime") {
                list.push(("startTime".to_string(), format!("{start}")));
            }
            // The combat log tab's lines at the end; other players' crafts
            // without their names.
            if let Ok(shown) = scenario.get::<mlua::Table>("combatLog") {
                for (i, line) in shown.sequence_values::<String>().enumerate() {
                    let line = line.map_err(|e| anyhow!("{e}"))?;
                    let line = crafter.replace(&line, "string Someone creates ");
                    let line = pet_owner.replace(&line, "string Someone's pet ");
                    list.push((format!("log{:04}", i + 1), scrub(&line)));
                }
            }
            write_table(
                &dir.join(format!("{file}.lua")),
                &format!(
                    "WoW Forever {version}: /stdtest engine scenario {file}: its events in\n\
                     order, and after the events Blizzard's UI answers, `= call -> answer`\n\
                     (cargo xtask ingest); the world before and after: {file}-before.json,\n\
                     {file}-after.json."
                ),
                &list,
            )?;
        }
    }
    Ok(())
}

/// The machine a recording was made on, by its answers battery's
/// `IsMacClient()`: `"mac"` or `"windows"` (decision T236).
fn recorded_platform(state: &mlua::Table) -> Option<&'static str> {
    let answer: String = state
        .get::<mlua::Table>("engineAnswers")
        .ok()?
        .get("IsMacClient()")
        .ok()?;
    match answer.as_str() {
        "1: boolean true" => Some("mac"),
        "1: boolean false" => Some("windows"),
        _ => None,
    }
}

/// A recorded answer as text: UTF-8 as it is, any other byte (a binary
/// answer, `C_EncodingUtil.SerializeCBOR`'s) as `\xNN`.
fn escaped_bytes(bytes: &[u8]) -> String {
    if let Ok(s) = std::str::from_utf8(bytes) {
        return s.to_string();
    }
    let mut out = String::new();
    for chunk in bytes.utf8_chunks() {
        out.push_str(chunk.valid());
        for b in chunk.invalid() {
            out.push_str(&format!("\\x{b:02x}"));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn gate(dir: &Path, unstamped: bool) -> Gate {
        std::fs::create_dir_all(dir.join("crates/wowlua-engine/tests/client")).unwrap();
        Gate::load(dir, unstamped).unwrap()
    }

    /// A part is taken only when stamped later than the repository's; an
    /// unstamped one only with `--unstamped`, and any stamp beats it.
    #[test]
    fn a_part_is_taken_only_when_newer() {
        let dir = tempfile::tempdir().unwrap();
        let mut g = gate(dir.path(), false);
        let at = |t: &str| -> Stamps {
            [(
                "answers".to_string(),
                (t.to_string(), "1.60.1 (70450)".to_string()),
            )]
            .into_iter()
            .collect()
        };
        assert!(g.take(&Stamps::new(), "answers", "v").is_none());
        assert!(g.take(&at("2026-10-19 12:00:00"), "answers", "v").is_some());
        assert!(g.take(&at("2026-10-19 12:00:00"), "answers", "v").is_none());
        assert!(g.take(&at("2026-10-19 11:00:00"), "answers", "v").is_none());
        assert!(g.take(&at("2026-10-19 13:00:00"), "answers", "v").is_some());
        g.save().unwrap();
        // Saved and loaded again; an unstamped part with the flag, then
        // beaten by any stamp.
        let mut g = gate(dir.path(), true);
        assert!(g.take(&at("2026-10-19 13:00:00"), "answers", "v").is_none());
        assert!(g.take(&Stamps::new(), "maps", "v").is_some());
        let maps: Stamps = [(
            "maps".to_string(),
            ("2026-01-01 00:00:00".to_string(), "c".to_string()),
        )]
        .into_iter()
        .collect();
        assert!(g.take(&maps, "maps", "v").is_some());
    }

    /// The default world's parts come from its account only.
    #[test]
    fn the_default_world_comes_from_its_account() {
        let path = Path::new("/WoW/_classic_beta_/WTF/Account/123#2/SavedVariables/StdTest.lua");
        assert_eq!(account_of(path).as_deref(), Some("2"));
        assert_eq!(account_of(Path::new("/tmp/StdTest.lua")), None);
        let dir = tempfile::tempdir().unwrap();
        let mut g = gate(dir.path(), false);
        g.account = Some("2".into());
        let stamped = |part: &str| -> Stamps {
            [(part.to_string(), ("2026-10-19 12:00:00".into(), "c".into()))]
                .into_iter()
                .collect()
        };
        assert!(g.take(&stamped("answers"), "answers", "v").is_none());
        assert!(
            g.take(&stamped("trainer:DRUID-3060"), "trainer:DRUID-3060", "v")
                .is_some()
        );
        g.world_account = "2".into();
        assert!(g.take(&stamped("answers"), "answers", "v").is_some());
    }

    /// A file whose content is the same keeps its header; answers merge,
    /// none lost, the run's winning.
    #[test]
    fn writes_keep_unchanged_files_and_merge_answers() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("f.lua");
        write_table(&path, "WoW Forever old", &[("a".into(), "1".into())]).unwrap();
        write_table(&path, "WoW Forever new", &[("a".into(), "1".into())]).unwrap();
        assert!(
            std::fs::read_to_string(&path)
                .unwrap()
                .starts_with("-- WoW Forever old")
        );
        let all = merged(
            read_table(&path),
            &[("b".into(), "2".into()), ("a".into(), "3".into())],
        );
        assert_eq!(
            all,
            vec![("a".into(), "3".into()), ("b".into(), "2".into())]
        );
    }

    /// The method probe alone names the player too (its first answers);
    /// a widget's answer loses the player's name, GUID and realm.
    #[test]
    fn the_method_probe_is_scrubbed() {
        let lua = mlua::Lua::new();
        let state: mlua::Table = lua
            .load(
                r#"return { engineMethodProbe = {
                    ['UnitName("player")'] = "2: string Tamsin, string Ashford",
                    ['UnitGUID("player")'] = "1: string Player-1234-" .. "0ABCDEF1",
                    ['GetRealmName()'] = "1: string Moonfall Vale",
                    ['FontStringWithFont:SetText(UnitName("player")) -> GetText()'] = "0:  || GetText(): 1: string Tamsin",
                    ['Frame:SetAttribute("x", UnitGUID("player")) -> GetAttribute(#1)'] = "0:  || GetAttribute(#1): 1: string Player-1234-" .. "0ABCDEF1 of Moonfall Vale",
                } }"#,
            )
            .eval()
            .unwrap();
        let me = identity_from_fullprobe(&state).expect("the player");
        assert_eq!(me.name, "Tamsin");
        let probe: mlua::Table = state.get("engineMethodProbe").unwrap();
        let entries = fullprobe_entries(&me, &probe).unwrap();
        let all = format!("{entries:?}");
        for leak in ["Tamsin", "Ashford", "Moonfall", "0ABCDEF1"] {
            assert!(!all.contains(leak), "{leak} in {all}");
        }
        assert!(all.contains("GetText(): 1: string Player"), "{all}");
    }

    /// The full probe names the player itself; its answers lose the
    /// player, the realm, the guild and other players' GUIDs.
    #[test]
    fn the_full_probe_is_scrubbed() {
        let lua = mlua::Lua::new();
        let state: mlua::Table = lua
            .load(
                r#"return { engineFullProbe = {
                    ['UnitName("player")'] = "2: string Tamsin, string Ashford",
                    ['UnitGUID("player")'] = "1: string Player-1234-" .. "0ABCDEF1",
                    ['GetRealmName()'] = "1: string Moonfall Vale",
                    ['GetNormalizedRealmName()'] = "1: string MoonfallVale",
                    ['GetGuildInfo("player")'] = "3: string Harbor Watch, string Member, number 2",
                    ['UnitFullName("player")'] = "2: string Tamsin, string MoonfallVale",
                    ['C_FriendList.GetFriendInfoByIndex(1)'] = "1: {name=string Otherfriend}",
                    ['UnitGUID("target")'] = "1: string Player-99-" .. "0FEDCBA9",
                    ['StdTestCrash()'] = "crashed",
                    ['UnitName("party1")'] = "2: string Partyone, string Partytwo",
                    ['GetRaidRosterInfo(1)'] = "12: string Partyone, number 0",
                    ['C_ChatInfo.GetChatLineSenderName(1)'] = "1: string Partyone",
                    ['GetQuestLogTitle(1)'] = "1: string |Hplayer:Partyone-Partytwo:12|h[Partyone]|h and |Hplayer:Tamsin|h",
                } }"#,
            )
            .eval()
            .unwrap();
        let me = identity_from_fullprobe(&state).expect("the player");
        assert_eq!(me.name, "Tamsin");
        let probe: mlua::Table = state.get("engineFullProbe").unwrap();
        let entries: std::collections::BTreeMap<String, String> = fullprobe_entries(&me, &probe)
            .unwrap()
            .into_iter()
            .collect();
        let all = format!("{entries:?}");
        for leak in [
            "Tamsin",
            "Ashford",
            "Moonfall",
            "Harbor Watch",
            "0ABCDEF1",
            "0FEDCBA9",
            "Otherfriend",
            "Partyone",
            "Partytwo",
        ] {
            assert!(!all.contains(leak), "{leak} in {all}");
        }
        assert_eq!(entries["GetRealmName()"], "1: string Realm");
        assert_eq!(entries["StdTestCrash()"], "crashed");
        assert_eq!(
            entries["UnitName(\"party1\")"],
            "2: string <scrubbed>, string <scrubbed>"
        );
        assert_eq!(
            entries["GetRaidRosterInfo(1)"],
            "12: string <scrubbed>, number 0"
        );
        assert_eq!(
            entries["GetQuestLogTitle(1)"],
            "1: string |Hplayer:Otherplayer:12|h[Otherplayer]|h and |Hplayer:Player|h"
        );
    }

    #[test]
    fn who_earned_an_achievement_is_scrubbed() {
        let info = "15: number 6, string Level 10, number 10, boolean true, number 10, \
                    number 18, number 26, string Reach level 10, number 0, number 236376, \
                    string , boolean false, boolean false, string Tamsin, boolean false";
        let scrubbed = scrub_earners("GetAchievementInfo(6)", info);
        assert!(scrubbed.ends_with("boolean false, string Player, boolean false"));
        assert!(!scrubbed.contains("Tamsin"));
        let criterion = "11: string Reach level 10, number 0, boolean true, number 10, \
                         number 10, string Tamsin, number 0, number 0, string 10, number 1, boolean true";
        assert!(!scrub_earners("GetAchievementCriteriaInfo(6, 1)", criterion).contains("Tamsin"));
        // Not earned: no name to scrub.
        let open = info.replace("string Tamsin", "string ");
        assert_eq!(scrub_earners("GetAchievementInfo(6)", &open), open);
    }
}
