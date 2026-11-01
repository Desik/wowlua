# Roadmap

Milestones are ordered so that every milestone produces a usable tool. Each
lists its exit criteria and, once reached, its status.

## M0: Foundation

**Status: done (2026-10-11).**

- Cargo workspace, CI (fmt, clippy, tests on macOS, Windows, Linux), release
  pipeline publishing binaries to GitHub Releases on tags (Homebrew, winget and
  Scoop follow later).
- `wowlua-span`, `wowlua-diagnostics`, `wowlua-flavors` (flavor table),
  `wowlua-manifest` (JSON schema generation, profile merging, `package.wowlua`
  check), `wowlua-toc` (layouts, suffix algorithm, line-length rules),
  `wowlua-xml` (reference checks).
- CLI skeleton: `new`, `init` (from TOC), `build` for **plain Lua projects**
  (passthrough, TOC generation, assets, `LICENSES.txt`, output validation),
  `install`, `uninstall`, `dev` (copy and link modes, safe installs), `clean`,
  `toc`, `explain`.
- `.wowlua` files are accepted when they are pure Lua 5.1 and passed through as
  `.lua`; files using wowlua syntax report that they need the M1 compiler.
  Bundling and minification are not available yet (release builds are
  unbundled, with a note).
- Output validation of Lua syntax uses the embedded Lua 5.1 VM (`mlua`,
  compiling without running) until the compiler's own parser exists in M1.
- Lockfile with the pinned flavor table.

**Exit:** an existing Lua addon can be built and installed from a `wowlua.json`
with generated TOCs for several flavors.

## M1: Parser and emitter

**Status: done (2026-10-11).**

- Lexer and parser for full Lua 5.1 plus all wowlua syntax, with error recovery.
- Lua IR and emitter with line modes, hygiene and source maps.
- Superset conformance suite: Lua 5.1 tests, addon corpus, bytecode round-trip.
- `wowlua check` (syntax only), `wowlua fmt` (initial: comment-preserving
  printer for Lua 5.1 constructs).

**Exit:** the corpus parses with zero errors, and round-trip bytecode is
identical for every corpus file.

Notes on the delivered state:

- `.wowlua` files are compiled with type-only syntax erased (decision T64);
  other wowlua syntax reports the milestone that implements it.
- The corpus (28 pinned addon releases and the Lua 5.1 test suite, 7,179
  files, 4.8 million lines) parses with the same accept/reject result as Lua
  5.1, and every file round-trips to identical bytecode in all line modes; in
  `preserve` mode even the line info is identical. The formatter formats every
  corpus file idempotently without changing its meaning.
- Fuzzing is done with deterministic randomized tests on stable Rust
  (decision T67). `wowlua check --watch` follows with incremental checking
  (M4).

## M2: Lowering of syntax sugar and modules

**Status: done (2026-10-11).**

- `+=`, `continue`, `?.`, `??`, interpolation, bitwise, `//`, literals, `const`,
  defaults, destructuring, preceding-statement linearization.
- Modules: `import`/`export`, resolution, declaration hoisting, runtime `core`
  slice.
- Load-order analysis: usage analysis of globals and addon-table fields,
  load-time/run-time edges, lazy bindings for run-time cycles, per-flavor
  module graphs and the union order for single TOCs.
- `wowlua:meta` (including `asset()`), static conditions, `@if`, flavor split
  builds, XML reference rewriting.

**Exit:** multi-module example addons build for all flavors and run in the
embedded Lua VM with identical behavior to hand-written Lua. A migrated
plain-Lua addon keeps its working load order.

Notes on the delivered state:

- Lowering is checked by differential tests that run compiled `.wowlua` and
  hand-written Lua in the embedded VM; multi-module and split-flavor packages
  are built and run per flavor in the CLI tests.
- `cargo xtask corpus addons` adopts every corpus addon with `wowlua init`
  and builds it: all 108 keep their TOC order. The usage analysis
  over-approximates without types; where it contradicts the listed order the
  listed order wins with W1035 (decision T70).
- Classes, enums and decorators other than `@if` report M3.

## M3: Classes, enums, decorators and stdlib infrastructure

**Status: done (2026-10-11).**

- Classes (including abstract and generic), widget classes, interfaces (parsed;
  checked in M4), enums.
- Core decorators (`@global`, `@deprecated`, `@if`, `@inline`) and user
  decorators.
- Runtime slices `class`, `widget`, `decorate`.
- Standard-library infrastructure: compiling `wowlua:*` modules from
  `wowlua-stdlib` into addons, tree-shaking them, and the `@intrinsic`
  decorator table, so later milestones can ship modules.

**Exit:** a UI-heavy sample addon written with widget classes works in game.

Notes on the delivered state:

- The sample addon is `examples/widget-demo` (decision T80); CI builds it and
  runs it against a mock frame API. It was checked in game on the WoW: Forever
  beta client (1.60.1).
- Class, enum and decorator checks that need only declarations are reported
  now (decision T75); type-based ones come with M4.
- Native widget types are declared by the compiler until M4 (decision T77).
- No public standard-library modules yet: the infrastructure is checked with
  test modules through `WOWLUA_STDLIB_DIR` (decision T79).

## M4: Type checker

**Status: done (2026-10-12).**

- Basic mode: inference, narrowing, assignability, arity, globals, classes,
  interfaces, generics, overloads.
- Declaration files. `wow-api-<flavor>` packages generated per flavor from
  Blizzard's API documentation and merged with Ketho's annotations;
  `wowlua types sync`, `wowlua types convert`.
- Strict mode, `Secret<T>`.
- `shared` flavor mode with flavor guards (E2030).
- `wowlua lint`, `wowlua fix`.

**Exit:** the corpus checks in basic mode with zero errors. Sample addons
check in strict mode with zero errors.

Notes on the delivered state:

- The corpus (108 addons, plain Lua checked with `check.lua`) checks with
  zero errors; applying all lint fixes to it keeps every addon building.
  `examples/widget-demo` and `examples/typed-demo` check in strict mode
  without diagnostics, and CI runs both.
- The `wow-api` declarations are embedded in the compiler (crate
  `wowlua-api`) and regenerated with `cargo xtask types fetch` and
  `cargo xtask types gen` (decisions T83–T85). The `wow-api-<flavor>` types
  packages and `wowlua types sync` arrive with the package manager.
- Leniency rules that keep valid Lua free of errors are decision T87;
  feature detection (existence tests) works in every build mode (T89).

## M5: Debugging

**Status: done (2026-10-12).**

- `wowlua:debug`, `wowlua:reflect`, `wowlua:log` (on the M3 stdlib
  infrastructure).
- Profiling, tracing, runtime checks, dev console.
- `wowlua trace`.

**Exit:** a profiling report appears in game, and runtime checks catch bad
values from the WoW API in a dev build.

Notes on the delivered state:

- Checked in game on the WoW: Forever client with `examples/typed-demo` and
  `examples/widget-demo`: the dev window's profiler, monitor (events,
  scripts, timers), log and errors tabs, and `/tdemo check`, whose runtime
  check catches a `nil` from `UnitName`.
- Dev builds have every tool built in without code changes (decision T96):
  every function is profiled, plain `.lua` files included (T97), handlers
  are monitored, errors are captured, and `/wl` opens the dev window, drawn
  in the SuuUI design with its fonts shipped (T101–T103). The corpus builds
  this way with zero failures.
- `wowlua trace` maps locations of line-preserved builds; decoding bundled
  builds through source maps arrives with bundling (M7).

## M6: Package manager

**Status: done (2026-10-12).**

- Path, git and archive sources, lua-library embedding (reachable files and
  licenses only), lockfile, PubGrub resolution over the union, cache,
  workspaces with generated declaration files.
- `add`, `remove`, `update`, `fetch`, `tree`, `outdated`, `cache clean`,
  `fix --manifest`.
- Dependency safety (archive extraction rules, submodule policy).
- `.pkgmeta` import and compatibility mode, packager keywords.

**Exit:** an Ace3-based addon migrates with `wowlua init --from-pkgmeta` and
builds identically.

Notes on the delivered state:

- Sources: git (bare clones, exported per commit), SVN (system `svn`,
  pinned to a revision), path and archive (`https`, `file://`); the registry
  came with M10 (T104), and the wow-api types stay embedded.
- The exit check (`packager-compat` workflow, T119): thirty-six addons built
  with the BigWigs packager, at pinned commits, packaged by the packager the
  way their CI does (multi-version, `-S`, or game by game with `-g`/`-m`)
  and built by wowlua after `init --from-pkgmeta`, load identical code in
  every flavor and client locale and ship the same files (the same bytes
  outside `src/`, with the packager's line endings and TOC versions): Bartender4, BigWigs (12 AddOns),
  LittleWigs (12), WeakAuras (5), DBM (8, with its voice pack as an external
  addon), Details (8), ElvUI (3), Inventorian (2), Plater, Masque, Mapster,
  Quartz, BugSack, SimulationCraft, AdvancedInterfaceOptions, GatherMate2
  (4), ShadowedUnitFrames (2), AtlasLootClassic (8), RCLootCouncil2,
  omnibar, Capping (2), BigWigs_Voice, BigWigs_Classic, SexyMap,
  MythicDungeonTools (2), TellMeWhen (2), AllTheThings, Dominos (6), OmniCC
  (2), Cell, TotalRP3 (2), BlizzMove (2), QuickQuest,
  BigWigs_WrathOfTheLichKing, Tukui and KuiNameplates (4). Rarity and hekili
  are left out: the packager itself cannot fetch their LibCompress SVN
  external. The corpus still adopts and builds all its addons.
- wowlua `library` packages compile into the addon (tree-shaken modules,
  their own check settings and meta, T110–T112); workspaces share one lock
  and `dist/`, with generated declaration files of `@global` APIs (T113,
  T114).

## M7: Optimization and publishing

- Bundling (with module isolation and XML-aware segments), minification passes
  and levels, global caching, inlining, dead-code removal, Lua-limit budgets.
- `package`, `publish` for CurseForge, Wago, WoWInterface and GitHub
  (`release.json`, archived source maps, `split-flavors`, `nolib`,
  `bundle-addons`), `login` with OS credential stores, changelog handling.
- `wowlua new --ci github` template.

**Exit:** a release is published to all four sites from CI with one command.

- Done 2026-10-13 with SuuUI, a WoW Forever addon adopted with `wowlua init`:
  one tag push (`v0.2.0-beta.2`) made CI publish to CurseForge and Wago (the
  readable build, as betas) and to GitHub Releases (the fully optimized
  build, with source maps and `release.json`). WoWInterface takes no WoW
  Forever addons yet; its upload is covered by the publish tests against a
  mock site.

- Optimization was checked in game on the WoW: Forever client (2026-10-13):
  Bartender4 adopted with `wowlua init` and built bundled (`strip`, release
  optimizations) loads from 2 bundles and its template XML, with no Lua
  errors; so does the build at `max` with `minify-libraries` (Ace3 and the
  other libraries renamed, compacted and optimized too: 774 KiB of Lua
  instead of 1.29 MB, compiled in a third of the time). The corpus checks every minification pass on 7179 files and builds
  its 108 addons bundled at every level.

## M8: Standard library

- Core set first: `events`, `lifecycle`, `slash`, `saved`, `timer`, `combat`,
  `hooks`, `log`, error policy.
- Then `locale` (with the compiler's locale pipeline: JSON compilation, the
  `L` type, client-locale TOC directives), `pool`, `settings`, `launcher`,
  `comm`, `serialize`, keybindings (with `Bindings.xml` generation), tooltip
  hooks, movable frames.
- Then unit/aura/spell helpers, colors, SharedMedia interop, item/spell data
  cache, callback registry, background jobs, dialogs, menus and lists.
- Widget themes for dialogs, the fallback menu and the built-in minimap button
  (decisions S17, S18).

**Exit:** a sample addon uses no third-party libraries for its events, saved
variables, settings, minimap button, keybindings and comms.

**Status: done (2026-10-15).** The stages follow the bullets above: S1 the
core set, S2 `locale` to movable frames, S3 the helpers, registries, jobs,
dialogs, menus and lists, then the widget themes, and S4 the patterns found
across the corpus. Every module ran in game on WoW: Forever (StdTest: 109
tests, automatic and guided, all passing). Released as 0.9.0.

- S1 done 2026-10-14: `errors`, `events`, `lifecycle`, `timer`, `combat`,
  `hooks`, `slash`, `saved` and `callbacks` (with `log`, `debug` and `reflect`
  from M5), written in wowlua and type-checked, so their exports are typed
  for importers. Their compile-time checks are E8001–E8003, E8010, E8020,
  E8021, E8030, E8100–E8104, E8112, E8117, W8011, W8040, W8101 and W8102.
  Tests run every addon in dev, release and fully minified builds against a
  mock client; the in-game
  suite (`tests/ingame`, `/stdtest`) passed on the WoW: Forever client,
  including combat lockdown and a `/reload` with real SavedVariables, and
  runs against the mock in CI. `examples/core-demo` uses every core module.
- Completed 2026-10-14: handlers are typed by their event or message
  (`on`, `once`, `onUnit`, `send`, `@event`, `@unitEvent`), hook targets are
  checked (W8040, and W8102 for globals of Blizzard's load-on-demand addons,
  marked `@onDemand` in the `wow-api` declarations), events missing from a
  shared-mode flavor warn (W8101; the declarations now list each flavor's
  own events, from client dumps), and `Secret<…>` in `saved.bind` defaults
  is E8104.
- S2 done 2026-10-14: `locale` done 2026-10-14 (JSON locales checked against the
  base locale and compiled into guarded Lua files that load first, with
  `[AllowLoadTextLocale]` tags; `L` typed by the base locale); `pool` done
  2026-10-14 (frame and table pools; `FramePool.new` typed by the pooled
  class's constructor; protected frames released after combat);
  `settings` done 2026-10-14 (Settings API pages bound to `saved` stores,
  every control kind, keys and types checked E8113/E8070/E8107/E8114,
  profile refresh, combat-deferred open; `saved.bind` now returns its
  defaults' type).
- `launcher` done 2026-10-14 (Addon Compartment entry, minimap button through
  LibDBIcon or a built-in shape-aware draggable one, LDB launcher, position
  in LibDBIcon's shape, E8080). Its `theme` waits for the widget themes.
- `serialize` done 2026-10-14 (binary encoding with limits, DEFLATE in plain
  Lua checked against zlib both ways, printable and addon-channel encodings,
  checksummed export strings); `comm` done 2026-10-14 (chunked messages in
  wire format 1 or AceComm's, ChatThrottleLib or a built-in throttle,
  server-throttle backoff, messaging lockdown, objects, E8050).
- Dogfooding 2026-10-14: the test scripts and the mock client are compiled
  as wowlua (type errors fail the tests), and the runtime is written in
  wowlua and compiled into the embedded Lua by `cargo xtask runtime`. New:
  `wowlua compile` (files to Lua without an AddOn folder, with imports and
  bundling). Found on the way: `C_Timer.NewTimer`/`NewTicker` returned their
  callback type (now `TimerHandle`/`TickerHandle`), error handlers took no
  extra arguments, and the dev window created an unused frame.
- Typing everything (2026-10-14): the standard library, the runtime and the
  in-game suite are checked in strict mode, with `any` left only for values
  that are dynamic by nature. `callbacks.Registry<E>` is typed as specified
  (E8420; it returned `any`). The checker gained what this needed: field
  narrowing (locals and globals), discriminated unions on `x.kind ==`,
  `assert` results, narrowing in closures for locals never reassigned,
  interface methods written as function fields or `function t.m(_, …)`,
  method calls on narrowed optional fields, contextual typing of generic call
  arguments, `#` on possibly-nil values (E2005), and a `setfenv` that only
  disables global checks when it targets the file's own code. Stdlib option
  types use `x?: T` for omittable fields. Then: Blizzard's Settings API,
  the chat frames, `WorldFrame` and LibStub's LibDataBroker/LibDBIcon are
  declared; the stdlib's stateful objects are classes (the minimap button a
  widget class); intrinsic-only declarations no longer hide the modules'
  types from callers (`events.on` returned `any`), and the API generator no
  longer aliases the compiler's types to `any` (`TimerHandle` was `any`).
- Idiomatic wowlua (2026-10-14): the stdlib, the runtime, the mock client and
  the in-game suite use enums (`comm.Distribution`, `log.Level`, members or
  their strings accepted), classes and static fields where state lives,
  `const`, `?.`, `??`, compound assignment and `continue`. The checker gained
  short-circuiting optional chains and `??` as an existence probe; inlined
  constant strings escape bytes outside UTF-8. The conformance corpus runs in
  parallel within a memory budget (nightly 40 → 24 minutes).
- `bindings` done 2026-10-14: `@binding` and `header` collected at compile
  time (constants, E8350–E8352), one generated `Bindings.xml` for every
  flavor calling the addon's dispatch table, `BINDING_*` labels at load time,
  `settings.keybinding` checked (E8353, also against a hand-written
  `Bindings.xml`), E4334, and `default` keys behind the `binding-defaults`
  client feature (W8354; decisions S19–S21). The mock client loads the
  generated file; on the WoW: Forever client the generated bindings are listed
  with their labels and shown in the settings page's keybinding row; a guided
  step presses a key bound to one (`RunBinding` is Blizzard-only there);
  all 66 in-game tests passed.
- `tooltip` done 2026-10-14: `@onTooltip` and `hook` with a `TooltipHook`
  class (enable/disable), `TooltipDataProcessor` post-calls where the client
  has the tooltip data system, `OnTooltipSet*` script hooks on the standard
  tooltips (and `attach`ed ones) elsewhere, normalized `TooltipData`, kinds
  checked per flavor from the API declarations (E8330, E8300; decision S22).
  The mock client shows tooltips both ways (`Mock.showTooltip`); the in-game
  suite fills `GameTooltip` and waits for a real mouseover. The checker now
  treats optional links as existence tests for load order. All 69 in-game
  tests passed.
- `frames` done 2026-10-14: `frames.movable` with drag (button, modifier),
  a resize grip or your own, Ctrl + wheel scale, clamping, lock, reset,
  restore and save; positions anchored at the frame's screen third
  (decisions S23–S25); restored once the store loads and on profile changes;
  protected frames wait for the end of combat; the store field checked
  (E8340). The mock client has frame geometry (`Mock.drag`, `Mock.sizeBy`,
  `Mock.wheel`). Found on the way: an imported type alias used in an
  interface or alias of another module resolved to `any` when that module's
  declarations came later; aliases now resolve where they are first used.
  **S2 is complete.**
- S3 in progress: `color` done 2026-10-14 (hex colors folded at compile
  time, E8390; class colors honoring `CUSTOM_CLASS_COLORS`; escapes; a
  secret string stays a string through `wrap`); `secret` done 2026-10-14
  (decision S26, from SuuUI's probes of the WoW: Forever client); the
  generated API declarations now let documented secret-accepting arguments
  (`SecretArguments`) take `Secret<T>`, widget setters included. `unit` done
  2026-10-14 (records with secrets passed through, WoW: Forever's
  first-and-surname names, the client's health percentage, group tokens;
  the mock client models units); `aura` done 2026-10-14 (guarded reads
  with `readable`, the engine's aura containers with filter groups, spell
  and dispel-type lists and the client's buff look, filter words per
  flavor E8310, containers E8300; decisions S27, S28; the mock client has
  auras and containers); `spell` done 2026-10-14 (cooldowns with secret
  times but a readable `onCooldown`, duration objects, the legacy cooldown
  API as fallback); `item` done 2026-10-14 (shared requests, timeouts,
  cancelling, an LRU cache, spell data; `await` comes with `jobs`; the
  record is `ItemData`, as the API's `ItemInfo` names the item reference).
  The checker now narrows an optional chain's base where the chain has a
  value (`if x?.f ~= nil then x.f … end`). `media` done 2026-10-14 (own
  registry, LibSharedMedia when any addon loads it, waiting registrations
  handed over, E8400; LibSharedMedia is declared for `LibStub`). The
  checker types table literals assigned to `t[i]` by the element type. `jobs` done 2026-10-14 (coroutines within a per-frame
  budget, weighted priorities, sleep, waiting for events, progress, combat
  pauses, W8430), with `item.await`; the mock client drives frames
  (`Mock.advanceFrames`, `Mock.spend`). `dialog` done 2026-10-15 (confirm,
  prompt, export, import on the addon's own frames in the client's popup
  look, Escape, one at a time; decision S29), with `WidgetTheme` and the
  theme-aware builders in `wowlua:frames`. Decision S30: `menu` and `list`
  wrap `MenuUtil` and ScrollBox, which every flavor has. `menu` done
  2026-10-15 (a chainable `MenuBuilder` over MenuUtil: titles, buttons,
  checkboxes, radios, submenus, dividers, disabled entries). `list` done
  2026-10-15 (ScrollBox holders showing pooled rows of a widget class,
  selection, refresh, scrolling; the mock client has a ScrollBox). Widget
  themes done 2026-10-15 for dialogs and the launcher's built-in minimap
  button (decision S31). **S3 is complete**; its in-game run is pending
  (the WoW: Forever beta was down), StdTest has suites for every module.
- Exit criterion 2026-10-15: `examples/core-demo` uses events, saved
  variables, a settings page, a minimap button, keybindings and addon
  messages from the standard library alone; a test runs it in the mock
  client (login, settings page, minimap button and compartment entry, a
  key press, a ping answered over addon messages).
- S4 complete 2026-10-15 (patterns found across the corpus): `table` and
  `format` (decision S32), `version` (S33), `alert`, `links`, `unit.roster`,
  `events.bucket` and `launcher.dataSource` (S34), `secure` (S35), and Edit
  Mode on `frames.movable` (S36). Not yet run in game (the WoW: Forever beta
  was down); StdTest has suites and guided steps for all of them.

## M9: Test runner, language server, formatter

The test framework runs on wowlua's own emulation of the WoW client
([WoW client emulation](toolchain/emulator.md), decisions T183 to T189).

- S1 **VM core**: a Lua 5.1 VM in Rust (`wowlua-vm`) running the compiler's
  output, with the exact standard library; verified by the Lua 5.1 test suite
  and differential runs against PUC Lua over the corpus.
  Done 2026-10-15: the corpus compiles to `luac`'s bytecode and lines and runs
  the same, and the test suite passes with the client's exclusions (emulator
  §17.8). Locations from source maps come with S4.
- S2 **WoW semantics**: WoW's Lua (no `os`/`io`/`debug`, its arithmetic, its
  library additions), taint and secure execution with taint reports, secret
  values as a VM value kind.
  Done 2026-10-16: the client's probe cases (819, WoW Forever 1.60.1) give the
  client's results but those waiting for the engine and Blizzard's Lua; its
  limits, crashes and call depths; elune's taint cases; taint reports with the
  full chain (the protected actions that raise them come with S3).
- S3 **Engine**: the WoW API generated per flavor from Blizzard's
  documentation (existence, protection, secrets), the widget system and the
  behaviour tests need, "not mocked" errors for the rest; Blizzard's FrameXML
  as secure code, opt-in.
  Built 2026-10-16; done 2026-10-17: Blizzard's UI on camelot (70425) loads,
  logs in and runs without an error, as the client's does, and all 15,933
  values of the `/stdtest engine` battery match. Built 2026-10-16: the engine
  crate with the documentation's API and argument checks (556 of 563
  recorded checks), camelot's undocumented C functions, frames, events,
  timers, units, auras, restrictions, protected actions, spells and items
  from the tests; the M8 mock's stdlib tests run on its frames, events and
  timers; Blizzard's UI code loads (191 addons on camelot), with which all
  819 recorded client cases and all eight recorded taint experiments give
  the client's results.
- S4 **Test framework**: `wowlua test`, `wowlua:test`, `wowlua:mock` (spies,
  stubs, fakes, scenario controls, taint assertions), per flavor, in parallel.
  World profiles (decision T213): the character (name, class, race, level,
  faction, stats), gear, bags, money, position, auras and the units around
  the player with their distances, CVars, bindings and saved variables, loaded
  before the addon; recorded from the client with `/stdtest snapshot`; every
  part drivable during a test, each change firing the client's events in its
  order. GUI tests act as the user (click, hover, drag, type, key bindings)
  and assert what is shown. Writing tests (Tooling §16.1.3): typed matchers,
  `@cases`, waiting in straight-line code, widget matchers, snapshots,
  compile-time tests, custom matchers, `import internal` and `mock.module`
  (T214 to T220, T223, T224); `@bench` (T221) and doc-comment examples
  (T222) are specified here and built in M11 and S8. The groundwork for M12
  to M14: the VM's debug hooks (shared with S5's coverage) and the engine's
  file access behind an interface (T212).
  Built 2026-10-17, waiting for review and an in-game run (`/stdtest
  snapshot`, event-order recordings): `wowlua test` runs a package's test
  files per flavor in the engine (the addon loaded from its TOC as the
  client loads it; `test.blizzard-ui` loads Blizzard's UI first, fetched on
  first use), with `wowlua:test` (tests, hooks, `@skip`, `@only`,
  `@flavors`, `@cases`, `describe`, `config`, typed matchers, widget
  matchers, snapshots, custom matchers, `@expectError`) and `wowlua:mock`
  (the engine's controls, spies, stubs, fakes, waiting, printed output and
  errors, `mock.module`, user input, casting, modifier keys, taint
  assertions); world profiles with the character's money, gear, bags and
  place; `import internal`; `--watch`; the three examples have tests.
  Checked against the client's recordings (Forever 70425, 2026-10-17):
  input, focus and text scripts; casts, equipment, bags, zones, targets,
  combat, deaths and auras (their events, order and payloads); Blizzard's
  UI runs through all of them without an error. `mock.reloadUI` reruns
  the test in a fresh VM with the SavedVariables as the client writes them
  (`/stdtest sv`). Kills (`PARTY_KILL`, `UNIT_LOOT`), the server's late
  answers (`UNIT_TARGET`, an interrupt's), soft targets, vendors and
  secure CVars as recorded. Not yet: `--game-data` (M13); the standard and
  classic flavors' Blizzard UI code still errors in the engine (the focus
  is camelot's, which runs without an error).
- S5 **Coverage and reports**: lines, statements, branches, functions and
  conditions counted by the VM; terminal, JUnit, TAP, JSON, GitHub, lcov,
  Cobertura and an HTML report in the wowlua design system; thresholds.
  Built 2026-10-17: the VM counts instructions and conditional jumps of the
  addon's files (runtime checks and generated functions left out), every
  report above, `test.coverage` minimums and exclusions, `--coverage-per-flavor`;
  the HTML report names the design system's fonts (shipped with M11).
- S6 `wowlua fmt` complete for all wowlua constructs, with an idempotence suite.
  Built 2026-10-17: every construct (interpolated strings keep their text);
  the AST check covers types, decorators and declarations; the suite formats
  the repository's 148 `.wowlua` and `.d.wowlua` files twice.
- S7 Persistent incremental cache (phase 2), shared with the LSP.
  Done 2026-10-25 (T386): the language server's checker writes each
  package's module cache to `$WOWLUA_HOME/cache/check/` after a full check
  and a new checker reads it, so the first check after the editor starts
  takes the unchanged modules (SuuUI: about 280 ms instead of 400 ms; 175 ms
  warm). Batch commands keep checking every module.
- S8 `wowlua lsp` and the VS Code extension, with a test explorer, coverage in
  the editor and "run test" code lenses; code examples in doc comments run as
  tests (T222). Started 2026-10-24 (T368, T369): diagnostics as you type,
  formatting, the outline and go to definition, in the extension too; hover
  (T376) and completion (T377) on 2026-10-25, then signature help (T381), the
  test explorer (T382), the "run test" code lenses (T383) and coverage in the
  editor (T384), and doc comments' examples run as tests (T222); the HTML
  coverage report in an editor tab (T385).

**Exit:** the full edit, test, debug loop works without leaving the editor, and
the in-game bugs found so far fail as tests the same way they failed in game
(`os.date` in `wowlua:format`, `0 / 0` in `wowlua:serialize`, SuuUI's
reparented status-bar containers breaking Edit Mode, taint in combat). StdTest
stays the in-game check until then (decision T189).

**Status: done (2026-10-25).** S1 to S8 as above; the four in-game bugs fail
as tests as they failed in game (`crates/wowlua-engine/data/coverage-regressions.txt`).
Camelot is the flavor the emulated client targets: the standard and classic
flavors' Blizzard UI code still errors in the engine, for a later milestone.
The in-game runs of S3 and S4 wait for the next beta (`tests/ingame/PROBES.md`).
Released as 0.10.0, together with M12.

## M10: Registry

- Registry server (in the workspace: `crates/wowlua-registry`, decision
  T104), `publish --target registry`, `package --registry`, registry
  sources in the resolver, owners and yanking.

**Exit:** libraries can be published and consumed by version requirement.

**Status: done (2026-10-26)**, released as 0.13.0. The registry runs at
`https://registry.wowlua.dev`, a container on the project's droplet behind
its Traefik, with its data in one volume; accounts are GitHub sign-ins.
Checked natively (`crates/wowlua-registry/tests/server.rs`, and
`crates/wowlua-cli/tests/registry.rs`, the `wowlua` binary against the
server in the test's process): publish, resolve, lock, download, yank and
W4322, owners and 403, 409 for an existing version, reserved names, 413 and
E4230. Not yet: the `wow-api-<flavor>` types packages on the registry (they
stay embedded, T83), search (`package.keywords`), and registry dependencies
in the browser's playground (M14), which has no downloads.

## M11: Performance optimization

WoW loads Lua source only, and its Lua 5.1 compiler barely optimizes, so the
compiler shapes the source the client compiles. Each optimization applies
only where the type checker proves its preconditions (plain tables without
metatables, numbers, no writes in between), so plain Lua and `any` values stay
as they are, and secure-code paths keep their call structure (taint).

- `wowlua bench`: benchmarks run on the embedded Lua 5.1 (WoW runs plain Lua
  5.1, not LuaJIT), comparing the optimized and the unoptimized build; every
  optimization is measured before it is kept. Benchmarks are `@bench`
  functions in test files (T221).
- Loop-invariant field reads hoisted out of loops (`self.frame.texture` read
  once).
- Repeated field reads in one stretch of code read once into a local.
- `ipairs` loops over plain arrays become numeric loops.
- `table.insert(t, v)` on plain tables becomes `t[#t + 1] = v`.
- Upvalues read often in hot functions copied into locals when not written
  meanwhile.
- Closures that capture nothing from a loop created once, outside it.
- Inlining of statement bodies and private methods (decision T166 left them
  out of M7).
- Constant propagation of locals assigned once with a constant.

In-game performance tools, to measure what the optimizations change
([Debugging §7.7](language/07-debugging.md#77-dev-console), decision T181):

- The dev window in the wowlua design system with its fonts (T171, T473).
- Recordings: live, per combat, per boss pull and manual; comparison per
  minute (T172, T173).
- Frame time, slow calls, call paths, event attribution and garbage per
  function, with the profile settings `profile-call-paths`,
  `profile-garbage`, `frame-budget` and `slow-call` (T174).
- The performance views (functions, tree, flame, by event, timeline, slow,
  garbage, bench) and `/wl record`, `/wl bench`, `/wl hints`, `/wl hud`.
- Performance hints with `P` codes and their `wowlua explain` texts; the
  code generator learns the `P` prefix (T176).
- The compact HUD (T177) and the error toast (T180).
- Export through SavedVariables and `wowlua prof report` (T175, T179).

**Status: in progress**, built in 0.13.0. The optimizations and `wowlua
bench` were built 2026-10-26: on `tests/bench` the release build runs its
eleven benchmarks 1.04x to 2.37x as fast as the unoptimized one, 1.49x in
the geometric mean (embedded Lua 5.1, 2026-10-27).

The in-game tools above were built 2026-10-26, ahead of the optimizations:
the dev window in the design system with its fonts, recordings, frame time,
slow calls, call paths, roots (slash commands and the standard library's
dispatch frames monitored too), garbage per function, the eight views,
the hints and their explanations, bench, the HUD, the error toast and the
export through SavedVariables, tested in the emulated client. `wowlua prof
report` was built the same day: it finds the exports in every client and
account of the installation, maps the slow calls' stacks to the source and
writes the HTML report with every view, tested on an export the emulated
client made. Not yet: the in-game run of a boss pull (the exit below).

**Exit:** the benchmark suite shows a measured speed-up of the optimized
release build over the unoptimized one, the corpus and differential tests show
unchanged behavior, and an optimized addon is checked in game; a recording of
a boss pull, taken in game with the dev window, is exported and opened with
`wowlua prof report`.

## M12: Debugger

A debugger attaches to the emulated client (decision T209): the VM pauses and
steps where the real client cannot be observed
([Debugger](toolchain/debugger.md), decisions T300 to T315). Built as
libraries for wowlua studio, natively and in the browser (T300, T301).

- The VM's debug hooks: line, call, return, error, load and the engine's
  dispatches, at no cost when off (T302, T303).
- `wowlua-debug`: sessions of a test file or the addon, breakpoints (lines,
  functions, API calls, events, scripts, timers, exceptions, taint),
  stepping through Lua and into event dispatch, `OnUpdate` and timers,
  stacks with dispatch frames, variables with taint and secrets, evaluation
  and a console (T304 to T306).
- Profiling from the VM's hooks: calls, instructions, garbage and host time
  per function, event, script, `OnUpdate` and game frame; call trees and
  speedscope files; `wowlua test --profile-out <dir>` (T307, T308).
- The scene: the engine's UI as a draw list and a frame tree (T309).
- From M13: the CASC reader for the user's textures (BLP), atlases and
  fonts, and the `wgpu` renderer, to windows and off-screen PNGs (T310,
  T311).
- Front-ends: `wowlua debug` (DAP), `wowlua mcp` (MCP for AI agents),
  `wowlua screenshot`, `mock.screenshot` and `toMatchScreenshot` (T312 to
  T315).
- From M13: 3D models in model frames and model scenes, from the user's
  installation (encrypted tables read with the client's own keys): creature
  displays, creatures and files; characters with their customizations
  and worn items; Blizzard's model scenes from the client's tables (T317
  to T321); particles, ribbons and billboarded bones.
- The minimap's terrain from the client's map tiles, at its zoom, turning
  with the player, with group members' blips and pings (T324, T329).
- With an installation at hand, items the tests do not define are the
  client's (`Item`, `ItemSparse`, T327).

**Exit:** a test or an addon session is stopped at a breakpoint inside an
event handler, stepped through and inspected with watches over DAP; an agent
clicks through the addon's UI over MCP and sees it in screenshots drawn with
the client's textures and fonts; a UI test compares a screenshot.

**Status: done (2026-10-25)**, released as 0.10.0 together with M9.
Built 2026-10-23: every part above, with tests (the VM's
hooks and profiler, the controller on the engine, a DAP client stopping a
test in an `OnClick` handler, an MCP agent clicking an addon by pixel into a
breakpoint, screenshot tests written, compared and updated, the renderer's
pixels, the CASC reader on all five products of this machine's
installation). Checked by hand on the widget demo with camelot's Blizzard
UI: its window drawn with the client's textures and Friz Quadrata, clicked
and stepped through over MCP; every window and tab of camelot's default UI
and SuuUI screenshotted. The hooks' cost when off, measured on a release
build (a mixed Lua workload, best of seven runs): about 1 to 2 percent
over the VM before them (37.0 ms to 37.4–38.3 ms). The VS Code
extension's debugger part (`editors/vscode`, T331); its grammar, language
server and test explorer came with M9 S8. Not yet: the indoor minimap.

## M13: Game data and rendering

Real game data and assets from the user's own client, never distributed
(decisions T203, T210).

- A CASC reader for the local installation: the DB2 tables (spells, talents,
  items, currencies, maps, …) behind the engine's game-data interface, next
  to the tests' data and the world profiles' (T203, T213).
- Full API types: the type generator types every function, event payload
  and table the game API has. Most are typed now (decisions T288 to T296:
  warcraft.wiki.gg, Blizzard's own Lua, the client's recorded answers);
  about 55 functions (2026-11-01) are still `(...unknown) -> ...unknown`,
  with no source that tells their types: mostly the classic clients' old
  functions and the character select screen's; a read-only probe of the
  clients records what the getters among them answer
  (`tests/ingame/PROBES.md`, "The untyped functions").
- UI assets and the `wgpu` renderer moved to M12 (T310); left here: text
  measured with the client's fonts, so `GetStringWidth`, wrapping and
  truncation match the client in tests too (T211). 3D models moved to M12
  (T318).

**Exit:** the debugger's viewer shows the default UI and an addon's frames
from a world profile as the client draws them, with the client's spell and
item data.

Started 2026-10-25: the reader and items came with M12 (T310, T327); the
spells from the client's tables and hotfix cache (T391); text the
recordings cannot measure is measured with the client's font files
(T392). Exit checked 2026-10-25: a screenshot of the widget demo with the
camelot world profile and the installation shows the default UI, the
demo's window, the spellbook's pages (once `Raise` moved panes with their
window, T343) and a spell's tooltip with the client's current cost, cast
time, cooldown and description (T391, wrapped: T393).

**Status: done (2026-10-25)**, released as 0.11.0. Follow-ups for the next beta, not blocking:
the untyped functions' probe, how the client answers for currencies,
factions and mounts a character lacks, the tooltip's wrap width and
descriptions with spell power (`tests/ingame/PROBES.md`).

## M14: Browser

The compiler, the VM and the engine run in the browser as WebAssembly
(decision T212).

- `wasm32-unknown-unknown` builds of the compiler pipeline, the VM, the engine
  and the renderer (`wgpu` on WebGPU or WebGL); file access through the
  engine's file system interface; checked by CI from M9 S4 on.
- A web playground: write wowlua, compile it, run the addon against the
  emulated UI with a world profile, with the debugger's views in the page.
- Shareable reproductions: code and profile in a link.
- Game data from the user's own installation through the File System Access
  API, read in the browser and never uploaded (T210).

**Exit:** an addon from the corpus compiles, runs and is debugged in the
browser against the default UI, from a link.

The playground's page and editor are the website's (wowlua.dev/playground,
with its own design); wowlua provides what the page embeds:

- S1 The compiler in the browser: `wowlua-driver` (loading, checking,
  building and formatting a package) builds for `wasm32-unknown-unknown`
  without its `native` feature, reading and writing through a host the
  program installs (`wowlua_driver::host`; `MemoryHost` keeps the files in
  memory), Lua's limits checked by wowlua's own Lua 5.1 instead of the C
  Lua; CI checks the build (T394).
  Done 2026-10-25: a package in memory loads, checks, builds and formats
  as on disk (`tests/memory.rs`); wowlua's Lua gives the C Lua's limit
  errors message for message (three differences fixed in the VM).
- S2 `@wowlua/web`: an npm package (wasm-bindgen) with a JavaScript API to
  check, build and format a package held in memory, the editor's services
  (diagnostics, hover, completion, go to definition, formatting) and share
  links (the files and the world profile in a URL). No editor or page of its
  own (T395).
  Built 2026-10-26: `crates/wowlua-web` (the playground's API, tested
  natively: `tests/playground.rs`), the language server embedded (the
  editor's services are `wowlua lsp`'s, over messages), `web/` (the
  JavaScript API, a Web Worker and its client) and `docs/reference/embedding.md`.
  Checked in Node (`web/test/smoke.mjs`, in CI) and in Chrome through the
  Web Worker (`web/demo/`): a check takes about 50 ms, the module 7.5 MB
  with gzip. Share links carry the files only (world profiles come with
  S3). The files the page writes are the package, as the disk is for the
  CLI: check, build, run and format read them (the editor's open
  documents do not reach them) and report what `wowlua check` and
  `wowlua build` report, with the manifest's `check.lints`; the language
  server reads the editor's open documents over them, as `wowlua lsp`
  reads unsaved texts over the disk, and checks again when the page
  writes, deletes or replaces files, as after files changed on disk
  (fixed in 0.12.1: the open documents' texts reached the package's
  calls, the lints were not applied, the server did not see the page's
  writes).
- S3 Run and draw: the built addon runs in the engine with a world profile
  and camelot's Blizzard UI (fetched by the page), drawn on a canvas by the
  renderer, with mouse and keyboard input, printed output and errors.
  Built 2026-10-26 (T396): the engine and the test runner read through
  `wowlua-host` too; the playground builds into memory and opens the
  test runner's addon session, after Blizzard's UI when the page fetched
  it (the files listed per flavor by `cargo xtask web-ui`, from jsDelivr);
  frames as the page's clock moves, the debugger's input, output to the
  page (through the worker with a canvas or without: its frames run
  either way, and `advance` moves the game at once; 0.12.1); the renderer draws into the canvas (WebGPU, else WebGL 2) and
  snapshots the screen. Checked natively (`crates/wowlua-web/tests/run.rs`,
  with the UI in CI's Blizzard UI job), in Node, and in Chrome on WebGPU
  and WebGL 2: camelot's default UI and an addon's frame drawn, a click
  on the canvas reaching the frame. Blizzard's textures and fonts wait for
  S5 (placeholders and stand-in fonts until then).
- S4 The debugger over the JavaScript API: breakpoints, stepping,
  variables, frames and events, as the M12 views have them.
  Built 2026-10-26 (T397): a run with `debug` attaches the M12 controller
  to the addon's VMs from their first line; its requests and events are
  the debugger protocol's, over a mailbox the worker waits on when
  stopped (a cross-origin isolated page); between frames the game serves
  them, and runs the input and events the debugger queued. Checked
  natively (`crates/wowlua-web/tests/debug.rs`: a breakpoint in a click
  handler, the stack, a value, a step), in Node through the page's link,
  and in Chrome: a click on the canvas stopped at the handler's line with
  its upvalue shown, continued; a pause between frames, an evaluation,
  stopping.
- S5 Game data and assets from the user's installation through the File
  System Access API, read in the browser and never uploaded (T210).
  Built 2026-10-26 (T398): the page hands the worker the picked folder (a
  directory handle, a folder input's files, or path and file pairs); the
  worker indexes what the CASC reader reads and reads it synchronously
  for it; the flavor's client then gives runs the renderer's textures,
  fonts and models and the engine's tables. Checked natively
  (`crates/wowlua-web/tests/installation.rs`: a spell's name from the
  client's tables, a texture and a font) and in Chrome: camelot's client
  opened from 501 files in 0.3 s, the default UI drawn with its art
  (minimap, portrait, frames, Friz Quadrata).
- S6 A bare test page that uses only the package, `docs/reference/embedding.md` for
  the website, and the package built and attached to each release.
  The test page (`web/demo/`) and the guide came with S2.
  Built 2026-10-26 (T399): the release workflow builds the package (the
  `web` profile, optimized for size), runs its Node test and attaches
  `wowlua-web-<version>.tgz` with its SHA-256; `xtask check` keeps
  `web/package.json` at the workspace's version. The tarball installs
  with npm and works from a fresh project.

Exit checked 2026-10-26 in Chrome: AdvancedInterfaceOptions from the
corpus, adopted with `wowlua init` (53 files) and opened from a share link
(142 KB), built for its seven flavors in the page (0.3 s), ran on camelot
after Blizzard's default UI; `/aio` typed into the chat stopped at a
breakpoint in its handler (`src/basicOptions.lua:126`, through Blizzard's
chat edit box), showed `msg`, stepped to the next line and ran on, and its
options opened in Blizzard's settings panel (once
`C_SettingsUtil.OpenSettingsPanel` fired its event, T400). Loading
Blizzard's UI waits for its on-demand addons' code by default (the
settings panel's is one).

World profiles everywhere (2026-10-26, T401; for the IDE, M15): checked
with the package (E4400 to E4402) and described by a generated schema
(`schema/world-profile-0.1.json`, applied by VS Code); a run starts in one
by name or from its text; a `world` request changes the running world
with the tests' controls (DAP `wowlua/world`, MCP `world`, the page's
`world`); share links carry how the package runs. Checked natively
(`crates/wowlua-web/tests/profiles.rs`, the debugger's DAP test), in Node
and in Chrome: a link opened in its profile, the world's money changed
from the page and seen by the addon.

**Status: done (2026-10-26)**, released as 0.12.0. Not yet: dependencies other than the
standard library in the page (M10's registry), Blizzard's UI for flavors
other than camelot (the engine runs camelot's), the timing of
`SETTINGS_PANEL_OPEN` (`tests/ingame/PROBES.md`); the IDE's needs are M15.

## M15: Studio

wowlua studio, wowlua's own IDE, in its own repository
([wowlua-studio](https://github.com/DomenikIrrgang/wowlua-studio), with its
roadmap and decisions in its `spec/`): a native Rust application for
Windows, macOS and Linux, drawn on `wgpu` with egui, with no HTML or
JavaScript (T480). It links the libraries M12 to M14 made in its own
process (T300): the driver, the language server (`wowlua_lsp::embedded`),
the test runner, the debugger, the emulated client and the renderer.
Desktop first; a browser build stays possible, as the libraries below it
build for the browser (T301). It edits real packages, with world profiles
as a first-class part, and pins a wowlua release.

What wowlua adds for it:

- What Studio's milestones need from the libraries as they come, called
  in process rather than over DAP or the language server's stdio.
- Blizzard's UI for every flavor (as the engine runs their UI code).
- For Studio's online workspace (a GitHub repository with no folder,
  after its first release): a host per workspace instead of one for the
  whole process (`wowlua_host`), and wowlua's commands callable in
  process.
- For the 3D world behind the UI (planned): unit positions in the engine
  and in world profiles, a world renderer with the installation's models.

**Exit:** SuuUI opened from disk in Studio on Windows, macOS and Linux:
edited and checked, its tests run, run against the default UI in a world
profile and debugged, the world changed while it runs, profiled,
installed into the WoW folder and committed.
