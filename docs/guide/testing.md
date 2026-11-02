# Testing your addon

`wowlua test` runs your addon in an emulated WoW client: a Lua 5.1 VM with
WoW's Lua, taint and secret values, the game's API and, if you want,
Blizzard's own UI. Tests spawn units, click buttons, type into edit boxes,
move time forward and compare screenshots, with no game client, and a whole
suite runs in seconds.

```console
$ wowlua test
 PASS src/math.wowlua (camelot, 0.07s)
   ✓ add example
 PASS tests/math.test.wowlua (camelot, 0.07s)
   ✓ doubles [1, 2]
   ✓ doubles [2, 4]
   ✓ doubles [-3, -6]
 PASS tests/party.test.wowlua (camelot, 0.07s)
   ✓ starts in Stormwind with a target
   ✓ Jaina accepts the invitation
 PASS tests/screen.test.wowlua (camelot, 0.27s)
   ✓ the target frame
 PASS tests/target.test.wowlua (camelot, 0.08s)
   ✓ shows the target's name and level
 PASS tests/time.test.wowlua (camelot, 0.08s)
   ✓ ticks every second
   ✓ waits for the item's data
 PASS tests/tracker.test.wowlua (camelot, 0.07s)
   ✓ starts at zero
   ✓ records kills
      Tested 12 passed, 0 failed, 0 skipped (7 files) in 0.38s
```

That is the run of the examples on this page.

## A first test

Test files end in `.test.wowlua` and live in `tests/`, or next to your code in
`src/`. They are never part of a build. Import the test decorators and
`expect` from `wowlua:test`, the controls of the game world from
`wowlua:mock`, and your own modules with a leading `/`, which names a module
under `src/`.

Here is a module to test:

```wowlua title=src/tracker.wowlua
export class Tracker
    private kills: number = 0

    function record(count: number = 1)
        self.kills += count
    end

    function total(): number
        return self.kills
    end

    function reset()
        self.kills = 0
    end

    function startTicker(onTick: () -> ())
        C_Timer.NewTicker(1, onTick)
    end
end
```

And its tests:

```wowlua title=tests/tracker.test.wowlua
import test, beforeEach, expect from "wowlua:test"
import Tracker from "/tracker"

local tracker: Tracker

@beforeEach
local function setup()
    tracker = Tracker.new()
end

@test("starts at zero")
local function startsAtZero()
    expect(tracker:total()).toBe(0)
end

@test("records kills")
local function recordsKills()
    tracker:record(2)
    tracker:record()
    expect(tracker:total()).toBe(3)
end
```

Before the tests run, the runner loads the whole addon as the game would: in
its load order, XML included. Then it fires `ADDON_LOADED`, `PLAYER_LOGIN`
and `PLAYER_ENTERING_WORLD`. By the time a test starts, your login code has
run and your saved variables are in place. The tests of one file share that
client, and every file starts with a fresh one. `@beforeAll`, `@afterEach`
and `@afterAll` are the other hooks.

> [!TIP]
> Matchers are typed. `expect(count).toBe("3")` is a compile error when
> `count` is a number, and a spy's `toHaveBeenCalledWith` is checked against
> the parameters of the function it spies on.

```wowlua fails=E2001
import test, expect from "wowlua:test"

@test("counts")
local function counts()
    local count = 3
    expect(count).toBe("3")
end
```

## Set up the world

`wowlua:mock` drives the emulated client. Each change to the world fires the
events the client fires for it, in the client's order, so your addon reacts as
it would in the game. Take a frame that shows the target:

```wowlua title=src/target.wowlua
import event from "wowlua:events"

export const frame = CreateFrame("Frame", "MyAddonTarget", UIParent)
frame:SetSize(160, 24)
frame:SetPoint("CENTER")

export const title = frame:CreateFontString(nil, "OVERLAY")
title:SetFont("Fonts\\FRIZQT__.TTF", 12, "")
title:SetPoint("CENTER")

@event("PLAYER_TARGET_CHANGED")
local function onTargetChanged()
    if UnitExists("target") then
        title:SetText(`{UnitName("target")} ({UnitLevel("target")})`)
    else
        title:SetText("")
    end
end
```

A test spawns a unit, targets it and reads the frame:

```wowlua title=tests/target.test.wowlua
import test, expect from "wowlua:test"
import * as mock from "wowlua:mock"
import title from "/target"

@test("shows the target's name and level")
local function showsTarget()
    local hogger = mock.spawnUnit({ name = "Hogger", level = 11, reaction = "hostile" })
    mock.player:target(hogger)        -- PLAYER_TARGET_CHANGED
    expect(title).toHaveText("Hogger (11)")
    expect(UnitHealth("target")).toBeSecret()

    mock.player:target(nil)           -- PLAYER_TARGET_CHANGED again
    expect(title).toHaveText("")

    mock.player:enterCombat()         -- PLAYER_REGEN_DISABLED
    expect(InCombatLockdown()).toBe(true)
end
```

The API answers as the flavor's client does. On camelot, a unit's health
reaches addons as a secret value, so the test checks that it is one with
`toBeSecret()`. Tests run tainted, like your addon, so `expect` never reads
what a secret hides.

The controls you reach for most:

- Units: `mock.spawnUnit`, `unit:damage`, `unit:heal`, `unit:kill`,
  `unit:applyAura`, `unit:castSpell`, `mock.group:add`, and `unit:update`
  to change a unit's level, name, reaction or any other field of its data
- Powers: `unit:setPower`, and `powers` in a unit's data for every power
  its class has (`powers = { COMBO_POINTS = { current = 2 } }`)
- Shields and heal prediction: `unit:setAbsorb`, `unit:setHealAbsorb`,
  `unit:setIncomingHeal`; damage uses up a shield before health
- The character: `mock.setPlayer`, `mock.player:setMoney`,
  `mock.player:equip`, `mock.setBag`, `mock.moveTo`
- Combat: `mock.player:enterCombat`, `mock.player:leaveCombat`,
  `mock.enterInstance`
- Game data: `mock.defineItem` and `mock.defineSpell`
- Events: `mock.fireEvent` and `mock.fireUnitEvent`, with the payload checked
  against the event's documentation
- Output: `mock.printed()` for what the chat frame shows, `mock.errors()` for
  the errors your code raised
- Spies and stubs: `mock.spy`, `mock.stub`, `mock.fake`, all restored after
  each test

The whole list is in
[Tooling §16.1.4](../../spec/toolchain/tooling.md#1614-wowluamock).

## Click, type and use slash commands

Test your UI the way a player uses it. These tests are from the
[widget demo](../../examples/widget-demo/tests/window.test.wowlua) in the
wowlua repository, which builds its window from widget classes:

```wowlua fragment
@test("/wdemo opens and closes the window")
local function toggles()
    mock.slash("/wdemo")
    expect(window()).toBeShown()
    expect(window()).toHaveSize(300, 380)
    mock.slash("/wdemo")
    expect(window()).never.toBeShown()
end

@test("clicking the counter counts and fills the bar")
local function counts()
    mock.slash("/wdemo")
    local clicks = child("Clicks")
    mock.click(clicks)
    mock.click(clicks)
    expect(clicks).toHaveText("Clicks: 2")
    expect(window().progress:GetValue()).toBe(2)
end

@test("typing and Enter add a row")
local function addsRows()
    local box = editBox()
    mock.typeText(box, "hello")
    mock.pressKey("ENTER")
    expect(box).toHaveText("")
    expect(window().entries).toEqual({ "hello" })
end
```

`window()`, `child()` and `editBox()` are small helpers in that file that find
the frames. Widgets have their own matchers: `toBeShown`, `toBeVisible` (shown
along with every parent), `toHaveText`, `toHaveSize` and `toBeAnchoredTo`.
`.never` negates any matcher. `mock.hover`, `mock.drag`, `mock.wheel` and
`mock.pressBinding` cover the rest of what a player does.

## Time and waiting

Tests run on a virtual clock, so timers and `OnUpdate` code run at once and
the same way every time. Move time forward, or write a wait as straight-line
code:

```wowlua title=tests/time.test.wowlua
import test, expect from "wowlua:test"
import * as mock from "wowlua:mock"
import Tracker from "/tracker"

@test("ticks every second")
local function ticks()
    local spy = mock.spy()
    Tracker.new():startTicker(spy)
    mock.advanceTime(3.5)
    expect(spy).toHaveBeenCalledTimes(3)
end

@test("waits for the item's data")
local function itemLoads()
    mock.defineItem(19019, { name = "Thunderfury, Blessed Blade of the Windseeker" }, { delay = 0.5 })
    C_Item.RequestLoadItemDataByID(19019)
    local itemID = mock.waitFor("ITEM_DATA_LOAD_RESULT")
    expect(itemID).toBe(19019)
    expect(C_Item.GetItemNameByID(19019)).toBe("Thunderfury, Blessed Blade of the Windseeker")
end
```

`mock.waitFor` returns the event's arguments. `mock.waitFrames`,
`mock.waitSeconds` and `mock.waitUntil` wait the same way, each frame running
`OnUpdate` and the timers that are due. A wait longer than 10 virtual seconds
fails the test and says what it waited for; `config({ timeout = … })` or
`@test(name, { timeout = … })` changes the limit.

A session starts at 12:00 realm time. `mock.setTimeOfDay(21, 30)` moves the
realm's clock to 21:30 of the same day (`GetGameTime`, the calendar, and the
3D world's light), without running the timers in between.

## Snapshots and screenshots

Two matchers catch a layout that changed when you didn't mean it to.
`toMatchSnapshot` stores a frame's widget tree (shown state, size, points,
text) as text that you review in your pull requests. `toMatchScreenshot`
stores a picture of what the player sees:

```wowlua title=tests/screen.test.wowlua
import test, expect from "wowlua:test"
import * as mock from "wowlua:mock"
import frame from "/target"

@test("the target frame")
local function targetFrame()
    mock.player:target(mock.spawnUnit({ name = "Hogger", level = 11 }))
    expect(frame).toMatchSnapshot("Hogger")
    expect(frame).toMatchScreenshot("Hogger")
    expect(mock.screen).toMatchScreenshot("whole screen")
end
```

The first run writes what is missing:

- `tests/__snapshots__/screen.test.wowlua.snap`, with the widget tree under
  `# the target frame › Hogger`;
- `tests/__screenshots__/screen.test.wowlua/the-target-frame-Hogger.png` and
  `the-target-frame-whole-screen.png`.

Commit them. Later runs compare against them. A snapshot that differs fails
the test with a line diff. A screenshot fails when more than 0.1% of its
pixels differ noticeably (`toMatchScreenshot(name, { threshold = 0.1,
maxDiffRatio = 0.001 })` sets both limits), and the run writes `….actual.png`
and `….diff.png`, with the changed pixels in red, beside the stored image.
When the change is what you wanted, `wowlua test -u` takes the new snapshots
and images.

`mock.screenshot("name")` writes a picture of the screen, or of a frame with
`mock.screenshot("name", frame)`, to `dist/screenshots/` and returns its
path. From the terminal, `wowlua screenshot` runs a test file and draws what
it leaves on screen:

```console
$ wowlua screenshot tests/screen.test.wowlua --test "target frame" --frame MyAddonTarget --scale 2 -o target.png
       Wrote target.png (the target frame)
```

> [!NOTE]
> The images are drawn as the client draws them, with the textures and fonts
> of your WoW installation for the test's flavor. They are read from there
> and never copied anywhere. Without an installation, Blizzard's textures are
> drawn as magenta checkers and text in a stand-in font, so make a project's
> stored images on machines that agree on that. Drawing needs a GPU, or a
> software one (WARP on Windows, lavapipe on Linux).

Model frames and model scenes show their 3D models from your installation
too: creatures (`SetDisplayInfo`, `SetModelByCreatureDisplayID`), model
files, and characters (`SetUnit`, `SetModelByUnit`) with their race, skin,
hair and what they wear. With Blizzard's UI loaded, the character sheet, the
dressing room and unit frames' portraits show them as in the game. Without an
installation, a model frame shows nothing.

## World profiles

A world profile describes a situation once: the character, its money, gear
and bags, where it stands, the units around it and saved variables. Put it in
`tests/profiles/<name>.json`:

```json
{
  "character": { "name": "Thrall", "class": "SHAMAN", "race": "Orc", "level": 60 },
  "money": 123456,
  "items": { "159": { "name": "Refreshing Spring Water" } },
  "trained": [403, 529],
  "bags": { "0": { "slots": 16, "items": { "1": { "id": 159, "count": 5 } } } },
  "position": { "mapID": 1453, "zone": "Stormwind City", "x": 0.5, "y": 0.6 },
  "time": "21:30",
  "units": [
    { "name": "Hogger", "level": 11, "reaction": "hostile", "target": true },
    { "name": "Jaina", "lastName": "Proudmoore", "player": true, "class": "MAGE", "level": 60 }
  ]
}
```

A test file starts there with `config({ profile = "stormwind" })` at its top,
or every test file does with `test.profile` in `wowlua.json`:

```wowlua title=tests/party.test.wowlua fragment
import test, expect, config from "wowlua:test"
import * as mock from "wowlua:mock"

config({ profile = "stormwind" })

@test("starts in Stormwind with a target")
local function startsThere()
    expect(UnitName("player")).toBe("Thrall")
    expect(GetMoney()).toBe(123456)
    expect(UnitName("target")).toBe("Hogger")
end

@test("Jaina accepts the invitation")
local function invites()
    C_PartyInfo.InviteUnit("Jaina Proudmoore")
    mock.waitUntil(function() return GetNumGroupMembers() == 2 end)
    expect(UnitName("party1")).toBe("Jaina")
end
```

You don't have to write a profile by hand: take your character's world in
the game and turn it into one.

1. **Take a snapshot** With a dev build loaded, the dev window records your
   character, gear, bags, action bars, auras, target and settings, then a
   reload saves it. `/wl snapshot`
2. **Write the profile** In your addon's folder: it finds the snapshot in your
   WoW installation, leaves out your character's name and realm and writes
   `tests/profiles/raid.json`. `wowlua profile snapshot raid`

wowlua also has a profile of its own for WoW Forever: `northshire`. Name it
as you'd name yours, with `config({ profile = "northshire" })` or
`test.profile`; a `tests/profiles/northshire.json` of your own comes first.
It starts the fresh level 1 character, with 50 gold, at Northshire's start
among test NPCs of every kind, each named with its title (Clara `<Cloth
Armor Merchant>`, Garrick `<Warrior Trainer>`):

- vendors of cloth, leather, mail and shields, and weapons, selling the
  game's own items with their stats (when your WoW installation answers the
  game data), and of general goods and of reagents, a repairer and an
  innkeeper;
- a trainer of each class;
- a banker, an auctioneer with items for sale, a postmaster with letters,
  and a flight master with the Alliance's flight paths;
- an NPC with gossip, and a target dummy, a kobold, a wolf, a boar and an
  elite bear to fight.

A profile can also set the screen: `"screen": { "width": 2560, "height":
1440, "uiScale": 0.8 }` draws at 2560 by 1440 with "Use UI Scale" at 80%,
over `test.screen` in `wowlua.json`. The UI scale takes the game's slider's
values: 0.65 to 1.15 in steps of 0.01.

A unit's `level` of -1 is a boss's "??" level: `UnitLevel` answers -1, as in
the game. Your character's level goes up to your flavors' max level (60 on
WoW Forever).

Your character's faction follows its race: an Orc is of the Horde, so
`"faction"` is only for a race of either faction, like the Pandaren or WoW
Forever's Skyborne (`"race": "Skyborne", "faction": "Alliance"` is a High
Order Skyborne). Your character also knows its race's racial spells without
listing them in `trained`: `IsPlayerSpell(20572)` is true for an Orc (Blood
Fury), and a Skyborne knows Walk on Air, Elemental Insight and Wind Blessed,
plus Read Leyline of the Alliance or Skysight of the Horde.

A race can be only the classes the game allows in your flavors: a Classic Era
Orc can't be a priest, a Blood Elf can be a paladin from TBC Anniversary on,
and WoW Forever has its own pairs (an Undead paladin; a High Order Skyborne
mage, a Windshaper Skyborne shaman).

A unit has every power its class has: a druid mana, rage, energy and combo
points, a rogue energy and combo points. Mana starts full, at the class's
base mana for the level in the game's data (a level 1 mage 100 on WoW
Forever), or 100 of 100 for a unit without one; the others at a default
(rage 0 of 100, energy 100 of 100, combo points 0 of 5). `powers` sets any
of them by token, with `current` and `max`:

```json
{
  "character": { "class": "DRUID", "race": "NightElf", "level": 60, "maxPower": 2000,
                 "powers": { "ENERGY": { "current": 40 }, "COMBO_POINTS": { "current": 2 } } }
}
```

`UnitPower("player", Enum.PowerType.ComboPoints)` then answers 2, and
`UnitPower("player")` the power the character shows (a druid's mana). The
older `power`, `maxPower` and `powerType` are the shown power's. In a test,
`unit:update({ powers = { RAGE = { current = 30 } } })` or
`unit:setPower(3, { powerType = "COMBO_POINTS" })` change a power and fire
`UNIT_POWER_UPDATE` with its token.

The profile is applied before your addon loads, so the addon finds the
target already there, as after a login. A unit with `"player": true` is
another player, online and accepting your invitations: an invite brings them
into your party a moment later, so you can test party and raid frames. With
Blizzard's UI loaded (`test.blizzard-ui`), `mock.slash("/invite Jaina
Proudmoore")` does the same through the chat. Everything a profile sets can
still be changed during the test.

`wowlua check` checks the profiles too: an unknown part, a UI scale the
game's slider doesn't have, a character above the max level, a `powers`
key that isn't a power token, a faction that isn't the race's or a race
and class the game doesn't pair is E4401, and
a profile that `config` names but that doesn't exist is E4402. The same
profiles start
[debug sessions in VS Code](vscode.md#debug-in-the-emulated-client).

## One test, many cases

```wowlua title=src/math.wowlua
--- Adds two numbers.
--- ```wowlua
--- expect(add(2, 3)).toBe(5)
--- ```
export function add(a: number, b: number): number
    return a + b
end

export function double(n: number): number
    return n * 2
end
```

```wowlua title=tests/math.test.wowlua
import test, cases, expect from "wowlua:test"
import double from "/math"

@test("doubles")
@cases({ { 1, 2 }, { 2, 4 }, { -3, -6 } })
local function doubles(n: number, expected: number)
    expect(double(n)).toBe(expected)
end
```

Each case runs and reports on its own (`doubles [2, 4]`), and each is
checked against the function's parameters: a case that doesn't fit is E8120.

## Examples in doc comments run too

A ` ```wowlua ` block in a `---` doc comment, like the one on `add` above, is a
test. It runs as `add example` with the addon loaded, sees `expect` and
everything its module exports without imports, and a failure or a type error
points at the comment's line. Your documentation can't go stale. Mark a block
`wowlua norun` to only show it.

## Running tests

```console
$ wowlua test                                              # every test file, for every flavor
$ wowlua test tracker                                      # files and tests whose path or name contains "tracker"
$ wowlua test "tests/tracker.test.wowlua::records kills"   # exactly one test
$ wowlua test --flavor camelot                             # one flavor
$ wowlua test --watch                                      # again on every change
$ wowlua test -u                                           # take the new snapshots and screenshots
$ wowlua test --game-data                                  # answer from your WoW installation's data
```

Each test file runs once per flavor of your manifest, in a fresh VM;
`@flavors("camelot")` limits one test. `@skip` and `@only` pick tests while
you work.

By default a test only sees the items and spells it defines, so it gives the
same result on every machine and in CI. With `--game-data`, what a test
doesn't define (items, spells, creatures, appearances, models) comes from
the client's own data. `wowlua data pull` reads that data from your WoW
installation into wowlua's cache once (about 300 MiB a flavor; run it again
after a game patch), and then `--game-data` needs no installation. Without a
pull, it reads the installation itself. Nothing is shipped with wowlua: the
data stays on your machine.

```console
$ wowlua data pull                     # the package's flavors, from the installation wowlua finds
$ wowlua data pull --flavor camelot --wow "/Applications/World of Warcraft"
```

`--world` turns on the 3D world from your installation: the character and
the units stand on the zone's ground (`UnitPosition` answers its height),
and `wowlua screenshot --world` draws the terrain, its buildings, trees and
props behind the UI, as far as the game's view distance setting reaches. It is off by default, so the
places a test gives stay as they are.

The world is lit as in the game: the zone's sky, sunlight, ambient light and
fog come from your installation's light tables at the realm's time of day,
so a profile's `"time": "21:30"` or `mock.setTimeOfDay(21, 30)` shows the
evening. The zone's sky models (clouds, stars, a storm) show behind it,
and its lakes, rivers, sea and lava are drawn with their animated surfaces,
the ground under the water showing through.
Each game version has its own light, as its client does.

With the world on, units are named as in the game. A unit's name floats
over it in its reaction's color, with an NPC's title under it
(`"title": "Cloth Armor Merchant"` in a profile's unit). The game's name
settings decide which units have a name (`UnitNameInteractiveNPC`,
`UnitNameHostleNPC`, `UnitNameFriendlyPlayerName`, …). The nameplate
settings give units their nameplates (`nameplateShowEnemies`,
`nameplateShowFriendlyNpcs`, `nameplateShowFriendlyPlayers`), with
`NAME_PLATE_UNIT_ADDED` as they turn on. A unit with a nameplate has no name
over it. `unit:showNameplate()` still gives one a plate whatever the
settings.

## Coverage

`--coverage` measures which lines, branches, functions and conditions of your
`.wowlua` and `.lua` files the tests ran. The terminal shows the totals and,
for each file, the lines that never ran:

```console
$ wowlua test --coverage --reporter html
coverage: lines 97.3% (36/37), branches 100.0% (2/2), functions 83.3% (5/6), conditions 100.0% (0/0)
  src/tracker.wowlua: lines 92.9%, not run: 13
       Wrote ~/addons/MyAddon/dist/test/coverage.html
      Tested 12 passed, 0 failed, 0 skipped (7 files) in 0.40s
```

Line 13 is the body of `Tracker:reset`, which no test calls. The `html`
report is a page to work from: the files to test next, a table of every file
you can sort and filter, and each file's source with the lines that never
ran (`j` and `k` jump between them) and each branch's counts. `lcov`,
`cobertura` and `coverage-json` write the same for other tools. Minimum
percentages go in `test.coverage` of `wowlua.json`; a full run below one
fails. In VS Code, *Run with Coverage* shows the same in the gutter.

## Benchmarks

A `@bench` function in a test file is a benchmark. `wowlua test` leaves it
out; `wowlua bench` calls it many times, in the build your `release` profile
optimizes and in the same build with every optimization off, on the same Lua
5.1 the tests run on, and shows both times. Here is a module worth timing:

```wowlua title=src/totals.wowlua
--!strict
export type Kill = { target: string, damage: number }

export function totalDamage(kills: Kill[]): number
    local sum = 0
    for _, kill in ipairs(kills) do
        sum = sum + kill.damage
    end
    return sum
end
```

And a benchmark of it:

```wowlua title=tests/totals.test.wowlua
import bench from "wowlua:test"
import totalDamage from "/totals"
import type Kill from "/totals"

local kills: Kill[] = {}
for i = 1, 50 do
    kills[i] = { target = "Hogger", damage = i * 10 }
end

@bench("total damage of 50 kills")
local function total()
    totalDamage(kills)
end
```

```console
$ wowlua bench
    Building MyAddon (camelot): optimized as `release` and unoptimized
     Running benchmarks (camelot)

tests/totals.test.wowlua (camelot)
  benchmark                          unoptimized               release  speed-up
  total damage of 50 kills        3.35 µs ± 1.5%        1.39 µs ± 1.5%  2.41x
```

Here the release build turned the `ipairs` loop into a numeric loop, as the
module is in strict mode and `kills` is an array. Each time is the median
time of a call, with how far the runs spread around it. The two builds take turns, so a busy machine slows both alike, and the
numbers are stable enough to compare from one run to the next. Your
`@beforeAll` hooks run once and `@beforeEach` and `@afterEach` around each
benchmark, so a benchmark sees the same world as your tests. `--profile`
times the optimizations of another profile, `--samples` and `--batch-ms`
measure longer, and `--json` prints the results for a script to compare. A
benchmark that fails fails the run.

The times are those of wowlua's Lua 5.1, not of the game: how much faster
the optimized build is says what an optimization buys, not what a frame in
WoW costs. For that, record in the game with the dev window (see
[Debugging](debugging.md)).

## In CI

Tests need no game, so they run wherever wowlua does. The `github` reporter
turns failures into annotations on your pull request; `junit`, `tap` and
`json` write files for other systems, to `dist/test/` unless `--output` says
otherwise. `wowlua publish` runs your tests before it uploads anything.

```yaml
- uses: DomenikIrrgang/wowlua/.github/actions/setup-wowlua@main
- run: wowlua test --locked --coverage --reporter github,junit
```

Everything a test can do is in
[Tooling §16.1](../../spec/toolchain/tooling.md#161-test-runner-wowlua-test).
