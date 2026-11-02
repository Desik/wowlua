//! The long tail of `C_` namespaces no other module models: the tooltip
//! data of things the world holds (a spellbook slot, an action, a
//! merchant's goods, a unit's aura…) from the getters that already answer
//! them; who the player interacts with; a trainer's settings; races and
//! creature GUIDs; text helpers (`C_Intl`); and the "nothing here" answers
//! of the systems WoW Forever lacks (garrisons, commentator, housing,
//! artifacts, soulbinds, azerite, the shop, WoW Labs…).
//!
//! A documented function of those systems answers its documentation's
//! empty values (`crate::defaults`: nil where nilable, false, 0, "", an
//! empty table, a structure of empty fields, nothing when it may return
//! nothing); an undocumented one (the client has `C_Garrison` and others
//! without documentation) answers by its kind, as `UNDOCUMENTED` lists:
//! false for a question, 0 for a count, an empty table for a list, nil for
//! the rest of the getters, nothing for an action.
//!
//! Unverified unless a comment says otherwise: Forever's own answers to
//! these were not recorded.

use std::rc::Rc;

use wowlua_vm::stdlib::Ret;
use wowlua_vm::{NativeCall, NativeFn, TableRef, Value, Vm};

use crate::units;
use crate::{engine, engine_mut, widgets};

// ------------------------------------------------------------- helpers

/// What the engine answers for the client function `name` called with
/// `args` (its behaviour; none: nothing). Secret answers are read as the
/// client's C code reads them, unwrapped (as `conditionals::ask`).
fn ask(vm: &mut Vm, name: &'static str, args: &[Value]) -> Vec<Value> {
    let Some(f) = engine(vm).behaviour(name) else {
        return vec![];
    };
    let full = vm.str(name);
    let call = NativeCall {
        args: args.iter().copied().collect(),
        taints: args.iter().map(|_| Default::default()).collect(),
        origins: args.iter().map(|_| Default::default()).collect(),
        upvals: Rc::from([full]),
        upval_taints: Rc::from([Default::default()]),
    };
    match f(vm, call) {
        Ok(values) => values.into_iter().map(|v| vm.hidden(v)).collect(),
        Err(_) => vec![],
    }
}

fn num(v: Value) -> Option<f64> {
    match v {
        Value::Num(n) => Some(n),
        _ => None,
    }
}

/// An argument as a number (a secret one unwrapped).
fn arg_num(vm: &Vm, c: &NativeCall, n: usize) -> Option<f64> {
    num(vm.hidden(c.arg(n)))
}

/// An argument as text (a secret one unwrapped).
fn arg_text(vm: &Vm, c: &NativeCall, n: usize) -> Option<String> {
    match vm.hidden(c.arg(n)) {
        Value::Str(s) => Some(String::from_utf8_lossy(vm.str_bytes(s)).into_owned()),
        Value::Num(n) if n.fract() == 0.0 && n.abs() < 1e15 => Some(format!("{}", n as i64)),
        Value::Num(n) => Some(format!("{n}")),
        _ => None,
    }
}

fn field(vm: &mut Vm, t: TableRef, k: &str) -> Value {
    let key = vm.str(k);
    vm.heap.table(t).get(key)
}

fn set(vm: &mut Vm, t: TableRef, k: &str, v: Value) {
    let key = vm.str(k);
    let _ = vm.heap.table_mut(t).set(key, v);
}

/// This module's settings (`SetCategorizeTrainerUI`, the name plates'
/// insets, the combat text's unit): a table the engine keeps for the VM's
/// life, among its compiled helpers' values.
fn state(vm: &mut Vm) -> TableRef {
    if let Some(Value::Table(t)) = engine(vm).lua_helpers.get("extras.state").copied() {
        return t;
    }
    let t = vm.new_table();
    widgets::keep(vm, Value::Table(t));
    engine_mut(vm)
        .lua_helpers
        .insert("extras.state", Value::Table(t));
    t
}

fn state_get(vm: &mut Vm, k: &str) -> Value {
    let t = state(vm);
    field(vm, t, k)
}

fn state_set(vm: &mut Vm, k: &str, v: Value) {
    let t = state(vm);
    set(vm, t, k, v);
}

// ---------------------------------------------------- "nothing here"

/// Undocumented functions of systems Forever lacks and their answers by
/// kind: `f` false, `0` zero, `t` an empty table, `s` an empty string, `n`
/// a nil each, empty nothing (unverified: named as their kind; Blizzard's
/// UI reads the lists `C_Garrison.GetFollowers(…) or {}`, the counts as
/// numbers, `GetLandingPageGarrisonType()` against 0).
///
/// `C_Garrison`, `C_CharacterServices` and `Kiosk` answer as the full
/// probe recorded them (WoW Forever 1.60.1): most of their getters give
/// nothing at all, whatever they are asked (no garrison; the character
/// services and the kiosk belong to the login screens), a few an empty
/// string, list or 0. The probe crashed in `C_Garrison` before its
/// getters after `GetShipmentItemInfo` (and skipped those before
/// `GetFollowerAbilityIsTrait`): those of them named as giving a nil give
/// nothing as all 27 recorded ones did (unverified).
const UNDOCUMENTED: &[(&str, &str)] = &[
    ("C_AdventureJournal.ActivateEntry", ""),
    ("C_AdventureJournal.GetNumAvailableSuggestions", "0"),
    ("C_AdventureJournal.GetPrimaryOffset", "0"),
    ("C_AdventureJournal.GetReward", "n"),
    ("C_AdventureJournal.GetSuggestions", "t"),
    ("C_AdventureJournal.SetPrimaryOffset", ""),
    ("C_AdventureJournal.UpdateSuggestions", ""),
    ("C_AdventureMap.Close", ""),
    ("C_AdventureMap.GetMapID", "n"),
    ("C_AdventureMap.GetMapInsetDetailTileInfo", "n"),
    ("C_AdventureMap.GetMapInsetInfo", "n"),
    ("C_AdventureMap.GetQuestInfo", "n"),
    ("C_AdventureMap.GetQuestOfferInfo", "n"),
    ("C_AdventureMap.GetZoneChoiceInfo", "n"),
    ("C_AdventureMap.StartQuest", ""),
    ("C_ArrowCalloutManager.AcknowledgeCallout", ""),
    ("C_ArrowCalloutManager.HideCallout", ""),
    ("C_BlackMarket.Close", ""),
    ("C_BlackMarket.GetHotItem", "n"),
    ("C_BlackMarket.GetItemInfoByID", "n"),
    ("C_BlackMarket.GetItemInfoByIndex", "n"),
    ("C_BlackMarket.GetNumItems", "0"),
    ("C_BlackMarket.IsViewOnly", "f"),
    ("C_BlackMarket.ItemPlaceBid", ""),
    ("C_BlackMarket.RequestItems", ""),
    (
        "C_CharacterServices.ArePaidCharacterTransfersBetweenBnetAccountsEnabled",
        "",
    ),
    ("C_CharacterServices.AssignFCMDistribution", ""),
    ("C_CharacterServices.AssignHCTDistribution", ""),
    ("C_CharacterServices.AssignNameChangeDistribution", ""),
    ("C_CharacterServices.AssignPCTDistribution", ""),
    (
        "C_CharacterServices.AssignRaceOrFactionChangeDistribution",
        "",
    ),
    ("C_CharacterServices.AssignUpgradeDistribution", ""),
    ("C_CharacterServices.GetActiveCharacterUpgradeBoostType", ""),
    ("C_CharacterServices.GetActiveClassTrialBoostType", ""),
    ("C_CharacterServices.GetAutomaticBoost", ""),
    ("C_CharacterServices.GetAutomaticBoostCharacter", ""),
    ("C_CharacterServices.GetCharacterServiceDisplayData", ""),
    (
        "C_CharacterServices.GetCharacterServiceDisplayDataByVASType",
        "",
    ),
    ("C_CharacterServices.GetCharacterServiceDisplayInfo", ""),
    (
        "C_CharacterServices.GetHardcoreTransferSuperDistrictDestinations",
        "",
    ),
    (
        "C_CharacterServices.GetPaidCharacterTransferSuperDistrictDestinations",
        "",
    ),
    ("C_CharacterServices.GetVASDistributions", ""),
    ("C_CharacterServices.HasRequiredBoostForClassTrial", ""),
    ("C_CharacterServices.HasRequiredBoostForUnrevoke", ""),
    ("C_CharacterServices.IsRealmlessRealmSelectionEnabled", ""),
    ("C_CharacterServices.SetAutomaticBoost", ""),
    ("C_CharacterServices.SetAutomaticBoostCharacter", ""),
    ("C_CharacterServicesPublic.ShouldSeeControlPopup", "f"),
    ("C_ClassTrial.GetClassTrialLogoutTimeSeconds", "0"),
    ("C_ClassTrial.IsClassTrialCharacter", "f"),
    ("C_Garrison.AllowMissionStartAboveSoftCap", "f"),
    ("C_Garrison.AreMissionFollowerRequirementsMet", "f"),
    ("C_Garrison.AssignFollowerToBuilding", ""),
    ("C_Garrison.CanGenerateRecruits", "f"),
    ("C_Garrison.CanOpenMissionChest", "f"),
    ("C_Garrison.CanSetRecruitmentPreference", "f"),
    ("C_Garrison.CanSpellTargetFollowerIDWithAddAbility", "f"),
    ("C_Garrison.CanUpgradeGarrison", "f"),
    ("C_Garrison.CancelConstruction", ""),
    ("C_Garrison.CastItemSpellOnFollowerAbility", ""),
    ("C_Garrison.CastSpellOnFollower", ""),
    ("C_Garrison.CastSpellOnFollowerAbility", ""),
    ("C_Garrison.CastSpellOnMission", ""),
    ("C_Garrison.ClearCompleteTalent", ""),
    ("C_Garrison.CloseArchitect", ""),
    ("C_Garrison.CloseGarrisonTradeskillNPC", ""),
    ("C_Garrison.CloseMissionNPC", ""),
    ("C_Garrison.CloseRecruitmentNPC", ""),
    ("C_Garrison.CloseTalentNPC", ""),
    ("C_Garrison.CloseTradeskillCrafter", ""),
    ("C_Garrison.GenerateRecruits", ""),
    ("C_Garrison.GetAllBonusAbilityEffects", "t"),
    ("C_Garrison.GetAllEncounterThreats", "t"),
    ("C_Garrison.GetAvailableMissions", ""),
    ("C_Garrison.GetAvailableRecruits", "t"),
    ("C_Garrison.GetBasicMissionInfo", ""),
    ("C_Garrison.GetBuffedFollowersForMission", "t"),
    ("C_Garrison.GetBuildingInfo", ""),
    ("C_Garrison.GetBuildingLockInfo", ""),
    ("C_Garrison.GetBuildingSizes", "t"),
    ("C_Garrison.GetBuildingSpecInfo", ""),
    ("C_Garrison.GetBuildingTimeRemaining", ""),
    ("C_Garrison.GetBuildingTooltip", ""),
    ("C_Garrison.GetBuildingUpgradeInfo", ""),
    ("C_Garrison.GetBuildings", "t"),
    ("C_Garrison.GetBuildingsForPlot", "t"),
    ("C_Garrison.GetBuildingsForSize", "t"),
    ("C_Garrison.GetClassSpecCategoryInfo", "t"),
    ("C_Garrison.GetCombatAllyMission", ""),
    ("C_Garrison.GetCompleteMissions", "t"),
    ("C_Garrison.GetCompleteTalent", ""),
    ("C_Garrison.GetCurrencyTypes", ""),
    ("C_Garrison.GetFollowerAbilities", "t"),
    ("C_Garrison.GetFollowerAbilityAtIndex", ""),
    ("C_Garrison.GetFollowerAbilityAtIndexByID", ""),
    ("C_Garrison.GetFollowerAbilityCounterMechanicInfo", ""),
    ("C_Garrison.GetFollowerAbilityCountersForMechanicTypes", "t"),
    ("C_Garrison.GetFollowerAbilityDescription", ""),
    ("C_Garrison.GetFollowerAbilityIcon", ""),
    ("C_Garrison.GetFollowerAbilityInfo", ""),
    ("C_Garrison.GetFollowerAbilityIsTrait", "f"),
    ("C_Garrison.GetFollowerAbilityLink", "s"),
    ("C_Garrison.GetFollowerAbilityName", "n"),
    ("C_Garrison.GetFollowerActivationCost", "0"),
    ("C_Garrison.GetFollowerBiasForMission", "0"),
    ("C_Garrison.GetFollowerClassSpec", "n"),
    ("C_Garrison.GetFollowerClassSpecAtlas", "s"),
    ("C_Garrison.GetFollowerClassSpecByID", ""),
    ("C_Garrison.GetFollowerClassSpecName", ""),
    ("C_Garrison.GetFollowerDisplayID", "n"),
    ("C_Garrison.GetFollowerInfo", ""),
    ("C_Garrison.GetFollowerInfoForBuilding", ""),
    ("C_Garrison.GetFollowerIsTroop", ""),
    ("C_Garrison.GetFollowerItemLevelAverage", "n"),
    ("C_Garrison.GetFollowerItems", "n"),
    ("C_Garrison.GetFollowerLevel", "n"),
    ("C_Garrison.GetFollowerLevelXP", "n"),
    ("C_Garrison.GetFollowerLink", "n"),
    ("C_Garrison.GetFollowerLinkByID", ""),
    ("C_Garrison.GetFollowerMissionTimeLeft", "n"),
    ("C_Garrison.GetFollowerMissionTimeLeftSeconds", "n"),
    ("C_Garrison.GetFollowerModelItems", "t"),
    ("C_Garrison.GetFollowerName", "n"),
    ("C_Garrison.GetFollowerNameByID", ""),
    ("C_Garrison.GetFollowerPortraitIconID", "n"),
    ("C_Garrison.GetFollowerPortraitIconIDByID", ""),
    ("C_Garrison.GetFollowerQuality", "n"),
    ("C_Garrison.GetFollowerQualityTable", "t"),
    ("C_Garrison.GetFollowerRecentlyGainedAbilityIDs", "t"),
    ("C_Garrison.GetFollowerRecentlyGainedTraitIDs", "t"),
    ("C_Garrison.GetFollowerShipments", ""),
    ("C_Garrison.GetFollowerSoftCap", "0"),
    ("C_Garrison.GetFollowerSourceTextByID", ""),
    ("C_Garrison.GetFollowerSpecializationAtIndex", "n"),
    ("C_Garrison.GetFollowerStatus", "n"),
    ("C_Garrison.GetFollowerTraitAtIndex", "n"),
    ("C_Garrison.GetFollowerTraitAtIndexByID", ""),
    ("C_Garrison.GetFollowerTypeByID", ""),
    ("C_Garrison.GetFollowerTypeByMissionID", ""),
    ("C_Garrison.GetFollowerUnderBiasReason", "n"),
    ("C_Garrison.GetFollowerXP", "n"),
    ("C_Garrison.GetFollowerXPTable", "t"),
    ("C_Garrison.GetFollowerZoneSupportAbilities", ""),
    ("C_Garrison.GetFollowers", ""),
    ("C_Garrison.GetFollowersSpellsForMission", ""),
    ("C_Garrison.GetFollowersTraitsForMission", ""),
    ("C_Garrison.GetGarrisonInfo", ""),
    ("C_Garrison.GetGarrisonUpgradeCost", "n"),
    ("C_Garrison.GetInProgressMissions", ""),
    ("C_Garrison.GetLandingPageGarrisonType", "0"),
    ("C_Garrison.GetLandingPageItems", ""),
    ("C_Garrison.GetLandingPageShipmentCount", "0"),
    ("C_Garrison.GetLandingPageShipmentInfo", ""),
    ("C_Garrison.GetLandingPageShipmentInfoByContainerID", ""),
    ("C_Garrison.GetLooseShipments", ""),
    ("C_Garrison.GetMissionBonusAbilityEffects", ""),
    ("C_Garrison.GetMissionCost", ""),
    ("C_Garrison.GetMissionDisplayIDs", ""),
    ("C_Garrison.GetMissionLink", ""),
    ("C_Garrison.GetMissionMaxFollowers", "0"),
    ("C_Garrison.GetMissionName", "s"),
    ("C_Garrison.GetMissionRewardInfo", ""),
    ("C_Garrison.GetMissionSuccessChance", "0"),
    ("C_Garrison.GetMissionTexture", ""),
    ("C_Garrison.GetMissionTimes", ""),
    ("C_Garrison.GetMissionUncounteredMechanics", ""),
    ("C_Garrison.GetNumActiveFollowers", ""),
    ("C_Garrison.GetNumFollowerActivationsRemaining", ""),
    ("C_Garrison.GetNumFollowerDailyActivations", "0"),
    ("C_Garrison.GetNumFollowers", "0"),
    ("C_Garrison.GetNumFollowersForMechanic", "0"),
    ("C_Garrison.GetNumFollowersOnMission", ""),
    ("C_Garrison.GetNumPendingShipments", ""),
    ("C_Garrison.GetNumShipmentCurrencies", "0"),
    ("C_Garrison.GetNumShipmentReagents", "0"),
    ("C_Garrison.GetOwnedBuildingInfo", ""),
    ("C_Garrison.GetOwnedBuildingInfoAbbrev", ""),
    ("C_Garrison.GetPartyBuffs", ""),
    ("C_Garrison.GetPartyMentorLevels", ""),
    ("C_Garrison.GetPartyMissionInfo", ""),
    ("C_Garrison.GetPendingShipmentInfo", ""),
    ("C_Garrison.GetPlots", "t"),
    ("C_Garrison.GetPlotsForBuilding", "t"),
    ("C_Garrison.GetPossibleFollowersForBuilding", "t"),
    ("C_Garrison.GetRecruitAbilities", ""),
    ("C_Garrison.GetRecruiterAbilityCategories", "t"),
    ("C_Garrison.GetRecruiterAbilityList", "t"),
    ("C_Garrison.GetRecruitmentPreferences", ""),
    ("C_Garrison.GetShipDeathAnimInfo", "n"),
    ("C_Garrison.GetShipmentContainerInfo", "nn"),
    ("C_Garrison.GetShipmentItemInfo", ""),
    ("C_Garrison.GetShipmentReagentCurrencyInfo", ""),
    ("C_Garrison.GetShipmentReagentInfo", ""),
    ("C_Garrison.GetShipmentReagentItemLink", ""),
    ("C_Garrison.GetSpecChangeCost", ""),
    ("C_Garrison.GetTabForPlot", ""),
    ("C_Garrison.HasGarrison", "f"),
    ("C_Garrison.HasShipyard", "f"),
    ("C_Garrison.IsAboveFollowerSoftCap", "f"),
    ("C_Garrison.IsFollowerCollected", "f"),
    ("C_Garrison.IsInvasionAvailable", "f"),
    ("C_Garrison.IsMechanicFullyCountered", "f"),
    ("C_Garrison.IsOnGarrisonMap", "f"),
    ("C_Garrison.IsOnShipmentQuestForNPC", "f"),
    ("C_Garrison.IsOnShipyardMap", "f"),
    ("C_Garrison.IsPlayerInGarrison", "f"),
    ("C_Garrison.IsUsingPartyGarrison", "f"),
    ("C_Garrison.IsVisitGarrisonAvailable", "f"),
    ("C_Garrison.MarkMissionComplete", ""),
    ("C_Garrison.MissionBonusRoll", ""),
    ("C_Garrison.PlaceBuilding", ""),
    ("C_Garrison.RecruitFollower", ""),
    ("C_Garrison.RemoveFollower", ""),
    ("C_Garrison.RemoveFollowerFromBuilding", ""),
    ("C_Garrison.RenameFollower", ""),
    ("C_Garrison.RequestClassSpecCategoryInfo", ""),
    ("C_Garrison.RequestGarrisonUpgradeable", ""),
    ("C_Garrison.RequestLandingPageShipmentInfo", ""),
    ("C_Garrison.RequestShipmentCreation", ""),
    ("C_Garrison.RequestShipmentInfo", ""),
    ("C_Garrison.ResearchTalent", ""),
    ("C_Garrison.SearchForFollower", "f"),
    ("C_Garrison.SetBuildingActive", ""),
    ("C_Garrison.SetBuildingSpecialization", ""),
    ("C_Garrison.SetFollowerFavorite", ""),
    ("C_Garrison.SetFollowerInactive", ""),
    ("C_Garrison.SetRecruitmentPreferences", ""),
    ("C_Garrison.SetUsingPartyGarrison", ""),
    ("C_Garrison.ShouldShowMapTab", "f"),
    ("C_Garrison.ShowFollowerNameInErrorMessage", "f"),
    ("C_Garrison.StartMission", ""),
    ("C_Garrison.SwapBuildings", ""),
    ("C_Garrison.TargetSpellHasFollowerItemLevelUpgrade", "f"),
    ("C_Garrison.TargetSpellHasFollowerReroll", "f"),
    ("C_Garrison.TargetSpellHasFollowerTemporaryAbility", "f"),
    ("C_Garrison.UpgradeBuilding", ""),
    ("C_Garrison.UpgradeGarrison", ""),
    ("C_HouseExterior.GetFixtureDebugInfoForGUID", "n"),
    ("C_HouseExterior.GetHoveredFixtureDebugInfo", "n"),
    ("C_HouseExterior.GetSelectedFixtureDebugInfo", "n"),
    ("C_HousingCatalog.GetCatalogEntryDebugInfoForID", "n"),
    ("C_HousingDecor.GetDecorDebugInfoForGUID", "n"),
    ("C_HousingDecor.GetHoveredDecorDebugInfo", "n"),
    ("C_HousingDecor.GetSelectedDecorDebugInfo", "n"),
    ("C_PrototypeDialog.EnsureRemoved", ""),
    ("C_PrototypeDialog.SelectOption", ""),
    ("C_Reincarnation.GetReincarnatingCharacter", "n"),
    ("C_Reincarnation.IsReincarnating", "f"),
    ("C_Reincarnation.StartReincarnation", ""),
    ("C_Reincarnation.StopReincarnation", ""),
    (
        "C_SharedCharacterServices.GetLastSeenCharacterUpgradePopup",
        "0",
    ),
    (
        "C_SharedCharacterServices.GetLastSeenExpansionTrialPopup",
        "0",
    ),
    ("C_SharedCharacterServices.GetUpgradeDistributions", "t"),
    ("C_SharedCharacterServices.HasFreePromotionalUpgrade", "f"),
    (
        "C_SharedCharacterServices.HasSeenFreePromotionalUpgradePopup",
        "f",
    ),
    ("C_SharedCharacterServices.IsPurchaseIDPendingUpgrade", "f"),
    ("C_SharedCharacterServices.QueryClassTrialBoostResult", ""),
    ("C_SharedCharacterServices.SetCharacterUpgradePopupSeen", ""),
    ("C_SharedCharacterServices.SetExpansionTrialPopupSeen", ""),
    ("C_SharedCharacterServices.SetPromotionalPopupSeen", ""),
    ("C_SpectatingUI.GetSpectateTargetInfo", "n"),
    ("C_SpectatingUI.GetSpectatingPlayerSpellItemQuality", "n"),
    ("C_SpectatingUI.LeaveSpectateMode", ""),
    ("C_SpectatingUI.SpectateChange", ""),
    ("C_SpectatingUI.StartSpectating", ""),
    ("C_TalkingHead.GetConversationsDeferred", "f"),
    ("C_TalkingHead.GetCurrentLineAnimationInfo", "n"),
    ("C_TalkingHead.GetCurrentLineInfo", "n"),
    ("C_TalkingHead.IgnoreCurrentTalkingHead", ""),
    ("C_TalkingHead.IsCurrentTalkingHeadIgnored", "f"),
    ("C_TalkingHead.SetConversationsDeferred", ""),
    ("C_Trophy.MonumentChangeAppearanceToTrophyID", ""),
    ("C_Trophy.MonumentCloseMonumentUI", ""),
    ("C_Trophy.MonumentGetCount", "0"),
    ("C_Trophy.MonumentGetSelectedTrophyID", "n"),
    ("C_Trophy.MonumentGetTrophyInfoByIndex", "n"),
    ("C_Trophy.MonumentLoadList", ""),
    ("C_Trophy.MonumentLoadSelectedTrophyID", ""),
    ("C_Trophy.MonumentRevertAppearanceToSaved", ""),
    ("C_Trophy.MonumentSaveSelection", ""),
    ("C_WoWLabsMatchmaking.AcceptPartyInvite", ""),
    ("C_WoWLabsMatchmaking.CanEnterMatchmaking", "f"),
    ("C_WoWLabsMatchmaking.ClearFastLogin", ""),
    ("C_WoWLabsMatchmaking.DeclinePartyInvite", ""),
    ("C_WoWLabsMatchmaking.GetAutoQueueOnLogout", "f"),
    ("C_WoWLabsMatchmaking.GetCurrentParty", "t"),
    ("C_WoWLabsMatchmaking.GetInQueueTimeStart", "0"),
    ("C_WoWLabsMatchmaking.GetNumPartyInvites", "0"),
    ("C_WoWLabsMatchmaking.GetPartyInviteByIndex", "n"),
    ("C_WoWLabsMatchmaking.GetPartyPlaylistEntry", "n"),
    ("C_WoWLabsMatchmaking.GetPartySize", "0"),
    ("C_WoWLabsMatchmaking.IsAloneInWoWLabsParty", "f"),
    ("C_WoWLabsMatchmaking.IsFastLogin", "f"),
    ("C_WoWLabsMatchmaking.IsFindingMatch", "f"),
    ("C_WoWLabsMatchmaking.IsPartyFull", "f"),
    ("C_WoWLabsMatchmaking.IsPartyLeader", "f"),
    ("C_WoWLabsMatchmaking.IsPlayer", "f"),
    ("C_WoWLabsMatchmaking.IsPlayerReady", "f"),
    ("C_WoWLabsMatchmaking.IsWowLabsMatchmakingMember", "f"),
    ("C_WoWLabsMatchmaking.LeaveParty", ""),
    ("C_WoWLabsMatchmaking.RemovePlayerFromParty", ""),
    ("C_WoWLabsMatchmaking.SendPartyInvite", ""),
    ("C_WoWLabsMatchmaking.SetAutoQueueOnLogout", ""),
    ("C_WoWLabsMatchmaking.SetPartyPlaylistEntry", ""),
    ("C_WoWLabsMatchmaking.SetPlayerReady", ""),
    ("C_WowLabsDataManager.GetConfirmedWoWLabsArea", "n"),
    ("C_WowLabsDataManager.GetWoWLabsAreaInfo", "n"),
    ("C_WowLabsDataManager.IsInPrematch", "f"),
    ("C_WowLabsDataManager.PushCircleInfoToLua", ""),
    ("C_WowLabsDataManager.QuerySelectedWoWLabsArea", ""),
    ("C_WowLabsDataManager.QueryWoWLabsAreaInfo", ""),
    ("C_WowLabsDataManager.SelectWoWLabsArea", ""),
    ("C_WowTokenPublic.BuyToken", ""),
    ("C_WowTokenPublic.GetGuaranteedPrice", "n"),
    ("C_WowTokenPublic.GetListedAuctionableTokenInfo", "n"),
    ("C_WowTokenPublic.GetNumListedAuctionableTokens", "0"),
    ("C_WowTokenPublic.IsConsumableWowToken", "f"),
    ("C_WowTokenPublic.UpdateListedAuctionableTokens", ""),
    ("C_WowTokenPublic.UpdateMarketPrice", ""),
    ("C_WowTokenPublic.UpdateTokenCount", ""),
    ("Kiosk.EnableGodMode", ""),
    ("Kiosk.GetCharacterTemplateSetID", ""),
    ("Kiosk.GetKioskLoginInfo", ""),
    ("Kiosk.GetSecondsRemaining", ""),
    ("Kiosk.GetSessionDuration", ""),
    ("Kiosk.IsCompetitiveModeEnabled", "f"),
    ("Kiosk.IsExpired", ""),
    ("Kiosk.IsHousingResetPending", ""),
    ("Kiosk.IsInLobby", ""),
    ("Kiosk.IsSessionActive", ""),
    ("Kiosk.IsSessionTimed", ""),
    ("Kiosk.RequestHousingReset", ""),
    ("Kiosk.ShouldDeleteAllCharacters", ""),
    ("Kiosk.ShutdownSession", ""),
    ("Kiosk.StartSession", ""),
];

/// What the client answered to functions of systems Forever lacks the
/// same whatever they were asked (the full probe, WoW Forever 1.60.1:
/// every call with arguments it took, a commentator's camera settings,
/// the shop's race list), in the recordings' form (`n: value, …`); their
/// other calls fail as the arguments' checks say. A color is written with
/// its `GetRGB` (made with `CreateColor`, without alpha as recorded).
const RECORDED: &[(&str, &str)] = &[
    ("C_ArtifactUI.IsArtifactDisabled", "1: boolean true"),
    ("C_ArtifactUI.IsEquippedArtifactDisabled", "1: boolean true"),
    (
        "C_CatalogShop.GetAvailableTransmogRaceInfos",
        "1: {1={displayName=string Human,raceID=number 1},2={displayName=string Orc,raceID=number 2},\
         3={displayName=string Dwarf,raceID=number 3},4={displayName=string Night Elf,raceID=number 4},\
         5={displayName=string Undead,raceID=number 5},6={displayName=string Tauren,raceID=number 6},\
         7={displayName=string Gnome,raceID=number 7},8={displayName=string Troll,raceID=number 8},\
         9={displayName=string High Order Skyborne,raceID=number 95},\
         10={displayName=string Windshaper Skyborne,raceID=number 96}}",
    ),
    ("C_CatalogShop.GetFailureInfo", "2: number 0, nil nil"),
    // The largest 64-bit amount: no refund pending.
    (
        "C_CatalogShop.GetRefundableDecors",
        "2: {}, number 9.2233720368548e+18",
    ),
    (
        "C_CatalogShop.GetSpellVisualInfoForMount",
        "1: {animID=number 91,spellVisualKitID=number 0}",
    ),
    ("C_Commentator.GetCameraCollision", "1: boolean true"),
    (
        "C_Commentator.GetDistanceBeforeForcedHorizontalConvergence",
        "1: number 8",
    ),
    (
        "C_Commentator.GetDurationToForceHorizontalConvergence",
        "1: number 2000",
    ),
    ("C_Commentator.GetHardlockWeight", "1: number 100"),
    (
        "C_Commentator.GetHorizontalAngleThresholdToSmooth",
        "1: number 0.78554916381836",
    ),
    (
        "C_Commentator.GetLookAtLerpAmount",
        "1: number 0.03999999910593",
    ),
    ("C_Commentator.GetMaxNumPlayersPerTeam", "1: number 6"),
    ("C_Commentator.GetMaxNumTeams", "1: number 2"),
    (
        "C_Commentator.GetMsToHoldForHorizontalMovement",
        "1: number 500",
    ),
    (
        "C_Commentator.GetMsToHoldForVerticalMovement",
        "1: number 500",
    ),
    (
        "C_Commentator.GetMsToSmoothHorizontalChange",
        "1: number 4000",
    ),
    (
        "C_Commentator.GetMsToSmoothVerticalChange",
        "1: number 2000",
    ),
    (
        "C_Commentator.GetPositionLerpAmount",
        "1: number 0.03999999910593",
    ),
    ("C_Commentator.GetSoftlockWeight", "1: number 100"),
    ("C_Commentator.GetSpeedFactor", "1: number 1"),
    (
        "C_Commentator.GetTeamColorByUnit",
        "1: {GetRGB=function,b=number 0,g=number 0,r=number 0}",
    ),
    ("C_Commentator.IsUsingSmartCamera", "1: boolean true"),
    (
        "C_ContributionCollector.GetContributionResult",
        "1: number 1",
    ),
    ("C_ContributionCollector.GetRewardQuestID", "1: nil nil"),
    ("C_Covenants.GetCovenantIDs", "1: {1=number 46,2=number 48}"),
    ("C_Garrison.GetFollowerActivationCost", "1: number 2500000"),
    // Without the documented texture and texture kit (an empty structure
    // has no 0 file or empty kit here, unlike others).
    (
        "C_Garrison.GetMissionDeploymentInfo",
        "1: {enemies={},isExhausting=boolean false,location=string ,xp=number 0}",
    ),
    (
        "C_Garrison.GetMissionEncounterIconInfo",
        "1: {isElite=boolean false,isRare=boolean false,missionScalar=number 0}",
    ),
    ("C_Garrison.GetNumFollowerDailyActivations", "1: number 1"),
    (
        "C_Garrison.GetShipDeathAnimInfo",
        "6: number 58040, number 0, number 58041, number 0, number 58041, number 0",
    ),
    ("C_Housing.CanEditCharter", "1: boolean true"),
];

/// A system Forever lacks: the client's recorded answer (`RECORDED`), its
/// documented empty values, or an undocumented function's by its kind
/// (`UNDOCUMENTED`).
fn nothing(vm: &mut Vm, c: NativeCall) -> Ret {
    let api = engine(vm).api;
    let recorded = |name: &str| RECORDED.iter().find(|(n, _)| *n == name).map(|(_, a)| *a);
    let name = match c.upvals.first().copied() {
        Some(Value::Num(i)) => {
            let Some(f) = api.functions.get(i as usize) else {
                return Ok(vec![]);
            };
            return Ok(match recorded(f.name) {
                Some(answer) => crate::recorded::parse_answer(vm, answer),
                None => crate::defaults(vm, api, &f.doc),
            });
        }
        Some(Value::Str(s)) => String::from_utf8_lossy(vm.str_bytes(s)).into_owned(),
        _ => return Ok(vec![]),
    };
    if let Some(answer) = recorded(&name) {
        return Ok(crate::recorded::parse_answer(vm, answer));
    }
    if let Some(f) = api.functions.iter().find(|f| f.name == name) {
        return Ok(crate::defaults(vm, api, &f.doc));
    }
    let spec = UNDOCUMENTED
        .iter()
        .find(|(n, _)| *n == name)
        .map_or("", |(_, s)| *s);
    Ok(spec
        .chars()
        .map(|ch| match ch {
            'f' => Value::Bool(false),
            '0' => Value::Num(0.0),
            't' => Value::Table(vm.new_table()),
            's' => vm.str(""),
            _ => Value::Nil,
        })
        .collect())
}

/// `C_CharacterServices.CapitalizeCharName(name)`: the name with its first
/// letter capital and the rest small (unverified: the client's rules for
/// accented letters).
fn capitalize_char_name(vm: &mut Vm, c: NativeCall) -> Ret {
    let Some(name) = arg_text(vm, &c, 1) else {
        return Ok(vec![]);
    };
    let mut chars = name.chars();
    let out: String = match chars.next() {
        Some(first) => first
            .to_uppercase()
            .chain(chars.flat_map(char::to_lowercase))
            .collect(),
        None => String::new(),
    };
    Ok(vec![vm.str(&out)])
}

// ------------------------------------------------------- tooltip data

/// `Enum.TooltipDataType`'s values used here.
const TOOLTIP_SPELL: f64 = 1.0;
const TOOLTIP_UNIT_AURA: f64 = 7.0;
const TOOLTIP_MOUNT: f64 = 10.0;
const TOOLTIP_PET_ACTION: f64 = 11.0;

/// An item's tooltip data (`C_TooltipInfo.GetItemByID`'s, the tests' game
/// data).
fn item_tooltip(vm: &mut Vm, id: f64) -> Vec<Value> {
    crate::tooltip::item_data_of(vm, id as i64)
}

/// A spell's tooltip data (`C_TooltipInfo.GetSpellByID`'s).
fn spell_tooltip(vm: &mut Vm, id: f64) -> Vec<Value> {
    ask(vm, "C_TooltipInfo.GetSpellByID", &[Value::Num(id)])
}

/// A link's tooltip data (`C_TooltipInfo.GetHyperlink`'s: an item's or a
/// spell's).
fn link_tooltip(vm: &mut Vm, link: Value) -> Vec<Value> {
    match link {
        Value::Str(_) => ask(vm, "C_TooltipInfo.GetHyperlink", &[link]),
        _ => vec![],
    }
}

/// The data's type set to `ty` (an aura's spell is a `UnitAura`'s data;
/// unverified: the lines the client adds for each type).
fn retyped(vm: &mut Vm, data: Vec<Value>, ty: f64) -> Vec<Value> {
    if let Some(Value::Table(t)) = data.first().copied() {
        set(vm, t, "type", Value::Num(ty));
    }
    data
}

/// A color as Blizzard's code takes it (`CreateColor`'s when its UI code
/// is loaded, else the plain fields).
fn color(vm: &mut Vm, (r, g, b): (f64, f64, f64)) -> Value {
    let create = vm.get_global("CreateColor");
    if matches!(create, Value::Func(_))
        && let Ok(v) = vm.call(
            create,
            &[Value::Num(r), Value::Num(g), Value::Num(b), Value::Num(1.0)],
        )
        && let Some(&c) = v.first()
    {
        return c;
    }
    let t = vm.new_table();
    for (k, v) in [("r", r), ("g", g), ("b", b), ("a", 1.0)] {
        set(vm, t, k, Value::Num(v));
    }
    Value::Table(t)
}

/// Tooltip data of the name line alone (a spell the game data lacks: its
/// spellbook entry's name; the line as `tooltip.rs` writes a spell's).
fn named_tooltip(vm: &mut Vm, ty: f64, id: f64, name: Value) -> Vec<Value> {
    crate::tooltip::named_data(vm, ty, id, name)
}

/// `C_DamageMeter.IsDamageMeterAvailable()`: available, no reason (SuuUI
/// probe meter, four samples).
fn damage_meter_available(vm: &mut Vm, _c: NativeCall) -> Ret {
    Ok(vec![Value::Bool(true), vm.str("")])
}

/// `C_TooltipInfo.GetSpellBookItem(slot, bank)`: the player's spellbook
/// slot's spell (its recorded entry; nothing for the pet's book or an
/// empty slot).
fn tooltip_spellbook_item(vm: &mut Vm, c: NativeCall) -> Ret {
    let Some(slot) = arg_num(vm, &c, 1) else {
        return Ok(vec![]);
    };
    // `Enum.SpellBookSpellBank`: Player 0, Pet 1.
    if arg_num(vm, &c, 2).unwrap_or(0.0) != 0.0 {
        return Ok(vec![]);
    }
    let Some(Value::Table(item)) = crate::stats::spellbook_item_at(vm, slot as i64) else {
        return Ok(vec![]);
    };
    let Some(spell) = num(field(vm, item, "spellID")) else {
        return Ok(vec![]);
    };
    let data = spell_tooltip(vm, spell);
    if !data.is_empty() {
        return Ok(data);
    }
    let name = field(vm, item, "name");
    if name.is_nil() {
        return Ok(vec![]);
    }
    Ok(named_tooltip(vm, TOOLTIP_SPELL, spell, name))
}

/// `C_TooltipInfo.GetAction(slot)`: the action's spell's or item's
/// (`GetActionInfo`; unverified: a macro's, a companion's).
fn tooltip_action(vm: &mut Vm, c: NativeCall) -> Ret {
    let Some(slot) = arg_num(vm, &c, 1) else {
        return Ok(vec![]);
    };
    let info = ask(vm, "GetActionInfo", &[Value::Num(slot)]);
    let kind = match info.first() {
        Some(Value::Str(s)) => String::from_utf8_lossy(vm.str_bytes(*s)).into_owned(),
        _ => return Ok(vec![]),
    };
    let Some(id) = info.get(1).copied().and_then(num) else {
        return Ok(vec![]);
    };
    Ok(match kind.as_str() {
        "spell" => spell_tooltip(vm, id),
        "item" => item_tooltip(vm, id),
        _ => vec![],
    })
}

/// `C_TooltipInfo.GetInventoryItemByID`, `GetOwnedItemByID`: the item's
/// (unverified: the client's only for an item the player has, with its
/// GUID).
fn tooltip_item_id(vm: &mut Vm, c: NativeCall) -> Ret {
    Ok(match arg_num(vm, &c, 1) {
        Some(id) => item_tooltip(vm, id),
        None => vec![],
    })
}

/// `C_TooltipInfo.GetMountBySpellID(spellID)`: the spell's, as a mount's
/// data; nothing for a spell no mount of the journal casts (the full
/// probe, 1.60.1: Attack and Seal of Righteousness nothing).
fn tooltip_mount(vm: &mut Vm, c: NativeCall) -> Ret {
    let data = match arg_num(vm, &c, 1) {
        Some(id) if crate::collections::is_mount_spell(vm, id) => spell_tooltip(vm, id),
        _ => vec![],
    };
    Ok(retyped(vm, data, TOOLTIP_MOUNT))
}

/// `C_TooltipInfo.GetMerchantItem(slot)`: the merchant's goods' item
/// (`GetMerchantItemID`).
fn tooltip_merchant_item(vm: &mut Vm, c: NativeCall) -> Ret {
    let Some(slot) = arg_num(vm, &c, 1) else {
        return Ok(vec![]);
    };
    let id = ask(vm, "GetMerchantItemID", &[Value::Num(slot)]);
    Ok(match id.first().copied().and_then(num) {
        Some(id) => item_tooltip(vm, id),
        None => vec![],
    })
}

/// A getter's link's tooltip data: `getter(args)`'s first answer.
fn tooltip_of_link(vm: &mut Vm, getter: &'static str, args: &[Value]) -> Ret {
    let link = ask(vm, getter, args).first().copied().unwrap_or_default();
    Ok(link_tooltip(vm, link))
}

/// `C_TooltipInfo.GetBuybackItem(index)`: the item sold there
/// (`GetBuybackItemLink`).
fn tooltip_buyback_item(vm: &mut Vm, c: NativeCall) -> Ret {
    tooltip_of_link(vm, "GetBuybackItemLink", &[c.arg(1)])
}

/// `C_TooltipInfo.GetLootItem(slot)`: the loot slot's item
/// (`GetLootSlotLink`).
fn tooltip_loot_item(vm: &mut Vm, c: NativeCall) -> Ret {
    tooltip_of_link(vm, "GetLootSlotLink", &[c.arg(1)])
}

/// `C_TooltipInfo.GetQuestItem(type, index)`: the quest window's reward or
/// required item (`GetQuestItemLink`).
fn tooltip_quest_item(vm: &mut Vm, c: NativeCall) -> Ret {
    tooltip_of_link(vm, "GetQuestItemLink", &[c.arg(1), c.arg(2)])
}

/// `C_TooltipInfo.GetInboxItem(message [, attachment])`: the letter's
/// attachment (`GetInboxItemLink`, the first when none is named).
fn tooltip_inbox_item(vm: &mut Vm, c: NativeCall) -> Ret {
    let attachment = match c.arg(2) {
        Value::Nil => Value::Num(1.0),
        v => v,
    };
    tooltip_of_link(vm, "GetInboxItemLink", &[c.arg(1), attachment])
}

/// `C_TooltipInfo.GetSendMailItem([attachment])`: the letter being
/// written's attachment (`GetSendMailItemLink`, the first when none is
/// named).
fn tooltip_send_mail_item(vm: &mut Vm, c: NativeCall) -> Ret {
    let attachment = match c.arg(1) {
        Value::Nil => Value::Num(1.0),
        v => v,
    };
    tooltip_of_link(vm, "GetSendMailItemLink", &[attachment])
}

/// `C_TooltipInfo.GetPetAction(slot)`: the pet action's spell
/// (`GetPetActionInfo`'s seventh answer; a token's, such as Attack:
/// nothing).
fn tooltip_pet_action(vm: &mut Vm, c: NativeCall) -> Ret {
    let info = ask(vm, "GetPetActionInfo", &[c.arg(1)]);
    let data = match info.get(6).copied().and_then(num) {
        Some(spell) => spell_tooltip(vm, spell),
        None => vec![],
    };
    Ok(retyped(vm, data, TOOLTIP_PET_ACTION))
}

/// `C_TooltipInfo.GetShapeshift(slot)`: the form's spell
/// (`GetShapeshiftFormInfo`'s fourth answer).
fn tooltip_shapeshift(vm: &mut Vm, c: NativeCall) -> Ret {
    let info = ask(vm, "GetShapeshiftFormInfo", &[c.arg(1)]);
    Ok(match info.get(3).copied().and_then(num) {
        Some(spell) => spell_tooltip(vm, spell),
        None => vec![],
    })
}

/// An aura's tooltip data: its spell's, as a unit aura's.
fn aura_tooltip(vm: &mut Vm, aura: Option<Value>) -> Ret {
    let Some(Value::Table(t)) = aura else {
        return Ok(vec![]);
    };
    let Some(spell) = num(field(vm, t, "spellId")) else {
        return Ok(vec![]);
    };
    let data = spell_tooltip(vm, spell);
    Ok(retyped(vm, data, TOOLTIP_UNIT_AURA))
}

/// `C_TooltipInfo.GetUnitAura(unit, index [, filter])`: the aura there
/// (`C_UnitAuras.GetAuraDataByIndex`), no filter matching none, as an
/// empty one (the full probe, 1.60.1: the player's first aura by
/// `GetAuraDataByIndex("player", 1)` and `("player", 1, nil)`, nothing by
/// the tooltip's `GetUnitAura("player", 1)`, `(…, nil)` and `(…, "")`).
fn tooltip_unit_aura(vm: &mut Vm, c: NativeCall) -> Ret {
    let filter = match vm.hidden(c.arg(3)) {
        Value::Nil => vm.str(""),
        _ => c.arg(3),
    };
    let aura = ask(
        vm,
        "C_UnitAuras.GetAuraDataByIndex",
        &[c.arg(1), c.arg(2), filter],
    );
    aura_tooltip(vm, aura.first().copied())
}

/// `C_TooltipInfo.GetUnitBuff(unit, index [, filter])`: the helpful aura
/// there (`C_UnitAuras.GetBuffDataByIndex`).
fn tooltip_unit_buff(vm: &mut Vm, c: NativeCall) -> Ret {
    let aura = ask(
        vm,
        "C_UnitAuras.GetBuffDataByIndex",
        &[c.arg(1), c.arg(2), c.arg(3)],
    );
    aura_tooltip(vm, aura.first().copied())
}

/// `C_TooltipInfo.GetUnitDebuff(unit, index [, filter])`: the harmful aura
/// there (`C_UnitAuras.GetDebuffDataByIndex`).
fn tooltip_unit_debuff(vm: &mut Vm, c: NativeCall) -> Ret {
    let aura = ask(
        vm,
        "C_UnitAuras.GetDebuffDataByIndex",
        &[c.arg(1), c.arg(2), c.arg(3)],
    );
    aura_tooltip(vm, aura.first().copied())
}

/// `C_TooltipInfo.GetUnitAuraByAuraInstanceID(unit, id [, filter])` and its
/// buff and debuff forms: the aura with that instance
/// (`C_UnitAuras.GetAuraDataByAuraInstanceID`; unverified: the filter
/// left out).
fn tooltip_unit_aura_by_instance(vm: &mut Vm, c: NativeCall) -> Ret {
    tooltip_aura_by_instance(vm, &c, None)
}

/// The buff form: none for a harmful aura (unverified).
fn tooltip_unit_buff_by_instance(vm: &mut Vm, c: NativeCall) -> Ret {
    tooltip_aura_by_instance(vm, &c, Some("isHelpful"))
}

/// The debuff form: none for a helpful aura (the full probe, 1.60.1: the
/// player's buff by its instance nothing, by token and GUID).
fn tooltip_unit_debuff_by_instance(vm: &mut Vm, c: NativeCall) -> Ret {
    tooltip_aura_by_instance(vm, &c, Some("isHarmful"))
}

fn tooltip_aura_by_instance(vm: &mut Vm, c: &NativeCall, kind: Option<&str>) -> Ret {
    let aura = ask(
        vm,
        "C_UnitAuras.GetAuraDataByAuraInstanceID",
        &[c.arg(1), c.arg(2)],
    );
    let aura = aura.first().copied();
    if let (Some(kind), Some(Value::Table(t))) = (kind, aura)
        && !field(vm, t, kind).truthy()
    {
        return Ok(vec![]);
    }
    aura_tooltip(vm, aura)
}

/// `C_TooltipInfo.GetWorldCursor()`: the unit under the cursor's
/// (`C_TooltipInfo.GetUnit("mouseover")`; unverified: objects in the
/// world).
fn tooltip_world_cursor(vm: &mut Vm, _c: NativeCall) -> Ret {
    let token = vm.str("mouseover");
    Ok(ask(vm, "C_TooltipInfo.GetUnit", &[token]))
}

// ------------------------------------------------ interactions, trainers

/// Whether the player is interacting with an NPC of `Enum.
/// PlayerInteractionType` `ty`: the windows the engine opens (the same
/// types their `PLAYER_INTERACTION_MANAGER_FRAME_SHOW` names).
fn interacting(vm: &Vm, ty: f64) -> bool {
    let w = &engine(vm).world;
    match ty as i64 {
        1 => w.party.trading.is_some(),
        3 => w.gossip.open.is_some(),
        4 => w.quests.dialog.is_some(),
        5 => w.merchant.open,
        6 => w.taxis.open.is_some(),
        7 => w.trainers.open.is_some(),
        17 => w.mail.open.is_some(),
        21 => w.auction.open.is_some(),
        8 | 67 => w.bank.open.is_some(),
        _ => false,
    }
}

/// `C_PlayerInteractionManager.IsInteractingWithNpcOfType(type)`: that
/// type's window open (unverified: the client's interaction outlasting a
/// window).
fn is_interacting_with_npc_of_type(vm: &mut Vm, c: NativeCall) -> Ret {
    let ty = arg_num(vm, &c, 1).unwrap_or(0.0);
    Ok(vec![Value::Bool(interacting(vm, ty))])
}

/// `C_PlayerInteractionManager.IsValidNPCInteraction(type)`: the NPC the
/// player interacts with offers that type (unverified: as
/// `IsInteractingWithNpcOfType`).
fn is_valid_npc_interaction(vm: &mut Vm, c: NativeCall) -> Ret {
    is_interacting_with_npc_of_type(vm, c)
}

/// `C_Trainer.GetTrainerType()`: `Enum.TrainerType.General` (recorded: 0
/// at all 14 class trainers probed; a profession trainer's unverified).
fn get_trainer_type(_vm: &mut Vm, _c: NativeCall) -> Ret {
    Ok(vec![Value::Num(0.0)])
}

/// `C_Trainer.GetCategorizeTrainerUI()`: the setting
/// `SetCategorizeTrainerUI` keeps, true at first (recorded: true at 11 of
/// 14 class trainers probed, false at three a druid probed; taken as the
/// character's setting, unverified).
fn get_categorize_trainer_ui(vm: &mut Vm, _c: NativeCall) -> Ret {
    Ok(vec![match state_get(vm, "categorizeTrainerUI") {
        Value::Bool(b) => Value::Bool(b),
        _ => Value::Bool(true),
    }])
}

fn set_categorize_trainer_ui(vm: &mut Vm, c: NativeCall) -> Ret {
    let on = vm.hidden(c.arg(1)).truthy();
    state_set(vm, "categorizeTrainerUI", Value::Bool(on));
    Ok(vec![])
}

// -------------------------------------------------- name plates, combat

/// `C_NamePlateManager.SetNamePlateHitTestInsets(type, left, right, top,
/// bottom)`: kept for `GetNamePlateHitTestInsets(type)`, 0 each at first.
fn set_name_plate_hit_test_insets(vm: &mut Vm, c: NativeCall) -> Ret {
    let ty = arg_num(vm, &c, 1).unwrap_or(0.0);
    let t = vm.new_table();
    for i in 0..4 {
        let v = Value::Num(arg_num(vm, &c, i + 2).unwrap_or(0.0));
        let _ = vm.heap.table_mut(t).set(Value::Num(i as f64 + 1.0), v);
    }
    state_set(vm, &format!("hitTestInsets{ty}"), Value::Table(t));
    Ok(vec![])
}

fn get_name_plate_hit_test_insets(vm: &mut Vm, c: NativeCall) -> Ret {
    let ty = arg_num(vm, &c, 1).unwrap_or(0.0);
    let Value::Table(t) = state_get(vm, &format!("hitTestInsets{ty}")) else {
        return Ok(vec![Value::Num(0.0); 4]);
    };
    Ok((1..=4)
        .map(|i| vm.heap.table(t).get(Value::Num(i as f64)))
        .collect())
}

/// `C_CombatText.SetActiveUnit(unit)`: the unit the floating combat text
/// follows, for `GetActiveUnit()` ("player" at first; unverified).
fn set_combat_text_unit(vm: &mut Vm, c: NativeCall) -> Ret {
    if let Value::Str(s) = vm.hidden(c.arg(1)) {
        state_set(vm, "combatTextUnit", Value::Str(s));
    }
    Ok(vec![])
}

fn get_combat_text_unit(vm: &mut Vm, _c: NativeCall) -> Ret {
    Ok(vec![match state_get(vm, "combatTextUnit") {
        Value::Nil => vm.str("player"),
        v => v,
    }])
}

/// `C_SwingTimer.IsTargetWithinSwingRange(swingType)`: the target within
/// melee reach (5 yards) for a hand, within a ranged weapon's (8 to 35
/// yards) for `Ranged`; nil without a target or its distance (unverified:
/// the client's reach, which grows with the units' sizes).
fn is_target_within_swing_range(vm: &mut Vm, c: NativeCall) -> Ret {
    let ty = arg_num(vm, &c, 1).unwrap_or(0.0);
    let u = &engine(vm).units;
    let distance = units::resolve(u, "target").and_then(|t| u.list[t.0].distance);
    Ok(vec![match distance {
        Some(d) if ty == 2.0 => Value::Bool((8.0..=35.0).contains(&d)),
        Some(d) => Value::Bool(d <= 5.0),
        None => Value::Nil,
    }])
}

// ------------------------------------------------------ creatures, races

/// `C_CreatureInfo.GetCreatureID(guid)`: a creature's (or pet's, or
/// vehicle's) NPC ID, the GUID's sixth part (`Creature-0-…-<id>-…`); nil
/// for a player's or another kind (warcraft.wiki.gg, API
/// C_CreatureInfo.GetCreatureID; unverified).
fn get_creature_id(vm: &mut Vm, c: NativeCall) -> Ret {
    let Some(guid) = arg_text(vm, &c, 1) else {
        return Ok(vec![Value::Nil]);
    };
    let parts: Vec<&str> = guid.split('-').collect();
    let id = match parts.first() {
        Some(&("Creature" | "Pet" | "Vehicle")) => parts.get(5).and_then(|p| p.parse::<f64>().ok()),
        _ => None,
    };
    Ok(vec![id.map_or(Value::Nil, Value::Num)])
}

/// A race: its ID, name, file name and faction.
type Race = (f64, &'static str, &'static str, Option<&'static str>);

/// The client's races (as `units.rs`'s `UnitRace` names them; the
/// Undead's and the Skyborne's recorded, the others unverified; the
/// Skyborne's faction unknown).
const RACES: &[Race] = &[
    (1.0, "Human", "Human", Some("Alliance")),
    (2.0, "Orc", "Orc", Some("Horde")),
    (3.0, "Dwarf", "Dwarf", Some("Alliance")),
    (4.0, "Night Elf", "NightElf", Some("Alliance")),
    (5.0, "Undead", "Scourge", Some("Horde")),
    (6.0, "Tauren", "Tauren", Some("Horde")),
    (7.0, "Gnome", "Gnome", Some("Alliance")),
    (8.0, "Troll", "Troll", Some("Horde")),
    (9.0, "Goblin", "Goblin", Some("Horde")),
    (10.0, "Blood Elf", "BloodElf", Some("Horde")),
    (11.0, "Draenei", "Draenei", Some("Alliance")),
    (22.0, "Worgen", "Worgen", Some("Alliance")),
    (96.0, "Windshaper Skyborne", "Skyborne", None),
];

/// The race an ID argument names, the ID truncated (the full probe,
/// 1.60.1: 1.5 named the humans).
fn race(vm: &Vm, c: &NativeCall) -> Option<&'static Race> {
    let id = arg_num(vm, c, 1)?.trunc();
    RACES.iter().find(|r| r.0 == id)
}

/// `C_CreatureInfo.GetRaceInfo(raceID)`: `{ raceName, clientFileString,
/// raceID }`, nil for a race the client lacks.
fn get_race_info(vm: &mut Vm, c: NativeCall) -> Ret {
    let Some(&(id, name, file, _)) = race(vm, &c) else {
        return Ok(vec![Value::Nil]);
    };
    let t = vm.new_table();
    let (name, file) = (vm.str(name), vm.str(file));
    set(vm, t, "raceName", name);
    set(vm, t, "clientFileString", file);
    set(vm, t, "raceID", Value::Num(id));
    Ok(vec![Value::Table(t)])
}

/// `C_CreatureInfo.GetFactionInfo(raceID)`: `{ name, groupTag }` of the
/// race's faction ("Horde" or "Alliance" both; unverified: the name's
/// localized global), nil for a race without one.
fn get_faction_info(vm: &mut Vm, c: NativeCall) -> Ret {
    let Some(&(_, _, _, Some(faction))) = race(vm, &c) else {
        return Ok(vec![Value::Nil]);
    };
    let t = vm.new_table();
    let name = vm.str(faction);
    set(vm, t, "name", name);
    set(vm, t, "groupTag", name);
    Ok(vec![Value::Table(t)])
}

// ------------------------------------------------------------- misc

/// `C_EditMode.IsValidLayoutName(name)`: a name with something besides
/// spaces (unverified: the client's length limit and banned words).
fn is_valid_layout_name(vm: &mut Vm, c: NativeCall) -> Ret {
    let ok = arg_text(vm, &c, 1).is_some_and(|n| !n.trim().is_empty());
    Ok(vec![Value::Bool(ok)])
}

/// `C_Macro.GetMacroName(index)`: the macro's name (`GetMacroInfo`'s
/// first answer), nil for none.
fn get_macro_name(vm: &mut Vm, c: NativeCall) -> Ret {
    let info = ask(vm, "GetMacroInfo", &[c.arg(1)]);
    Ok(vec![info.first().copied().unwrap_or_default()])
}

/// `C_LevelSquish.ConvertPlayerLevel(level)` and `ConvertFollowerLevel(level,
/// max)`: the level as given (Forever's levels were never squished;
/// unverified).
fn same_level(vm: &mut Vm, c: NativeCall) -> Ret {
    Ok(vec![Value::Num(arg_num(vm, &c, 1).unwrap_or(0.0))])
}

/// `C_NameUtil.ReplaceSurnameSeparatorWithLinkSeparator(fullName)`: the
/// name as given (Forever's names have no surname separator; unverified).
fn same_name(vm: &mut Vm, c: NativeCall) -> Ret {
    let name = arg_text(vm, &c, 1).unwrap_or_default();
    Ok(vec![vm.str(&name)])
}

/// `C_ColorOverrides.GetDefaultColorForQuality(quality)`: the quality's
/// color (a quality beyond: the common white, unverified): the client's
/// own quality colors, as `C_ColorOverrides.GetColorForQuality`
/// has them (the full probe, 1.60.1: every quality 0 to 8 alike; a string
/// as 0).
fn get_default_color_for_quality(vm: &mut Vm, c: NativeCall) -> Ret {
    let quality = arg_num(vm, &c, 1).unwrap_or(0.0).max(0.0) as usize;
    let rgba = crate::facts::QUALITY_COLORS
        .get(quality)
        .copied()
        .unwrap_or(crate::facts::QUALITY_COLORS[1]);
    Ok(vec![color(vm, (rgba[0], rgba[1], rgba[2]))])
}

// ---------------------------------------------------------- auctions

/// `C_AuctionHouse.HasFullBidResults()` and `HasFullOwnedAuctionResults()`:
/// true (the engine's lists are whole: `GetNumBids` and
/// `GetNumOwnedAuctions` answer 0, recorded at an auction house;
/// unverified before their queries).
fn yes(_vm: &mut Vm, _c: NativeCall) -> Ret {
    Ok(vec![Value::Bool(true)])
}

/// `C_AuctionHouse.GetTimeLeftBandInfo(band)`: the band's least and most
/// seconds: Short under 30 minutes, Medium to 2 hours, Long to 12 hours,
/// VeryLong to 48 (the auction house's time left texts,
/// `AUCTION_TIME_LEFT1`…`4`; unverified on Forever).
fn get_time_left_band_info(vm: &mut Vm, c: NativeCall) -> Ret {
    let (min, max) = match arg_num(vm, &c, 1).unwrap_or(0.0) as i64 {
        0 => (0.0, 1800.0),
        1 => (1800.0, 7200.0),
        2 => (7200.0, 43200.0),
        _ => (43200.0, 172800.0),
    };
    Ok(vec![Value::Num(min), Value::Num(max)])
}

/// `C_AuctionHouse.GetItemKeySampleLink(itemKey)`: the key's item's link
/// (`C_Item.GetItemInfo`'s second answer), nil when unknown or for an item
/// that binds on pickup or is a quest's, which no auction sells (the full
/// probe, 1.60.1: a quest crate's key nil; unverified: other bound items).
fn get_item_key_sample_link(vm: &mut Vm, c: NativeCall) -> Ret {
    let Value::Table(key) = vm.hidden(c.arg(1)) else {
        return Ok(vec![Value::Nil]);
    };
    let Some(id) = num(field(vm, key, "itemID")) else {
        return Ok(vec![Value::Nil]);
    };
    let info = ask(vm, "C_Item.GetItemInfo", &[Value::Num(id)]);
    if matches!(info.get(13).copied(), Some(Value::Num(b)) if b == 1.0 || b == 4.0) {
        return Ok(vec![Value::Nil]);
    }
    Ok(vec![match info.get(1).copied() {
        Some(v @ Value::Str(_)) => v,
        _ => Value::Nil,
    }])
}

/// A search's results' field `key`, each row's (`count` rows, `row(i)`
/// the engine's answer for row `i`).
fn result_fields(
    vm: &mut Vm,
    count: &'static str,
    row: &'static str,
    search: Value,
    key: &str,
) -> Vec<f64> {
    let n = ask(vm, count, &[search])
        .first()
        .copied()
        .and_then(num)
        .unwrap_or(0.0);
    let mut out = Vec::new();
    for i in 1..=(n as i64) {
        let info = ask(vm, row, &[search, Value::Num(i as f64)]);
        if let Some(Value::Table(t)) = info.first().copied()
            && let Some(v) = num(field(vm, t, key))
        {
            out.push(v);
        }
    }
    out
}

/// The largest of `values`, nil for none.
fn largest(values: Vec<f64>) -> Ret {
    Ok(vec![
        values
            .into_iter()
            .reduce(f64::max)
            .map_or(Value::Nil, Value::Num),
    ])
}

/// `C_AuctionHouse.GetItemSearchResultsQuantity(itemKey)`: the item
/// search's results' quantities summed (`GetItemSearchResultInfo`'s).
fn get_item_search_results_quantity(vm: &mut Vm, c: NativeCall) -> Ret {
    let total: f64 = result_fields(
        vm,
        "C_AuctionHouse.GetNumItemSearchResults",
        "C_AuctionHouse.GetItemSearchResultInfo",
        c.arg(1),
        "quantity",
    )
    .iter()
    .fold(0.0, |a, b| a + b);
    // From +0 (an empty sum is 0, the full probe; `sum` gives -0).
    Ok(vec![Value::Num(total)])
}

/// `C_AuctionHouse.GetMaxItemSearchResultBuyout(itemKey)`: the item
/// search's largest buyout, nil for none.
fn get_max_item_search_result_buyout(vm: &mut Vm, c: NativeCall) -> Ret {
    largest(result_fields(
        vm,
        "C_AuctionHouse.GetNumItemSearchResults",
        "C_AuctionHouse.GetItemSearchResultInfo",
        c.arg(1),
        "buyoutAmount",
    ))
}

/// `C_AuctionHouse.GetMaxItemSearchResultBid(itemKey)`: the item search's
/// largest bid, nil for none.
fn get_max_item_search_result_bid(vm: &mut Vm, c: NativeCall) -> Ret {
    largest(result_fields(
        vm,
        "C_AuctionHouse.GetNumItemSearchResults",
        "C_AuctionHouse.GetItemSearchResultInfo",
        c.arg(1),
        "bidAmount",
    ))
}

/// `C_AuctionHouse.GetMaxCommoditySearchResultPrice(itemID)`: the
/// commodity search's largest unit price, nil for none.
fn get_max_commodity_search_result_price(vm: &mut Vm, c: NativeCall) -> Ret {
    largest(result_fields(
        vm,
        "C_AuctionHouse.GetNumCommoditySearchResults",
        "C_AuctionHouse.GetCommoditySearchResultInfo",
        c.arg(1),
        "unitPrice",
    ))
}

/// `C_AuctionHouse.RequestMoreItemSearchResults(itemKey)`: whether the
/// results are whole (`HasFullItemSearchResults`; the engine's searches
/// answer in one page).
fn request_more_item_search_results(vm: &mut Vm, c: NativeCall) -> Ret {
    let full = ask(vm, "C_AuctionHouse.HasFullItemSearchResults", &[c.arg(1)]);
    Ok(vec![Value::Bool(full.first().is_none_or(|v| v.truthy()))])
}

/// `C_AuctionHouse.RequestMoreCommoditySearchResults(itemID)`: whether the
/// results are whole (`HasFullCommoditySearchResults`).
fn request_more_commodity_search_results(vm: &mut Vm, c: NativeCall) -> Ret {
    let full = ask(
        vm,
        "C_AuctionHouse.HasFullCommoditySearchResults",
        &[c.arg(1)],
    );
    Ok(vec![Value::Bool(full.first().is_none_or(|v| v.truthy()))])
}

// -------------------------------------------------------------- C_Intl

/// `C_Intl.GetCurrentLocale()`: the client's locale (`GetLocale`'s
/// answer; unverified: ICU's form of it).
///
/// The full probe (1.60.1): "en-US", the locale's language and region
/// joined by a hyphen (`GetLocale`'s "enUS").
fn intl_current_locale(vm: &mut Vm, _c: NativeCall) -> Ret {
    let locale = ask(vm, "GetLocale", &[]);
    let text = match locale.first().copied() {
        Some(Value::Str(s)) => String::from_utf8_lossy(vm.str_bytes(s)).into_owned(),
        _ => "enUS".to_string(),
    };
    let text = if text.len() == 4 && text.is_ascii() {
        format!("{}-{}", &text[..2], &text[2..])
    } else {
        text
    };
    Ok(vec![vm.str(&text)])
}

/// A text function of `C_Intl` (Unicode's case rules; unverified against
/// the client's ICU for special cases).
fn intl_text(vm: &mut Vm, c: &NativeCall, f: impl FnOnce(&str) -> String) -> Ret {
    let Some(text) = arg_text(vm, c, 1) else {
        return Ok(vec![]);
    };
    let out = f(&text);
    Ok(vec![vm.str(&out)])
}

fn intl_to_lower(vm: &mut Vm, c: NativeCall) -> Ret {
    intl_text(vm, &c, str::to_lowercase)
}

fn intl_to_upper(vm: &mut Vm, c: NativeCall) -> Ret {
    intl_text(vm, &c, str::to_uppercase)
}

/// `C_Intl.FoldCase(text)`: its case folded (small letters).
fn intl_fold_case(vm: &mut Vm, c: NativeCall) -> Ret {
    intl_text(vm, &c, str::to_lowercase)
}

/// `C_Intl.ToTitle(text)`: each word's first letter capital, the rest
/// small; a word ends at any character but a letter or digit, a color
/// code's bar too (the full probe, 1.60.1: "|cffff0000Red|r" is
/// "|Cffff0000red|R"); an apostrophe, period or colon inside a word keeps
/// it whole, as ICU's word breaks do (unverified).
fn intl_to_title(vm: &mut Vm, c: NativeCall) -> Ret {
    intl_text(vm, &c, |s| {
        let mut out = String::with_capacity(s.len());
        let mut start = true;
        let mut prev_letter = false;
        for ch in s.chars() {
            if ch.is_alphanumeric() {
                if start {
                    out.extend(ch.to_uppercase());
                } else {
                    out.extend(ch.to_lowercase());
                }
                start = false;
                prev_letter = true;
            } else {
                out.push(ch);
                start = !(prev_letter && matches!(ch, '\'' | '\u{2019}' | '.' | ':'));
                prev_letter = false;
            }
        }
        out
    })
}

/// `C_Intl.Normalize(text, form)`: the text as given (unverified: only
/// ASCII text is already normal in every form).
fn intl_normalize(vm: &mut Vm, c: NativeCall) -> Ret {
    intl_text(vm, &c, str::to_string)
}

/// `C_Intl.IsNormalized(text, form)`: ASCII text is (unverified beyond).
fn intl_is_normalized(vm: &mut Vm, c: NativeCall) -> Ret {
    let ascii = arg_text(vm, &c, 1).is_none_or(|t| t.is_ascii());
    Ok(vec![Value::Bool(ascii)])
}

/// `C_Intl.Length(text)`: its characters (code points).
fn intl_length(vm: &mut Vm, c: NativeCall) -> Ret {
    Ok(match arg_text(vm, &c, 1) {
        Some(t) => vec![Value::Num(t.chars().count() as f64)],
        None => vec![],
    })
}

/// `C_Intl.CompareStrings(left, right, strength)`: -1, 0 or 1; the
/// primary and secondary strengths ignore case (unverified: ICU's
/// collation of accents and punctuation).
fn intl_compare_strings(vm: &mut Vm, c: NativeCall) -> Ret {
    let (Some(a), Some(b)) = (arg_text(vm, &c, 1), arg_text(vm, &c, 2)) else {
        return Ok(vec![]);
    };
    let strength = arg_num(vm, &c, 3).unwrap_or(2.0);
    let order = if strength < 2.0 {
        a.to_lowercase().cmp(&b.to_lowercase())
    } else {
        a.to_lowercase()
            .cmp(&b.to_lowercase())
            .then_with(|| b.cmp(&a))
    };
    Ok(vec![Value::Num(match order {
        std::cmp::Ordering::Less => -1.0,
        std::cmp::Ordering::Equal => 0.0,
        std::cmp::Ordering::Greater => 1.0,
    })])
}

/// A whole number with thousands' commas (en-US).
fn grouped(n: i64) -> String {
    let digits = n.unsigned_abs().to_string();
    let mut out = String::new();
    for (i, ch) in digits.chars().enumerate() {
        if i > 0 && (digits.len() - i).is_multiple_of(3) {
            out.push(',');
        }
        out.push(ch);
    }
    if n < 0 { format!("-{out}") } else { out }
}

/// `C_Intl.FormatNumber(number, style)`: en-US's: `Decimal` grouped with
/// up to three decimals, `Integer` rounded, `Percent` times 100 rounded
/// with "%" (ICU's patterns); `Currency` nothing (the full probe, 1.60.1).
fn intl_format_number(vm: &mut Vm, c: NativeCall) -> Ret {
    let Some(n) = arg_num(vm, &c, 1) else {
        return Ok(vec![]);
    };
    let style = arg_num(vm, &c, 2).unwrap_or(0.0) as i64;
    let with_decimals = |n: f64, places: usize, keep_zeros: bool| {
        let text = format!("{:.*}", places, n.abs());
        let (whole, frac) = text.split_once('.').unwrap_or((text.as_str(), ""));
        let frac = if keep_zeros {
            frac.to_string()
        } else {
            frac.trim_end_matches('0').to_string()
        };
        let whole = grouped(whole.parse::<i64>().unwrap_or(0));
        let sign = if n < 0.0 && (whole != "0" || !frac.is_empty()) {
            "-"
        } else {
            ""
        };
        if frac.is_empty() {
            format!("{sign}{whole}")
        } else {
            format!("{sign}{whole}.{frac}")
        }
    };
    let out = match style {
        1 => grouped(n.round() as i64),
        2 => format!("{}%", grouped((n * 100.0).round() as i64)),
        // Currency: nothing (the full probe, 1.60.1: every number).
        3 => return Ok(vec![]),
        _ => with_decimals(n, 3, false),
    };
    Ok(vec![vm.str(&out)])
}

// ------------------------------------------- what the documentation says

fn flagged(doc: &serde_json::Value, flag: &str) -> bool {
    doc.get(flag).and_then(serde_json::Value::as_bool) == Some(true)
}

/// Whether a documented function answers nothing before its arguments
/// are checked: one that needs a commentator (`RequiresCommentator`,
/// `RequiresActiveCommentator`), WoW Forever having none (the full probe:
/// all 615 calls of the 39 asked answered nothing, those with arguments
/// of the wrong type or none too); one that needs the diminishing returns
/// UI (`RequiresSpellDiminishUI`), which Forever does not support
/// (`C_SpellDiminish.IsSystemSupported()` false; all 39 calls of the 3
/// asked answered nothing, `GetSpellDiminishCategoryInfo()` and `(-1)`
/// too).
pub(crate) fn silent_unchecked(doc: &serde_json::Value) -> bool {
    flagged(doc, "RequiresActiveCommentator")
        || flagged(doc, "RequiresCommentator")
        || flagged(doc, "RequiresSpellDiminishUI")
}

/// Whether a documented function whose arguments passed their checks
/// answers nothing: one that needs a timeline event
/// (`RequiresValidTimelineEvent`), for an ID not on the timeline (the
/// full probe, with none there: the 7 asked answered nothing to every
/// event ID that passed, 0 to 2147483647; an addon's script events are
/// there, `timeline.rs`).
pub(crate) fn silent_checked(doc: &serde_json::Value) -> bool {
    flagged(doc, "RequiresValidTimelineEvent")
}

// ------------------------------------------- retail systems' game data

/// An argument as a whole number as the client's C code takes it (a
/// fraction cut, a non-number 0: the full probe's `"x"` and 1.5).
fn arg_whole(vm: &Vm, c: &NativeCall, n: usize) -> f64 {
    arg_num(vm, c, n).map_or(0.0, f64::trunc)
}

/// `C_Commentator.GetTeamColor(teamIndex)`: the first team blue, any other
/// red, without alpha (the full probe: 1 and 1.5 (100, 179, 255), 0 and
/// 2147483647 (255, 0, 0)).
fn commentator_team_color(vm: &mut Vm, c: NativeCall) -> Ret {
    let answer = if arg_whole(vm, &c, 1) == 1.0 {
        "1: {GetRGB=function,b=number 1,g=number 0.70196080207825,r=number 0.39215689897537}"
    } else {
        "1: {GetRGB=function,b=number 0,g=number 0,r=number 1}"
    };
    Ok(crate::recorded::parse_answer(vm, answer))
}

/// `C_Commentator.GetOrCreateSeries(teamName1, teamName2)`: a new series
/// of the two teams, no score (the full probe: two teams of score 0, their
/// names blanked by the ingest; the names the ones asked, unverified).
fn commentator_series(vm: &mut Vm, c: NativeCall) -> Ret {
    let teams = vm.new_table();
    for i in 1..=2 {
        let team = vm.new_table();
        let name = vm.hidden(c.arg(i));
        set(vm, team, "name", name);
        set(vm, team, "score", Value::Num(0.0));
        let _ = vm
            .heap
            .table_mut(teams)
            .set(Value::Num(i as f64), Value::Table(team));
    }
    let t = vm.new_table();
    set(vm, t, "teams", Value::Table(teams));
    Ok(vec![Value::Table(t)])
}

/// The encounter timeline's tracks (`Enum.EncounterTimelineTrack`, 0 to
/// 4): their type, sort direction (none for some) and longest event in
/// seconds, as the full probe's `C_EncounterTimeline.GetTrackList()` gave
/// them (the indeterminate track's the largest 32-bit milliseconds); no
/// other bound set.
const TRACKS: [(f64, Option<f64>, f64); 5] = [
    (1.0, Some(1.0), 0.0),
    (2.0, None, 0.0),
    (2.0, None, 0.0),
    (1.0, Some(0.0), 0.0),
    (0.0, None, 4_294_967.295),
];

/// The track an argument names (a non-number the first).
fn track(vm: &Vm, c: &NativeCall) -> Option<usize> {
    let n = arg_whole(vm, c, 1);
    (0.0..TRACKS.len() as f64)
        .contains(&n)
        .then_some(n as usize)
}

fn track_info(vm: &mut Vm, i: usize) -> Value {
    let (ty, sort, longest) = TRACKS[i];
    let t = vm.new_table();
    set(vm, t, "id", Value::Num(i as f64));
    set(vm, t, "type", Value::Num(ty));
    if let Some(sort) = sort {
        set(vm, t, "sortDirection", Value::Num(sort));
    }
    for k in [
        "minimumDuration",
        "minimumEventIntroDuration",
        "minimumEventGapDuration",
        "maximumEventCount",
    ] {
        set(vm, t, k, Value::Num(0.0));
    }
    set(vm, t, "maximumDuration", Value::Num(longest));
    Value::Table(t)
}

/// `C_EncounterTimeline.GetTrackInfo(track)`.
fn timeline_track_info(vm: &mut Vm, c: NativeCall) -> Ret {
    Ok(match track(vm, &c) {
        Some(i) => vec![track_info(vm, i)],
        None => vec![],
    })
}

/// `C_EncounterTimeline.GetTrackList()`: every track, in order.
fn timeline_track_list(vm: &mut Vm, _c: NativeCall) -> Ret {
    let list = vm.new_table();
    for i in 0..TRACKS.len() {
        let info = track_info(vm, i);
        let _ = vm
            .heap
            .table_mut(list)
            .set(Value::Num((i + 1) as f64), info);
    }
    Ok(vec![Value::Table(list)])
}

/// `C_EncounterTimeline.GetTrackType(track)`.
fn timeline_track_type(vm: &mut Vm, c: NativeCall) -> Ret {
    Ok(match track(vm, &c) {
        Some(i) => vec![Value::Num(TRACKS[i].0)],
        None => vec![],
    })
}

/// `C_EncounterTimeline.GetTrackMaxEventDuration(track)`.
fn timeline_track_max_event_duration(vm: &mut Vm, c: NativeCall) -> Ret {
    Ok(match track(vm, &c) {
        Some(i) => vec![Value::Num(TRACKS[i].2)],
        None => vec![],
    })
}

/// `C_EncounterTimeline.GetCurrentTime()`: the client's clock, as
/// `GetTime` (the full probe: 206454.927 within the run's `GetTime`
/// answers; one sample, unverified).
fn timeline_current_time(vm: &mut Vm, _c: NativeCall) -> Ret {
    let now = ask(vm, "GetTime", &[]);
    Ok(vec![now.first().copied().unwrap_or(Value::Num(0.0))])
}

/// Artifact power a trait's ranks cost, from rank 0, whatever the tier
/// (the full probe: rank 0 100, rank 1 300, tiers 0 and 1 alike; the
/// answers battery: ranks 2 to 10, and the totals of ranks 1 to 2, 1 to 5,
/// 1 to 10 and 3 to 6 agree). Past rank 10 nothing here, unverified, as a
/// rank far past the table does (2147483647: 0).
const ARTIFACT_RANK_COSTS: [f64; 11] = [
    100.0, 300.0, 325.0, 350.0, 375.0, 400.0, 425.0, 450.0, 525.0, 625.0, 750.0,
];

fn artifact_rank_cost(rank: f64) -> f64 {
    if rank < 0.0 {
        return 0.0;
    }
    ARTIFACT_RANK_COSTS
        .get(rank as usize)
        .copied()
        .unwrap_or(0.0)
}

/// `C_ArtifactUI.GetCostForPointAtRank(rank, tier)`.
fn artifact_cost_at_rank(vm: &mut Vm, c: NativeCall) -> Ret {
    Ok(vec![Value::Num(artifact_rank_cost(arg_whole(vm, &c, 1)))])
}

/// `C_ArtifactUI.GetTotalPowerCost(startingTrait, numTraits, tier)`: the
/// ranks' costs from the starting one (counted from 1) on (the full probe:
/// (1, 1) 100, (0, 1) and (1, 0) and (2147483647, 1) 0; (1, 2147483647)
/// gave 2.78e17, the ranks this table lacks).
fn artifact_total_power_cost(vm: &mut Vm, c: NativeCall) -> Ret {
    let (start, count) = (arg_whole(vm, &c, 1), arg_whole(vm, &c, 2));
    if start < 1.0 || count < 1.0 {
        return Ok(vec![Value::Num(0.0)]);
    }
    let first = start - 1.0;
    let last = (first + count).min(ARTIFACT_RANK_COSTS.len() as f64);
    let mut total = 0.0;
    let mut rank = first;
    while rank < last {
        total += artifact_rank_cost(rank);
        rank += 1.0;
    }
    Ok(vec![Value::Num(total)])
}

/// `C_DelvesUI.GetCurioLink(spellID, rarity)`: a curio link of the spell
/// (by ID or name) in its rarity's quality color, the second epic tier
/// epic; empty for a spell the game lacks (the full probe:
/// `|cnIQ1:|Hcurio:6603|h[Attack]|h|r` for (6603, 1) and ("Attack", 1),
/// IQ4 for rarity 5, "" for -1, 0 and "x").
fn curio_link(vm: &mut Vm, c: NativeCall) -> Ret {
    let spell = vm.hidden(c.arg(1));
    let rarity = arg_whole(vm, &c, 2).clamp(0.0, 4.0);
    let info = match spell {
        Value::Num(n) if n > 0.0 => ask(vm, "C_Spell.GetSpellInfo", &[spell]),
        Value::Str(_) => ask(vm, "C_Spell.GetSpellInfo", &[spell]),
        _ => vec![],
    };
    let Some(Value::Table(t)) = info.first().copied() else {
        return Ok(vec![vm.str("")]);
    };
    let (id, name) = (field(vm, t, "spellID"), field(vm, t, "name"));
    let (Value::Num(id), Value::Str(name)) = (id, name) else {
        return Ok(vec![vm.str("")]);
    };
    let name = String::from_utf8_lossy(vm.str_bytes(name)).into_owned();
    let link = format!("|cnIQ{}:|Hcurio:{}|h[{name}]|h|r", rarity as i64, id as i64);
    Ok(vec![vm.str(&link)])
}

// ------------------------------------------------------------ the list

/// The functions answered here (several with the same behaviour), and
/// the "nothing here" ones of the systems Forever lacks.
pub fn behaviours() -> Vec<(&'static str, NativeFn)> {
    let mut list: Vec<(&'static str, NativeFn)> = vec![
        (
            "C_DamageMeter.IsDamageMeterAvailable",
            damage_meter_available,
        ),
        // Tooltip data.
        ("C_TooltipInfo.GetSpellBookItem", tooltip_spellbook_item),
        ("C_TooltipInfo.GetAction", tooltip_action),
        ("C_TooltipInfo.GetInventoryItemByID", tooltip_item_id),
        ("C_TooltipInfo.GetOwnedItemByID", tooltip_item_id),
        ("C_TooltipInfo.GetMountBySpellID", tooltip_mount),
        ("C_TooltipInfo.GetMerchantItem", tooltip_merchant_item),
        ("C_TooltipInfo.GetBuybackItem", tooltip_buyback_item),
        ("C_TooltipInfo.GetLootItem", tooltip_loot_item),
        ("C_TooltipInfo.GetQuestItem", tooltip_quest_item),
        ("C_TooltipInfo.GetInboxItem", tooltip_inbox_item),
        ("C_TooltipInfo.GetSendMailItem", tooltip_send_mail_item),
        ("C_TooltipInfo.GetPetAction", tooltip_pet_action),
        ("C_TooltipInfo.GetShapeshift", tooltip_shapeshift),
        ("C_TooltipInfo.GetUnitAura", tooltip_unit_aura),
        ("C_TooltipInfo.GetUnitBuff", tooltip_unit_buff),
        ("C_TooltipInfo.GetUnitDebuff", tooltip_unit_debuff),
        (
            "C_TooltipInfo.GetUnitAuraByAuraInstanceID",
            tooltip_unit_aura_by_instance,
        ),
        (
            "C_TooltipInfo.GetUnitBuffByAuraInstanceID",
            tooltip_unit_buff_by_instance,
        ),
        (
            "C_TooltipInfo.GetUnitDebuffByAuraInstanceID",
            tooltip_unit_debuff_by_instance,
        ),
        ("C_TooltipInfo.GetWorldCursor", tooltip_world_cursor),
        // Interactions and trainers.
        (
            "C_PlayerInteractionManager.IsInteractingWithNpcOfType",
            is_interacting_with_npc_of_type,
        ),
        (
            "C_PlayerInteractionManager.IsValidNPCInteraction",
            is_valid_npc_interaction,
        ),
        ("C_Trainer.GetTrainerType", get_trainer_type),
        (
            "C_Trainer.GetCategorizeTrainerUI",
            get_categorize_trainer_ui,
        ),
        (
            "C_Trainer.SetCategorizeTrainerUI",
            set_categorize_trainer_ui,
        ),
        // Name plates, combat.
        (
            "C_NamePlateManager.SetNamePlateHitTestInsets",
            set_name_plate_hit_test_insets,
        ),
        (
            "C_NamePlateManager.GetNamePlateHitTestInsets",
            get_name_plate_hit_test_insets,
        ),
        ("C_CombatText.SetActiveUnit", set_combat_text_unit),
        ("C_CombatText.GetActiveUnit", get_combat_text_unit),
        (
            "C_SwingTimer.IsTargetWithinSwingRange",
            is_target_within_swing_range,
        ),
        // Creatures and races.
        ("C_CreatureInfo.GetCreatureID", get_creature_id),
        ("C_CreatureInfo.GetRaceInfo", get_race_info),
        ("C_CreatureInfo.GetFactionInfo", get_faction_info),
        // Misc.
        ("C_EditMode.IsValidLayoutName", is_valid_layout_name),
        ("C_Macro.GetMacroName", get_macro_name),
        (
            "C_ColorOverrides.GetDefaultColorForQuality",
            get_default_color_for_quality,
        ),
        ("C_LevelSquish.ConvertPlayerLevel", same_level),
        ("C_LevelSquish.ConvertFollowerLevel", same_level),
        (
            "C_NameUtil.ReplaceSurnameSeparatorWithLinkSeparator",
            same_name,
        ),
        (
            "C_CharacterServices.CapitalizeCharName",
            capitalize_char_name,
        ),
        // Auctions.
        ("C_AuctionHouse.HasFullBidResults", yes),
        ("C_AuctionHouse.HasFullOwnedAuctionResults", yes),
        (
            "C_AuctionHouse.GetTimeLeftBandInfo",
            get_time_left_band_info,
        ),
        (
            "C_AuctionHouse.GetItemKeySampleLink",
            get_item_key_sample_link,
        ),
        (
            "C_AuctionHouse.GetItemSearchResultsQuantity",
            get_item_search_results_quantity,
        ),
        (
            "C_AuctionHouse.GetMaxItemSearchResultBuyout",
            get_max_item_search_result_buyout,
        ),
        (
            "C_AuctionHouse.GetMaxItemSearchResultBid",
            get_max_item_search_result_bid,
        ),
        (
            "C_AuctionHouse.GetMaxCommoditySearchResultPrice",
            get_max_commodity_search_result_price,
        ),
        (
            "C_AuctionHouse.RequestMoreItemSearchResults",
            request_more_item_search_results,
        ),
        (
            "C_AuctionHouse.RequestMoreCommoditySearchResults",
            request_more_commodity_search_results,
        ),
        // Text.
        ("C_Intl.GetCurrentLocale", intl_current_locale),
        ("C_Intl.ToLower", intl_to_lower),
        ("C_Intl.ToUpper", intl_to_upper),
        ("C_Intl.ToTitle", intl_to_title),
        ("C_Intl.FoldCase", intl_fold_case),
        ("C_Intl.Normalize", intl_normalize),
        ("C_Intl.IsNormalized", intl_is_normalized),
        ("C_Intl.Length", intl_length),
        ("C_Intl.CompareStrings", intl_compare_strings),
        ("C_Intl.FormatNumber", intl_format_number),
        // Retail systems' game data.
        ("C_Commentator.GetTeamColor", commentator_team_color),
        ("C_Commentator.GetOrCreateSeries", commentator_series),
        ("C_EncounterTimeline.GetTrackInfo", timeline_track_info),
        ("C_EncounterTimeline.GetTrackList", timeline_track_list),
        ("C_EncounterTimeline.GetTrackType", timeline_track_type),
        (
            "C_EncounterTimeline.GetTrackMaxEventDuration",
            timeline_track_max_event_duration,
        ),
        ("C_EncounterTimeline.GetCurrentTime", timeline_current_time),
        ("C_ArtifactUI.GetCostForPointAtRank", artifact_cost_at_rank),
        ("C_ArtifactUI.GetTotalPowerCost", artifact_total_power_cost),
        ("C_DelvesUI.GetCurioLink", curio_link),
    ];
    list.extend(NOTHING.iter().map(|&n| (n, nothing as NativeFn)));
    list.extend(UNDOCUMENTED.iter().map(|&(n, _)| (n, nothing as NativeFn)));
    list
}

/// Documented functions of systems Forever lacks or the engine does not
/// model, answering their documented empty values (`nothing`).
const NOTHING: &[&str] = &[
    "C_AccountStore.BeginPurchase",
    "C_AccountStore.GetCategories",
    "C_AccountStore.GetCategoryInfo",
    "C_AccountStore.GetCategoryItems",
    "C_AccountStore.GetCurrencyAvailable",
    "C_AccountStore.GetCurrencyIDForStore",
    "C_AccountStore.GetCurrencyInfo",
    "C_AccountStore.GetItemInfo",
    "C_AccountStore.GetStoreFrontState",
    "C_AccountStore.RefundItem",
    "C_AccountStore.RequestStoreFrontInfoUpdate",
    "C_AchievementTelemetry.LinkAchievementInClub",
    "C_AchievementTelemetry.LinkAchievementInWhisper",
    "C_AchievementTelemetry.ShowAchievements",
    "C_AddOnProfiler.AddMeasuredCallEvent",
    "C_AddOnProfiler.AddPerformanceMessageShown",
    "C_AddOnProfiler.CheckForPerformanceMessage",
    "C_AddOnProfiler.GetAddOnMetric",
    "C_AddOnProfiler.GetApplicationMetric",
    "C_AddOnProfiler.GetOverallMetric",
    "C_AddOnProfiler.GetTicksPerSecond",
    "C_AddOnProfiler.GetTopKAddOnsForMetric",
    "C_AddOnProfiler.IsEnabled",
    "C_AdventureMap.GetAdventureMapTextureKit",
    "C_AdventureMap.GetNumMapInsets",
    "C_AdventureMap.GetNumQuestOffers",
    "C_AdventureMap.GetNumZoneChoices",
    "C_AdventureMap.GetQuestPortraitInfo",
    "C_AlliedRaces.GetAllRacialAbilitiesFromID",
    "C_AlliedRaces.GetRaceInfoByID",
    "C_AnimaDiversion.CloseUI",
    "C_AnimaDiversion.GetAnimaDiversionNodes",
    "C_AnimaDiversion.GetOriginPosition",
    "C_AnimaDiversion.GetReinforceProgress",
    "C_AnimaDiversion.GetTextureKit",
    "C_AnimaDiversion.OpenAnimaDiversionUI",
    "C_AnimaDiversion.SelectAnimaNode",
    "C_ArdenwealdGardening.GetGardenData",
    "C_ArdenwealdGardening.IsGardenAccessible",
    "C_ArtifactUI.AddPower",
    "C_ArtifactUI.ApplyCursorRelicToSlot",
    "C_ArtifactUI.CanApplyArtifactRelic",
    "C_ArtifactUI.CanApplyCursorRelicToSlot",
    "C_ArtifactUI.CanApplyRelicItemIDToEquippedArtifactSlot",
    "C_ArtifactUI.CanApplyRelicItemIDToSlot",
    "C_ArtifactUI.CheckRespecNPC",
    "C_ArtifactUI.Clear",
    "C_ArtifactUI.ClearForgeCamera",
    "C_ArtifactUI.ConfirmRespec",
    "C_ArtifactUI.DoesEquippedArtifactHaveAnyRelicsSlotted",
    "C_ArtifactUI.GetAppearanceInfo",
    "C_ArtifactUI.GetAppearanceInfoByID",
    "C_ArtifactUI.GetAppearanceSetInfo",
    "C_ArtifactUI.GetArtifactArtInfo",
    "C_ArtifactUI.GetArtifactInfo",
    "C_ArtifactUI.GetArtifactItemID",
    "C_ArtifactUI.GetArtifactTier",
    "C_ArtifactUI.GetArtifactXPRewardTargetInfo",
    "C_ArtifactUI.GetEquippedArtifactArtInfo",
    "C_ArtifactUI.GetEquippedArtifactItemID",
    "C_ArtifactUI.GetEquippedArtifactNumRelicSlots",
    "C_ArtifactUI.GetEquippedArtifactRelicInfo",
    "C_ArtifactUI.GetEquippedRelicLockedReason",
    "C_ArtifactUI.GetForgeRotation",
    "C_ArtifactUI.GetItemLevelIncreaseProvidedByRelic",
    "C_ArtifactUI.GetMetaPowerInfo",
    "C_ArtifactUI.GetNumAppearanceSets",
    "C_ArtifactUI.GetNumObtainedArtifacts",
    "C_ArtifactUI.GetNumRelicSlots",
    "C_ArtifactUI.GetPointsRemaining",
    "C_ArtifactUI.GetPowerHyperlink",
    "C_ArtifactUI.GetPowerInfo",
    "C_ArtifactUI.GetPowerLinks",
    "C_ArtifactUI.GetPowers",
    "C_ArtifactUI.GetPowersAffectedByRelic",
    "C_ArtifactUI.GetPowersAffectedByRelicItemLink",
    "C_ArtifactUI.GetPreviewAppearance",
    "C_ArtifactUI.GetRelicInfo",
    "C_ArtifactUI.GetRelicInfoByItemID",
    "C_ArtifactUI.GetRelicLockedReason",
    "C_ArtifactUI.GetRelicSlotType",
    "C_ArtifactUI.GetRespecArtifactArtInfo",
    "C_ArtifactUI.GetRespecArtifactInfo",
    "C_ArtifactUI.GetRespecCost",
    "C_ArtifactUI.GetTotalPurchasedRanks",
    "C_ArtifactUI.IsArtifactDisabled",
    "C_ArtifactUI.IsArtifactItem",
    "C_ArtifactUI.IsEquippedArtifactDisabled",
    "C_ArtifactUI.IsEquippedArtifactMaxed",
    "C_ArtifactUI.IsMaxedByRulesOrEffect",
    "C_ArtifactUI.IsPowerKnown",
    "C_ArtifactUI.IsViewedArtifactEquipped",
    "C_ArtifactUI.SetAppearance",
    "C_ArtifactUI.SetForgeCamera",
    "C_ArtifactUI.SetForgeRotation",
    "C_ArtifactUI.SetPreviewAppearance",
    "C_ArtifactUI.ShouldSuppressForgeRotation",
    "C_AssistedCombat.GetNextCastSpell",
    "C_AssistedCombat.IsAvailable",
    "C_AuctionHouse.CalculateCommodityDeposit",
    "C_AuctionHouse.CanCancelAuction",
    "C_AuctionHouse.CancelAuction",
    "C_AuctionHouse.CancelSell",
    "C_AuctionHouse.ConfirmPostCommodity",
    "C_AuctionHouse.ConfirmPostItem",
    "C_AuctionHouse.GetAuctionInfoByID",
    "C_AuctionHouse.GetBidInfo",
    "C_AuctionHouse.GetBidType",
    "C_AuctionHouse.GetBids",
    "C_AuctionHouse.GetCancelCost",
    "C_AuctionHouse.GetExtraBrowseInfo",
    "C_AuctionHouse.GetItemKeyRequiredLevel",
    "C_AuctionHouse.GetMaxBidItemBid",
    "C_AuctionHouse.GetMaxBidItemBuyout",
    "C_AuctionHouse.GetMaxOwnedAuctionBid",
    "C_AuctionHouse.GetMaxOwnedAuctionBuyout",
    "C_AuctionHouse.GetNumReplicateItems",
    "C_AuctionHouse.GetOwnedAuctionInfo",
    "C_AuctionHouse.GetOwnedAuctionType",
    "C_AuctionHouse.GetOwnedAuctions",
    "C_AuctionHouse.GetReplicateItemBattlePetInfo",
    "C_AuctionHouse.GetReplicateItemInfo",
    "C_AuctionHouse.GetReplicateItemLink",
    "C_AuctionHouse.GetReplicateItemTimeLeft",
    "C_AuctionHouse.HasMaxFavorites",
    "C_AuctionHouse.PlaceBid",
    "C_AuctionHouse.PostCommodity",
    "C_AuctionHouse.ReplicateItems",
    "C_AuctionHouse.RequestMoreBrowseResults",
    "C_AuctionHouse.RequestOwnedAuctionBidderInfo",
    "C_AuctionHouse.SearchForFavorites",
    "C_AuctionHouse.SearchForItemKeys",
    "C_AuctionHouse.SetFavoriteItem",
    "C_AutoComplete.GetAutoCompletePresenceID",
    "C_AutoComplete.GetAutoCompleteRealms",
    "C_AutoComplete.GetAutoCompleteResults",
    "C_AutoComplete.IsRecognizedName",
    "C_AutoLoot.SetUseAutoLootToggle",
    "C_AzeriteEmpoweredItem.CanSelectPower",
    "C_AzeriteEmpoweredItem.ConfirmAzeriteEmpoweredItemRespec",
    "C_AzeriteEmpoweredItem.GetAllTierInfo",
    "C_AzeriteEmpoweredItem.GetAllTierInfoByItemID",
    "C_AzeriteEmpoweredItem.GetAzeriteEmpoweredItemRespecCost",
    "C_AzeriteEmpoweredItem.GetPowerInfo",
    "C_AzeriteEmpoweredItem.GetPowerText",
    "C_AzeriteEmpoweredItem.GetSpecsForPower",
    "C_AzeriteEmpoweredItem.HasAnyUnselectedPowers",
    "C_AzeriteEmpoweredItem.HasBeenViewed",
    "C_AzeriteEmpoweredItem.IsAzeriteEmpoweredItem",
    "C_AzeriteEmpoweredItem.IsAzeritePreviewSourceDisplayable",
    "C_AzeriteEmpoweredItem.IsHeartOfAzerothEquipped",
    "C_AzeriteEmpoweredItem.IsPowerAvailableForSpec",
    "C_AzeriteEmpoweredItem.IsPowerSelected",
    "C_AzeriteEmpoweredItem.SelectPower",
    "C_AzeriteEmpoweredItem.SetHasBeenViewed",
    "C_AzeriteEssence.ActivateEssence",
    "C_AzeriteEssence.CanActivateEssence",
    "C_AzeriteEssence.CanDeactivateEssence",
    "C_AzeriteEssence.CanOpenUI",
    "C_AzeriteEssence.ClearPendingActivationEssence",
    "C_AzeriteEssence.CloseForge",
    "C_AzeriteEssence.GetEssenceHyperlink",
    "C_AzeriteEssence.GetEssenceInfo",
    "C_AzeriteEssence.GetEssences",
    "C_AzeriteEssence.GetMilestoneEssence",
    "C_AzeriteEssence.GetMilestoneInfo",
    "C_AzeriteEssence.GetMilestoneSpell",
    "C_AzeriteEssence.GetMilestones",
    "C_AzeriteEssence.GetNumUnlockedEssences",
    "C_AzeriteEssence.GetNumUsableEssences",
    "C_AzeriteEssence.GetPendingActivationEssence",
    "C_AzeriteEssence.HasNeverActivatedAnyEssences",
    "C_AzeriteEssence.HasPendingActivationEssence",
    "C_AzeriteEssence.IsAtForge",
    "C_AzeriteEssence.SetPendingActivationEssence",
    "C_AzeriteEssence.UnlockMilestone",
    "C_BehavioralMessaging.SendNotificationReceipt",
    "C_BlizzCon2026.GetExperience",
    "C_BlizzCon2026.IsActive",
    "C_BlizzCon2026.IsColdSwapFeatureEnabled",
    "C_BlizzCon2026.SetExperience",
    "C_Browser.CloseFullscreenBrowser",
    "C_CampaignInfo.GetAvailableCampaigns",
    "C_CampaignInfo.GetCampaignChapterInfo",
    "C_CampaignInfo.GetCampaignID",
    "C_CampaignInfo.GetCampaignInfo",
    "C_CampaignInfo.GetChapterIDs",
    "C_CampaignInfo.GetCurrentChapterID",
    "C_CampaignInfo.GetFailureReason",
    "C_CampaignInfo.GetState",
    "C_CampaignInfo.IsCampaignQuest",
    "C_CampaignInfo.SortAsNormalQuest",
    "C_CatalogShop.BulkPurchaseProducts",
    "C_CatalogShop.BulkRefundDecors",
    "C_CatalogShop.CloseCatalogShopInteraction",
    "C_CatalogShop.ConfirmHousingPurchase",
    "C_CatalogShop.FindBestCurrencyProductForNeededAmount",
    "C_CatalogShop.GetAvailableTransmogRaceInfos",
    "C_CatalogShop.GetCatalogShopProductDisplayInfo",
    "C_CatalogShop.GetCategoryInfo",
    "C_CatalogShop.GetCategorySectionInfo",
    "C_CatalogShop.GetFailureInfo",
    "C_CatalogShop.GetFirstCategoryByProductID",
    "C_CatalogShop.GetNewProducts",
    "C_CatalogShop.GetProductAvailabilityTimeRemainingSecs",
    "C_CatalogShop.GetProductIDsForBundle",
    "C_CatalogShop.GetProductIDsForCategory",
    "C_CatalogShop.GetProductIDsForCategorySection",
    "C_CatalogShop.GetProductInfo",
    "C_CatalogShop.GetProductSortOrder",
    "C_CatalogShop.GetRefundableDecors",
    "C_CatalogShop.GetSectionIDsForCategory",
    "C_CatalogShop.GetSpellVisualInfoForMount",
    "C_CatalogShop.GetVCProductInfos",
    "C_CatalogShop.GetVirtualCurrencyBalance",
    "C_CatalogShop.IsProductIncludedInAnyBundle",
    "C_CatalogShop.OnLegalDisclaimerClicked",
    "C_CatalogShop.OnLegalPersonalizedOptOutClicked",
    "C_CatalogShop.OpenCatalogShopInteractionFromHouse",
    "C_CatalogShop.OpenCatalogShopInteractionFromShop",
    "C_CatalogShop.ProductDisplayedTelemetry",
    "C_CatalogShop.ProductSelectedTelemetry",
    "C_CatalogShop.PurchaseProduct",
    "C_CatalogShop.RefreshRefundableDecors",
    "C_CatalogShop.RefreshVirtualCurrencyBalance",
    "C_CatalogShop.ShouldShowHousingWarning",
    "C_CatalogShop.StartHousingVCPurchaseConfirmation",
    "C_ChromieTime.CloseUI",
    "C_ChromieTime.GetChromieTimeExpansionOption",
    "C_ChromieTime.GetChromieTimeExpansionOptions",
    "C_ChromieTime.SelectChromieTimeOption",
    "C_ClickBindings.ExecuteBinding",
    "C_ClickBindings.ResetCurrentProfile",
    "C_ClickBindings.SetProfileByInfo",
    "C_ClickBindings.SetTutorialShown",
    "C_ColorOverrides.ClearColorOverrides",
    "C_ColorOverrides.RemoveColorOverride",
    "C_ColorOverrides.SetColorOverride",
    "C_CombatAudioAlert.AddToKnownTargetingList",
    "C_CombatAudioAlert.GetCategoryVoice",
    "C_CombatAudioAlert.RemoveFromKnownTargetingList",
    "C_CombatAudioAlert.SetCategoryVoice",
    "C_CombatAudioAlert.SetCategoryVolume",
    "C_CombatAudioAlert.SetFormatSetting",
    "C_CombatAudioAlert.SetSpeakerSpeed",
    "C_CombatAudioAlert.SetSpecSetting",
    "C_CombatAudioAlert.SetThrottle",
    "C_CombatAudioAlert.SpeakText",
    "C_CombatLog.ApplyFilterSettings",
    "C_CombatLog.AreFilteredEventsEnabled",
    "C_CombatLog.ClearEntries",
    "C_CombatLog.GetEntryRetentionTime",
    "C_CombatLog.GetMessageLimit",
    "C_CombatLog.IsCombatLogRestricted",
    "C_CombatLog.RefilterEntries",
    "C_CombatLog.SetEntryRetentionTime",
    "C_CombatLog.SetFilteredEventsEnabled",
    "C_CombatLog.SetMessageLimit",
    "C_CombatText.GetCurrentEventInfo",
    "C_Commentator.AddPlayerOverrideName",
    "C_Commentator.AddTrackedDefensiveAuras",
    "C_Commentator.AddTrackedOffensiveAuras",
    "C_Commentator.AreTeamsSwapped",
    "C_Commentator.AssignPlayerToTeam",
    "C_Commentator.AssignPlayersToTeam",
    "C_Commentator.AssignPlayersToTeamInCurrentInstance",
    "C_Commentator.CanUseCommentatorCheats",
    "C_Commentator.ClearCameraTarget",
    "C_Commentator.ClearFollowTarget",
    "C_Commentator.ClearLookAtTarget",
    "C_Commentator.EnterInstance",
    "C_Commentator.ExitInstance",
    "C_Commentator.FindSpectatedUnit",
    "C_Commentator.FindTeamNameInCurrentInstance",
    "C_Commentator.FindTeamNameInDirectory",
    "C_Commentator.FlushCommentatorHistory",
    "C_Commentator.FollowPlayer",
    "C_Commentator.FollowUnit",
    "C_Commentator.ForceFollowTransition",
    "C_Commentator.GetAdditionalCameraWeight",
    "C_Commentator.GetAdditionalCameraWeightByToken",
    "C_Commentator.GetAllPlayerOverrideNames",
    "C_Commentator.GetCamera",
    "C_Commentator.GetCameraCollision",
    "C_Commentator.GetCameraPosition",
    "C_Commentator.GetCombatEventInfo",
    "C_Commentator.GetCommentatorHistory",
    "C_Commentator.GetCommentatorMatchDataState",
    "C_Commentator.GetCurrentMapID",
    "C_Commentator.GetDampeningPercent",
    "C_Commentator.GetDistanceBeforeForcedHorizontalConvergence",
    "C_Commentator.GetDurationToForceHorizontalConvergence",
    "C_Commentator.GetExcludeDistance",
    "C_Commentator.GetHardlockWeight",
    "C_Commentator.GetHorizontalAngleThresholdToSmooth",
    "C_Commentator.GetIndirectSpellID",
    "C_Commentator.GetInstanceInfo",
    "C_Commentator.GetLookAtLerpAmount",
    "C_Commentator.GetMapInfo",
    "C_Commentator.GetMatchDuration",
    "C_Commentator.GetMaxNumPlayersPerTeam",
    "C_Commentator.GetMaxNumTeams",
    "C_Commentator.GetMode",
    "C_Commentator.GetMsToHoldForHorizontalMovement",
    "C_Commentator.GetMsToHoldForVerticalMovement",
    "C_Commentator.GetMsToSmoothHorizontalChange",
    "C_Commentator.GetMsToSmoothVerticalChange",
    "C_Commentator.GetNumMaps",
    "C_Commentator.GetNumPlayers",
    "C_Commentator.GetPlayerAuraInfo",
    "C_Commentator.GetPlayerAuraInfoByUnit",
    "C_Commentator.GetPlayerCooldownInfo",
    "C_Commentator.GetPlayerCooldownInfoByUnit",
    "C_Commentator.GetPlayerCrowdControlInfo",
    "C_Commentator.GetPlayerCrowdControlInfoByUnit",
    "C_Commentator.GetPlayerData",
    "C_Commentator.GetPlayerFlagInfo",
    "C_Commentator.GetPlayerFlagInfoByUnit",
    "C_Commentator.GetPlayerItemCooldownInfo",
    "C_Commentator.GetPlayerItemCooldownInfoByUnit",
    "C_Commentator.GetPlayerOverrideName",
    "C_Commentator.GetPlayerSpellCharges",
    "C_Commentator.GetPlayerSpellChargesByUnit",
    "C_Commentator.GetPositionLerpAmount",
    "C_Commentator.GetSmoothFollowTransitioning",
    "C_Commentator.GetSoftlockWeight",
    "C_Commentator.GetSpeedFactor",
    "C_Commentator.GetStartLocation",
    "C_Commentator.GetTeamColorByUnit",
    "C_Commentator.GetTimeLeftInMatch",
    "C_Commentator.GetTrackedSpellID",
    "C_Commentator.GetTrackedSpells",
    "C_Commentator.GetTrackedSpellsByUnit",
    "C_Commentator.GetUnitData",
    "C_Commentator.GetWargameInfo",
    "C_Commentator.HasTrackedAuras",
    "C_Commentator.IsSmartCameraLocked",
    "C_Commentator.IsTrackedDefensiveAura",
    "C_Commentator.IsTrackedOffensiveAura",
    "C_Commentator.IsTrackedSpell",
    "C_Commentator.IsTrackedSpellByUnit",
    "C_Commentator.IsUsingSmartCamera",
    "C_Commentator.LookAtPlayer",
    "C_Commentator.RemoveAllOverrideNames",
    "C_Commentator.RemovePlayerOverrideName",
    "C_Commentator.RequestPlayerCooldownInfo",
    "C_Commentator.ResetFoVTarget",
    "C_Commentator.ResetSeriesScores",
    "C_Commentator.ResetSettings",
    "C_Commentator.ResetTrackedAuras",
    "C_Commentator.SendAddonMessage",
    "C_Commentator.SendAddonMessageLogged",
    "C_Commentator.SetAdditionalCameraWeight",
    "C_Commentator.SetAdditionalCameraWeightByToken",
    "C_Commentator.SetBlocklistedAuras",
    "C_Commentator.SetBlocklistedCooldowns",
    "C_Commentator.SetBlocklistedItemCooldowns",
    "C_Commentator.SetCamera",
    "C_Commentator.SetCameraCollision",
    "C_Commentator.SetCameraPosition",
    "C_Commentator.SetCheatsEnabled",
    "C_Commentator.SetCommentatorHistory",
    "C_Commentator.SetDistanceBeforeForcedHorizontalConvergence",
    "C_Commentator.SetDurationToForceHorizontalConvergence",
    "C_Commentator.SetExcludeDistance",
    "C_Commentator.SetFollowCameraSpeeds",
    "C_Commentator.SetHardlockWeight",
    "C_Commentator.SetHorizontalAngleThresholdToSmooth",
    "C_Commentator.SetLookAtLerpAmount",
    "C_Commentator.SetMapAndInstanceIndex",
    "C_Commentator.SetMouseDisabled",
    "C_Commentator.SetMoveSpeed",
    "C_Commentator.SetMsToHoldForHorizontalMovement",
    "C_Commentator.SetMsToHoldForVerticalMovement",
    "C_Commentator.SetMsToSmoothHorizontalChange",
    "C_Commentator.SetMsToSmoothVerticalChange",
    "C_Commentator.SetPositionLerpAmount",
    "C_Commentator.SetRequestedDebuffCooldowns",
    "C_Commentator.SetRequestedDefensiveCooldowns",
    "C_Commentator.SetRequestedItemCooldowns",
    "C_Commentator.SetRequestedOffensiveCooldowns",
    "C_Commentator.SetSeriesScore",
    "C_Commentator.SetSeriesScores",
    "C_Commentator.SetSmartCameraLocked",
    "C_Commentator.SetSmoothFollowTransitioning",
    "C_Commentator.SetSoftlockWeight",
    "C_Commentator.SetSpeedFactor",
    "C_Commentator.SetTargetHeightOffset",
    "C_Commentator.SetUseSmartCamera",
    "C_Commentator.SnapCameraLookAtPoint",
    "C_Commentator.SpellUsesItemCharges",
    "C_Commentator.StartWargame",
    "C_Commentator.SwapTeamSides",
    "C_Commentator.ToggleCheats",
    "C_Commentator.UpdateMapInfo",
    "C_Commentator.UpdatePlayerInfo",
    "C_Commentator.ZoomIn",
    "C_Commentator.ZoomIn_Position",
    "C_Commentator.ZoomOut",
    "C_Commentator.ZoomOut_Position",
    "C_ConsoleScriptCollection.GetCollectionDataByID",
    "C_ConsoleScriptCollection.GetCollectionDataByTag",
    "C_ConsoleScriptCollection.GetElements",
    "C_ConsoleScriptCollection.GetScriptData",
    "C_ContributionCollector.Close",
    "C_ContributionCollector.Contribute",
    "C_ContributionCollector.GetActive",
    "C_ContributionCollector.GetAtlases",
    "C_ContributionCollector.GetBuffs",
    "C_ContributionCollector.GetContributionAppearance",
    "C_ContributionCollector.GetContributionResult",
    "C_ContributionCollector.GetDescription",
    "C_ContributionCollector.GetManagedContributionsForCreatureID",
    "C_ContributionCollector.GetName",
    "C_ContributionCollector.GetOrderIndex",
    "C_ContributionCollector.GetRequiredContributionCurrency",
    "C_ContributionCollector.GetRequiredContributionItem",
    "C_ContributionCollector.GetRewardQuestID",
    "C_ContributionCollector.GetState",
    "C_ContributionCollector.HasPendingContribution",
    "C_ContributionCollector.IsAwaitingRewardQuestData",
    "C_CovenantCallings.AreCallingsUnlocked",
    "C_CovenantCallings.RequestCallings",
    "C_CovenantPreview.CloseFromUI",
    "C_CovenantPreview.GetCovenantInfoForPlayerChoiceResponseID",
    "C_CovenantSanctumUI.CanAccessReservoir",
    "C_CovenantSanctumUI.CanDepositAnima",
    "C_CovenantSanctumUI.DepositAnima",
    "C_CovenantSanctumUI.EndInteraction",
    "C_CovenantSanctumUI.GetAnimaInfo",
    "C_CovenantSanctumUI.GetCurrentTalentTreeID",
    "C_CovenantSanctumUI.GetFeatures",
    "C_CovenantSanctumUI.GetRenownLevel",
    "C_CovenantSanctumUI.GetRenownLevels",
    "C_CovenantSanctumUI.GetRenownRewardsForLevel",
    "C_CovenantSanctumUI.GetSanctumType",
    "C_CovenantSanctumUI.GetSoulCurrencies",
    "C_CovenantSanctumUI.HasMaximumRenown",
    "C_CovenantSanctumUI.IsPlayerInRenownCatchUpMode",
    "C_CovenantSanctumUI.IsWeeklyRenownCapped",
    "C_CovenantSanctumUI.RequestCatchUpState",
    "C_Covenants.GetActiveCovenantID",
    "C_Covenants.GetCovenantData",
    "C_Covenants.GetCovenantIDs",
    "C_CreatureInfo.GetCreatureFamilyIDs",
    "C_CreatureInfo.GetCreatureFamilyInfo",
    "C_CreatureInfo.GetCreatureTypeIDs",
    "C_CreatureInfo.GetCreatureTypeInfo",
    "C_CurveUtil.EvaluateGameCurve",
    "C_DamageMeter.GetAvailableCombatSessions",
    "C_DamageMeter.GetCombatSessionFromID",
    "C_DamageMeter.GetCombatSessionFromType",
    "C_DamageMeter.GetCombatSessionSourceFromID",
    "C_DamageMeter.GetCombatSessionSourceFromType",
    "C_DamageMeter.ResetAllCombatSessions",
    "C_Discord.Authorize",
    "C_Discord.GetDiscordChannelName",
    "C_Discord.GetDiscordUserID",
    "C_Discord.GetDiscordUserName",
    "C_Discord.GetDisplayNameType",
    "C_Discord.GetNumDiscordChannels",
    "C_Discord.GetNumDiscordServers",
    "C_Discord.GetServerLinkableChannels",
    "C_Discord.GetServerName",
    "C_Discord.GuildLink",
    "C_Discord.GuildUnlink",
    "C_Discord.IsGuildChannelLinked",
    "C_Discord.IsGuildSettingSet",
    "C_Discord.RefreshAuth",
    "C_Discord.SetGuildSetting",
    "C_Discord.UpdateDiscordServers",
    "C_Discord.UpdateGuildLobby",
    "C_DyeColor.GetAllDyeColorCategories",
    "C_DyeColor.GetAllDyeColors",
    "C_DyeColor.GetDyeColorCategoryInfo",
    "C_DyeColor.GetDyeColorInfo",
    "C_DyeColor.GetDyeColorsForItem",
    "C_DyeColor.GetDyeColorsForItemLocation",
    "C_DyeColor.GetDyeColorsInCategory",
    "C_DyeColor.IsDyeColorOwned",
    "C_EditMode.ConvertLayoutInfoToString",
    "C_EditMode.ConvertStringToLayoutInfo",
    "C_EditMode.OnEditModeExit",
    "C_EditMode.OnLayoutAdded",
    "C_EditMode.OnLayoutDeleted",
    "C_EditMode.SetAccountSetting",
    "C_EncounterEvents.GetEventColor",
    "C_EncounterEvents.GetEventInfo",
    "C_EncounterEvents.GetEventList",
    "C_EncounterEvents.GetEventSound",
    "C_EncounterEvents.HasEventInfo",
    "C_EncounterEvents.PlayEventSound",
    "C_EncounterEvents.SetEventColor",
    "C_EncounterEvents.SetEventSound",
    "C_EncounterTimeline.AddEditModeEvents",
    "C_EncounterTimeline.AddScriptEvent",
    "C_EncounterTimeline.CancelAllScriptEvents",
    "C_EncounterTimeline.CancelEditModeEvents",
    "C_EncounterTimeline.CancelScriptEvent",
    "C_EncounterTimeline.FinishScriptEvent",
    "C_EncounterTimeline.GetEventColor",
    "C_EncounterTimeline.GetEventCountBySource",
    "C_EncounterTimeline.GetEventHighlightTime",
    "C_EncounterTimeline.GetEventInfo",
    "C_EncounterTimeline.GetEventList",
    "C_EncounterTimeline.GetEventState",
    "C_EncounterTimeline.GetEventTimeElapsed",
    "C_EncounterTimeline.GetEventTimeRemaining",
    "C_EncounterTimeline.GetEventTimer",
    "C_EncounterTimeline.GetEventTrack",
    "C_EncounterTimeline.GetSortedEventList",
    "C_EncounterTimeline.GetViewType",
    "C_EncounterTimeline.HasActiveEvents",
    "C_EncounterTimeline.HasAnyEvents",
    "C_EncounterTimeline.HasPausedEvents",
    "C_EncounterTimeline.HasVisibleEvents",
    "C_EncounterTimeline.IsEventBlocked",
    "C_EncounterTimeline.IsFeatureAvailable",
    "C_EncounterTimeline.IsFeatureEnabled",
    "C_EncounterTimeline.PauseScriptEvent",
    "C_EncounterTimeline.ResumeScriptEvent",
    "C_EncounterTimeline.SetEventIconTextures",
    "C_EncounterTimeline.SetViewType",
    "C_EncounterWarnings.GetColorForSeverity",
    "C_EncounterWarnings.GetEditModeWarningInfo",
    "C_EncounterWarnings.GetPlayCustomSoundsWhenHidden",
    "C_EncounterWarnings.GetSoundKitForSeverity",
    "C_EncounterWarnings.GetWarningsShown",
    "C_EncounterWarnings.IsFeatureAvailable",
    "C_EncounterWarnings.IsFeatureEnabled",
    "C_EncounterWarnings.PlaySound",
    "C_EncounterWarnings.SetPlayCustomSoundsWhenHidden",
    "C_EncounterWarnings.SetWarningsShown",
    "C_EndOfMatchUI.GetEndOfMatchDetails",
    "C_EventScheduler.ClearReminder",
    "C_EventScheduler.GetActiveContinentName",
    "C_EventScheduler.GetEventUiMapID",
    "C_EventScheduler.GetEventZoneName",
    "C_EventScheduler.GetOngoingEvents",
    "C_EventScheduler.GetScheduledEvents",
    "C_EventScheduler.HasData",
    "C_EventScheduler.RequestEvents",
    "C_EventScheduler.SetReminder",
    "C_ExpansionTrial.OnTrialLevelUpDialogClicked",
    "C_ExpansionTrial.OnTrialLevelUpDialogShown",
    "C_ExternalEventURL.IsNew",
    "C_ExternalEventURL.LaunchURL",
    "C_Flyout.FlyoutHasSpell",
    "C_Flyout.GetFlyoutID",
    "C_Flyout.GetFlyoutInfo",
    "C_Flyout.GetFlyoutSlotInfo",
    "C_Flyout.GetFlyoutTexture",
    "C_Flyout.GetNumFlyouts",
    "C_GamepadTargeting.Disable",
    "C_GamepadTargeting.Enable",
    "C_GamepadTargeting.SetFilter",
    "C_GamepadUI.GetFirstGamepadActionBarStorageSlotIndexForActiveStance",
    "C_GamepadUI.IsValidGamepadActionStorageSlotIndex",
    "C_GamepadUI.IsValidGamepadPossessBarStorageSlotIndex",
    "C_Garrison.AddFollowerToMission",
    "C_Garrison.GetAutoCombatDamageClassValues",
    "C_Garrison.GetAutoMissionBoardState",
    "C_Garrison.GetAutoMissionEnvironmentEffect",
    "C_Garrison.GetAutoMissionTargetingInfo",
    "C_Garrison.GetAutoMissionTargetingInfoForSpell",
    "C_Garrison.GetAutoTroops",
    "C_Garrison.GetCombatLogSpellInfo",
    "C_Garrison.GetCurrentCypherEquipmentLevel",
    "C_Garrison.GetCurrentGarrTalentTreeFriendshipFactionID",
    "C_Garrison.GetCurrentGarrTalentTreeID",
    "C_Garrison.GetCyphersToNextEquipmentLevel",
    "C_Garrison.GetFollowerAutoCombatSpells",
    "C_Garrison.GetFollowerAutoCombatStats",
    "C_Garrison.GetFollowerMissionCompleteInfo",
    "C_Garrison.GetGarrisonTalentTreeCurrencyTypes",
    "C_Garrison.GetGarrisonTalentTreeType",
    "C_Garrison.GetMaxCypherEquipmentLevel",
    "C_Garrison.GetMissionCompleteEncounters",
    "C_Garrison.GetMissionDeploymentInfo",
    "C_Garrison.GetMissionEncounterIconInfo",
    "C_Garrison.GetTalentInfo",
    "C_Garrison.GetTalentPointsSpentInTalentTree",
    "C_Garrison.GetTalentTreeIDsByClassID",
    "C_Garrison.GetTalentTreeInfo",
    "C_Garrison.GetTalentTreeResetInfo",
    "C_Garrison.GetTalentTreeTalentPointResearchInfo",
    "C_Garrison.GetTalentUnlockWorldQuest",
    "C_Garrison.HasAdventures",
    "C_Garrison.IsAtGarrisonMissionNPC",
    "C_Garrison.IsEnvironmentCountered",
    "C_Garrison.IsFollowerOnCompletedMission",
    "C_Garrison.IsLandingPageMinimapButtonVisible",
    "C_Garrison.IsTalentConditionMet",
    "C_Garrison.RegenerateCombatLog",
    "C_Garrison.RemoveFollowerFromMission",
    "C_Garrison.RushHealAllFollowers",
    "C_Garrison.RushHealFollower",
    "C_Garrison.SetAutoCombatSpellFastForward",
    "C_GenericWidgetDisplay.Acknowledge",
    "C_GenericWidgetDisplay.Close",
    "C_HouseEditor.ActivateHouseEditorMode",
    "C_HouseEditor.EnterHouseEditor",
    "C_HouseEditor.GetActiveHouseEditorMode",
    "C_HouseEditor.GetHouseEditorAvailability",
    "C_HouseEditor.GetHouseEditorModeAvailability",
    "C_HouseEditor.GetHouseEditorPlayerType",
    "C_HouseEditor.IsHouseEditorActive",
    "C_HouseEditor.IsHouseEditorModeActive",
    "C_HouseEditor.IsHouseEditorStatusAvailable",
    "C_HouseEditor.LeaveHouseEditor",
    "C_HouseExterior.CancelActiveExteriorEditing",
    "C_HouseExterior.GetCoreFixtureOptionsInfo",
    "C_HouseExterior.GetCurrentHouseExteriorSize",
    "C_HouseExterior.GetCurrentHouseExteriorType",
    "C_HouseExterior.GetHouseExteriorSizeOptions",
    "C_HouseExterior.GetHouseExteriorTypeOptions",
    "C_HouseExterior.GetSelectedFixturePointInfo",
    "C_HouseExterior.HasHoveredFixture",
    "C_HouseExterior.HasSelectedFixturePoint",
    "C_HouseExterior.IsAnyDecorAttachedToCoreFixture",
    "C_HouseExterior.IsAnyDecorAttachedToDoor",
    "C_HouseExterior.IsAnyDecorAttachedToHouseExterior",
    "C_HouseExterior.IsAnyDecorAttachedToSelectedFixturePoint",
    "C_HouseExterior.IsExteriorDecorHidden",
    "C_HouseExterior.RemoveFixtureFromSelectedPoint",
    "C_HouseExterior.SelectCoreFixtureOption",
    "C_HouseExterior.SelectFixtureOption",
    "C_HouseExterior.SetExteriorDecorHidden",
    "C_HouseExterior.SetHouseExteriorSize",
    "C_HouseExterior.SetHouseExteriorType",
    "C_Housing.AcceptNeighborhoodOwnership",
    "C_Housing.CanEditCharter",
    "C_Housing.CanTakeReportScreenshot",
    "C_Housing.CreateGuildNeighborhood",
    "C_Housing.CreateNeighborhoodCharter",
    "C_Housing.DeclineNeighborhoodOwnership",
    "C_Housing.DoesFactionMatchNeighborhood",
    "C_Housing.EditNeighborhoodCharter",
    "C_Housing.GetCurrentHouseInfo",
    "C_Housing.GetCurrentHouseLevelFavor",
    "C_Housing.GetCurrentHouseRefundAmount",
    "C_Housing.GetCurrentNeighborhoodGUID",
    "C_Housing.GetHouseLevelFavorForLevel",
    "C_Housing.GetHouseLevelRewardsForLevel",
    "C_Housing.GetHousingAccessFlags",
    "C_Housing.GetMaxHouseLevel",
    "C_Housing.GetNeighborhoodTextureSuffix",
    "C_Housing.GetOthersOwnedHouses",
    "C_Housing.GetPlayerOwnedHouses",
    "C_Housing.GetUIMapIDForNeighborhood",
    "C_Housing.GetVisitCooldownInfo",
    "C_Housing.HasHousingExpansionAccess",
    "C_Housing.HouseFinderDeclineNeighborhoodInvitation",
    "C_Housing.HouseFinderIgnoreNeighborhood",
    "C_Housing.HouseFinderRequestNeighborhoods",
    "C_Housing.HouseFinderRequestReservationAndPort",
    "C_Housing.IsHousingMarketCartFullRemoveEnabled",
    "C_Housing.IsHousingMarketEnabled",
    "C_Housing.IsHousingMarketShopEnabled",
    "C_Housing.IsInsideOwnedHouse",
    "C_Housing.IsInsideOwnedHouseOrPlot",
    "C_Housing.IsInsideOwnedPlot",
    "C_Housing.IsInsidePlot",
    "C_Housing.LeaveHouse",
    "C_Housing.OnCharterConfirmationAccepted",
    "C_Housing.OnCharterConfirmationClosed",
    "C_Housing.OnCreateCharterNeighborhoodClosed",
    "C_Housing.OnCreateGuildNeighborhoodClosed",
    "C_Housing.OnHouseFinderClickPlot",
    "C_Housing.OnRequestSignatureClicked",
    "C_Housing.OnSignCharterClicked",
    "C_Housing.RelinquishHouse",
    "C_Housing.RequestCurrentHouseInfo",
    "C_Housing.RequestHouseFinderNeighborhoodData",
    "C_Housing.RequestPlayerCharacterList",
    "C_Housing.ResetHouse",
    "C_Housing.ReturnAfterVisitingHouse",
    "C_Housing.SaveHouseSettings",
    "C_Housing.SearchBNetFriendNeighborhoods",
    "C_Housing.SearchBNetFriendNeighborhoodsByID",
    "C_Housing.SetTrackedHouseGuid",
    "C_Housing.StartTutorial",
    "C_Housing.TeleportHome",
    "C_Housing.TryRenameNeighborhood",
    "C_Housing.ValidateCreateGuildNeighborhoodSize",
    "C_Housing.ValidateNeighborhoodName",
    "C_Housing.VisitHouse",
    "C_HousingBasicMode.CancelActiveEditing",
    "C_HousingBasicMode.CommitDecorMovement",
    "C_HousingBasicMode.CommitHouseExteriorPosition",
    "C_HousingBasicMode.FinishPlacingNewDecor",
    "C_HousingBasicMode.GetHoveredDecorInfo",
    "C_HousingBasicMode.GetSelectedDecorInfo",
    "C_HousingBasicMode.IsDecorSelected",
    "C_HousingBasicMode.IsFreePlaceEnabled",
    "C_HousingBasicMode.IsGridSnapEnabled",
    "C_HousingBasicMode.IsGridVisible",
    "C_HousingBasicMode.IsHouseExteriorHovered",
    "C_HousingBasicMode.IsHouseExteriorSelected",
    "C_HousingBasicMode.IsHoveringDecor",
    "C_HousingBasicMode.IsPlacingNewDecor",
    "C_HousingBasicMode.RemoveSelectedDecor",
    "C_HousingBasicMode.RotateDecor",
    "C_HousingBasicMode.RotateHouseExterior",
    "C_HousingBasicMode.SetFreePlaceEnabled",
    "C_HousingBasicMode.SetGridSnapEnabled",
    "C_HousingBasicMode.SetGridVisible",
    "C_HousingBasicMode.StartPlacingPreviewDecor",
    "C_HousingBlueprint.CanExportRoom",
    "C_HousingBlueprint.CanExportTypeFromCurrentLocation",
    "C_HousingBlueprint.CanImportTypeFromCurrentLocation",
    "C_HousingBlueprint.DeleteBlueprint",
    "C_HousingBlueprint.ExportBlueprint",
    "C_HousingBlueprint.ExportRoomBlueprint",
    "C_HousingBlueprint.GetBlueprintHyperlink",
    "C_HousingBlueprint.GetBlueprintTypeForCode",
    "C_HousingBlueprint.GetExportAvailability",
    "C_HousingBlueprint.GetFeatureAvailability",
    "C_HousingBlueprint.GetImportAvailability",
    "C_HousingBlueprint.ImportBlueprint",
    "C_HousingBlueprint.IsShareCodeValid",
    "C_HousingBlueprint.RenameBlueprint",
    "C_HousingBlueprint.RequestBlueprintCollection",
    "C_HousingBlueprint.RequestBlueprintContents",
    "C_HousingBlueprint.RequestBlueprintContentsForContext",
    "C_HousingBlueprint.StartImportRoomBlueprint",
    "C_HousingBlueprint.UpdateBlueprintStringFromInput",
    "C_HousingCatalog.DeletePreviewCartDecor",
    "C_HousingCatalog.GetAllFilterTagGroups",
    "C_HousingCatalog.GetAllVariantInfosForEntry",
    "C_HousingCatalog.GetBundleInfo",
    "C_HousingCatalog.GetCartSizeLimit",
    "C_HousingCatalog.GetCatalogCategoryAndSubcategoryNames",
    "C_HousingCatalog.GetCatalogEntryRefundTimeStampByRecordID",
    "C_HousingCatalog.GetCatalogEntryVariantInfo",
    "C_HousingCatalog.GetDecorMaxOwnedCount",
    "C_HousingCatalog.GetDecorTotalOwnedCount",
    "C_HousingCatalog.GetDestroyableInstanceCount",
    "C_HousingCatalog.GetFeaturedBundles",
    "C_HousingCatalog.GetFeaturedSmallProducts",
    "C_HousingCatalog.GetMarketInfoForDecor",
    "C_HousingCatalog.HasFeaturedEntries",
    "C_HousingCatalog.HousingMarketActionAddToCart",
    "C_HousingCatalog.HousingMarketActionClearCart",
    "C_HousingCatalog.HousingMarketActionRemoveFromCart",
    "C_HousingCatalog.HousingMarketActionViewBundle",
    "C_HousingCatalog.HousingMarketActionViewInStore",
    "C_HousingCatalog.IsPreviewCartItemShown",
    "C_HousingCatalog.PromotePreviewDecor",
    "C_HousingCatalog.RequestHousingMarketInfoRefresh",
    "C_HousingCatalog.RequestHousingMarketRefundInfo",
    "C_HousingCatalog.SetPreviewCartItemShown",
    "C_HousingCleanupMode.GetHoveredDecorInfo",
    "C_HousingCleanupMode.IsHoveringDecor",
    "C_HousingCleanupMode.RemoveSelectedDecor",
    "C_HousingCustomizeMode.ApplyDyeToSelectedDecor",
    "C_HousingCustomizeMode.ApplyPetToSelectedDecor",
    "C_HousingCustomizeMode.ApplyThemeToRoom",
    "C_HousingCustomizeMode.ApplyThemeToSelectedRoomComponent",
    "C_HousingCustomizeMode.ApplyWallpaperToAllWalls",
    "C_HousingCustomizeMode.ApplyWallpaperToSelectedRoomComponent",
    "C_HousingCustomizeMode.CancelActiveEditing",
    "C_HousingCustomizeMode.ClearDyesForSelectedDecor",
    "C_HousingCustomizeMode.ClearTargetRoomComponent",
    "C_HousingCustomizeMode.CommitDyesForSelectedDecor",
    "C_HousingCustomizeMode.GetHoveredDecorInfo",
    "C_HousingCustomizeMode.GetHoveredRoomComponentInfo",
    "C_HousingCustomizeMode.GetNumDyesToRemoveOnSelectedDecor",
    "C_HousingCustomizeMode.GetNumDyesToSpendOnSelectedDecor",
    "C_HousingCustomizeMode.GetPreviewDyesOnSelectedDecor",
    "C_HousingCustomizeMode.GetRecentlyUsedDyes",
    "C_HousingCustomizeMode.GetRecentlyUsedThemeSets",
    "C_HousingCustomizeMode.GetRecentlyUsedWallpapers",
    "C_HousingCustomizeMode.GetSelectedDecorInfo",
    "C_HousingCustomizeMode.GetSelectedDecorPetInfo",
    "C_HousingCustomizeMode.GetSelectedRoomComponentInfo",
    "C_HousingCustomizeMode.GetThemeSetInfo",
    "C_HousingCustomizeMode.GetWallpapersForRoomComponentType",
    "C_HousingCustomizeMode.IsDecorSelected",
    "C_HousingCustomizeMode.IsHouseExteriorDoorHovered",
    "C_HousingCustomizeMode.IsHoveringDecor",
    "C_HousingCustomizeMode.IsHoveringRoomComponent",
    "C_HousingCustomizeMode.IsRoomComponentSelected",
    "C_HousingCustomizeMode.RoomComponentSupportsVariant",
    "C_HousingCustomizeMode.RoomConnectionSupportsDoorType",
    "C_HousingCustomizeMode.SetRoomComponentCeilingType",
    "C_HousingCustomizeMode.SetRoomComponentDoorType",
    "C_HousingDecor.AnyDecorPlacedInRoom",
    "C_HousingDecor.CancelActiveEditing",
    "C_HousingDecor.CommitDecorMovement",
    "C_HousingDecor.EnterPreviewState",
    "C_HousingDecor.ExitPreviewState",
    "C_HousingDecor.GetAllMaxPlacementBudgets",
    "C_HousingDecor.GetAllPlacedDecor",
    "C_HousingDecor.GetAllSpentPlacementBudgets",
    "C_HousingDecor.GetDecorAssignedPetName",
    "C_HousingDecor.GetDecorCanAttachPet",
    "C_HousingDecor.GetDecorHyperlink",
    "C_HousingDecor.GetDecorIcon",
    "C_HousingDecor.GetDecorInstanceInfoForGUID",
    "C_HousingDecor.GetDecorName",
    "C_HousingDecor.GetHoveredDecorInfo",
    "C_HousingDecor.GetMaxPetPlacementBudget",
    "C_HousingDecor.GetNumDecorPlaced",
    "C_HousingDecor.GetNumPreviewDecor",
    "C_HousingDecor.GetSelectedDecorInfo",
    "C_HousingDecor.GetSpentPetPlacementBudget",
    "C_HousingDecor.HasMaxPlacementBudget",
    "C_HousingDecor.IsDecorSelected",
    "C_HousingDecor.IsGridVisible",
    "C_HousingDecor.IsHouseExteriorDoorHovered",
    "C_HousingDecor.IsHouseExteriorHovered",
    "C_HousingDecor.IsHoveringDecor",
    "C_HousingDecor.IsModeDisabledForPreviewState",
    "C_HousingDecor.IsPreviewState",
    "C_HousingDecor.RemovePlacedDecorEntry",
    "C_HousingDecor.RemoveSelectedDecor",
    "C_HousingDecor.SetGridVisible",
    "C_HousingDecor.SetPlacedDecorEntryHovered",
    "C_HousingDecor.SetPlacedDecorEntrySelected",
    "C_HousingExpertMode.CancelActiveEditing",
    "C_HousingExpertMode.CommitDecorMovement",
    "C_HousingExpertMode.CommitHouseExteriorPosition",
    "C_HousingExpertMode.GetHoveredDecorInfo",
    "C_HousingExpertMode.GetPrecisionSubmode",
    "C_HousingExpertMode.GetPrecisionSubmodeRestriction",
    "C_HousingExpertMode.GetSelectedDecorInfo",
    "C_HousingExpertMode.IsDecorSelected",
    "C_HousingExpertMode.IsGridVisible",
    "C_HousingExpertMode.IsHouseExteriorHovered",
    "C_HousingExpertMode.IsHouseExteriorSelected",
    "C_HousingExpertMode.IsHoveringDecor",
    "C_HousingExpertMode.RemoveSelectedDecor",
    "C_HousingExpertMode.ResetPrecisionChanges",
    "C_HousingExpertMode.SelectNextRotationAxis",
    "C_HousingExpertMode.SetGridVisible",
    "C_HousingExpertMode.SetPrecisionIncrementingActive",
    "C_HousingExpertMode.SetPrecisionSubmode",
    "C_HousingInspectMode.EnterInspectMode",
    "C_HousingInspectMode.ExitInspectMode",
    "C_HousingInspectMode.GetHoveredDecorGUID",
    "C_HousingInspectMode.IsHoveringDecor",
    "C_HousingInspectMode.IsInInspectMode",
    "C_HousingLayout.AnyRoomsOnFloor",
    "C_HousingLayout.CanSetViewedFloor",
    "C_HousingLayout.CancelActiveLayoutEditing",
    "C_HousingLayout.ConfirmStairChoice",
    "C_HousingLayout.DeselectFloorplan",
    "C_HousingLayout.DeselectRoomOrDoor",
    "C_HousingLayout.GetBaseRoomFloor",
    "C_HousingLayout.GetHighestOccupiedFloorIndex",
    "C_HousingLayout.GetLowestOccupiedFloorIndex",
    "C_HousingLayout.GetNumActiveRooms",
    "C_HousingLayout.GetRoomPlayerIsIn",
    "C_HousingLayout.GetSelectedBlueprintFloorplan",
    "C_HousingLayout.GetSelectedDoor",
    "C_HousingLayout.GetSelectedFloorplan",
    "C_HousingLayout.GetSelectedRoom",
    "C_HousingLayout.GetSelectedStairwellRoomCount",
    "C_HousingLayout.GetViewedFloor",
    "C_HousingLayout.HasAnySelections",
    "C_HousingLayout.HasRoomPlacementBudget",
    "C_HousingLayout.HasSelectedBlueprintFloorplan",
    "C_HousingLayout.HasSelectedDoor",
    "C_HousingLayout.HasSelectedFloorplan",
    "C_HousingLayout.HasSelectedRoom",
    "C_HousingLayout.HasStairs",
    "C_HousingLayout.HasValidConnection",
    "C_HousingLayout.IsBaseRoom",
    "C_HousingLayout.IsDraggingRoom",
    "C_HousingLayout.MoveDraggedRoom",
    "C_HousingLayout.MoveLayoutCamera",
    "C_HousingLayout.RemoveRoom",
    "C_HousingLayout.RoomHasStairs",
    "C_HousingLayout.RotateFocusedRoom",
    "C_HousingLayout.RotateRoom",
    "C_HousingLayout.SelectFloorplan",
    "C_HousingLayout.SetViewedFloor",
    "C_HousingLayout.StartDrag",
    "C_HousingLayout.StopDrag",
    "C_HousingLayout.StopDraggingRoom",
    "C_HousingLayout.ZoomLayoutCamera",
    "C_HousingNeighborhood.CanReturnAfterVisitingHouse",
    "C_HousingNeighborhood.CancelInviteToNeighborhood",
    "C_HousingNeighborhood.DemoteToResident",
    "C_HousingNeighborhood.GetCornerstoneHouseInfo",
    "C_HousingNeighborhood.GetCornerstoneNeighborhoodInfo",
    "C_HousingNeighborhood.GetCornerstonePurchaseMode",
    "C_HousingNeighborhood.GetCurrentNeighborhoodTextureSuffix",
    "C_HousingNeighborhood.GetDiscountedMovePrice",
    "C_HousingNeighborhood.GetMoveCooldownTime",
    "C_HousingNeighborhood.GetNeighborhoodMapData",
    "C_HousingNeighborhood.GetNeighborhoodName",
    "C_HousingNeighborhood.GetNeighborhoodPlotName",
    "C_HousingNeighborhood.GetPreviousHouseIdentifier",
    "C_HousingNeighborhood.HasPermissionToPurchase",
    "C_HousingNeighborhood.InvitePlayerToNeighborhood",
    "C_HousingNeighborhood.IsNeighborhoodManager",
    "C_HousingNeighborhood.IsNeighborhoodOwner",
    "C_HousingNeighborhood.IsPlayerInOtherPlayersPlot",
    "C_HousingNeighborhood.IsPlotAvailableForPurchase",
    "C_HousingNeighborhood.IsPlotOwnedByPlayer",
    "C_HousingNeighborhood.OnBulletinBoardClosed",
    "C_HousingNeighborhood.OnCornerstoneClosed",
    "C_HousingNeighborhood.PromoteToManager",
    "C_HousingNeighborhood.RequestNeighborhoodInfo",
    "C_HousingNeighborhood.RequestNeighborhoodRoster",
    "C_HousingNeighborhood.RequestPendingNeighborhoodInvites",
    "C_HousingNeighborhood.TransferNeighborhoodOwnership",
    "C_HousingNeighborhood.TryEvictPlayer",
    "C_HousingNeighborhood.TryMoveHouse",
    "C_HousingNeighborhood.TryPurchasePlot",
    "C_ImmersiveInteraction.HasImmersiveInteraction",
    "C_InstanceLeaver.IsPlayerLeaver",
    "C_InterfaceFileManifest.GetInterfaceArtFiles",
    "C_Intl.FindBreaks",
    "C_Intl.FindStringMatches",
    "C_Intl.FormatCurrency",
    "C_Intl.FormatDate",
    "C_Intl.FormatDateTime",
    "C_Intl.FormatTime",
    "C_Intl.GetCharacterProperties",
    "C_Intl.GetCurrencyFractionDigits",
    "C_Intl.GetCurrencyName",
    "C_Intl.GetDisplayName",
    "C_Intl.GetSortKey",
    "C_Intl.ParseCurrency",
    "C_Intl.ParseNumber",
    "C_Intl.SelectPlural",
    "C_Intl.TransformLocale",
    "C_Intl.Transliterate",
    "C_InvasionInfo.AreInvasionsAvailable",
    "C_InvasionInfo.GetInvasionInfo",
    "C_InvasionInfo.GetInvasionTimeLeft",
    "C_IslandsQueue.CloseIslandsQueueScreen",
    "C_IslandsQueue.GetIslandDifficultyInfo",
    "C_IslandsQueue.GetIslandsMaxGroupSize",
    "C_IslandsQueue.GetIslandsWeeklyQuestID",
    "C_IslandsQueue.QueueForIsland",
    "C_IslandsQueue.RequestPreloadRewardData",
    "C_LegendaryCrafting.CloseRuneforgeInteraction",
    "C_LegendaryCrafting.CraftRuneforgeLegendary",
    "C_LegendaryCrafting.GetRuneforgeItemPreviewInfo",
    "C_LegendaryCrafting.GetRuneforgeLegendaryComponentInfo",
    "C_LegendaryCrafting.GetRuneforgeLegendaryCost",
    "C_LegendaryCrafting.GetRuneforgeLegendaryCraftSpellID",
    "C_LegendaryCrafting.GetRuneforgeLegendaryCurrencies",
    "C_LegendaryCrafting.GetRuneforgeLegendaryUpgradeCost",
    "C_LegendaryCrafting.GetRuneforgeModifierInfo",
    "C_LegendaryCrafting.GetRuneforgeModifiers",
    "C_LegendaryCrafting.GetRuneforgePowerInfo",
    "C_LegendaryCrafting.GetRuneforgePowerSlots",
    "C_LegendaryCrafting.GetRuneforgePowers",
    "C_LegendaryCrafting.GetRuneforgePowersByClassSpecAndCovenant",
    "C_LegendaryCrafting.IsRuneforgeLegendary",
    "C_LegendaryCrafting.IsRuneforgeLegendaryMaxLevel",
    "C_LegendaryCrafting.IsUpgradeItemValidForRuneforgeLegendary",
    "C_LegendaryCrafting.IsValidRuneforgeBaseItem",
    "C_LegendaryCrafting.MakeRuneforgeCraftDescription",
    "C_LegendaryCrafting.UpgradeRuneforgeLegendary",
    "C_LimitedInput.LimitedInputAllowed",
    "C_LobbyMatchmakerInfo.AbandonQueue",
    "C_LobbyMatchmakerInfo.EnterQueue",
    "C_LobbyMatchmakerInfo.GetCurrQueuePlaylistEntry",
    "C_LobbyMatchmakerInfo.GetCurrQueueState",
    "C_LobbyMatchmakerInfo.GetQueueFromMainlineEnabled",
    "C_LobbyMatchmakerInfo.GetQueueStartTime",
    "C_LobbyMatchmakerInfo.RespondToQueuePop",
    "C_Log.LogMessage",
    "C_Log.LogMessageWithPriority",
    "C_Log.LogWarningMessage",
    "C_LoreText.RequestLoreTextForCampaignID",
    "C_MacOptions.AreOSShortcutsDisabled",
    "C_MacOptions.GetGameBundleName",
    "C_MacOptions.HasNewStyleInputMonitoring",
    "C_MacOptions.IsInputMonitoringEnabled",
    "C_MacOptions.IsMicrophoneEnabled",
    "C_MacOptions.IsUniversalAccessEnabled",
    "C_MacOptions.OpenInputMonitoring",
    "C_MacOptions.OpenMicrophoneRequestDialogue",
    "C_MacOptions.OpenUniversalAccess",
    "C_MacOptions.SetOSShortcutsDisabled",
    "C_Macro.GetSelectedMacroIcon",
    "C_Macro.RunMacroText",
    // Runs no macro here: the callback is never called.
    "C_Macro.SetMacroExecuteLineCallback",
    "C_ModifiedInstance.GetModifiedInstanceInfoFromMapID",
    "C_NamePlateManager.SetNamePlateSimplified",
    "C_NeighborhoodInitiative.AddTrackedInitiativeTask",
    "C_NeighborhoodInitiative.GetActiveNeighborhood",
    "C_NeighborhoodInitiative.GetAvailableHouseXP",
    "C_NeighborhoodInitiative.GetInitiativeActivityLogInfo",
    "C_NeighborhoodInitiative.GetInitiativeTaskChatLink",
    "C_NeighborhoodInitiative.GetInitiativeTaskInfo",
    "C_NeighborhoodInitiative.GetInitiativeTaskRewardScaling",
    "C_NeighborhoodInitiative.GetNeighborhoodInitiativeInfo",
    "C_NeighborhoodInitiative.GetRequiredLevel",
    "C_NeighborhoodInitiative.IsInitiativeEnabled",
    "C_NeighborhoodInitiative.IsPlayerInNeighborhoodGroup",
    "C_NeighborhoodInitiative.IsViewingActiveNeighborhood",
    "C_NeighborhoodInitiative.PlayerHasInitiativeAccess",
    "C_NeighborhoodInitiative.PlayerMeetsRequiredLevel",
    "C_NeighborhoodInitiative.RemoveTrackedInitiativeTask",
    "C_NeighborhoodInitiative.RequestInitiativeActivityLog",
    "C_NeighborhoodInitiative.RequestNeighborhoodInitiativeInfo",
    "C_NeighborhoodInitiative.SetActiveNeighborhood",
    "C_NeighborhoodInitiative.SetViewingNeighborhood",
    "C_PerksActivities.AddTrackedPerksActivity",
    "C_PerksActivities.ClearPerksActivitiesPendingCompletion",
    "C_PerksActivities.GetAllPerksActivityTags",
    "C_PerksActivities.GetPerksActivitiesInfo",
    "C_PerksActivities.GetPerksActivitiesPendingCompletion",
    "C_PerksActivities.GetPerksActivityChatLink",
    "C_PerksActivities.GetPerksActivityInfo",
    "C_PerksActivities.GetPerksUIThemePrefix",
    "C_PerksActivities.RemoveTrackedPerksActivity",
    "C_PerksProgram.ClearFrozenPerksVendorItem",
    "C_PerksProgram.CloseInteraction",
    "C_PerksProgram.GetAvailableCategoryIDs",
    "C_PerksProgram.GetAvailableVendorItemIDs",
    "C_PerksProgram.GetCategoryInfo",
    "C_PerksProgram.GetCurrencyAmount",
    "C_PerksProgram.GetDraggedPerksVendorItem",
    "C_PerksProgram.GetFrozenPerksVendorItemInfo",
    "C_PerksProgram.GetPendingChestRewards",
    "C_PerksProgram.GetPerksProgramItemDisplayInfo",
    "C_PerksProgram.GetTimeRemaining",
    "C_PerksProgram.GetVendorItemInfo",
    "C_PerksProgram.GetVendorItemInfoRefundTimeLeft",
    "C_PerksProgram.IsAttackAnimToggleEnabled",
    "C_PerksProgram.IsFrozenPerksVendorItem",
    "C_PerksProgram.IsMountSpecialAnimToggleEnabled",
    "C_PerksProgram.ItemSelectedTelemetry",
    "C_PerksProgram.PickupPerksVendorItem",
    "C_PerksProgram.RequestCartCheckout",
    "C_PerksProgram.RequestPendingChestRewards",
    "C_PerksProgram.RequestPurchase",
    "C_PerksProgram.RequestRefund",
    "C_PerksProgram.ResetHeldItemDragAndDrop",
    "C_PerksProgram.SetFrozenPerksVendorItem",
    "C_PhotoSharing.BeginAuthorizationFlow",
    "C_PhotoSharing.ClearAuthorization",
    "C_PhotoSharing.CompleteAuthorizationFlow",
    "C_PhotoSharing.GetCropRatio",
    "C_PhotoSharing.GetPhotoSharingAuthURL",
    "C_PhotoSharing.GetStatus",
    "C_PhotoSharing.IsAuthorized",
    "C_PhotoSharing.SetScreenshotPreviewTexture",
    "C_PhotoSharing.TakePhoto",
    "C_PhotoSharing.UploadPhotoToService",
    "C_Ping.GetCooldownInfo",
    "C_Ping.GetTextureKitForType",
    "C_Ping.SendMacroPing",
    "C_Ping.TogglePingListener",
    "C_PlayerInteractionManager.ClearInteraction",
    "C_PlayerInteractionManager.ConfirmationInteraction",
    "C_PlayerInteractionManager.InteractUnit",
    "C_PlayerInteractionManager.IsReplacingUnit",
    "C_PlayerInteractionManager.ReopenInteraction",
    "C_PlayerInteractionManager.ShardTransferConfirm",
    "C_PlayerMentorship.GetMentorLevelRequirement",
    "C_PlayerMentorship.GetMentorRequirements",
    "C_PlayerMentorship.GetMentorshipStatus",
    "C_PlayerMentorship.IsActivePlayerConsideredNewcomer",
    "C_PlayerMentorship.IsMentorRestricted",
    "C_RecruitAFriend.CanSummonFriend",
    "C_RecruitAFriend.ClaimActivityReward",
    "C_RecruitAFriend.ClaimNextReward",
    "C_RecruitAFriend.GenerateRecruitmentLink",
    "C_RecruitAFriend.GetRecruitActivityRequirementsText",
    "C_RecruitAFriend.GetRecruitInfo",
    "C_RecruitAFriend.GetSummonFriendCooldown",
    "C_RecruitAFriend.IsRecruitAFriendLinked",
    "C_RecruitAFriend.RemoveRAFRecruit",
    "C_RecruitAFriend.RequestUpdatedRecruitmentInfo",
    "C_RecruitAFriend.SummonFriend",
    "C_RemixArtifactUI.ClearRemixArtifactItem",
    "C_RemixArtifactUI.GetAppearanceInfoByID",
    "C_RemixArtifactUI.GetArtifactArtInfo",
    "C_RemixArtifactUI.GetArtifactItemInfo",
    "C_RemixArtifactUI.GetCurrArtifactItemID",
    "C_RemixArtifactUI.GetCurrItemSpecIndex",
    "C_RemixArtifactUI.GetCurrTraitTreeID",
    "C_RemixArtifactUI.ItemInSlotIsRemixArtifact",
    "C_Roleset.ApplyRolesetFilters",
    "C_Roleset.GetActiveAllowedRolesets",
    "C_Roleset.GetActiveBlockedRolesets",
    "C_SkillInfo.AbandonSkill",
    "C_SkillInfo.CollapseSkillHeader",
    "C_SkillInfo.ExpandSkillHeader",
    "C_SkillInfo.GetSkillLineInfoByID",
    "C_Soulbinds.ActivateSoulbind",
    "C_Soulbinds.CanActivateSoulbind",
    "C_Soulbinds.CanModifySoulbind",
    "C_Soulbinds.CanResetConduitsInSoulbind",
    "C_Soulbinds.CanSwitchActiveSoulbindTreeBranch",
    "C_Soulbinds.CloseUI",
    "C_Soulbinds.CommitPendingConduitsInSoulbind",
    "C_Soulbinds.FindNodeIDActuallyInstalled",
    "C_Soulbinds.FindNodeIDAppearingInstalled",
    "C_Soulbinds.FindNodeIDPendingInstall",
    "C_Soulbinds.FindNodeIDPendingUninstall",
    "C_Soulbinds.GetActiveSoulbindID",
    "C_Soulbinds.GetConduitCollection",
    "C_Soulbinds.GetConduitCollectionCount",
    "C_Soulbinds.GetConduitCollectionData",
    "C_Soulbinds.GetConduitCollectionDataAtCursor",
    "C_Soulbinds.GetConduitCollectionDataByVirtualID",
    "C_Soulbinds.GetConduitDisplayed",
    "C_Soulbinds.GetConduitHyperlink",
    "C_Soulbinds.GetConduitIDPendingInstall",
    "C_Soulbinds.GetConduitQuality",
    "C_Soulbinds.GetConduitRank",
    "C_Soulbinds.GetConduitSpellID",
    "C_Soulbinds.GetInstalledConduitID",
    "C_Soulbinds.GetNode",
    "C_Soulbinds.GetSoulbindData",
    "C_Soulbinds.GetSpecsAssignedToSoulbind",
    "C_Soulbinds.GetTree",
    "C_Soulbinds.HasAnyInstalledConduitInSoulbind",
    "C_Soulbinds.HasAnyPendingConduits",
    "C_Soulbinds.HasPendingConduitsInSoulbind",
    "C_Soulbinds.IsConduitInstalled",
    "C_Soulbinds.IsConduitInstalledInSoulbind",
    "C_Soulbinds.IsNodePendingModify",
    "C_Soulbinds.IsUnselectedConduitPendingInSoulbind",
    "C_Soulbinds.ModifyNode",
    "C_Soulbinds.SelectNode",
    "C_Soulbinds.UnmodifyNode",
    "C_SpellDiminish.GetAllSpellDiminishCategories",
    "C_SpellDiminish.GetSpellDiminishCategoryInfo",
    "C_SpellDiminish.IsSystemSupported",
    "C_SpellDiminish.ShouldTrackSpellDiminishCategory",
    "C_SplashScreen.AcknowledgeSplash",
    "C_SplashScreen.RequestLatestSplashScreen",
    "C_SplashScreen.SendSplashScreenActionLaunchedTelem",
    "C_SplashScreen.SendSplashScreenCloseTelem",
    "C_StableInfo.BuyStableSlot",
    "C_StableInfo.ClosePetStables",
    "C_StableInfo.GetActivePetList",
    "C_StableInfo.GetAvailablePetSpecInfos",
    "C_StableInfo.GetNumActivePets",
    "C_StableInfo.GetNumStablePets",
    "C_StableInfo.GetStablePetFoodTypes",
    "C_StableInfo.GetStabledPetList",
    "C_StableInfo.IsAtStableMaster",
    "C_StableInfo.IsBonusPetSlotAvailable",
    "C_StableInfo.IsPetFavorite",
    "C_StableInfo.PickupStablePet",
    "C_StableInfo.SetPetFavorite",
    "C_StableInfo.SetPetSlot",
    "C_SwingTimer.EnableRangeCheck",
    "C_TimerunningUI.GetActiveTimerunningSeasonID",
    "C_TooltipComparison.CompareItem",
    "C_TooltipComparison.GetItemComparisonDelta",
    "C_TooltipComparison.GetItemComparisonInfo",
    "C_TooltipInfo.GetAchievementByID",
    "C_TooltipInfo.GetArtifactItem",
    "C_TooltipInfo.GetArtifactPowerByID",
    "C_TooltipInfo.GetAzeriteEssence",
    "C_TooltipInfo.GetAzeriteEssenceSlot",
    "C_TooltipInfo.GetAzeritePower",
    "C_TooltipInfo.GetBackpackToken",
    "C_TooltipInfo.GetBagItemChild",
    "C_TooltipInfo.GetCompanionPet",
    "C_TooltipInfo.GetConduit",
    "C_TooltipInfo.GetCurrencyByID",
    "C_TooltipInfo.GetCurrencyToken",
    "C_TooltipInfo.GetEnhancedConduit",
    "C_TooltipInfo.GetEquipmentSet",
    "C_TooltipInfo.GetExistingSocketGem",
    "C_TooltipInfo.GetGuildBankItem",
    "C_TooltipInfo.GetInstanceLockEncountersComplete",
    "C_TooltipInfo.GetItemByGUID",
    "C_TooltipInfo.GetItemByItemModifiedAppearanceID",
    "C_TooltipInfo.GetItemInteractionItem",
    "C_TooltipInfo.GetLFGDungeonReward",
    "C_TooltipInfo.GetLFGDungeonShortageReward",
    "C_TooltipInfo.GetLootCurrency",
    "C_TooltipInfo.GetLootRollItem",
    "C_TooltipInfo.GetMerchantCostItem",
    "C_TooltipInfo.GetMinimapMouseover",
    "C_TooltipInfo.GetOutfit",
    "C_TooltipInfo.GetPossession",
    "C_TooltipInfo.GetPvpBrawl",
    "C_TooltipInfo.GetPvpTalent",
    "C_TooltipInfo.GetQuestCurrency",
    "C_TooltipInfo.GetQuestLogCurrency",
    "C_TooltipInfo.GetQuestLogItem",
    "C_TooltipInfo.GetQuestLogSpecialItem",
    "C_TooltipInfo.GetQuestPartyProgress",
    "C_TooltipInfo.GetRecipeRankInfo",
    "C_TooltipInfo.GetRecipeReagentItem",
    "C_TooltipInfo.GetRecipeResultItem",
    "C_TooltipInfo.GetRecipeResultItemForOrder",
    "C_TooltipInfo.GetRuneforgeResultItem",
    "C_TooltipInfo.GetSlottedKeystone",
    "C_TooltipInfo.GetSocketGem",
    "C_TooltipInfo.GetSocketedItem",
    "C_TooltipInfo.GetSocketedRelic",
    "C_TooltipInfo.GetTalent",
    "C_TooltipInfo.GetTotem",
    "C_TooltipInfo.GetTradePlayerItem",
    "C_TooltipInfo.GetTradeTargetItem",
    "C_TooltipInfo.GetTraitEntry",
    "C_TooltipInfo.GetUpgradeItem",
    "C_TooltipInfo.GetWeeklyReward",
    "C_TooltipInfo.GetWorldLootObject",
    "C_UIFileAsset.GetFileID",
    "C_UIFileAsset.IsKnownFile",
    "C_UIFileAsset.IsLooseFile",
    "C_UIWidgetManager.GetBulletTextListWidgetVisualizationInfo",
    "C_UIWidgetManager.GetButtonHeaderWidgetVisualizationInfo",
    "C_UIWidgetManager.GetCaptureBarWidgetVisualizationInfo",
    "C_UIWidgetManager.GetCaptureZoneVisualizationInfo",
    "C_UIWidgetManager.GetDiscreteProgressStepsVisualizationInfo",
    "C_UIWidgetManager.GetDoubleIconAndTextWidgetVisualizationInfo",
    "C_UIWidgetManager.GetDoubleStateIconRowVisualizationInfo",
    "C_UIWidgetManager.GetDoubleStatusBarWidgetVisualizationInfo",
    "C_UIWidgetManager.GetFillUpFramesWidgetVisualizationInfo",
    "C_UIWidgetManager.GetHorizontalCurrenciesWidgetVisualizationInfo",
    "C_UIWidgetManager.GetIconAndTextWidgetVisualizationInfo",
    "C_UIWidgetManager.GetIconTextAndBackgroundWidgetVisualizationInfo",
    "C_UIWidgetManager.GetIconTextAndCurrenciesWidgetVisualizationInfo",
    "C_UIWidgetManager.GetItemDisplayVisualizationInfo",
    "C_UIWidgetManager.GetMapPinAnimationWidgetVisualizationInfo",
    "C_UIWidgetManager.GetPreyHuntProgressWidgetVisualizationInfo",
    "C_UIWidgetManager.GetScenarioHeaderCurrenciesAndBackgroundWidgetVisualizationInfo",
    "C_UIWidgetManager.GetScenarioHeaderDelvesWidgetVisualizationInfo",
    "C_UIWidgetManager.GetScenarioHeaderTimerWidgetVisualizationInfo",
    "C_UIWidgetManager.GetSpacerVisualizationInfo",
    "C_UIWidgetManager.GetSpellDisplayVisualizationInfo",
    "C_UIWidgetManager.GetStackedResourceTrackerWidgetVisualizationInfo",
    "C_UIWidgetManager.GetStatusBarWidgetVisualizationInfo",
    "C_UIWidgetManager.GetTextColumnRowVisualizationInfo",
    "C_UIWidgetManager.GetTextWithStateWidgetVisualizationInfo",
    "C_UIWidgetManager.GetTextWithSubtextWidgetVisualizationInfo",
    "C_UIWidgetManager.GetTextureAndTextRowVisualizationInfo",
    "C_UIWidgetManager.GetTextureAndTextVisualizationInfo",
    "C_UIWidgetManager.GetTextureWithAnimationVisualizationInfo",
    "C_UIWidgetManager.GetTugOfWarWidgetVisualizationInfo",
    "C_UIWidgetManager.GetUnitPowerBarWidgetVisualizationInfo",
    "C_UIWidgetManager.GetZoneControlVisualizationInfo",
    "C_UIWidgetManager.RegisterUnitForWidgetUpdates",
    "C_UIWidgetManager.SetProcessingUnit",
    "C_UIWidgetManager.SetProcessingUnitGuid",
    "C_UIWidgetManager.UnregisterUnitForWidgetUpdates",
    "C_UserFeedback.SubmitBug",
    "C_UserFeedback.SubmitSuggestion",
    "C_WarbandScene.GetRandomEntryID",
    "C_WarbandScene.GetWarbandSceneEntry",
    "C_WarbandScene.HasWarbandScene",
    "C_WarbandScene.IsFavorite",
    "C_WarbandScene.SearchWarbandSceneEntries",
    "C_WarbandScene.SetFavorite",
    "C_WarbandScene.WarbandScenesAvailable",
    "C_WorldLootObject.DoesSlotMatchInventoryType",
    "C_WorldLootObject.GetWorldLootObjectDistanceSquared",
    "C_WorldLootObject.GetWorldLootObjectInfo",
    "C_WorldLootObject.GetWorldLootObjectInfoByGUID",
    "C_WorldLootObject.IsWorldLootObject",
    "C_WorldLootObject.IsWorldLootObjectByGUID",
    "C_WorldLootObject.IsWorldLootObjectInRange",
    "C_WorldLootObject.OnWorldLootObjectClick",
    "C_WowSurvey.OpenSurvey",
    "C_WowSurvey.TriggerSurveyServe",
    "C_WowTokenUI.ShouldDisplayInAuctionHouse",
    "C_WowTokenUI.StartTokenSell",
    "C_ZoneAbility.GetZoneAbilityIcon",
    // No totems are down (`GetTotemInfo` recorded every slot empty).
    "DestroyTotem",
];

#[cfg(test)]
mod tests {
    #[test]
    fn numbers_grouped() {
        assert_eq!(super::grouped(0), "0");
        assert_eq!(super::grouped(999), "999");
        assert_eq!(super::grouped(1000), "1,000");
        assert_eq!(super::grouped(-1234567), "-1,234,567");
    }

    #[test]
    fn lists_disjoint() {
        let names: Vec<&str> = super::behaviours().iter().map(|(n, _)| *n).collect();
        let mut sorted = names.clone();
        sorted.sort_unstable();
        sorted.dedup();
        assert_eq!(sorted.len(), names.len(), "a function listed twice");
    }
}
