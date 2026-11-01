# Changelog

All notable changes to wowlua. Each minor version closes a milestone of the [roadmap](spec/roadmap.md).

## Unreleased

### Fixed

- A debug session in real time keeps to the game's frame limit: at `maxFPS` 120 it runs at 120 frames a second, not about 110.

## 0.13.8 (2026-10-31)

### New

- **Units have places**: a unit's `world = { y, x, z }` and `facing` in `mock.spawnUnit`, `unit:update` and world profiles. When your character has a place, a unit's distance follows the two places: a unit given only a distance stands that far in front of you, `mock.moveTo` and a unit's new `world` change the distances your range checks see (`CheckInteractDistance`, `UnitInRange`, spell ranges), and `unit:setDistance` moves the unit. `UnitPosition` answers a unit's height too.
- **Your character moves**: the game's movement functions the key bindings call (`MoveForwardStart`, `TurnLeftStart`, `ToggleRun`, `ToggleAutoRun`, …) move and turn your character each frame at the game's speeds, telling `PLAYER_STARTED_MOVING` and `PLAYER_STOPPED_MOVING`; the units' distances follow. A debug session can hold a key down and let it go (`wowlua/input`'s `keyDown` and `keyUp`).
- A debug session can turn the 3D world on and off while it runs (`wowlua/showWorld`), and its input turns the camera and your character with the mouse (`look`), zooms (`zoom`) and targets the unit you click in the world (`clickWorld`); `wowlua/worldPoint` tells the place of the world under a point of the screen.
- **Units as NPCs**: a world profile's unit can be a vendor (its items, prices, limited counts, repairs), a class trainer, have gossip, or be a banker, mailbox, auctioneer or flight master; `attack`, `threat` and `loot` set how it fights and what it drops. Talking to a vendor opens its window directly, as in the game (`mock.defineVendor`), and in a debug session a right-click on a unit in the world talks to it.
- **Units in the 3D world**: with `--world`, your character and the units stand in the world as their models, walking, running or lying dead; a unit shows its data's `displayID` (or `npcID`'s creature).
- **Trees and props in the 3D world**: with `--world`, the zone's trees, rocks, fences and props stand where the game places them, outside and in the buildings.
- With `--world`, your character's model moves as it goes: it runs turned toward a strafe, walks backwards, and shuffles as it turns in place. While the right mouse button is held over the world, the turn keys strafe, as in the game.
- **Playing in a debug session**: the mouse and keyboard work as in the game at once (`wowlua/input`'s `mouseMove`, `mouseDown`, `mouseUp`, `wheel` and `text`): frames under the mouse are hovered, clicked, dragged and scrolled; over the world, the unit under the mouse is moused over (`mouseover`, its tooltip), clicked to target, right-clicked to talk to, a drag turns the camera and the wheel zooms; text goes into the focused edit box, with Backspace and Delete.
- Edit boxes, sliders and the other kinds of widgets the game lets take the mouse do so without `EnableMouse`, as in the game; before, only buttons did.
- **Names over units**: with `--world`, a unit's name floats over it in its reaction's color, an NPC's title under it (a profile unit's `"title"`), as the game's name settings say (`UnitNameInteractiveNPC`, `UnitNameHostleNPC`, …). Units get their nameplates from the game's nameplate settings (`nameplateShowEnemies`, `nameplateShowFriendlyNpcs`, `nameplateShowFriendlyPlayers`), and a unit with a nameplate shows no name.
- **A test world in Northshire**: the world profile `northshire` comes with wowlua (WoW Forever). Name it with `config({ profile = "northshire" })` or `test.profile`, and your character starts at Northshire's start among test NPCs: vendors of gear with the game's own items and stats, repairs, a trainer of each class, a banker, an auctioneer, a postmaster, a flight master, gossip, and things to fight.
- **Solid trees and props**: with `--world`, the zone's fences, posts, crates, trees and the props in buildings stop your character and the units, hold them up and block line of sight, by each model's own collision shape. Your character and the units are as wide as their models: you stop with your shoulder at a wall.
- **Solid buildings**: with `--world`, your character and the units stand on buildings' floors and are stopped by their walls, the camera stays in front of walls, and a spell at a target behind a wall or a hill is refused ("Target not in line of sight").
- **Buildings in the 3D world**: with `--world`, the zone's buildings (the abbey, inns, towers) stand on the terrain where the game places them.
- **The 3D world's terrain**: `wowlua screenshot --world` draws the zone's terrain from your WoW installation behind the UI, as far as the game's view distance reaches, and `wowlua test --world` stands your character and the units on its ground. Off by default.
- **The camera**: `CameraZoomIn`, `CameraZoomOut`, the `MoveView…` functions, `FlipCameraYaw` and the saved views move a camera behind your character, from the client's camera settings; `GetCameraZoom` tells its distance.
- **Nameplates follow their units**: a unit's nameplate shows while the camera sees it within 45 yards, sits where the unit is and moves with it (`NAME_PLATE_UNIT_ADDED` and `NAME_PLATE_UNIT_REMOVED` as it comes and goes).
- **Ranges and facing**: the action bars' hotkeys turn red as the target walks out of a spell's range, and a spell pressed out of range ("Out of range.") or, for one that needs it, at a target behind you is refused, as in the game.
- **Units act**: a unit's `actions` in a world profile or `mock.spawnUnit` make it attack you while in reach, cast a spell every few seconds or turn to you when you come near (`{ at = 0, ["do"] = "attack", damage = 50 }`, `{ every = 8, ["do"] = "cast", spell = 133 }`, `{ within = 10, ["do"] = "face" }`).
- **Units walk**: `unit:patrol(path)`, `unit:follow()`, `unit:approach()`, `unit:flee()`, `unit:moveTo(place)` and `unit:stop()`, or a unit's `movement` in a world profile, make it walk on its own, its distance to you following.
- A debug session tells what the game's chat windows show (`wowlua/chatLines`: each line's text and colour, your addon's prints too) and sends a chat message as the player would type it (`wowlua/input` with `chat`: your addons get its `CHAT_MSG_SAY`, `CHAT_MSG_PARTY`, …), for wowlua Studio's chat tab.
- **The game's graphics settings**: a debug session runs as fast as `maxFPS` lets it (`maxFPSBk` while wowlua Studio is in the background) and, with `vsync` on, at most the display's rate (your monitor's own in wowlua Studio), and `GetFramerate` answers the frames a second it really runs at. With `--world`, the 3D world is drawn at the render scale (`RenderScale`, 0.33 to 2) and stretched to the screen, the UI sharp over it.
- A debug session's `units` tell whether you can see each unit (`sight`) and where it faces (`facing`), and its scene tells which items stand over units (nameplates, names), for views of the world from other cameras.

### Changed

- **A faster 3D world**: frames with many units, names and nameplates draw many times faster (Northshire among its test NPCs: 50 ms to about 2 ms a frame), the units' models are posed on the graphics card, and a debug session's frames take far less time with a large UI: hidden frames cost nothing each frame, and what didn't change since the last frame isn't laid out again (SuuUI with Blizzard's UI: 13 ms to 1.2 ms a frame).
- In a debug session, each frame moves time as far as it really took: your character runs at its speed however fast the frames come. Before, every frame counted 1/60 s, so a slow frame rate slowed the game down.

### Fixed

- A creature the client's cache knows from a newer client now shows its model (its displays were missed).
- A world profile's units' `npcId` was ignored; it now names the creature, as `npcID` does in a test.
- `GetRealmName` and `GetNormalizedRealmName` always gave `Realm` in a test; they now give your character's realm from the world profile or `mock.setPlayer`, as `UnitFullName` and chat already did (`GetNormalizedRealmName` without spaces and hyphens).

## 0.13.7 (2026-10-30)

### New

- **World profiles from the game**: `/wl snapshot` records your character's world in a dev build (gear, bags, action bars, auras, target, settings), and `wowlua profile snapshot <name>` writes it as `tests/profiles/<name>.json`, without your character's name and realm.
- A world profile's `trained` lists the spells your character has trained, by ID: they are known in the test (`IsSpellKnown`, `IsPlayerSpell`), and `/wl snapshot` records your spellbook into it. `mock.player:learn` and `mock.player:unlearn` change them in a test. A profile's `spells` is only for changing a spell's data now.
- `unit:update(data)` changes a unit's data in place, any field `mock.spawnUnit` takes: its level, name, reaction, class, health, distance and so on. The unit stays the same unit, and each change fires the event the game fires for it (`UNIT_LEVEL`, `UNIT_FACTION`, `UNIT_NAME_UPDATE`, …).
- Completion offers the functions, classes, values and enums your other files export, and adds their `import` when you pick one.
- Editors can choose when the language server checks (while typing, when you pause, on save), which build profile its diagnostics follow, and how many threads it uses, in its initialization options or `workspace/didChangeConfiguration`.
- Each flavor has its max level (WoW Forever 60, Classic Era 60, TBC Anniversary 70, Retail 90): a world profile's character above the max level of your flavors is E4401, and `GetMaxLevelForLatestExpansion` answers it in tests.
- A world profile's unit can have `"level": -1`, a boss's "??" level: `UnitLevel` answers -1.
- The world profile schema describes `position.world` (`{ y, x, z, instanceID }`, where `UnitPosition` places your character) and the fields of `channels` (`joined` channels, `groupChannels`, `regionalService`), so editors complete them.
- `addon.toc-extra` documents the TOC directives you can add there (`AddonCompartmentFunc`, `AllowLoadGameType`, …) in its schema and the manifest reference.
- A function breakpoint can name where a function is defined: `src/Threat.wowlua:560` stops in every call of the function defined on that line, local functions too.
- `mock.setHardcore(true)` makes the test's realm a hardcore realm on `vanilla` (Classic Era): `C_GameRules.IsHardcoreActive()` answers true.
- **Every power of a class**: in tests a unit has all the powers its class has, not just one. A druid has mana, rage, energy and combo points, a rogue energy and combo points, a death knight runic power and runes, a monk energy, mana and chi. `UnitPower(unit, Enum.PowerType.ComboPoints)` and `UnitPowerMax` answer each one; without a type they answer the power the unit shows, as before.
- `mock.spawnUnit`, `mock.setPlayer`, `unit:update` and a world profile's units take `powers`, by token: `powers = { RAGE = { current = 30 }, COMBO_POINTS = { current = 2, max = 5 } }`. A changed power fires `UNIT_POWER_UPDATE` and `UNIT_MAXPOWER` with its token, and `unit:setPower(n, { powerType = "COMBO_POINTS" })` sets any of them. A key that isn't a power token is E4401 in a profile.
- Each flavor lists its classes with their powers and its races, so wowlua and wowlua Studio show the same ones (WoW Forever's nine classes and its races, the Skyborne among them; MoP Classic's Death Knights, Monks and Pandaren; Retail's thirteen classes).
- **Racial spells**: your character knows its race's racial spells in tests without listing them (`IsPlayerSpell`, `C_SpellBook.IsSpellKnown`), taken from the game's own data for WoW Forever, Classic Era, TBC Anniversary and MoP Classic, by class where they differ (an Undead's Touch of the Grave). WoW Forever's Skyborne know Walk on Air, Elemental Insight and Wind Blessed, plus Read Leyline as the Alliance's High Order Skyborne or Skysight as the Horde's Windshaper Skyborne.
- A world profile's character can only be a race and class the game pairs in your flavors (the game's own data): a Classic Era Orc priest or an Alliance Skyborne shaman is E4401. The flavor table lists each race's classes, so wowlua Studio offers only those.
- Each race in the flavor table has its faction (Alliance, Horde, or either for the Pandaren, Dracthyr, Earthen, Haranir and Skyborne), so wowlua Studio offers a faction choice only where the player picks one.

### Changed

- `test.screen.uiScale` and a world profile's `screen.uiScale` take the game's UI Scale slider's values, 0.65 to 1.15 in steps of 0.01: another value is E4002 in `wowlua.json` and E4401 in a profile.
- Coverage minimums in `test.coverage` are percentages from 0 to 100, and `stdlib.log.history-size` is at least 1: other values are E4002 (a history size of 0 made every log call fail).
- wowlua's flavors are camelot (WoW Forever), standard (Retail), vanilla (Classic Era), tbc (TBC Anniversary) and mists (MoP Classic). camelot is supported everywhere: tests, the emulated client and the debugger run it. The others are coming soon: wowlua checks and builds them, but doesn't run them yet.
- `mists` stays a flavor, now named MoP Classic: it is coming soon like Retail, Classic Era and TBC Anniversary, and a world profile's character on it goes up to level 90.
- `cata` and `wrath` (and the alias `titan`) are no flavors anymore: a `wowlua.json` naming one gets E4004. `wowlua init` reads no `_Cata` or `_Wrath` TOC, and a packager project's `--@version-wrath@`, `--@version-titan@` and `--@version-cata@` blocks are left out of every build.
- Watches and hovers in the debugger only read: they set nothing and call only functions that change nothing (the Lua library's, like `string.format`, and the game's queries, like `UnitHealth` or `frame:GetWidth()`). Anything else says so instead of running; the debug console still runs any code.
- A unit with a class shows its class's power unless you give `powerType`: a spawned warrior shows rage and a rogue energy (mana before), each at its default (rage 0 of 100, energy 100 of 100, combo points 0 of 5). Changing a unit's class with `unit:update` gives it the new class's powers and fires `UNIT_DISPLAYPOWER`.
- Mana starts full: a character's mana is its class's base mana for its level in the game's data (a level 1 mage 100, a priest 110 on WoW Forever), and a unit without one has 100 of 100 instead of 0 of 100.
- `UnitHasPowerType` is true for any power the unit has, and an item's mana only changes the mana of a character that has it.
- A character's or unit's faction follows its race: `UnitFactionGroup` of an Orc is "Horde" even if `faction` says otherwise, and a world profile giving a race another faction is E4401. `faction` is for races of either faction (a Skyborne's `UnitRace` then answers High Order Skyborne or Windshaper Skyborne) and for creatures. A player given only `faction = "Horde"` is an Orc instead of a Human.
- `wowlua test --profile-out` and the MCP server's `profile_stop` keep the whole profile in the speedscope file: each function's calls, the events and scripts, and the call trees of the last 600 game frames. speedscope still opens it.

### Fixed

- A world profile's unit with `"nameplate": true` no longer stops Blizzard's nameplates with an error (`bad argument #2 to 'SetSize'`): its nameplate appears once your character enters the world, as in the game.
- camelot's client is found in `_forever_`, where WoW Forever installs it: `wowlua install`, `wowlua dev`, the game's art in screenshots and the game data read from the client work with it again (`_classic_beta_` still works for a beta client).
- On `standard`, `mists`, `tbc` and `vanilla`, functions the game client has but Blizzard does not document exist, as on `camelot`: `GetQuestLogTitle` on Classic Era, `GetGlyphSocketInfo` on Mists, `GetReadyCheckStatus` everywhere. Without a behaviour they raise "not mocked", so a test can mock them; before they were nil, even though the type checker knew them. A function only some clients have still exists only on theirs.
- `C_ActionBar.GetBonusBarOffset`, `C_ActionBar.HasOverrideActionBar`, `C_ActionBar.HasVehicleActionBar`, `C_PetBattles.IsInBattle`, `IsTargetLoose`, `UnitIsMinion`, `UnitNameplateShowsWidgetsOnly`, `UnitIsTapDenied`, `GetRaidTargetIndex`, `GetNumArenaOpponentSpecs`, `GetArenaOpponentSpec`, `GetReadyCheckStatus`, `C_TTSSettings.GetSpeechRate` and `C_TTSSettings.GetSpeechVolume` answer on every flavor as the client does with nothing there, instead of "not mocked" or nil. `GetExpansionLevel`, `GetServerExpansionLevel`, `GetClassicExpansionLevel`, `GetAccountExpansionLevel`, `GetMaximumExpansionLevel` and `GetMinimumExpansionLevel` answer what each client does (the server's expansion only after login).
- `C_ClassColor.GetClassColor` answers each class's colour on every flavor, as the game does (Classic Era's Shaman in pink), and nothing for a class the client doesn't have.
- On `mists`, `tbc` and `vanilla`, `GetTalentGroupRole` raises the client's "API unsupported in this version of World of Warcraft." error, as do `GetNumTalentTabs`, `GetNumTalentGroups` and `GetUnspentTalentPoints` on `mists` and `GetNumSpecGroups` and `GetTalentTierInfo` on `tbc` and `vanilla`. `GetRuneType` answers each rune's type as the game does (1, 1, 3, 3, 2, 2), and nothing for another rune.
- `Kiosk.GetCharacterTemplateSetIndex` exists on `standard`, `mists`, `tbc` and `vanilla` (as on `camelot`), and `C_SocialRestrictions.IsAgeVerificationRestricted` on `standard`.
- On `mists`, `tbc` and `vanilla`, `C_CombatLog.GetCurrentEventInfo()` answers the payload of the last `COMBAT_LOG_EVENT_UNFILTERED` a test fired, `C_GameRules.IsHardcoreActive()` answers false unless the test set a hardcore realm, and `GetWeaponEnchantInfo()` answers no enchant with the client's number of values (8 on Mists, 12 on TBC and Classic Era).

## 0.13.6 (2026-10-28)

### Fixed

- W4337 also warns about a `build.load-order` entry that differs from its file only in case (`src/Core.lua` for `src/core.lua`), and names the file's spelling: the build matches your own files by their case, so such an entry ordered nothing.
- `compat.localization` fills `@localization(…)@` lines without `compat.packager-keywords` too; before, it did nothing unless that was on.
- A nameplate shown with `unit:showNameplate()` is anchored to `WorldFrame`, its parent, instead of `UIParent`.

## 0.13.5 (2026-10-28)

### New

- **Shields and heal prediction**: `unit:setAbsorb`, `unit:setHealAbsorb` and `unit:setIncomingHeal` put absorb shields, heal absorbs and incoming heals on a unit in tests. Damage uses up a shield before health, and healing uses up a heal absorb. `UnitGetTotalAbsorbs`, `UnitGetIncomingHeals` and a heal prediction calculator (`CreateUnitHealPredictionCalculator`) answer them, so shield and heal prediction bars can be tested.
- An adopted addon's `@localization(…)@` lines can be filled from its own locale files: `"compat": { "localization": "locales/{locale}.lua" }` puts each locale's file in place of its keyword, and a locale without a file stays a comment. Before, the keyword was always W4030.
- `mock.unitAt(index)` is the unit at that place of the world's units (0 the player), for a test that goes through the units a world profile brings.

### Fixed

- A secure group header (`SecureGroupHeaderTemplate`) runs its `initialConfigFunction` for buttons made from your own template that has an `<OnLoad>`, as the game does; before, the header skipped it.
- `GetTotemInfo` answers an empty slot for every slot `GetNumTotemSlots()` counts (8 on WoW Forever), not just the first four; slots 5 to 8 answered nothing.
- An aura button's `SetDurationText(fontString, {})` keeps its default text: options you leave out, such as `textColor`, stay out, as in the game; before, Blizzard's code errored with "a color curve expected".
- A group member moved in or out of range with `unit:setDistance` fires `UNIT_IN_RANGE_UPDATE`, so unit frames that fade out-of-range members (Blizzard's raid frames among them) react in tests; before, only `UnitInRange` changed.
- A status bar given `SetTimerDuration` fills (or drains) with its duration, in `GetValue` and in screenshots, so cast bars that run on a timer show their progress, with or without a range set first; before, the bar stayed where it was.
- Nameplates shown with `unit:showNameplate()` have a size and a place on the screen, so what nameplate addons build on them shows in screenshots; before, nothing on them was drawn.
- E5001 for a file whose main chunk is over Lua's 200 locals with the locals the build adds (`__wl` in a plain Lua file's dev and test builds) names them, and says which profile settings leave them out; before, it told you to split your own code.
- `wowlua mcp` and `wowlua debug` in a package that doesn't load (a dependency that can't be installed, say) show its errors, as `wowlua build` does; before, they said no `wowlua.json` was found.
- W4337 warns about a `build.load-order` entry that names no file of the build (a typo, or a file outside `build.src`), which the build leaves out; before, it was dropped without a word.
- A frame anchored to a status bar's texture (`bar:GetStatusBarTexture()`), such as a heal prediction or heal absorb bar at the health fill's edge, is placed and drawn, and follows the bar's value.
- `WorldFrame` covers the screen as in the game: 768 units high at any resolution and UI scale, its width following the screen's aspect ratio. Before, it had no size, and frames anchored to it weren't drawn.
- Pausing a running addon session in VS Code stops it: the pause button was answered and did nothing until a breakpoint hit.

## 0.13.4 (2026-10-28)

### New

- A world profile can set the screen the client draws on: `"screen": { "width": 2560, "height": 1440, "uiScale": 0.8 }` in `tests/profiles/<name>.json`, over `test.screen` in `wowlua.json`.

### Changed

- The game's settings change the screen while it runs, as in game: the window size (`C_VideoOptions.SetGameWindowSize`) and "Use UI Scale" with its slider (the `useUiScale` and `uiScale` CVars) resize UIParent, and your addon gets `DISPLAY_SIZE_CHANGED` and `UI_SCALE_CHANGED`.

## 0.13.3 (2026-10-28)

### Fixed

- The reference count above an exported function or class method showed twice.

## 0.13.2 (2026-10-27)

### New

- **The registry's new pages** (https://registry.wowlua.dev), in wowlua.dev's design: search with filters by kind and flavor, a page per package with its install command, a usage example, its README, versions, dependencies and the packages that depend on it, and the account and reserved-name pages. A reserved name's page links a form to claim it.
- `wowlua new` takes `--check strict` (or `basic`, `nocheck`) for the check mode, `--world-profile` for a starter world profile in `tests/profiles/default.json` that every test starts in, and `--no-git` to skip the Git repository.
- `wowlua init --no-pkgmeta` adopts an addon from its TOC files only, leaving its `.pkgmeta` as it is.
- **Editor colours, folds and hints**: the language server colours names by what they are (types, functions, parameters, fields, decorators, and `class`, `import` or `type` only where they are keywords), folds functions, blocks, tables, comments and imports, shows the types of locals declared without one, offers a warning's fixes as quick fixes (`Ctrl`/`Cmd` + `.`), and counts each function's references above it. VS Code shows all of them. Editors also learn when a check begins and ends (`wowlua/checking`).

### Changed

- Hovering a function with overloads, like `CreateFrame`, shows each overload on a line of its own instead of one long type joined by `&`.

## 0.13.1 (2026-10-27)

### New

- `"check": { "secret-widget-results": true }` types every widget getter Blizzard documents as possibly secret (`GetText`, `GetWidth`, `IsShown`, `GetPoint`, …) as `T | Secret<T>`, as 0.13.0 did for everyone.

### Changed

- Widget getters are typed plainly again (`GetWidth()` is a `number`): in 0.13.0 every one Blizzard documents as possibly secret was `T | Secret<T>`, which turned whole addons red although only widgets a setter gave a secret return one. `wowlua test` still returns secrets where the game does.

### Fixed

- The install scripts find the latest release without GitHub's API, which refuses requests without a token after 60 an hour (`curl: (56) … error: 403`, then a download of `wowlua--<target>`). Without a version found they stop and ask for `WOWLUA_VERSION`.
- `mock.partial` fakes pass runtime checks: a library's tests no longer fail with `bad argument … expected, got table` when a fake reaches one of its exported functions.
- `wowlua fmt` reports `Finished 11 of 12 files changed` instead of `11 of 12 12 files changed`.
- A closure sees what a local holds where the closure is made again: after `local f = self.frame` and `if not f then f = make() end`, `f:Show()` in a callback is no longer E2005.
- Empty tables in a value assigned to a local take the local's element types: `cards = { { title = "…", lines = {} } }` for a `{ title: string, lines: string[] }[]` is no longer E2001.

## 0.13.0 (2026-10-27)

### New

- **wowlua registry**: depend on libraries by version (`"lib-tooltip": "^1.2"`, or `wowlua add lib-tooltip@^1.2`), from https://registry.wowlua.dev or another registry. Versions are locked with their checksums, `wowlua update` and `wowlua outdated` know them, and a yanked version your lockfile has keeps working with W4322.
- `wowlua publish` publishes `library`, `lua-library` and `types` packages to the registry, and `wowlua package --registry` writes the archive without uploading it. Sign in with GitHub at https://registry.wowlua.dev/login for a token, then save it with `wowlua login registry` or set `WOWLUA_REGISTRY_TOKEN`.
- `wowlua search <words>` finds registry packages by name, description and keywords.
- `wowlua yank <name>@<version>` yanks a version you published (`--undo` takes it back), and `wowlua owner list|add|remove` manages a package's owners.
- **Faster release builds**: release builds reshape the Lua they write for WoW's Lua 5.1. Fields such as `self.config.scale` are read once before a loop that can't change them, and once for repeated reads; `ipairs` over arrays becomes a numeric loop and `table.insert(t, v)` becomes `t[#t + 1] = v`; outer locals read in loops are copied into locals; closures made in a loop that need nothing from it are made once; locals holding a constant become the constant. The type-based ones apply in strict-mode files only, plain `.lua` stays as it is, and each has a switch under `optimize`.
- **`wowlua bench`**: `@bench` functions in test files are benchmarks. `wowlua bench` runs them optimized and unoptimized and shows both times and the speed-up (`--profile`, `--samples`, `--batch-ms`, `--json`).
- **Performance report**: `wowlua prof report` turns the recordings you marked for export in the dev window into one HTML page: hints, functions, time by event, call tree and flame graph, the frame timeline against the frame budget, slow calls with their stacks mapped to your source, and garbage. It finds them in your WoW installation's SavedVariables, asks which account or client when several have some (`--account`, `--client`), and opens the page in your browser (`--no-open`, `--out`, `--recording`).
- **Performance tab**: recordings of every combat and boss pull, `/wl record`, eight views (functions, tree, flame, by event, timeline, slow, garbage, bench) and comparing recordings per minute.
- **Hints**: P7101, P7102, P7201 and P7301 in the dev window and in `/wl hints`, explained by `wowlua explain P7301`.
- `wowlua explain` explains every error and warning: what triggers it, how to fix it, an example that reports it and one that compiles, and related codes. The same explanations are on each code's page on wowlua.dev.
- Every WoW event and many more API functions now have a description, which the editor shows on hover and wowlua.dev's new WoW API reference shows on each entry's page with how far `wowlua test` emulates it.
- W8301 warns when a standard-library function does less on a flavor you build for: Item, Spell and Unit tooltip hooks on the classic flavors, `launcher.register` without the Addon Compartment.
- E2053, E2054, E2056, E2058 and E2059 are reported: recursive aliases, conflicting merged interface fields, misplaced type packs, an event and a message with the same name, defaults in event and message parameters.
- In strict mode a misspelled field of a table you fill in (`stats.deths`) or of a `setmetatable` object's methods is E2003.
- `implements` checks the class where it is declared (E3006).
- Decorators are type-checked: their arguments (E2001, and E1003 for undefined names), whether the decorator accepts the declaration (E3022), and a decorator that returns a function gives the function its result type.
- `@script` checks the script name and the handler's parameters against the frame type.
- W3021 when a built-in decorator's name (`@profile`, …) hides a function of yours.
- In optimized builds, `@inline` private methods are inlined where their class calls them, like `@inline` local functions.
- In `--!strict` files, backtick strings skip `tostring` for strings and numbers, `??` compiles to `or`, and `?.` compiles to `and`/`or` where the types allow it.
- `wowlua check --watch` checks again after every change.
- `wowlua types sync` moves wowlua.lock's interface numbers to your wowlua's flavor table and warns (W4313) when a client alias such as `anniversary` now names another flavor.
- `-v` shows the git and svn commands wowlua runs and the requests `wowlua publish` makes.
- `--features a,b` and `--no-default-features` choose a build's features, and `wowlua add --no-default-features` turns a dependency's defaults off.
- `wowlua test` runs a library's tests, and `wowlua check` checks `lua-library` and `types` packages.
- `@wowlua/web`: the worker has an `advance` call, `play(null, onOutput)` runs the game without drawing it, and `check()` diagnostics carry `text`, the diagnostic as `wowlua check` prints it.
- `wowlua data pull` reads your WoW installation's game data into wowlua's cache, so `wowlua test --game-data` works without the installation.
- `wowlua build --watch` builds again after every change.
- `wowlua build --reproducible` takes `build.time` from `SOURCE_DATE_EPOCH` or the commit time instead of the clock.
- The HUD (`/wl hud`), the error toast, `/wl bench`, and exporting recordings for `wowlua prof report`.
- The dev window shows the addon's own garbage rate next to all addons'.
- Profile settings `profile-call-paths`, `profile-garbage`, `frame-budget` and `slow-call`.
- The monitor also times slash commands.
- `optimize.inline` also inlines functions and `@inline` private methods whose body is statements, where they're called as a statement.
- E4231: a package for the registry depends on a `git`, `svn`, `path` or `archive` source.

### Changed

- With your WoW installation, atlases (`C_Texture.GetAtlasInfo`, `C_Texture.GetAtlasExists`, `SetAtlas` sizes) come from your client's game tables.
- With your WoW installation, trainer services' tooltips, descriptions and icons come from your client's spell data, and spell tooltips show combo points, melee and minimum ranges, passive, next-swing and channeled spells, and the forms, weapons, tools and reagents a spell needs.
- With your WoW installation, profession recipes (`C_TradeSkillUI.GetRecipeSchematic`, `GetRecipeInfo`, `GetRecipeRequirements`, `GetCategoryInfo`) come from your client's game tables; what the character knows stays as recorded.
- With your WoW installation, item class and subclass names (`C_Item.GetItemClassInfo`, `C_Item.GetItemSubClassInfo`) come from your client's game tables.
- With your WoW installation, achievements' names, descriptions, points, icons, categories and series come from your client's game tables (`GetAchievementInfo`, `GetCategoryInfo`, `GetCategoryList`, …); whether the character earned them stays as recorded.
- With your WoW installation, the world map's data (`C_Map.GetMapInfo`, `GetMapChildrenInfo`, `GetMapRectOnMap`, `GetMapWorldSize`, the map art and its tiles) comes from your client's game tables.
- With your WoW installation, the world map's flight paths (`C_TaxiMap.GetTaxiNodesForMap`) come from your client's game tables.
- With your WoW installation, every transmog source and appearance answers from your client's game tables (`C_TransmogCollection.GetAppearanceSources`, `GetSourceInfo`, `GetAllAppearanceSources`, …), not only those wowlua recorded.
- With your WoW installation (`wowlua test --game-data`, `wowlua data pull`, the playground), camelot's talent trees come from your client's game tables: nodes, entries, connections, costs and "Spend N more points" requirements match the build you play, and such a requirement counts only the points spent in its own tab.
- `if flavor == …` branches count for E8300, E8310 and E8330: a call is checked only for the flavors that run it.
- The router's help and `Usage:` lines start with the slash command (`/mya scale <number>`).
- Using a possibly secret value as a table key is E2040 (W2040 in basic mode).
- In strict mode a table returned from a function is sealed: adding a new field to it is E2051 and reading an unknown one is E2003.
- Extending an interface, a type alias, an enum or a plain table is E3008.
- Two types with one name in one scope (an interface, a type alias, a class or an enum) are E1031; before, only two interfaces were reported, and otherwise the later one silently replaced the earlier.
- `ns.__wowlua = …` is E2003.
- `@inline` on a function that isn't local gives W3031.
- Widget getters Blizzard documents as possibly secret (`GetText`, `IsShown`, `GetWidth`, `GetAlpha`, `GetPoint`, …) are typed `T | Secret<T>`: check them with `issecretvalue` or `canaccessvalue` before computing with them. `mock.textOf` returns `string | Secret<string>`.
- `wowlua check` and dev builds report E3032 when a `@debugOnly` function is used as a value, as release builds already did; uses in code only debug builds keep are allowed.
- An XML file loading a file that `build.exclude` or a profile's `exclude` leaves out warns W5004 instead of failing with E5002.
- `wowlua new --ci github` writes a workflow that checks, tests and builds every push and publishes tags.
- `flavors.<id>.types` must accept the API version wowlua ships (E4318).
- `frames.movable` with `editMode` warns W8301 when you build for Cata Classic, which has no Edit Mode.
- Shared flavor mode checks members of Blizzard's utility tables per flavor: `ChatFrameUtil.SendTell` without a guard is E2030 when you build for cata.
- The dev window uses the wowlua design system and its fonts.
- `@event` handlers and the standard library's timers show in the monitor.
- A local declared with a value keeps that value's type: after `local count = 0`, `count = "none"` is E2001 (W2001 in a plain Lua file), as with `local count: number = 0`. Code that reuses a local for another type now reports E2001: annotate it (`local count: number | string = 0`) or use a second local.
- In strict mode `nil` no longer fits a local declared with a non-optional value: write `local timer: TimerHandle? = …` for one you clear later.
- `pcall` and `xpcall` results are typed as the game returns them: test the first result (`if ok then`) before using the others; until then the second value may be the error (`unknown`).
- W3031 now names the new reasons a function isn't inlined.
- `@inline` on a private method with a statement body is no longer W3031.
- `wowlua init` turns the new optimizations off in an adopted addon's release profile.
- Registry dependencies no longer stop with E4102 "not available yet"; E4102 now reports a package or version the registry doesn't have.
- `wowlua add <name>` without a source adds a registry dependency with `^` and its newest version.
- `wowlua update <name> --precise <version>` pins a registry dependency to a version.
- Publishing a library no longer needs an up-to-date `wowlua.lock`.

### Fixed

- `C_Texture.GetAtlasInfo` finds the atlases whose names contain a space (`Raid Frame`, `shop-virtual currency-menu-bg`).
- Function descriptions taken from old-style warcraft.wiki.gg pages no longer include the page's raw markup (`----`, `;''Returns''`).
- The router checks the types of `run`'s parameters against `args` and `flags` (E8112).
- `L.KEY({...})` reports a missing or unknown placeholder.
- `L["1ST"]` works for locale keys that start with a digit.
- The minimap button of `wowlua:launcher` follows profile switches.
- `print.to` accepts `ChatFrame3`, `chat.window(...)` and scrolling message frames.
- `local f = log.warn` and `Log.debug` are E8118, and `debug.assert` used as a value is E3045.
- W8430 finds yields inside metamethods.
- E8340 rejects store fields that aren't a `FramePosition?`.
- Two `launcher.register` calls in an if/else are no longer E8080.
- Assigning to a `const` reports one E2010 instead of two errors and an E2001.
- `interface` and `type` declared inside a function or `do` block can be used there.
- `setmetatable(o, C)` with `C.__index = C` gives `o` the methods of `C`, and their calls are checked.
- Comparing a possibly secret value in basic mode is only the warning W2040.
- Calls of a global function are checked against its `overload` signatures.
- `if r.maybe then` on a record no longer reports E2003 in strict mode.
- A misspelled method says "has no method", and messages name `self` in `function T:m()` as `T`.
- E4012 and E0101 (wowlua syntax in a `.lua` file) read as sentences.
- Assigning a private or protected field, or using a private static member, from outside the class is E2011.
- A subclass's inherited constructor no longer depends on file order (E2006 "expected 0").
- In a decorated function, recursive calls are checked against the decorator's result type.
- `wowlua:format`'s `duration`, `short`, `clock`, `money` and `date` work in `wowlua test` without `test.blizzard-ui`: the emulated client has the game UI's `SecondsToTime`, `SecondsToTimeAbbrev`, `SecondsToClock`, `GetMoneyString`, `FormatShortDate`, `FormatLargeNumber` and `COPPER_PER_SILVER`.
- `format.short` returns the time ("2 m") instead of a format string ("%d m").
- `SendChatMessage`, `DoEmote` and `CancelEmote` work in `wowlua test` without `test.blizzard-ui`.
- `NUM_BAG_SLOTS` and `NUM_TOTAL_EQUIPPED_BAG_SLOTS` are defined in camelot tests without `test.blizzard-ui`.
- A status bar's `OnValueChanged` and `OnMinMaxChanged` get a secret after a secret `SetValue` or `SetMinMaxValues`, and `HasSecretAspect(Enum.SecretAspect.BarValue)` reports it; their handler is `(self, value)`.
- `C_Container.GetContainerItemInfo` returns `nil` for an item without item data, and such an item in a world profile no longer makes Blizzard's UI raise an error.
- `mock.fireEvent` rejects a wrongly typed enumeration value (`mock.fireEvent("BAG_UPDATE", "x")`).
- `FormatShortDate`'s `year` is optional.
- Hover and completion show Blizzard's own descriptions where warcraft.wiki.gg has none.
- The example addons' tests pass on every flavor they list.
- Code removed by `@if` no longer warns W1001 about names of an import that `@if` also removes.
- A missing module reports one E1015, without the extra "imports need the package context" error, and its imported names no longer warn W1001.
- With `line-mode: "pretty"` or `"statement"`, trailing comments stay after the code of their line.
- A publish target that minifies differently from its profile (`minify: "max"`) gets its own build id, so `wowlua trace` decodes its errors with the right maps.
- `wowlua dev` started in the package folder no longer prints an empty folder name.
- `publish --dry-run` without a CurseForge token no longer contacts CurseForge or adds E4202.
- A new library package no longer warns W1003.
- A misspelled `--target` is E4006 instead of doing nothing.
- Path dependencies work in the browser playground.
- An asset a profile excludes or another flavor owns is E4307 where it is used, not a misleading E1038 or a missing texture.
- `@wowlua/web`: `check`, `build`, `run` and `format` read what `writeFile` wrote, also while the language server has the file open, and the language server checks again when the page writes, deletes or replaces files.
- `@wowlua/web`: the manifest's `check.lints` apply as in `wowlua check`: W2063 is off until set to `"warn"`, `"allow"` drops a warning and a `"deny"` lint stops a build or a run.
- `@wowlua/web`: a run through the worker without a canvas moves, and what it prints reaches `onOutput`.
- A file starting with `--!strict` (or another directive) no longer gets that directive as a comment after the generated first line.
- `unit.healthPercent` no longer answers 0 to 1 on a client with `UnitHealthPercent` but without `CurveConstants`.
- `frames.movable` with `editMode` no longer fails on Cata Classic; the option is ignored there.
- Tooltip hooks (`tooltip.hook`, `@onTooltip`) run on the classic flavors again, through the tooltips' scripts.
- `STANDARD_TEXT_FONT`, `GENERAL_CHAT_DOCK`, `LE_PARTY_CATEGORY_INSTANCE`, `LE_EXPANSION_LEVEL_CURRENT` and other client constants are declared for every flavor that has them.
- Messages show record keys that aren't identifiers as written: `{ ["1ST"]: string }`.
- `wowlua explain W2001` (and every plain-Lua warning form) no longer reports "unknown diagnostic code".
- `wowlua build --watch`, `check --watch` and `test --watch` no longer run twice for one save when the file watcher reports it twice under load.
- In strict mode, the fast forms (`??` as `or`, `?.` as `and`, interpolation without `tostring`) now also apply inside class methods, constructors and accessors.
- `wowlua publish` runs your tests before packaging, and stops when one fails; `--no-test` skips them. A `--tag` is created only once they pass.
- `wowlua build --watch`, `check --watch` and `test --watch` on Linux no longer run twice for each change: reading a file is not a change.
- `wowlua test` with `test.blizzard-ui` needs a flavor's UI code only when that flavor has test files to run.

## 0.12.0 (2026-10-26)

### New

- **wowlua in the browser**: wowlua's checker, language server, emulated client and debugger now run in a web page, the base for the playground on wowlua.dev. A package can be edited, checked, built and shared as a link, run against Blizzard's default UI and debugged with breakpoints, with the game's art from your own installation read in the browser and never uploaded. Each release has the package attached (`wowlua-web-<version>.tgz`).
- World profiles are checked with your package: `wowlua check`, the editor and `wowlua test` report an unknown part (a typo the world ignored before), a wrong type, and a `test.profile` or `config({ profile = "…" })` that names no profile (E4400, E4401, E4402). VS Code completes a profile's parts and fields from its schema (`schema/world-profile-0.1.json`).
- While debugging an addon session, `wowlua/world` (and the MCP tool `world`) changes the running world with the same controls tests use, such as `mock.player:setMoney(500)` or `mock.player:enterCombat()`.

### Fixed

- `Settings.OpenToCategory` opens Blizzard's settings panel in tests and the emulator (an addon's `/command` that shows its options, such as AdvancedInterfaceOptions' `/aio`); it did nothing.
- In tests, `loadstring` of code past Lua's limits gives the game's error message: "more than 199 variables in assignment" (not 200), an upvalue too many reported on the line the game reports, and "function or expression too complex near '…'" with the token it is near.

## 0.11.0 (2026-10-25)

### New

- **Find All References**: Shift+F12 (or *Go to References*) in VS Code lists every place in your workspace that uses a name: a local, a function, a class, a method or field (`self.count` too), a type, an import, a global; in every file, open or not, in your tests, and in the packages of your workspace that depend on it.
- **Rename Symbol**: F2 renames a name everywhere it is used, across files and the packages of your workspace, methods called with `:` and `self.` fields included; an `import a as b` keeps its alias. When a rename can't be done safely (the game's API or Lua's functions, an override or interface that would no longer match, a name already taken, a name an XML file uses), wowlua says why and changes nothing.
- When wowlua can't load a package (a broken `wowlua.json`, or a `"wowlua"` version this wowlua doesn't satisfy), the editor now shows an error saying so, such as "suuui can't be checked: this package requires wowlua ^0.9, but this is wowlua 0.10.1 (wowlua.json)", instead of silently checking nothing.
- **Spells from your WoW installation**: with your installation at hand (`wowlua test --game-data`, debug sessions, `wowlua screenshot`), a spell your test doesn't define is the game's: `C_Spell.GetSpellInfo` gives its name, rank, icon, cast time and range, `C_Spell.GetSpellPowerCost` its costs, and `C_Spell.GetSpellDescription` its text with the numbers for your test's character ("Burns the enemy for 7 to 9 Arcane damage and then an additional 12 Arcane damage over 9 sec."), in every flavor, and its tooltip shows its cost, range, cast time, cooldown and description. Spells and texts the server changed are read from the game's hotfix cache, as the game shows them, and the installation's current data wins over wowlua's recordings of older builds, for items too.
- With your WoW installation at hand, text in fonts and characters wowlua's recordings don't cover (accented letters, other alphabets, the game's other fonts) is measured with the game's own font files, so `GetStringWidth`, wrapping and truncation come close to the game's instead of treating each such character as a `?`.

### Fixed

- Screenshots and the UI panel show the spellbook's pages: a window raised as it opens (`frame:Raise()`) now raises its panes with it, as `SetFrameLevel` does, where the spellbook's pages were drawn under its background.
- A tooltip line added to wrap (`GameTooltip:AddLine(text, r, g, b, true)`, spell descriptions) wraps at the tooltip's width instead of stretching the tooltip across the screen.
- *Go to Definition* on a module's own function, class or enum, used in that module, now finds it, and leads into a library of your workspace at its real file instead of a copy.
- In the editor, a package of your workspace that uses a library or addon of the workspace now sees that library's unsaved edits, and saving a file of a workspace member checks the members that depend on it again.

## 0.10.1 (2026-10-25)

### Fixed

- A release build of a workspace on a tag no longer stops with E4319 for members of another version: the tag is the root package's, and the members (example plugins, libraries) are built as untagged builds (`0.1.0-alpha+<hash>`). The root package's version must still match the tag.

## 0.10.0 (2026-10-25)

### New

- **Debugger**: `wowlua debug` lets VS Code (or any editor that speaks the Debug Adapter Protocol) debug a test file or your addon running in the emulated client: line breakpoints with conditions, hit counts and log messages; breakpoints on functions, API calls (`C_Timer.After`), events (`event:PLAYER_LOGIN`), scripts (`script:MyFrame:OnClick`) and timers; stop on errors, on taint or on a blocked protected action. Step into event handlers, `OnUpdate` and timers, and out of one handler into the next. The call stack shows which event or script ran your code, variables show their taint and secret values what they hide, widgets their size, points and children, and watches can change locals. VS Code asks which flavor to debug when your launch configuration names none and the package has several, and the Debug Console says which flavor runs and shows chat lines as the chat frame does (no colour codes, "2 days" for `|4day:days;`). Debugging the addon itself (`"addon": true`), type slash commands in the Debug Console as in chat (`/wdemo`), and `/reload` loads the UI again with your SavedVariables, your breakpoints still set. A launch configuration's `"profile"` starts the session in that world profile (your character, its gear and bags). Errors in event and script handlers stop the debugger in tests too (they reach the error handler), a stop in Blizzard's UI code or another addon's opens that file at the line, and stopping the debugger ends the session at once.
- **VS Code**: the wowlua extension (`editors/vscode`) adds the `wowlua` debug type, so VS Code runs `wowlua debug` for you: breakpoints in `.wowlua` and `.lua` files, and launch configurations for the addon or a test file.
- **VS Code: the game's screen**: while you debug, the wowlua extension shows the UI beside your code, drawn as the game draws it with your installation's textures and fonts. It redraws at every breakpoint and step and about once a second while the addon runs, and a click in the picture is a click in the game, so your `OnClick` breakpoints stop. Zoom in with Ctrl or Cmd and the wheel (or a pinch), or pick a frame by name to zoom to it; zoomed in, the picture is drawn sharper. The new **wowlua Frames** view in the debug sidebar lists the frames on screen; select one to outline it in the picture, or zoom to it. Open the panel with *wowlua: Show UI*; `wowlua.ui.autoOpen` and `wowlua.ui.refreshInterval` set when it opens and how often it redraws.
- **Screenshots**: `expect(frame).toMatchScreenshot()` (or `expect(mock.screen)`) compares what the player would see with an image stored in `__screenshots__` next to the test; a missing one is written, `wowlua test -u` updates it, and a mismatch writes the new image and a diff beside it. `mock.screenshot()` and `wowlua screenshot tests/x.test.wowlua -o x.png` write images too. Drawn with your own WoW installation's textures and fonts when there is one (for camelot, retail and every classic client), never copied anywhere.
- **3D models in screenshots**: model frames and model scenes show their models, read from your WoW installation: `SetDisplayInfo`, `SetCreature`, `SetModel` with a file ID, and an actor's `SetModelByCreatureDisplayID` and `SetModelByFileID`, posed in their animation, turned by `SetFacing`, zoomed by `SetPortraitZoom`, placed by the scene's camera. Characters too (`SetUnit`, `SetModelByUnit`, `TryOn`): race, skin, face and hair, and what they wear, so the default UI's character sheet shows your character; humanoid NPCs wear their gear. With an installation at hand, `C_ModelInfo` answers from the client's model scene tables, so Blizzard's model scenes place their models: the character sheet, the dressing room (`DressUpItemLink`) and the wardrobe show your character. `SetPortraitTexture` draws the unit's head in 3D, as unit frames show it in game. `SetCreature` finds creatures you have met (the client's creature cache), `SetItem` shows an item's own model, and models show their particles (with their tails, falling and changing texture cells over their life), glows, reflections, trails behind moving parts (ribbons) and parts that always face the camera.
- **Minimap in screenshots**: the minimap shows the land around the player from your WoW installation's map tiles, at the minimap's zoom, through its mask, with the player's arrow in the middle. It follows the player: `mock.moveTo({ mapID = 1429, x = 0.42, y = 0.65 })` and `Minimap:SetZoom(2)` show Goldshire close up. `mock.moveTo({ facing = math.pi / 2 })` turns the player west (`GetPlayerFacing`): the arrow turns, or with the rotating minimap (`rotateMinimap`) the map and the compass around it do, as in game. Group members you place with `mock.spawnUnit({ …, world = { y, x } })` show as their class's dots, and `Minimap:PingLocation(x, y)` shows a ping (`GetPingPosition`). As in WoW Forever's client, `MINIMAP_PING` reaches only event callbacks (`frame:RegisterEventCallback`); `RegisterEvent` refuses it. Indoors the minimap stays empty for now.
- **Profiler**: `wowlua debug` and `wowlua mcp` profile calls, instructions run, memory allocated and time per function, event, script, `OnUpdate` and game frame, and write speedscope files (speedscope.app opens them); `wowlua test --profile-out prof` writes one for each test file.
- **For AI agents**: `wowlua mcp` is an MCP server: an agent can start your addon in the emulated client, look at it in screenshots, find frames, click, type, run slash commands, move time on, set breakpoints, step, read variables and profile it.
- **Language server**: `wowlua lsp` checks your addon as you type, with the edits you haven't saved, and the VS Code extension uses it: errors and warnings show in the Problems view a moment after you stop typing (and at once when you save), *Format Document* formats as `wowlua fmt` does with your `fmt` settings, and the outline and breadcrumbs list a file's functions, classes with their methods, locals, enums and types, hovering a name shows its type (a variable's as inferred where you read it, a function's parameters and results) with its `---` doc comment, *Go to Definition* (F12, or Ctrl or Cmd + click) takes you from a name to where it is defined: a local, an imported function or class in its module, a method or field in its class (also inherited ones), a type, a global function of your addon, or the game's API (`C_Timer.After` opens its declaration), and completion lists what you can write as you type: the locals in scope, your addon's and the game's globals (`GetSp` offers `GetSpellInfo`), after `.` and `:` the fields and methods of the value (`self.`, a frame's widget API, `C_Spell.`, a module you imported, an enum, `s:upper()`), type names in annotations, and in strings the names their type allows (event names in `frame:RegisterEvent("…")` with each event's payload, anchor points in `SetPoint("…")`, the values of a string union you declared, also after `event == "`), after `@` the decorators you can put on what follows (`@script` on a method, `@global`, `@inline`, … with what each does, your own decorator functions, `@events.event("…")` with event names), each with its type and doc comment; and inside a call's parentheses the function's parameters with their types and descriptions, the one you are typing highlighted (several forms for an overloaded function). It never writes your files or `wowlua.lock` and never reaches the network. It stays quick on large addons: errors in the file you edit show about a fifth of a second after you stop typing (first the file and what it imports, the rest of your addon once you pause), formatting, the outline and go to definition answer at once even while your addon is checked, a save checks the packages that depend on yours too, files changed outside the editor (a branch checked out) are checked again, and a check that fails or hangs is logged and stopped without taking the server down; the server itself uses little memory. Turn it off with `wowlua.languageServer`; *wowlua: Restart Language Server* restarts it, and it restarts by itself if it stops. Other editors can start `wowlua lsp` themselves.
- **VS Code: syntax colors**: `.wowlua` files are colored: keywords, strings with their `{…}` expressions, comments (doc comments apart), decorators, class, function and type names, base classes, and type annotations (with a type parameter's `extends`) as types. `Cmd+/` (or `Ctrl+/`) toggles `--` comments, brackets and quotes close by themselves, and blocks indent as you type. *Format Document* uses wowlua for `.wowlua` files even when another formatter is your default.
- **The API explained in the editor**: hovering or completing a game API function shows what it does, what its parameters and results mean and a link to its warcraft.wiki.gg page (`UnitHealth`, `C_Spell.GetSpellInfo`, `frame:SetPoint`), as the wiki and Blizzard's documentation describe them; for the functions neither describes, a sentence wowlua wrote from the function's name and signature, marked as such. Lua's own functions (`string.format`, `table.insert`, `pairs`) and every function of wowlua's modules are explained too.
- **VS Code: Testing view**: the extension lists your test files and their `@test`s in VS Code's Testing view, with "Run test" and "Debug" above each test in the editor (and "Run all tests" at the top of the file; `wowlua.testCodeLens` hides them), runs a file or a single test with a click, shows each result at the test, a failure at the line it failed on (with the flavor when several run), debugs a test in `wowlua debug`, and with Run with Coverage shows which of your addon's lines, branches and functions the tests ran (VS Code 1.88 or newer). The cases of a `@cases` test appear as they run. `wowlua test "tests/x.test.wowlua::adds two numbers"` runs exactly that test.
- **VS Code: the coverage report in the editor**: *wowlua: Show Coverage Report* opens the HTML coverage report in an editor tab, from your last Run with Coverage in the Testing view or the newest `coverage.html` `wowlua test --coverage --reporter html` wrote (right-click a `coverage.html` in the Explorer to open that one; the Test Coverage view has a button for it too). It follows your light or dark theme, and clicking a line's number opens that line of the source beside it.
- **Examples that run**: a ```` ```wowlua ```` code block in a `---` doc comment is a test now. `wowlua test` (and the Testing view) runs it as `add example` with `expect` and everything the module exports at hand, reports it under its source file, and points a failure or a type error at the comment's line; the editor shows an example's type errors as you type. Mark a block `wowlua norun` to only show it.
- **Other players in the world**: a world profile's `units` can be other players, online and not in your group (`"player": true`). `/invite` brings them in a moment later, as if they accepted, so you can fill a party or, after converting to a raid, a raid of up to 40 and see your party and raid frames with them. `/uninvite`, `/kick` and `/promote` work on them (with the game's errors: "Your party is full.", "You are not the party leader."), `/ginvite` adds them to your guild, and `/who` lists them. `mock.spawnUnit({ …, acceptsInvites = true })` does the same in a test. The widget demo's `camelot` profile has eight to try.
- `"fmt": { "exclude": ["src/Data/**"] }` leaves files out of `wowlua fmt`, such as data your tools generate. A file you name (`wowlua fmt src/Data/Quests.wowlua`) is still formatted.
- `SetPortraitTextureFromCreatureDisplayID` draws the creature's portrait in screenshots, and a model given `SetCreature(npcID)` reports its display through `GetDisplayInfo()` when the game's data has that creature.
- A profile can leave files out of its builds: `"profiles": { "release": { "exclude": ["src/Diagnostics/**"] } }` keeps developer tools in your dev builds and tests and out of the release players get. Guard an import of such a file in code you keep with `@if(build.debug)`.
- `mock.setPlayer(data, { events = true })` and `mock.setRenown(faction, level, { events = true })` fire the events a real change would (`PLAYER_LEVEL_CHANGED`, `UNIT_LEVEL`, `UNIT_HEALTH`, `MAJOR_FACTION_RENOWN_LEVEL_CHANGED`, …), so Blizzard's UI and your addon see the new level or renown. Without the option they stay quiet, as before.

### Changed

- The language server remembers your addon between VS Code sessions: the first check after opening VS Code reuses what the last session found out about the files you haven't changed since, so errors show sooner (SuuUI: about 0.3 s instead of 0.4 s). It is kept in `~/.wowlua/cache/check/` (`wowlua cache clean` removes what no session used for 30 days, `--all` everything); set `WOWLUA_NO_CHECK_CACHE=1` to turn it off.
- Class methods without a result annotation return what their body returns, as functions do: `obj:get()` is a `number` when `get` returns `self.count`, where it was `any`. Completion and hover show the results (`-> ()`, `-> number`), and code that used a method's results wrongly now gets the error. Generic methods still return `any` unless annotated.
- `wowlua check`, `wowlua build` and `wowlua test` type-check code that passes event names (`EventName`) to generic functions much faster: SuuUI's workspace checks in two thirds of the time.
- `wowlua test` uses your WoW installation's own data (item names, creatures, appearances) only with `--game-data`: by default a test sees only what it defines, so it gives the same result on every machine and in CI. Debug sessions and `wowlua screenshot` still show the game as your installation has it.
- `texture:GetTexture()` and `GetTextureFileID()` give the file's ID, as in game, instead of the path you set: `SetTexture("Interface\\Buttons\\WHITE8X8")` then `GetTexture()` is 130871 with your WoW installation, nil for a file that doesn't exist. A test that compared it with a path now compares with the ID (or reads `GetTextureFilePath()`). `SetTexture("")` and `SetTexture(0)` return false, and `SetAtlas` replaces a file set before.
- The HTML coverage report (`wowlua test --coverage --reporter html`) is a page to work from: the files to test next, a file table you can sort and filter, and one file at a time with the lines that never ran (jump between them with `j` and `k`), its functions and how often each was called, and its source with each branch's counts in words. It carries wowlua's mark and colours, and is less than half the size it was.

### Fixed

- `wowlua fmt` keeps a comment after a field of a table type (`color?: string, -- a class colour`) on that field; it moved one field down on every run.
- `wowlua fmt` breaks a function header that is too long one parameter per line, instead of in the middle of a parameter's type (`units: UnitToken` / `| UnitToken[]`), and puts the `end` of an empty function on its own line when the header and `end` together would be too long.
- `wowlua fmt` leaves a table on one line when a comment on its own line stands before it, where it used to spread `{ "0000" }` over three lines.
- In tests, `C_FriendList.GetWhoInfo(i)` for a row that isn't there answers nothing, `GetUICameraInfo` of an unknown camera nine zeros and `false`, and `C_TransmogCollection.GetAppearanceCameraID` of an unknown appearance `0`, as the game does. A character loaded from a snapshot keeps its own talent config.
- A frame with an `OnEnter`, `OnLeave`, `OnMouseDown` or `OnMouseUp` script takes the mouse, as in game, even without `EnableMouse(true)`: clicks reach it and `IsMouseEnabled()` is true (`OnMouseWheel` turns on the wheel). The side tabs of the character frame and the group finder can be clicked now.
- More of what you type gets the server's answer, as in game: `/roll` ("Player rolls 42 (1-100)"), `/played`, `/afk` and `/dnd` (with `UnitIsAFK` and `UnitIsDND`), `/ignore` and `/unignore` (a real ignore list), `/who`, and the game's errors for `/invite`, `/ginvite`, `/gpromote`, `/gremove` and `/gquit` when the name doesn't exist or the guild master would leave. Emotes (`/dance`, `/wave`) no longer print "Type '/help' for a listing of a few commands.", and plural words in game text ("1 day", "2 days") show as the game shows them.
- Chat you send gets the server's answer, as in game: what you say, yell and emote shows in chat, party, raid and guild chat shows when you are in one (else the game's error, "You aren't in a party."), and a whisper shows as sent, or "No player named '…' is currently playing." for a name nobody has. Before, `SendChatMessage` did nothing.
- `C_SkillInfo.SetSelectedSkill` selects the skill line, so the character window's skills tab shows the first skill's details when it opens, as in game.
- Addons that load their own modules on demand through `GetAddOnInfo`'s reason (AtlasLoot) load them: it is `"DEMAND_LOADED"` for a load-on-demand addon that isn't loaded yet.
- `GetFileIDFromPath` answers nil for a file that doesn't exist, such as one of an addon you haven't installed, so AtlasLoot no longer draws the Atlas addon's maps without it. With your WoW installation, a game file answers its own ID.
- With your WoW installation, `C_Map.GetAreaInfo` names every area (AtlasLoot's instance names), and `C_Item.DoesItemExistByID` is true for items the game ships without a name.
- `C_Item.GetItemInfoInstant` and `C_Item.GetItemIconByID` answer for every item in the game's item table, also those whose names WoW only gets from the server, as in game: addons' loot lists show the items' icons instead of question marks.
- `texture:SetColorTexture` replaces a file set before, and `SetTexture` a plain color, as in game: textures given a file and then a faint color were drawn as the file, solid white (the Forever Dungeon Journal's dungeon cards).
- Text in a scaled frame (`frame:SetScale`) is drawn at that scale in screenshots: it was drawn full size and cut off (the Settings list's "NEW" badge read "N...").
- The friends window shows your BattleTag and your online status icon: `BNGetInfo()` answers the account, inline atlases (`|A:name:…|a`) are read right, and a texture named `.tga` that the game stores as `.blp` is found.
- Texture gradients (`texture:SetGradient` and XML `<Gradient>`) are kept and drawn in screenshots: Blizzard's help tips are dark gold boxes again instead of white ones.
- The dressing room lists the outfit beside the model (each slot's item with its icon), as in game: an actor's `GetItemTransmogInfoList()` holds what it wears and tried on, and its model scene gets `OnDressModel` as it dresses. The panel was empty.
- Textures and atlases inline in text (`|T…|t`, `|A…|a`) are drawn in screenshots and take their width in the line, as in game: the quest log's "characters with a ! above their head" shows its icon.
- The character window's PvP tab shows your rank, rank points, the next rank's rewards and the season's cap, as in game (WoW Forever's PvP ranks, major faction 2800), with its rank ring: `texture:SetAtlas` with an atlas the game lacks leaves the texture as it was, as in game. Side panes show their descriptions (an XML font string without anchors fills its frame), and the atlas table follows the updated client (1.60.1.70532).
- `wowlua test` no longer crashes with "stale handle" when garbage is collected while a test waits (`mock.waitSeconds` and the like): the running test could be freed between its steps. Long tests hit it most.
- The world map shows the player's arrow where the player stands, turned as the player faces (`UnitPositionFrame:AddUnit`, which Blizzard's map pins use).
- `frame:SetFrameLevel(n)` moves the frame's children that have their own level along with it, as in game: Blizzard's dropdown and right-click menus show their entries over the menu's background (they were drawn faint underneath).
- Tooltips show in screenshots: a tooltip lays out its lines and sizes itself as the game's does, `mock.showTooltip` places it where the default UI places a tooltip, and a `|cAARRGGBB` color code ignores its alpha byte, as in game (a line written `|c00…` was invisible).
- A frame with `useParentLevel` is drawn together with its parent, layer by layer, as in game, and XML fonts keep their `justifyH`, `justifyV` and `spacing`: the gossip window shows its parchment and its options sit left beside their icons.
- `button:GetTextWidth()` measures the button's text in its font string's font, as in game: Blizzard's money displays show the amount beside each coin (a vendor's prices were drawn over the item icons).
- Text is drawn over the textures of its own layer, as in game: window titles on Blizzard's metal frames ("Options" on the Settings panel) show again.
- XML textures take their `<Color>` (a plain color without a file, a tint with one) and frames their `clipChildren`, as in game: the Settings panel has its background again (no more white corners or the world showing through), and its scrolling lists stop at their edges.
- A font string without a draw layer is drawn on `OVERLAY`, as in game, so a button's text shows over its background (the Legacy challenges' categories were empty rows), and `button:SetNormalFontObject("GameFontNormal")` and its highlight and disabled versions take a font's name, as `SetFontObject` does (the text was drawn a pixel high).
- `button:SetFormattedText(...)` shows the text on the button, as `SetText` does; it was kept but not drawn (the macro window's "Specific Macros" tab was blank).
- When two anchors set the same edge of a frame, the one WoW gives priority to wins (`TOPLEFT` before `TOPRIGHT` and so on), as in game: the default UI's dropdowns show their text inside the box instead of over its bottom edge, and window tabs sit side by side instead of overlapping ("Merchant" was cut off).
- The wardrobe shows each appearance from the camera WoW uses for your character's race and sex (`C_TransmogCollection.GetAppearanceCameraID`), read from your WoW installation; it used one Undead male's for everyone.
- Model frames framed with `Model_ApplyUICamera` (the wardrobe, the dressing room's slots) show the model as in game: `TransformCameraSpaceToModelSpace` scales by the frame's effective scale, as the client's does, and screenshots draw the shot at the client's field of view, with the model's pitch and roll turned as the client turns them.
- Models drawn with material blend 7 add their light, as in game (a totem's rune showed on a black square), and particles of the newer emitters draw no stray tails (a fire elemental showed long streaks).
- `Minimap:GetPingPosition()` gives the ping's place from the player in widths of the minimap's view, the ping staying where it was in the world, and `C_Minimap.GetViewRadius()` follows the zoom, as in game.
- Screenshots read your WoW installation after the client updates: a file the updated client's index lists twice is read from the entry that holds it (all textures were missing after today's camelot patch).
- `frame:RegisterEvent` refuses the events WoW Forever keeps for event callbacks alone, as the client does ("Attempt to register unknown event"): `MINIMAP_PING`, `TOOLTIP_SHOW_ITEM_COMPARISON`, `COMBAT_LOG_EVENT_INTERNAL_UNFILTERED`, `COMBAT_LOG_APPLY_FILTER_SETTINGS`, `COMBAT_LOG_REFILTER_ENTRIES` and `CLASS_TALENTS_SWITCH_TO_LOADOUT_BY_NAME`. `COMBAT_LOG_EVENT_UNFILTERED` still registers.
- An addon's `RegisterEventCallback` for `MINIMAP_PING`, `COMBAT_LOG_ENTRIES_CLEARED`, `COMBAT_LOG_APPLY_FILTER_SETTINGS` or `COMBAT_LOG_REFILTER_ENTRIES` is forbidden, as in game: `ADDON_ACTION_FORBIDDEN` fires, the callback is never called, and `frame:RegisterEventCallback` returns false. Only Blizzard's own code hears these events.
- Talent points follow the character's level: one a level from level 10 (earlier with Legacy's "Talented" ranks), at most 51, so the talent window and `C_Traits.GetTreeCurrencyInfo` show what a character of that level has.
- The default world's talent tree shows its talents: the player's talent config is the one the emulated client holds talents for (it named another, so Blizzard's talents tab drew only its backgrounds).
- With your WoW installation at hand, an item your test doesn't define is the game's own: `C_Item.GetItemInfo`, `DoesItemExistByID` and Blizzard's item loading answer with its name, quality, icon, class and description from the client's tables, so Blizzard's Legacy rewards panel no longer raises "invalid itemID". Items you define stay yours.
- Textures in XML without anchors fill their frame even when they have a size, sliders place their thumb by their value (anchored `LEFT` to the slider's `LEFT`, so `thumb:GetPoint()` answers as in game), and a texture's `rotation` attribute turns it: Blizzard's group finder, Settings' sliders and the group finder's divider look as in game.
- `C_SpecializationInfo.HasPlayerEarnedATalentPoint()` follows the player's level (from level 10, earlier with Legacy's "Talented" ranks), as in game: the talents button turns on once the character reaches it.
- `frame:GetScaledRect()` gives the rectangle times the frame's effective scale, as in game. Layouts built on it (Blizzard's action bars, `ResizeLayoutFrame`) came out too large by one over UIParent's scale, so the default UI's bars sat apart.
- A button's normal, pushed, disabled and highlight textures that `SetNormalAtlas` or `SetNormalTexture` makes fill the button, as in game; they had no size (the default UI's micro menu buttons showed no icons).
- `<MaskedTextures>` in XML masks the textures it names (`<MaskedTexture childKey="icon"/>`): action buttons' icons are masked and drawn over their slot art.
- `bar:SetStatusBarTexture("atlas-name")` takes an atlas by its name, as Blizzard's bars use it (the experience bar).
- A frame in XML with both `setAllPoints="true"` and its own `<Anchors>` is placed by its anchors and size, as in game: the player frame's resting "Z" sits at the portrait's top right instead of over the health bar.
- Textures snap to the pixel grid unless `SetSnapToPixelGrid(false)` or `snapToPixelGrid="false"` says otherwise, as in game: `IsSnappingToPixelGrid()` answers true and `GetTexelSnappingBias()` 0.3 at first, and screenshots draw their edges on whole pixels. XML's `horizTile`, `vertTile` and `texelSnappingBias` attributes are read too.
- Text with a named colour (`|cnNAME:text|r`, as chat links write it) shows and measures only its text; it showed part of the colour's name.
- `font:CopyFontObject(other)` copies the other font's file, height, colours and shadow, as in game: Blizzard's chat frames lost their font when they made it their own.
- An animation group with `setToFinalAlpha` keeps its alpha animations' end values when it finishes, so `GetAlpha` on their regions reads them, as in game; `flipBookRows`, `flipBookColumns` and `flipBookFrames` in XML are kept too.
- `GetBindingText(key, 1)` shortens the modifiers as the action bars show them (`s-1`, `c-2`), so Blizzard's hotkey labels in tests and screenshots read as in game.
- Textures from XML templates (`UIPanelButtonTemplate`'s and others') now get their `<TexCoords>` and `alphaMode`, as in game: `GetTexCoord` and `GetBlendMode` give what the template sets.
- The HTML coverage report of a workspace showed no source and took the name of the last addon in it: it now shows each file's source and the root package's name.
- Code in a branch your build removes (`if build.debug then … end` in a release, `if flavor == "vanilla" then … end` in another flavor) is no longer checked, as documented: it may use what that build doesn't have, such as an import under `@if(build.debug)`.

## 0.9.8 (2026-10-24)

### New

- **Function overloads**: a top-level function can declare several signatures before its implementation, as a class method can: `overload function find(id: number): Item?`, `overload function find(name: string): Item?`, then `function find(key: number | string): Item?`. Callers see only the overloads; `export overload function` exports them. A signature with no implementation right after it is E3047.
- A script handler set on an instance of your widget class gets that class: in `toggle:SetScript("OnClick", function(self) … end)` with `class Toggle extends Button`, `self` is a `Toggle`, so its fields are typed (it was a `Button`). The same goes for every method of the game's classes that takes or gives `Self`.
- `keyof T` and `T[K]` types, as in TypeScript: `function get<K extends keyof Defaults>(key: K): Defaults[K]` gives each setting its own type, and a key that isn't a field of `Defaults` is an error (E2014).
- `SetScript` and `HookScript` know what the game passes each script: in `frame:SetScript("OnUpdate", function(self, elapsed) … end)`, `self` is the frame and `elapsed` a number, for every script with known arguments (`OnClick`, `OnEnter`, `OnValueChanged`, `OnKeyDown`, …).
- W2063 reports values that are `any` without anyone writing it: a parameter nothing gives a type, or a local whose value is `any`. It is off unless you turn it on (`"check": { "lints": { "W2063": "warn" } }`), for projects that want `any` only where it is written. It also reports a generic type written without its type arguments (`Box` for `Box<T>`), which makes them `any`. And a field or method your widget class (`class Toggle extends Button`) doesn't declare: the game's frames take any field, so it passed as `any`.
- `EventArgs<E>` works in your own functions, not only in `wowlua:events`: `function on<E extends EventName>(event: E, fn: (name: E, EventArgs<E>...) -> ())` gives `on("UNIT_HEALTH", function(name, unit) … end)` the event's argument types, and `function send<M extends MessageName>(name: M, ...: EventArgs<M>...)` checks the values sent against `declare message`. Before, it was `any`.
- A widget class can extend `FontString`, `Texture`, `Line` or `MaskTexture`: build it on the region its frame makes with `MyLabel.adopt(frame:CreateFontString(), …)`.
- `MyButton.adopt(frame, …)` builds a widget class on a frame that already exists, such as the buttons a secure group header makes, instead of creating a new one: your constructor runs on that frame and your `@script` handlers are set.
- `@deferred` on an interface method's callback parameter (`function on(name: string, @deferred fn: Handler)`) tells the load-order check that every implementation calls it later, so a callback registered through that interface at load time no longer makes files depend on each other.
- `wowlua:reflect` is typed as documented: `reflect.isInstance(v, Dog)` narrows `v` to `Dog`, `reflect.classOf(dog)` gives `ClassOf<Dog>?`, and `reflect.enumFrom(Direction, raw)` gives `Direction?` (`reflect.enumEntries` gives the members as `Direction`). Before, they were `any` and `boolean`. Your own generic functions can do the same: a parameter `ClassOf<T>` now takes `T` from the class or enum passed.
- **Parameter properties**: a constructor's parameter can declare a field, as in TypeScript: `constructor(private sink: Sink = printer, public readonly name: string)` makes `self.sink` and `self.name` and sets them from the arguments. A field declared twice is now an error (E3046).
- The standard library reads what the game gives without trusting its type: `secret.asString`, `asNumber` and `asBoolean` read a value typed `unknown`; `errors.try(fn, ...)` and `errors.tryMethod(obj, name, ...)` call a game function a client may lack, quietly; `table.field(t, "a", "b")` reads through values that may not be tables; `table.weakKeys()` and `weakValues()` make typed weak tables; `frames.find(name)` and `frames.asButton(v)` (and the other widget kinds) find the game's frames; `frames.keepAway(frame)` keeps one out of sight while it stays shown. `format.money(c, { short = true })` gives "12g 5s 3c", and `mock.partial({ … })` makes a typed fake record for a stub.
- `errors.tryValue(fn, ...)` calls a game function a client may lack, quietly, as `errors.try` does, and keeps its result type: `errors.tryValue(C_TradeSkillUI.GetRecipeInfo, id)` is a `TradeSkillRecipeInfo?`, and its arguments are checked. `secret.isNumber(v)` and `secret.isString(v)` narrow a value to a number or string that isn't secret.
- `table.list(v)` walks a list the game gives as `unknown` without a cast (`for _, x in ipairs(table.list(v))`), and `format.ago(seconds)` says how long ago: "5 min ago", or with `{ long = true }` "3 days ago".
- `wowlua:chat` opens the player's chat line: `chat.whisper(name)`, `chat.insertLink(link)` (as a shift-click; a new line when none is open) and `chat.open(text)`, through the game's own chat code on every client. `unit.invite(name)` invites a player to your group.
- `secure.overlay(button)` puts a secure action button over one of your own buttons, for what only a secure click may do (a slash command, a click on one of the game's protected buttons): `overlay.set("type1", "macro")`, `overlay.show()` (or `overlay.show(row)` over another frame, such as the row under the mouse), `overlay.hide()`. It is anchored only while shown, so your window never turns protected and still opens in combat.
- Restyling the game's frames: `frames.textures(f)`, `frames.fontStrings(f)` and `frames.children(f)` give typed lists, `frames.stateTextures(button)` a button's state art, and `frames.rows(list)` the rows one of the game's lists shows (an object pool or a scroll box). `events.onRegistry(name, fn)` listens to Blizzard's `EventRegistry` callbacks, with error isolation and a subscription to cancel.
- `chat.whisperBnet(accountName)` and `unit.inviteBnet(gameAccountID)` whisper and invite Battle.net friends, and `chat.filter("CHAT_MSG_SYSTEM", fn)` hides the chat lines `fn` returns true for, through the game's own filters (an error in `fn` is reported and the line shows; cancel the subscription to remove it).
- `frames.mapPins(WorldMapFrame, "QuestPinTemplate")` lists the pins one of the game's maps shows of a template, as frames.
- `wowlua:chat` finds the game's chat windows, typed: `chat.windows()` (the numbered ones, then the whisper windows), `chat.docked()` (in the dock's order), `chat.window("ChatFrame3")`, `chat.editBox(w)`, `chat.isTemporary(w)` and `chat.conversation(w)` (a whisper window's chat type and player).
- `table.listAt(t, key)` and `table.recordAt(t, key)` give the list or record at `t[key]`, made and stored there when it is missing: `table.insert(tbl.listAt(saved, "errors"), text)` adds to a list kept in saved variables read as `unknown`, without a cast. `frames.keyed(frame)` gives the widgets a frame holds in its own fields, by key (`frame.ScrollBox`), for reading or restyling the game's frames.
- `mock.tooltipLines(tip?, right?)` gives the lines a tooltip shows in a test (its left texts, or the right ones), and `mock.sentAddonMessages()` is typed: each message's `prefix`, `message`, `chatType` and `target`.
- `mock.textOf(label)` gives the text a font string, button or edit box shows in a test, `""` when it has none.
- In tests, more of the game's widget methods do what they do in the game: `frame:Raise()` and `frame:Lower()` change the frame level, `editBox:HighlightText()` selects text that `editBox:Insert(text)` and `mock.type` then replace (typing used to append), `font:CopyFontObject(other)` copies the font, `AdjustPointsOffset` moves the anchors, and `ClearModel`, `ClearNormalTexture`, `ClearText`, `ClearParentKey`, `ResetTexCoord` and `StopAnimating` undo what was set. `SetMerchantFilter`, `SetGuildRosterSelection` and `TaxiRequestEarlyLanding` work too instead of failing as not mocked.
- Tests can target as the player does: a keybinding or secure button calling `TargetNearestEnemy`, `TargetNearestFriend` and the other tab-targeting functions cycles the units in range, nearest first; `TargetLastTarget`, `AssistUnit`, `FocusUnit`, `CancelUnitBuff` and `CancelSpellByName` work too. An edit box keeps the lines `AddHistoryLine` adds, and `mock.pressKey("UP")` brings them back; the arrow keys run `OnArrowPressed` and move the cursor.
- No game function fails as "not mocked" in tests anymore: auto attack (`StartAttack`, `StopAttack`), `SetPartyAssignment`, `SendSystemMessage`, money on the cursor (`PickupPlayerMoney`, `GetCursorMoney`), `TargetLastEnemy`, `Sign` and the rest have behaviour, and what the test client doesn't draw or keep (models' cameras, the browser, housing, movies) answers the game's empty values. `ClearAttributes`, `ClearScripts`, `RemoveAnimations` and `LockHighlight` work as in the game.

### Changed

- **The game's API is typed throughout**: the about 3,000 functions per game version that were `any` (Blizzard's UI functions such as `StaticPopup_Show` and `CreateDataProvider`, legacy C functions such as `C_Garrison.GetFollowers` and `C_PetJournal.GetPetInfoByIndex`, mixin methods) now have their parameters and results, from warcraft.wiki.gg's API pages and from Blizzard's own UI code; globals such as `NUM_BAG_SLOTS` and `ACHIEVEMENT_YELLOW_BORDER_COLOR` have their types. What no source tells is `unknown` instead of `any`: you can pass it anything, and check what it gives you before using it. A mistake in a call to one of them is now an error where it passed before.
- A class's `overload function` signatures are what callers see now (they saw the implementation), and the implementation must accept each one (E3044), as documented.
- More of the game's API is typed as the game answers: `GetAchievementInfo(categoryID, index)` gives the achievement's values (it gave none), its month, day and year can be nil, `GetChatWindowMessages` gives every message group, and `fontString:GetText()` and a button's `GetText()` are `string?`, because a fresh one has no text. Code that used their result as a string now has to handle nil.
- `CreateFrame("Browser")` gives a `Browser` (and `"Checkout"` a `Checkout`) on the game versions that have them; it gave a plain `Frame`. `frames.asBrowser(v)` reads one of the game's browsers (`HelpBrowser`) as a `Browser`.
- More game functions that were untyped have their types: `Clamp`, `Round`, `Lerp`, `CountTable`, `UnitAura`, `UnitBuff`, `FindBaseSpellByID`, `CombatLogGetCurrentEventInfo` and others, from warcraft.wiki.gg or from how another game version declares them.
- The game's own functions that nothing documents are typed from what the game itself answered when probed (`HasInboxItem(messageIndex, attachIndex)`, `GetPlayerTradeCurrency()` two numbers), or by their names (`IsChatAFK()` a boolean, `GetNumFlyouts()` a number); `CombatLogGetCurrentEventInfo()` gives the combat log's eleven base values typed.
- `UnitAura`, `UnitBuff` and `UnitDebuff` give their values typed (name, icon, count, …), and Blizzard's utility tables have all their functions: `AuraUtil.UnpackAuraData`, `AuraUtil.ForEachAura` and the others where only some were declared.
- `hook`, `hookScript` and the test decorators (`@test`, `@skip`, `@only`, `@flavors`, `@cases`, `@expectError`, `@beforeEach`, …) are typed instead of `any`: `hook(name, fn)` and `hook(target, name, fn, opts?)` take the function, `@hook(name)` gives the decorator, and a mistake in their arguments is an error. `jobs.start`'s function may return anything, and a function wrapped with `@outOfCombat` returns nothing (its call may wait for combat to end).
- The standard library takes and gives `any` in fewer places: `mock.click`, `script`, `hover`, `typeText`, `drag`, `slide`, `dragTo`, `sizeBy`, `wheel` and `protect` take frames (a `nil` frame fails the test with "mock.click: no frame"), `mock.frames()` gives `ScriptRegion[]` and `mock.defineTemplate`'s function gets a `Frame`; `serialize.decode` and `importString`, `reflect.getClass`, `superclass` and `classes` give `unknown` (check it before you use it); `debug.measure`, `errors.callIn` and `errors.callOwned` keep the function's argument and result types, and `debug.stats()` its entries' fields; values only passed on (`log.info(...)`, `events.send`, `errors.report`, `table.equal`, `mock.fireEvent`, an owner, …) are `unknown`.
- An addon that lists another addon of its workspace in `addon-dependencies` sees that addon's `@global` API with all its types, as that addon has them (a value without an annotation, a class or an enum was `any`): `wowlua check` reads the other addon's sources with yours, without building them into yours or reporting their problems. You can import its types (`import type UnitFrame from "suuui/UI/UnitFrame"`); importing a value from it is E1041, as its modules aren't part of your addon.
- A global made with `@global` has its value's type where your other files read it (it was `any`).
- A value typed `unknown` stays `unknown` inside once you check its kind: after `type(v) == "table"`, `v.name` is `unknown` (read it with `secret.asString` or check its type), and after `type(f) == "function"`, `f(...)` gives `unknown`. Before, both gave `any`, so a mistake there went unnoticed. `local ok, err = pcall(f)` with an `f` that returns nothing gives `err` as `unknown` too.
- Font strings in tests break text into lines as the game does (measured in the game): also after a hyphen, a word too wide for the line cut between characters, `GetNumLines()` counting every line (also with word wrap off or beyond `SetMaxLines`), and text held to fewer lines ending in an ellipsis, with `GetStringWidth()` the width of what shows. `IsTruncated()` answers. Kerning is known at more sizes.
- `local o = opts or {}` keeps the type of `opts` (an `Options?` gives an `Options`), as do `list or {}` and `x ?? {}`: the empty table no longer turns its fields into `any`.
- A class's fields and methods typed with a `typeof` alias (`type Api = { UnitClass: typeof(UnitClass) }`, `api: Api`) have its type inside the class's methods, where they were `any`.
- `select(n, …)` with a number written as the index gives the types of the values from that one on: `select(2, f())` is `f`'s second result, where it was `any`.
- A generic function's result type comes from a function you pass it when nothing else gives it: `local ok, name = pcall(function() return unit.name end)` types `name`, where it was `any`.
- `group:CreateAnimation("Alpha")` gives an `Alpha` (and each other animation type its own), so its methods are checked; it was untyped.
- Reading a record with a key typed `keyof` it (`colors[k]` with `k: keyof Colors`) gives that field's type, where it was `any`.
- In a generic function or class, a value whose type is a type parameter with a constraint has the constraint's fields and methods: with `B extends Frame`, `b:GetWidth()` is a `number`, where it was `any`.
- A generic function keeps the type of a value you pass it when that value already has one: `assert(tokens[k])` with `tokens: { [string]: Token }` is a `Token`, not a `string`. A literal written as the argument still widens (`id("x")` is a `string`).
- A value typed as two shapes at once, such as `number[] & { hex: string }`, reads `c.hex` as a `string` and `c[1]` as a `number`, where both were `any`.
- `local p = cache[key]; if not p then p = { seen = {} } end` gives `p.seen` the type of the cache's `seen` field, where the empty table made it `any`.
- Calling a method a record type doesn't declare, on a record with an indexer (`m:Setting("x")` with `{ Get: …, [string]: unknown }`), calls the indexer's value: an `unknown` there is an error until it is narrowed (E2004), where the call was `any`.
- A function passed where a function with `...` parameters is expected types its parameters from them: in `text:gsub("(%w+)=(%w+)", function(key, value) … end)`, `key` and `value` are strings, where they were `any`.
- W2063 (when turned on) also reports a field one of the game's frames doesn't declare, read or called (`button.owner`, `tooltip:AppendInfo()`): it is `any`. Give the value a type that has it (`Button & { owner: Frame? }`), or read it with `frames.keyed` or `table.field`.
- A script handler set on a frame typed with fields of your own (`local b = CreateFrame("Button") :: Button & { owner: Frame? }`) gets that type as `self`: `self.owner` is typed in `b:SetScript("OnClick", function(self) … end)`, where it was `any`.
- `pcall(CreateFrame, "ModelScene", nil, parent)` types the frame (the overload the arguments match), `select(i, ...)` with a variable `i` keeps the values' type, and `reflect.isInstance(SuuUI.GetModule("x"), MyModule)` narrows to `MyModule` though the module's declared type is unrelated; all three were `any` or unusable.
- `local p = x :: FramePoint` keeps `FramePoint` (and `local v = t?.field` the field's type), where the local widened it to `string`.
- A value typed `unknown` is `false` or `nil` where it is falsy: `x and true or false` is a `boolean`, and `if not x then` knows `x` is `false` or `nil`.
- The game's transmog info (`ItemTransmogInfo`, as `C_TransmogCollection.GetCustomSetItemTransmogInfoList` gives it) has its `appearanceID`, `secondaryAppearanceID` and `illusionID`.
- `typeof(x)` in a function's parameters and results has `x`'s type: `local function hex(k: keyof typeof(colors))` takes only the colours' names and `colors[k]` is typed, where both were `any`.
- `local w = cache[key]; if w == nil then w = false end` keeps `w` as the cache's `T | false`, where `false` became `boolean` and `cache[key] = w` was an error.
- A record type that refers to itself (`type Entry = { name: string, members: Entry[] }`) types what it holds: `entry.members[1].name` is a `string`, where it was `any`.
- A type parameter with a constraint keeps a literal argument: `function f<K extends "a" | "b">(k: K): K` gives `"a"` for `f("a")`, where it gave `string` and reported that `string` doesn't satisfy the constraint.
- An error naming a very long union (such as `EventName`) shows its first members and how many more there are, instead of all of them.
- A function in a table you pass to a generic function gets its parameter types from the rest of the table: in `segmented({ options = { { value = 5, label = "5" } }, onChange = function(v) … end })`, `v` is the options' value type. Before, `v` needed an annotation (E2050). And `print(f(x))` no longer turns the result of a generic `f` into `any`.
- A local without an annotation that a loop assigns keeps its type in the loop and after it, where it was `any`: `local found = nil` set to an index in a search loop is a `number?` after the loop. Code that used such a local as if the loop had always set it now gets an error there (check it, or use `assert`).
- A function you write in `hook(…)` or `hookScript(…)` gets the types of what it hooks: in `hook(GameTooltip, "SetUnit", function(tip, unit) … end)`, `tip` is the tooltip and `unit` a string, and `hookScript(frame, "OnSizeChanged", function(self, w, h) … end)` types `w` and `h`. They were `any`.
- `import internal` of a module's `export function` gives it its type, where it was `any`.
- W1004 also warns about a `local function` with the name of another local in the same block: calls meant for the first one reach the second, also from inside it.
- Strict mode: reading a named field of a list (`list.label`) or a number key a record doesn't have (`art[1]` of `{ atlas: string }`) is an error (E2003); both read as `any` before. A list with named entries needs its type written: `{ [number]: string, label?: string }`. Reading a table that has no indexer with a key (`t[k]`) is still `any`, and W2063 now reports it.
- A local without a type annotation that a function changes keeps a type inside functions: `local count = 0` read in a callback that does `count = count + 1` is a `number`, a forward-declared `local update` is the function assigned to it later, and a flag saved and restored is a `boolean`. Before, they were `any` there, so mistakes with them went unreported; now they are checked, and W2063 no longer reports them.
- In a class's method or constructor, a function whose parameter is named `self` (`frame:SetScript("OnEnter", function(self) … end)`) now warns (W1004): inside it, `self` is the frame, not your instance, and `self.field` reads the wrong table. Name the parameter differently. Plain `function M:m()` code is unchanged.
- `assert` returns every argument it was given, as Lua's does, so `return assert(x, "message")` in a function that returns one value is now reported (too many values returned): it also handed the message to the caller, as a second value. Write `return (assert(x, "message"))`.
- A parameter's default can't read that parameter or a later one: in the Lua it compiles to, every parameter is in scope there and not set yet, so `function f(conf: Conf = conf)` or `constructor(private settings: Store = settings.store)` read `nil`, not the outer value wowlua checked them against. That is now an error (E1004); give the parameter another name. A function in a default may still use any parameter.
- The load-order check follows `MyClass.new(…)`: what the constructor (its default arguments included) and the field initializers read while a file loads now orders the files. Before, a default instance built at load could run before the module it reads, inside an import cycle, and fail in game with no warning.
- The load-order check follows your classes: a method called on `self`, or on an instance you made with `MyClass.new(…)` in the same file, is that class's method (or a subclass's override), so callbacks passed through your own classes no longer count as called at load. Before, such calls were unknown and could report a dependency cycle (E1014) or W1034 that wasn't real.
- When a generic call's result doesn't fit where it goes, the error names the call's type alone: `local b: boolean = id(5)` reports `number?`, not `nil | boolean | number`.
- A `readonly` field can only be assigned in its initializer or in its own class's constructor, as documented: assigning it in a method, or in a subclass's constructor, is now an error (E2010).
- A method replaced on a widget class after it is defined (`MyButton.refresh = newRefresh`, as a plugin might) now reaches the frames the class builds afterwards, its subclasses' too, as a plain class's instances see it. Frames built before keep the method they were given.

### Fixed

- A value written with `::` keeps its type in a table: `{ corner = "TOPLEFT" :: FramePoint }` has a `FramePoint` field, where it was a `string`.
- `frames.children`, `frames.textures` and `frames.fontStrings` work on a frame with many children or regions, such as `UIParent`; they crashed the game there.
- `if b.owner then b.owner:Hide() end` narrows `b.owner` when `b` is typed as two parts (`Button & { owner: Frame? }`), as it does for a record; it stayed `Frame?`.
- A type predicate narrows one of the game's frames to its type: after `if canDisable(f) then` with `canDisable(f: Frame): f is Frame & Disableable`, `f:SetDisabled(true)` is checked; it was `any`.
- `Browser` and `OffScreenFrame` frames have a frame's methods (`SetPoint`, `SetScript`, …); they had only their own.
- A class field typed by another file's alias that uses `typeof` (`api: UnitApi`, with `UnitApi = { UnitHealth: typeof(UnitHealth), … }` in another file) is typed in the class's methods; it was `any` there.
- A function taking an `EventName` (or another type of many names) no longer stops the development build from loading ("chunk has too many syntax levels"): its argument check is now a single lookup.
- A value of several types at once fits a type of some of them: `A & B & C` is an `A & B`, where only a single `A` was accepted. The game's chat windows (`ChatFrame1`) now fit `ScrollingMessageFrameMixin & Frame`.
- `CreateFrame("ScrollingMessageFrame")` gives a message frame: `AddMessage`, `SetFading`, `ScrollToBottom`, `GetScrollOffset` and the rest are typed, and so are the same methods of the game's chat windows (`ChatFrame1:GetNumMessages()` is a `number`). It was a plain `Frame` without them, and the chat windows' methods took and gave `any`.
- `ItemLocation:CreateFromBagAndSlot(bag, slot)` gives an item location the game's functions take (`C_AuctionHouse.IsSellItemValid`, `C_Item.GetItemName`, …); it was a type of its own that none of them took.
- `date("*t")` gives the time's parts (`year`, `month`, `day`, `hour`, `min`, `sec`, `wday`, `yday`, `isdst`) and `date("%H:%M")` a string; both were `any`.
- W2062 warns about `local x, y = pos and pos:GetXY()`: an `and` or `or` keeps only one of the call's values, so `y` is always `nil`.
- In strict mode, a name the `string` library doesn't have (`s.message`, `s:nope()`) is an error (E2003); it read as `any`. So is a named entry in a table literal where a list is expected (`{ class = "X", "Y" }` for a `string[]`), which was dropped without a word.
- A class field typed with `typeof(…)` (`map: typeof(WorldMapFrame)? = nil`) has that type; it read as `any`.
- In tests, `GetItemInfo` gives an item defined with `mock.defineItem` its link (it gave none, so `item.get` found nothing for it).
- `wowlua fix` crashed before doing anything. It works again; migrating `wowlua.json` to the current schema is now `wowlua fix --migrate`, since `--manifest <path>` names the manifest for every command.
- A class extending a class you imported with `import type` (`class Slot extends ItemSlot`) is now an error (E3008): it checked clean and failed when the addon loaded. Import the class itself (`import ItemSlot from …`).
- `C_DurationUtil.CreateDurationTextBinding()` gives a typed binding (`DurationTextBindingObject`: `SetDuration`, `SetEnabled`, `SetFormatter`, …), where it was `any`, and the rule and abbreviated number formatters (`C_StringUtil.CreateNumericRuleFormatter()`, `CreateAbbreviatedNumberFormatter()`) are `NumericFormatter`s, so they can be passed as an aura button's text formatter or a binding's.
- The development build's argument check of a game object type (`DurationObject`, a colour) now takes the engine's objects: they are userdata, which it refused ("DurationObject? expected, got userdata").
- Using `self` in the arguments of `super(…)`, also inside a function written there, is now reported (E3001): such a function may run before `self` is your new instance (a widget class's frame doesn't exist yet), so it read the wrong table.
- `fontString:SetFormattedText(format, ...)` accepts secret values among its arguments, as the game does (a target's name in a dungeon).
- A field added to a table inside another (`API.UI.Button = button` after `API.UI = { … }`) is now part of the table other files import: they read it typed, where they got `any`, and a misspelled field there is an error.
- `for k, v in pairs(t)` over a record type with an `unknown` indexer (`{ name?: string, [string]: unknown }`) is no longer refused (E2001).
- Strict mode: a field your constructor sets in every branch of an `if … else … end` counts as set (E2052 said it wasn't), as does one set in a `do` block.
- Strict mode: after `self.icon = parent:CreateTexture()`, calling a method on the field (`self.icon:SetTexture(…)`) no longer makes the next line's `self.icon` "may be nil" again. A call that gets a field's table can't replace the field; only calls that get the table holding it forget what you set.
- A WoW function with several forms (`SetTexCoord`) accepts a last argument that gives several values, as Lua passes them: `tex:SetTexCoord(unpack(coords))` was refused (E2012).
- `Texture:GetTexture()` can give a number, the file ID of a texture set by one, as in the game; it was typed as a path only.
- A cast of an optional value to a supertype of what it holds (`w.tabs :: Button[]` for a `TabButton[]?`) is no longer refused (E2015): a cast now also compares the value's type with its `nil` left out.
- E2052 (a field not assigned in the constructor) says so when the field is assigned, but only after a call of one of the class's own methods on `self`, and names that call; before, it said the field was not assigned at all.

## 0.9.7 (2026-10-23)

### New

- A new warning, W2062: reading more values than a function of yours can give, which are always `nil`. wowlua counts what each of its `return`s gives, even when it is declared `...any`, and warns on `local a, b, c = f()` or `select(5, f())` past that. It checks your `local function`s, your module functions, the functions you import from your other files, and the methods of a file-level table (`function M:Load()`) called as `M:Load()`, `M.Load()` or `self:Load()`, wherever in the file they are defined. A common case is a `pcall` helper that returns `a, b, c, d, e, f, g, h`: values 9 and up are lost. To pass every value on, return them through `...`. It found two such bugs in SuuUI.
- Tests: `mock.drag(frame, dx, dy, frames)` drags over that many frames, so code that follows the mouse in `OnUpdate` while dragging runs as it would in the game. The mouse now starts on the dragged frame and moves with it (`GetCursorPosition`).
- Tests: `anything` and `ofType("table")` from `wowlua:test` match any value (but `nil`), or any value of a type, inside `toEqual` and `toHaveBeenCalledWith`: `expect(onShow).toHaveBeenCalledWith(anything, "TOOLTIP")`.
- Tests: the encounter timeline keeps your addon's timers (`C_EncounterTimeline.AddScriptEvent`), as WoW Forever does even with its own timeline off: they are listed, readable (`GetEventInfo`, `GetEventState`, `GetEventTimeRemaining`), can be paused, resumed, finished or cancelled, and end when their time runs out, with the timeline's events.
- Tests: `mock.postLFGListing(createData)` lists your character in the group finder, as the game's own listing panel does (the listing arrives a moment later with `LFG_LIST_ACTIVE_ENTRY_UPDATE`), and `mock.delistLFG()` takes it down again. Your addon's own `C_LFGList.CreateListing` is blocked, as in the game.
- Tests: put a group member or a pet on a creature's threat list with `mob:setThreat(value, member)`. `UnitThreatSituation` and `UnitDetailedThreatSituation` then give each of them their status and percentages as the game does: the one the creature attacks holds it, and someone above them is about to pull. `UNIT_THREAT_SITUATION_UPDATE` tells each change. Before, only your own threat could be set, always as the holder.

### Fixed

- Tests: a frame anchored upside down (its top below its bottom, or its right edge left of its left one) has a size of 0, as in the game, instead of a negative size.
- Tests: a part stack dropped on a full stack of the same item swaps places with it, as in the game; before, nothing happened.
- Tests: an accepted quest is watched only while the "auto quest watch" setting (`autoQuestWatch`) is on, as in the game, and a watch you remove stays removed.
- Tests: `C_GuildInfo.GetMOTD()` (also named `GetGuildRosterMOTD()`) from your addon in combat is blocked, as in the game ("Interface action failed because of an AddOn"). Read the message of the day before the fight, or from `GUILD_MOTD`.
- Tests: chat windows keep what Blizzard's chat code or your addon sets (`SetChatWindowName`, `SetChatWindowSize`, colour, alpha, shown, locked, docked, saved position and size), and `GetChatWindowInfo` answers it, as in the game. Before, the setters did nothing.
- A function with two parameters of the same name and types (`function(_: number, _: string)`) no longer fails its runtime check on correct calls: the first `_` was checked against the second's value.
- `wowlua test --coverage` no longer reports the closing line of a multi-line `return f({ … })` as not run.
- `GetCursorInfo()` gives the cursor's kind (`"item"`, `"spell"`, …) and its details to the checker. It was declared without results, so strict code had to read it through `any`.
- Tests: `ClearPoint("TOPLEFT")` removes just that point, as in the game. Before, it did nothing, so a Blizzard dropdown menu that opens upward near the bottom of the screen kept both points and came out upside down.
- Tests: `IsMouseOver` measures the mouse against the frame, as in the game: its offsets (`IsMouseOver(12, -12, -24, 24)`) grow or shrink the area, and a child sticking out of its parent is not over the parent. `mock.hover(frame)` and `mock.click` put the mouse on the frame (`GetCursorPosition` follows), and `mock.hover(nil)` takes it off every frame. Before, `IsMouseOver` only asked whether the hovered frame was the frame or one of its children.
- Tests: an item put on a part stack of the same item in your bags (`C_Container.PickupContainerItem` onto it) stacks onto it, as in the game; whatever doesn't fit stays where it was. It used to stay on the cursor, so a bag sort that puts part stacks together failed in tests.
- Tests: a frame that holds a secure (protected) frame is protected too, as in the game: `IsProtected()` gives `true, false`, showing, hiding or moving it from your addon in combat is blocked, and your secure snippets may resize or move it in combat (Blizzard's spells window among them). It used to count as a plain frame.
- Tests: `C_SpellBook.PickupSpellBookItem` from your addon in combat is blocked ("Interface action failed"), as in the game; out of combat it still picks the spell up. To let players drag spells in combat, pick them up through Blizzard's secure drag template (`SecureHandlerDragTemplate` with an `_ondragstart` that returns `"spell", spellID`).
- Tests: anchoring a secure (protected) frame to a texture or a font string fails with the game's error ("Cannot anchor protected frames to regions"), as it does in the game even out of combat. It used to work in tests.
- Tests: a frame whose alpha was set from a secret value keeps reading its alpha back as a secret, as in the game, even after you set a plain alpha. Code that reads `GetAlpha()` again later (a fade from the current alpha) now fails in the test as it does in the game.
- `wowlua test --coverage` with a filter (`wowlua test --coverage Units/Threat`) no longer fails on the coverage minimums in `wowlua.json`: they are for the whole suite, and a filtered run's totals still count every file. It used to fail even when all its tests passed.
- Tests: a group member logging in or out (`member:setOnline(false)`) now sends `UNIT_CONNECTION`, for their target token too, as the game does. Only the roster update came before.
- Tests: a spell your character learns from a trainer is known afterwards, as in the game: `IsPlayerSpell` and `C_SpellBook.IsSpellKnown` say so. They kept answering as before the training.
- Tests: your addon's tooltips show in combat, as in the game. `GameTooltip:SetUnit`, `SetSpellByID`, `SetItemByID` and the other setters gave an empty tooltip in a fight because `C_RestrictedActions.CheckAllowProtectedFunctions` refused your addon on any frame; it now refuses only on a protected one (a secure button, UIParent).
- Tests: an edit box lets go of the keyboard when it is hidden (or a parent of it is), as in the game. It kept the focus, so keys pressed later in the test went into the hidden box instead of opening windows with their key bindings.

## 0.9.6 (2026-10-22)

### New

- **Collections in tests**: `mock.setMounts`, `mock.setToys` and `mock.setAppearances` give the account its mounts, toys and collected appearances, and a world profile can hold them (`mounts`, `toys`, `appearances`). The mount journal, the toy box and the wardrobe then answer from them as the game does (their filters, search, favourites and events, recorded from the game), so a collections window can be tested without stubbing every journal call. `/stdtest snapshot` records them from your character.
- Tests: choose the machine the game runs on with `"test": { "platform": "windows" }` in `wowlua.json`, or `config({ platform = "windows" })` in one test file. Key names (`GetBindingText`: "Delete" on Windows, "Del" on a Mac), graphics and sound answers and `IsMacClient`/`IsWindowsClient` follow it. The Mac stays the default.
- Tests: choose the screen the game draws on with `"test": { "screen": { "width": 2560, "height": 1440 } }` in `wowlua.json`, or `config({ screen = { ... } })` in one test file, with an optional `dpiScale` and `uiScale` ("Use UI Scale" on). UIParent's size and scale, `GetScreenWidth` and `GetPhysicalScreenSize` follow it. The default is 1920 by 1080.
- Tests: `mock.setField(t, key, value)` sets any field, `nil` included, until the test ends, and puts it back untainted as `mock.stub` does. Taking a frame or a game API away with an assignment instead leaves Blizzard's field tainted for the rest of the file.
- A new warning, W2061: `a and nil or b` (or `false` in the middle) always gives `b`, since the `nil` makes the `or` take over. Write an `if`, or `not a and b or nil` when `b` is never `nil` or `false`. It found three such bugs in real code.

### Changed

- Tests: `mock.drag(frame, dx, dy)` and `mock.sizeBy(grip, dw, dh)` move by screen units, as `GetCursorPosition` counts them: a distance in the frame's own units times its `GetEffectiveScale()`. Since UIParent now has the screen's UI scale (0.71 by default), a test that dragged by frame units moves the frame further than before: multiply its distances by the frame's effective scale.

### Fixed

- The dev console's error list: clicking an open error closes it again (it stayed open).
- Tests: `C_QuestLog.GetQuestsOnMap` lists the quests in your character's log, as in the game. It listed quests of the character wowlua was recorded with, which your test's character didn't have (no title, not in the log).
- Tests: a table you pass to `mock.fireEvent` or `mock.fireUnitEvent` (`{ isFullUpdate = true }`) reaches Blizzard's code as the game's own would, without your addon's taint. Blizzard's buff frame no longer errors in combat ("Auras cannot be accessed when secret while tainted") after a test fires `UNIT_AURA`.
- Tests: talents open at level 5 as in the game: `C_SpecializationInfo.CanPlayerUseTalentUI()` and `CanPlayerUseTalentSpecUI()` follow your character's level, so Blizzard's spells window opens its Talents tab for a character of level 5 and up. They stayed "available at level 5" whatever the level.
- Tests: a group made with `mock.group:add` is a group you made yourself, not a dungeon finder's: `IsInGroup(LE_PARTY_CATEGORY_INSTANCE)`, `IsInRaid(LE_PARTY_CATEGORY_INSTANCE)` and `GetNumGroupMembers(LE_PARTY_CATEGORY_INSTANCE)` say none. They ignored the category, so an addon picked `INSTANCE_CHAT` for its party messages and they were refused.
- Tests: `UnitFullName("player")` gives your realm as its second value, as in the game, so your addon recognizes its own messages (sent as `Name-Realm`). It gave `nil`.
- Tests: worn gear can be given its durability in a strictly typed test: `mock.player:equip(slot, { id, durability, maxDurability })` (the fields were read but not declared).
- Tests: a button of yours made with `SecureHandlerClickTemplate` runs its `_onclick` snippet when clicked (`mock.click`), as in the game. Blizzard's handler failed with "Invalid 'self' frame handle" and the snippet never ran.
- Tests: text is as wide and as tall as in the game. `GetStringWidth`, `GetUnboundedStringWidth`, wrapping, truncation and `SetTextToFit` measure each character with the game's own fonts (Friz Quadrata, Arial Narrow, Skurri and Morpheus), at the size the game draws them on your test's screen. They used a guess per character, so text-sized frames and wrapped lines were off.
- Tests: `UIParent:GetScale()` and every frame's `GetEffectiveScale()` include the screen's UI scale as in the game (0.71 on a 1920 by 1080 screen; it was 1). A frame that ignores its parent's scale (`SetIgnoreParentScale(true)`) or has no parent is now placed, sized and measured as in the game: one at UIParent's scale lines up with UIParent and its text is as wide.
- Tests: every atlas of the game is known, as the game has it (its own tables): `C_Texture.GetAtlasInfo` gives its size, file, texture coordinates, tiling and `rawSize`, `C_Texture.GetAtlasExists` tells, and a texture set with `useAtlasSize` takes its size. An atlas the game lacks gives nothing, as in the game; before, any atlas wowlua had not recorded was a 64 by 64 whole texture, and Blizzard's quest log errored ("Division by zero") on one sized 0.
- Tests: map positions match the game's (`C_Map.GetMapPosFromWorldPos` with an override map, Azeroth's world map, `GetWorldPosFromMapPos`), and the game's second Eastern Kingdoms and Kalimdor maps (1463, 1464) exist.
- Tests: `C_GameRules.IsGameRuleActive` answers every game rule as the game does, and the artifact ranks cost what they do in the game up to rank 10.
- Tests: some errors read as the game words them: a `CreateAbbreviateConfig` divisor that is not a power of ten, and infinity passed to a widget's number (on Windows "must be a finite number").
- Tests: `C_TransmogCollection.GetAppearanceSources(appearanceID)` without a category gives the appearance's sources, as in the game (it gave nothing), and a source has its item's name and quality once the item is loaded.
- Tests: a key bound to a button with `SetBindingClick` or `SetOverrideBindingClick` clicks it once, as in the game: only for the clicks it registered (by default the left button's release). It clicked on both the key's press and release, so a toggle opened and closed at once.
- Tests: a model's or actor's `GetItemTransmogInfoList()` has an entry for every slot, so Blizzard's dressing room no longer errors ("attempt to index local 'mainHandInfo'").
- Tests: `mock.slash` runs Blizzard's own slash commands (`/inspect` and the others the game keeps in its own lists), which it silently skipped: with Blizzard's UI the line is typed into the chat and sent, as you do in the game, so an unknown command also gets the game's help line.
- Tests: `GetText(token, gender)` gives the global string by its name (a woman's form for gender 3), as in the game; it gave nothing.
- Tests: `mock.spawnUnit` and `mock.setPlayer` take `healthMax` and `powerMax` as declared (they were ignored: the unit's maximum was its current health). The types now know every unit field the engine reads (`factionGroup`, `pvp`, `classification`, the creature's type and family) and every unit method (`joinGroup`, `loot`, `openTrade`, `setInspect`, `setThreat`, `useSpell`), so strict tests can use them.
- Tests: an inspected player's gear has its item ID, icon and quality (`GetInventoryItemID`, `GetInventoryItemTexture`, `GetInventoryItemQuality`), not only its link.
- `wowlua test --coverage` no longer crashes with "internal compiler error: stale handle" when code it counts (a Blizzard frame's or your addon's) is thrown away during the run.
- An internal compiler error prints where it happened when `RUST_BACKTRACE=1` is set, for the bug report.

## 0.9.5 (2026-10-21)

### Changed

- Fewer false warnings: a local in a test may reuse the name of a test or a `@beforeEach` function (W1004), since nothing calls those by name.
- Fewer false warnings: what your test files do to the game's API (taking `C_EventUtils` away and back, `_G[name] = nil`) no longer counts toward your addon's load order, so your addon's own reads of it are no longer W1034 or W1033.
- Fewer false warnings: the bundling warning W6001 now only flags code that takes a Lua file name from `debugstack` or `debuglocals`. Keeping a stack for an error report, or reading your addon's folder from it, works the same in a bundled build.
- Tests: an item from `mock.defineItem` is ready at once unless you give `cached = false`, a `delay` or `missing` (as the docs said: before, an item defined without options never loaded), and has item level 1 unless you give one.

### Fixed

- Tests: the auction house answers every query. A query sent while another was under way was never answered, so a search right after the house opened never came back.
- Tests: each `mock.defineAuctionHouse` starts a fresh auction house, so purchases, search results and posted auctions no longer carry over from an earlier test (two auctions could share an ID, and cancelling one removed both).
- Tests: changing an item key from the auction house (for a sell search) no longer changes the house's own data.
- Tests: a Shift-click counts as a modified click (`IsModifiedClick("CHATLINK")`), so Shift-click linking works as in the game.

## 0.9.4 (2026-10-21)

M9 so far: faster filtered test runs for large suites, found while
continuing SuuUI's unit tests on 0.9.3. The language server, the VS Code
extension and the milestones after M9 are planned; see the
[roadmap](spec/roadmap.md).

### Changed

- `wowlua test` with a filter loads only the test files whose path or source contains it (a test's name is written in its file), so a filtered run of a large suite takes seconds instead of loading every file's emulated client
- `wowlua test` runs `--jobs` test files at once when given (or `WOWLUA_JOBS`, `build.jobs`), else one a core

### Fixed

- `toHaveBeenCalledWith` lets a call leave out an optional argument the spied function takes (`spy("guildInvite")` for `(name: string, member: Member?)`), as a call of the function does, instead of E2007
- `wowlua test` with Blizzard's UI: wheeling the quest log no longer raises "Division by zero" (its bottom shadow's atlas size)

## 0.9.3 (2026-10-21)

M9 so far: what writing SuuUI's unit tests turned up in the test runner,
the mock library, the emulated client and the checker, and the emulator's
coverage report. The language server, the VS Code extension and the
milestones after M9 are planned; see the [roadmap](spec/roadmap.md).

### New

- The emulator's coverage report ([docs/emulator-coverage.md](docs/emulator-coverage.md)), generated by `cargo xtask coverage-report` and kept current by `cargo xtask check`: which API functions and widget methods the engine models, answers from recordings or from the documentation, how its answers compare with the client's, the rules still unverified, the known gaps and the in-game probes that would close them (S4)
- `wowlua test` takes several filters (`wowlua test Core/Util UI/Bar`): the files and tests matching any of them run
- `mock.group:remove(unit)` and `mock.group:leave()`; `C_PartyInfo.LeaveParty()` leaves the group a round trip later ("You leave the group.", `GROUP_LEFT`, the roster)
- E2060: `local a, b: any, any = …` (a local after an annotated one named after a type) is an error, not a third local named `any`

### Changed

- `wowlua check` type-checks the test side too (test files, mocks and helpers), as `wowlua test` compiles it
- `mock.errors()` lists the errors since the running test began (its `@beforeEach` hooks included), not every error since the file started
- `mock.click` sends the clicks the button registered for (`RegisterForClicks`, the XML's `registerForClicks`), down then up, as the client does: by default only the left button's up click, so a secure action button acting on the key down (`ActionButtonUseKeyDown`) acts
- `unit:setName` tells `UNIT_NAME_UPDATE` for the unit's tokens when the name changes (`{ quiet = true }`: not)

### Fixed

- `wowlua test` with one test file that does not compile shows its errors and runs the other files (the file fails the run unless a filter leaves it out) instead of running nothing
- `mock.fake` and `mock.stub` put the field back with its taint: a global the client set is secure again after the test, so Blizzard's code reading it later no longer runs tainted (and errors on secret values)
- The matchers compare two secrets by the values inside them (`toHaveBeenCalledWith(unit, mock.secret(true))`)
- `expect(nil).never.toBeShown()` (and the other widget matchers on a value that is not a widget) fails with "expected a frame, got nil" instead of an internal error
- `mock.stub` takes any table, widgets and objects included, without a cast
- A ticker due exactly at the end of `mock.advanceTime` runs in that advance, whatever fractions the clock moved by before
- `@cases` values are typed by the test's parameters, as arguments are (a literal fits a literal union)
- A module's local that only test files read through `import internal` is no longer reported unused (W1003)
- The emulated client: `OnSizeChanged` runs when a frame's size follows its anchors (a `SetAllPoints` child of a resized frame), `SetAlphaFromBoolean` sets the alpha, `SetVerticalScroll` and `SetHorizontalScroll` run `OnVerticalScroll` and `OnHorizontalScroll`, `SetIgnoreParentScale` works (`IsIgnoringParentScale`, the effective scale), `IsIgnoringParentAlpha` reads `SetIgnoreParentAlpha`, `SetTexture(file)` clears the texture's atlas, and a secret text's width is secret

## 0.9.2 (2026-10-20)

M9 so far: what porting SuuUI (a 137k-line UI replacement) to strict wowlua
turned up in the checker, the WoW API declarations, the standard library and
the tooling, a standard library safe with secret values throughout, and the
emulated client closer to the real one. The language server, the VS Code
extension and the milestones after M9 are planned; see the
[roadmap](spec/roadmap.md).

### New

- The emulated client answers 33,075 of the 33,727 calls the client answered in its full probe, and 25 of the 29 recorded scenarios replay line for line (combat, auradeath, inspect, legacy and mail2 among them; quest, profession, death and hearth within a few lines) (S4)
- The default camelot character leads a guild, as the recorded one does (ranks and rights, roster, bank, club); `mock.joinGuild`, `leaveGuild` and `defineGuildMember` choose another (S4)
- `/stdtest engine methodprobe` asks the client every widget method of every widget kind, as the full probe does for the API (S4)
- A field named by a string key may be optional: `{ ["not"]?: boolean }`
- A function over the 60-upvalue limit reads its excess imports through the module registry, as the spec describes, instead of failing with E5001
- `wowlua:callbacks`: `registry:off(name, handler)` cancels every subscription of a function, and `offAll(owner, name?)` only an owner's subscriptions to `name`; both checked against the event type (E8420)
- `wowlua:combat`: `defer(fn, key)`: calls with one key keep the first one's place in the queue, and only the latest function runs
- `wowlua:log`: `configure({ prefix, chatLevel })` sets the chat prefix of the default sink and `print`, and the lowest level the default sink writes to chat (`"off"`: none)
- `wowlua:secret`: `readable(v)` narrows to a present value that may be looked into (`Plain`): not nil, not a secret, and not a table whose security keeps addons out
- `wowlua:slash`: `Router:add(name, command, owner?)` (checked like the definitions, E8112), `remove`, `removeAll(owner)` and `commands()`; `router(defs, { order = "added" })` lists help in the order commands were added
- `wowlua:table`: `copy(t, { shared = false })` gives every nested table a copy of its own; `keys(t, less)` sorts by a comparator
- `wowlua:table`: a stable `sort` that never raises "invalid order function", `by` (order by a key), `order`, and the binary-search helpers `insertSorted` and `searchSorted`; secret elements and keys never reach a comparator and sort last
- `wowlua:collections`: `RingBuffer`, `Heap` and `LruCache`, which store secret values without reading them
- `wowlua:utf8`: `len`, `sub`, `truncate` and `chars` by characters, not bytes; a secret string passes `truncate` whole
- `wowlua:aura`: containers take `lazy` groups (added to the engine once `setGroupMax` gives them room), `exclude` spell IDs, a group's own `spacing`, `order` and `gap`, `setGroupFilter` and `setGroupSize` (buttons resized out of combat); `aura.hasContainers()`
- Spec: the unchecked cast (`local f: T = e :: any`) and `@unchecked` next to the runtime checks
- `wowlua:events`: `watchUnit(events, handler, owner?)`: unit events on one frame of the watch's own, pointed at other units with `setUnit(unit?, unit2?)` without subscribing again (`nil` unregisters); the handler gets the event name first
- `wowlua:events`: `offAll(owner, name?)` cancels only the owner's handlers of one event or message (a watch stops watching that event)
- `wowlua:errors`: listeners get `(msg, info)` with `info = { component?, owner?, stack? }`: the stdlib component, the owner of the failing handler (events, callback registries, hooks with `opts.owner`) and the stack, taken only when an error happens
- `wowlua:timer`: `repeating(sec, fn)`, a repeating timer with `start`, `stop`, `setRunning(running)` and `isRunning()` (stopped at first; the first tick one interval after each start)
- A string-keyed field may be read-only: `{ readonly ["end"]: number }`
- E1016: an export that redeclares a module-level name (`local items = {}` then `export function items()`, a forward-declared `local f` then `export function f()`), and a name exported twice
- W1036: a module of an import cycle that reads an export without a type annotation (or a `typeof` alias) of a module of the cycle checked after it, where it is `any`, is told to annotate it there
- W1036 also for a `typeof` alias of a later module of the cycle used through a namespace import (`local c: qn.Conf`), and for an unannotated file-level local a test file's `import internal` reads; annotated locals and functions read that way have their types

### Changed

- The standard library's tests run with the addon's code tainted, as the client runs it, so secret values refuse there what they refuse in game
- `wowlua:errors`' isolated calls, `wowlua:events`' dispatch, `wowlua:hooks`' hooks and `wowlua:timer`'s callbacks allocate nothing when the callback succeeds (an event handler's dispatch made about 550 bytes of garbage before)
- `wowlua:callbacks`: a handler cancelled during a `fire` is skipped for the rest of it (handlers added during a `fire` still wait for the next one)
- `wowlua:events`: a handler cancelled during a dispatch is skipped for the rest of it, as in `wowlua:callbacks` (handlers added during a dispatch still wait for the next one)
- `wowlua:events`: `onUnit` and `@unitEvent` take any number of units, registered in pairs on frames of their own (WoW filters a frame by two units at most); E8100 is gone
- `wowlua:table`: `copy` keeps keys as they are (a frame or a table used as a key stays itself), as Blizzard's `CopyTable` does
- `wowlua:secret`: `flag` takes any value and answers by truthiness, so legacy `1`/`nil` answers work
- `wowlua:secret`: `known<T, F>(v, fallback: F): T | F`: the fallback may be of another type, so `known(name, nil)` is a `string?`
- Generic inference binds `T` through a `T | Secret<T>` parameter (the secret members through `Secret<T>`, the rest to `T`)
- `wowlua:secure`: `state` without a function only registers the driver (for an `_onstate-<name>` snippet) and hooks nothing
- `wowlua:spell`: `charges` returns `SpellChargeInfoSecret?`, the readable table the client answers, whose counts and times may be secret on restricted clients (nil when it hands back a table addons may not index)
- A module's top-level `function f()` is a hoisted local of the module in the compiled code too, as the checker already treated it, never a global; write `_G.f = f` (or assign a declared global) for a real one
- `table` (a value `type(x) == "table"` narrowed) is assignable to every map whose values are `any`: `{ [string]: any }`, `{ [number]: any }`
- A type predicate's branch that no member of its argument reaches (a second `if not issecretvalue(c) and c then` after the first returned, the `else` of a predicate every member fits) is unreachable: the argument is `never` there and nothing in it is reported; also for `canaccessvalue`

### Fixed

- An import cycle in a package no longer makes other imports `any`: standard-library modules, library packages and the modules a cycle imports keep their types, and the modules of a cycle see each other's annotated exports
- `wowlua test` names the addon's files, its test files and the runtime as the client does (`Interface/AddOns/<Name>/src/x.lua`), so `debugstack()`, `GetSourceLocation()` and error messages show the client's names; the reports still show source paths
- W4306's help names where a wowlua library's license comes from (a `LICENSE` file or the package's `license`), not only `lua-library.license-files`
- `wowlua publish` says when it reads a token `wowlua login` saved, instead of waiting without a word while the OS credential store asks to allow it
- `wowlua publish` names the packaged zips as soon as packaging finishes, not after the uploads
- `wowlua publish` without a terminal (standard input or standard error not one) never waits for an OS credential store dialog: on every platform it then reads tokens only from the environment and `config.json`, and E4201 says why and how to fix it
- `wowlua login` and `logout` without a terminal save and remove tokens in `config.json`, where a publish without a terminal finds them, instead of the OS credential store
- `wowlua test` with Blizzard's UI: an aura container (`wowlua:aura`'s `container`, or Blizzard's `AuraContainer`) made while the addon loads takes its groups without errors; the intrinsic type's scripts no longer compile as the loading addon's code, so `AddAuraGroup` no longer fails on `CancelSignal` and a secret `SetSize`
- `wowlua test` with Blizzard's UI: a Blizzard addon the addon loads with `C_AddOns.LoadAddOn` while it loads is Blizzard's secure code, as when a test loads it, not the addon's (its functions were tainted and its files named inside the addon's folder); Blizzard's scripts compile untainted whoever's code makes them
- A wowlua library inside a workspace (a member, or a path dependency in its folder) without a license of its own has the workspace root's (its `LICENSE` file or package `license`), so no W4306
- A wowlua library path dependency in a sub-folder of a package outside a workspace has the package's license too (the package counts as its own workspace)
- `export const x: T = { … }` and an annotated plain `const` have the annotation's type, not the literal's
- An exported table has the fields assigned to it after its declaration (`function chat.Hide() … end`), sealed, for importers
- A callback taking a wider parameter type no longer widens a type argument (`table.sort(rows, function(a: { ms: number }, …) … end)`)
- Namespaces, enum tables, class tables and instances are tables for `{ [any]: any }`, so `pairs(Enum.X)` and `pairs(C_X or {})` check
- A list literal fits a record with a numeric indexer and optional named fields (`{ [number]: any, talent?: boolean }`)
- An array value fits a record whose named fields are all optional and whose numeric indexer takes its elements, also in an intersection (`any[] & { jewel?: boolean }`)
- `type X = typeof(v)` is the type of `v` in the file's later annotations, for importers and for a library's consumers, instead of `any`; a type-only import of such a module orders the check
- A declaration `@if` removes no longer hides a kept declaration of the same name in its scope (alternatives for different builds, a hoisted export)
- A table's own method replaces the one `setmetatable`'s `__index` gives it
- Reading or writing a field of a function, number or boolean is E2003 with any key, not only a `.` read
- No false W1003 for a forward-declared local that a module's `function name()` defines
- E2001 names the missing or mismatched field of a record (and hints at `x?: T`), and the first missing return value; E2015 names the union member that does not fit; a function passed to `pcall` with too few arguments is E2007 naming the argument
- `canaccessvalue(v)` narrows as the complement of `issecretvalue`: its true branch drops `Secret<…>`, in an `if` and in an `and` chain
- `cond and x` with a boolean `cond` is `false | x` in an inferred local or result, so `if y then` leaves `x`, not `true | x`; so are the results `pcall` passes on (`local ok, r = pcall(f)`)
- A generic predicate (`plain<V>(v: V | Secret<V> | nil): v is V`) infers its type parameters from the argument and narrows to them
- A declared local narrowed before a loop (`n = 0` on a `number?`) keeps its narrowing in the loop when the body keeps it; `break` paths count after the loop
- Assigning a field through a narrowed annotated local (`if item then item.x = 1 end` on an `Item | false`) checks against the members it can be, not `false`
- E2005 on a local a closure reads says when the closure sees it as declared because it is assigned after its declaration (`opts = opts or {}`); E2015 on a possibly secret value points to `canaccessvalue` and `issecretvalue`
- `wowlua:aura`: `setGroupSpells` keeps the group's dispel types
- WoW API declarations: big integers (auction house prices, `GetCancelCost`, bank tab costs, order IDs) and UI sizes (`C_NamePlate.SetNamePlateSize`) are numbers, not strings
- WoW API declarations: `issecrettable`, `canaccessallvalues`, `mapvalues` and the other secret-value functions take any value, not a function
- WoW API declarations: a structure a possibly secret function returns (`C_Spell.GetSpellCooldown`, `C_ActionBar.GetActionCooldown`, charges, loss of control) is a readable table with possibly secret fields, as the client answers, not a possibly secret table: its companion `<Name>Secret` (`SpellCooldownInfoSecret`), so functions that never answer secrets keep the plain structure (`C_Spell.GetItemCooldown`, `C_LossOfControl.GetActiveLossOfControlData`)
- WoW API declarations: functions the annotations know only by name (`SelectActiveQuest`, `GetQuestLogCompletionText`, …) take and answer any values instead of none
- WoW API declarations: `GameTooltip`'s data methods (`SetInboxItem(mailIndex, attachmentIndex)`, `SetSendMailItem(index)`, …) take the parameters of their `C_TooltipInfo` function; widget methods the annotations give no results get Blizzard's documented ones (`Minimap:GetZoom()`)
- WoW API declarations: optional `hasVoice` of `JoinChannelByName`, `exclusive` of `SetTrainerServiceTypeFilter` and `isPet` of `IsSpellKnown`; `SendChatMessage`'s target takes a channel number; `C_Traits.GetConditionInfo` has its UI wrapper's parameters and `tooltipText`
- WoW API declarations: camelot declares the client's own number globals (`LE_PARTY_CATEGORY_INSTANCE`, `NUM_LE_*`, …), and globals the annotations also name as a class (`ItemLocation`, `NamePlateDriverFrame`)
- `wowlua:errors`: `call` and `wrap` are generic as the spec declares them, so `errors.call(f, …)` checks the arguments against `f` and returns its results typed, not `any`
- `wowlua:errors`: debug builds append the module ID to a report (`[core/cache]`, before the component), as the spec says
- `wowlua:events`: `onUnit` of an event the client lacks (shared flavor mode) no longer raises; its handler is never called, as with `on`
- WoW API declarations: `UnitName`, `UnitFullName`, `UnitNameUnmodified` and `UnitNameFromGUID` answer optional names and realms (`nil, nil` for a unit that does not exist)
- WoW API declarations: named frames of Blizzard's XML are typed by their mixins and widget type (`ChatFrame1: PrimaryChatFrameMixin & … & Frame` has `ApplySystemAnchor`), and the mixins are declared with the methods their Lua defines (parameters `any`)
- W8040 is not reported inside an existence test of the hooked member (`if obj.Method then hook(obj, "Method", fn) end`); W8102 still is
- The standard library is safe with secret values throughout: no module compares, indexes, measures or truth-tests a value the client may hand back as a secret, or a table whose security keeps addons out. Values only carried pass through unchanged; where an operation needs the value, it is skipped or read as unknown, as each module's chapter now says. Among the paths that raised in game before:
  - `wowlua:table`: `merge` and `defaults` walked secret tables; `contains` compared secrets
  - `wowlua:saved`: profile copies and migrations walked secret tables
  - `wowlua:serialize`: `encode` refused any value holding a secret (now encoded as nil); `decode` and `fromPrintable` of a secret string
  - `wowlua:comm` and `wowlua:version`: a secret message, target, prefix or sender
  - `wowlua:errors`: an error whose `tostring` is secret ended in "error in error handling" and was never reported
  - `wowlua:aura`: aura tables addons may not index reached `find`; a secret spell ID in a container's `spells`
  - `wowlua:unit`, `wowlua:format`, `wowlua:menu`, `wowlua:dialog`, `wowlua:launcher`, `wowlua:alert`: secret booleans in options and answers
  - `wowlua:tooltip`, `wowlua:color`, `wowlua:links`, `wowlua:media`, `wowlua:pool`, `wowlua:list`, `wowlua:frames`, `wowlua:settings`, `wowlua:slash`, `wowlua:reflect`, `wowlua:debug`: secret links, colors, names, keys, indexes, positions and conditions
- `wowlua:test` and `wowlua:mock`: a matcher, spy, snapshot or wait given a secret fails with a message saying so instead of raising inside the framework; new matcher `toBeSecret`

## 0.9.1 (2026-10-20)

M9 so far, released before the milestone closes: wowlua's emulation of the WoW
client, and the test framework that runs on it. The language server, the VS
Code extension and the milestones after M9 are planned; see the
[roadmap](spec/roadmap.md).

### New

- `wowlua-vm`, a Lua 5.1 VM in Rust: the corpus compiles to `luac`'s bytecode and lines, and the Lua 5.1 test suite passes (S1)
- WoW's Lua environment in the VM: taint and secure execution with taint reports, secret values, the client's limits and call depths (S2)
- `wowlua-engine`: the WoW API from Blizzard's documentation with its argument checks, frames, events, timers, units, auras and protected actions; Blizzard's UI code loads as secure code, and on WoW: Forever it logs in and runs without an error, as the client's does (S3)
- `wowlua test`: a package's test files run per flavor in the emulated client, with the addon loaded from its TOC as the client loads it; `test.blizzard-ui` loads Blizzard's UI first, `test.addons` other addons' folders, each as its own addon; `--watch` ([docs](docs/getting-started.md#test-it), S4)
- `wowlua:test`: tests, hooks, `@skip`, `@only`, `@flavors`, `@cases`, `describe`, typed matchers, widget matchers, snapshots (`-u` updates them), custom matchers and compile-time tests with `@expectError` (S4)
- `wowlua:mock`: units, combat, time and events of a simulated world, spies, stubs and fakes, waiting, printed output and errors, `mock.module`, user input (click, hover, drag, typing, key bindings), casting, vendors, `mock.reloadUI`, and taint assertions (`toBeBlocked`, `toBeSecure`) (S4)
- World profiles: a test file starts with the character, money, gear, bags, place, auras and units of `tests/profiles/<name>.json`; `/stdtest snapshot` records one in the client (S4)
- `import internal` reaches a module's private declarations from tests (S4)
- Coverage and reports: `wowlua test --coverage` (lines, statements, branches, functions, conditions), `test.coverage` minimums, and the `junit`, `tap`, `json`, `github`, `lcov`, `cobertura`, `coverage-json` and `html` reporters (S5)
- The three examples have tests
- The standard library's tests run in the VM, on the engine
- `-j, --jobs` (`WOWLUA_JOBS`, `build.jobs`) and `build.jobs-memory`: the worker threads of a build and the memory they may take
- Lua compile results are kept between runs in the cache (`build.compile-cache`, `build.compile-cache-size`, `WOWLUA_NO_COMPILE_CACHE`)
- The emulated client answers WoW: Forever's whole native API and its widget methods; answers come from client recordings where there are any, from models of the game's state (bags, mail, quests, auras, the group finder…) and otherwise from Blizzard's documentation (S4)
- Twenty-four recorded client scenarios replay in the engine (bank, vendor, mail, auction, taxi, talents, professions, quests, combat, death, the group finder, repair, bags, the quest log…), checked line by line against the client (S4)
- SuuUI, Ellesmere UI and ElvUI run in `wowlua test` with Blizzard's UI as in the client, ElvUI with exactly the client's errors (S4)
- `/stdtest engine answers`, `/stdtest engine fullprobe` and `/stdtest engine methodprobe` (every widget method on fresh hidden widgets) ask the client the engine's open questions; `cargo xtask ingest` turns the answers into the engine's data (S4)

### Changed

- `wowlua fmt` formats all of wowlua's syntax (S6)
- Builds parse, lower, check, lint and walk the load order's call graph on worker threads
- Builds take less memory: smaller syntax trees and IR, each source's text kept once, source maps kept encoded
- `wowlua dev` rebuilds once per batch of changes, keeps compiled files between builds, does not parse unchanged plain Lua files again and writes only the files that changed
- Runs end with a count of errors and warnings; a source line longer than 160 characters is shown as a window around its labels
- Load order: no W1033 for files whose order `build.load-order` or an XML file fixes
- Load order: a load-on-demand addon may read its dependencies' tables while it loads without W1034
- Load order: branches that run only when an argument is not a function no longer count for callers that pass one (fewer W1035)

### Fixed

- Long operator chains build without a stack overflow or quadratic time, also on Windows
- `check.lints` accepts the warning forms of name and type errors (`"W2001": "allow"`)
- `wowlua dev` ignores the output folder `build.out` names, not only `dist/`
- Field assignments of an annotated variable are checked against its declared type

## 0.9.0 (2026-10-15)

M8, the standard library: `wowlua:` modules written in wowlua, typed, and
tested in game on WoW: Forever. `examples/core-demo` uses no third-party
libraries for its events, saved variables, settings, minimap button,
keybindings and comms.

### New

- Core: `events`, `lifecycle`, `timer`, `combat`, `hooks`, `slash`, `saved`, `callbacks`, `errors`, `jobs`, `table`, `format`
- Interface: `locale` (JSON translations, a typed `L`), `pool`, `settings`, `launcher`, `frames` (with Edit Mode), `bindings`, `menu`, `list`, `dialog`, `color`, `media`, `alert`, `links` and widget themes
- Game data, secret-value aware: `secret`, `unit`, `aura`, `spell`, `item`, `tooltip`; and `secure`, `serialize`, `comm`, `version`
- `wowlua compile`: files to Lua without an AddOn folder
- WoW's Lua environment as probed in game, with W2060 for a literal zero divisor

### Changed

- Builds are about 5x faster with 2–3x less memory on large multi-flavor addons
- The runtime and the mock client are written in wowlua

### Fixed

- Variable-file folders load only in their own flavor
- Load order: shared files are not loaded by the shipping addon, and API functions do not call their arguments
- Declared table types bind exactly in generic calls

## 0.8.2 (2026-10-13)

### Fixed

- Load order: fields of global and addon tables (`ns.Tools.Run`) order files too ([#1](https://github.com/DomenikIrrgang/wowlua/issues/1))

## 0.8.1 (2026-10-13)

M7's exit criterion is met: SuuUI, a WoW: Forever addon adopted with
`wowlua init`, was published from CI to CurseForge, Wago and GitHub with one
tag push.

### Changed

- CurseForge, Wago and WoWInterface targets build unminified unless the target sets `minify`
- W6010 warns when an addon-site target publishes minified code, no longer for GitHub targets

## 0.8.0 (2026-10-13)

M7, optimization and publishing: bundled, minified and optimized release
builds, and `wowlua publish` to four addon sites.

### New

- `wowlua package`: target builds and reproducible zips
- `wowlua publish` to CurseForge, Wago, WoWInterface and GitHub
- `wowlua login` / `logout` with the OS credential store
- Bundling with segments, bundle source maps, and `wowlua trace` through maps
- Minification passes and levels, with W6010 for renaming levels on publish targets
- Inline, cache-globals and dead-code optimizations
- `build --stats`, shared bundles and script-only XML flattening
- `publish.bundle-addons`; member targets stay with their project

### Changed

- The CI template publishes on tags; `init` shows how to switch packager workflows
- Optimizations never break Lua limits

### Fixed

- `publish` tags without a git identity
- Packager token variables are read from `.env` only

## 0.7.0 (2026-10-12)

M6, the package manager. Addons built with the BigWigs packager migrate with
`wowlua init --from-pkgmeta` and build the same, checked on thirty-six addons.

### New

- Git, SVN, path and archive sources, a global cache and `wowlua.lock`
- `add`, `remove`, `update`, `fetch`, `tree`, `outdated`, `cache clean`
- Workspaces for addon suites, with generated declaration files
- `.pkgmeta` import and packager compatibility mode
- Per-flavor load orders, SavedVariables and publish IDs; external addons

### Fixed

- Shipped files compare case-insensitively, like the game's file systems
- SVN trunk libraries on a tag take the tag's revision, like the packager
- Packager parity for TOC versions, line endings and shipped files

## 0.6.0 (2026-10-11)

M5, debugging: `wowlua:debug`, `wowlua:reflect` and `wowlua:log`; profiling,
tracing and runtime checks; the in-game dev window (`/wl`); `wowlua trace`.

### New

- `wowlua:debug`, `wowlua:reflect` and `wowlua:log`
- Profiling, tracing and runtime type checks
- Dev builds with every tool built in: every function profiled, events, scripts and timers monitored, errors captured
- The dev window (`/wl`) with profiler, monitor, log, errors, watch and inspector tabs, and `/wl` chat commands
- `wowlua trace` for line-preserved builds

## 0.5.0 (2026-10-11)

M4, the type checker: basic and strict mode, generics, narrowing, flavor
guards, `wowlua lint` and `wowlua fix`.

### New

- Basic mode: inference, narrowing, assignability, arity, globals, classes, interfaces, generics, overloads
- Strict mode and `Secret<T>`
- The WoW API of every flavor, generated from Blizzard's documentation
- Declaration files and `wowlua types convert` for LuaLS annotations
- `shared` flavor mode with flavor guards (E2030)
- `wowlua lint` and `wowlua fix`

## 0.4.0 (2026-10-11)

M3: classes, widget classes, enums and decorators, and the standard-library
infrastructure.

### New

- Classes (abstract and generic), widget classes, interfaces (parsed) and enums
- Core decorators (`@global`, `@deprecated`, `@if`, `@inline`) and user decorators
- Standard-library infrastructure: `wowlua:*` modules compiled into addons and tree-shaken
- `examples/widget-demo`

## 0.3.0 (2026-10-11)

M2: lowering of syntax sugar and modules.

### New

- `+=`, `continue`, `?.`, `??`, string interpolation, bitwise operators, `//`, `const`, default parameters and destructuring
- `import` / `export` modules
- Load-order analysis of globals and addon-table fields, per flavor
- `wowlua:meta`, static conditions, `@if`, flavor split builds and XML reference rewriting

## 0.2.0 (2026-10-11)

M1: the parser and the emitter.

### New

- A parser for Lua 5.1 and all of wowlua's syntax, with error recovery
- The Lua emitter, with line modes and source maps
- `.wowlua` files compile with their type annotations erased
- `wowlua check` (syntax only) and a first `wowlua fmt` for Lua 5.1

## 0.1.0 (2026-10-11)

M0, the foundation: an existing Lua addon builds and installs from a
`wowlua.json`, with generated TOCs for several flavors.

### New

- `new`, `init` (from a TOC), `build` for plain Lua projects (TOC generation, assets, `LICENSES.txt`, output validation), `install`, `uninstall`, `dev` (copy and link modes, safe installs), `clean`, `toc` and `explain`
- `.wowlua` files that are pure Lua 5.1
- `wowlua.lock` with the pinned flavor table
- Release binaries for macOS, Linux and Windows on GitHub Releases

