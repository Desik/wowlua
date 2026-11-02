# 16. Developer Tooling

## 16.1 Test runner (`wowlua test`)

Tests run outside the game, in wowlua's **emulation of the WoW client**
([WoW client emulation](emulator.md)): a Lua 5.1 VM written in Rust with WoW's
Lua differences, taint and secret values, and an engine with the flavor's WoW
API, optionally with Blizzard's own UI code. Like the game it has no `io`,
`os`, `require` or file access. The framework ships with wowlua; tests need no
third-party library (decision T184).

### 16.1.1 Writing tests

| Files | Role |
|-------|------|
| `tests/**/*.test.wowlua`, `src/**/*.test.wowlua` | Test files |
| `tests/mocks/**/*.wowlua` | Project mocks. Loaded before the addon, never treated as tests. |
| other `tests/**/*.wowlua` | Test helpers, importable from test files |

Test-only files are compiled with the `test` profile and never included in
addon builds. Importing `wowlua:test` or `wowlua:mock` from a non-test file is
error E8108. In test files, `/x` names `build.src`'s `x` (the addon's
modules), and `./x` a file next to the test file (helpers under `tests/`).

```wowlua
-- tests/tracker.test.wowlua
import test, beforeEach, expect from "wowlua:test"
import * as mock from "wowlua:mock"
import Tracker from "/tracker"

local tracker: Tracker

@beforeEach
local function setup()
    mock.resetSpies()
    tracker = Tracker.new()
end

@test("records kills")
local function recordsKills()
    tracker:record(2)
    expect(tracker:total()).toBe(2)
end

@test("flushes on logout")
local function flushOnLogout()
    tracker:record()                       -- saved data is loaded: lifecycle already ran
    mock.fireEvent("PLAYER_LOGOUT")
    expect(MyAddonDB.kills).toBe(1)
end

@test("ticks every second")
local function ticks()
    local spy = mock.spy()
    tracker:startTicker(spy)
    mock.advanceTime(3.5)
    expect(spy).toHaveBeenCalledTimes(3)
end
```

### 16.1.2 What a test run loads

For each test file and flavor, the runner:

1. creates a fresh VM and installs `wowlua:mock` for that flavor,
2. loads the TOC's files up to the runtime (locale files, the runtime), then
   the project mocks (`tests/mocks/`, with what they import), then the
   manifest's `test.addons` (below),
3. loads the rest of the **whole addon** in its computed load order,
   compiled with the `test` profile (exactly as the generated TOC would load
   it, XML included, as the addon's code),
4. loads the test file after the helpers it imports (its top-level code
   runs),
5. unless the file opted out, fires the lifecycle: `ADDON_LOADED` for each
   of the `test.addons` loaded and then for the addon, `PLAYER_LOGIN`, and
   `PLAYER_ENTERING_WORLD` (`isInitialLogin = true`),
6. runs `@beforeAll`, then each test with its `@beforeEach` / `@afterEach`
   hooks, then `@afterAll`.

A test file opts out of step 5 with `config({ lifecycle = "manual" })` from
`wowlua:test` at its top level, and then drives the lifecycle itself with
`mock.login()`. Tests within one file share the VM. Each file starts fresh.
An error while loading (a step above) fails the file, Blizzard's code's
included (with `test.blizzard-ui`: it runs without an error in the client,
so the engine must too). An error that reaches
the error handler during a test (an event handler's, a timer's) fails that
test, unless the test read it with `mock.errors()` (the errors since the
test began, its `@beforeEach` hooks included). An error the client
drops without a report (a `secureexecuterange` callback's: no error handler
hears it, the next entries still run) fails the test or the file too, though
`mock.errors()` does not list it: the code failed. The addon's
`Bindings.xml` is read before its files load, as the client does.

Every file loads with the name the client gives an addon's file,
`Interface/AddOns/<Name>/` and its path in the AddOn folder
(`Interface/AddOns/MyAddon/src/core/tracker.lua`), the test files, the
runtime and the `test.addons`' files too: `debugstack()`,
`GetSourceLocation()` and error messages show it (error messages as Lua
shortens a longer name: `...` and its last 52 bytes). The runner's reports
show the source paths instead (`src/core/tracker.wowlua:14`), a shortened
name's too when only one of the addon's files ends that way.

`test.addons` (manifest §9.18) names other addons' folders, a test's
neighbours as the client would load them: each folder is an addon named
after it, read from the TOC the client picks for the flavor
(`<Name>_Mainline.toc` and the flavor's other suffixes, else `<Name>.toc`),
its files loaded as its own code with its own addon table (`local name, ns =
...`), its `Bindings.xml` read. They load in the listed order, each after
those of its `## Dependencies` / `## RequiredDeps` the list has (and after
the Blizzard addons it depends on, with `test.blizzard-ui`). An addon whose
`AllowLoadGameType` / `ExcludeLoadGameType` / `AllowLoad` leaves out the
flavor's game is skipped. A `## LoadOnDemand: 1` addon is known but not
loaded: `C_AddOns.LoadAddOn(name)` loads it (its dependencies, its files,
then its `ADDON_LOADED`; its files' errors reach the error handler).
`C_AddOns.GetNumAddOns`, `GetAddOnInfo`, `GetAddOnMetadata`,
`IsAddOnLoaded` and `IsAddOnLoadOnDemand` see each of them, in load order,
and the addon itself last, as its generated TOC describes it (its `Title`,
`Version`, `Notes`, `X-…` fields), with or without `test.addons`;
`GetAddOnInfo`'s reason is `"DEMAND_LOADED"` for a load-on-demand addon
not loaded yet (decision T354). `GetFileIDFromPath` answers nil for a path
into an addon folder that is not there, or to a file its folder lacks
(any case, with or without its extension; the addon's own files are not
looked at), and with the installation at hand for a game file the
client's storage lacks; a game file there answers its ID (decision T355).
Their SavedVariables (`## SavedVariables`, `## SavedVariablesPerCharacter`)
survive `mock.reloadUI()` as the addon's do. An error while one loads fails
the file, naming the addon. Coverage counts only the addon's own files.

### 16.1.3 `wowlua:test`

| Export | Purpose |
|--------|---------|
| `@test(name?, opts?)` | Marks a test function (top level); the name defaults to the function's; `opts.timeout` (virtual seconds) for its waits |
| `@beforeEach`, `@afterEach`, `@beforeAll`, `@afterAll` | Hooks, per file |
| `@skip(reason?)`, `@only` | Test selection |
| `@flavors("standard", …)` | Run only for these flavors |
| `config({ lifecycle: "auto" \| "manual", timeout?, profile?, platform?, screen? })` | Per-file runner settings (`profile`: the world profile; `platform`: `"mac"` or `"windows"`, the machine the client runs on, decision T236; `screen`: `{ width, height, dpiScale?, uiScale? }`, the screen it draws on, decision T237; all read before anything loads) |
| `describe(name, fn)` | Grouping for runtime-generated tests (`test(name)(fn)` inside `fn`) |
| `expect(v)` | Matchers: `toBe`, `toEqual` (deep), `toBeNil`, `toBeTruthy`, `toBeFalsy`, `toBeInstanceOf`, `toContain`, `toHaveLength`, `toBeCloseTo`, `toThrow`, `toHaveBeenCalled`, `toHaveBeenCalledWith`, `toHaveBeenCalledTimes`, `toMatchSnapshot`, `toBeSecret`; for widgets `toBeShown`, `toBeVisible`, `toHaveText`, `toHaveSize`, `toBeAnchoredTo`, `toMatchScreenshot` ([Debugger §18.12.5](debugger.md#18125-in-tests)). Negation with `expect(v).never.toBe(x)`. Type-checked (below). Expected values may hold wildcards (decision T261): `anything` matches any value but `nil`, `ofType(kind)` any value whose `type()` is `kind`, in `toEqual` (at any depth) and `toHaveBeenCalledWith`; messages name them. |
| `@cases(list)` | Runs a test once per case, the case's values as its arguments (below) |
| `@expectError(code, …)` | A compile-time test: the function must not compile, with these diagnostics (below) |
| `matcher(name, fn)` | Defines a typed matcher (below) |
| `@bench(name?)` | A benchmark, run by `wowlua bench` (M11), not by `wowlua test` |

**Typed matchers** (decision T214). `expect` is generic: `expect(x)` has the
type `Expect<T>` for the type `T` of `x`, and its matchers take `T`:
`expect(count).toBe("3")` is a type error, as is a matcher on a kind of
value it does not take (E8121: `toHaveLength` and `toContain` need a string
or a table, `toBeCloseTo` a number, `toThrow` and the spy matchers a
function). A spy keeps the type of the function it spies on, so
`toHaveBeenCalledWith(...)` checks its arguments against that function's
parameters. Matchers are called with a dot: `expect` returns a table of
them.

**Secret values.** Tests run tainted, like the addon, so `wowlua:test`
never reads a secret: messages, test names and snapshots show one as
`<secret>` (a table addons may not index as `<inaccessible table>`), and
`toBeSecret()` asserts one (`issecretvalue`). A matcher whose answer needs
a secret's value (or such a table's contents) fails, negated or not,
saying so: `toBe`, `toEqual`,
`toContain`, `toHaveBeenCalledWith` and `toHaveText` tell a secret from a
value of another type (a secret number is not `"x"`), and two secrets
apart by the values inside them (the runner looks, as the addon cannot:
`toHaveBeenCalledWith(unit, mock.secret(true))` matches a spy handed a
secret `true`); a secret against a plain value of its type they cannot
tell; `toBeTruthy` and `toBeFalsy` take a secret that is not a boolean;
`toBeNil`, `toBeSecret` and `toMatchSnapshot` take any; the other
matchers fail on a secret value or argument. Custom matchers get secrets
as they are (an answer that is secret fails). A `@cases` case may be or
hold secrets, and a test that raises a secret fails saying so.

**Parametrized tests** (decision T215). `@cases` gives a test its cases;
each runs and is reported on its own, named after the test and the case's
values (`doubles [2, 4]`), and each is checked against the test function's
parameters (E8120 when a case does not fit):

```wowlua
@test("doubles")
@cases({ { 1, 2 }, { 2, 4 }, { -3, -6 } })
local function doubles(n: number, expected: number)
    expect(double(n)).toBe(expected)
end
```

**Waiting** (decision T216). A test may wait for the world, written as
straight-line code: `mock.waitFor(event, predicate?)` (the event's arguments
are returned), `mock.waitFrames(n)`, `mock.waitSeconds(s)` and
`mock.waitUntil(fn)`. A test runs as a coroutine; waiting advances the virtual
clock and the frames (60 a second, each running `OnUpdate` and the due
timers), so a test stays deterministic and runs instantly. A
wait longer than `timeout` virtual seconds (default 10, `config({ timeout })`
or `@test(name, { timeout })`) fails the test, naming what it waited for.

```wowlua
@test("loads the item first")
local function itemLoads()
    tooltip:showItem(19019)
    local itemID = mock.waitFor("ITEM_DATA_LOAD_RESULT")
    expect(itemID).toBe(19019)
    expect(tooltip.frame).toHaveText("Thunderfury, Blessed Blade of the Windseeker")
end
```

**Widget matchers** (decision T217): `toBeShown()` (`IsShown`),
`toBeVisible()` (shown with every parent), `toHaveText(text)` (a font string,
an edit box, a button's text, or a frame with one font string), `toHaveSize(w,
h)`, `toBeAnchoredTo(other, point?, relativePoint?)`, and
`expect(GameTooltip).toHaveLines({ … })` for a tooltip's left and right texts,
line by line. A value that is not a widget fails them, negated or not
(`expect(nil).never.toBeShown()`: "expected a frame, got nil").

**Snapshots** (decision T218). `expect(v).toMatchSnapshot(name?)` compares a
value (serialized deterministically: keys sorted, numbers with at most two
decimals, frames as their widget tree with shown state, size, points and
text, then their regions and children) with the snapshot stored in
`__snapshots__/<test file>.snap` next to the test, keyed by the test's name
and `name` (else numbered); a missing snapshot is written, a different one
fails the test with a line diff, and `wowlua test -u` rewrites them (and,
when every test of the file ran, drops those no test matched). Snapshots
are reviewed in version control: each is a `# <key>` line and its text,
each line after `| `.

**Compile-time tests** (decision T219). `@expectError` marks a function that
must not compile: the test passes when the checker reports exactly these
diagnostics in it (by code, optionally `{ code, message }`), and the function
is never run. Libraries pin down what their types reject, the stdlib its own
diagnostics:

```wowlua
@expectError("E2001")
local function wrongArgument()
    tracker:record("two")    -- record takes a number
end
```

**Custom matchers** (decision T220). `matcher(name, fn)` adds a matcher to
`expect`, typed by `fn`'s signature: `fn(actual: T, ...): (boolean, string)`
returns whether it passed and the message; a library exports its matchers and
a test file that imports it gets them, checked like the built-in ones.

**Benchmarks and examples.** `@bench` functions in test files are run by
`wowlua bench` (§16.1.7, decision T221), not by `wowlua test`.

Code examples in `---` doc comments run as tests (decision T222). A fenced
block marked `wowlua` (```` ```wowlua ````) in a doc comment of an addon
source is one test, named after the declaration below the comment (`add
example`, then `add example 2`, …; `file example` with none); a block
marked `wowlua norun` (or `ignore`) or of another language is not. It runs
in the test client with the addon loaded, as a test file's test does, and
sees `expect` and everything its module exports (no imports written). The
build of a test run makes, for each source with examples, the test file
`<path>.examples.test.wowlua` (never written to disk): the source's lines,
blank but its examples', which keep their lines and columns (the `---`
blanked; the imports and each test's header on the first fence's line,
`end` on each closing one), and diagnostics name it by the source's path.
So a type error or a failure points at the comment's line, the language
server shows an example's errors in the source, and the reports name the
source (`src/math.wowlua`, `src/math.wowlua::add example` selects one). An
examples file that does not compile is left out of the run, as a test
file is.

### 16.1.3.1 Module internals and mocked imports

A test file may import what a module does not export (decision T223):
`import internal parse, Cache from "/tracker"` binds the module's
file-level locals and types by name, as if exported, typed as the module has
them; its exports are among them, with their export types (`import internal
M, parse` may name an `export function parse` too, decision L66); only test
files may (E8119 elsewhere). The module registers the
locals test files read at its end (their values then), in test builds only:
an addon build is unchanged.

`mock.module(namespace, replacement)` replaces a module's exports for the
rest of the test file (decision T224): `namespace` is the module's namespace
import (`import * as clock from "/clock"`), `replacement` the exports to
replace, checked against the module's export types (E8123 for one it does
not have, a type error for a mistyped one); the others stay. In a test
build the addon's files read their imports at each use, so every importer
sees the replacement (a value an importer stored before keeps the old
one).

### 16.1.4 `wowlua:mock`

`wowlua:mock` drives the emulated client ([WoW client emulation](emulator.md)):
every API function of the flavor exists, generated from Blizzard's
documentation with the client's argument checks, and reads a **simulated
game world** that tests set up and change. A change to the world fires the
events the client fires for it, in the client's order (decision T201).
Functions the flavor lacks are absent, as in the game; a function without
behaviour raises `"<name> is not mocked"` unless a test fakes it. A widget
method without behaviour follows its name: `SetX(...)` and `EnableX(...)`
store their arguments, `GetX()` returns what `SetX` stored, `IsX()` whether
its first value was true (`IsXEnabled` reads `EnableX`), and another method
that returns nothing does nothing; only a method with results and nothing
stored is not mocked.

```wowlua
@test("shows the target's name")
local function targetName()
    local hogger = mock.spawnUnit({ name = "Hogger", level = 11, reaction = "hostile", health = 666 })
    mock.player:target(hogger)        -- PLAYER_TARGET_CHANGED
    expect(MyFrame.title:GetText()).toBe("Hogger")
    hogger:damage(100)                -- UNIT_HEALTH ("target")
    mock.player:enterCombat()         -- PLAYER_REGEN_DISABLED; combat restrictions apply
    expect(issecretvalue(UnitHealth("target"))).toBe(true)
end
```

**The world.**

| Part | Controls |
|------|----------|
| Units | `mock.player`, `mock.spawnUnit(data)` returning a unit: name, GUID (generated unless given), level, class, race, sex, reaction (`"hostile"`, `"neutral"`, `"friendly"`), player or NPC, health and power (`health`, `healthMax`, `power`, `powerMax`, `powerType`: the shown power; decision T232), its powers (`powers = { RAGE = { current, max }, … }`, by token: a unit has every power its class has in the flavor table, [Manifest §9.4.1](manifest.md#941-flavor-table), each at the flavor's default unless named; it shows its class's first, the player its specialization's where the table names one, unless `powerType` says; a unit without a class has the one it shows, mana by default; decision T512), dead or alive, distance from the player (yards), place in the world (`world = { y, x, z }`: `UnitPosition`, a group member's minimap blip) and facing (`facing`, radians counter-clockwise from north; toward the player when not given). Where the player has a place (`mock.moveTo`'s `world`, or a map position), every unit has one and its distance follows the two: a unit given only a distance stands that far in front of the player, the distances change as the player moves (`mock.moveTo`) or a unit does (`unit:update{ world }`), each change told as `setDistance` tells it, and `setDistance` moves a unit along its bearing from the player (decision T515). Unit methods act and fire events: `unit:damage(n)` (the unit's damage absorbs take what they can, its health the rest), `unit:heal(n)` (its heal absorbs first), `unit:setAbsorb(n)` and `unit:setHealAbsorb(n)` (the unit's shields and heal absorbs in all: `UNIT_ABSORB_AMOUNT_CHANGED`, `UNIT_HEAL_ABSORB_AMOUNT_CHANGED`), `unit:setIncomingHeal(n, healer?)` (the heal the healer, the player unless given, casts on it: `UNIT_HEAL_PREDICTION`); the global functions (`UnitGetTotalAbsorbs`, `UnitGetTotalHealAbsorbs`, `UnitGetIncomingHeals`) and a heal prediction calculator (`UnitGetDetailedHealPrediction`, worked out by its documented modes) answer them (decision T486), `unit:kill()`, `unit:setPower(n, { frequent?, powerType? })` (its shown power, or `powerType`'s, a token or an `Enum.PowerType` value of a power it has: `UNIT_POWER_UPDATE(unit, token)`, `frequent` a regeneration tick's `UNIT_POWER_FREQUENT` alone), `unit:applyAura(data)` (`quiet = true`: already there, no `UNIT_AURA`), `unit:removeAura(id)`, `unit:castSpell(id, opts?)` (`UnitCastingInfo` / `UnitChannelInfo`, `UNIT_SPELLCAST_START` then `SUCCEEDED` and `STOP`, or the channel's; `opts.castTime` in seconds, else the spell data's, else 1.5; the order unverified), `unit:interruptCast()`, `mock.player:setMoving(moving)` (`PLAYER_STARTED_MOVING` / `PLAYER_STOPPED_MOVING`; a spell with a cast time refused meanwhile; the player also moves as the game's movement functions say, `MoveForwardStart` and the others the key bindings call, each frame at the game's speeds, running 7 yards a second, walking 2.5 after `ToggleRun`, backwards 4.5, a turn half a circle a second, the map's position and the units' distances following, decision T516), `unit:patrol(path, { speed?, loop? })`, `unit:follow({ keep?, speed? })`, `unit:approach({ keep?, speed? })`, `unit:flee({ speed? })`, `unit:moveTo(place, { speed? })` and `unit:stop()` (a unit walking on its own each frame, facing its way, its distance following: a patrol walks, 2.5 yards a second, the others run, 7; a unit's data and a world profile's take `movement = { kind, path, loop, speed, keep }`; decision T517), a unit's `actions` (in its data and a world profile's: each `at` seconds after it appears, `every` so many seconds or `within` so many yards of the player, `do` `"attack"`, swings of `damage` (10) every `speed` seconds (2) while within 5 yards as `unit:swing` swings, `"cast"`, `spell` with `castTime` as `unit:castSpell` casts it, or `"face"`, turning to the player; decision T518), ranges and facing from the places (a slot `C_ActionBar.EnableActionRangeCheck` enabled told by `ACTION_RANGE_CHECK_UPDATE` each time its range to the target changes; `player:useSpell` at a target out of the spell's range refused, "Out of range.", or behind the player for a spell that needs it in front, data `facing`, from the client's tables; decision T519), the camera behind the player (`cameraSavedDistance`, `cameraSavedPitch`, `cameraFov` as the client has them; `CameraZoomIn`, `CameraZoomOut`, the `MoveView…` holds, `FlipCameraYaw`, `SetView`, `SaveView`, `ResetView`, `NextView`, `PrevView`, `CenterCamera` move it and `GetCameraZoom` tells its distance; decision T520) and the nameplates of units at a place shown while it sees them within `nameplateMaxDistance`, where it sees them (`NAME_PLATE_UNIT_ADDED` and `_REMOVED` as they come and go; decision T521), `unit:setDistance(yards)` (`CheckInteractDistance`, `UnitInRange`, `UnitDistanceSquared`; with places, the unit moved that far along its bearing; a group member crossing `UnitInRange`'s 38 yards: `UNIT_IN_RANGE_UPDATE(unit, isInRange)` for each of its group tokens, `isInRange` secret, decision T485), `unit:setOnline(online)` (a group member logging in or out: `UnitIsConnected`, `UNIT_CONNECTION(unit, isConnected)` for each of its tokens, then `GROUP_ROSTER_UPDATE`; the event unverified, decision T249), `mock.player:learn(spells, { quiet? })` and `mock.player:unlearn(spells)` (spells the player has trained, an ID or a list: `IsSpellKnown`, `IsPlayerSpell`; `LEARNED_SPELL_IN_SKILL_LINE` for each new one unless `quiet`, then `SPELLS_CHANGED`; decision T500, the events unverified), `unit:update(data)` (the fields of its data that `data` names, changed in place, its GUID kept, each told as the client tells it: `UNIT_LEVEL`, `UNIT_NAME_UPDATE`, the health's and each power's events (`UNIT_MAXPOWER`, `UNIT_POWER_UPDATE` with its token, in the unit's order of powers; a power new to it compared with its default), `UNIT_DISPLAYPOWER` for another power shown (a new class brings its powers, those of both kept, and shows its own), `UNIT_FACTION` for the reaction, faction and PvP flag, `UNIT_CLASSIFICATION_CHANGED`, `UNIT_PORTRAIT_UPDATE` and `UNIT_MODEL_CHANGED` for the class, race and sex; `distance`, `role` and `online` as `setDistance`, `setRole` and `setOnline`; `dead` a death without a killer or alive again; the player's level as `setPlayer`'s events tell it, the player's death with `die`; decision T497, unverified), `unit:despawn()`, `caster:resurrect(target, { health? })` (the dead player is offered it: `RESURRECT_REQUEST`, Blizzard's popup, `AcceptResurrect` a round trip later alive with `PLAYER_ALIVE`; another dead unit lives at once; unverified). `mock.setPlayer(data, { events? })` sets the player's data, without events unless `events` is true: then what changed is told as the client tells it (the level: `PLAYER_LEVEL_CHANGED(old, new, true)` and `UNIT_LEVEL`; the name: `UNIT_NAME_UPDATE`; `UNIT_MAXHEALTH`, `UNIT_HEALTH`, each power's `UNIT_MAXPOWER` and `UNIT_POWER_UPDATE`, `UNIT_DISPLAYPOWER` for another power shown; decisions T325, T512); `mock.unitOf(token)` is the unit a token names (a world profile's target, say), nil when none. `mock.unitAt(index)` is the unit at that place of the world's list (0 the player; decision T494). `mock.setPet(unit?, { hunter, happiness, loyalty, xp, xpMax, actions })`: the player's pet (`pet`; `UNIT_PET`, `PET_BAR_UPDATE`, `UNIT_HAPPINESS`; Blizzard's pet frame and pet bar, `HasPetUI`, `GetPetActionInfo`, `CastPetAction`, `C_PetInfo.GetPetHappiness`, `GetPetExperience`; `pet:setStats` its stats; `nil` dismisses it; unverified). A spell the player learns from a trainer (`mock.defineTrainer`, its service bought) is known from then on: `IsPlayerSpell`, `C_SpellBook.IsSpellKnown` (decision T248). |
| The character | `mock.player:setMoney(copper)` (`PLAYER_MONEY`), `mock.player:equip(slot, item)` (`PLAYER_EQUIPMENT_CHANGED`, `UNIT_INVENTORY_CHANGED`; `GetInventoryItem…`), `mock.setBag(bag, { slots, items })` (`BAG_UPDATE`, `BAG_UPDATE_DELAYED`; `C_Container`, `C_Item.GetItemCount`), `mock.setItemCooldown(itemID, duration, remaining?)` (`BAG_UPDATE_COOLDOWN`; `C_Container.GetContainerItemCooldown`, `C_Container.GetItemCooldown`), `mock.setEquipmentSets({ { name, icon, items = { [slot] = itemID }, ignored, spec }, … })` (the saved sets, IDs from 0; `C_EquipmentSet`, worn, in the bags or lost by the gear and bags), `mock.setNewMail(senders?)` (unread mail waiting: `HasNewMail`, `GetLatestThreeSenders`), the server's bag packets: `mock.setBagOrder({ bag, … })` (the order a session's server tells the bags' updates in), `mock.setItemDataDelay(seconds)` (a split stack's data after the answer), `mock.setSortResult({ [bag] = { [slot] = { id, count, shownFirst, after } } })` (where `C_Container.SortBags` puts the items), `mock.moveTo({ mapID, zone, subZone, x, y, world, facing })` (`ZONE_CHANGED_NEW_AREA`, `ZONE_CHANGED`; `GetZoneText` and the other zone texts, `C_Map.GetBestMapForUnit`, `C_Map.GetPlayerMapPosition`, `UnitPosition`; `facing`, radians counter-clockwise from north, `GetPlayerFacing`, no event). `mock.setHardcore(active)` (on vanilla: a Classic Era hardcore realm's rules, `C_GameRules.IsHardcoreActive`; off by default). An item is its ID or `{ id, count, link, bound, locked, cooldown = { remaining, duration } }`; links are the test's, the item data's, or made like the client's. The events and their order are the client's (`/stdtest engine record`); camelot's links have its form (`|cnIQ1:|Hitem:…`). |
| Collections | The account's, the whole of each (decision T235): `mock.setMounts({ [mountID] = { spellID, name, icon, sourceType, source, description, display, mountType, modelScene, animID, visualKit, selfMount, noPreview, faction, hidden, collected, usable, favorite, steadyFlight, item } })` (the mount journal; the name and icon the spell's unless given; collected and usable unless said, `usable` whether the character can ride it, collected or not; `C_MountJournal`'s lists, info by ID and by displayed index, favourites, usability, the mount a spell or an item is), `mock.setToys({ [itemID] = { collected, usable, favorite, fanfare } })` (the toy box; the name, icon and quality the item's, the name while its data is loaded; `C_ToyBox`, `PlayerHasToy`, `C_ToyBoxInfo.HasAnyToy`), `mock.setAppearances({ collected = { sourceID, … }, favorites = { appearanceID, … } })` (the wardrobe's collected sources and favourite appearances over the flavor's recorded catalogue; every answer that tells a source or an appearance collected, the categories' counts; a source's item's name and quality while the item is loaded). They set state only: a test fires the client's events for what it models (`NEW_MOUNT_ADDED`, `NEW_TOY_ADDED`, …). As the client does (recorded, `tests/client/forever-camelot-journals.lua`): the mount list follows the collected, not collected and unusable checks (the unusable one for uncollected mounts too) and the search, by name; a change of a check sends `MOUNT_JOURNAL_SEARCH_UPDATED` and `CVAR_UPDATE("mountJournalGeneralFilters", n)` (1 collected, 2 not collected, 4 unusable) at once; any displayed mount is offered as a favourite, an uncollected one's not taken; the toy box lists collected toys only, through its collected and unusable checks and search, a check's change sending `TOYS_UPDATED` (three nils) on the next frame (its `toyBoxCollectedFilters` CVar not modelled); an appearance's favourite sends `TRANSMOG_COLLECTION_UPDATED(category + 1, source, appearance, "favorite")`, its removal also `TRANSMOG_COLLECTION_ITEM_FAVORITE_UPDATE(appearance, false)`, then `TRANSMOG_COLLECTION_UPDATED` (four nils) 0.6 s later. Unverified: where favourites and collected ones go in the lists (first), the source and type filters (not applied), a collected mount's or toy's favourite's events (`MOUNT_JOURNAL_SEARCH_UPDATED`, `TOYS_UPDATED(itemID)`). Unset, each is the recorded account's: 139 mounts with data for two, none displayed; 21 toys, none shown; the recorded wardrobe |
| Vendors | `mock.openMerchant({ name, items = { { id, price, stack, available, maxStack, usable, itemAfter } }, canRepair, repairCost, damaged, cached, repairMoneyAfter, repairCosts })` (`MERCHANT_SHOW`, the interaction frame, then `MERCHANT_UPDATE` unless `cached`; the vendor is the `npc` unit; `itemAfter`: a bought item's data that many seconds after the answer; `repairMoneyAfter`: a repair's money after that many items' durability; `repairCosts`: each worn slot's mending price, the repair cursor's `PickupInventoryItem` mending one), `mock.closeMerchant()`, `mock.sell(bag, slot)` (the user's right-click); buying, selling, buying back and repairing through the client's API (`BuyMerchantItem`, `BuybackItem`, `RepairAllItems`, `C_MerchantFrame`), answered by the server a round trip later with the recorded events |
| Auction house | `mock.defineAuctionHouse(auctioneer, { browse = { { itemKey, minPrice, totalQuantity, containsOwnerItem } }, infoReceived, commodities = { [itemID] = { { auctionID, quantity, unitPrice, timeLeftSeconds, totalNumberOfOwners, owners, numOwnerItems, containsOwnerItem, containsAccountItem } } }, items = { [itemID] = { … } }, deposit, nextAuctionID, mailSender })`, then `mock.player:interact(auctioneer)` opens it a round trip later (`AUCTION_HOUSE_SHOW`, the interaction frame). Searches, a commodity's purchase, an item's post, the player's auctions and cancelling them answer as recorded; the server's messages go one at a time (`AUCTION_HOUSE_THROTTLED_MESSAGE_SENT`), a later one queued (`…_QUEUED`) and sent when the one before is answered. With no house open, a definition starts a fresh house: what an earlier one was searched for, sold, bought and told of the player's auctions is gone, and `nextAuctionID` numbers new auctions. Item keys are new tables at every call, as the client's (decision T229). Bids, Scan all (`ReplicateItems`), favourites and a commodity's post are not emulated. |
| Inspecting | `unit:setInspect({ spec, itemLevel, items = { [slot] = { link, cached } }, received = { itemID, … }, config, traitCharacter })`: what the server knows of another player. `NotifyInspect(unit)` answers a round trip later (`INSPECT_HONOR_UPDATE`, `INSPECT_READY(guid)`), with the gear's cached links at first and the others' after (`GET_ITEM_INFO_RECEIVED`). The unit's `GetInventoryItemLink`, `GetInventoryItemID`, `GetInventoryItemTexture` and `GetInventoryItemQuality` answer from the links known (decision T231), `C_PaperDollInfo.GetInspectItemLevel` from `itemLevel`. |
| Group finder | Other groups' listings on the server: `mock.defineLFGListing({ activities = { activityID, … }, comment, name, age, playstyle, newPlayerFriendly, voiceMode, autoAccept, members = { { name, class, className, level, area, role, roles = { tank, healer, dps } }, … } })` returning a handle (the members' class by file, `WARRIOR`; `role` `"TANK"`, `"HEALER"`, `"DAMAGER"` or `"NONE"`; the leader first), found by `C_LFGList.Search` of its category and activities a round trip later, each search numbering its results anew; `mock.updateLFGListing(handle, changes)` (`LFG_LIST_SEARCH_RESULT_UPDATED(searchResultID)` when the last search found it), `mock.removeLFGListing(handle)` (delisted: `isDelisted`, the same event), `mock.lfgSearchResultID(handle)` (its ID among the last search's results, or nil), `mock.failLFGSearch(reason?)` (the next search's answer `LFG_LIST_SEARCH_FAILED(reason)`, no results). The player's own listing is made through Blizzard's listing panel, or by `mock.postLFGListing(createData)` (`C_LFGList.CreateListing`'s table: listed as from the game's panel, `LFG_LIST_ACTIVE_ENTRY_UPDATE` a round trip later; an addon's own `CreateListing` is blocked) and withdrawn by `mock.delistLFG()` (`LFG_LIST_ACTIVE_ENTRY_UPDATE(nil)`; whether there was one); its searches find it (`hasSelf`) |
| Unit tokens | Follow from relationships, as in the client: `mock.player:target(unit)`, `:focus(unit)`, `:mouseover(unit)`, `mock.player:softTarget(unit, kind?, { usableAfter? }?)` (`softenemy`, `softfriend`, `softinteract`; a soft friend's actions' usability `usableAfter` seconds later), `unit:showNameplate({ entering? })` (`nameplateN`; with `entering`, a world profile's, shown before the login it appears after `PLAYER_ENTERING_WORLD`, once Blizzard's nameplate driver has its options; its frame the client's size, `C_NamePlate.GetNamePlateSize`'s, at a spot of the screen's middle the unit picks (anchored to its parent, `WorldFrame`), the same each run, decision T488), `mock.group:add(unit, raid? | { raid?, subgroup?, quiet? })` (`partyN`; with `raid`, `raidN`; the player leading; `quiet`: in it already, untold; a home group: `IsInGroup`, `IsInRaid` and `GetNumGroupMembers` of `LE_PARTY_CATEGORY_INSTANCE` answer none), bosses (`bossN`); compound tokens (`targettarget`) resolve through them. |
| Combat and restrictions | `mock.player:enterCombat()`, `:leaveCombat()`; `mock.enterInstance(kind, { name, instanceID, difficultyID, difficultyName, maxPlayers }?)` (a restricted map; with a kind `IsInInstance`, `GetInstanceInfo`), `mock.leaveInstance()`, encounters (`mock.startEncounter({ id, name, difficultyID, groupSize, bosses })`: `ENCOUNTER_START`, the bosses on `bossN`, `INSTANCE_ENCOUNTER_ENGAGE_UNIT`; `mock.endEncounter(success)`: `BOSS_KILL`, `ENCOUNTER_END`; unverified), challenge modes, PvP matches, chat lockdown. The client's addon restrictions follow ([WoW client emulation §17.5](emulator.md#175-secret-values)): `ADDON_RESTRICTION_STATE_CHANGED`, secret API results, protected functions refused.; the server's aura states: a blow past a health threshold (below 20, 25, 35, 50 percent, above 75, 80) of the player or the target, or the player's dodge with a `procOnAvoid` spell known, tells `PET_BAR_UPDATE_USABLE` with the blow or `statesAfter` seconds later (`stateFor`: the defensive state's length), `mock.tellAuraStates()` the owed update now; a creature turning on the player in a fight tells it at the moment's end, `mock.tellTurns()` now; threat: `mob:setThreat(value, who?)` puts the player (or `who`, a group member or pet) on a creature's threat list (`nil`: off it), each unit's status from the list (the unit the creature targets holds it, else the highest: 3 while no one is above, 2 once someone is; anyone else 1 above the holder, else 0; the raw % of the holder's threat, the scaled % of what pulls it, 110% in melee and 130% beyond), `UNIT_THREAT_LIST_UPDATE(mob)` and `UNIT_THREAT_SITUATION_UPDATE(unit)` for each unit whose situation changed (the player alone as recorded: `true, 3, 100, 255, value`) |
| Time | `GetTime` and `C_Timer` on a virtual clock: `mock.advanceTime(sec)`; `OnUpdate` per simulated frame: `mock.nextFrame()`, `mock.advanceFrames(n, ms?)`; the client's batching of a frame's events to its end: `mock.setFrameBatching(on)`; the damage meter's next session's ID: `mock.setDamageMeterSession(id)`; a function run once just before the engine next fires an event (a world event among a server packet's): `mock.beforeEvent(name, fn)`; waiting in a test: `mock.waitFor`, `mock.waitFrames`, `mock.waitSeconds`, `mock.waitUntil` (§16.1.3) |
| Lifecycle | `mock.login()` (`ADDON_LOADED`, `PLAYER_LOGIN`, `PLAYER_ENTERING_WORLD`), `mock.loadAddon(name)`, `mock.reloadUI()` (`/reload`: the pass ends; the test runs again from the top in a fresh VM that kept only the TOC's SavedVariables, as the client writes them, logged in as a reload; `mock.reloads()` counts the passes; the file's other tests stay in the first VM; code that calls `ReloadUI()` itself ends the session there, `PLAYER_LOGOUT` and the rest, nothing of it running after, and the pass at the test's next wait) |
| Guild | The camelot character leads a guild from the start, as the recorded one (WoW Forever's full probe): "Guild", five ranks (Guild Master, Officer, Veteran, Member, Initiate), 27 members, the others offline, its club in `C_Club` (`GetGuildClubId`, `GetSubscribedClubs`, members, streams), the bank's money and tabs, the professions list. `mock.joinGuild(guild?)` (the recorded guild, or `{ name, rank, ranks = { { name, flags = { [n] = true } } }, members, motd, info, bankMoney, bankTabs }`: the player's rank index, 0 the leader; `PLAYER_GUILD_UPDATE`), `mock.leaveGuild()`, `mock.defineGuildMember({ name, lastName, guid, class, race, level, rank, zone, note, officerNote, online, status, lastOnline = { years, months, days, hours }, achievementPoints, achievementRank })` returning a handle, `mock.updateGuildMember(handle, changes)`, `mock.removeGuildMember(handle)` (each `GUILD_ROSTER_UPDATE` a round trip later). The rank's rights (`GuildControlGetRankFlags`) answer `CanGuild…`, `CanEdit…`, officer notes, the bank's withdrawals; promotions, demotions, removals, notes and the message of the day change the roster |
| Communication | `mock.receiveAddonMessage(prefix, message, chatType, sender)` (another player's), `mock.sentAddonMessages()` (`SentAddonMessage[]`: `prefix`, `message`, `chatType`, `target?`; decision S47), `mock.systemMessage(text)` (the server's `CHAT_MSG_SYSTEM`); the player hears their own group and guild messages and whispers to themselves on the next frame, for registered prefixes; chat the player sends (`C_ChatInfo.SendChatMessage`, `SendChatMessage`, typed in the chat box) gets the server's answer on the next frame: what they say, yell and emote, their party, raid, instance, guild and officer lines when in one (else `ERR_NOT_IN_GROUP`, `ERR_NOT_IN_RAID`, `ERR_GUILD_PLAYER_NOT_IN_GUILD` as `CHAT_MSG_SYSTEM`), a whisper as sent (`CHAT_MSG_WHISPER_INFORM`) to themselves (then heard, `CHAT_MSG_WHISPER`) or a player the world has, else `ERR_CHAT_PLAYER_NOT_FOUND_S` (decision T360); and the server answers the rest of what the player types, a round trip later, in Blizzard's words (decision T362): a roll (`RANDOM_ROLL_RESULT`, its own dice), the time played (`TIME_PLAYED_MSG`, the session's time standing in), away and busy (`/afk`, `/dnd`: `UnitIsAFK`, `UnitIsDND`, `PLAYER_FLAGS_CHANGED`), the ignore list (`/ignore`, `/unignore`: only a player the world has; else `ERR_IGNORE_NOT_FOUND`), a name the world does not have for `/invite`, `/ginvite` and the guild's roster commands, party commands outside a party, the guild master's `/gquit` (`ERR_GUILD_LEADER_LEAVE`), a who's results (the online players its filter finds: `GetWhoInfo`, three lines at most in chat as `WHO_LIST_FORMAT`, then `WHO_NUM_RESULTS`'s count); with players of the world (decision T363) `/invite` and the leader's `/uninvite`, `/kick` and `/promote` act on the group, with the server's refusals (`ERR_GROUP_FULL`, `ERR_ALREADY_IN_GROUP_S`, `ERR_NOT_LEADER`, `ERR_TARGET_NOT_IN_GROUP_S`); an emote is not restricted (`PerformEmote` false); WoW Forever's characters are addressed by first and last name (`unit:setName(name, lastName)`), as `/stdtest engine comm` recorded |
| Addons | `mock.defineAddon(name, { metadata, loadOnDemand, load })`: an addon the client knows (`C_AddOns.GetAddOnMetadata`, `GetAddOnInfo`, `LoadAddOn`) |
| Input | `mock.slash(line)` (with Blizzard's chat: typed into its edit box and sent with Enter, as the player does, so the client's own lookup runs: secure commands, chat types, emotes, Blizzard's commands and the help line for an unknown one; without it, `SlashCmdList`'s handler; decision T233), `mock.defineBinding(name, action, { header, runOnUp })` (from `Bindings.xml`), `mock.pressBinding(name)`, `mock.pressKey(key)` (the key's binding, as `SetBinding` bound it; a `CLICK` binding clicks its button for the clicks it registered, decision T234; `ENTER`, `ESCAPE`, `TAB` and `SPACE` go to the focused edit box first), `mock.click(frame, button?)` (a visible frame: `OnMouseDown`, `OnMouseUp`, then an enabled button's `PreClick`, `OnClick`, `PostClick`; the order unverified), `mock.hover(frame?)` (the mouse onto the frame's middle unless it is over it already, `nil` off every frame; `OnLeave`, `OnEnter`; `GetMouseFoci`; decision T257), `mock.typeText(editBox, text)` (keyboard focus, then `OnChar` and `OnTextChanged(true)` per character; `HasFocus`, `GetCurrentKeyBoardFocus`; the focus goes, with `OnEditFocusLost`, once the edit box is out of sight: hidden itself or by a parent, decision T244), `mock.drag(frame, dx, dy, frames?)` (the frame, or an overlay's parent, moves if it starts moving; the distance in screen units, as `GetCursorPosition` counts them: the frame's units times its effective scale; the mouse starts on the frame and moves with it; with `frames`, in that many steps with a frame after each, decision T264), `mock.sizeBy(grip, dw, dh)` (a resize grip's frame; screen units too), `mock.wheel(frame, delta)`, `mock.script(frame, name, ...)` (any other user action), `mock.setModifiers(...)`, `mock.setCursorPosition(x, y)` (where the mouse is: `GetCursorPosition`, screen units; `IsMouseOver` follows) |
| UI | tooltips (`mock.showTooltip(kind, data, tip?)` with Blizzard's UI code: an item `{ id }`, `{ link }`, `{ bag, slot }` or equipped `{ slot }`, a spell `{ id }`, a unit `{ unit }`; the engine's `C_TooltipInfo` gives the data from the tests' game data, Blizzard's tooltip code shows it and runs the addons' hooks; returns the lines; `mock.tooltipLines(tip?, right?)`: the lines a tooltip shows now, its left texts or with `right` its right ones, `GameTooltip` by default, decision S47); `mock.textOf(region)`: the text a font string, button or edit box shows, `""` when it has none (a secret one as it is), decision S52, menus and dialogs, Settings, the screen size, locale, CVars, other addons and libraries (`mock.provideLibrary`); `mock.frames()` (every frame, in creation order); stand-ins for Blizzard's UI code: `mock.defineTemplate(name, fn)` (`CreateFrame` with the template calls `fn(frame)`; an undefined template only warns) and `mock.defineFrameType(name, base, fn)` (an intrinsic frame type such as `EventFrame`) |
| Output | `mock.printed()` (what the chat frame shows: `print`'s lines, and with Blizzard's UI everything `DEFAULT_CHAT_FRAME:AddMessage` gets), `mock.errors()` |

**World profiles** (decision T213). A test file starts in the situation a
profile describes: `config({ profile = "stormwind" })` (a literal, read
before anything loads) or `test.profile` of the manifest names
`tests/profiles/<name>.json`, applied through the controls above before the
addon loads:

```json
{
  "character": { "name": "Thrall", "realm": "Realm", "class": "SHAMAN", "race": "Orc", "level": 60 },
  "money": 123456,
  "items": { "19019": { "name": "Thunderfury, Blessed Blade of the Windseeker", "quality": 5 } },
  "spells": { "403": { "name": "Lightning Bolt", "known": true } },
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
  "savedVariables": { "MyAddonDB": { "logins": 41 } },
  "screen": { "width": 2560, "height": 1440, "uiScale": 0.8 }
}
```

`renown` maps faction IDs to numeric renown levels, including zero (for
example, `"renown": { "2802": 16, "2507": 0 }`). It applies through
`mock.setRenown` before the addon loads; an omitted or null part leaves the
world's default renown unchanged.

`screen` is the screen the client draws on, as the manifest's `test.screen`
gives it and over it; a file's `config({ screen })` is over both (decision
T482). Its UI scale turns "Use UI Scale" on, as the client's CVars read; it
is a value of the game's UI Scale slider, 0.65 to 1.15 in steps of 0.01
(decision T508).

A unit's `level` is `UnitLevel`'s answer; -1 is a boss's "??" level, as the
client answers it. Its `powers` (the character's too) are keyed by power
token, each `{ "current": n, "max": n }`, either left out
(`"powers": { "MANA": { "current": 300, "max": 900 }, "COMBO_POINTS": {
"current": 2 } }`): `current` is `UnitPower(unit, type)`, `max`
`UnitPowerMax(unit, type)`. A key that is no power token of the flavor
table is E4401 (decision T512). `power`, `powerMax` (or `maxPower`) and
`powerType` stay the shown power's. A unit's faction follows its `race`
(decision T513): `faction` (or `factionGroup`) is for a race of either
faction (the Pandaren, the Skyborne) or a creature, and another faction than
a race's own is E4401; so is a `class` the race can't be in any of the
package's flavors (decision T513: a vanilla Orc priest). The character's level is 1 to the highest max level of
the package's flavors ([Manifest §9.4.1](manifest.md#941-flavor-table): 60 on
camelot). `position.world` is where `UnitPosition` places the character,
`{ y, x, z?, instanceID? }` (yards north, west and up, the continent's
instance ID; `z` and `instanceID` 0 by default). `channels` is
`mock.setChannels`'s data: `joined`, the channels (`{ name, fullName?,
category?, disabled?, shortcut?, instanceID?, zoneChannelID?, channelType?,
regional?, number? }`), `groupChannels`, a number (`GetNumGroupChannels`),
and `regionalService`, a boolean (`C_ChatInfo.IsRegionalServiceAvailable`;
decision T508).

A `units` entry with `"player": true` and no `group` is another player of the
world, online and accepting the player's invitations (`acceptsInvites`,
true unless the entry says false; decision T363): `/invite` brings them into
the party a moment after (a raid once converted, up to 40, the party's and
raid's frames showing them), `/ginvite` into the guild's roster, `/who`
lists them, `/ignore` and whispers know them. `mock.spawnUnit` takes
`acceptsInvites` too (false by default, so a test drives `unit:joinGroup()`
itself).

`/wl snapshot` records the character's world in the client with a dev
build ([Debugging §7.7.10](../language/07-debugging.md#7710-snapshot), then
`/reload`), and `wowlua profile snapshot <name>` writes it as
`tests/profiles/<name>.json` with the character's name and realm, other
players' names and identifying CVars scrubbed (decision T498; StdTest's
`/stdtest snapshot` and `cargo xtask profile` take the same snapshot). `character` is
`mock.setPlayer`'s data; `items` and `spells` are
`defineItem` and `defineSpell` data, over what the client's tables answer
(items cached unless `cached` is `false`); `trained` lists the spells the
character has trained by ID (`mock.player:learn`, decision T500; the
snapshot records the spellbook's), besides its race's racial spells, which
it knows without them (decision T513); equipment slots are IDs or the client's slot names
(`GetInventorySlotInfo`); a unit's `target`, `focus`, `group` (`"party"`,
`"raid"`) and `nameplate` relate it to the player, its `auras` are
applied; `mounts`, `toys` and `appearances` are `setMounts`,
`setToys` and `setAppearances` data (the snapshot records every mount and
toy, the toy box's checks opened for it and put back, and the wardrobe's
collected sources); object keys that are whole numbers become number keys. Every part
stays drivable during the test.

A profile's parts and their fields are described by
`schema/world-profile-0.1.json` (editors complete them: a profile may
name it in `$schema`, and the VS Code extension applies it to
`tests/profiles/*.json`). A check of the package (`wowlua check`, the
language server, `wowlua test`) checks every profile: E4400 when it is
no JSON object, E4401 when a part is unknown (the world would ignore
it), has the wrong type, is keyed by something other than IDs where
the schema says so, or has a UI scale off the game's slider, a
character's level outside 1 to the flavors' max level, a faction that is
not its race's or a race and class no flavor of the package pairs; a part's fields beyond the schema's pass to the
controls as they are. A profile `test.profile` or a test file's
`config({ profile })` names must be there (E4402). A debugging session
or a playground run starts in a profile by name too, or in one given as
text, and changes its world while it runs with a script of the controls
(`wowlua/world`, decision T401).

**Raw events.** `mock.fireEvent(name, ...)` and `mock.fireUnitEvent(name,
unit, ...)` fire any event the flavor documents, the payload checked against
its documentation (count, types, an enumeration's value a number, and
secrets where the documentation says);
an event the flavor does not document is an error (decision T202). A plain
table in the payload reaches the handlers as the client's would: a copy with
its fields written securely, so Blizzard's handlers reading it stay secure
(a frame or another table with a metatable is passed as it is; decision
T239).

**Game data.** Spells, items and other game data come from the tests
(decision T203): `mock.defineSpell(id, data)` (`name`, `iconID`,
`description`, `known`, `petKnown`, `cooldown`, `charges`, ranges, `cached`),
`mock.defineItem(id, data, { cached?, delay?, missing? })` (`GetItemInfo`'s
fields by name, `itemLevel` 1 unless given, a `link` made as the client
makes one unless given, decision T283; cached unless `cached` is
`false` or a `delay` or `missing` is given, and then `RequestLoadItemDataByID` fires `ITEM_DATA_LOAD_RESULT`
after `delay`, 0.2 s by default, decision T228), in the test or in `tests/mocks/`. The data
stays the test's table, so its values may be secrets. An ID the test did not define behaves as data
the client does not have: in the bags, `C_Container.GetContainerItemInfo`
answers nothing for it, as the client does while an item's data has not
arrived. Optionally, tests can run with the real client's
data: `wowlua test --game-data` answers from the WoW installation's own
tables where a test defines nothing (items, creatures, appearances, models:
decisions T321, T327; T366). `wowlua data pull` reads the flavor's game data
from the installation into a local cache once (nothing is distributed with
wowlua, M13); `--game-data` answers from that cache when there is one, so
it needs no installation then, and from the installation otherwise. Off by
default, so a test gives the same
result on every machine and in CI; a debug session and `wowlua screenshot`
show the game as the installation has it. Screenshots draw the
installation's textures either way.

**Secret values.** `mock.secret(v)` makes a secret; the restrictions above
make the API return secrets where the client does. Spies record secret
arguments as they are, and `mock.waitFor` returns a secret payload as it
is; a `waitUntil` condition or `waitFor` predicate that gives a secret
boolean fails the test (§16.1.3).

Mocking (decision T187):

- **Spies, stubs and fakes** for any function: `mock.spy(fn?)`,
  `mock.stub(t, key, fn)` (`t` any table: a module's, a widget, an object),
  `mock.fake(name, impl)` for an engine function, restored after each test
  as they were, taint included: a global the client set is secure again
  (`expect(name).toBeSecure()`), so Blizzard's code reading it later does
  not run tainted. `mock.setField(t, key, value)` sets any field (a frame or a
  game API taken away with `nil`) the same way until the test ends
  (decision T230). `mock.partial(fields)` is a fake of a record the game
  gives, with only the fields a test needs, typed by where it goes
  (`local info: BNetAccountInfo = mock.partial({ bnetAccountID = 1 })`), so
  a stub can return it; the fields are not checked (decision S42), by
  the runtime checks either (decision S64).
- **Scenario controls** set the game's state: combat, units, group, auras,
  spells, items, time, screen, locale, CVars, other addons and libraries.
- **Taint in tests.** A protected action refused because of taint fails the
  test with its taint chain ([WoW client emulation §17.4](emulator.md#174-taint-and-secure-execution)),
  unless the test expected it: `expect(fn).toBeBlocked()` asserts that
  calling `fn` is refused, and reading `mock.refusals()` marks the refusals
  so far as expected. `expect("Name").toBeSecure()` asserts that a global
  is untainted, `expect(t).toBeSecure(key)` a table's field
  (`issecurevariable`).

### 16.1.5 Running

```
wowlua test [filter…] [--flavor <f>] [-u] [--watch] [--coverage] [--reporter <r>,…] [--output <dir>] [--game-data]
```

- Each test file runs once **per manifest flavor** by default. `--flavor`
  restricts the run, and `@flavors` restricts individual tests.
- Each file and flavor runs in a fresh VM. Files run in parallel, one a
  core, or `--jobs` at once.
- `filter` keeps the test files and tests whose path or name contains it;
  several keep those that contain any of them. A file whose path and
  source contain none of them is not loaded (a test's name is written in
  its file); a filter on a name made at run time (a group's prefix, a
  case's values) finds only the files whose source holds it. A filter
  `path::name` selects exactly the test `name` of the file `path`
  (package-relative, or a path ending in it), every case of a `@cases` test
  of that name, and nothing else (decision T382).
- A test file that does not compile (its errors all in test files) is left
  out and the others run: its errors are shown and it fails the run when
  the run selects it (no filter, or one in its path); a filtered run that
  does not select it only notes it. An error in the addon, a mock or a
  helper still stops the run.
- The test build of each flavor goes to `<build.out>/.test/<flavor>/<Name>`
  (the AddOn folder as the client would load it, with the test side next to
  it); error locations in reports are the sources' (the `test` profile keeps
  lines).
- Test reporters: `pretty` (the terminal default), `junit` (XML,
  `junit.xml`), `tap` (`results.tap`), `json` (`results.json`), `github`
  (annotations, on standard output). Several may be given (or
  `test.reporters` names them); files go to `<build.out>/test/` unless
  `--output` says otherwise.
- Exit code 1 if any test fails, or if coverage is below a threshold
  (§16.1.6).

### 16.1.6 Coverage

`--coverage` measures, mapped to the `.wowlua` and `.lua` sources
(decision T188):

| Kind | Counts |
|------|--------|
| Lines and statements | Each statement executed |
| Branches | Each arm of `if`/`elseif`/`else` (the implicit "no else" too), loops entered or skipped, both outcomes of `and`, `or`, `??` and `?.` |
| Functions | Each function, method and closure called |
| Conditions | Each condition of a compound test (`a and b`, `a or b`) both true and false |

The VM counts them; the compiled code is not changed. It counts the
instructions that run and the outcomes of conditional jumps, mapped to the
sources (the `test` profile keeps lines): a line is covered when code on it
ran, statements count as lines, a branch is one outcome of a conditional
jump, a condition one on a line with several (a compound test), and the
compiler's runtime checks and generated functions are left out, as is the
`RETURN` Lua puts after a tail call, which never runs (decision T260). Code that
`@if` removes for the flavor under test counts as not applicable, and
coverage of all flavors is merged unless `--coverage-per-flavor` is given
(reports then in `<flavor>/`). `-- wowlua: coverage ignore` (the next line
with code, or the line it ends) and `-- wowlua: coverage ignore-file`
exclude code, and `test.coverage.exclude` excludes files.

Reports, any combination: a terminal summary (always, with each file's lines
that never ran), `lcov` (`lcov.info`, with branch and function data),
`cobertura` (`cobertura.xml`), `coverage-json` (`coverage.json`), and `html`
(`coverage.html`): a self-contained page under wowlua's mark, in its colours
(dark and light), drawn by its script from the counts and sources it embeds.
The index has the totals, the files to test next (the most lines not run,
the functions never called) and every file with its four percentages and
lines not run, by folder, sortable by any column, filterable by name and to
the files not fully covered. A file (`#/src/a.wowlua`, a line
`#/src/a.wowlua:L12`) has its totals, the lines that never ran as links with
next and previous ones (`j`, `k`), its functions with their calls (the ones
never called first), and its source with each line's hits and each branch's
counts ("both ways", "one way only"). In a workspace its title is the root
package's, and each file's source is read from the addon that has it.

Thresholds in `test.coverage` ([Manifest §9.18](manifest.md#918-test)) set
minimum percentages per kind; a run of the whole suite below one fails. A
run with filters (`wowlua test --coverage Units/Threat`) checks part of the
suite while its totals still count every source file, so no minimum
applies to it (decision T250).

With coverage, each test also names the addon's functions it called (by
source and the line each is defined on), for an editor to show which tests
run a function. A run's steps are a library (`wowlua_test::runs`) that
`wowlua test` and wowlua studio share: the test build of a package for its
flavors with the test files that do not compile left out, the runner's
suite, and the minimums; `run_with` tells each file's result as it finishes
and runs no more files once asked to stop. The renderer's camera that
draws screenshots is `wowlua_test::camera`, with the `camera` feature
(decision T481).

### 16.1.7 Benchmarks (`wowlua bench`)

```
wowlua bench [filter…] [--flavor <f>] [--profile <name>] [--samples <n>] [--batch-ms <ms>] [--json]
```

`wowlua bench` measures what the optimizations change
([Optimization §15.2](optimization.md#152-profile-controlled-optimizations)):
the `@bench` functions of the test files run on the embedded Lua 5.1 (the
test runner's VM, §16.1.2) in two test builds that differ in their
`optimize` settings only, and the report puts their times side by side
(decision T437).

```wowlua fragment
@bench("auras: filter")
local function filter()
    shown(auras, now)
end
```

- `@bench(name?)` marks a benchmark; the name defaults to the function's.
  `@skip` and `@flavors` apply as to tests. The function is called many
  times with no arguments, and its result is dropped.
- **The two builds.** Both are the test build (§16.1.5) with the
  `runtime-checks`, `instrument`, `assertions`, `log-level` and line mode
  of `--profile` (default `release`): one with its `optimize` settings,
  one with every optimization off. Each goes to
  `<build.out>/.test/bench-<optimized|unoptimized>-<flavor>/<Name>`.
- **What runs.** One flavor (the first, or each `--flavor`). Each test
  file loads in both builds, with its `@beforeAll` hooks run in each; each
  benchmark runs between the file's `@beforeEach` and `@afterEach` hooks,
  one benchmark and one file at a time. `filter` selects files and
  benchmarks as for `wowlua test`.
- **How it is measured** (stable enough to compare runs). A benchmark is
  called in batches: as many calls as make a batch of the unoptimized build
  last `--batch-ms` (20 by default), found by doubling, which warms both
  builds up. Then `--samples` batches of each (10 by default) run in turns,
  the builds' order swapped every other turn, each after a full garbage
  collection, so a machine that slows down slows both alike. A build's time
  is the median of its batches, per call, with the median distance of its
  batches from it as its spread (`± 1.2%`); the speed-up is the unoptimized
  time over the optimized one.
- **Report.** A table per file and flavor: each benchmark's time in both
  builds and the speed-up; `--json` prints the same as JSON (`profile`,
  `samples`, `batch_ms`, and per file each benchmark's `calls` per batch,
  `unoptimized` and `optimized` with `nanos` and `spread`, and `speedup`).
- A benchmark that raises an error is reported with it and fails the run
  (exit code 1), as does a file that does not load or compile.

## 16.2 Language server (`wowlua lsp`)

A Language Server Protocol implementation over stdio, shipped in the same
binary. It shares the parser, binder and checker with the compiler.

| Feature | Details |
|---------|---------|
| Diagnostics | All compiler diagnostics, live, per flavor and profile (both switchable from the editor) |
| Hover | Types, documentation comments, class members, `wowlua:meta` folded values |
| Completion | Locals, imports with auto-import, members, string literal unions (events, anchors, frame types), decorator names, manifest keys in `wowlua.json` |
| Navigation | Go to definition (including into `.d.wowlua` and dependencies), find references, document and workspace symbols |
| Refactoring | Rename (scope-aware), organize imports, quick fixes from diagnostic `fixes` |
| Signature help | Including overloads |
| Inlay hints | Inferred types of locals and parameters, parameter names |
| Semantic tokens | Distinguishes contextual keywords from identifiers |
| Formatting | Via the formatter (§16.3) |
| Code lens | Profiler and test shortcuts (run test, run file) |

**Configuration discovery.** For each open file, the server uses the nearest
`wowlua.json` found by walking up from the file. If that manifest belongs to a
workspace, it uses the workspace root manifest. The selected flavor and profile
are stored in the editor's settings. The server never accesses the network
unless the editor setting `wowlua.lsp.fetch` allows it. Missing dependencies
produce a diagnostic suggesting `wowlua fetch`.

**The editor's settings (decision T504).** `initialize`'s
`initializationOptions`, and `workspace/didChangeConfiguration`'s
`settings.wowlua`, may set:
- `check`: `typing` checks 100 ms after the last edit (the default),
  `pause` 600 ms after, and `save` only when a file is saved (or changed on
  disk);
- `profile`: the build profile the diagnostics follow (default `dev`);
- `threads`: the threads for per-file work (default as `--jobs`).

A changed `profile` or `threads` checks the open files' packages again.

**Auto-import (decision T506).** A name's completion offers the values,
functions, classes and enums the package's other modules (and the
libraries and standard-library modules it builds with) export, which the
file does not import yet, each with its import as an
`additionalTextEdits` edit. The edit adds the name to an import of that
module (not an `import type` or `* as` one), or else adds an
`import <name> from "<module>"` line after the file's first imports. The
module is written as an import resolves it: `/<path>` in the same
package, `<library>/<path>`, `wowlua:<name>`. Types are left out, as they
need `import type`.

`Embedded::keep_on_disk` keeps a program's embedded server's checked
modules in `$WOWLUA_HOME/cache/check/`, as `wowlua lsp --worker` does
(decision T505).

A VS Code extension (TextMate grammar plus LSP client) is part of the project.
Other editors use the LSP directly.

**Built so far (M9 S8, decisions T368, T370, T372).** `wowlua lsp` checks
a package as `wowlua check` does with its tests, the editor's unsaved texts
read over the files on disk, in two passes. 100 ms after the last edit, the
first pass type-checks the package's open files and the modules they
import (directly or not), builds nothing, skips the load order (its
diagnostics come with the second pass), and reports those files only.
Once no package was edited for 2 s, the second pass checks the whole
package as `wowlua check` does, reporting every file; after a save (its
`wowlua.json` too) or files changed on disk (not in the package's `dist/`
or hidden folders) the second pass covers every package of its workspace
(from a member's file too: every member), so those that depend on it are
checked too. It checks every flavor the
package names with the `dev` profile and applies `check.lints`. It writes no files: no
build output, and dependencies resolve as the lockfile would change while
`wowlua.lock` stays as it is; they come from the cache only. A diagnostic
carries its code, the first label's message, its notes and its help; one
about the package as a whole goes to its manifest, and a file whose
diagnostics are gone is cleared. Formatting replaces the document with
`wowlua fmt`'s result under the package's `fmt` settings (nothing when it
does not parse). The outline lists a file's top-level functions (`A.b`,
`A:c`), local functions, locals, constants, classes with their methods and
fields, interfaces, enums and type aliases, exported or not. The server
speaks LSP 3.17 with incremental document sync.

The server stays responsive and survives what goes wrong (decisions T370,
T373). Checks run in a long-lived child process of the server, the checker
(`wowlua lsp --worker`: a request a line on its input with the open
documents, its answer a line on its output: diagnostics and the open
files' definitions), one at a time, while the server answers formatting,
outline and definition requests; only go to definition in a package with
unchecked edits waits for its check. The checker keeps what an unchanged
file gives between checks: the folders' walks (as long as no folder, ignore
file or git index they read changed, and until the editor reports files
created or deleted), each file's load-order usage, `import internal`
reads and the game's parsed API (by path, `FileId` and bytes), the
repository's state, and the compiled files of full checks. It also keeps
each module's exports from the last check that checked it (decision T375),
written without that check's ids (named types by name, type parameters by
place), with what they were checked from: the module's bytes, the
package's context (declaration files, the globals files assign and
`@global` declares), and each module it imports, directly or not, by a
fingerprint of its interface (its exports, and the types it declares
without positions) where the import was checked first, else by the types
it declares, else by its bytes. A first pass takes an imported module's
exports instead of checking it while all of these are as they were, so an
edit that leaves a module's interface as it was (a function's body) checks
that module alone; modules that make `typeof` aliases are always checked.
A first pass counts every file for the globals the package assigns, as a
full check does, and checks only its files and their imports. Full checks
take nothing from it: they keep it current and stay exact. After a full
check the checker writes these module entries, with each file's globals,
to `$WOWLUA_HOME/cache/check/` (one file per package, by its manifest's
path; decision T386), and a new checker reads them before its first check
of the package, so the first pass after the editor starts or the checker
is replaced takes the unchanged modules as a running one does (compiler
§13.9). A check that
crashes, fails or runs longer than two minutes ends the checker (it is
started again for the next check) and is logged to the editor; the server
goes on with the last results. After 300 checks the checker is replaced
by a fresh one, which gives its memory back to the system, so neither
process grows. A message that is no JSON is skipped; a request whose
handler fails gets an error reply. A first pass's diagnostics replace
those of its files only; the other files keep what the last full check
found. A file below one of the editor's workspace folders is reported in
the editor's spelling of that folder (`initialize`'s `rootUri` and
`workspaceFolders`), also where it is reached through a symbolic link.

Go to definition (decision T369) answers from the last check: the checker
records, for each name it resolves, what it refers to. A local leads to its
declaration, an imported name (and the name in the `import` statement) to
the export in its module, a namespace import to the module and its member
(`M.f`) to that export, a member
(field, method, getter, setter, static, `new`, namespace member, enum
member) to where its class, interface or namespace declares it (`new` to
the constructor, else the class), a type name to its declaration, and a
global to the first script that defines it (`function Name()`,
`Name = …`) or else its declaration. A package with edits not yet checked
is checked before the answer. Declarations built into wowlua (the game's
API, Lua's and the standard library's) open from a copy the server writes
to the system's temporary directory, one folder per wowlua version. Names
in plain Lua files are found where those files are type-checked.

Hover (decision T376) answers from the last check too: for the files open
in the editor, the checker records the type of every expression (up to
120 bytes long: a table literal's fields show, not its record) and of every
name it declares (locals, parameters, functions, methods) or calls as a
method, as messages show types. The innermost one under the pointer is
shown, a name as `name: type` (a variable's type where it is read, as
narrowed there), with the `---` doc comment above what it refers to (or,
where it is declared, above the declaration; decorators between are
skipped).

Completion (decision T377) checks the text as it is at the request. The
word at the cursor (letters, digits and `_` on both sides) becomes a
placeholder name; unfinished code around it is repaired until the file
parses, trying in turn: the placeholder alone, as a call (`self.P()`), the
rest of the line left out with its open brackets closed, an `if`, `elseif`,
`while` or `for` header finished (`then end`, `do end`), and the line
replaced by `local _ = <receiver>.P` (opening a `do` where the line opened
a block). In a quoted string (one line, no escapes) the string's text
becomes the placeholder, closed if its quote is still missing (decision
T378). Nothing is completed in a comment, a long string or a number, nor
where no repair parses. The file is checked as a first pass checks it
(its imports taken from earlier checks), and where the checker meets the
placeholder it lists what could stand there: where a name is read, the
locals in scope with their types (as narrowed there), the package's
globals, the game's globals that match the word typed, and the keywords;
after `.`, the members of the value's type (a class's own, inherited and
the game's, a record's fields, a module's exports, a namespace's or an
enum's members, a class value's statics and methods; of a union, the
members every part has; private and protected ones only where they can be
read), after `:` its methods without `self` (a string's: the `string`
library); where a type is written, the type parameters, `Self`, the
built-in types, the module's and its imports' types, its namespaces, and
the game's types that match; in a string, the string literals its expected
type names (an annotated local's, a parameter's: anchor points, `EventName`;
each overload's parameter there for an overloaded call; with `==`, the
other side's type), and the game's events (with their payloads) as the
first argument of a frame's `RegisterEvent`, `RegisterUnitEvent`,
`UnregisterEvent` and `IsEventRegistered`, which the game's API types
`string`. After `@` (decision T379): the core decorators that apply to the
declaration that follows (§4.3's targets: `@script` on a method, `@global`
on a function, a class, a local or an enum, …; not `@intrinsic`, which is
the compiler's, nor `@deferred`, which is put on parameters), each with how
it is written and what it does, and where user decorators may stand the
functions and modules in scope; after `@m.`, the members of `m`; in an
event decorator's string argument (`@events.event("…")`), the game's
events. A decorator with nothing after it yet is completed as if a
function followed. The game's names match when the letters typed
appear in order with the first at the start of the name or of a word in it
(`spell` matches `GetSpellInfo`), a single letter only at the start; the
list is then incomplete, so the editor asks again as the word grows. Each
item has its kind, and its type (the game's: on `completionItem/resolve`,
with the `---` doc comment above its declaration for every item). The
completion check's diagnostics are not published; only the latest request
waits for a check (an older one is answered empty), and it runs before the
checks edits wait for.

Signature help (decision T381) shows the parameters of the call the cursor
is in, the one at the cursor highlighted. The innermost unclosed `(` before
the cursor that follows a name, `)` or `]` is the call (a grouping
parenthesis or an open table belongs to the argument around it; in a
function body inside the arguments, or past the start of a statement, there
is none), and the commas before the cursor at its level count the argument.
The completion check of that text and cursor answers it: where the checker
meets the placeholder, the innermost call whose arguments it checks gives
its forms (an overload each: `name(a: T, b: U?, ...: V) -> R`, each
parameter's place in that label). The doc comment of what the called name
refers to gives each form its summary (the text before **Parameters**) and
each parameter its line. The active form is the first with a parameter at
the cursor's argument (or a rest), the active parameter that argument's (or
the rest's). `(` and `,` trigger it; a completion and a signature request of
the same text and cursor are answered by one check. The test explorer (decision T382): `wowlua/tests` (a request of
wowlua's own, with a document's URI) answers a test file's tests from its
syntax, its open text or else the file on disk: each function a
`wowlua:test` `@test` decorates (through a named or a namespace import),
named by `@test`'s string or else the function, with its range and whether
`@skip` or `@cases` marks it (`@expectError` tests, never run, are left
out). The VS Code extension lists every `*.test.wowlua` of the workspace
in the Testing view, each with those tests, again a moment after an edit;
it runs what is chosen with `wowlua test --reporter json` in the file's
package (a file by its path, a test as `path::name`), and shows each
test's result from the report: a test is failed when a flavor fails it,
errored when its file did not load on one, its messages naming the flavor
when several ran, a failure at the line the report names; the cases of a
`@cases` test and the tests a `describe` makes appear with the results.
Debugging a test starts `wowlua debug` on its file with `filter` set to
`path::name`. Code lenses (decision T383) put "Run all tests" and "Debug"
at the top of a test file and "Run test" and "Debug" above each test,
running it as the Testing view does (its results there and at the test);
`wowlua.testCodeLens` turns them off. Run with Coverage (decision T384)
adds `--coverage` and the `coverage-json` report: each of the addon's
files covered gets its lines', branches' and functions' counts in the
Testing view's coverage, and, shown in the editor, its lines with their
hits, each branch's two sides and its functions with their calls (VS
Code's test coverage, 1.88 or newer), and keeps the `html` report for
*wowlua: Show Coverage Report* (decision T385). That command shows the
HTML report (§16.1.6) in an editor tab: the Testing view's last run with
coverage, or the newest `coverage.html` in the workspace (from `wowlua
test --coverage --reporter html`); in the Explorer's menu of a
`coverage.html`, and in the editor's title while one is open, it shows that
one, and the Test Coverage view's title has it too. In the tab the report
follows the editor's light or dark theme, its links stay in the tab, and a
line's number, or "Open in editor" on a file, opens the source at that
line beside it. The rest of the table follows.

Find references (decision T389) checks the whole workspace of the
document's package (every member, from a member's file too, so the
packages that depend on it are checked) when asked, with the editor's
texts as they are, after a waiting completion and before the checks edits
wait for; its diagnostics and definitions replace the last check's as a
full check's do. The name at the cursor leads to what it refers to (as go
to definition does), or where it is declared it is that declaration;
every name the check resolved to the same declaration is a reference, in
every file of the workspace's packages and their tests, open in the editor
or not, with the declaration when the request asks for it. A module's
top-level functions, classes and enums lead to their declaration from
their uses in the module too (they are hoisted). An import's alias
(`import a as b`, `import * as m`) is a name of its own: from it, the
references are the names spelled as it in its file; from the export, the
aliases and their uses are among them. A member that leads to its class
(one declared without a place of its own) is no reference of the class. A
dependency's file that is on disk (a workspace member, a path or cached
dependency) is reported, and opened by go to definition, where it is, and a
package that depends on a member reads that member's files with the
editor's unsaved texts.

Rename (decision T389, `prepareRename` and `rename`) gives every name the
references search finds that is spelled as the name at the cursor the new
name, as a `WorkspaceEdit` across files (the server writes none): from an
alias, that file's names spelled as it, else the declarations and the
names in every file, imports and `import a as b`'s `a` included. It is
refused, with the reason and no edit, when the new name is not a name or is
one of Lua's keywords; the name is `self`, `super` or `new`; what it refers
to is declared by wowlua (the game's API, Lua's library, the standard
library), is a module, or is declared in a file no check of the workspace
reads names in (a dependency that is no member); the name stands as a
word in a file the checker doesn't read names in (a plain Lua file that is
not type-checked, an XML file's attribute values and text); or the
workspace checked again with the rename made (over the editor's texts, not
written) has more errors of a code in a file than before (an override left
overriding nothing, E3005; a class that no longer satisfies an interface
it is used as) or its names spelled as the new name are not exactly the
renamed ones referring to the renamed declaration (the new name is taken
there). What the checker does not resolve is not found and not renamed: a
member named in a string (`self[key]`, `hooksecurefunc(t, "name")`), a
member of a type without a declaration. `prepareRename` answers from the
document and the last check: the name's range and text; nothing where no
name is; an error for a keyword, `self` and `super`, and for a name the
last check leads into wowlua's declarations.

A package that can't be loaded (decision T390) is checked no further: its
manifest, or its workspace's, does not parse, has a schema error, or names
a `package.wowlua` this wowlua does not satisfy (E4300). Its manifest gets
the diagnostic as before, and the server sends `window/showMessage`, an
error naming the package (its `package.name`, else its folder), the first
error and the manifest's file: "suuui can't be checked: this package
requires wowlua ^0.9, but this is wowlua 0.11.0 (wowlua.json)". It is sent
once for a package and problem: again when the problem changes, or comes
back after a check that loaded the package.

The VS Code extension starts `wowlua lsp` (the `wowlua.path` command) when a
`.wowlua` file opens or the workspace has a `wowlua.json`, and sends it the
open `.wowlua` and `.lua` files, each edit as the range it changed, and the
`.wowlua`, `.lua`, `.xml`, `.toc`, `wowlua.json` and `wowlua.lock` files
changed on disk; *Go to Definition* (F12) and Ctrl or Cmd + click use it,
hovering a name shows its type (a function's overloads one per line,
decision T479), its completions list as you type (and
after `.`, `:`, `@` and a quote), and in a call's parentheses its signature
help shows the parameters; *Find All References* (Shift+F12) and *Rename
Symbol* (F2, its refusals shown at the cursor) use it too, and its
`window/showMessage` is an error, warning or information message (and a
line of its output).
A server that stops by itself is started again at once, up to three times
in three minutes, then the extension asks. The extension colors `.wowlua`
and `.d.wowlua` files with a TextMate grammar of its own (decision T371):
Lua's syntax, interpolated strings with their expressions, decorators,
wowlua's contextual keywords where they are keywords (§1.2.5), declared
names, and type annotations (`name: T`, `name?: T`, `): T`, `:: T`, type
parameters) as types, with the scopes TypeScript's grammar gives (a type
parameter's `extends` a modifier, a base class or interface after `extends`
and `implements` an inherited class), so themes color them alike; its language configuration toggles `--` comments,
matches and closes brackets and quotes, and indents blocks. It makes
itself the formatter of `.wowlua` files. `wowlua.languageServer` (on by
default) turns it off; *wowlua: Restart Language Server* restarts it; the
**wowlua Language Server** output shows what it logs. Its client is
hand-written, as the rest of the extension, so it needs no build step.

### 16.2.9 What the editor draws

Five requests come from a file's syntax and its package's last check
(decision T478):

- **Semantic tokens** (`textDocument/semanticTokens/full`, the legend
  `keyword`, `type`, `class`, `function`, `method`, `parameter`,
  `property`, `decorator`, `enumMember`, `namespace`, no modifiers) give
  each name what the syntax tree makes it: a declared class, interface or
  enum `class`, a type in an annotation, a type parameter or alias `type`,
  the namespaces of a qualified type `namespace`, a declared or called
  function `function`, a method `method`, a parameter `parameter`, a field
  (`a.b`, a table constructor's or a member's name) `property`, a
  decorator's name `decorator`, an enum item `enumMember`. A name the tree
  uses as none of these is a variable and gets no token. A contextual
  keyword (§1.2.5) is a `keyword` where the parser read it as one: where
  the tree has no name at its place.
- **Folding ranges** (`textDocument/foldingRange`): statements with a body,
  functions, classes, interfaces, enums, `declare` blocks and table
  constructors over several lines, each up to the line before its last
  (the `end` or `}` stays in sight); a block comment or two line comments
  or more in a row (`comment`); top-level imports in a row (`imports`).
- **Inlay hints** (`textDocument/inlayHint`, in the range asked for): after
  the name of a `local` or a `for … in` variable declared without a type,
  `: T`, the type the last check gave it (from the hovers it records, so in
  files the editor has open). None for a local whose value is a literal or
  a function, nor for `any` or a type longer than 60 characters.
- **Quick fixes** (`textDocument/codeAction`, kind `quickfix`): a
  diagnostic's machine-applicable fixes (those `wowlua fix` applies) travel
  in its `data` (`{ "fixes": [{ "title", "edits": [{ "uri", "range",
  "newText" }] }] }`); the editor sends the diagnostics at the cursor back
  and gets one action per fix, the first preferred.
- **Code lenses** (`textDocument/codeLens`): over each function the file
  declares (`function A.b`, `A:c`, local functions, class methods with a
  body), how many names refer to it, counted in the last check's
  definitions of every checked file; its command, `wowlua.showReferences`
  with the file's URI and the name's position, shows them (the extension's
  references peek). The lenses for tests and profiling (the table above)
  are not built yet.

A sixth thing is a notification: `wowlua/checking` tells the editor when
a check of a package begins (`{ "state": "begin", "manifest": uri,
"first": true }` for the first pass of an edit, `false` for a full check)
and ends (`{ "state": "end", "manifest": uri }`, with `"failed": true`
when it failed); completion checks are not told. Editors show it as a
background job (wowlua studio's jobs).

The VS Code extension asks for all five; it asks again for hints and
lenses when a check publishes its diagnostics.

## 16.3 Formatter (`wowlua fmt`)

An opinionated formatter for `.wowlua` and `.d.wowlua`. It can optionally format
`.lua` with `fmt.lua = true`.

- Idempotent, and preserves comments (it is built on the lossless trivia kept by
  the parser).
- Configuration is deliberately small, in the manifest:
  `"fmt": { "indent": "tab" | 2 | 4, "line-width": 100, "quote-style": "double" | "single" | "preserve" }`.
  Defaults: `indent` `"tab"`, `line-width` 100, `quote-style` `"double"`
  ([Manifest §9.13](manifest.md#913-fmt)). `exclude` (globs) leaves files
  out of `wowlua fmt` of the package or a folder, such as generated ones; a
  file named on the command line is formatted (decision T387).
- `wowlua fmt --check` exits 1 if any file would change (for CI).
- Style (decision T65), following StyLua: lines are reflowed at `line-width`;
  call arguments are always parenthesized (`f "x"` becomes `f("x")`); tables
  print as `{ a = 1, b = 2 }` when they fit, otherwise one field per line with
  a trailing comma; a single table or function argument, or a function as the
  last argument, hugs the parentheses; long binary expressions break after
  the operator with a hanging indent; a function's parameters stay on its
  line when they fit, otherwise one per line (`(` and `)` on the lines
  around them), never broken inside a parameter's type; blocks are never
  kept on one line, except an empty function body (`function() end`), whose
  `end` goes on its own line when the line would be too long; at most one blank line is
  kept; `;` is written only before a statement that starts with `(`.
  Trailing comments stay on their line (after a table's or a table type's
  field, with that field), comments on their own line stay on their own
  line (one before a table stands before it and leaves the table on one
  line when it fits). wowlua's syntax (M9 S6): decorators each on their own
  line; annotations as `name: T`, `name?: T` and `name: T = default`;
  `function f<T extends X>(a: T, ...: U): R`; function types as
  `(a: A, ...B) -> R` and a results list of one `...T` without
  parentheses; table types like tables; long unions and intersections
  break before `|` / `&`; class and interface members one per line with
  their blank lines kept; enums like tables; `import`, `export`, `const`,
  `continue`, compound assignment, destructuring, `?.`, `?:`, `??` and
  `::` with single spaces; `declare` statements like the declarations they
  declare (members one per line); interpolated strings keep their text.
- Formatting never changes semantics. Every formatted file is verified to
  produce the same AST (types, decorators and declarations included); an
  idempotence suite formats every `.wowlua` file of the repository twice.

## 16.4 Trace decoder (`wowlua trace`)

Maps error messages and stack traces from bundled or minified builds back to
source locations, using the source maps of the matching build
([Code generation §14.6](codegen.md#146-source-maps)).

```
$ wowlua trace < bugsack.txt
MyAddon/MyAddon.lua:4821: attempt to index local 'a' (a nil value)
  → src/ui/window.wowlua:87 in Window:refresh   ('a' is 'self.frame')
[string "@Interface/AddOns/MyAddon/MyAddon.lua"]:5310: in function <MyAddon/MyAddon.lua:5302>
  → src/main.wowlua:22 in onLogin
```

- WoW error messages and stacks contain **line numbers only**, never columns.
  Every minification level keeps at least one line break per statement
  ([Optimization §15.4](optimization.md#154-minification-levels)), so a line
  always identifies one source statement.
- Input: pasted text, a file, or `--clipboard`. It recognizes WoW error-frame
  and BugSack formats.
- Build matching: the build id (`--build`, the `X-Wowlua-Build` TOC field,
  also included in the load errors of bundles) selects the right source maps:
  a build in `dist/` with that id, else the archive. Without one, the newest
  build in `dist/` is used. Maps of published builds
  are archived as described in [Code generation §14.6](codegen.md#146-source-maps),
  and can be uploaded as release assets (`publish.archive-maps`). That way,
  reports from users of old versions can still be decoded.
- Renamed locals are reported with their original names, looked up by line and
  minified name.
- Line-preserved builds need no map. `wowlua trace` passes their locations
  through, rewriting the output path (including any flavor directory) to the
  `.wowlua` source file, and names the enclosing function found in the source
  (`in Window:refresh`).
- Locations of other addons are left as they are.

## 16.5 Profiling reports (`wowlua prof report`)

`wowlua prof report` turns recordings exported from the dev window
([Debugging §7.7.9](../language/07-debugging.md#779-export)) into a single,
self-contained HTML page and opens it in the browser (`--no-open` only writes
it). The default output is `dist/reports/<Name>-<date>.html` (the date as
`YYYY-MM-DD`, UTC); `--out` sets another path.

- **Finding the data.** The command reads `WowluaDevDB_<Addon>.lua` of the
  current package from the SavedVariables folders of the WoW installation,
  found as for `wowlua dev` ([CLI §12.5](cli.md#125-wow-installation-integration)):
  `--wow`, `WOWLUA_WOW_PATH`, the global config, then auto-detection. Every
  client folder and every account under `WTF/Account/` is searched; only
  exports of format 1 count. When more than one has exported recordings, the
  command lists them and asks, or takes `--client` and `--account` (decision
  T179). Without a terminal to ask on, it lists them and fails. None found is
  an error that says how to export.
- **What it shows.** Every exported recording (`--recording <name>` selects
  some; a name that was not exported is an error),
  with the functions table, the call tree and flame graph when call paths
  were on, the frame timeline with the frame budget, the slow calls with
  their stacks, the garbage view and the hints found in that recording, in
  the style of the wowlua website. Locations are mapped back to source lines
  as `wowlua trace` does (§16.4), using the build id stored with each
  recording.
- **Nothing is uploaded.** The report is a local file; the command does not
  use the network.
