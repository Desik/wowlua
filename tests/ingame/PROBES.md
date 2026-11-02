# In-game probes to run

The probes that would close the emulator's open gaps
([coverage report](../../docs/emulator-coverage.md)), for WoW Forever (the
camelot flavor). Install a current StdTest build first (`wowlua install
--client _forever_` in `tests/ingame`), then `/reload` in game so it
loads.

Everything here is read-only: no item is used, nothing is bought, moved or
sent, the character does not move, and no output stores a player's name.
After a run that StdTest records, `/reload` writes the results to the saved
variables; `cargo xtask ingest` then takes them into the fixtures.

## Still to run (2026-10-21, postponed)

The beta's last day left these open, most needed first. Each gives data the
engine cannot get otherwise.

1. **`/stdtest engine screen` with "Use UI Scale" on and the slider all the
   way left**, then `/reload`. Why: the scale rule with "Use UI Scale" on
   is recorded at 1.15 only (UIParent's scale is the slider's); its lower
   bound is not. That run was blocked last time: note the blocked
   function's name in the message ("StdTest has been blocked from an
   action..."): it is a taint finding (StdTest's code tainting Blizzard's
   settings code), for the engine's taint emulation too.
2. **A fresh Human Warrior's `/stdtest snapshot`**, twice a few seconds
   apart, then `/reload`. Why: the engine's default world is that
   character; the new snapshot also records its mounts, toys and wardrobe
   (decision T235).
3. **The replay scenarios** with a fresh character (step by step in the
   session notes): a quest turned in for an item reward
   (`/stdtest engine scenario turnin`), a second level-up (`levelup`),
   Heroic Strike with a target change (`heroicstrike`), smelting bronze
   (`smelting`, needs Mining); each `/stdtest engine scenario stop` then
   `/reload`. Why: see "Replay leftovers" below.
4. **The SuuUI Escape check** (by hand): see "Regressions" below.
5. **`/stdtest engine journals`** on a character with a mount or a toy.
   Why: the events after a collected mount's or toy's favourite changes are
   unrecorded (the recording character had none; decision T235).
6. **How the client breaks lines**: done 2026-10-24 (the beta's last
   night): `tests/client/forever-camelot-wrap.lua` in wowlua-engine, with a
   `/run` of line counts at widths 6 to 70; the engine follows it (decision
   T282). Left open: why the client keeps some texts on one line below two
   glyphs' width, and one line count without word wrap.
7. **An early landing** (the scenario recorder as for the taxi): at a
   flight master, pick a route with a stop on the way; type
   `/stdtest engine scenario earlylanding`, take the flight, and before the
   first stop `/run TaxiRequestEarlyLanding()`; once landed,
   `/stdtest engine scenario stop` and `/reload`. Why: the engine lands the
   flight a server round trip after the request (decision T289, a guess:
   it keeps no stops on the way); the recording gives when it lands and
   which events come.
8. **Atlases on a 1x screen**: the answers battery's atlas checks at a
   1920x1080 screen with Windows' DPI scale at 100%. Why: both recordings
   were on high-DPI screens and gave the 2x textures' files and
   coordinates (decision T238); a 1x screen may take the 1x textures (the
   sizes are the same either way).
9. **Talents the engine doesn't know yet**: partly done 2026-10-24
   (`tests/client/forever-camelot-talents2.lua` in wowlua-engine: the spec
   groups' configs, the active config, another class's view loadout). Left
   open: a rank planned and taken back (`PurchaseRank` refused, the
   character had no unspent point) and `/stdtest engine talentwatch` (a
   level-up to 5).
10. **Addon messages restricted?** In a raid out of an instance and out of
   combat: `/dump C_ChatInfo.AreOutgoingAddonChatMessagesRestricted(),
   C_ChatInfo.InChatMessagingLockdown()`, then
   `/dump C_ChatInfo.SendAddonMessage("SuuUI", "x", "RAID")` and whether a
   raid member's SuuUI hears it. Why: the full probe recorded `true` for
   the first (alone, out of combat), so the engine answers `true` always;
   SuuUI's raid planner no longer reads it (its specs were never shared).
11. **Moving a raid member**: as leader, `SetRaidSubgroup(i, g)`; then
   `GetRaidRosterInfo(i)` at once and after `GROUP_ROSTER_UPDATE` (with
   `GetTime()` at each). Why: SuuUI's probe saw the update 0.4-0.7 s
   later; the engine moves them and fires it at once.
12. **The game's settings while the UI runs** (decision T482: the
    events' order, whether a window size that changes UIParent's scale
    tells both, the CVars). In windowed mode, out of combat:
    1. `/run W={} F=CreateFrame("Frame") for _,e in ipairs({"UI_SCALE_CHANGED","DISPLAY_SIZE_CHANGED","CVAR_UPDATE"}) do F:RegisterEvent(e) end F:SetScript("OnEvent",function(_,e,a,b) W[#W+1]=e..(a and (":"..tostring(a)..":"..tostring(b)) or "") end)`
    2. Esc → Options → Graphics: tick "Use UI Scale", Apply.
    3. `/run print(table.concat(W," | ")) print(UIParent:GetScale(), GetCVar("useUiScale"), GetCVar("uiScale")) W={}`
    4. Move the UI Scale slider to its middle, Apply.
    5. `/run print(table.concat(W," | ")) print(UIParent:GetScale(), GetCVar("uiScale")) W={}`
    6. Pick another Window Size, Apply.
    7. `/run print(table.concat(W," | ")) local s=C_VideoOptions.GetCurrentGameWindowSize(0,false) print(s.x, s.y, GetCVar("gxWindowedResolution"), UIParent:GetScale(), GetScreenWidth(), GetScreenHeight()) W={}`
    8. `/run C_VideoOptions.SetGameWindowSize(1000, 700)`, then step 7 again (a size the monitor does not list).
    9. Untick "Use UI Scale", set the Window Size back, Apply.

- **`GetBindingText(key, 1)` with modifiers** (the action bars' hotkeys):
  `SHIFT-1`, `CTRL-ALT-2`, `META-E`, `SHIFT-MOUSEWHEELUP`, `SHIFT-NUMPAD1`.
  Why: the engine shortens the modifiers with the client's
  `*_KEY_TEXT_ABBR` strings and joins them with `-` (`s-1`), and keeps the
  key's own name whole; neither the joining nor the key names are
  recorded (M12's default-UI screenshot showed the hotkeys cut to `SHI…`).

## The answers battery's new calls

About a minute, standing anywhere, out of a group and out of combat.

1. Log in (or `/reload` if the game was running) so the new StdTest loads.
2. Type `/stdtest engine answers` and wait for the chat line `engine answers:
   N calls recorded; type /reload so the file is written`. It can pause for a
   few seconds (map lookups, atlases, glyph measurements).
3. Type `/reload`, then run `cargo xtask ingest`.

If the game freezes or crashes instead, the newest file in the client's
`Errors/` folder shows where it stopped (its "Lua Stack").

What it answers:

- Coins: `GetCoinIcon` 0/99/9999, `C_CurrencyInfo.GetCoinIcon` 100/10000.
- Units: `UnitStat("player", 6)`, `GetSpellBonusDamage` 2 to 8,
  `GetMirrorTimerInfo` 2/3/4, `C_PaperDollInfo.GetTemporaryEnchantmentInfo`
  16/17/18/20, `GetNumCompanions("MOUNT")`/`("CRITTER")`, `UnitGUID(nil)`;
  what `UnitName`/`UnitPVPName` give for 17 unit tokens ("none", "target",
  "softenemy"...), as nil, empty, length and whether it equals the player's
  own (no names are stored).
- Game rules: `C_GameRules.IsGameRuleActive` 10/213/214, and every
  `Enum.GameRule` value in one check.
- Commentator and timeline: `C_Commentator.GetTeamColor` 2/3;
  `C_EncounterTimeline.GetCurrentTime()` between two `GetTime()`s.
- Artifact: `C_ArtifactUI.GetCostForPointAtRank` ranks 2 to 10 (both
  argument orders), `GetTotalPowerCost` for a few spans.
- Maps: `C_Map.GetWorldPosFromMapPos` at (0,0) and (1,1) for every recorded
  map; `C_Map.GetMapPosFromWorldPos` for 6 world points (Orgrimmar,
  Crossroads, Mulgore, Stormwind, Ironforge, Undercity) without and with 8
  override maps; `GetMapChildrenInfo` of 1414/1415/947 as counts and IDs;
  which map IDs from 1 to 2500 exist.
- Layout: `C_Texture.GetAtlasInfo` (size, file, texture coordinates, tiling)
  for every atlas the engine knows; string width, height and unbounded width
  of 8 texts and every printable ASCII glyph in GameFontNormal,
  GameFontHighlightSmall, SystemFont_Shadow_Large_Outline and
  NumberFontNormal.
- Script objects: `CreateFrame(..., "BackdropTemplate")`;
  `CreateAbbreviateConfig` with divisors 1/1 (raw and global abbreviation)
  and 60 (as significand and as fraction divisor); writing a field to a
  `C_Timer.NewTimer` and a `NewTicker` handle (both cancelled at once).

## The collections (decision T235)

About a minute, on any character, out of combat; the journals may stay
closed. A character with a mount or a toy also records the events after its
favourite changes; without, the probe records whether the client takes an
uncollected one's (a new character has its starting gear's appearances).

1. Type `/stdtest engine journals` and wait for the chat line `engine
   journals: ... lines; type /reload so the file is written` (about 7
   seconds). It records the order of the mount journal's and the toy box's
   lists (as their filters are, and with every filter on, put back after),
   and every event the client sends when one mount's, one toy's and one
   appearance's favourite changes. Each favourite is changed and put back,
   a second apart; the filters are put back as they were.
2. Type `/stdtest snapshot`: the world profile now records the mount
   journal (every mount with its info, and which uncollected ones the
   character can ride), the toy box (its collected toys; its checks are
   opened for it, put back after) and the wardrobe's collected sources. A toy
   whose item the client has not loaded is asked for: run the snapshot a
   second time a few seconds later so its name comes along.
3. Type `/reload`, and keep a copy of `WTF/Account/<account>/SavedVariables/StdTest.lua`.
4. Then `/stdtest engine journals2` (about 8 seconds, any character): each
   filter's CVar on its own, the toy box's list a second after its filters
   change, every toy, and an appearance's favourite in three more categories,
   on and off. Everything is put back. `/reload` after.

## The method probe

About 10 to 15 minutes, after the answers battery, out of combat, standing
anywhere. `/stdtest engine methodprobe` calls every widget method the client
has (about 13,000 calls on 53 widget kinds), each on a fresh widget under a
hidden frame of its own: nothing shows, no Blizzard frame is touched, no
setting changes. It reloads the UI by itself every 500 calls.

1. Type `/stdtest engine methodprobe`. The chat says `methodprobe: N calls
   (...)`; after each reload it prints `methodprobe: n/N calls answered`. Do
   not type while it runs; in combat it waits. `/stdtest engine methodprobe
   status` shows where it is, `/stdtest engine methodprobe stop` pauses it
   (the same command goes on).
2. It ends with `methodprobe: done, ...; reloading so the file is written`
   and, after that reload, `methodprobe: finished`.
3. Run `cargo xtask ingest` (it writes `forever-camelot-methodprobe.lua`).

If the game freezes or crashes, start it again and log in with the same
character. The probe notices the crash at login, sets that stretch of 500
calls aside, goes on, and comes back to it in smaller steps until the one
call that crashes is found and skipped. The riskiest calls (boundary numbers
like -1 and 2^31-1) come last. More than three crashes: keep the newest file
in `Errors/` and the output of `/stdtest engine methodprobe status`.

## Before and after the next full probe

- `/stdtest engine snapshot` right before `/stdtest engine fullprobe` and
  again right after it finishes, so the replay starts from the exact state
  (bind location, mail, macros, channels, the quest log).
- `/run for i=1,GetNumBindings() do local c=GetBinding(i) print(i,c) end`
  (binding command names only): the binding list.
- `/dump C_Spell.GetSpellPowerCost(20271)`, `/dump C_Spell.GetSpellInfo(20271)`
  and `/dump C_Spell.IsSpellHarmful(20271)` (Judgement): spell cost, range,
  cast time and harmfulness, the data `IsHarmfulAction` needs.
- `/run print(date("%H:%M"), date("!%H:%M"))`: the machine's time zone, for
  `C_DateAndTime.GetCalendarTimeFromEpoch`.

## Situations (one /dump each, whenever it happens)

A screenshot of the output or the values typed back is enough.

1. Under water (until the breath bar shows): `/dump GetMirrorTimerInfo(1)`,
   `/dump GetMirrorTimerInfo(2)` and `/dump GetMirrorTimerProgress("BREATH")`:
   a running breath timer.
2. In a party: `/dump UnitGroupRolesAssigned(), UnitGroupRolesAssigned("player"), UnitGroupRolesAssigned("party1")`
   and `/dump UnitGetAvailableRoles("party1")`: whether the no-argument form
   follows the group role.
3. Inside a dungeon or battleground entrance (only when going there anyway):
   `/dump GetInstanceInfo()` and `/dump GetRealZoneText()`.
4. With a soft target showing:
   `/run local n=UnitName("softinteract") print(n and #n, n==UnitName("target"))`
   (only the length is printed).
5. With a temporary weapon enchant already on (do not use one just for
   this): `/dump C_PaperDollInfo.GetTemporaryEnchantmentInfo(16)` and
   `/dump GetWeaponEnchantInfo()`.
6. In combat with a target that has a buff or debuff:
   `/run local a=C_UnitAuras.GetAuraDataByIndex("target",1) print(a and issecrettable(a), a and issecretvalue(a.spellId), a and issecretvalue(a.duration), a and issecretvalue(a.name))`
   and the same for `C_UnitAuras.GetPlayerAuraBySpellID(<one of your buffs' spell IDs>)`:
   whether the aura table or only its fields are secret.

7. In a party when a member logs out or back in (decision T249): a frame
   registered for `UNIT_CONNECTION` and `GROUP_ROSTER_UPDATE` printing each
   with its arguments: whether `UNIT_CONNECTION("party1", false)` comes, for
   `target` too while they're targeted, and before or after the roster
   update.

8. Anywhere, once: an addon's timer on the encounter timeline. That it
   goes on it is settled (2026-10-22: SuuUI's pull countdown shows on its
   timeline in game; the engine keeps script events, decision T255); what
   it answers is the engine's guess from the documentation:
   `/dump C_EncounterTimeline.AddScriptEvent({ spellID = 100, iconFileID = 132337, duration = 30, overrideName = "Probe" })`,
   then `/dump C_EncounterTimeline.GetEventList()` and, for the ID,
   `/dump C_EncounterTimeline.GetEventInfo(<id>)`,
   `/dump C_EncounterTimeline.GetEventState(<id>)`,
   `/dump C_EncounterTimeline.GetEventTimeRemaining(<id>)`: the ID (the
   engine counts from 41), the info's fields (its `spellName` the
   `overrideName`), the state while it runs (0) and after it ran out.

9. In combat, from an addon (a `/run` counts as one): `/run C_Spell.PickupSpell(6603)`
   (Auto Attack) and `/run PickupAction(1)`: whether either is blocked as
   `C_SpellBook.PickupSpellBookItem` is (decision T253), then
   `/run ClearCursor()`.

10. On a rogue (or a druid in cat form), in a fight with a target: a frame
    registered for `UNIT_POWER_UPDATE`, `UNIT_POWER_FREQUENT` and
    `PLAYER_TARGET_CHANGED` printing each with its arguments, then build
    two combo points and
    `/dump GetComboPoints("player", "target"), UnitPower("player", Enum.PowerType.ComboPoints), UnitPowerMax("player", Enum.PowerType.ComboPoints)`;
    then target another creature and dump again. Why: the engine has no
    combo points (SuuUI's finding V31): their events, and whether they stay
    on the target or with the player.
11. In a duel with a friend (when one happens anyway): a frame registered
    for `UNIT_FACTION` and `PLAYER_TARGET_CHANGED` printing each with its
    arguments, the friend targeted;
    `/dump UnitCanAttack("player", "target"), UnitReaction("target", "player"), UnitIsEnemy("player", "target")`
    before the duel starts, during it, and after. Why: a unit can't turn
    hostile in the engine (SuuUI's finding V30): the events and answers as
    it does.

## Replay leftovers

- Before turning in a quest with an item reward, two lines:
  `/run Q=function() print(GetTime(), C_Container.GetContainerNumFreeSlots(0)) end`
  then `/run local f=CreateFrame("Frame") f:RegisterEvent("QUEST_TURNED_IN") f:SetScript("OnEvent",function() Q() C_Timer.After(0.2,Q) end)`:
  when the reward takes its bag slot.
- Around a second level-up: `/stdtest engine scenario`, to confirm the
  specialization event is batched.
- Queue Heroic Strike on a mob, then change target before the swing; note
  any error text.
- While smelting bronze with copper, tin and bronze stacks all in the bags
  (with and without a skill-up), and once crafting with the last reagent
  stack: `/stdtest engine scenario`. Answers the order of the item counts
  and when a used-up stack is removed (the profession replay's leftovers).

## The guild

One `/dump` each, any time (names in the output can be cropped from a
screenshot).

1. `/dump C_GuildInfo.GuildControlGetRankFlags(2), C_GuildInfo.GuildControlGetRankFlags(3)`
   and the same for 4 and 5.
2. `/dump GuildControlGetAllowedShifts(3), GuildControlGetAllowedShifts(4), GuildControlGetAllowedShifts(5)`
3. `/run for i=1,GetNumGuildMembers() do local _,r,ri,l,c=GetGuildRosterInfo(i) print(i,ri,l,c) end`
   (rank index, level and class only), once with "show offline" on and once
   off.
4. `/run for i=3,10 do print(GetGuildTradeSkillInfo(i)) end`
5. `/dump C_Club.GetClubInfo(C_Club.GetGuildClubId())` and
   `/dump C_Club.GetClubPrivileges(C_Club.GetGuildClubId())`.

## Widget state (decision T289)

The engine's guesses for methods that had no behaviour. Anywhere, out of
combat; each step is one line to paste, then a screenshot of the chat.

**Frame levels** (`Raise`, `Lower`):

1. `/run A=CreateFrame("Frame",nil,UIParent) B=CreateFrame("Frame",nil,UIParent) A:SetFrameStrata("FULLSCREEN_DIALOG") B:SetFrameStrata("FULLSCREEN_DIALOG") B:SetFrameLevel(7) A:Raise() print(A:GetFrameLevel(),B:GetFrameLevel())`
2. `/run C=CreateFrame("Frame",nil,UIParent) C:SetFrameStrata("FULLSCREEN_DIALOG") C:SetFrameLevel(20) C:Hide() A:Raise() print(A:GetFrameLevel())`
3. `/run A:Lower() print(A:GetFrameLevel(),B:GetFrameLevel())`

Screenshot after step 3. The engine says 8 7, then 8 (a hidden frame
doesn't count), then 0 7.

**An edit box's selection** (`HighlightText`, `Insert`):

4. `/run E=CreateFrame("EditBox",nil,UIParent) E:SetSize(200,20) E:SetPoint("CENTER") E:SetFontObject(ChatFontNormal) E:SetAutoFocus(false) E:SetText("hello world") E:HighlightText(0,5) E:Insert("HELLO") print(E:GetText(),E:GetCursorPosition())`
5. `/run E:SetScript("OnTextChanged",function(s,u) print("changed",u) end) E:Insert("!") print("after Insert")`
6. `/run E:SetCursorPosition(1) E:SetText("abc") print(E:GetCursorPosition())`
7. `/run E:SetText("abcdef") E:HighlightText(4,1) E:Insert("Z") print(E:GetText())`

Screenshot after step 7. The engine says `HELLO world 5`; then "after
Insert" before "changed false" (Insert's `OnTextChanged` comes in the next
frame, as SetText's); then 3; then `aZef` (a selection given backwards is
the same one).

8. `/run E:SetScript("OnEditFocusGained",function(s) s:HighlightText() end) E:SetText("Sheep")`
9. By hand: click into the box (in the middle of the screen), type `x`.
10. `/run print(E:GetText()) E:ClearFocus() E:Hide()`

Screenshot after step 10. The engine says `x`: the text selected on focus
stays selected after the click, and typing replaces it. If the click put
the cursor somewhere instead, it says where the text went.

**A font copied** (`CopyFontObject`):

11. `/run F1=CreateFont("ProbeF1") F1:SetFont("Fonts\\FRIZQT__.TTF",14,"OUTLINE") F1:SetTextColor(1,0,0) F2=CreateFont("ProbeF2") F2:CopyFontObject(F1) F1:SetTextColor(0,1,0) print(F2:GetFont()) print(F2:GetTextColor())`

Screenshot. The engine says the file, 14 and OUTLINE, then 1 0 0 1: a
copy, not a link to the other font.

**Anchors moved** (`AdjustPointsOffset`, `ClearPointsOffset`):

12. `/run P=CreateFrame("Frame",nil,UIParent) P:SetPoint("TOPLEFT",10,20) P:AdjustPointsOffset(5,-5) print(select(4,P:GetPoint(1))) P:ClearPointsOffset() print(select(4,P:GetPoint(1)))`

Screenshot. The engine says 15 15, then 0 0. If the client keeps the
adjustment apart from the point, `GetPoint` may say 10 20 both times.

**The clears**:

13. `/run Q=CreateFrame("Button",nil,UIParent) Q:SetNormalTexture("Interface\\Buttons\\WHITE8X8") T=Q:GetNormalTexture() Q:ClearNormalTexture() print(Q:GetNormalTexture(),T:IsShown(),T:GetParent()==Q)`
14. `/run M=CreateFrame("PlayerModel",nil,UIParent) M:SetModel(130737) M:ClearModel() print(M:GetModelFileID())`

Screenshot after step 14. The engine says nil false true, then 0.

**At a vendor** (the merchant's window open):

15. `/run local f=CreateFrame("Frame") f:RegisterEvent("MERCHANT_UPDATE") f:SetScript("OnEvent",function(_,e) print(e,GetMerchantFilter(),GetMerchantNumItems()) end) print(LE_LOOT_FILTER_ALL,LE_LOOT_FILTER_CLASS,GetMerchantFilter(),GetMerchantNumItems())`
16. `/run SetMerchantFilter(LE_LOOT_FILTER_ALL)`
17. `/run ResetSetMerchantFilter()`

Screenshot after step 17. The engine keeps the filter and tells
`MERCHANT_UPDATE` once per change, every item still listed; the client may
list fewer items with the class filter.

**In a guild**:

18. `/run print(GetGuildRosterSelection()) SetGuildRosterSelection(2) print(GetGuildRosterSelection()) SetGuildRosterSelection(999) print(GetGuildRosterSelection())`

Screenshot. The engine says 0, 2, 0 (a row past the roster selects
none).

## Targeting and history (decision T291)

**Tab-targeting**, near a few creatures (out of combat):

1. `/run local f=CreateFrame("Frame") f:RegisterEvent("PLAYER_TARGET_CHANGED") f:SetScript("OnEvent",function() print(UnitName("target"),UnitReaction("target","player")) end)`
2. Press Tab five times, then Shift-Tab twice.

Screenshot. The engine cycles the hostile and neutral creatures within
40 yards nearest first, Shift-Tab back; the names and reactions show
whether neutral ones count and how far it looks.

**An edit box's history and arrows**:

3. `/run H=CreateFrame("EditBox",nil,UIParent) H:SetSize(200,20) H:SetPoint("CENTER") H:SetFontObject(ChatFontNormal) H:SetAutoFocus(false) H:AddHistoryLine("one") H:AddHistoryLine("two") print(H:GetHistoryLines())`
4. `/run H:SetScript("OnArrowPressed",function(s,k) print("arrow",k,s:GetText()) end) H:SetScript("OnTextChanged",function(s,u) print("changed",u,s:GetText()) end) H:SetFocus()`
5. By hand: press Up twice, then Down twice.
6. `/run H:SetHistoryLines(5) H:AddHistoryLine("three") H:SetFocus()`
7. By hand: press Up, then Escape.

Screenshot after step 7. The engine keeps no line without
`SetHistoryLines` (step 5 changes nothing), runs `OnArrowPressed` before
the text changes, and step 7 brings back "three".

## Leftovers (decision T295)

**Money on the cursor** (bags open, some money):

1. `/run local f=CreateFrame("Frame") f:RegisterEvent("CURSOR_CHANGED") f:SetScript("OnEvent",function(_,e,...) print(e,...) end) PickupPlayerMoney(1) print(GetCursorMoney(),GetCursorInfo())`
2. `/run ClearCursor() print(GetCursorMoney())`

Screenshot after step 2. The engine says `CURSOR_CHANGED false 2 0 0`,
then 1 (and no cursor info), then `CURSOR_CHANGED true 0 2 0` and 0.

**Assignments** (as a raid leader, a member targeted):

3. `/run local f=CreateFrame("Frame") f:RegisterEvent("GROUP_ROSTER_UPDATE") f:SetScript("OnEvent",function(_,e) print(e) end) SetPartyAssignment("MAINTANK","target") C_Timer.After(1,function() print(GetPartyAssignment("MAINTANK","target")) end)`

Screenshot. The engine tells `GROUP_ROSTER_UPDATE` at once and true.

**A system line and Sign**:

4. `/run SendSystemMessage("probe line") print(Sign(-3),Sign(0),Sign(2))`

Screenshot. The engine shows "probe line" as a system message, then
-1 0 1.

## Regressions and other flavors

1. With SuuUI's current bags, open the spellbook, enter combat (a training
   dummy is enough) and press Escape: the spells window should close, with no
   "Interface action failed" in chat. Confirms the fix the regression test
   `suuui_bag_calls_taint_escape_in_combat` stands for.
2. On a classic flavor's client (vanilla, tbc, wrath or mists), run the
   function inventory probe as on camelot, so the engine knows which of the
   old global names (`GetItemCount`, `InviteUnit`, `UseContainerItem`, ...)
   that client really defines.

## Model frames' cameras

The wardrobe's head shots (`Model_ApplyUICamera`) sit off center in
wowlua's screenshots: how the client's model frame camera sees a model is
not known yet. Answered (2026-10-23, decision T334) with a DressUpModel of
the player under UIParent and UI camera 542 at 200 by 250 and 400 by 250,
with and without `SetPosition(0, 0, 0)`; the wardrobe's head cells match
a Skyborne female's screenshot once the camera follows the race (T335).
Still open: the body's transmog skin (`useTransmogSkin`, drawn dark in
game, the character's clothes in wowlua).

1. Open the appearances collection on the head slot and take a screenshot of
   the first page (the cells' framing to compare with).
2. With the collections open, two lines:
   `/run M=WardrobeCollectionFrame.ItemsCollectionFrame.Models[1] print(M:GetCameraPosition()) print(M:GetCameraTarget()) print(M:GetPosition())`
   then `/run print(M:GetFacing(), M:GetPitch(), M:GetRoll()) print(M:IsUsingModelCenterToTransform()) print(M:TransformCameraSpaceToModelSpace(CreateVector3D(4,0,0)):GetXYZ())`
3. `/run local m=CreateFrame("PlayerModel") m:SetSize(100,100) m:SetUnit("player") print(m:GetCameraDistance(), m:GetCameraPosition()) print(m:GetCameraTarget())`
   Answers the model frame's own camera and field of view.

## Textures the screenshots draw

1. `/run local t=MainStatusTrackingBarContainer.bars and MainStatusTrackingBarContainer.bars[1] or nil print(t and t.StatusBar:GetStatusBarTexture():GetAtlas(), t and t.StatusBar:GetStatusBarTexture():GetTexture())`
   with the experience bar shown: whether `SetStatusBarTexture(atlasName)`
   makes the texture's atlas (wowlua: it does; `GetTexture` answers the
   atlas's file ID, decision T356).
2. An action button with a spell: `/run local b=ActionButton1 print(b.icon:GetDrawLayer()) print(b.SlotArt:GetDrawLayer()) print(b.icon:GetNumMaskTextures())`
   and a screenshot of it hovered: wowlua draws a masked texture over the
   unmasked ones of its layer and sublevel (the icon over the slot art).
   Answered (2026-10-23): both `BACKGROUND` 0, one mask, as in wowlua;
   still open: the hovered screenshot (which of the two is drawn on top).
3. `/run local t=UIParent:CreateTexture() print(t:IsSnappingToPixelGrid(), t:GetTexelSnappingBias())`:
   a fresh texture's pixel snapping (wowlua: true, 0). Answered
   (2026-10-23): true, 0.30000001192093; wowlua now gives the same.
4. What `GetTexture` answers for an addon's own file, an atlas and a color
   (decision T356; wowlua: the file's negative loose ID, the atlas's file
   ID, nil), and what `GetTextureFilePath` answers (wowlua: the path set).
   With an addon that ships a texture (here SuuUI's
   `Interface\AddOns\SuuUI\media\logo`; use any file it has):
   1. `/run T=UIParent:CreateTexture() print(T:SetTexture("Interface\\AddOns\\SuuUI\\media\\logo"), T:GetTexture(), T:GetTextureFileID(), T:GetTextureFilePath())`
   2. `/run print(GetFileIDFromPath("Interface\\AddOns\\SuuUI\\media\\logo"))`
   3. `/run T:SetAtlas("128-RedButton-Exit") print(T:GetTexture(), T:GetTextureFileID(), T:GetTextureFilePath())`
   4. `/run T:SetColorTexture(1, 0, 0) print(T:GetTexture(), T:GetTextureFileID(), T:GetTextureFilePath())`
   5. `/run print(GetFileIDFromPath("Interface\\AddOns\\NoSuchAddon\\x"), GetFileIDFromPath("Interface\\Buttons\\WHITE8X8"))`
      (wowlua: nil, 130871; decision T355).

## Minimap

wowlua draws the minimap's terrain at the long-known zoom spans and the
player's arrow 32 by 32 UI units; neither is recorded on camelot.

1. Outdoors in Northshire (or any place with a landmark near), take a
   screenshot of the minimap at each zoom: `/run Minimap:SetZoom(0)` to
   `/run Minimap:SetZoom(5)`, with `/run print(UnitPosition("player"))`
   and `/run print(C_Map.GetPlayerMapPosition(C_Map.GetBestMapForUnit("player"), "player"):GetXY())`
   for each. The landmark's distance in the shots gives each zoom's span.
2. The same spot with `/console rotateMinimap 1`, facing east: the terrain
   turned and the arrow up.
3. In a party, a screenshot of the minimap with a member in sight, and
   one of a ping (click the minimap) at once, with
   `/run C_Timer.NewTicker(0.5,function() print("ping",GetTime(),Minimap:GetPingPosition()) end,16)`
   first: the blip's size, the ping's look and how long it keeps its
   place (`RegisterEvent("MINIMAP_PING")` is refused as unknown,
   2026-10-23: a callback event alone).
4. Which callback events `RegisterEvent` refuses, and whether an addon's
   callback hears one:
   `/run P=function(e) local f=CreateFrame("Frame") print(e,(pcall(f.RegisterEvent,f,e)),C_EventUtils.IsEventValid(e)) end`
   then `P` for each of the documentation's twelve `CallbackEvent`s, and
   `/run CreateFrame("Frame"):RegisterEventCallback("MINIMAP_PING",function(_,...) print("cb",...) end)`
   before a ping (the payload's order).

## Unanchored textures and slider thumbs

1. `/run local f=CreateFrame("Frame",nil,UIParent) f:SetSize(200,100) f:SetPoint("CENTER") local t=f:CreateTexture() t:SetSize(40,20) print(t:GetNumPoints(), t:GetSize())`
   then the same with a texture from XML that has `<Size>` and no
   `<Anchors>` (any addon's): wowlua fills the parent with an XML one
   (2 points, the parent's size), leaves a Lua one unplaced.
2. Open Settings, Mouse, hover the Mouse Look Speed slider with
   `/fstack` on, and note the thumb's left relative to the slider at the
   value shown, at the minimum and at the maximum (wowlua: it travels the
   slider's width less the thumb's, by the value). Answered for a
   horizontal slider made in Lua (2026-10-23): it does, by one point,
   `LEFT` to the slider's `LEFT`; still open: a vertical slider's point.


## Owed from the M12 screenshots (next beta, about 2026-11-07)

The camelot beta closed on 2026-10-24; these close what the screenshot work
left as proposals. Each item stands alone: after a `/reload` (or an error)
start the item again from its first step. In the chat box `|` is escaped,
so the lines write it as `\124`. Take the screenshots with the default UI
alone unless an item names an addon.

1. **Load-on-demand addons' reason** (decision T354; wowlua:
   `DEMAND_LOADED` before loading, empty after). With AtlasLoot installed:
   1. `/run for i=1,C_AddOns.GetNumAddOns() do local n,_,_,l,r=C_AddOns.GetAddOnInfo(i) if C_AddOns.IsAddOnLoadOnDemand(i) then print(n,tostring(l),"["..tostring(r).."]",C_AddOns.IsAddOnLoaded(i)) end end`
   2. `/run print(C_AddOns.LoadAddOn("AtlasLootClassic_Crafting"), "["..tostring(select(5,C_AddOns.GetAddOnInfo("AtlasLootClassic_Crafting"))).."]")`
2. **AceGUI's `SetDesaturation`** (AtlasLoot's options window: wowlua has
   no such global, so AceGUI's checkbox errors):
   1. `/run print(SetDesaturation, LibStub and LibStub("AceGUI-3.0",true) and select(2,LibStub("AceGUI-3.0")))`
3. **Which point sets an edge** (decision T337; wowlua: `TOPLEFT` places
   the left edge over `LEFT`, whatever order they were set in, so `50 50`;
   `GetPoint(1)` stays the first set, `RIGHT`):
   1. `/run F=CreateFrame("Frame",nil,UIParent) F:SetHeight(10) F:SetPoint("RIGHT",UIParent,"CENTER") F:SetPoint("LEFT",UIParent,"CENTER",-100,0) F:SetPoint("TOPLEFT",UIParent,"CENTER",-50,0)`
   2. `/run print(UIParent:GetWidth()/2-F:GetLeft(), F:GetWidth(), (F:GetPoint(1)))`
4. **A button's font objects by name** (decision T338; wowlua: a name
   works, an unknown name unverified):
   1. `/run local b=CreateFrame("Button",nil,UIParent) b:SetNormalFontObject("GameFontRed") print(b:GetNormalFontObject():GetName()) print(pcall(b.SetHighlightFontObject,b,"NoSuchFont"))`
5. **Font strings over textures in one layer** (decision T340; wowlua: the
   text is drawn over the red, though made first):
   1. `/run F=CreateFrame("Frame",nil,UIParent) F:SetSize(240,60) F:SetPoint("CENTER") local s=F:CreateFontString(nil,"ARTWORK","GameFontNormalHuge") s:SetPoint("CENTER") s:SetText("TEXT ON TOP?")`
   2. `/run local t=F:CreateTexture(nil,"ARTWORK") t:SetAllPoints() t:SetColorTexture(1,0,0)`
   3. A screenshot.
6. **A frame on its parent's level** (decision T341; wowlua: the child's
   green `BACKGROUND` is drawn under the parent's blue `ARTWORK`, the two
   sorted together by layer):
   1. `/run local p=CreateFrame("Frame",nil,UIParent) p:SetSize(200,100) p:SetPoint("CENTER") local a=p:CreateTexture(nil,"ARTWORK") a:SetPoint("LEFT") a:SetSize(120,100) a:SetColorTexture(0,0,1) P=p`
   2. `/run local c=CreateFrame("Frame",nil,P) c:SetUsingParentLevel(true) c:SetAllPoints() local g=c:CreateTexture(nil,"BACKGROUND") g:SetPoint("RIGHT") g:SetSize(120,100) g:SetColorTexture(0,1,0)`
   3. A screenshot: is the middle strip blue (wowlua) or green?
7. **`SetFrameLevel` and descendants** (decision T343; wowlua: the
   child raised by 5 moves with the parent, the grandchild stays one over
   its parent):
   1. `/run P=CreateFrame("Frame",nil,UIParent) C=CreateFrame("Frame",nil,P) C:SetFrameLevel(P:GetFrameLevel()+5) G=CreateFrame("Frame",nil,C) print(P:GetFrameLevel(),C:GetFrameLevel(),G:GetFrameLevel())`
   2. `/run P:SetFrameLevel(P:GetFrameLevel()+10) print(P:GetFrameLevel(),C:GetFrameLevel(),G:GetFrameLevel())`
8. **Inline textures in text** (decision T347; wowlua: the icon is as tall
   as the text for height 0, the atlas 20 by 20, each its width in the
   line):
   1. `/run S=UIParent:CreateFontString(nil,"OVERLAY","GameFontNormal") S:SetPoint("CENTER") S:SetText("ab") print(S:GetStringWidth(),S:GetStringHeight())`
   2. `/run S:SetText("a\124TInterface\\Icons\\INV_Misc_QuestionMark:0\124tb\124A:128-RedButton-Exit:20:20\124ac") print(S:GetStringWidth(),S:GetStringHeight())`
   3. A screenshot of the text in the middle of the screen.
9. **The dressing room** (decision T348; wowlua: `OnDressModel` once per
   changed slot, at once; the outfit list shows an item it does not know
   as not collectable, red):
   1. `/run DressUpItemLink("item:2575") DressUpFrame.ModelScene:HookScript("OnDressModel",function(_,...) print("dress",GetTime(),...) end)`
   2. `/run DressUpItemLink("item:6125")`
   3. A screenshot of the dressing room with the outfit list open.
10. **Gradients** (decision T349; wowlua: red at the left to blue at the
    right, and turned with the texture):
    1. `/run local t=UIParent:CreateTexture() t:SetSize(200,50) t:SetPoint("CENTER") t:SetColorTexture(1,1,1) t:SetGradient("HORIZONTAL",CreateColor(1,0,0,1),CreateColor(0,0,1,1)) T=t`
    2. A screenshot.
    3. `/run T:SetRotation(math.pi/4)` and a screenshot.
11. **`.png` and `.jpg` paths of `.blp` files** (decision T350; wowlua: a
    `.tga` or `.png` the storage lacks is its `.blp`, a `.jpg` unverified):
    1. `/run local t=UIParent:CreateTexture() for _,x in ipairs({".blp",".tga",".png",".jpg",""}) do print(x,t:SetTexture("Interface\\FriendsFrame\\StatusIcon-Online"..x),t:GetTexture()) end`
12. **Text in a scaled frame** (decision T351; wowlua: 0.8 the size, the
    width the same in the frame's units):
    1. `/run F=CreateFrame("Frame",nil,UIParent) F:SetScale(0.8) F:SetSize(200,40) F:SetPoint("CENTER") S=F:CreateFontString(nil,"OVERLAY","GameFontNormal") S:SetPoint("CENTER") S:SetText("Scaled 0.8 text")`
    2. `/run print(S:GetStringWidth(),S:GetEffectiveScale())`
    3. A screenshot.
13. **A creature display's portrait** (decision T353; wowlua: the face,
    framed as a unit's portrait):
    1. `/run local t=UIParent:CreateTexture() t:SetSize(128,128) t:SetPoint("CENTER") SetPortraitTextureFromCreatureDisplayID(t,7970)`
    2. A screenshot (7970 is Taragaman the Hungerer, AtlasLoot's).
14. **An unknown atlas** (decision T346; wowlua: `SetAtlas` leaves the
    texture as it was; its answer unknown):
    1. `/run local t=UIParent:CreateTexture() t:SetColorTexture(1,0,0) print(t:SetAtlas("NoSuchAtlas"), t:GetAtlas(), t:GetTexture())`
15. **Restricted callback events** (decision T336; wowlua: refused from
    addon code, `ADDON_ACTION_FORBIDDEN` with `Frame:RegisterEventCallback()`).
    It shows a blocked-action message: `/reload` after it.
    1. `/run local f=CreateFrame("Frame") print(f:RegisterEventCallback("COMBAT_LOG_ENTRIES_CLEARED",print))`
16. **Tooltip spacing** (decision T342; wowlua: 10 padding, 2 between
    lines, 40 between a double line's sides):
    1. `/run GameTooltip:SetOwner(UIParent,"ANCHOR_NONE") GameTooltip:SetPoint("CENTER") GameTooltip:AddLine("Title") GameTooltip:AddDoubleLine("Left","Right")`
    2. `/run GameTooltip:AddLine("A long line to see where and how a tooltip wraps its text",1,1,1,true) GameTooltip:Show() print(GameTooltip:GetSize())`
    3. A screenshot of the tooltip.
17. **PvP ranks 5 to 14** (decision T345; wowlua's formula gives 24150 for
    14, the table 24750):
    1. `/run for n=5,14 do print(n,C_MajorFactions.GetTotalReputationForRenownLevel(2800,n)) end`
    2. `/run for n=2,6 do local r=C_MajorFactions.GetRenownRewardsForLevel(2800,n) print(n,#r,r[1] and (r[1].name or r[1].itemID)) end`
18. **The calendar's holidays** (open: wowlua shows no holiday events):
    1. Open the calendar on this month and take a screenshot.
    2. `/run local m=C_Calendar.GetMonthInfo(0) for d=1,m.numDays do for j=1,C_Calendar.GetNumDayEvents(0,d) do local e=C_Calendar.GetDayEvent(0,d,j) print(d,e.title,e.calendarType,e.sequenceType,e.eventID) end end`
19. **The skill bars' fill.** In game (screenshot of 2026-10-23, build
    70532, the same Blizzard code as wowlua's) the skills list's bars fill
    the whole track at any value, the left `p` of the atlas stretched over
    it (`SetTexCoord(0, p, 1, 0)`), while the detail pane's bar fills by
    value (87/150 to 57%). wowlua fills the list's by value too. Blizzard's
    `SetFillPercent` sets the fill to `p * FillArea:GetWidth()`, so in game
    `FillArea` must read 0 when the rows are set, and a 0-wide fill with one
    point is drawn at its atlas size (240, clipped by the bar's mask). Why
    it reads 0 is open: rows made and initialized in the same frame are the
    likely cause, but Blizzard's micro menu and text-to-speech options read
    sizes of regions made moments before at login and need them at once
    (unless they fail silently in game; step 1 shows it). Steps:
    1. `/console scriptErrors 1`, then `/reload`; note any error shown at
       login (`FrameUtil.lua:247`, `TextToSpeechFrame.lua:597`?), then
       `/console scriptErrors 0`.
    2. `/reload`, and do not open the character window before step 4.
    3. `/run hooksecurefunc(SkillsBarMixin,"SetFillPercent",function(b,p) local a=b.FillArea print(format("%.2f",p),a:GetWidth(),tostring(a:IsRectValid()),b:GetWidth(),b:GetParent():GetParent():GetWidth()) end)`
    4. Open the character window's Skills tab; note the lines printed.
    5. `/run for _,f in ipairs(SkillsFrame.ScrollBox:GetFrames()) do local s=f.Content and f.Content.SkillsBar if s then print(s.FillArea:GetWidth(),s.Fill:GetWidth(),s.Fill:GetTexCoord()) end end`
    6. `/run local b=SkillsFrame.SkillDetailFrame.RankBar print(b.FillArea:GetWidth(),b.Fill:GetWidth())`
    7. `/run T=UIParent:CreateTexture(nil,"OVERLAY") T:SetAtlas("common-stat-bar-blue",true) T:SetPoint("LEFT",UIParent,"CENTER",-100,0) T:SetWidth(0) print(T:GetSize())`
    8. A screenshot: is a 240-wide blue bar drawn in the middle (wowlua:
       nothing)?
    9. `/run local p=CreateFrame("Frame",nil,UIParent) p:SetPoint("CENTER") p:SetSize(100,50) local c=CreateFrame("Frame",nil,p) c:SetAllPoints() local g=CreateFrame("Frame",nil,c) g:SetAllPoints() print(g:GetWidth(),g:IsRectValid(),c:IsRectValid())`
       (wowlua: `100 true true`): whether a chain made in one call has its
       size at once.
20a. **Chat the player sends** (decision T360; wowlua: `CHAT_MSG_SAY`
    back, `CHAT_MSG_WHISPER_INFORM` then `CHAT_MSG_WHISPER` for a whisper to
    yourself, `ERR_CHAT_PLAYER_NOT_FOUND_S` for a name nobody has, `ERR_NOT_IN_GROUP`
    for /p alone):
    1. `/run F=CreateFrame("Frame") for _,e in ipairs({"CHAT_MSG_SAY","CHAT_MSG_WHISPER","CHAT_MSG_WHISPER_INFORM","CHAT_MSG_SYSTEM","CHAT_MSG_PARTY"}) do F:RegisterEvent(e) end F:SetScript("OnEvent",function(_,e,...) print(e,...) end)`
    2. `/s probe one`, then `/p probe two` (not in a group).
    3. `/w <your first and last name> probe three`, then `/w Nobody Anyone probe four`.
    4. A screenshot of the chat with the printed lines.
20b. **Typed commands' answers** (decision T362; wowlua's lines in the
    decision). With the frame of 20a also registering `CHAT_MSG_TEXT_EMOTE`,
    `TIME_PLAYED_MSG`, `WHO_LIST_UPDATE`, `PLAYER_FLAGS_CHANGED`, and
    `IGNORELIST_UPDATE`, type each and note what prints:
    1. `/roll`, `/roll 1 6`, `/played`, `/afk`, `/afk`, `/dnd`, `/dnd busy`
    2. `/ignore Nobody Anyone`, `/invite Nobody Anyone`, `/ginvite Nobody Anyone`,
       `/gpromote Nobody Anyone`, `/gquit` (as guild master), `/who Nobody`
    3. `/dance`, `/friend Nobody Anyone`, `/1 hello`, `/join Trade`
20c. **Inviting players** (decision T363), with a second character of yours
    online: `/invite` it and note the lines and how long until it shows in
    the party after accepting; `/promote` and `/kick` it; `/who` with three
    or fewer results and with more (the chat lines); `/ginvite` (the lines
    after it accepts).
20d. **Mouse scripts turn the mouse on** (decision T365; wowlua: `false
    true true true true true`; the in-game suite's "Mouse script enables"
    asks the same):
    1. `/run local f=CreateFrame("Frame") local a=f:IsMouseEnabled() f:SetScript("OnMouseUp",function() end) local b=f:IsMouseEnabled() f:SetScript("OnMouseUp",nil) print(a,b,f:IsMouseEnabled())`
    2. `/run local g=CreateFrame("Frame") g:SetScript("OnEnter",function() end) local h=CreateFrame("Frame") h:SetScript("OnMouseWheel",function() end) print(g:IsMouseEnabled(),h:IsMouseEnabled(),h:IsMouseWheelEnabled())`
    3. Open the character frame (C) and type `/run local t=CharacterFrameModeTab2 print(t:IsMouseEnabled(),t:IsMouseClickEnabled(),t:IsMouseMotionEnabled())`
20e. **`BNGetInfo()`** (two recordings disagree: the full probe of
    2026-10-20 has nothing, the answers of 2026-10-21 eight values; wowlua
    answers the eight):
    1. Right after logging in (before opening anything): `/run print(select("#",BNGetInfo()),BNGetInfo())`
    2. `/reload`, then the same line again.
    3. `/run print(select("#",BNGetInfo("player")),BNConnected())`
20. **Screenshots to compare with wowlua's** (in `~/Desktop/wowlua-*`):
    1. Settings, Accessibility: the "Colors" category and the "Use UI
       Scale" check box (Graphics).
    2. The Forever Dungeon Journal: a boss with its portrait (the ring under
       it).
    3. EllesmereUI: the action bars' key labels and the chat window's text.
    4. AtlasLoot: Ragefire Chasm, Taragaman, its loot list.


## Owed from M13 (next beta, about 2026-11-07)

What the M13 work (the client's tables in tests) guessed. Each item stands
alone: after a `/reload` (or an error) start it again from its first step.
Take a screenshot after each step that names one.

1. **A tooltip's wrap width** (decision T393; wowlua wraps at the widest
   other line, at least 300). Take a screenshot after each of:
   1. `/run GameTooltip:SetOwner(UIParent,"ANCHOR_CURSOR") GameTooltip:SetSpellByID(26573) GameTooltip:Show() print(GameTooltip:GetWidth())`
   2. `/run GameTooltip:SetOwner(UIParent,"ANCHOR_CURSOR") GameTooltip:AddLine("Short") GameTooltip:AddLine(string.rep("word ",60),1,1,1,true) GameTooltip:Show() print(GameTooltip:GetWidth())`
2. **A spell's description with spell power** (decision T391; wowlua adds
   the coefficient times `GetSpellBonusDamage`): on a paladin with gear
   that gives spell power:
   1. `/run print(GetSpellBonusDamage(2), GetSpellBonusHealing())`
   2. `/run print(C_Spell.GetSpellDescription(26573))`
   3. `/run print(C_Spell.GetSpellDescription(635))`
3. **Currencies, factions and mounts the character doesn't have**
   (task #30: what the client answers for an ID it knows but the character
   lacks):
   1. `/dump C_CurrencyInfo.GetCurrencyInfo(1792)`
   2. `/dump C_CurrencyInfo.GetCurrencyInfo(1901)`
   3. `/dump C_Reputation.GetFactionDataByID(69)`
   4. `/dump C_Reputation.GetFactionDataByID(930)`
   5. `/dump C_MountJournal.GetMountInfoByID(6)`

## Owed from M14 (next beta, about 2026-11-07)

1. **When `SETTINGS_PANEL_OPEN` comes** (decision T400; wowlua fires it
   inside `C_SettingsUtil.OpenSettingsPanel`, at once). With the settings
   closed:
   1. `/run local f=CreateFrame("Frame") f:RegisterEvent("SETTINGS_PANEL_OPEN") f:SetScript("OnEvent",function(_,e,a,b) print("event",a,b,GetTime()) end) WOWLUA_F=f`
   2. `/run print("before",GetTime()) C_SettingsUtil.OpenSettingsPanel() print("after",GetTime(),SettingsPanel:IsShown())`

   Note the order of the three lines ("before", "event", "after"): is the
   event before "after", and is the panel shown at "after"?

## Owed from the contributions in 0.13.5 (next beta, about 2026-11-07)

What pull requests #81 to #91 guessed. Their decisions (T485 to T490)
stay proposals, open for review, until these answer. Each item stands
alone: after a `/reload` (or an error) start it again from its first step.
After each step, note what it prints; take a screenshot where a step says
so. "wowlua:" is what the engine answers now.

1. **`GetTotemInfo` beyond the first four slots** (#81; slots 1 to 4
   recorded empty, 5 to 8 guessed the same):
   1. `/run for s=0,9 do print(s, select("#", GetTotemInfo(s)), GetTotemInfo(s)) end`

   wowlua: slots 1 to 8 `7 false  0 0 nil 0 0`, slots 0 and 9 `0`.
2. **Aura options left out** (#82; wowlua leaves out an optional structure
   unless every field of it has a default):
   1. `/dump C_AuraContainerUtil.ProcessCustomAuraButtonDurationTextOptions({})`
   2. `/dump C_AuraContainerUtil.ProcessAuraTooltipBackdropOptions({ backdropInfo = {} })`

   wowlua: the first has no `textColor` and no `textFormat`; the second
   has `anchorOffsets` with `left = 0`.
3. **`UNIT_IN_RANGE_UPDATE`** (decision T485): in a party with another
   player, who can walk away.
   1. `/run local f=CreateFrame("Frame") f:RegisterEvent("UNIT_IN_RANGE_UPDATE") f:SetScript("OnEvent",function(_,e,u,r) print(e,u,issecretvalue(r)) end) WOWLUA_RANGE=f`
   2. The other player walks more than 40 yards away.
   3. The other player comes back next to you.

   wowlua: one line at each crossing of 38 yards, for `party1` only, with
   `true` (the payload secret). Note every token it names, and whether
   the payload is secret.
4. **The heal prediction calculator** (decision T486):
   1. `/run local c=CreateUnitHealPredictionCalculator() print(c:GetIncomingHealOverflowPercent(), c:GetMaximumHealthMode(), c:GetHealAbsorbMode(), c:GetDamageAbsorbClampMode(), c:GetHealAbsorbClampMode(), c:GetIncomingHealClampMode())`
   2. `/run local c=CreateUnitHealPredictionCalculator() c:SetPredictedValues({health=60000,healthMax=100000,totalDamageAbsorbs=36000,totalHealAbsorbs=5000,totalIncomingHeals=12000,totalIncomingHealsFromHealer=8000}) print(c:GetIncomingHeals()) print(c:GetHealAbsorbs()) print(c:GetDamageAbsorbs()) print(c:GetMaximumIncomingHeals(), c:GetMaximumDamageAbsorbs(), c:GetMaximumHealAbsorbs())`
   3. `/run local c=CreateUnitHealPredictionCalculator() c:SetPredictedValues({health=60000,healthMax=100000,totalDamageAbsorbs=36000,totalHealAbsorbs=5000,totalIncomingHeals=12000,totalIncomingHealsFromHealer=8000}) c:SetHealAbsorbMode(Enum.UnitHealAbsorbMode.Total) print(c:GetIncomingHeals()) print(c:GetHealAbsorbs()) print(c:GetDamageAbsorbs())`

   wowlua: step 1 `1 0 0 0 0 0`; step 2 `7000 7000 0 false`, `0 false`,
   `33000 true`, `40000 33000 60000`; step 3 `12000 8000 4000 false`,
   `5000 false`, `28000 true`.
5. **Shields and `UNIT_HEALTH`** (decision T486): on a character that can
   get an absorb shield (a priest's Power Word: Shield).
   1. `/run local f=CreateFrame("Frame") for _,e in ipairs({"UNIT_ABSORB_AMOUNT_CHANGED","UNIT_HEAL_ABSORB_AMOUNT_CHANGED","UNIT_HEALTH","UNIT_HEAL_PREDICTION"}) do f:RegisterUnitEvent(e,"player") end f:SetScript("OnEvent",function(_,e) print(GetTime(),e) end) WOWLUA_ABS=f`
   2. Get the shield, then take one hit the shield takes whole.
   3. Take a hit bigger than what is left of the shield.

   wowlua: the shield's `UNIT_ABSORB_AMOUNT_CHANGED` before `UNIT_HEALTH`,
   and no `UNIT_HEALTH` for the hit the shield takes whole.
6. **A status bar on a timer** (decision T487):
   1. `/run local b=CreateFrame("StatusBar",nil,UIParent) b:SetSize(200,20) b:SetPoint("CENTER") b:SetStatusBarTexture("Interface\\Buttons\\WHITE8X8") print(b:GetMinMaxValues()) b:SetScript("OnValueChanged",function() WOWLUA_VC=(WOWLUA_VC or 0)+1 end) b:SetScript("OnMinMaxChanged",function() WOWLUA_MC=(WOWLUA_MC or 0)+1 end) local d=C_DurationUtil.CreateDuration() d:SetTimeFromStart(GetTime(),10) b:SetTimerDuration(d) WOWLUA_BAR=b print(b:GetMinMaxValues()) print(b:GetValue())`
   2. About 3 seconds later, with a screenshot: `/run print(WOWLUA_BAR:GetMinMaxValues()) print(WOWLUA_BAR:GetValue(), WOWLUA_VC, WOWLUA_MC)`
   3. `/run WOWLUA_BAR:SetValue(3) print(WOWLUA_BAR:GetValue())`
   4. About 2 seconds later: `/run print(WOWLUA_BAR:GetValue())`
   5. `/run local b=CreateFrame("StatusBar",nil,UIParent) b:SetMinMaxValues(0,200) local d=C_DurationUtil.CreateDuration() d:SetTimeFromStart(GetTime(),10) b:SetTimerDuration(d,Enum.StatusBarInterpolation.Immediate,Enum.StatusBarTimerDirection.RemainingTime) WOWLUA_BAR2=b print(b:GetMinMaxValues()) print(b:GetValue())`
   6. About 3 seconds later: `/run print(WOWLUA_BAR2:GetValue()) WOWLUA_BAR2:SetMinMaxValues(0,0) print(WOWLUA_BAR2:GetValue())`

   wowlua: step 1 `0 0` before the timer, `0 10` and `0` after it; step 2 `0 10`, about
   `3`, `nil nil` (no scripts run); steps 3 and 4 `3` (a value set stops
   the timer); step 5 `0 10` and `10`; step 6 about `7`, then `0`.
7. **A frame anchored to a bar's fill, with a secret value** (decision
   T490): in a dungeon, where the player's health is secret in combat.
   1. Out of combat: `/run local b=CreateFrame("StatusBar",nil,UIParent) b:SetSize(200,20) b:SetPoint("CENTER") b:SetStatusBarTexture("Interface\\Buttons\\WHITE8X8") local n=CreateFrame("Frame",nil,b) n:SetPoint("TOPLEFT",b:GetStatusBarTexture(),"TOPRIGHT") n:SetPoint("BOTTOMLEFT",b:GetStatusBarTexture(),"BOTTOMRIGHT") n:SetWidth(10) local t=n:CreateTexture() t:SetAllPoints() t:SetColorTexture(1,0,0) WOWLUA_HB=b WOWLUA_HN=n print(b:GetStatusBarTexture():GetNumPoints())`
   2. In combat, with a screenshot: `/run WOWLUA_HB:SetMinMaxValues(0,UnitHealthMax("player")) WOWLUA_HB:SetValue(UnitHealth("player")) print(issecretvalue(UnitHealth("player")), issecretvalue(WOWLUA_HN:GetLeft()))`
   3. Still in combat: `/run print(WOWLUA_HN:GetLeft())`

   wowlua: step 1 `0`; step 2 `true false`, the red square at the fill's
   right edge; step 3 a plain number. If step 2 prints `false` first, the
   health isn't secret there: try again in a boss fight.

## Owed from the world profiles (next beta, about 2026-11-07)

The snapshot moved from StdTest into the dev runtime (decision T498):
`/wl snapshot` takes it in any dev build and `wowlua profile snapshot`
writes the profile. Its calls are StdTest's, probed before; these check the
move. Install StdTest (`wowlua install --client _forever_` in
`tests/ingame`) and log in with a character that has gear, bags, action
bars and a target.

1. **`/wl snapshot`**:
   1. `/reload`
   2. `/wl snapshot StdTest`
   3. Wait for the line `StdTest: snapshot taken; /reload, then wowlua profile snapshot <name>`
      and note whether it says that parts failed.
   4. `/reload`
   5. On the desktop, in `tests/ingame`: `wowlua profile snapshot probe-world --client _forever_`

   Send `tests/ingame/tests/profiles/probe-world.json` and the command's
   output. The file must not name your character or realm, and its
   `trained` must list your spellbook's spells (decision T500): Attack
   (6603) among them.
2. **`/stdtest snapshot` through the runtime's**:
   1. `/stdtest snapshot`
   2. Wait for `snapshot: recorded`, then `/reload`
   3. On the desktop: `cargo xtask profile "<WoW>/_forever_/WTF/Account/<account>/SavedVariables/StdTest.lua" stdtest-world.json`

   Send `stdtest-world.json`.

## The other clients' answers (decisions T510, T511)

Recorded on 2026-10-29 on Retail 12.1.5 (70575), MoP Classic 5.5.4
(70631), TBC Anniversary 2.5.6 (70577) and Classic Era 1.15.9 (70576),
with a character alone, no target and out of combat (a Death Knight on
Retail and MoP Classic). Run each step again on each of those clients when
they change, and note what it prints. "wowlua:" is what the engine answers
now; "Recorded:" what the clients printed.

1. **Functions the documentation lacks**:
   1. `/dump type(GetQuestLogTitle), type(GetGlyphSocketInfo), type(GetReadyCheckStatus)`

   wowlua: Retail `nil nil function`; MoP Classic `function function
   function`; TBC and Classic Era `function nil function`.
   Recorded: the same.
2. **Nothing there**:
   1. `/dump IsTargetLoose(), C_ActionBar.GetBonusBarOffset(), UnitIsTapDenied("target"), GetRaidTargetIndex("target")`
   2. `/dump select("#", GetReadyCheckStatus("player")), C_TTSSettings.GetSpeechRate(), C_TTSSettings.GetSpeechVolume()`

   wowlua: `false 0 false nil` and `0 0 100` on every client.
   Recorded: the same.
3. **Expansions** (after the login, as the server's is 0 before):
   1. `/dump GetExpansionLevel(), GetServerExpansionLevel(), GetAccountExpansionLevel(), GetMaximumExpansionLevel(), GetMinimumExpansionLevel()`
   2. `/dump GetClassicExpansionLevel and GetClassicExpansionLevel()`

   wowlua: Retail `11 11 11 11 10`; MoP Classic `4 4 0 4 3`; TBC `1 1 0 1
   5`; Classic Era `0 0 0 0 5`.
   Recorded: the same, and `GetClassicExpansionLevel()` 11, 4, 1 and 0.
4. **Class colours**:
   1. `/run for _, c in ipairs({"WARRIOR", "SHAMAN", "MONK", "DEATHKNIGHT", "EVOKER"}) do local x = C_ClassColor.GetClassColor(c) print(c, x and x.r, x and x.g, x and x.b) end`

   wowlua: Retail's colours where the client has the class (Classic Era's
   Shaman the Paladin's pink), nothing for a class it lacks.
   Recorded: the same values, to the last digit.
5. **Refused functions**:
   1. `/run for _, n in ipairs({"GetTalentGroupRole", "GetNumTalentTabs", "GetNumTalentGroups", "GetUnspentTalentPoints", "GetNumSpecGroups", "GetTalentTierInfo"}) do print(n, pcall(_G[n], 1)) end`

   wowlua: on MoP Classic the first four `false Script_<name>: API
   unsupported in this version of World of Warcraft.`; on TBC and Classic
   Era `GetTalentGroupRole`, `GetNumSpecGroups` and `GetTalentTierInfo`.
   Recorded: the same. The others answer: on Retail the first four do not
   exist, `GetNumSpecGroups(1)` is 0 and `GetTalentTierInfo(1)` nothing (as
   on MoP Classic, and in wowlua); on TBC and Classic Era `GetNumTalentTabs`
   3, `GetNumTalentGroups` 1 and `GetUnspentTalentPoints` the character's
   (0 at level 1, 1 at level 11), which a test mocks.
6. **Runes and weapon enchants** (the classic clients):
   1. `/run for i = 1, 7 do print(i, GetRuneType(i)) end`
   2. `/run print(select("#", GetWeaponEnchantInfo()))`

   wowlua: `1 1 3 3 2 2` and nothing for rune 7; 8 values on MoP Classic,
   12 on TBC and Classic Era.
   Recorded: the same. On Retail `GetRuneType` does not exist, and
   `GetWeaponEnchantInfo` gives 12 values: Blizzard's deprecation fallback
   (`Blizzard_Deprecated`, `Deprecated_12_1_0.lua`) defines it, not the
   client, so wowlua's `standard` has none without Blizzard's code.
7. **Hardcore** (the classic clients):
   1. `/dump C_GameRules.IsHardcoreActive()`

   wowlua: `false` (a test sets a Classic Era hardcore realm with
   `mock.setHardcore(true)`).
   Recorded: `false` on the classic clients; on Retail the function does
   not exist.

## The untyped functions (M11's full API types)

About 55 functions (2026-11-01) are still `(...unknown) -> ...unknown`:
no source types them (warcraft.wiki.gg has no pages for them, Ketho's
annotations no types, Blizzard's Lua calls only a few). These steps record
what the getters among them answer, so the type generator can type them.
Only getters: the functions that act (`DoCraft`, `StartAuction`,
`GMSubmitSuggestion`, the combat log's filters, …) are not called. Run each
step on the clients named, with a character alone, no target and out of
combat, and note what it prints. Every step starts with the helper `P`,
which calls a function safely and prints each answer with its type
(`number=0.05`); `false` after the name is the error the call raised.

1. **Arena teams and the auction house** (Classic Era, TBC Anniversary,
   MoP Classic):
   1. `/run P=function(n,...) local function f(ok,...) local t={} for i=1,select("#",...) do local v=select(i,...) t[i]=type(v).."="..tostring(v) end print(n,ok,table.concat(t," ")) end f(pcall(_G[n],...)) end`
   2. `/run P("GetAuctionHouseDepositRate") P("GetArenaTeamRosterShowOffline") P("GetPreviousArenaSeasonUsesTeams") P("IsInArenaTeam") P("IsRatedBattleground")`

   wowlua: untyped. Recorded: (to do)
2. **Spells by ID** (Classic Era, TBC Anniversary, MoP Classic; 6603 is
   Auto Attack):
   1. `/run P=function(n,...) local function f(ok,...) local t={} for i=1,select("#",...) do local v=select(i,...) t[i]=type(v).."="..tostring(v) end print(n,ok,table.concat(t," ")) end f(pcall(_G[n],...)) end`
   2. `/run P("GetSpellSubtext",6603) P("GetSpellRank",6603) P("IsHelpfulSpell",6603) P("IsHarmfulSpell",6603) P("SpellHasRange",6603)`
   3. `/run P("IsAutoRepeatSpell",6603) P("IsConsumableSpell",6603)`

   wowlua: untyped. Recorded: (to do)
3. **Spells by their place in the spellbook** (the same clients):
   1. `/run P=function(n,...) local function f(ok,...) local t={} for i=1,select("#",...) do local v=select(i,...) t[i]=type(v).."="..tostring(v) end print(n,ok,table.concat(t," ")) end f(pcall(_G[n],...)) end`
   2. `/run P("GetSpellSubtext",1,"spell") P("IsHelpfulSpell",1,"spell") P("IsHarmfulSpell",1,"spell") P("SpellHasRange",1,"spell") P("IsAutoRepeatSpell",1,"spell")`

   wowlua: untyped. Recorded: (to do)
4. **Quests** (the same clients; with a quest in the log and tracked, and
   once with none):
   1. `/run P=function(n,...) local function f(ok,...) local t={} for i=1,select("#",...) do local v=select(i,...) t[i]=type(v).."="..tostring(v) end print(n,ok,table.concat(t," ")) end f(pcall(_G[n],...)) end`
   2. `/run P("GetQuestWatchInfo",1) P("IsQuestHardWatched",1) P("QuestPOIGetQuestIDByIndex",1) P("IsUnitOnQuestByQuestID",1,"player")`

   wowlua: untyped. Recorded: (to do)
5. **Professions and talents** (the same clients; once with a profession
   window open):
   1. `/run P=function(n,...) local function f(ok,...) local t={} for i=1,select("#",...) do local v=select(i,...) t[i]=type(v).."="..tostring(v) end print(n,ok,table.concat(t," ")) end f(pcall(_G[n],...)) end`
   2. `/run P("GetTradeSkillItemLevelFilter") P("GetTradeSkillItemNameFilter") P("GetPreviewTalentPointsSpent") P("GetPreviewTalentPointsSpent",1) P("GetArenaTeamGdfInfo")`

   wowlua: untyped. Recorded: (to do)
6. **The combat log's entries** (MoP Classic and Retail):
   1. `/run P=function(n,...) local function f(ok,...) local t={} for i=1,select("#",...) do local v=select(i,...) t[i]=type(v).."="..tostring(v) end print(n,ok,table.concat(t," ")) end f(pcall(_G[n],...)) end`
   2. `/run P("CombatLogGetNumEntries") P("CombatLogGetNumEntries",true) P("CombatLogGetCurrentEntry")`

   wowlua: untyped. Recorded: (to do)

## Answered on the beta's last day (2026-10-24)

- PvP ranks (major faction 2800): `GetMajorFactionProgressionInfo` gives
  maxLevel 14, renownLevel 0, renownLevelThreshold 750, the weekly fields
  0 and weekNumber -1 on a rank 0 character; ranks 1 to 4 total 750, 1650,
  2700, 3900; rank 1's reward is the Faction Tabard (decision T345).
- A frame anchored to UIParent has its width and left at once and on the
  next frame alike (200, 100).
- A frame anchored inside a frame that is sized but not placed reports
  width 0 (the parent 200): `child 0 200` (decision T346).
- A texture with `SetAtlas(atlas, true)`, a `LEFT` point and `SetWidth(0)`
  reports width 0 and its atlas's height.
- The macro window shows no "Create Macros" title in game either.
- `|cAARRGGBB`'s alpha could not be checked from the chat box (it escapes
  `|`); decision T342 stays a proposal.
- Still owed: the skill bars' fill (item 19 of "Owed from the M12
  screenshots").

## Moving and facing (decisions T516, T519)

Unverified numbers and answers from M13's movement and ranges. A paladin
(or any class with an instant harmful spell that needs facing, and a heal).

1. **Speeds**: stand still and enter
   `/run print(GetUnitSpeed("player"))`, then press the walk key (`/` on
   the numpad by default) and enter it again while walking forward. Answers
   the run and walk speeds (wowlua: 7 and 2.5).
2. **A spell at a target behind you**: enter
   `/run local f=CreateFrame("Frame") f:RegisterEvent("UI_ERROR_MESSAGE") f:SetScript("OnEvent",function(_,_,c,t) print(c,t) end)`,
   target a hostile mob within 10 yards, turn your back to it and press
   Judgement (or the class's instant attack spell). Answers the code and
   text (wowlua: 57, "Target needs to be in front of you.").
3. **A heal with a far hostile target**: with the frame from step 2, target
   a hostile mob more than 45 yards away and press Flash of Light (or the
   class's heal). Note whether it heals you or prints "Out of range."
   (wowlua: it heals you).
4. **The bars' range at login**: `/reload` with a hostile mob 10 yards away
   targeted, then enter
   `/run local f=CreateFrame("Frame") f:RegisterEvent("ACTION_RANGE_CHECK_UPDATE") f:SetScript("OnEvent",function(_,_,s,i,c) print(s,i,c) end)`
   and walk away from it until a button's hotkey turns red. Answers the
   slots told and their values (wowlua: each slot's `false true` once).

## A solid world (decisions T524, T532)

1. **Line of sight**: enter
   `/run local f=CreateFrame("Frame") f:RegisterEvent("UI_ERROR_MESSAGE") f:SetScript("OnEvent",function(_,_,c,t) print(c,t) end)`,
   target a mob with a wall or hill between you and press an attack spell
   in range. Answers the code and text (wowlua: 57, "Target not in line of
   sight").
2. **A step's height**: walk onto a crate or a ledge and note the highest
   one you step onto without jumping (wowlua: 0.6 yards, to the knee).
3. **The camera and the trees**: zoom the camera out, then turn it so a
   tree's trunk stands between it and your character. Note whether the
   camera moves in front of the trunk, as it does for a wall (wowlua: it
   does not).

## Names over units (decision T534)

Any character, outdoors near a town's NPCs, with nameplates for friendly
NPCs off.

1. **The settings**: enter
   `/run for _,n in ipairs({"UnitNameNPC","UnitNameInteractiveNPC","UnitNameHostleNPC","UnitNameFriendlySpecialNPCName","UnitNameNonCombatCreatureName","nameplateShowAll","nameplateShowEnemies","nameplateShowFriendlyNpcs","nameplateMaxDistance"}) do print(n, GetCVar(n)) end`.
   Answers the settings wowlua reads.
2. **A guard**: stand by a guard (a friendly NPC with nothing to sell)
   and note whether its name shows, then enter
   `/run SetCVar("UnitNameFriendlySpecialNPCName", 0)` and note whether
   it still shows (wowlua: it goes).
3. **How far**: walk away from a vendor until its name goes, then enter
   `/run print(UnitPosition("player"))`; walk back to it and enter the
   same. Answers how far names show (wowlua: as far as nameplates, 45
   yards).
4. **Plates out of combat**: enter `/run SetCVar("nameplateShowAll", 0)`
   and look at a hostile mob out of combat: note whether its plate shows
   (wowlua: not until it is in combat). Enter
   `/run SetCVar("nameplateShowAll", 1)` after.
5. **The look**: take a screenshot (Print Screen) of a vendor's name and
   title close up, for their size, colors and height over its head
   (wowlua: 15 and 13 pixels at 1080, green, 2.4 yards up).

## How far the world is drawn (decision T525)

1. **The settings**: enter
   `/run for _,n in ipairs({"farclip","terrainLodDist","horizonStart","lodObjectCullSize","lodObjectMinSize","wmoLodDist"}) do print(n, GetCVar(n)) end`.
   Answers the view distance settings wowlua draws by.
2. **Small things far away**: stand at Northshire's start facing the
   abbey, then walk back from it. Note at about which distance the
   lanterns and crates disappear (wowlua: where they look smaller than 18
   pixels).

## Frame rate and render scale (decisions T538, T539)

1. **The render scale's limits**: enter
   `/run SetCVar("RenderScale","0.1") print(GetCVar("RenderScale"))`,
   then `/run SetCVar("RenderScale","4") print(GetCVar("RenderScale"))`,
   then `/run SetCVar("RenderScale","1")`.
   Answers whether the client keeps the setting to 0.33 to 2 (wowlua does).
2. **How GetFramerate averages**: enter
   `/run SetCVar("maxFPS","30") C_Timer.After(2,function() print(GetFramerate()) end)`,
   then `/run SetCVar("maxFPS","120") C_Timer.After(0.1,function() print(GetFramerate()) end)`.
   Answers whether the frame rate follows at once (one frame's time) or
   over a second (wowlua: the last second's frames).
3. **The background's frame rate**: enter
   `/run C_Timer.After(5,function() print(GetFramerate()) end)`, then click
   another window within the 5 seconds and wait. Answers the frame rate in
   the background (wowlua: `maxFPSBk`, 30).

## The camera (decisions T520, T521)

The camera's numbers wowlua could not read from the settings. Any
character, outdoors.

1. **The furthest zoom**: scroll all the way out, then enter
   `/run print(GetCameraZoom(), GetCVar("cameraDistanceMaxZoomFactor"))`.
   Answers the furthest zoom (wowlua: 15 times the factor).
2. **Zooming at once**: enter
   `/run CameraZoomOut(5) print(GetCameraZoom())`. Answers whether the
   distance changes at once (wowlua) or as the camera glides.
3. **A view never saved**: enter `/run ResetView(3) SetView(3)`. Note where
   the camera goes (wowlua: it stays).
4. **A nameplate's height**: target a mob 20 yards ahead on flat ground,
   then enter
   `/run local p=C_NamePlate.GetNamePlateForUnit("target") print(p:GetCenter()) print(WorldFrame:GetCenter())`.
   Answers how far above the screen's middle its plate sits (wowlua: the
   pivot 2 yards over the player's feet, the plate 2.5 over the mob's).

## The dev tools (0.12.1)

The dev window's performance tab, hints, HUD and error toast are tested in
the emulator (`crates/wowlua-cli/tests/devtools.rs`); these check what the
emulator cannot: the fonts, the look and the real frame times. StdTest's dev
build has them all (`wowlua install` in `tests/ingame`).

1. **The window in the design system's fonts**: `/wl`, then every tab.
   Titles in Martian Mono, everything else in IBM Plex Mono; no empty boxes
   (a glyph the fonts lack). A screenshot of the performance tab's
   functions view and of the flame view (switch call paths on first).
2. **A combat and a boss pull**: fight something, then a boss in a
   dungeon; the recordings list gets `combat N` and `pull N` with the boss's
   name. Compare the pull with `live` (right-click): the change column is
   per minute.
3. **The HUD**: `/wl hud`, move it, `/reload`: it comes back where it was.
   It hides while the window is open and during a cinematic, not in a pet
   battle.
4. **The toast**: an error from StdTest's own code (any probe that fails;
   `/run error("x")` is not the addon's and shows none) shows the toast at
   the top for 8 seconds, longer with the cursor on it; in combat it waits
   for the end of combat. Clicking it opens the errors tab.
5. **Export**: mark the pull for export, `/reload`, and check that
   `WTF/Account/<account>/SavedVariables/StdTest.lua` holds
   `WowluaDevDB_StdTest` with `exports`; `wowlua prof report` reads it.

## The Camelot update (decision T553)

On WoW Forever: from its launch on 5 November 2026 (no login before).

The declarations and Blizzard's UI code now come from the installed client
(1.60.1.70658 or newer), but the engine's recordings are of 70425: its
`Enum` corrections, CVars, number globals, colors, spells, talents and
trainers. These steps record them again on the updated client.

1. **Update the client**: start the Battle.net launcher, let it update WoW
   Forever, and log in a character once (the update is then on disk).
2. **Install StdTest**: in `tests/ingame`, `wowlua install --client _forever_`,
   then `/reload` in game.
3. **The engine**: `/stdtest engine`, wait for it to finish, then `/reload`.
4. **Take it in**: `cargo xtask ingest`, then `cargo xtask types fetch` and
   `cargo xtask types gen`.

   wowlua: `engine_probe`'s `NEWER_BUILD` lists what 70658 changed. Recorded:
   (to do)
5. **One color** (decision T554): `/run for _,c in ipairs(C_UIColor.GetColors()) do if c.baseTag=="COMPACT_UNIT_FRAME_FRIENDLY_HEALTH_COLOR" then print(c.color:GetRGBA()) end end`

   wowlua: `0.16862745583057 0.57647061347961 0.019607843831182 0`, the
   installed table's alpha (0); 70425 answered 1. Recorded: (to do)

## The Classic clients' data (decision T554)

Each Classic client answers for itself: run every step on MoP Classic,
TBC Anniversary and Classic Era, with a character alone, no target and out
of combat, and note what each prints. "wowlua:" is what the engine answers
now.

1. **Locales**:
   1. `/run for i,l in ipairs(GetAvailableLocaleInfo()) do print(i,l.localeId,l.localeName) end`

   wowlua: nothing (no answer yet on these clients). Recorded: (to do)
2. **Loot rolls and product choices**:
   1. `/dump C_LootHistory.GetNumItems(), #C_ProductChoice.GetChoices()`

   wowlua: `0 0` (unverified). Recorded: (to do)
3. **Max level**:
   1. `/dump GetMaxPlayerLevel(), GetMaxLevelForPlayerExpansion(), GetMaxLevelForLatestExpansion()`

   wowlua: MoP Classic `90 90 90`, TBC Anniversary `70 70 70`, Classic Era
   `60 60 60` (the flavor table's). Recorded: (to do)
4. **Skill lines**:
   1. `/dump GetNumSkillLines()`
   2. `/dump GetSkillLineInfo(1)`
   3. `/dump GetSkillLineInfo(2)`

   wowlua: nothing (skills are not modelled). Recorded: (to do)
5. **Functions the dump lacks**:
   1. `/dump type(C_UnitAurasPrivate), type(C_UnitAurasPrivate and C_UnitAurasPrivate.SetShowDispelTypeCallback), type(C_StoreSecure)`

   wowlua: `nil nil nil` (Ketho's dumps of older builds lack them).
   Recorded: (to do)
6. **Constants the client defines**:
   1. `/dump Constants.InventoryConstants.NumGenericBankSlots, LE_GAME_ERR_SPELL_FAILED_TOTEMS, LE_GAME_ERR_SPELL_FAILED_EQUIPPED_ITEM`
   2. `/run local n=0 for k in pairs(_G) do if type(k)=="string" and k:find("^LE_GAME_ERR_") then n=n+1 end end print(n)`

   wowlua: `nil nil nil` and `0`. Recorded: (to do)

## The world's motion (decision T558)

On WoW Forever: from its launch on 5 November 2026 (no login before).

Look, do not run anything: on WoW Forever, stand at Northshire Abbey on a
clear day and watch the trees and bushes about it for ten seconds.

1. **Trees in the wind**: do their leaves or branches sway?

   wowlua: they stand still (their models have no animation; a sway in
   the game would be the shaders' wind). Seen: (to do)

## The Graphics settings (decision T561)

What each Graphics setting's levels set, recorded on each client. It
changes your graphics settings for about a minute and puts every one back
at the end. The Classic clients can be recorded now; WoW Forever from its
launch on 5 November 2026 (no login before).

1. **Classic Era**:
   1. in `tests/ingame`: `wowlua install --client _classic_era_`
   2. in game: `/reload`, then `/stdtest engine graphics`, wait for
      "engine graphics: … lines", then `/reload`
   3. `cargo xtask ingest --client _classic_era_`
2. **TBC Anniversary**:
   1. in `tests/ingame`: `wowlua install --client _anniversary_`
   2. in game: `/reload`, then `/stdtest engine graphics`, wait for
      "engine graphics: … lines", then `/reload`
   3. `cargo xtask ingest --client _anniversary_`
3. **MoP Classic**:
   1. in `tests/ingame`: `wowlua install --client _classic_`
   2. in game: `/reload`, then `/stdtest engine graphics`, wait for
      "engine graphics: … lines", then `/reload`
   3. `cargo xtask ingest --client _classic_`
4. **WoW Forever** (from 5 November):
   1. in `tests/ingame`: `wowlua install --client _forever_`
   2. in game: `/reload`, then `/stdtest engine graphics`, wait for
      "engine graphics: … lines", then `/reload`
   3. `cargo xtask ingest`

   wowlua: `crates/wowlua-engine/data/<flavor>-graphics.txt`; camelot's has
   only the shadow levels seen in Config.wtf files (unverified), the
   Classic clients' none. Recorded: (to do)
