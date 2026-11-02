# 17. WoW Client Emulation

**Status: planned (M9).** This chapter describes the environment `wowlua test`
runs addons in ([Tooling §16.1](tooling.md#161-test-runner-wowlua-test)): a Lua
VM and a WoW engine written in Rust, faithful enough that a test catches what
the game client would raise.

## 17.1 Goals

- **Fidelity over convenience.** A test passes only if the same code would run
  in the client of that flavor. Where the emulation cannot know the client's
  behaviour, it raises a clear error instead of guessing.
- **The bugs that matter.** Taint, combat lockdown, secret values, flavor
  differences and WoW's own Lua differences are where addons break in game; the
  emulation models them, and reports them better than the game does.
- **Verified against the client.** Every behaviour is backed by the Lua 5.1
  reference, Blizzard's documentation and UI code, or an in-game probe
  (`/stdtest probe`), and the verification is automated (§17.8).
- **One binary.** The VM and the engine are part of `wowlua`, in Rust, with no
  C toolchain (decision T185).

## 17.2 The VM

A Lua 5.1 virtual machine in Rust, in its own crate (`wowlua-vm`).

- **Language.** Lua 5.1 as the client runs it: values, tables with metatables
  and every metamethod, closures and upvalues, varargs, coroutines,
  `setfenv`/`getfenv`, weak tables, `__gc` on userdata, error values and
  `pcall`/`xpcall` with tracebacks, `string.dump` absent.
- **Input.** The VM runs what the compiler produces: wowlua's output and plain
  Lua files, with the compiler's source maps, so every location the VM reports
  is a `.wowlua` (or `.lua`) line and column.
- **Standard library.** Exact Lua 5.1 behaviour: Lua patterns, `string.format`
  (including `%g` and `%q` output byte for byte), `tostring` of numbers, table
  sorting, number parsing.
- **Determinism.** Iteration order, random numbers (`math.random`,
  `fastrandom`, seeded per test), time and memory are deterministic, so a test
  run repeats exactly.
- **Instrumentation.** Coverage counters (lines, statements, branches,
  functions, conditions) and profiling are counted by the VM, not inserted into
  the code (§17.6, Tooling §16.1.6).
- **Limits.** Lua's limits apply as in the client (200 locals, 60 upvalues,
  C stack depth), so code that compiles but would not load in game fails here
  too.

Its design (§17.9): a register bytecode of its own, a tracing garbage
collector, and stackless coroutines.

## 17.3 WoW's Lua

The differences from Lua 5.1 elsewhere, as probed on WoW: Forever (decisions
S37, S38):

- **Libraries.** No `os`, `io`, `debug` or `package`; `date`, `time` and
  `difftime` as globals. The client's additions to `string`, `table` and `math`
  ([Types §2.8](../language/02-types.md#28-globals)), `bit`, and WoW's global
  aliases and helpers (`strsplit`, `strjoin`, `strtrim`, `tinsert`, `wipe`,
  `debugstack`, `debuglocals`, `debugprofilestop`, `geterrorhandler`, …).
- **Arithmetic.** `x / 0` and `x % 0` raise "Division by zero"; `math.sqrt`
  and `math.log` raise on a negative argument; `string.format("%d", x)` raises
  for an infinite `x`.
- **Limits.** The client's limits, at its thresholds, from crash-search
  probes (`/stdtest crashlimit`, separate from `/stdtest probe`): a C
  function's frame holds 8000 slots with its arguments (`unpack` and
  `string.byte` fail beyond, "too many results to unpack", "stack overflow
  (string slice too long)"); `strsplit` returns at most 4000 pieces
  ("strsplit(): Stack overflow"). The client runs its Lua with API
  assertions on and gives C functions 100 free slots: `pcall` returning
  its argument count plus 100 or more values (`xpcall` one fewer, its
  handler takes a slot; `pcallwithenv` three fewer, it pushes the function
  and its environment after them) pushes its status past them, the assertion
  `L->top < L->ci->top` fails and the game crashes. Such calls fail with
  an error naming the call (decision T194). The call depth is Lua
  5.1's: a thread's call-info array (a slot per Lua or C call) doubles
  from 8, so the first "stack overflow" comes at 16384 calls; after one,
  the array is cut back to 20000 slots and each collection may halve it
  again (the next overflow comes at 20000, then 19968, 19456 or 18432),
  and while handling one it may grow to 32768 ("error in error
  handling"). Nested C calls (`pcall`, metamethods, `loadstring`'s
  parser, which counts its syntax levels on from them) stop at 200 with
  "C stack overflow"; unlike stock Lua 5.1's, the client's coroutines
  continue their resumer's count, so nesting `coroutine.resume` stops
  there too instead of overflowing the C stack. `securecall`,
  `securecallfunction`, `securecallmethod` and the
  coroutine functions return up to the 8000 slots without crashing.
- **Per flavor.** Where flavors differ (a library function, an arithmetic
  rule), the VM follows the flavor under test. Behaviour not yet probed on a
  flavor follows the newest probed client and is listed in the emulation's
  coverage report (§17.8).

### 17.3.1 Layers

The client's in-game environment, sorted from the client itself
(`/stdtest inventory`, recorded in `crates/wowlua-vm/tests/client/`) against
Blizzard's UI code and API documentation for the build:

| Layer | What | Where wowlua has it |
|-------|------|---------------------|
| Lua runtime | Lua 5.1's libraries and the client's C extensions: the string, table, math and bit additions, the aliases (`strfind`, `tinsert`, …), `date`/`time`, `debugstack`, the secure API, and the FrameScript functions (`issecretvalue`, `securecallmethod`, `Mixin`, …) | the VM (`Vm::wow`) |
| Engine API | C functions: the documented API (`C_*` and globals) and older undocumented ones (`GetActionInfo`, …) | the engine (§17.6) |
| Blizzard's Lua | FrameXML and SharedXML: `print`, `tostringall`, `assertf`, the degree-based `sin`/`cos`, mixins, and Lua wrappers even inside `C_` namespaces (`C_Traits.GetEntryInfo`) | Blizzard's own code, loaded by the engine (§17.7) |
| GlueXML | the login and character screens' Lua | not in game; not emulated |

On WoW Forever 1.60.1 (70374): 5,037 documented C functions, 2,059
undocumented ones (151 of them the runtime's), 3,942 Lua functions, every one
defined in the published UI code for the build.

## 17.4 Taint and secure execution

The client's security model, in the VM:

- **Secure and insecure code.** Blizzard's code (the engine's Lua and, when
  loaded, FrameXML, §17.7) runs secure; addon code runs insecure, tainted with
  its addon's name.
- **Propagation.** A value written by insecure code carries that taint; secure
  code that reads a tainted value becomes tainted for the rest of its
  execution. Taint follows table fields, globals, upvalues and locals, and
  crosses coroutines as in the client.
- **The secure API.** `issecure`, `issecurevariable`, `securecall`,
  `securecallfunction`, `secureexecuterange`, `hooksecurefunc` (hooks run
  insecure without tainting the hooked call), `forceinsecure`, `scrub`, and the
  taint barriers the client has (error handlers, `__gc`).
- **Protected actions.** Protected functions and frame methods are refused
  from tainted execution: the call does nothing, the client's event fires
  with the tainting addon's name and the action, **and** the test fails.
  - Protected global functions, always: `ADDON_ACTION_FORBIDDEN` with the
    addon and the action as Lua's debug information names the call
    (`"MyAddon", "UseAction()"` from `UseAction(…)`, as `/stdtest taint`
    recorded; `UNKNOWN()` through `pcall`, `/stdtest engine protected`);
    the call returns its documented results' empty values (`false` from
    `ClearTarget`). The documentation does not mark them; the engine lists
    the known ones (targeting, casting, using actions and items, movement,
    macros), the harmless ones checked in game: TargetUnit, ClearTarget,
    FocusUnit, AssistUnit, SpellStopCasting and SpellStopTargeting, the
    movement stops and PetFollow are protected; ClearFocus,
    TargetLastTarget and StopAttack are not.
  - Protected methods (the documentation's `IsProtectedFunction`: `Show`,
    `Hide`, `SetPoint`, `SetSize`, `SetParent`, …) and `SetAttribute` of a
    protected frame, in combat lockdown: `ADDON_ACTION_BLOCKED`, after the
    method's own argument checks (`/stdtest engine blocked`). A frame is
    protected through a secure template (`Secure…Template`; `IsProtected`
    then gives `true, true`) or `mock.protect`, and through a protected
    frame it holds, however deep (`true, false`; decision T254): such a
    parent is blocked in combat as well, and secure code may use its handle
    then (a secure snippet sizing Blizzard's spells window). A template's script
    handlers from Blizzard's XML are Blizzard's code whoever makes the frame
    (decision T243): an addon's `SecureHandlerClickTemplate` button runs its
    `_onclick` snippet on the user's click. An addon's template's handlers
    are the addon's code, but making them does not taint the code applying
    the template (decision T493): a secure group header runs its
    `initialConfigFunction` for buttons of an addon's template with an
    inline `OnLoad`. The `OnAttributeChanged` a `SetAttribute` runs starts
    as the client calls a script, untainted for a secure handler, also
    within such an `OnLoad` of an addon's `CreateFrame` (Forever 70707's
    smart navigation sets its secure values from tainted template code
    through its own attribute handler, which asserts it runs secure;
    unverified, decision T576).
    `C_RestrictedActions.CheckAllowProtectedFunctions(object)` follows the
    same rule for its object: false for tainted code in combat lockdown on a
    protected frame (UIParent among them), true on an unprotected one, so an
    addon's `GameTooltip:SetUnit` & co. show in a fight (decision T245).
- **Taint reports.** A refusal reports the taint chain: the protected call,
  the secure code that became tainted, the value it read, and the addon code
  (file and line) that wrote that value, through any number of hops through
  secure code (decision T196). This is what `taint.log` cannot show:

  ```text
  UseAction() blocked (ADDON_ACTION_BLOCKED)
    ActionButton_OnClick  FrameXML/ActionButton.lua:211
      read field 'action'
    written by ActionBarButton_Update
      FrameXML/ActionButton.lua:98
      while tainted by MyAddon, after reading
    global 'ACTIONBAR_PAGE'
      written by MyAddon  src/bars.wowlua:42
  ```

  Every tainted value knows the event behind its taint: the store that put
  it there, and what tainted the code that stored it; every tainted
  execution, the read that tainted it. Tests can also assert that a value or
  a handler stays secure, and get the same chain when it does not (decision
  T197).
- **Taint trace.** `WOWLUA_TAINT_TRACE=1` (or `all`) prints to stderr each
  new site where secure execution became tainted, and each site where secure
  code stored a tainted value, with the chain behind it and the Lua calls
  leading there. Each is printed once per site and cause. Any other value
  keeps only the sites whose source contains it
  (`WOWLUA_TAINT_TRACE=Blizzard_AuraContainer`). This finds where Blizzard's
  code picks up an addon's taint before anything is refused:

  ```text
  taint: secure code stored a value tainted by SuuUI
    written by Set
      Interface/AddOns/Blizzard_SharedXMLBase/Flags.lua:71
      while tainted by SuuUI, after reading
    upvalue 'channels'
      at Interface/AddOns/Blizzard_SharedXMLBase/Flags.lua:71
      at Interface/AddOns/Blizzard_SharedXML/MixinUtil.lua:398
      at ...d_AuraContainer/Blizzard_AuraContainerFlowLayout.lua:23
      at src/UI/Auras.lua:321
  ```

## 17.5 Secret values

Secret values (WoW Forever and the 12.x clients) are a value kind of the VM,
as the client has them (probed on WoW Forever 1.60.1, `/stdtest probe`):

- **What tainted code may do.** A secret keeps its type: `type`, passing,
  storing as a table value, `select`, `unpack`, `pcall` and `error` work,
  and so does whatever needs only the type: truth tests of a secret number,
  string or table (a secret `nil` is false), `not` of those, and `==` with
  a value of another type (false) or of a secret `nil` with `nil` (true).
  Secret strings and numbers concatenate into a secret string, and
  `tostring`, `string.format`, `strjoin` and `strconcat` of a secret give a
  secret string.
- **What it may not.** Arithmetic, comparisons (`==` of two values of one
  type too), `#`, indexing, calls, `..` of other types, and a secret
  boolean's truth test fail with the client's error, naming the operand as
  Lua does: "attempt to compare local 's' (a secret number value, while
  execution tainted by 'MyAddon')", or without a name "attempt to compare a
  secret number value (execution tainted by 'MyAddon')". A secret table key
  fails for reads and writes ("attempted to index a table that cannot be
  indexed with secret keys"). Library functions see the type `secret` ("table
  expected, got secret") and refuse to convert one ("attempt to perform
  string conversion on a secret string value (execution tainted by …)");
  the client's own functions with usage messages add "Secret values are only
  allowed during untainted execution for this argument".
- **The functions.** `secretwrap`, `issecretvalue`, `hasanysecretvalues`,
  `scrubsecretvalues`, `scrub`, `canaccesssecrets`, `canaccessvalue`,
  `canaccessallvalues`, `canaccesstable`, `issecrettable`,
  `dropsecretaccess`, `mapvalues`, `dumpobject`, `settablesecurity` and
  `securecopy` behave as in the client.
- **Secure code** may use secrets; what it computes from one is secret again
  (decision T195, from Blizzard's documentation: addon code cannot observe
  it). That includes calling a secret function: the function it hides runs
  and its results are secret.

The engine returns secrets where the client does: from the documentation's
`SecretReturns`, `SecretArguments` and `SecretWhen…` flags, under the
addon restrictions in effect (combat, encounters, challenge modes, PvP
matches, restricted maps and chat lockdown; `/stdtest restrictions`
records the client's by the restrictions active, in
`tests/client/forever-camelot-restrictions.lua`). The rules above do not
change with them: every conformance case gives the same result in combat on
a restricted map as out of it.

A widget's alpha set from a secret (`SetAlpha`, `SetAlphaFromBoolean`) is
its secret alpha aspect: `GetAlpha` reads back a secret, and keeps doing so
after a plain alpha is set, whose value still applies (decision T251;
found in game). What takes the aspect away again is unverified.

A widget shown or hidden from a secret (`SetShown(secret)`, which only
secure code may pass) follows the value the secret hides: a secret false
or nil hides it (decision T575; Blizzard's aura buttons hide themselves
with `SetShown(secretwrap(false))` when they hold no aura). `IsShown`
answers a plain boolean (unverified: whether the client answers a secret
there).

A status bar's value or range set from a secret is its bar value aspect:
`GetValue`, `GetInterpolatedValue` and `GetMinMaxValues` read back
secrets, `OnValueChanged` and `OnMinMaxChanged` get the value and range
as secrets, and `HasSecretAspect(Enum.SecretAspect.BarValue)` is true,
until a plain value (or range) is set again (unverified: the scripts'
values, and what the client keeps).

A status bar given a duration (`SetTimerDuration(duration, interpolation,
direction)`) follows it: its range becomes 0 to the duration's length in
seconds, and its value the seconds passed
(`Enum.StatusBarTimerDirection.ElapsedTime`, the default) or left
(`RemainingTime`), read at each `GetValue` and moved each frame for
drawing; secret while the duration is. A value or a range set stops it
(Blizzard's encounter timeline sets no range for its timer bars, and
releases a timer with `SetMinMaxValues(0, 0)`; decision T487;
unverified: the range and value the client reports, and whether it runs
`OnValueChanged` or `OnMinMaxChanged`, which the engine doesn't).

A status bar's texture without points of its own is laid out over the
part of the bar its value fills, from the right (or the top, vertical)
when the bar fills in reverse, secret value or not, so a frame anchored to
its edge follows the value (decision T490). `GetNumPoints` still gives 0
(unverified: the points the client reports for it, and whether a
frame anchored to it answers its position as secrets while the bar's value
is secret; here it answers plain numbers).

A protected frame anchored to a region (a texture, a font string, a line,
a mask texture) is refused with the client's error, out of combat too:
"Action[SetPoint] failed because[Cannot anchor protected frames to
regions]: attempted from: Button:SetPoint." (decision T252; found in game).

Some global functions are blocked for tainted code in combat lockdown
only, with `ADDON_ACTION_BLOCKED`: `C_SpellBook.PickupSpellBookItem`
(decision T253; found in game) and `C_GuildInfo.GetMOTD`, which
Blizzard's deprecated guild script also names `GetGuildRosterMOTD`
(decision T265; found in game). Blizzard's own code, the secure drag
handlers among it, may call them.

The encounter timeline keeps addons' timers (`C_EncounterTimeline.
AddScriptEvent`, decision T255; found in game though the timeline is off
on Forever): listed, read, paused, resumed, finished, cancelled and ended
when their time runs out, with `ENCOUNTER_TIMELINE_EVENT_ADDED`,
`…_STATE_CHANGED` and `…_REMOVED`. The functions that require a timeline
event answer nothing for an ID not on it, as recorded. The IDs, the info's
fields and the events' order follow the documentation, unverified.

`IsMouseOver(top, bottom, left, right)` measures the mouse against the
region's rectangle, each edge moved by its offset (up or right for a
positive one), whatever covers it (decision T257; documented). A region
without points has no rectangle; a hidden one's rectangle still counts
(seen in game). The mouse is off every frame at first; the test runner's
hover and click put it on the frame.

The chat windows' settings are kept (decision T263): what
`SetChatWindowName`, `…Size`, `…Color`, `…Alpha`, `…Shown`, `…Locked`,
`…Docked`, `…Uninteractable`, `…SavedPosition` and `…SavedDimensions`
set is what `GetChatWindowInfo`, `GetChatWindowSavedPosition` and
`GetChatWindowSavedDimensions` answer after, over the recorded answers.
No events follow (unverified).

`ClearPoint(point)` removes that one point and keeps the others (decision
T258; documented): Blizzard's menus flip a point this way to open above
their owner near the screen's bottom.

An item put on a part stack of the same item in the bags stacks onto it
(decision T256; recorded by SuuUI's bag sort probe): the target slot
locked at once, the cursor let go, and a round trip later the source's
bag update, the target's lock gone, its bag update and
`BAG_UPDATE_DELAYED`. A split part stacks the same way, and what doesn't
fit stays in the source slot; a part stack put on a full stack of its own
swaps places with it (all seen in game; their events unverified). A split
part onto a full stack: nothing (unverified).

An accepted quest is watched only while `autoQuestWatch` is on (decision
T267; seen in game: off, `GetQuestWatchType` answers nothing for it, and
Blizzard's UI still focuses it); a watch added or removed by hand stays
so through the log's updates.

## 17.6 The engine

The WoW API the VM's code calls, per flavor.

- **Generated.** Every function, namespace, event, enum and widget method is
  generated from the flavor's API documentation (the same source as the
  `wow-api` declarations, decision T83): its signature, its existence in the
  flavor, whether it is protected or restricted, and which of its arguments and
  results are secret. A function the flavor lacks is absent, as in the client.
  The documentation comes as data (decision T199): `wowlua-types-gen` writes
  each flavor's as JSON, every key kept, to
  `crates/wowlua-api/data/<flavor>.api.json.gz`. Functions the client has
  but does not document (`UseAction`, about 2,000 on WoW Forever, from
  `/stdtest inventory`) exist too, with their behaviour or "not mocked";
  on standard, mists, tbc and vanilla, whose clients StdTest does not record yet,
  the client's C functions are those of Ketho's dump of it (the one the
  flavor's declarations take, `data/<flavor>-functions.txt`), so a
  function only some clients have exists on theirs alone (decision T510).
  What every client answers with nothing there (no vehicle bar, pet
  battle, ready check or raid target; the flavor's expansion) answers on
  every flavor, and `C_CombatLog.GetCurrentEventInfo`, which only the
  classic clients give addons, answers the payload of the combat log
  event last fired. As recorded on those clients (decision T511), each
  class's colour is Retail's (Classic Era's Shaman the Paladin's pink,
  a class the client lacks none), and the classic clients refuse
  `GetTalentGroupRole` with "Script_<name>: API unsupported in this version
  of World of Warcraft." (Mists also the talent tabs' `GetNumTalentTabs`,
  `GetNumTalentGroups` and `GetUnspentTalentPoints`, TBC and Classic Era
  the tiers' `GetNumSpecGroups` and `GetTalentTierInfo`), and their
  `GetRuneType` answers two runes of each type (1, 1, 3, 3, 2, 2) on every
  class, and no value for another rune. The expansion
  getters answer the client's (the server's only after login, as the
  client's does), and the classic clients' `GetWeaponEnchantInfo` answers
  no enchant on each hand (Mists: two hands, 8 values; TBC and Classic Era
  also the ranged slot, 12).
  The `Enum` and `Constants` the client changes and the number globals it
  defines itself (`LE_*`) are camelot's recording; on standard, mists, tbc
  and vanilla, what the documentation lacks comes from Ketho's dump of the
  client (its `LuaEnum.lua`, at the commit the declarations take: older
  than an installed client, so it only adds), and the tables the flavor's
  own `EnvironmentCleanup.lua` removes are secure, as camelot's recorded
  ones (`data/<flavor>-enums.txt`, `data/<flavor>-globals.txt`; decision
  T554). A `Constants` table the recording has that a newer build's
  documentation names otherwise, while that build's code still reads the
  recorded name, keeps its recorded values until the battery is recorded
  again (camelot 70707's `LevelConstsExposed`, documented as
  `LevelConstsExposed_Camelot`; decision T576). `C_UIColor.GetColors` answers the installed client's `GlobalColor`
  table, by name, each byte times 1/255 in floats as the client's
  (`data/<flavor>-colors.txt`, which `cargo xtask types gen` writes).
- **Natives.** The API functions are natives of the VM, as the client's are C
  functions (decision T198): `[C]` in stack traces, the client's argument
  errors, no taint of their own.
- **Behaviour.** Functions have hand-written behaviour where tests need it:
  the widget system (frames, regions, textures, font strings, scripts, events
  and unit events, points and sizes, show and hide, parenting, templates,
  mixins, protected frames), the event loop and `OnUpdate` frames, timers on a
  virtual clock, combat lockdown, units, auras, spells and items (from test
  data), skill lines (the one selected is the one `SetSelectedSkill`
  chose, decision T357), saved variables across reloads, slash commands, keybindings, addon
  messages, settings, tooltips, menus and dialogs. The M8 test mock is the
  starting point.
- **Powers.** A unit has every power its class has in the flavor table
  ([Manifest §9.4.1](manifest.md#941-flavor-table); decision T512), its
  mana full at its class's base mana for its level (100 of 100 without),
  and shows one (`UnitPowerType`, its token the table's). `UnitPower(unit,
  type)`, `UnitPowerMax`, `UnitPowerMissing` and `UnitPowerPercent` answer
  that power, the shown one without a type (or with one not a number); a
  type the unit lacks a plain 0 (recorded: a paladin's
  `UnitPower("player", 1..29)`). A power it has but does not show is as
  secret as the shown one, combo points never (`GetPowerTypeSecrecy(4)`
  answers never, recorded; the answers unverified); `UnitHasPowerType` and
  `C_Secrets.ShouldUnitPowerBeSecret` follow the powers it has. Changing a
  power tells `UNIT_POWER_UPDATE` (a regeneration tick
  `UNIT_POWER_FREQUENT`) and a maximum `UNIT_MAXPOWER`, each with the
  power's token; another power shown, `UNIT_DISPLAYPOWER`. A spell's cost
  and an action's usability read the shown power, an item's mana the mana
  (none for a character without it). Unverified: the answers for powers
  not shown, `UnitPowerType(unit, 1)` of a unit with several powers (none,
  as recorded for one with one).
- **Races and factions.** A unit's faction (`UnitFactionGroup`) follows
  its race's in the flavor table ([Manifest §9.4.1](manifest.md#941-flavor-table);
  decision T513), whatever its data says; a race of either faction (the
  Pandaren, the Skyborne) has the one given (`faction`, `factionGroup`),
  none when none is given; a creature its own. `UnitRace` of a race of
  either answers its faction's race ID and name (camelot's Skyborne: 95
  High Order Skyborne, 96 Windshaper Skyborne; with none, 96). A player
  given no race is a Human, an Orc when given the Horde. The player knows
  its race's racial spells for its faction and class, from the table,
  without learning them (`IsPlayerSpell`, `C_SpellBook.IsSpellKnown`);
  unverified: the recorded spellbook does not list them.
- **Layout.** A region's rectangle comes from its points as the client
  lays it out: each edge from the points that set it (a `LEFT` and a
  `RIGHT` point give the width, a `TOP` and a `BOTTOM` the height),
  otherwise its set size placed by the axis's first point. Of two points
  that set one edge, the one earlier in `Enum.FramePoint`'s order
  (`TOPLEFT`, `TOP`, `TOPRIGHT`, `LEFT`, `CENTER`, `RIGHT`, `BOTTOMLEFT`,
  `BOTTOM`, `BOTTOMRIGHT`) places it, whatever order they were set in
  (decision T337). Points that
  put the right edge left of the left one, or the top below the bottom,
  give no size: 0, at the `LEFT` and `BOTTOM` points (decision T266; seen
  in game). `GetSize`,
  `GetWidth` and `GetHeight` give the laid-out size of an anchored region
  (unverified beyond fresh widgets; `/stdtest engine` records
  `layoutSizes`).
- **Screen and text.** The client draws on a screen (decision T237, a
  test setting; a world profile's `screen` and the game's settings while
  it runs, decision T482: the window's size, "Use UI Scale" and its slider,
  told by `DISPLAY_SIZE_CHANGED` and `UI_SCALE_CHANGED`): UIParent's size
  and scale and the screen functions follow it, every region's effective scale includes UIParent's (unless it ignores
  its parents' scale), WorldFrame covers the screen unscaled, 768 units
  high at any resolution and UI scale, its width following the screen's
  aspect ratio (decision T492), and text is measured from the client's recorded
  glyphs at the pixel size the client renders at that scale, so string
  widths, wrapping and truncation are the client's. Lines break as the
  client breaks them (decision T282, recorded by the wrap probe): at the
  end of a space run or after a hyphen, a line as long as its advances fit
  the width; a word wider than the line is cut between characters (at
  least one a line), and without `SetNonSpaceWrap(true)` the rest of the
  text then fills the lines character by character and the string is
  truncated. `GetNumLines` counts every line the text breaks into, also
  with word wrap off or beyond `SetMaxLines`; the height is the lines
  shown; held to fewer lines, the last shown line ends in an ellipsis (one
  glyph three periods wide) and `GetStringWidth` is the shown text's;
  `IsTruncated` says whether lines are left out or a word was cut. The
  client's advances are finer than the recorded whole pixels, so a narrow
  line now and then holds a glyph fewer than in the client. The recordings
  cover Friz Quadrata, Arial Narrow, Skurri and Morpheus at 8 to 45 pixels,
  printable ASCII; with the user's installation at hand (tests with
  `--game-data`, sessions with a screenshot host), a glyph they lack (another
  of the client's fonts, another character, another size) is measured with
  the client's font file (decision T392): its advance rounded up, its width
  alone (a line's last glyph) its ink's right edge rounded up and one more,
  a space's its advance; checked against the recordings, 98.6 % of the
  advances and 95 % of the widths are the same. Without it, such a glyph
  is the recorded Friz Quadrata's (scaled to the size), a character outside
  ASCII a `?`'s. Unverified: kerning beyond the recorded pairs (none), an
  outlined font's glyphs (as the plain one's), a font of an addon's (not
  in the installation: as without it).
- **Atlases.** Every atlas of the client, from its own tables (decision
  T238): sizes, files, texture coordinates and tiling as the client gives
  them; an atlas it lacks is none.
- **Textures' files.** A texture shows one image: a color, a file or an
  atlas, each set replacing the others (decisions T352, T356). It tells its
  file by ID, as the client does: `GetTexture()` and `GetTextureFileID()`
  give a file ID set, a path's file as `GetFileIDFromPath` gives it (a game
  file's own ID where the installation is at hand, an addon's file its
  loose file's, nil for a file there is not), an atlas's file, else nil;
  `GetTextureFilePath()` the path set. A loose file's ID set
  (`b:SetTexture(a:GetTexture())`) shows that file. `SetTexture` fails
  (false) for `""` and file ID 0. Without the installation a game file's ID is not known, and
  a path to one answers a stand-in: a negative number, the same for the
  same path.
- **Widget state.** A method with no result changes what the widget's
  getters answer as the client's does (decision T289): `Raise()` and
  `Lower()` put a frame above or below every other shown frame of its
  strata (`GetFrameLevel`); an edit box keeps a selection
  (`HighlightText`, `ClearHighlightText`) that `Insert(text)` and the
  user's typing (`mock.type`) replace, else they go in at the cursor, and
  `SetText` puts the cursor at the end, as does the user's click or typing
  into a box without the focus (before its `OnEditFocusGained`);
  `AdjustPointsOffset(x, y)` moves every anchor, `ClearPointsOffset()`
  zeroes them; the clears (a button's `ClearNormalTexture`
  and the other states', `FontString:ClearText`, `ClearParentKey`,
  `ResetTexCoord`, `StopAnimating`, `ClearAttributes`, `ClearScripts`,
  `RemoveAnimations`) undo their setters, but a button's cleared state
  texture stays its own, emptied and hidden (`GetNormalTexture` answers
  it: Forever 70707's item buttons mask it after their `OnLoad`, in which
  the professions' recraft slot clears it; unverified, decision T576); `LockHighlight` is
  `SetHighlightLocked(true)`. The methods of what the engine draws or keeps
  nothing of (model cameras and lights, the browser, the shop's checkout,
  housing's pins, the minimap's blips, movies, vector graphics) change
  nothing, and their getters answer the documented empty values instead of
  "not mocked" (decision T295). An edit box keeps
  the lines `AddHistoryLine` adds (as many as `SetHistoryLines` allows,
  `ClearHistory` forgets them); the arrow keys run `OnArrowPressed`, left
  and right move the cursor, up and down step through the history (with
  `SetAltArrowKeyMode(true)` only while Alt is held; decision T291).
- **Targeting.** Tab-targeting cycles the units of its kind within 40
  yards, nearest first (`TargetNearestEnemy([reverse])` and its kin);
  `TargetLastTarget`, `AssistUnit` and `FocusUnit` follow the units'
  targets; `CancelUnitBuff` and `CancelSpellByName` remove the player's
  own buff (decision T291). They are protected: an addon's call is
  refused first.
- **Leftovers.** `StartAttack` and `AttackTarget` turn auto attack on
  against a target that can be attacked, `StopAttack` off;
  `SetPartyAssignment` and `ClearPartyAssignment` keep the raid's main
  tanks and assists for `GetPartyAssignment`; `SendSystemMessage` prints a
  system line; `PickupPlayerMoney`, `DropCursorMoney` and `GetCursorMoney`
  hold money on the cursor (`ClearCursor` lets it go); `TargetLastEnemy`
  and `TargetLastFriend` retarget; `Sign` and `secretunwrap` answer as the
  client's. The client's debug switches and the systems the engine does not
  model answer their documented empty values (decision T295). Every
  function the camelot client has now has behaviour.
- **Not mocked.** A generated function without behaviour raises
  "`<name>` is not mocked" when called, unless the project provides a mock
  (Tooling §16.1.4), so gaps are visible instead of silently returning nil.
- **Engine-side Lua.** Lua the client defines in its engine (not FrameXML) is
  provided as secure code.
- **What addons use of Blizzard's UI code.** Until Blizzard's UI code loads
  (without `test.blizzard-ui`, never), the engine has the parts of it that
  addons and wowlua's standard library use as the client's:
  `SecondsToTime`, `SecondsToTimeAbbrev`, `SecondsToClock`,
  `FormatShortDate`, `GetMoneyString` and `FormatLargeNumber`, as
  Blizzard's `TimeUtil.lua` and `FormattingUtil.lua` write them (from the
  global strings they read, the client's enUS text where those are not
  loaded); `SendChatMessage`, `DoEmote` and `CancelEmote`, which call the
  `C_ChatInfo` functions as Blizzard's deprecation file does; the coins'
  constants (`COPPER_PER_SILVER`, `SILVER_PER_GOLD`, `COPPER_PER_GOLD`)
  and global strings (`GOLD_AMOUNT`, `SILVER_AMOUNT`, `COPPER_AMOUNT`);
  and the container constants Blizzard's `Constants.lua` makes from
  `Constants.InventoryConstants` (`NUM_BAG_SLOTS`, `NUM_REAGENTBAG_SLOTS`,
  `NUM_TOTAL_EQUIPPED_BAG_SLOTS`, …) where the flavor's `Constants` has
  their values (camelot's are recorded). Blizzard's code defines its own
  in their place as it loads.

## 17.7 Blizzard's UI code

With `test.blizzard-ui` ([Manifest §9.18](manifest.md#918-test)), the test
runner loads the flavor's FrameXML (Lua and XML from wow-ui-source, at the
client version the API declarations come from) as secure code before the
addon. Tests then run against the real Edit Mode, Settings, action bars, unit
frames and the rest, so taint the addon spreads into Blizzard's code, and
Blizzard code that fails because of what an addon did, show up as in game.
Off by default: mock-only tests start faster (decision T186).

Where the flavor's client is installed, the code and the global strings are
the installed build's own (decision T553), read once per build into the user
cache (`$WOWLUA_HOME/cache/blizzard-ui/<flavor>-installed-<version>/`). The
client names its interface files by hashes only, so they are found as the
client finds them: from Blizzard's addons' names (wow-ui-source's, at the
commit the declarations name), each addon's TOC (the client's suffixes
first), the files it lists with `[Game]` and `[Family]` as the flavor's, its
key bindings (`Bindings_<suffix>.xml`, `Bindings.xml`), the files its XML
includes, and the addons it depends on or loads (`LoadAddOn("…")`). The global
strings are the client's `GlobalStrings` table's that are set in the game,
the escapes the table writes them with (`\n`, `\32`, `\\`) read as in Lua
(decision T576).
`WOWLUA_UI_FROM=upstream` leaves the installation out.

Elsewhere (CI), the code comes from wow-ui-source at the flavor's client
version and the global strings from the same dump the declarations use,
downloaded on first use into a user cache keyed by version (decision T204; about 10 seconds and
45 MB per flavor, once: `$WOWLUA_HOME/cache/blizzard-ui/`, at the commit the
flavor's declarations name; `WOWLUA_UI_SOURCE` names a local copy instead), for
each flavor that has test files to run (`build.flavor-files` may give them to
other flavors). The engine's loader (`wowlua_engine::ui::load`)
takes the flavor's `Interface` folder and its global strings:

- **Startup set** (as `/stdtest engine` recorded: the 201 Blizzard addons
  WoW Forever had loaded when StdTest loaded): every Blizzard addon allowed
  in the game (`AllowLoad` game or both) and for the flavor's game types
  (its id and family, for `AllowLoadGameType` and `ExcludeLoadGameType`),
  read from the client's suffixed TOC first (`_Mainline.toc`); a
  `LoadOnDemand` one only as a startup addon's dependency; `OnlyBetaAndPTR`
  ones on beta and PTR clients (WoW Forever's beta ran from `_classic_beta_`).
- **Load order** (inferred, unverified): `LoadFirst` ones first, each after
  its `Dep`, `Dependencies`, `RequiredDep` and `OptionalDep` addons,
  otherwise by name; a startup addon with `LoadWith` right after the addons
  it names (Classic's Blizzard_UIParentPanelManager after Blizzard_UIParent,
  before the `LoadFirst` addons that use its `UIPanelWindows`; decision
  T554).
  File lines follow their inline directives; `[Family]`, `[Game]` and
  `[TextLocale]` name the flavor's folders (`Mainline`, `Camelot`, `enUS`).
  `ADDON_LOADED` fires after each addon.
- **Global strings** come first, from the client's `GlobalStrings` dump.
- **Lua files** run as secure code with the addon's name and table, also
  when an addon's code loads a load-on-demand one (`C_AddOns.LoadAddOn`)
  while the addon loads: Blizzard's files and scripts compile as Blizzard's
  code whoever runs when they load, and keep their own file names. A file's
  result is its module: `import(".Dir.Name")` gives the result of
  `Dir/Name.lua` next to the importing file, `import("AddOn.Dir.Name")` that
  of `Dir/Name.lua` in the addon `AddOn`'s folder (decision T576).
- **XML** builds widgets as the client does: virtual elements are templates
  (for `inherits` and `CreateFrame`), intrinsic ones frame types; an
  element's templates apply first, then its attributes, mixins, `parentKey`
  and `parentArray`, key values, sizes, anchors (an element's own
  `<Anchors>` take the place of its `setAllPoints`, decision T323; a
  texture with none fills its parent, sized or not, but a slider's thumb,
  which the slider anchors by its `LEFT` to its own `LEFT` (`TOP` to `TOP`
  on a vertical slider), offset along its length by its value, so
  `GetNumPoints` gives 1 and `GetPoint` that anchor, decision T326; a
  texture's `rotation` is in degrees, turning it about its center), layers,
  child frames and scripts (`function`, `method` or a body;
  `inherit="prepend"` / `"append"`; a texture's `<Color>` is its plain
  color without a file and its tint with one, `clipChildren` clips,
  decision T339);
  `OnLoad` runs once the widget and its children are built. Fonts and font
  families become font objects.
- **Secure APIs for addons** (Blizzard_AuraContainer's custom containers):
  a type or template declared under `<ScopedModifier
  allowUntaintedCreation="true">` is built untainted for any caller of
  `CreateFrame`: its mixins, its scripts (compiled as Blizzard's) and its
  `OnLoad`, also when an addon creates it while the addon loads (an
  intrinsic type's scripts, like a template's, are the code of the addon
  that declared it, never the caller's). A `<Mixin secureDelegates="true">` puts secure delegates on the
  frame: they call the mixin's function untainted, with the arguments as
  untainted values, and give the caller's taint back after. A tainted
  caller passing a secret gets no elevation from a mixin's delegates
  (Blizzard's ScriptErrorsFrame says so), but does from
  `CreateSecureDelegate`'s. Partitions are not modelled: an object is its
  own `GetForbiddenObjectTable` (secure code only), and the delegate stays
  on the frame where a later private mixin has the same function.
  Unverified: errors in a delegate reach its caller.
- **Gaps.** Without game data or a recording, Blizzard's code gets empty
  values from functions that have no behaviour: secure code calling such a
  function gets its documented results' empty values (nil where nilable,
  false, 0, "", a table) and the engine lists the function; addon code still
  gets "not mocked". Secure code may register events and set scripts the
  engine's lists lack. On camelot (70425) the UI loads, logs in and runs
  frames without a Lua error, as the client's does (its probe logged none):
  enums, constants and namespaces the documentation lacks come from the
  client's recording (`/stdtest engine`), secure addons run in the client's
  secure environment, and Blizzard's errors are caught where ScriptErrors
  reports them (`C_Log.LogErrorMessage`). The CVars start at the client's defaults (the 248 the UI code reads that
  have one, recorded by `/stdtest engine`). The other flavors' clients are
  not recorded yet: their UI code meets enums, colors and constants the
  documentation lacks and raises errors while it loads, so for now
  `test.blizzard-ui` is for camelot, and wowlua's example addons run
  their tests on camelot alone (their manifests' `flavor-files` give
  `tests/**` to camelot) until standard's and vanilla's are recorded.
- **The client's cases.** With camelot's UI code loaded (and the addon's
  saved variables loaded as the addon's), all 819 cases `/stdtest probe`
  recorded on WoW Forever 1.60.1 give the client's results, the 22 the VM
  alone cannot pass included (Blizzard's `string.rtgsub`, `getglobal`,
  the string metatable `RestrictedExecution.lua` sets up).

## 17.8 Verification

- **Lua 5.1 conformance.** Against PUC Lua 5.1 (embedded only in wowlua's own
  tests):
  - `cargo xtask corpus vm` compiles every file of the conformance corpus to
    the same bytecode and line info as `luac`, and runs its main chunk with
    the same output and error.
  - `cargo xtask corpus lua51` runs the official Lua 5.1 test suite. It
    excludes only what the client does not have, each with its reason: the
    `io`, `os`, `debug` and `package` libraries, `string.dump`, the implicit
    `arg` of vararg functions, `string.gfind`, and a 32-bit `size_t`
    overflow. Shims serve the suite's plumbing only (timing, temporary files
    in memory, loading its helper files).
  - Differential tests compare snippets' output and errors.
- **Sources of truth.** In-game probes on the current client first, then
  Blizzard's documentation and UI code for the build (the generated API
  documentation's `SecretArguments`, `ProtectedFunction`, `HasRestrictions`,
  `SecureHooksAllowed` and `SecretWhen…` flags, `FrameScriptDocumentation`).
  elune (an open reimplementation of an older client) is only a hint: a rule
  that rests on it alone is listed as unverified in the coverage report.
- **Taint.** StdTest's taint experiments (`/stdtest taint`) taint one value on
  a path the client's secure code takes to a protected function and record
  whether the action is blocked; the VM must agree. On WoW Forever 1.60.1
  (`tests/client/forever-camelot-taint.lua`): secure code reading a tainted
  global (Lua or C function) or table field is tainted, and the protected
  call raises `ADDON_ACTION_FORBIDDEN` naming the addon; `hooksecurefunc`
  leaves the hooked variable secure and the click allowed; `setfenv` taints
  the function itself (its variable stays secure, the click is forbidden).
  With camelot's UI code loaded, the engine reproduces all eight: the
  action button's click reaches `UseAction` through Blizzard's own
  `SecureTemplates.lua` and is forbidden exactly where the client forbade it
  (`wowlua-engine`'s `tests/taint.rs`). elune's taint cases
  (vendored in `crates/wowlua-vm/tests/elune/`) also pass on the VM, but count
  only where the experiments confirm them.
- **The client's behaviour.** Every WoW-specific rule (§17.3 to §17.5) has a
  probe in StdTest whose recorded in-game result is a test fixture; a rule
  changes only with a new probe.
- **In-game regressions.** Bugs found in game become emulation tests that must
  fail the same way: `os.date` in `wowlua:format`, `0 / 0` in
  `wowlua:serialize`, SuuUI's reparented status-bar containers breaking Edit
  Mode, and taint met in combat.
- **Coverage of the emulation.** A report lists which API functions have
  behaviour, which are generated only, and which rules were probed on which
  flavor: [`docs/emulator-coverage.md`](../../docs/emulator-coverage.md),
  written by `cargo xtask coverage-report` from the engine's own tables, the
  fixtures and the replay tests' ratchets; `cargo xtask check` fails when it
  is out of date.

## 17.9 VM design

- **Execution** (decision T190). The VM compiles the compiler's output to a
  register bytecode of its own, close to Lua 5.1's design. Each instruction
  carries its source span (from the source maps), and coverage, taint and
  profiling attach per instruction.
- **Memory** (decision T191). A tracing garbage collector over an arena, in
  safe Rust, stopping the world at the interpreter's allocation points:
  - Cycles are collected and weak tables clear.
  - `__gc` runs as in the client: newest first and unprotected, so an error
    in a finalizer reaches the code that caused the collection.
  - `collectgarbage` reports memory, and `step` counts work, as Lua 5.1's
    incremental collector would.
- **Taint** (decision T193). Exact, as the client tracks it: every stack
  slot, table field, upvalue and object carries the taint (the addon) of the
  code that wrote it, and each thread its execution's taint (§17.4).
- **Coroutines** (decision T192). Stackless: each coroutine is its own VM
  stack, and a yield unwinds to its `resume` without the native stack. As in
  the client's Lua 5.1, a yield cannot cross `pcall`, a metamethod or another
  native call ("attempt to yield across metamethod/C-call boundary"); each
  thread has its own global table (`setfenv(0, t)`).
