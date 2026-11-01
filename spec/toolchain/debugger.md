# 18. Debugger, Profiler and UI Rendering

**Status: planned (M12).** The debugger attaches to the emulated client
([WoW client emulation](emulator.md)): the VM pauses and steps where the real
client cannot be observed, the profiler counts what every function, event and
script costs, and the engine's UI is drawn as an image of the state it holds
(decisions T209, T300 to T315).

## 18.1 Layers

Everything is a library first; the command-line front-ends and wowlua studio
(the planned native IDE) use the same pieces.

| Layer | Crate | Role |
|-------|-------|------|
| Debug hooks | `wowlua-vm` | Line, call, return, error and load events, the profiler's counters (§18.2) |
| Debug core | `wowlua-debug` | Sessions, breakpoints, stepping, stacks, variables, evaluation, profiles (§18.3 to §18.8) |
| Scene | `wowlua-engine` | The UI as a draw list: what is visible, where, in which order (§18.9) |
| Assets | `wowlua-casc` | Textures, atlases and fonts from the user's own WoW installation (§18.10) |
| Renderer | `wowlua-render` | Draws a scene with `wgpu`: to a window, or off-screen to a PNG (§18.11) |
| Front-ends | `wowlua-cli` | `wowlua debug` (DAP), `wowlua mcp` (MCP), `wowlua screenshot`, the test API (§18.12); the VS Code extension (`editors/vscode`) registers the `wowlua` debug type and runs `wowlua debug` over stdio (decision T331) |

**Native and the browser** (decisions T212, T301). Every layer but the
front-ends builds for `wasm32-unknown-unknown`: no file system, threads,
processes or clocks of their own. Files come through the engine's file access
interface; time through a clock the host gives (`std::time::Instant`
natively, `performance.now()` in the browser). A pause never blocks the
thread that renders: the paused VM runs a command loop (§18.3), and the host
decides where it waits (a thread natively, a Web Worker in the browser).
Studio links the debug core directly, in its own process; DAP and MCP are
for other programs.

## 18.2 Debug hooks

The VM reports what it runs to a hook the host installs (decision T302):

- **Line**: before an instruction that starts a new line, as Lua 5.1's line
  hook (a function's first instruction, a backward jump, a different line).
- **Call** and **return**: every Lua and native function, tail calls
  included; an error unwinding a frame reports its return too.
- **Error**: an error raised, before it unwinds, with whether a `pcall` or
  `xpcall` up the stack catches it.
- **Load**: a chunk compiled, with every function in it.
- **Dispatch**: the engine tells the hook when it starts and ends running
  an event's handlers, a widget's script, a frame's `OnUpdate`, a timer and
  a game frame, with their names; stacks and profiles show them as frames of
  their own.

**Cost.** One byte in the VM says which hooks are on; with none, an
instruction costs one test of that byte, and a call or return one more
(decision T303). Line events come only from functions that have a
breakpoint, or while stepping: breakpoints mark the functions they are in,
so code without one runs at full speed under the debugger. A running session
can be interrupted from another thread or the browser's event loop: the
interrupt is an atomic flag the VM looks at every 1024 instructions while a
debugger is attached, and never otherwise.

## 18.3 Sessions

A session is one emulated client under the debugger, started from:

- **A test file** (`test`): the file runs as `wowlua test` runs it
  ([Tooling §16.1.2](tooling.md#1612-what-a-test-run-loads)), its tests
  filtered by name (`filter`); the session ends with the file.
- **The addon** (`addon`): the package's test build loads as the client
  loads it, with the manifest's world profile, Blizzard's UI when
  `test.blizzard-ui` is set, and its other addons; the player logs in and
  the game's frames run until the session is stopped.

**Clock** (decision T304). A session's time is the engine's virtual clock.
In `realtime` mode (the default of DAP sessions) frames run as fast as the
game's settings let them, each moving time as far as it really took, so
`OnUpdate` and timers advance as in game: no faster than `maxFPS`
(`maxFPSBk` while the session is in the background) and, with `vsync` on,
the display's rate (decision T538): 60 Hz, or what the host tells
(`SetDisplayRate`, decision T544), and at that rate exactly: a frame
starts when the one before planned it, so a wait that ends late does not
slow the frames after it (decision T548); in `manual` mode
(the default of MCP sessions) time moves only when asked
(`advance(seconds | frames)`), so what an agent sees repeats exactly. Time
stops while paused.

**Paused** (decision T305). At a breakpoint, a step's end, an exception or a
pause request, the VM stops inside the hook and serves commands until told
to go on: stack, variables, evaluation, breakpoints, the scene, screenshots,
the profile. Input sent while paused waits and reaches the UI, in order,
when the game's next frame runs; the scene and screenshots show the state at
the pause, layout included.

## 18.4 Breakpoints

| Kind | Written as | Stops |
|------|-----------|-------|
| Line | a source line, with an optional condition, hit count and log message | before the line runs |
| Function | `Name`, `Table.name`, `Table:method` | when that Lua function is entered |
| Function by definition | `src/Threat.wowlua:560` (a source and the line a function is defined on, as a profile names it) | when any closure of that function is entered, local ones too (decision T495) |
| API call | `C_Timer.After`, `CreateFrame` | when Lua calls that native function |
| Event | `event:PLAYER_LOGIN`, `event:*` | before the event's first handler runs |
| Script | `script:OnClick`, `script:MyFrame:OnUpdate` | before a widget's script runs (of any widget, or the named one) |
| Timer | `timer` | before a timer's callback runs |
| Exception | `uncaught`, `all` | when an error reaches the error handler, or any error |
| Taint | `taint`, `blocked` | where execution becomes tainted, or a protected action is blocked |

An error reaches the error handler when the nearest handler that takes it
is the client's: an event or script handler, a timer, anything the engine
ran, unless a `pcall` or `xpcall` inside it catches the error first; a
test's own `xpcall` around the test does not count (decision T358).

Line breakpoints are set in `.wowlua` and `.lua` sources: test and dev
builds keep every statement on its source line
([Code generation §14.4](codegen.md)), so a source line is the chunk's line.
A line with no code moves to the next one that has some, as editors expect;
the breakpoint answers with the line it took. Conditions and log messages
are Lua expressions evaluated in the stopped frame (§18.7); a log message's
`{expr}` parts are evaluated and printed to the console without stopping.

## 18.5 Stepping

Step into, over and out, by source line, in the running coroutine:

- **Into** stops at the next line run anywhere: in a function the line
  calls, or in a handler the engine runs because of it (`frame:Show()`
  runs `OnShow`, `C_Timer.After(0, f)` does not run `f` yet).
- **Over** stops at the next line of the same function or its callers.
- **Out** stops at the next line of the caller; out of a handler, at the
  next handler of the same dispatch (the next frame's `OnEvent` for the same
  event), else where the engine resumes Lua next.

Steps go into Blizzard's UI code only when the session loads it with
`debug-blizzard-ui: true`; otherwise its functions are stepped over and its
frames are marked in stacks.

## 18.6 Stack frames

A stack lists, innermost first: Lua functions with their source location
(path relative to the package, line), native functions as `[C] name`, and
the engine's dispatches as frames of their own (`event PLAYER_LOGIN`,
`MyFrame:OnEvent`, `OnUpdate`, `timer`, `frame 1234`), so a handler shows
why it runs. In a coroutine the stack continues with the code that resumed
it. Each frame carries the taint of its function (the addon whose code it
is, or secure).

## 18.7 Variables and evaluation

Scopes per frame: **Locals** (parameters first), **Varargs**, **Upvalues**,
and **Globals** (the function's environment). Tables expand lazily, sorted
as `toMatchSnapshot` sorts keys, with their metatable as a last entry;
widgets show their type, name, shown state, size, points, parent and
children before their fields. Every value shows its taint (decision T306):
`secure`, or the addon that wrote it; a secret value shows its hidden value
marked as secret, which no addon code can read.

**Evaluation** (watches, the console, conditions, log messages): a Lua
expression or statement list, compiled as a chunk whose environment reads
the frame's locals and upvalues first, then its environment; it runs with
the frame's execution taint (so it cannot make insecure code secure), with
breakpoints off, and its errors are its result. Assignments to locals and
upvalues write the frame's. A watch or a hover only reads (decision T496):
it assigns nothing (a local, an upvalue or a global), and calls only the
Lua library's functions that change nothing (`string`'s, `bit`'s,
`math`'s but `random`, `table.concat`, `type`, `tostring`, `pairs`, …) and
the game's functions and methods named as queries (`Get…`, `Is…`, `Has…`,
`Can…`, `Unit…`, `Should…`, `Does…`, none with `Create`); any other call,
a Lua function's too, is refused with an error that says so, before it
runs. The console, conditions and log messages run any code. What the
session prints goes to the console,
as the chat frame shows it: colour codes, links' codes and inline
textures left out, `|4one:many;` its word for the number (a chat line's
text, `ChatFrame1:AddMessage`'s, is otherwise the game's own).

## 18.8 Profiler

The VM counts, per function and per dispatch (decision T307):

- calls;
- instructions run, self and total (the default measure: the same on every
  machine and every run, so CI compares them);
- bytes allocated, self and total (the garbage a function makes);
- host time, self and total, in nanoseconds (the emulator's speed, not the
  client's; shown as a second measure).

Functions are keyed by their definition (source, line defined; natives by
name), dispatches by kind and name (`event PLAYER_LOGIN`, `MyFrame:OnEvent`,
`OnUpdate MyFrame`, `timer`), and game frames by number. The call tree keeps
every path (callers and callees) up to depth 256, and each of the newest 600
game frames keeps its own: the nodes that ran in it, with their calls and
costs in that frame (decision T495). A profile is started and
stopped in a session (or covers a whole test file with
`wowlua test --profile-out <dir>`), and is given as a table (functions sorted by a
measure), a call tree, a per-frame timeline, or a file:
[speedscope](https://www.speedscope.app) JSON with one profile per measure,
which Studio and speedscope open (decision T308). The file keeps the whole
profile under `wowlua` for wowlua's readers, which speedscope leaves alone:
the dispatches' kinds, the tree with each node's calls and own cost, and the
kept frames with their trees; read back, it is the profile again, and
another tool's speedscope file reads as the tree of its samples (decision T495).

## 18.9 Scene

The scene is what the screen shows, as data (decision T309): every visible
region in draw order, in screen pixels (top-left origin, UIParent's scale
applied), each with the widget it belongs to; the items standing over units
as the game's camera sees them (the nameplates' and the names over units)
are told apart, so a view of the world from another camera can leave them
out, and each unit of the world carries its index (decision T546). It is
built from the engine's
state on request (a screenshot, a viewer's frame). Each frame's items are
kept from one scene to the next and laid out again only when something
they show changed (decision T542): a widget changed by a method (anything
but its `Get…`, `Is…`, `Has…` and `Can…`) or by the engine, the widgets
under it and those anchored to it or masked by it; a font object or the
screen changing lays everything out again; a minimap, a model, a message
frame and what an animation plays are laid out each time. The frames'
order is kept too, until a frame is shown or hidden, made, reparented or
re-layered (decision T543). A debug build,
or `WOWLUA_SCENE_CHECK=1`, lays every scene out anew as well and stops on
a difference.

- **Order**: frame strata (`BACKGROUND` to `TOOLTIP`), frame level, then a
  frame's regions (with those of its children on its level,
  `useParentLevel`, decision T341) by draw layer (`BACKGROUND`, `BORDER`, `ARTWORK`,
  `OVERLAY`, `HIGHLIGHT` while the mouse is over its button; a region given
  none, a font string on `OVERLAY` and the others on `ARTWORK`, as the
  client's fresh regions answer, decision T338) and sublevel
  (-8 to 7), font strings after textures (decision T340), then masked textures after unmasked ones (the action
  buttons' masked icons over their slot art), then creation order; a
  frame draws before its regions'
  children frames of the same level. `SetFrameLevel`, `Raise` and `Lower`
  move a frame's descendants that have levels of their own with it
  (decision T343).
- **Textures**: the rectangle, the image (a file path or file ID, an atlas
  as the client's tables give it, or a color), texture coordinates (four
  corners), vertex color, effective alpha, blend mode (`BLEND`, `ADD`,
  `MOD`, `ALPHAKEY`, `DISABLE`), desaturation, rotation, tiling, mask
  textures, and whether it snaps to the pixel grid (on unless
  `SetSnapToPixelGrid(false)` or `snapToPixelGrid="false"`: Blizzard's
  code only turns it off; probed in WoW Forever, as is a texel snapping
  bias of 0.3 at first): a snapping texture is drawn with its edges on
  whole pixels.
- **Text**: the rectangle, the laid-out lines (wrapped and truncated as the
  engine measures them, §17.6), font file, height and flags (`OUTLINE`,
  `THICKOUTLINE`, `MONOCHROME`), color, shadow, justification and spacing.
- **Lines** (`Line` regions), status bars' and sliders' textures at their
  value, cooldowns' swipes, and a frame's backdrop (from its template's
  mixin, as any texture) are textures too.
- **Animations**: the groups playing show their animations' current
  state: an alpha animation's value (linear) in place of its region's own
  alpha, a flip book's frame (its rows, columns and frame count). A group
  set to its final alpha keeps its alpha animations' end values when it
  finishes, as `GetAlpha` reads them.
- **Message frames**: a `MessageFrame`'s messages inside it, newest at
  the bottom, each wrapped to its width, older ones above while they fit
  (unverified: fading, the insertion mode).
- **Portraits**: a texture given `SetPortraitTexture` (the client renders
  the unit's head in 3D) is the unit's model as it looked then (race, sex,
  class, what it wore) through its portrait camera, on black; where no
  model can be drawn, the client's flat portrait of the unit's race and
  sex (`TemporaryPortrait-<Sex>-<Race>`).
- **Models** (decision T318): a model frame (`Model`, `PlayerModel`,
  `DressUpModel`, …) with a model set, or a `ModelScene` with shown actors,
  is a model item over the frame's own regions: each actor's source (the
  last of `SetModel`, `SetDisplayInfo`, `SetCreature`, `SetUnit`,
  `SetItem`, `SetModelByFileID`, `SetModelByCreatureDisplayID`,
  `SetModelByUnit`, …; a unit as its race, sex, class and worn items, with
  items tried on; less what `Undress`, `UndressSlot` or `SetAutoDress(false)`
  took off), position, yaw (a frame's facing), pitch, roll, scale,
  alpha and animation; the camera (a frame's: the whole model in view from
  in front, toward its portrait camera by `SetPortraitZoom`, farther by
  `SetCamDistanceScale`, or a camera of the model's own by `SetCamera`; a
  scene's: `SetCameraPosition`, its axes, field of view and clip planes);
  the light; the session's time for animations. `ClearModel` shows nothing.
  Unverified: the client's framing, field of view and default light, and
  whether a frame's model draws over all its regions.
- **Clipping**: frames that clip their children (`SetClipsChildren`,
  scroll frames) give their rectangle to everything below them.
- **Hit testing**: the topmost mouse-enabled frame at a point, as the
  engine's input finds it, so a viewer's click is the engine's click. A
  button takes the mouse from the start; any frame once given (set,
  hooked or from XML) an `OnEnter`, `OnLeave`, `OnMouseDown` or
  `OnMouseUp` script, as the client turns the mouse on for it
  (`OnMouseWheel` the wheel, a key script the keyboard), until
  `EnableMouse(false)`.

The scene also comes as the frame tree (names, types, shown and visible
state, rectangles, text, strata, levels) for agents and inspectors.

## 18.10 Assets

Blizzard's textures, atlases and fonts are read only from the user's own
installation and never distributed or cached outside it (decisions T210,
T310).

- **Finding it**: `--wow`, `WOWLUA_WOW_PATH`, the global config, then the
  default locations, as `wowlua dev` finds it
  ([CLI §12.5](cli.md#125-wow-installation-integration)); the flavor's client
  folder picks the product (`.build.info`). Without an installation, or for
  a file it does not have, a stand-in is drawn (below).
- **Reading it**: the local CASC storage (`Data/`): the build config, the
  encoding table and the root, then each file by its path's name hash or
  file ID, from the archives' index: each of a key's entries in turn (an
  updated client lists a file twice, one entry pointing past the block's
  30-byte header), the block's BLTE data after that header or at once.
  Files are decoded once per session and kept in memory.
- **Textures**: BLP (palettized, DXT1/3/5 and uncompressed, with mipmaps)
  and TGA, from the installation or from the addon's own folder (an
  `Interface/AddOns/<Name>/…` path).
- **Atlases**: from the build's own tables (`UiTextureAtlas`,
  `UiTextureAtlasMember`, `UiTextureAtlasElement`), so they match its
  files (decision T316): a name is an element, drawn with its variant of
  the flavor's atlas set (camelot's `-c60`) over the default set's, at 1x;
  a variant's own name is drawn as itself. Where the tables cannot be read,
  the engine's recorded atlases are drawn. An atlas with nine-slice margins
  (`UiTextureAtlasElementSliceData`: left, top, right, bottom in its
  pixels, the middle stretched or tiled) is drawn in pieces: its corners as
  they are, at the region's scale, its edges and middle filled; an atlas
  that tiles repeats at its size instead of stretching.
  The API's atlases (`C_Texture.GetAtlasInfo`, `GetAtlasExists`, a
  texture's atlas size) are the installation's tables' too (decision
  T472), chosen as the API chooses: the flavor's set over the default's,
  the larger canvas (2x) over 1x, then the first; the size the member's
  override or its own over its canvas's scale, the texture coordinates in
  single precision, tiling by its flags 4 and 2, the raw size its pixels.
  Without the installation, the engine's snapshot of the same build's
  tables and the recorded answers. Checked: all 16,857 atlases of the
  snapshot and all 291 recorded answers the same.
- **Encrypted files** (decision T317): chunks encrypted with a key the
  client has (its `TactKeyLookup` and `TactKey` tables, and the keys its
  hotfix cache holds, `Cache/ADB/<locale>/DBCache.bin`) are decrypted in
  memory (Salsa20); a chunk whose key the client lacks reads as zeros, as
  in the client, so a table's records of unreleased content are left out.
- **Tables** (decision T319): DB2 tables (WDC3 to WDC5: inline, bit-packed,
  palette and common fields, arrays, copies, relationship maps), their
  fields named by the build's layout hash, else by a known layout of the
  same field count. A value from a palette or the common data is cut to
  the field's size where the layout gives it (a one-byte field's palette
  holds four bytes). What the server changed since the build is taken
  from the client's hotfix cache (`Cache/ADB/<locale>/DBCache.bin`, enUS
  first; decision T391): a record of a table (by its hash) replaces the
  record of its ID or is added, a removed one is left out. A hotfix
  record holds the fields one after another at the sizes the layout gives
  (`~n` in wowlua's layouts; a table whose layout lacks them takes no
  hotfixes), strings inline, and, four bytes more, its parent's ID in a
  relationship map (else a replaced record keeps its parent).
- **Models** (decision T318): M2 models (the chunked `MD21` form) with
  their skin profile, skeleton (`.skel`) and the `.anim` files of the
  sequences shown; a creature display through `CreatureDisplayInfo` and
  `CreatureModelData` (its skins as texture kinds 11 to 13, its scale and
  alpha, the geosets `CreatureDisplayInfoGeosetData` names), a creature
  through its first display (the client's `Creature` table's, else what
  the server told the client, its `Cache/WDB/<locale>/creaturecache.wdb`:
  decision T322), a file by ID. Geosets not named show by the
  model's defaults (the body, each group's first variant). An item
  (`SetItem`, `SetItemAppearance`) is its display's models with their
  textures (no race's in particular; one of a shoulder pair); its
  particle emitters too.
- **Characters** (decision T320): a unit (`SetUnit`, `SetModelByUnit`) is
  its race and sex's model (`ChrRaceXChrModel`, `ChrModel`) with a choice
  of each customization option (the first the class may have: the
  engine's characters have no customizations of their own), its geosets
  (the option's other choices' hidden), its skin composited from the
  choices' textures into their sections (`ChrModelTextureLayer`,
  `CharComponentTextureSections`, by blend mode), other textures (hair) by
  kind, and models skinned to it; and what it wears (`GetInventoryItemID`,
  items tried on over their slot): each item's display
  (`ItemModifiedAppearance`, `ItemAppearance`, `ItemDisplayInfo`) gives
  geosets by slot (gloves over chest over shirt for sleeves, …), textures
  in the skin's sections (`ItemDisplayInfoMaterialRes`, the file for the
  race, sex and class), a cape's texture, and models (for the race and
  sex; left and right shoulders) at attachment points (helm, shoulders,
  hands, a shield's) or skinned to the body; a helm hides the geoset
  groups `HelmetGeosetData` names for the race (hair, ears). A humanoid creature
  (`CreatureDisplayInfoExtra`) is its race and sex with its options
  (`CreatureDisplayInfoOption`), its baked skin and the item displays it
  wears.
- **Client tables** (decision T321): where the installation is at hand
  (tests and sessions with a screenshot host), the engine answers
  `C_ModelInfo.GetModelSceneInfoByID`, `GetModelSceneCameraInfoByID`,
  `GetModelSceneActorInfoByID` and `GetModelSceneActorDisplayInfoByID`
  from `UiModelScene` and its tables, so Blizzard's model scenes (the
  character sheet, the dressing room, the collections) place their actors
  and cameras; and an item the test has no data for has the inventory
  type the client's `Item` table gives it (`C_Item.IsDressableItemByID`,
  so `DressUpItemLink` opens the dressing room), and is the client's item
  (decision T327): its name, quality, levels, stack size, sell price,
  bind type and description from `ItemSparse` (a sparse table, its strings inline; else
  the name, quality and levels from `ItemSearchName`, one to a stack), its
  icon, class, subclass and slot from `Item`, its class names as
  `C_Item.GetItemClassInfo` answers, a link made as the engine makes them,
  cached, so
  `GetItemInfo`, `DoesItemExistByID` and Blizzard's item loading answer for
  it (`DoesItemExistByID` also for an item whose name `ItemSparse` lacks,
  decision T355); an item the test defines stays the test's.
  `C_Map.GetAreaInfo(id)` names an area the recordings lack as
  `AreaTable` does (T355). Without it, they answer
  nothing, as before. `GetUICameraInfo(id)` answers the client's
  `UiCamera` row as it is (the model's position, its yaw, pitch and roll,
  the animation, whether it is centered), as the recordings show; without
  the tables, the recordings. `C_TransmogCollection.GetAppearanceCameraID`
  and `GetAppearanceCameraIDBySource` answer the client's camera for the
  appearance's item on the player (decision T335): an armor slot's by the
  player's race and gender (`UiCamFbackTransmogChrRace`, its variation's,
  else the slot's first), a weapon's by its class and subclass
  (`UiCamFbackTransmogWeapon`); without the tables, the recorded cameras
  (the recording's character's). A model frame given a camera
  (`SetCameraPosition` and `SetCameraTarget`, as `Model_ApplyUICamera`
  does) is seen from it, in camera space (decision T334): model space is
  camera space times 0.8171 times the frame's effective scale
  (`TransformCameraSpaceToModelSpace` scales by it), so the camera's
  position and target are taken back by that factor, the model stands at
  its `SetPosition` at its own size, pitch and roll turn it about the
  view's axes after its facing, and the field of view is 50 degrees
  across the frame's diagonal.

  A spell no test defines (decision T391) is the client's: its name, rank
  and icon from `SpellName`, `Spell` and `SpellMisc`, its cast time
  (`SpellCastTimes`), range (`SpellRange`, the first of its pair) and
  costs (`SpellPower`: rage and runic power in whole points, a finisher's
  cost the most it takes, `minCost` the least), so
  `C_Spell.GetSpellInfo`, `GetSpellSubtext`, `GetSpellTexture`,
  `DoesSpellExist` and `GetSpellPowerCost` answer for it in every flavor;
  a cost in percent of the base power is the character's and is not
  given. `C_Spell.GetSpellDescription` gives the client's text with its
  placeholders filled for the player as the engine has them (level, the
  school's `GetSpellBonusDamage`, `GetSpellBonusHealing`, attack power):
  `$s` an effect's points (`EffectBasePointsF`, grown by
  `EffectRealPointsPerLevel` from `SpellLevels`' level to its maximum,
  plus the spell power coefficient times the power; with a `Variance`, "low
  to high", the spread halved each way, rounded; else rounded down),
  `$m`/`$M` the low and high ends, `$o` the points times the whole ticks
  of the duration (`EffectAuraPeriod`), `$t` the period in seconds, `$d` the
  duration ("12 sec", "1.5 sec", "10 min", "1 |4hour:hrs;", "2
  |4day:days;"), `$a` the radius, `$h` the proc chance, `$u` the stacks, `$n`
  the charges, `$x` the chain targets, `$r` the range, `$12345s1` another
  spell's, `$/1000;` and `$*2;` scaled (a range's ends each),
  `$lpoint:points;` by the number before it, `$@spellname12345` and
  `$@spelldesc12345`, formulas `${$m1*8}` (numbers and placeholders,
  `+ - * /`, parentheses; the placeholders in them signed, the result
  whole, `${…}.2` to two decimals); numbers from 1,000 up with a
  separator. A description with a placeholder it cannot fill exactly
  (conditions `$?`, variables `$<…>`, functions in formulas, gender `$g`)
  is the recorded one, else empty. A spell's tooltip
  (`C_TooltipInfo.GetSpellByID`, `GameTooltip:SetSpellByID`) shows its
  cost and range ("435 Mana", "30 yd range"), its cast time and cooldown
  ("Instant" or "10 sec cast", "8 sec cooldown") and the description, as
  the trainers' recorded tooltips do (decision T471: a cost on each line,
  a finisher's "1 to 5 Combo |4Point:Points;"; "Melee Range" for a melee
  range, "8-25 yd range" for one with a minimum, "Unlimited range"; the
  range on the left for a spell without any cost; "Passive", "Next
  melee" or "Channeled" in place of the cast time; "Requires Bear Form,
  Dire Bear Form" in red for a spell that needs a form, "Requires
  Daggers" for one that needs a weapon the player does not wield, its
  tools and reagents in red, from `SpellShapeshift`,
  `SpellEquippedItems`, `SpellTotems` and `SpellReagents`). A trainer's
  service is its spell's tooltip without the rank, its description and
  icon the client's, the description as for a spell not known (`$w1`, an
  aura's live value, left as written; `GetSpellDescription` fills it;
  decision T471). Checked against the
  1,789 recorded trainer services: every icon the same, 1,624 tooltips
  the same but for the character's numbers (its level, spell power, base
  mana, haste); unverified: the rest (ranges no table tells, as Maul's
  none, costs in health, reagents the character held).
  `C_Spell.IsSpellPassive` (`SpellMisc`'s attributes 0, 0x40) and
  `GetSpellBaseCooldown` (its own cooldown or its category's, the longer,
  and the global one it starts; `SpellCooldowns`) answer from the tables:
  the 1,343 and 1,307 recorded answers the same. The installation's current data
  wins over the recordings and camelot's scan of older builds (spells and
  items alike); a test's own data wins over both. Checked against the client's recorded descriptions (about
  2,000, `wowlua-casc`'s installation tests): the same, except where the
  recorded character's gear, talents or a damage modifier count (2 % more
  damage on a level-21 druid). Unverified: talents and auras that change a
  spell's numbers or costs (the client's answers include them: a druid's
  feral abilities cost 2 less, Improved Thunder Clap), spells changed by
  the server beyond its hotfixes (Hunter's Mark shows half its table's
  value).

  The talent trees (decision T435) are the client's where its tables are
  at hand: a tree's nodes (`TraitNode`, by ID), each node's entries,
  groups, conditions and edges in their tables' order
  (`TraitNodeXTraitNodeEntry`, `TraitNodeGroupXTraitNode`,
  `TraitNodeXTraitCond` then its groups' `TraitNodeGroupXTraitCond`,
  `TraitEdge`), an entry's definition and ranks (`TraitNodeEntry`), a
  definition's spell and overrides (`TraitDefinition`), a rank's cost
  (the node's own or its groups' costs, else its tree's:
  `TraitTreeXTraitCost`, `TraitCost`), whether its ranks can be refunded
  (the tree's flags), and a condition's points to spend in its node group
  (a tab of the tree; else its tree), its flags and its tooltip
  (`TraitCond`, `SharedString`). `C_Traits.GetTreeNodes`, `GetNodeInfo`,
  `GetEntryInfo`, `GetDefinitionInfo`, `GetNodeCost` and
  `GetConditionInfo` answer from them; a node or entry the build lacks
  answers nothing. The character's own part stays the recording's (its
  configs and their trees, the ranks bought, the currencies' maxima, the
  traits' descriptions), as do the trees without the tables. Checked
  against the recorded answers of 11 characters (3,578 answers,
  `wowlua-render`'s installation test): the same, except the five mage
  nodes the build changed since the recording. Unverified: costs of a
  node's own besides its tree's, conditions of quests, achievements or
  levels (camelot's have none: they count as met).

  A transmog source or appearance the recordings lack (decision T462) is
  the client's: a source's item, appearance, modifier and source type
  (`ItemModifiedAppearance`; the API's source type renumbers the table's 7
  and 8 as 5 and 6 and has none for its 0 and 5), its slot (the item's
  inventory type plus one) and category (`Enum.TransmogCollectionType`:
  armor by its slot, shields and off-hands by theirs, weapons by their
  subclass), an appearance's sources by ID; `GetAppearanceSources`,
  `GetAllAppearanceSources`, `GetValidAppearanceSourcesForClass`,
  `GetSourceInfo`, `GetCategoryForItem` and the collection checks answer
  for them, not collected unless the test collects them, with the flags of
  an item not loaded (all false). A recorded appearance keeps its recorded
  sources and a category its recorded appearances: the client listed what
  the server let it (the tables hold 2.4 times the appearances and more
  sources for half of them, and no field of theirs tells which are shown).
  Checked against the 5,359 recorded sources: the same, their character's
  flags and loaded items' names aside.

  A world map's flight paths (`C_TaxiMap.GetTaxiNodesForMap`, decision
  T463) are the client's: the `TaxiNodes` of each continent the map is
  assigned to (`UiMapAssignment`, by its ID), shown to the Alliance, the
  Horde or both (its flags' 1 and 2: the faction and its icon), the
  file's records then those the server changed, each placed by the
  assignment's world rectangle and its part of the map (off the map
  beyond 0 to 1); never undiscovered (recorded; unverified for a
  character that has not found them). A flight map (the taxi system's)
  stays the open flight master's. Checked against the 1,960 nodes of the
  58 recorded world maps: the same, positions to 0.0001.

  Calls whose answers are the client's tables' (decision T464) are
  answered from them where a recorded answer would be taken, before the
  recordings: a world map's info, children (of a type, all descendants
  after each), rectangle on another map of its continent (else zeros),
  world size (its first part by order, each extent over the other axis's
  part of the map, as the client gives Azeroth's), art ID, art layers and
  their tiles (row by row), art text positions and whether it has art
  (`UiMap`, `UiMapAssignment`, `UiMapXMapArt`, `UiMapArt`,
  `UiMapArtStyleLayer`, `UiMapArtTile`). Checked against the 809 recorded
  answers of these calls: the same.

  An achievement's own data (decision T465) is the client's
  (`Achievement`, `Achievement_Category`, `TraitCurrencySource`):
  `GetAchievementInfo(id)`'s ID, name, points, description, flags, icon
  (nil for none), reward, guild and statistic; its category, the one it
  follows and the one following it in its series; a category's name and
  parent (-1 under none or the statistics' root, 1); the achievement and
  statistic category lists (the statistics' root's descendants or the
  others, by display order then ID); `C_AchievementInfo.IsValidAchievement`
  and `IsGuildAchievement`; `C_Traits.GetTraitCurrencyForAchievement`
  (the sources' amounts). The character's part of an answer stays the
  recorded one: whether and when it was earned and by whom (none for an
  achievement not recorded). Which achievements a category lists, their
  criteria and their order stay recorded: they follow the character (a
  series its current step, faction-only criteria, "most used" statistics
  one, three quarters of the lists by the character's counts). Checked
  against the 2,236 recorded answers of these calls: the same.

  An item class's and subclass's names (decision T466) are the client's:
  `C_Item.GetItemClassInfo` the class's (`ItemClass`; nil for none),
  `GetItemSubClassInfo` the subclass's verbose name, else its shown one,
  and whether it is an armor type (its flag 512; `ItemSubClass`; nothing
  for none). Checked against the 129 recorded answers: the same.

  A profession window's recipes (decision T470) are the client's: a
  recipe's schematic (its spell's name, the item it makes and how many,
  `SpellEffect`'s create item; its reagents in order, `SpellReagents`), its
  requirements (its spell focus, `SpellCastingRequirements` and
  `SpellFocusObject`; its tools, `SpellTotems` and `TotemCategory`), its
  category (`TradeSkillCategory`: a header without a parent, else a
  subheader) and its info's own fields (category, skill line ability, its
  lowest ID's, skill-ups, trivial rank, the item's icon and level). The
  character's part stays the recorded window's: whether a recipe is
  learned, its colour, whether it can be made or raise the skill, its link,
  a requirement met, and the level of an item the tables lack (one the
  server sent); a recipe not recorded is unlearned, without a link. Which
  recipes a window lists and their order stay recorded (the order follows
  the character's skill: the colours, then the trivial ranks and the
  items' levels). An item with no icon of its own (`Item`) has its first
  appearance's (`ItemAppearance`), for recipes and items alike. Checked
  against the 1,609 recorded answers of blacksmithing and mining: the
  same.
- **Fonts**: the client's TrueType fonts from the installation; without
  them, the bundled stand-in fonts (open licenses), at the same pixel
  heights. Glyph positions follow the engine's measured widths (§17.6), so
  layout does not depend on which font draws; a glyph the recorded widths
  lack is measured with the installation's font file (decision T392,
  [Emulator §17.6](emulator.md)).
- **Stand-ins**: a texture that cannot be read is drawn as a checkered
  magenta rectangle with its vertex color, so a missing image is visible;
  `--strict-assets` makes a missing file an error.

## 18.11 Renderer

`wowlua-render` draws a scene with `wgpu` (decision T311): Metal, DirectX 12
or Vulkan natively, WebGPU or WebGL 2 in the browser.

- **Targets**: a window's surface (Studio), or an off-screen texture read
  back as RGBA and written as PNG. Reading back is asynchronous, as the
  browser requires; native callers block on it.
- **Drawing**: one batched pass of textured quads in scene order, with the
  client's blend modes, premultiplied alpha, linear filtering and texture
  coordinates; masks as a second texture; text from a glyph atlas
  rasterized from the font at the scene's pixel height, with outlines and
  shadows, each line's baseline its height times the font's ascent over
  its ascent and descent (as the client's page number sits between the
  action bar's arrows).
- **Models** (decision T318): each model item is drawn into a texture of
  its rectangle's size with a depth buffer, then into the UI pass in its
  place (premultiplied alpha): the actors posed on the CPU at the
  animation's time (bones, then skinning), each visible batch with its
  material's blend mode (opaque, alpha key, alpha, add, mod, mod 2x),
  texture (the model's own, or the display's by kind; mipmapped, each
  level the average of the one above, so a small portrait averages its
  texels as the client's does), color and
  transparency tracks, texture transform, lit by the frame's light unless
  unlit; up to three textures a batch, combined by the client's shaders
  (its shader ID: the vertex shader, which coordinates each texture takes,
  the second set, an environment map, an edge fade; the pixel shader,
  how they combine: modulated, doubled, added, by alpha, glows); particles
  as their emitters' steady state at the moment (each particle's start,
  direction, speed and life from a seed of its own, so a frame is the
  same each time and the next follows on; its color, alpha and size over
  its life; facing the camera, with the emitter's blend mode; pulled by its
  gravity, a compressed one's direction too; its texture cell from its
  head's (or tail's) cell track over its life, or a random one where the
  emitter says so; a tail where the emitter has one (its head-or-tail 1
  or 2; other values, newer emitters', a head alone), from the particle
  back along its velocity times the tail length, across the view). Model
  files are read once; a frame's model textures are reused from frame to
  frame. Unverified: drag, the cell tracks' reading (interpolated, then
  whole), the tails' width. Billboarded bones (a
  spherical one, or one locked to its Z axis) face the camera as it will
  be placed: the scene's, the model's own, or the frame's from in front
  turned toward the portrait camera by the zoom; their children follow
  (bones locked to X or Y as posed, unverified). Ribbons trail behind
  their bones: the bone's point where the model was posed over the edges'
  lifetime (an edge each 1 / edges a second), each edge its height above
  and below along the bone's Z axis, the older ones pulled down by its
  gravity, with the ribbon's first texture and material, its color and
  alpha at the moment (unverified: the strip's fade along its length, its
  texture's rows and columns). Material blend 7 adds, never darkening
  what is behind (in game a totem's rune, its texture without alpha, draws
  no black; probed).
- **Minimap** (decision T324): a `Minimap` frame shows the terrain about the
  player, under its regions: the minimap textures of the 533⅓-yard terrain
  tiles its zoom covers (466⅔, 400, 333⅓, 266⅔, 200 or 133⅓ yards across
  its width at zooms 0 to 5), north up, each cut to the frame and masked by
  `SetMaskTexture`'s atlas or file; with `rotateMinimap` set, turned
  clockwise by the player's facing (`mock.moveTo`'s `facing`) about its
  center, whole tiles cut by the frame's rectangle and the mask, and the
  texture named `MinimapCompassTexture` turned with it, its north toward
  north (as the client turns it; no Lua does; probed); the
  player's arrow (`Interface\Minimap\MinimapArrow`, 32 by 32 UI units)
  at its center, over its regions, turned by the facing, or up when the
  minimap turns. The tiles come from the
  map's WDT (`Map`'s `WdtFileDataID`, else `world/maps/<dir>/<dir>.wdt`):
  the last of each tile's eight file IDs in its `MAID` chunk, row by row
  from the north-west; the player's tile is column `32 - y / 533⅓`, row
  `32 - x / 533⅓` of `UnitPosition`'s north `y` and west `x`. The minimap
  follows `mock.moveTo` and `SetZoom` at the next frame. Group members
  with a place in the world (a unit's `world`) are their class's blips
  (`Interface\Minimap\PartyRaidBlips`, its 8 by 4 grid in class ID order,
  16 UI units) within its circle; a ping (`PingLocation`: kept where it
  was in the world, `GetPingPosition` its place from the player in widths
  of the view; `MINIMAP_PING("player", x, y)` to event callbacks: on
  camelot a callback event alone, `RegisterEvent` refusing it, and a
  restricted one, Blizzard's callbacks alone, decision T336) shows for
  5 seconds
  as the client's center, growing ring and turning marks (decision T329).
  Indoors nothing (the building's own map, not read), no quest areas or
  tracked objects. Unverified: the zoom sizes, the arrow's and blips'
  sizes, the ping's look and time, and the order of the terrain and the
  frame's regions on this client.
- **Speed**: textures and glyphs are uploaded once and kept on the GPU;
  quads go in one vertex buffer per frame, drawn in as few calls as texture
  changes allow; nothing is drawn when the scene did not change.
- **Adapter**: any GPU, or a software adapter (WARP on Windows, lavapipe on
  Linux); without one, rendering fails with an error that says so.

**Screenshots** are the whole screen or one frame's rectangle (its
descendants included), at the screen's size (`test.screen`) or a given
scale.

**Comparison** (decision T312). A screenshot matches a stored one when at
most `maxDiffRatio` of its pixels (default 0.001) differ by more than
`threshold` (default 0.1, in YIQ color distance as pixelmatch measures it),
so GPUs that round differently still match. A mismatch writes the actual
image and a diff image next to the stored one.

## 18.12 Front-ends

### 18.12.1 `wowlua debug`: Debug Adapter Protocol

`wowlua debug` speaks the [Debug Adapter Protocol](https://microsoft.github.io/debug-adapter-protocol/)
on stdio, or on a TCP port with `--port <n>` (decision T313). `launch`
takes `test` (a test file) and `filter`, or `addon: true`; and `flavor`,
`profile`, `stopOnEntry`, `clock` (`realtime` | `manual`) and
`debugBlizzardUI`. It supports breakpoints of every kind of §18.4 (line
breakpoints with conditions, hit counts and log points; function
breakpoints for functions, API calls, events, scripts and timers;
exception breakpoints for `uncaught`, `all`, `taint` and `blocked`),
stepping, one thread (the client runs one; a coroutine's stack goes on
with the code that resumed it), stack traces, scopes, variables,
`setVariable`, `evaluate` (watch, hover, repl: a line starting with `/` is
a slash command, as typed in chat, `/wdemo`), `pause` and `terminate`. In
the addon's session a `/reload` (`ReloadUI()`, `C_UI.Reload()`) loads the
UI again as the client does: a fresh VM, the SavedVariables kept, the
clock going on, the reload's login, the breakpoints on the new load
(decision T361)
(during a stop it ends the session at once). A session runs one flavor,
the package's first unless `flavor` names one; the console names it as the
session starts, and the VS Code extension asks which of the package's
flavors to debug when the configuration names none and the package has
several (decision T358). A frame of code that is not the package's carries
its file when there is one on disk (Blizzard's UI in the local cache, a
`test.addons` folder's file), so the editor opens it at the line; it is
shown dimmed when one of the package's frames lies below it (the editor
then shows that one), else not (a stop in Blizzard's UI as it loads shows
there). Once a session ends, nothing more loads.
Custom requests: `wowlua/screenshot` (PNG as base64), `wowlua/frameTree`,
`wowlua/findFrames`, `wowlua/input` (click, hover, drag, type, key, slash,
and `chat`: a message as the player sends it from the chat box,
`SendChatMessage(text, chatType, nil, target)`, so addons get its
`CHAT_MSG_*`), `wowlua/chatLines` (what the chat windows, `ChatFrame1` and
on, showed from line `since` on: each line's number, window, text and
colour, the last 1000 kept, and the next line's number; decision T514),
`wowlua/advance`, `wowlua/fireEvent`, `wowlua/world` (`script`: Lua with
`mock` the engine's controls, run between frames as a world profile is
applied, so the running world changes: `mock.player:setMoney(500)`;
decision T401), `wowlua/profile` (`start`, `stop`:
the report). A value's taint shows in its type (`string · tainted by
MyAddon`), a secret's value after `secret`.

The VS Code extension shows the game with them (decision T359): the
**UI panel** (beside the editor; opened as a session starts, or with
*wowlua: Show UI*) draws `wowlua/screenshot` at every stop and step, and
while the game runs every `wowlua.ui.refreshInterval` milliseconds (1000;
0 only at stops and on Refresh). It zooms: the wheel with Ctrl or Cmd (a
trackpad's pinch) at the pointer, its buttons, *Fit* and *1:1* (the
screen's own pixels), panned with the scrollbars, the wheel, or a drag
with the middle button or Space held; *Zoom to frame…* picks a visible
named frame and zooms so it fills the panel, outlined (the Frames view's
zoom button too): opened, it lists the frames shown, renewed as they
come without closing, filtered as their names are typed (arrows and
Enter pick). Closed and opened again during a session, the panel draws on
as before. Shown larger than its pixels, the screen is drawn at 2
(above 1.25 device pixels per screen pixel) or 3 times (above 2.5)
`wowlua/screenshot`'s scale, so it stays sharp. A click in it is `wowlua/input`'s click at that pixel (the
right button for a right-click), so the frame under it gets the click and
its handlers' breakpoints stop; while the session is paused, clicks wait
for it to run. The **wowlua Frames** view in the debug sidebar is
`wowlua/frameTree`'s shown frames, refreshed at every stop (and with its
Refresh button); each shows its type, size and text, and selecting one
outlines its rectangle in the panel.

### 18.12.2 `wowlua mcp`: Model Context Protocol

`wowlua mcp` is an MCP server on stdio for AI agents (decision T314). Its
tools: `start_session` (a test file or the addon, flavor, profile, screen),
`stop_session`, `screenshot` (the screen or a frame, returned as an image),
`frame_tree` (optionally under a frame, with hidden frames or not),
`find_frames` (by name, type or text), `click` (a frame, or a point in the
screenshot's pixels), `hover`, `drag`, `type_text`, `press_key`, `slash` (a
slash command), `chat` (a message the player sends, decision T514), `advance`, `fire_event`, `world` (as `wowlua/world`), `eval`, `set_breakpoints`, `continue`,
`step`, `stack`, `variables`, `profile_start`, `profile_stop`, `output`
(what the session printed and the errors it raised). Sessions are `manual`
clocked unless asked otherwise. A tool that runs code (`continue`, `step`,
`click`, `advance`, …) answers once the session stops or settles, with its
state (where it stopped) and what it printed meanwhile. Profiles count game
frames as one node (`game frame`), each frame's cost in the report's
timeline.

### 18.12.3 In process: wowlua studio

wowlua studio links the debugger and runs a session on a thread of its own
(decision T484): `session::run_hosted` takes a frame hook that runs there
with the client's VM whenever the client may have changed (after each game
frame, an `advance`, requests served, and at a stop, a test file's session
too: decision T494), in which Studio builds the scene
(§18.9) and hands it to its UI thread, which draws it with the renderer
(§18.11) on Studio's own GPU device: no screenshots, no polling. Besides the
requests above it has:

- `inspect(frame)`: a frame's or region's details by its frame tree `id`:
  strata and level (a region: layer and sublevel), rectangle, alpha and
  scale as set and as drawn, anchors, scripts and hooks with where their
  functions are, registered events, regions (layer, texture or atlas,
  text), child frames, and the Lua line that made it (`created`).
- `pick(x, y)`: what draws topmost at a point of the screen, taking the
  mouse or not: the frame, and the region when a region draws there.
- `units` (the world's units: index, GUID, tokens, name, level, class,
  reaction, health, distance, position, whether targeted) and
  `target(unit)` (the player targets it as a click in the world does).
- `logEvents(on)` and `events`: the events told since the last read, with
  their arguments, the frames they reached and their handlers' time
  (decision T483).
- `setScreen(width, height, uiScale?)`: the game's settings (decision
  T482), as the Graphics settings change them.
- Input `dragAt(x, y, dx, dy)`: the frame at a point dragged by an offset,
  as the mouse drags it.

For its debugger page (decision T494):

- `breakpoints`: every breakpoint with its state and how often it was
  hit this session (its condition held), line breakpoints with their
  source.
- `stopped` carries an exception's `detail`: an error's message, the stack
  it was raised in and whether a handler caught it; a blocked action's
  addon, action and taint chain; a taint's addon and chain.
- `continued` after every resume (a continue, a step), `reloaded` after a
  `/reload`.
- `pause` while the addon runs stops it at the next line run, else
  between frames.
- `setClock(manual)`: the clock from now on, over the launch's.
- `setDebugBlizzardUi(on)`: stepping into Blizzard's UI code from now on,
  over the launch's `debugBlizzardUI` (decision T496).
- `frameTree` marks a frame the client or Blizzard's UI code made
  (`blizzard`), so a tree can leave them out (decision T496).
- `saveWorld`: the running world as a world profile's JSON (the
  character, its money and place, the other units with the target,
  focus and group, the CVars changed); items, bags, spells, auras and
  saved variables are not written.
- `units` tells each unit's threat on the player's target, whether the
  player sees it (`sight`: the line between their eyes meets no hill or
  building; none without a place) and where it faces (`facing`, radians;
  decision T546), and `mock.unitAt(index)` is the unit at that place of
  the list (in `world` scripts).

### 18.12.4 `wowlua screenshot`

`wowlua screenshot <test file> [--test <name>] [--frame <name>] [--flavor <f>]
[--scale <s>] -o <file.png>` runs the test file (its tests whose names contain
`--test`) and draws the screen, or the named frame and what it holds, as each
test leaves it: the first test's image at `-o`, the others' beside it
(`<file>-<test>.png`).

### 18.12.5 In tests

`expect(frame).toMatchScreenshot(name?, { threshold?, maxDiffRatio? })` (or
`expect(mock.screen)` for the whole screen) compares the image with
`__screenshots__/<test file>/<test>-<name>.png` next to the test file (its
key, the test's name and `name`, as a file name: spaces as dashes; decision
T315): a missing image is written, a different one fails the test with the
share of pixels that differ and writes `<key>.actual.png` and
`<key>.diff.png` beside it, and `wowlua test -u` rewrites it, as snapshots
(the summary counts them with the snapshots). `mock.screenshot(name?,
frame?)` draws now, writes `<build.out>/screenshots/<test file>/<name>.png`
and returns its path. The images are drawn at the screen's size
(`test.screen`), with the installation's textures and fonts when there is
one, else the stand-ins: a project's stored images are made on machines
that agree on that.
