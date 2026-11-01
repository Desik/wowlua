# 10. Package Manager

The package manager is part of the `wowlua` binary. It resolves, fetches,
locks and embeds dependencies, and manages workspaces. Publishing is covered
in [Publishing](publishing.md), and the manifest format in [Manifest](manifest.md).

## 10.1 Packages

A **package** is a directory with a `wowlua.json`. Its `kind` determines how it
is built and consumed:

| Kind | Produces | Consumed as |
|------|----------|-------------|
| `addon` | An installable AddOn folder | Not a dependency. Other addons reference it at runtime through `addon-dependencies`. |
| `library` | wowlua modules | Compiled **into** each dependent addon (private copy, tree-shaken) |
| `lua-library` | Plain Lua/XML files (e.g. LibStub-based libraries) | Copied into each dependent addon under `<libs-dir>/<install-path>/` (default `libs/<name>/`), loaded in their declared order |
| `types` | `.d.wowlua` declarations only | Used by the type checker. Never emitted. |

Plain Lua libraries without a `wowlua.json` (most existing WoW libraries)
can be used directly: the dependency declaration supplies `kind:
"lua-library"` and, if needed, the file list (§10.5).

## 10.2 Dependency kinds

| Manifest section | Meaning |
|------------------|---------|
| `dependencies` | Libraries embedded into the build output |
| `dev-dependencies` | Only for checking, testing and tooling: type packages, mocks, test helpers. Never embedded. |
| `addon-dependencies` | Other AddOns that must or may be installed in the game. Written to the TOC. Never fetched or embedded. |

## 10.3 Sources

```json
"dependencies": {
  "libstub":      { "git": "https://github.com/example/LibStub", "tag": "1.0.3", "kind": "lua-library" },
  "ace3":         { "git": "https://github.com/WoWUIDev/Ace3", "rev": "8d2a1c0", "kind": "lua-library",
                    "files": ["AceAddon-3.0/AceAddon-3.0.xml", "AceEvent-3.0/AceEvent-3.0.xml"] },
  "shared-utils": { "path": "../shared-utils" },
  "libdeflate":   { "archive": "https://example.com/LibDeflate-1.0.2.zip",
                    "sha256": "9f2c…", "kind": "lua-library", "strip-prefix": "LibDeflate/" },
  "wowlua-ui":    "^0.4.0"
}
```

| Source | Fields | Notes |
|--------|--------|-------|
| Registry | `"<semver req>"` or `{ "version": "…", "registry": "…" }` | Default registry: `https://registry.wowlua.dev` ([§10.10](#1010-registry)), or `registries.default` in `config.json`. `registry` is a URL or a name under `registries` in `config.json` ([CLI §12.7](cli.md#127-configuration)). Locked to a version, `registry+<url>` in the lockfile. A package the registry does not have, or no version matching the requirement, is E4102. |
| Git | `git`, plus one of `tag`, `branch`, `rev`; optional `subdir`, `submodules` (default `false`) | Locked to a commit hash. Uses the system `git`, so credentials and SSH work as usual. Submodules are only fetched with `"submodules": true`; their commits are fixed by the locked parent commit. `"tag": "latest"` names the newest tag among the last 50 commits of the default branch, re-resolved by `wowlua update` (decision T149). |
| Path | `path` | Relative to the manifest. Never locked by checksum. For workspaces and local development. |
| Archive | `archive` (URL to `.zip` / `.tar.gz`; `https://`, or `file://` for a local archive, decision T108), `sha256` (required, of the archive file), optional `strip-prefix` | For release downloads. Without `strip-prefix`, an archive holding a single folder (a GitHub archive) has that folder as its root (decision T152). |
| SVN | `svn` (repository URL, may include a path such as `…/trunk/AceAddon-3.0`), optional `rev`, optional `submodules` (default `false`: SVN externals are not fetched) | Locked to a revision number: the explicit `rev`, otherwise the last revision that changed the URL (decision T105). Uses the system `svn` client. Many WoW libraries (Ace3, LibStub, CallbackHandler, LibSharedMedia on `repos.curseforge.com`) are only published this way (decision T51). |

Common fields:

| Field | Meaning |
|-------|---------|
| `kind` | Overrides or supplies the package kind (required for sources without a `wowlua.json`) |
| `optional` | Only included when a feature enables it ([Manifest §features](manifest.md#96-features)) |
| `flavors` | Only included for these flavors |
| `bundle` | `false` keeps the library as separate files even in bundled builds |
| `files` | `lua-library` only: explicit ordered list of `.lua` / `.xml` / `.toc` entry files |
| `load` | `false`: a `lua-library` shipped without entry files, not loaded (a converted project's external nothing loads, decision T156) |
| `moved-up` | The trailing folders of `subdir` / `strip-prefix` a `.pkgmeta` `move-folders` moved up: in a converted project the other files of the folder above ship next to them, as the packager leaves them (decision T152) |
| `types` | A `types` package or a path to `.d.wowlua` files describing this library |
| `install-path` | Folder under `build.libs-dir` (default: the dependency key) |
| `features`, `default-features` | Features of the dependency to enable ([Manifest §9.5](manifest.md#95-dependencies)) |
| `curseforge-slug` | Slug used for CurseForge relations |

## 10.4 Versions and resolution

- Versions follow **SemVer 2.0**. Requirement syntax is Cargo's: `^1.2` (the
  default for bare versions), `~1.2.3`, `=1.2.3`, `>=1.0, <2.0`, `*`.
- **Resolution covers the union.** The resolver resolves all members of a
  workspace together, over the union of all features, all flavors and all
  `dev-dependencies`. Enabling features, building another flavor, or running
  tests therefore never changes the lockfile.
- **One version per package per addon.** Every dependency is embedded into a
  single AddOn, so each addon, across all of its flavors, uses exactly one
  version of each package. Across a workspace, the resolver prefers one version
  for everyone. Addons with incompatible requirements may end up on different
  versions; lock entries record which addons use them (`used-by`). Conflicting
  requirements within one addon are error E4101, explained by the resolver's
  derivation tree. `dev-dependencies` are resolved but never embedded.
- **Identity.** A package is identified by its name **and** source. The same
  name coming from two different sources in one addon's graph (e.g. git and
  registry) is error E4321.
- The resolver uses the **PubGrub** algorithm (`pubgrub` crate) and prefers the
  highest compatible version, or the locked version if one exists and still
  satisfies all requirements. Yanked versions are never chosen by a new
  resolution. A locked yanked version keeps working with warning W4322.
  `wowlua update` lets locked versions move; `--precise <version>` pins a
  registry dependency to one version.
- **Registry indexes** are cached (§10.7). A resolution reads the
  registry's index of a package once per command with `If-None-Match`,
  unless the cached index has the root's locked version and it still
  matches: then the cache serves, so a build with an up-to-date lockfile
  needs no network. `fetch`, `update`, `add` and `outdated` always read the
  indexes again (and so report W4322 for a version yanked since). When the
  registry cannot be reached, a cached index serves.
- Git and archive dependencies without a `wowlua.json` have version
  `0.0.0+<commit or sha>`. With a manifest, they have its version.

## 10.5 Embedding

### 10.5.1 wowlua libraries

Library modules are compiled together with the addon:

- Module IDs are prefixed with the package name (`wowlua-ui/widgets/button`).
- Only modules reachable from the addon's imports are included (tree-shaking at
  module granularity, plus dead-code removal inside modules when enabled).
- Every addon gets its **own private copy** in its addon table. Two addons
  using different versions of the same library never interfere.
- The library is type-checked against its own manifest's check settings. The
  consumer sees its exported types.
- **`wowlua:meta` inside a library.** `addon`, `build` and `flavor` describe the
  consuming addon's build. `features` and `config` are the library's own: its
  features are enabled through the consumer's dependency entry (`features`,
  `default-features`), and its `config` defaults (the library manifest's
  top-level `config`) can be overridden by the dependency entry's `config`
  object.
- **Flavors.** A library supports all flavors unless its manifest lists
  `flavors`. Depending on a library for a flavor it does not support is error
  E4327, unless the dependency entry restricts itself with `flavors`.
- **Output.** Modules are emitted to `<libs-dir>/<install-path>/` (default
  `libs/<name>/`), mirroring the library's `build.src`, and load in the order
  the load-order analysis computes for them, like the package's own modules.
- **Dependency code** (decision T111). Warnings and lints in library code are
  not reported to the consumer (errors are), and library code is never
  instrumented, monitored or runtime-checked by the consumer's profile.
- **Runtime meta** (decision T110). At run time, `wowlua:meta`'s `config` and
  `features` tables are the consuming addon's. A library may therefore only
  use values that fold to constants (scalars); reading a table-valued path at
  run time is an error.
- **Checking a library** (decision T112). `wowlua check` in a library package
  checks it on its own, for all its flavors, and `wowlua test` runs its tests
  against it built as an addon. `wowlua build` refuses: a library is only
  built into the addons that depend on it.

**`types` packages** contribute their declarations (`types.declarations`,
default every `.d.wowlua`) to the checker, from `dependencies` and
`dev-dependencies`. A dependency entry's `types` may also name a path (a
`.d.wowlua` file or a folder of them) relative to the manifest.

### 10.5.2 Plain Lua libraries

`lua-library` dependencies are placed in `<libs-dir>/<install-path>/`
(default `libs/<dependency key>/`), keeping their relative layout. The files to
load, in order, are determined by the first of these that applies:

1. the `files` field of the dependency declaration (entries may be `.lua`,
   `.xml`, or a `.toc` whose file list is used);
2. the `lua-library.files` field of the library's own `wowlua.json`, if
   present;
3. a single `.toc` file in the library root (its file list, ignoring
   `## Interface` and metadata). Flavor-suffixed TOCs are matched to the build
   flavor;
4. a single `lib.xml`, `embeds.xml` or `<name>.xml` in the library root.

Otherwise the dependency is error E4110 and asks for `files`. Only the entry
files appear in the TOC. Entry paths are relative to the library root and
resolve case-insensitively, like the client.

**Load position** (decision T106). Entry files load before the package's own
files: dependencies of a library before the library, otherwise in the
manifest's declaration order. `build.load-order` may name an entry file by its
output path (`libs/LibStub/LibStub.lua`) to place it among the package's files;
`wowlua init --from-pkgmeta` writes such entries, so migrated addons keep
their exact order.

**The package's own XML** (decision T107). A reference in one of the package's
XML files that resolves into `<libs-dir>/<install-path>/` (relative to the
source folder, as an `embeds.xml` written for the packager does) names that
library's file. It is rewritten to the absolute in-game path
(`Interface\AddOns\<Name>\Libs\LibStub\LibStub.lua`), the file is copied with
everything it reaches, and it is not listed in the TOC again.

**Copied files.** Only what the addon can load is copied: the entry files,
every file reachable from them through XML `<Script>` / `<Include>` chains
(which must resolve), and the library's license files (`LICENSE*`, `COPYING*`,
or `lua-library.license-files`). Tests, docs and other files of the library
repository are not copied. A converted packager project
(`compat.packager-keywords`) ships every file of the library instead, like the
packager ships externals: all but dotfiles, its repository's `.pkgmeta`
`ignore` and the project's `build.exclude` (decision T144).

These libraries keep their own runtime versioning (LibStub). wowlua does not
rename or rewrite them, except when the minification level applies to bundled
libraries (`minify-libraries`, default `false`). License header comments are
always preserved ([Code generation §14.8.6](codegen.md#1486-licenses)).

### 10.5.3 Nested dependencies

Dependencies of dependencies are embedded as well, deduplicated per addon. A
`lua-library` that itself embeds a copy of a library (e.g. a bundled `LibStub`)
is left untouched. LibStub resolves duplicates at runtime.

## 10.6 Lockfile

`wowlua.lock` is generated JSON. It is committed for addons and ignored for
libraries.

```json
{
  "version": 1,
  "flavor-table": {
    "version": "2026.10.29",
    "interfaces": { "standard": [120100], "tbc": [20506], "vanilla": [11509] },
    "aliases": { "anniversary": "tbc" }
  },
  "packages": [
    {
      "name": "ace3",
      "version": "0.0.0+8d2a1c0e9f…",
      "source": "git+https://github.com/WoWUIDev/Ace3?rev=8d2a1c0#8d2a1c0e9f…",
      "checksum": "sha256-3b4f…",
      "dependencies": [],
      "used-by": ["my-addon"]
    },
    {
      "name": "wow-api-standard",
      "version": "12.1.0+gen.3",
      "source": "registry+https://registry.wowlua.dev",
      "checksum": "sha256-77e0…",
      "dependencies": [],
      "used-by": ["my-addon"]
    }
  ]
}
```

- **Flavor table.** The lockfile pins the flavor-table version and the
  resolved interface numbers of every targeted flavor, so every machine
  generates identical TOCs. `aliases` records the ruleset each client alias
  the manifest uses (`anniversary`, `forever`) named when the
  lockfile was made; it is left out when the manifest uses none. `wowlua
  types sync` updates them deliberately (W4313 for an alias that now names
  another ruleset, [Manifest §9.4.1](manifest.md#941-flavor-table)); the
  change shows up in the lockfile diff. `--flavor` limits which flavors'
  interface numbers move. Until the registry provides the `wow-api-<flavor>`
  packages (§10.9), their declarations ship with the compiler (decision T83),
  so `types sync` locks none.
- Checksums cover the normalized file tree of the package (sorted paths,
  contents, executable bit ignored) and are verified on every fetch (E4103).
- `--locked` fails if the lockfile would change (E4104). `--offline` uses only
  the cache: resolution may still run and update the lockfile, but nothing is
  downloaded (E4105 if something is missing). `--frozen` means `--locked` plus
  `--offline`. `--locked` and `--frozen` are meant for CI.
- Packages are sorted by name, and keys are written in a fixed order, so diffs
  stay minimal.
- Lockfile versions: see [Manifest §9.16](manifest.md#916-schema-versions-and-migration).

## 10.7 Cache

| Path | Content |
|------|---------|
| `$WOWLUA_HOME` (default `~/.wowlua`) | Root |
| `…/cache/git/<name>-<hash>/` | Bare clones |
| `…/cache/checkouts/<kind>-<name>-<hash>-<pin>/` | Exported trees: one per git commit, SVN revision; without version-control metadata |
| `…/cache/archives/<sha256>`, `…/cache/archives/<sha256>.d/` | Downloaded archives and their extracted trees |
| `…/cache/registry/<registry>/<name>/<version>/` | Unpacked registry packages; beside them `<name>/.index` (the index file) and `.index.etag`, and `<registry>/config.json`. `<registry>` is the registry's host (`registry.wowlua.dev`; a port joins it with `_`, a path adds a hash) |
| `…/cache/compile/<hash>` | Lua 5.1 compile results (see below) |
| `…/maps/<build id>/` | Archived source maps of published builds ([Code generation §14.6](codegen.md#146-source-maps)) |
| `…/config.json` | Global configuration: WoW install paths, registries (see [CLI](cli.md#127-configuration)). Tokens are not stored here unless the OS keychain is unavailable (§10.11). |

`wowlua cache clean` removes entries that no fetch or build has used for 30
days; every use stamps the entry (`<entry>.used`). `--all` removes everything
(decision T109). All cache writes are atomic
(write to a temp location, then rename), so concurrent builds are safe.

**Compile results.** Builds compile Lua with the embedded Lua 5.1 to report
its limits (E5001) and to choose between the optimizer's variants. The
result depends only on the bytes, the chunk name and the Lua build, so the
CLI keeps it in `…/cache/compile/<hash>` (a blake3 hash of the three): a
later build of the same bytes takes it from there, and only that build's
new or changed files are compiled. An entry is created once and records
its length; one cut short is a miss and is written again, so concurrent
builds are safe. An entry's modification time is its last use: when the
cache grows past `build.compile-cache-size` (MiB, default 256), the least
recently used entries are removed, at most once a day, until it is back
under three quarters of the limit. `cache clean` removes entries unused for
30 days, `--all` all of them. `WOWLUA_NO_COMPILE_CACHE=1` or
`build.compile-cache: false` turns the cache off ([CLI
§12.7](cli.md#127-configuration); decision T208). Debug builds of wowlua compile anyway and
check that the two results agree.

**Checked modules.** The language server keeps each package's checked
modules in `…/cache/check/<hash>` (one file per package; [compiler
§13.9](compiler.md#139-caching-and-incremental-builds), decision T386).
A file's modification time is its last use; files unused for 30 days are
removed by the language server and by `cache clean`, `--all` removes all
of them. `WOWLUA_NO_CHECK_CACHE=1` turns it off.

## 10.8 Workspaces

A workspace groups several packages, typically an addon suite such as a core
addon plus load-on-demand modules, together with shared libraries.

```json
{
  "workspace": {
    "members": ["addons/*", "libs/*"],
    "default-members": ["addons/core"]
  }
}
```

- One `wowlua.lock` and one `dist/` at the workspace root.
- **Members** are the folders with a `wowlua.json` that `members` matches
  (and `exclude` does not), plus the root itself when its manifest has a
  `package` section.
- **Selection** (decision T113). `-p <package>` selects one member. Otherwise
  a command run inside a member's folder works on that member, and one run at
  the root works on `default-members` (all members if it is empty). `build`,
  `check`, `install`, `uninstall`, `clean`, `fmt`, `fetch`, `update`, `tree`
  and `outdated` work on every selected member; `build` and `install` skip
  library members, which `check` checks. Commands on one package (`add`,
  `remove`, `toc`, `lint`, `fix`, `dev`, `trace`) need exactly one selected
  member.
- An addon may list another workspace addon in `addon-dependencies`. The
  compiler then checks it together with that addon's sources, **types only**
  (decision T287): the files that make `@global` declarations and what they
  import (its modules, its wowlua libraries, the standard-library modules)
  are analyzed and type-checked with the addon's own, so its `@global` API
  has the types the checker infers for it, as inside that addon
  ([Language §4.3](../language/04-decorators.md), decision L83). They are
  never compiled into the addon, loaded with it or reported: the other addon
  reports its own problems. Only its top level is read, not its own addon
  dependencies, so two addons that list each other still build
  ([Compiler §13.3.1](compiler.md#1331-stages), stage 12).
- Its modules' **types** can be imported (`import type UnitFrame from
  "suuui/UI/UnitFrame"`, by its package name); a value import of one is error
  E1041, as the module is not part of the addon.
- A cycle of `required` addon dependencies within the workspace is error E4324.
  Two members with the same `addon.name` are error E4325.
- SavedVariables names must be unique across the workspace (E4120).
- `wowlua package` can produce one zip containing several AddOn folders
  (`publish.bundle-addons`).

## 10.9 Types packages

The WoW API is typed by **pre-generated `types` packages**, one per flavor:

| Package | Content |
|---------|---------|
| `wow-api-<flavor>` (e.g. `wow-api-standard`, `wow-api-camelot`) | Global API functions, `C_*` namespaces, enums, structures, events with typed payloads, widget classes, FrameXML globals and mixins, and WoW's Lua environment additions, as `.d.wowlua` |

**Generation.** The packages are produced in the wowlua project's CI by
`wowlua-types-gen`:

1. **Primary source: Blizzard's API documentation**
   (`Blizzard_APIDocumentationGenerated`), read per flavor from the public
   `wow-ui-source` mirror branches (`live`, `ptr`, `classic`, `classic_era`,
   `classic_anniversary`, `classic_titan`, `forever`, …). It provides functions
   with `Nilable`/`Default` parameters, multiple returns, structures, enums,
   events with payloads, and secret-value annotations (→ `Secret<T>`, see
   [Types §2.11](../language/02-types.md#211-secret-values)). It is loaded by
   executing the documentation tables in a sandboxed Lua VM, as existing
   community tools do. Where the flavor's client is installed, `cargo xtask
   types fetch` puts the installed build's UI code and global strings in
   place of the mirror's (read as the test runner reads them,
   [Emulator §17.7](emulator.md#177-blizzards-ui-code), decision T553), and
   the declarations name that build besides the mirror commit CI fetches.
   Where it is wrong or incomplete against the client
   (a recording or Blizzard's own calls), the generator corrects it, each
   with its evidence: `C_Timer`'s handles, the unit name functions' absent
   values, `GetCursorInfo`'s results (decision T259), and others.
2. **Enrichment: Ketho's vscode-wow-api annotations** (MIT). They are
   converted from LuaLS `---@` annotations and merged where they add
   information that Blizzard's documentation lacks: widget class hierarchies
   and methods, hand-written structures and string-literal unions (unit tokens,
   frame types, templates), FrameXML globals and mixins, documentation links,
   and deprecations. On conflict, Blizzard's data wins for signatures, and the
   enrichment wins only where Blizzard's data is missing or untyped (`any`).
   Where Blizzard documents a widget method's arguments or results as
   possibly secret (`SecretArguments`, `SecretReturns`,
   `SecretReturnsForAspect`), the annotated method takes or returns
   `T | Secret<T>` there too ([Types §2.11](../language/02-types.md#211-secret-values)).
   Blizzard's UI code adds its globals: the named frames of its XML, typed by
   their mixins, and those mixins as classes with the functions their Lua
   defines ([Types §2.8](../language/02-types.md#28-globals)).
3. **warcraft.wiki.gg's API pages** (CC BY-SA 4.0) for the functions
   Blizzard's documentation and the annotations leave untyped (decision
   T288): `cargo xtask types wiki` reads the pages through the wiki's API,
   once, and writes their signatures and the records they name as reviewed
   LuaLS annotations to `xtask/data/warcraft-wiki-api.lua`, which the
   generator reads (builds never reach the wiki). The declarations made
   from them carry the wiki's attribution and license
   (`LICENSE-warcraft-wiki.txt`). A page that shows several calls
   (`GetAchievementInfo(achievementID)` and `GetAchievementInfo(categoryID,
   index)`) gives an overload per call, with the same results; a value whose
   description says it can be `nil` is optional (decision T290).
4. **Blizzard's UI code** types the functions it defines and the ones it
   calls (decision T288): each global function and mixin function of its
   Lua gets the parameters its definition names, typed by what the body
   does with them (passed to a typed API function, arithmetic, compared
   with a literal, called) and by the literals Blizzard's calls pass;
   optional where the body tests it for `nil` or a call leaves it out; its
   results from its `return`s (literals, operators, typed calls, string
   methods, mixins it makes, its locals). A C function nothing documents
   takes as many parameters as Blizzard's calls pass, typed where they
   agree. A global value is typed by what it is made from (a mixin table
   is its mixin class, `EnumUtil.MakeEnum` a `{ [string]: number }`, a
   call its declared result, an alias what it names). The mixins the
   declarations refer to are classes. A field read of a value of a
   declared type is that type's field (`auraData.name` of an `AuraData`,
   through `?` and `Secret<…>`); a definition in a top-level `do … end`
   block counts as one at the top level; a returned call ending in `...`
   gives its typed values, then the rest open. A table the annotations
   declare as a namespace (`AuraUtil`) also has the functions its Lua
   defines that they lack (decision T296). A global or function no source of
   its flavor types takes the wiki's signature, else the type another
   flavor gives it when every type it names exists in this one (camelot's
   own `Clamp` is typed as retail's MathUtil.lua defines it; decision
   T293).
5. **Typed throughout.** The generated declarations themselves write no
   `any`: what no source tells is `unknown` (a function nothing types is
   `(...unknown) -> ...unknown`, a parameter `unknown` takes any argument, a
   result `unknown` is checked before use; decision T288). This is about
   the declarations wowlua ships, not the language: `any` stays a type
   authors may write ([Types §2.2](../language/02-types.md)).
6. **Flavor availability** from per-flavor global name dumps, used to verify
   that every declared global exists in that flavor.
7. **The client's answers.** Where the client's recorded answers (the
   engine's recordings, Emulation §17.8) contradict the sources, the
   generator corrects the declaration (decision T290): results the client
   gives that the documentation lacks (`C_PetBattles.IsTrapAvailable()`'s
   reason), lists the wiki gives as their first values
   (`GetChatWindowMessages(index)` is `...string`), and getters a fresh
   widget answers nil (`FontString:GetText()` is `string?`); a status
   bar's `OnValueChanged` handler takes the value alone (its `userInput`
   is a slider's), and its scripts get the value and range as possibly
   secret, as its getters return them. A function
   that gives nothing for an ID that names nothing keeps its documented
   results. The client's own C functions no source documents take what the
   client itself told (decision T294): the argument checks it was probed
   for (`camelot-usages.txt`: names and kinds), its answers to a call
   without arguments (the engine's `blanks.rs`), and else their names
   (`Is…`, `Has…`, `Can…` a boolean, `GetNum…` a number; unverified). The
   combat log's payload is typed as the wiki's `COMBAT_LOG_EVENT` page
   gives its eleven base values.
8. **What the functions do** (decision T380). Each API function, namespace
   function and widget method has a `---` doc comment above its
   declaration, which the language server shows on hover and completion
   (§16.2): the summary of its wiki page (the text before the first
   section, templates and a call written as text left out), the
   descriptions its `==Arguments==` and `==Returns==` give their values
   (under **Parameters** and **Returns**, values without one left out),
   and a link to the page, the wiki's markup turned into Markdown.
   `cargo xtask types wiki` writes them to
   `xtask/data/warcraft-wiki-docs.json`. Where the wiki says nothing,
   Blizzard's documentation's `Documentation` lines of the function and its
   values do; where neither does, a sentence wowlua wrote from the
   function's name, signature and the emulator's behaviour
   (`xtask/data/written-docs.json`), marked as such ("Description by
   wowlua: the API's documentation has none.").

Package versions follow the game version: `wow-api-standard@12.1.0` for build
12.1.0, and pre-releases for test clients (`12.1.5-ptr.70077`). Generator fixes
are released as `+gen.N` build metadata. SemVer ignores build metadata for
precedence; for `types` packages only, wowlua deliberately deviates and orders
versions that are otherwise equal by `N`, numerically, so the newest generator
output wins. The packages are published to the wowlua registry and as GitHub
release assets of the wowlua project, and include the MIT notice of the
enrichment source.

**Use.** For every flavor in the manifest, the matching `wow-api-<flavor>`
package is an implicit dev-dependency with the requirement `=X.Y.Z`, derived
from the flavor's pinned interface number (120100 → `=12.1.0`), using the
newest `+gen.N`. It can be pinned in `dev-dependencies` or per flavor
(`flavors.<id>.types`). `wowlua types sync` updates the flavor table and the
locked type package versions after a game patch.

**Converter.** `wowlua-types-gen` is also exposed as `wowlua types convert
<path>` and converts any LuaLS-annotated library (e.g. a library's `---@meta`
files) into `.d.wowlua`. Supported LuaLS tags: `@meta`, `@class` (with multiple
inheritance, which becomes an intersection for non-primary parents), `@field`, `@param`,
`@return`, `@overload`, `@alias` (including `---|` literal unions), `@enum`,
`@generic` (including backtick captures, which become literal-type generics),
`@type`, `@deprecated`, plus the type syntax `fun(...)`, `table<K,V>`, `T[]`,
tuples, literals and dotted names. Other tags are ignored.

## 10.10 Registry

The wowlua registry hosts `library`, `lua-library` and `types` packages.
Addons are distributed through addon sites instead. It runs at
`https://registry.wowlua.dev`; its server is the workspace's
`wowlua-registry` crate, which shares the manifest validation and the
archive rules of this section with the client (decision T104).

### 10.10.1 Index (sparse HTTP)

- `GET {registry}/config.json` returns `{ "api": "<api base>", "dl": "<download base>" }`.
- `GET {registry}/index/<name>` returns newline-delimited JSON, one line per
  published version:
  `{"name":"wowlua-ui","vers":"0.4.2","deps":[{"name":"libstub","req":"^1.0","kind":"normal"}],"cksum":"sha256-…","features":{},"yanked":false,"wowlua":">=0.3"}`
- Index files are cacheable and support `ETag` / `If-None-Match`.
- `cksum` is the checksum of the package's file tree (§10.6), which the
  server computes from the archive: the lockfile's `checksum`. `deps` are
  the manifest's `dependencies` (kind `normal`; `"optional": true` for an
  optional one, `"registry": "<url>"` for one from another registry);
  `dev-dependencies` are not published.

### 10.10.2 Download and publish

All registry traffic uses HTTPS; plain HTTP only to a registry on the same
computer (`localhost`, `127.0.0.1`, `[::1]`: a registry run locally, and
tests). Authenticated requests send `Authorization: Bearer <token>`.

| Request | Meaning | Success |
|---------|---------|---------|
| `GET {dl}/<name>/<version>.tar.gz` | Download a package archive | 200 |
| `PUT {api}/v1/packages/<name>/<version>` | Publish. `multipart/form-data` with `manifest` (the package's `wowlua.json`, normalized) and `archive` (the `.tar.gz`). The server checks that name and version match the manifest and computes the checksum. | 201 `{"name":…,"vers":…,"cksum":…}` |
| `DELETE {api}/v1/packages/<name>/<version>` | Yank: the version stays downloadable for existing lockfiles but is never chosen by new resolutions | 200 |
| `PUT {api}/v1/packages/<name>/<version>/unyank` | Undo a yank | 200 |
| `GET` / `PUT` / `DELETE {api}/v1/packages/<name>/owners` | List, add, remove owners (body: `{"users": [...]}`) | 200 |
| `GET {api}/v1/search?q=<words>&page=<n>&per_page=<n>` | Search (below); no token | 200 |

Errors use HTTP status 400 (invalid package), 401 (missing or bad token), 403
(not an owner), 404 (unknown package), 409 (version already exists), 413
(archive too large) and 429 (rate limit, with `Retry-After` in seconds), with the body
`{"errors": [{"code": "<machine code>", "detail": "<message>"}]}`.

**Search.** `q` holds words separated by spaces (at most 10); a package
matches when every word occurs, case-insensitively, in its name, its
keywords (`package.keywords`) or its description (`package.description`),
as its newest version has them. Each word scores 100 for the exact name, 60
for a name that starts with it, 40 for one that contains it, plus 30 for an
exact keyword (15 for a keyword that contains it) and 10 for the
description; results are ordered by the sum, then by name. An empty `q`
lists every package by name. A package whose versions are all yanked is not
found. `page` counts from 1, `per_page` defaults to 20 (at most 100); 400
for one that is not a number from 1. The answer:

```json
{ "packages": [ { "name": "lib-tooltip", "vers": "1.2.3", "description": "Tooltips for frames",
                  "keywords": ["ui"], "kind": "library", "score": 110 } ],
  "total": 1, "page": 1, "per_page": 20 }
```

`vers` is the newest version that is not yanked.

**Rate limits.** The server limits requests per client address with a token
bucket per class: publishes (by default 30 an hour), sign-ins (the GitHub
redirect and callback, 10 a minute), other changes (yank, owners, account
forms; 120 an hour) and reads (600 a minute); `/health` is not limited.
Behind a proxy, the client's address is the last `X-Forwarded-For` entry that
is not a trusted proxy, read only when the connection comes from one (the
server's `WOWLUA_REGISTRY_TRUSTED_PROXIES`); otherwise it is the
connection's. Over a limit is 429 `rate-limited` with `Retry-After`.

- Versions are immutable. A published version can be yanked, never replaced.
  A yanked version stays downloadable.
- Package names: lowercase `[a-z0-9-]`, 2–64 characters, first come first served.
  Names of well-known WoW libraries are reserved for their authors: the
  server's data file lists them (`crates/wowlua-registry/data/reserved.json`),
  and its admins extend it and assign each name to GitHub logins
  (`GET {api}/v1/reserved`, and for admins `PUT` / `DELETE
  {api}/v1/reserved/<name>` with `{"users": [...], "note": "…"}`). Until
  assigned, only admins may publish a reserved name (403). An author claims
  one with an issue form in wowlua's repository, which the name's page and
  the 403 link.
- **Pages.** `{registry}/` lists and searches the packages (by kind and
  flavor too) and `{registry}/packages/<name>[/<version>]` shows one: its
  manifest's fields, the install command, its README, versions,
  dependencies and the registry's packages that depend on it. A README is
  shown without raw HTML, with links to `http(s)`, `mailto` and anchors
  only and images as links (T475).
- **Accounts.** Users are GitHub logins: the registry's page
  (`{registry}/login`) signs in with GitHub and creates and revokes API
  tokens, which the server stores as their SHA-256 only. The publisher of a
  new package is its owner; owners added must be GitHub users. The last
  owner cannot be removed. Admins may act as an owner of every package.
- **Checks of a publish.** The manifest is validated with the manifest rules
  of the server's version (a package whose `package.wowlua` excludes that
  version is checked for the fields below only); the URL's name and version
  must match it and the archive's `wowlua.json` (400 otherwise); the kind
  must be a library kind (400, E4230); every dependency must come from a
  registry (400, E4231); the archive must follow §10.10.3 (400). The
  machine codes are `invalid-name`, `invalid-version`, `invalid-manifest`,
  `manifest-mismatch`, `invalid-archive`, `addon`, `dependency-source`,
  `unknown-user`, `last-owner`, `invalid-request`, `unauthorized`,
  `not-owner`, `reserved`, `not-admin`, `not-found`, `version-exists`,
  `too-large` and `rate-limited`. The size limit is the server's setting (10 MiB by default).

### 10.10.3 Package archive

`wowlua package --registry` (or `wowlua publish --target registry`) creates a
`.tar.gz` containing `wowlua.json`, sources, `.d.wowlua` files and license
files, but no `dist/`, tests or dev files. Precisely (decision T104):

- `wowlua.json` is the normalized manifest: the package's, without
  `dev-dependencies`, `publish` and `workspace`, a registry dependency's
  `registry` name replaced by its URL, pretty-printed. It is also the
  publish request's `manifest` field.
- Every other file of the package, except the build output (`build.out`),
  the tests (the `tests/` folder and `*.test.wowlua` files), dev files (a
  file or folder whose name starts with `.`, `wowlua.lock`) and what
  `build.exclude` excludes. Links are not followed.
- Entries are regular files at the archive's root, sorted, with mode 0644
  and the time `SOURCE_DATE_EPOCH` (else the commit time, else 0), so the
  archive is reproducible. It is written to
  `dist/packages/<name>-<version>.tar.gz`.
- Extraction takes regular files and folders only: a link, an absolute
  path or `..` is E4323; more than 10,000 files or 256 MiB unpacked is
  refused.
- An addon is E4230; a dependency from a `git`, `svn`, `path` or `archive`
  source is E4231 (consumers resolve a registry package's dependencies from
  its index).

## 10.11 Security

- **No code runs at fetch or build time.** Packages have no build scripts,
  install hooks or plugins. Dependency Lua is only compiled or copied. It runs
  in the game, or in the sandboxed test VM (no `io`, `os`, `loadfile`,
  `dofile`, `require`).
- **Archive and checkout safety.** Extraction rejects absolute paths, paths
  containing `..`, and symlinks or hardlinks that point outside the package
  (E4323). Git submodules are only fetched when opted in (§10.3).
- **Integrity.** Checksums are verified on every fetch (E4103). Registry and
  archive downloads require HTTPS (a registry on the same computer may use
  HTTP, §10.10.2).
- **Tokens.** `wowlua login <type>` stores tokens in the OS credential store
  (macOS Keychain, Windows Credential Manager, Secret Service on Linux; service
  `wowlua`, account = the site type); `wowlua logout <type>` removes them.
  Environment variables take precedence. Without a terminal (standard input
  or standard error not a terminal), wowlua does not use the credential
  store: `wowlua publish` reads tokens from `config.json` only, `wowlua login`
  saves them there (permissions `0600`) and says so, and `wowlua logout`
  removes them from there. Otherwise tokens are written to `config.json` only
  when no credential store is available, with warning W4326. Tokens are redacted in all output, including
  `-v` and `--dry-run`.
