# wowlua.json

`wowlua.json` sits at the root of every package and describes it: the addon's
metadata, the flavors it is built for, its dependencies, build settings,
profiles, tests and publish targets. This page lists its keys section by
section, with their types and defaults.

The file is strict JSON: no comments, no trailing commas. Keys are
`kebab-case`, and an unknown key is error E4002, so a typo never goes
unnoticed. Point `$schema` at the published JSON schema and your editor
completes and checks the keys as you type. `wowlua add`, `remove` and
`update` edit the file in place and keep its key order and indentation.

The top-level keys are `$schema`, `package`, `addon`, `lua-library`, `types`,
`flavors`, `dependencies`, `dev-dependencies`, `addon-dependencies`,
`features`, `build`, `check`, `config`, `profiles`, `minify-levels`,
`publish`, `meta`, `fmt`, `test`, `compat`, `workspace` and `stdlib`. Only
`package` is required, and a workspace root may leave even that out.

The [manifest chapter of the specification](../../spec/toolchain/manifest.md)
is the normative text behind this page.

## Complete example

An addon for retail and WoW Forever with one library, a release profile that
leaves a folder out, and three publish targets:

```json
{
  "$schema": "https://raw.githubusercontent.com/DomenikIrrgang/wowlua/main/schema/wowlua-0.1.json",
  "package": {
    "name": "my-addon",
    "version": "1.4.2",
    "description": "Tracks things.",
    "authors": ["Jane Doe <jane@example.com>"],
    "license": "MIT",
    "wowlua": "^0.13"
  },
  "addon": {
    "title": "|cff33ff99My|r Addon",
    "notes": "Tracks things.",
    "saved-variables": ["MyAddonDB"],
    "saved-variables-per-character": ["MyAddonCharDB"]
  },
  "flavors": {
    "standard": {},
    "camelot": {}
  },
  "dependencies": {
    "ringlib": { "git": "https://github.com/example/RingLib", "tag": "1.0.3", "kind": "lua-library" }
  },
  "build": {
    "assets": ["media/**"],
    "exclude": ["src/**/*.scratch.wowlua"]
  },
  "check": {
    "mode": "basic",
    "lints": { "W1003": "allow" }
  },
  "config": {
    "maxHistory": 50
  },
  "profiles": {
    "dev": { "config": { "maxHistory": 500 } },
    "release": { "exclude": ["src/Diagnostics/**"] }
  },
  "publish": {
    "changelog": "CHANGELOG.md",
    "targets": {
      "curseforge": { "type": "curseforge", "project-id": 123456 },
      "wago": { "type": "wago", "project-id": "a1b2c3d4" },
      "github": { "type": "github", "repository": "jane/my-addon" }
    }
  },
  "fmt": { "indent": 4 },
  "test": { "coverage": { "lines": 80 } }
}
```

There is no TOC file to write: wowlua generates one per flavor from this file.
`wowlua toc` prints what it would write.

## package

What the package is and which compiler it needs.

### `package.name`

A string, required.

The package ID: 2 to 64 characters, lowercase letters, digits and `-`. Other
packages depend on it by this name. The addon's folder name comes from
`addon.name`, not from here. An invalid name is error E4003.

### `package.version`

A string, required.

The version, in SemVer 2.0 (`1.4.2`, `2.0.0-beta.1`). The TOC's `## Version:`
is the build version made from it: `1.4.2` for a release build on a matching
tag, `1.4.2-alpha+<hash>` for a release build that is not on one, and
`1.4.2-dev+<hash>` for `dev` and `test` builds. A release build on a tag that
does not match the version is error E4319.

### `package.kind`

`"addon"`, `"library"`, `"lua-library"` or `"types"`. Default `"addon"`.

What the package is: an addon, a wowlua library other packages import, a
library of plain Lua files, or a package of type declarations. The `addon`
section is only allowed in an `addon` package (E4317). See
[Libraries and dependencies](../guide/packages.md).

### Other `package` keys

| Key | Type | Default | Meaning |
|-----|------|---------|---------|
| `description` | string | none | One line about the package |
| `authors` | string[] | `[]` | `"Name"` or `"Name <email>"`. `wowlua new` fills it from git's `user.name` |
| `license` | string | none | An SPDX expression, such as `"MIT"` |
| `repository`, `homepage` | string | none | URLs |
| `keywords` | string[] | `[]` | For registry search |
| `wowlua` | version requirement | none | The compiler version the package needs, such as `"^0.13"`. It is read before anything else: an older compiler stops with E4300, which names both versions, instead of reporting newer keys as unknown |
| `main` | path | `src/init.wowlua` | A library's main module |

### `lua-library` and `types`

Two sections for library packages, in place of `addon`:

| Key | Type | Default | Meaning |
|-----|------|---------|---------|
| `lua-library.files` | path[] | the package's `.toc`, if it has exactly one | The entry files in load order: `.lua`, `.xml`, or a `.toc` whose file list is used |
| `lua-library.license-files` | path[] | `LICENSE*`, `COPYING*` | Files copied with the library as its license |
| `types.declarations` | glob[] | `["**/*.d.wowlua"]` | The declaration files of a `types` package |

## addon

The addon's metadata. Every key becomes a TOC directive, a field of
`wowlua:meta`, or both. Only an `addon` package has this section.

### `addon.name`

A string. Default: `package.name` in PascalCase (`my-addon` becomes
`MyAddon`).

The AddOn's folder name and the name of its TOC file. Any name a folder can
have works, including load-order prefixes such as `!MyLoader`. It must not
contain `\ / : * ? " < > |` or control characters, nor start or end with a
space or a dot.

### `addon.saved-variables`

A string array. Default `[]`. TOC: `SavedVariables`.

The globals the game saves for your addon across sessions. They are declared
as globals for the checker, and they are the names `saved.bind` from
[`wowlua:saved`](../stdlib/saved.md) accepts (E8020 for any other).
`saved-variables-per-character` is the same for `SavedVariablesPerCharacter`.

### Other `addon` keys

| Key | Type | Default | TOC directive and notes |
|-----|------|---------|-------------------------|
| `title` | string | `addon.name` | `Title`. May contain color codes |
| `title-localized` | `{locale: string}` | `{}` | `Title-<locale>` |
| `notes`, `notes-localized` | string, `{locale: string}` | none | `Notes`, `Notes-<locale>` |
| `author` | string | the names in `package.authors`, without emails, joined by `, ` | `Author` |
| `category`, `category-localized` | string, `{locale: string}` | none | `Category`, `Category-<locale>` |
| `group` | string | none | `Group`: the parent addon of a suite |
| `icon-texture`, `icon-atlas` | string | none | `IconTexture`, `IconAtlas` |
| `load-on-demand` | bool | `false` | `LoadOnDemand`. `false` together with `addon-dependencies.load-with` is E4312 |
| `default-state` | `"enabled"`, `"disabled"` | `"enabled"` | `DefaultState` |
| `saved-variables-per-character` | string[] | `[]` | `SavedVariablesPerCharacter` |
| `load-saved-variables-first` | bool | `false` | `LoadSavedVariablesFirst` |
| `allow-table-access` | bool | `false` | `AllowAddOnTableAccess`: other addons may read your addon table with `C_AddOns.GetAddOnLocalTable` |
| `only-beta-and-ptr` | bool | `false` | `OnlyBetaAndPTR` |
| `toc-extra` | `{string: string}` | `{}` | Extra directives, written as they are: `AddonCompartmentFunc`, `AddonCompartmentFuncOnEnter`, `AddonCompartmentFuncOnLeave`, `AllowLoadGameType`, `ExcludeLoadGameType`, or your own `X-` fields. Other unknown keys are warning W4001; a directive wowlua writes from another key (`Title`, `SavedVariables`, …) is error E4002 |
| `expose-meta` | string | none | No directive. Also stores `wowlua:meta`'s runtime tables in this global |
| `meta` | `{string: string, number or bool}` | `{}` | No directive. Your own values, in `addon.meta` of `wowlua:meta` |
| `flavor-overrides` | `{flavor: object}` | `{}` | Metadata that differs per flavor: `null` removes a field for that flavor ([§9.3.1](../../spec/toolchain/manifest.md#931-per-flavor-metadata)) |
| `toc-data` | `{flavor: path[]}` | `{}` | Files of `## Key: Value` lines added to that flavor's TOC, for addons that keep data in the TOC ([§9.3.2](../../spec/toolchain/manifest.md#932-toc-data-files)) |

`## Version:` always comes from `package.version` and has no key of its own.

## flavors

The game clients the package is built for. Each key is a flavor ID or an
alias, and its value an object, usually empty:

```json
"flavors": { "standard": {}, "camelot": {} }
```

Without `flavors`, an addon is built for `standard` only, and a library
supports every flavor.

| Flavor ID | Aliases | Client | Support |
|-----------|---------|--------|---------|
| `camelot` | `forever` | WoW Forever | Supported |
| `standard` | `retail`, `mainline` | Retail | Coming soon |
| `vanilla` | `classic-era` | Classic Era | Coming soon |
| `tbc` | `anniversary` | TBC Anniversary | Coming soon |
| `mists` | | MoP Classic | Coming soon |

wowlua runs a supported flavor everywhere (tests, the emulated client, the
debugger); a flavor coming soon is checked and built, but not run yet. An
unknown flavor is error E4004, and so are `cata` and `wrath`: wowlua has no
Wrath of the Lich King Classic or Cataclysm Classic flavor. How code differs
per flavor is in [Build info and profiles](../language/flavors.md).

### `flavors.<flavor>.interface`

A number array. Default: the numbers pinned in `wowlua.lock`.

The interface numbers written in that flavor's TOC. Leave it out: the first
build pins the numbers of the flavor table that ships with wowlua in
`wowlua.lock`, so every machine writes the same TOC, and later builds keep
those pins. After a game patch, `wowlua types sync` moves the pins to the
flavor table of the wowlua you have. Set it to write other numbers. A flavor
with neither is error E4318.

The other keys of a flavor are `client-folders`, the folders under your WoW
installation that `wowlua dev` and `wowlua install` copy the addon into
(default: the flavor's own), and `types`, a version requirement for the
flavor's API declarations. wowlua ships one version of each flavor's
declarations, the game version of its interface number (`12.1.0` for
`120100`); a requirement it doesn't meet is error E4318.

## dependencies

Libraries and other packages the addon uses. `dependencies` ship with the
addon. `dev-dependencies` are only for checking, tests and tools, such as type
packages and test helpers, and are never embedded.

```json
"dependencies": {
  "ringlib": { "git": "https://github.com/example/RingLib", "tag": "1.0.3", "kind": "lua-library" },
  "shared-utils": { "path": "../shared-utils" }
}
```

### `dependencies.<name>`

A version requirement string or a source object.

The key names the dependency; the object says where it comes from:

| Source | Fields |
|--------|--------|
| registry | `version` (a version requirement), optional `registry` |
| git | `git`, plus one of `tag`, `branch` or `rev`; optional `subdir`, and `submodules` (default `false`) |
| svn | `svn`, optional `rev` and `submodules` |
| path | `path`, relative to the manifest |
| archive | `archive` (a `https://` or `file://` URL of a `.zip` or `.tar.gz`), `sha256`, optional `strip-prefix` |

A plain string such as `"^0.4"` is a version requirement against the
[wowlua registry](https://registry.wowlua.dev) (`^1.2`, `~1.2.3`, `=1.2.3`,
`>=1.0, <2.0`, `*`, as in Cargo). The object form `{ "version": "^0.4",
"registry": "…" }` names another registry: its URL, or a name under
`registries` in `~/.wowlua/config.json`. A package or version the registry
doesn't have is error E4102. A package published to the registry depends
on registry packages only (E4231).

Every dependency may also have these fields:

| Field | Type | Default | Meaning |
|-------|------|---------|---------|
| `kind` | package kind | the package's own | Needed for a source without a `wowlua.json`, such as a plain Lua library |
| `optional` | bool | `false` | Only included when a feature turns it on |
| `flavors` | flavor[] | all | Only included for these flavors |
| `bundle` | bool | the profile's | `false` keeps the library in its own files in bundled builds |
| `files` | path[] | from the library | A `lua-library`'s entry files, in order |
| `load` | bool | `true` | `false` ships a `lua-library` without loading it |
| `types` | string | none | A `types` package or a path to `.d.wowlua` files for the library |
| `install-path` | path | the dependency's key | Its folder under `build.libs-dir` |
| `addon-name` | string | the install folder's name | A `lua-library`'s name as a standalone addon, for `OptionalDeps` in nolib packages |
| `curseforge-slug` | string | none | For CurseForge relations |
| `features` | string[] | `[]` | The dependency's features to turn on |
| `default-features` | bool | `true` | Turn on its `default` features |
| `config` | object | `{}` | Overrides of a wowlua library's `config` defaults |

The standard library ships with the compiler and is never declared here. The
whole story is in [Libraries and dependencies](../guide/packages.md).

### `addon-dependencies`

Other addons yours needs in the game, as TOC directives:

| Key | TOC directive | Entries |
|-----|---------------|---------|
| `required` | `Dependencies` | AddOn names, or `{ "name": …, "curseforge-slug": …, "flavors": […] }` |
| `optional` | `OptionalDeps` | the same |
| `load-with` | `LoadWith` | AddOn names. Makes the addon load on demand |
| `load-managers` | `LoadManagers` | AddOn names |

An entry with `flavors` is only written for those flavors.

### `features`

Optional parts of the package that a build turns on:

```json
"features": {
  "default": ["minimap"],
  "minimap": [],
  "comms": ["dep:libdeflate"]
}
```

Each feature lists other features, or `dep:<name>` to include an optional
dependency. `default` is on unless `--no-default-features` is given, and
`--features a,b` turns on more. In code a feature is a constant of
`wowlua:meta`, in camelCase (`experimental-ui` is `features.experimentalUi`).
An entry naming an unknown feature, an unknown dependency or one that is not
optional is error E4314.

## build

Where the sources are and what goes into the AddOn folder.

### `build.src`

A path. Default `"src"`.

The folder that holds your sources.

### `build.libs-dir`

A path. Default `"libs"`.

The folder in the output that dependencies go into. Projects adopted from the
BigWigs packager often use `"Libs"`, so their XML files keep working.

### `build.assets`

A glob array. Default `[]`.

Textures, sounds, fonts and other files copied into the AddOn as they are, at
their path relative to the package root. `build.exclude` leaves files out of
both the sources and the assets:

```json
"build": {
  "assets": ["media/**"],
  "exclude": ["src/**/*.scratch.wowlua"]
}
```

### Other `build` keys

| Key | Type | Default | Meaning |
|-----|------|---------|---------|
| `out` | path | `"dist"` | The output root. Workspace members use the workspace's (E4316 if a member sets its own) |
| `exclude` | glob[] | `[]` | Files left out of sources and assets |
| `load-order` | (path or glob)[] | `[]` | An explicit order for files with no load-time dependency between them. `wowlua init` writes it |
| `flavor-load-order` | `{flavor: (path or glob)[]}` | `{}` | One flavor's own order, used instead of `load-order` |
| `keep` | string[] | `[]` | Module IDs that dead-code removal never removes |
| `flavor-files` | `{glob: flavor[]}` | `{}` | Restricts the matching files to those flavors. Mainly for plain Lua files, which can't use `@if` |
| `variable-files` | object[] | `[]` | TOC file lines with `[Game]`, `[Family]` or `[TextLocale]` ([§9.7.1](../../spec/toolchain/manifest.md#971-file-variables)) |
| `external-files`, `shared-files` | object[], path[] | `[]` | Files one addon ships and another addon's TOC loads ([§9.7.2](../../spec/toolchain/manifest.md#972-files-shared-between-addons)) |
| `flavor-mode` | `"split"`, `"shared"` | `"split"` | One build per flavor, or one build all flavors share ([Build info and profiles](../language/flavors.md)) |
| `toc-layout` | `"auto"`, `"single"`, `"per-flavor"` | `"auto"` | One TOC for all flavors or one per flavor; `auto` picks one TOC when every targeted client supports it |
| `locales` | `"client"`, `"all"` | `"client"` | Load only the player's language, or all of them ([Localization](../stdlib/locale.md)) |
| `locales-dir` | path | `"locales"` | The folder of locale JSON files |
| `locale-base` | locale code | `"enUS"` | The locale that defines the keys and fills in missing translations |

## check

How strictly `wowlua check`, the build and the editor check your code.

### `check.mode`

`"nocheck"`, `"basic"` or `"strict"`. Default `"basic"`.

The project's check mode. `basic` reports likely mistakes and lets unknown
names pass with a warning; `strict` asks for types where it can't infer them
and treats possible `nil` as an error. `check.strict` lists files checked in
strict mode whatever the mode is, and `check.lua` (default `false`) also
type-checks the project's `.lua` files, reporting warnings only. See
[Types](../language/types.md).

### `check.secret-widget-results`

A boolean. Default `false`.

A widget's getter returns a secret once one of its setters got one, and the
checker can't follow which widget did. By default the getters Blizzard
documents this way (`GetText`, `GetWidth`, `IsShown`, `GetPoint`, …) are
typed plainly; `true` types them `T | Secret<T>`, so every use needs
`issecretvalue` or `canaccessvalue` first. A status bar's `GetValue()` and
`GetMinMaxValues()` are among them.

```json
"check": { "mode": "strict", "secret-widget-results": true }
```

### `check.lints`

An object from a warning code to `"allow"`, `"warn"` or `"deny"`. Default
`{}`.

Changes a warning's level: `allow` hides it, `deny` makes it an error, `warn`
keeps it a warning. The keys are `W` codes, including the warning forms that
name and type errors take in plain Lua files (`"W2001"`); anything else is
error E4002. W2063, an implicit `any`, is off until you set it to `"warn"`.
`check.globals` adds names the checker accepts as globals, typed `any`.

```json
"check": {
  "mode": "strict",
  "lints": { "W1003": "allow", "W2063": "warn" },
  "globals": ["SomeOtherAddon"]
}
```

What each code means is in [Errors and warnings](diagnostics.md).

## profiles

The settings of each build. The built-in profiles are `dev` (what
`wowlua dev` builds), `release` (what you publish) and `test` (what
`wowlua test` runs). A profile here is merged over the built-in one of the same
name: objects key by key, arrays and other values replaced. Any other name is a
custom profile and must say which profile it builds on with `inherits` (E4315
if it doesn't; a cycle is E4007).

```json
"profiles": {
  "release": { "minify": "compact" },
  "beta": { "inherits": "release", "log-level": "info" }
}
```

### `profiles.<profile>.minify`

A level name. Default `"none"` in `dev` and `test`, `"strip"` in `release`.

How much the Lua output is minified: `none`, `strip` (comments and whitespace),
`compact`, `rename` (local names too), `max`, or a level of your own from
`minify-levels`. CurseForge, Wago and WoWInterface targets build unminified
unless the target sets `minify` itself. A target that does, with a level that
minifies anything, gets warning W6010: Blizzard's AddOn policy says code must
not be hidden or obfuscated. See
[Optimization §15.4](../../spec/toolchain/optimization.md#154-minification-levels).

### `profiles.<profile>.bundle`

A bool. Default `false` in `dev` and `test`, `true` in `release`.

Bundles the Lua into as few files as possible. Every built-in profile writes
source maps, so `wowlua trace` still maps errors from the game back to your
source lines.

### `profiles.<profile>.optimize`

An object of these settings; one you leave out keeps the profile's value.
Default: all off in `dev` and `test`, all on (`cache-globals` `"safe"`) in
`release`.

| Key | Values | What it does |
|-----|--------|--------------|
| `inline` | bool | Replaces calls of small local functions (and `@inline` private methods) with their body: a single `return` expression wherever it's called, statements where the call is a statement of its own |
| `cache-globals` | `"off"`, `"safe"` | Copies read-only globals, such as `pairs` or a `C_*` namespace table, into file-level locals |
| `dead-code` | bool | Removes unused locals and local functions, and modules without side effects that nothing uses (warning W6002 names each) |
| `constants` | bool | A local assigned once with a constant (a number, a boolean, `nil`, a short string) becomes the constant where it's read |
| `cache-fields` | bool | Reads a field chain such as `self.frame.texture` once into a local: before a loop that can't change it, or once for repeated reads in a stretch of code |
| `cache-upvalues` | bool | A function that reads an outer local in a loop (or three times or more), which nothing assigns, starts with `local name = name` |
| `numeric-loops` | bool | `for i, v in ipairs(t)` over an array becomes a numeric loop that stops at the first `nil`, as `ipairs` does |
| `table-insert` | bool | `table.insert(t, v)` becomes `t[#t + 1] = v` |
| `hoist-closures` | bool | A function made in a loop that captures nothing of the loop is made once, before it: a local function only called, or one passed to `pcall`, `xpcall`, `table.sort` or `string.gsub` |

`cache-fields`, `numeric-loops` and `table-insert` need types: they apply in
strict-mode files (`--!strict`, `check.mode`), where the type checker proved
the tables plain (no metatable function between you and the field) and the
values numbers. None of the last six touch plain `.lua` files. Unbundled
builds keep every instruction on its source line, so they don't inline
statement bodies or move closures. The passes stay within Lua 5.1's limits
on locals and upvalues. `wowlua bench` measures what they change; see
[Testing](../guide/testing.md#benchmarks) and
[Optimization §15.2](../../spec/toolchain/optimization.md#152-profile-controlled-optimizations).

```json
"optimize": { "inline": true, "cache-globals": "safe", "dead-code": true, "hoist-closures": false }
```

### `profiles.<profile>.exclude`

A glob array. Default `[]`.

Files left out of this profile's builds, in addition to `build.exclude`. Use
it to keep developer tools in `dev` and test builds and out of the release.
Code that is kept and imports such a file guards the import with
`@if(build.debug)`.

```json
"profiles": {
  "release": { "exclude": ["src/Diagnostics/**"] }
}
```

### Other profile keys

The debugging settings, with their built-in values. What each does is in
[Debugging helpers](../language/debugging.md).

| Key | Type | `dev` | `test` | `release` |
|-----|------|-------|--------|-----------|
| `debug` | bool | `true` | `true` | `false` |
| `assertions` | bool | `true` | `true` | `false` |
| `log-level` | `"trace"`, `"debug"`, `"info"`, `"warn"`, `"error"`, `"off"` | `"debug"` | `"trace"` | `"warn"` |
| `instrument` | `"none"`, `"decorated"`, `"exports"`, `"all"` | `"all"` | `"none"` | `"none"` |
| `runtime-checks` | `"none"`, `"exports"`, `"all"` | `"exports"` | `"all"` | `"none"` |
| `reflection` | `"none"`, `"decorated"`, `"all"` | `"decorated"` | `"all"` | `"decorated"` |
| `profile-errors` | bool | `false` | `false` | `false` |
| `profile-call-paths` | bool | `false` | `false` | `false` |
| `profile-garbage` | bool | `false` | `false` | `false` |
| `frame-budget` | number (ms) | `1` | `1` | `1` |
| `slow-call` | number (ms) | `2` | `2` | `2` |
| `dev-console` | bool | `true` | `false` | `false` |
| `monitor` | bool | `true` | `false` | `false` |
| `capture-errors` | bool | `true` | `false` | `false` |
| `source-maps` | bool | `true` | `true` | `true` |

`profile-call-paths` and `profile-garbage` start the dev window with call
paths or garbage per function switched on, and `frame-budget` and
`slow-call` are its starting frame budget and slow-call threshold. The
window can change all four while the game runs and remembers them per
addon.

`minify-libraries` (default `false`) applies the minification level to Lua
library dependencies too. `line-mode` (`"preserve"`, `"statement"` or
`"pretty"`) sets how output lines match source lines: by default `preserve`
for unbundled builds and `statement` for bundled ones. `config` overrides
values of the top-level `config`.

### `config`

Your own settings with their defaults, in the top-level `config`. Profiles
change them in their own `config`, and code reads the result as `config` from
`wowlua:meta`, typed after the top-level object:

```json
"config": { "maxHistory": 50 },
"profiles": { "dev": { "config": { "maxHistory": 500 } } }
```

A key a profile sets that the top-level `config` lacks, or a value of another
type, is error E4012.

### `minify-levels`

Minification levels of your own, by name: `{ "extends": "<level>",
"passes": [ … ] }`. An unknown pass name is error E6003. The passes are listed
in [Optimization §15.4.1](../../spec/toolchain/optimization.md#1541-passes).

## publish

Where `wowlua publish` uploads the addon, and with which changelog. The steps
are in [Building and publishing](../guide/publishing.md).

### `publish.targets`

An object from a target name to a target. Default `{}`.

The names are yours. Each target has a `type` and the fields that type needs,
and may change the profile, the minification and more:

| Field | Applies to | Type | Default |
|-------|------------|------|---------|
| `type` | all | `"curseforge"`, `"wago"`, `"wowinterface"`, `"github"`, `"registry"` | required |
| `enabled` | all | bool | `true` |
| `profile` | all | profile name | `"release"` |
| `minify` | all | level name | `"none"` for CurseForge, Wago and WoWInterface; the profile's for the others |
| `bundle` | all | bool | the profile's |
| `flavors` | all | flavor[] | every flavor of the manifest |
| `split-flavors` | all | bool | `false` |
| `nolib` | all | bool | `false`: a package without the embedded Lua libraries |
| `release-type` | all | `"auto"`, `"release"`, `"beta"`, `"alpha"` | `publish.release-type` |
| `changelog`, `changelog-format` | all | as in `publish` | `publish`'s |
| `token-env` | all | string | the type's environment variable |
| `api-url` | all | URL | the site's |
| `project-id` | curseforge, wowinterface | number | required |
| `project-id` | wago | string of 8 characters | required |
| `relations` | curseforge | `{ "slug", "type" }[]` | from the dependencies |
| `changelog-output` | wowinterface | `"bbcode"`, `"markdown"` | `"bbcode"` |
| `archive-previous` | wowinterface | bool | `true` |
| `upload-untagged` | wowinterface | bool | `false` |
| `repository` | github | `"owner/repo"` | required |
| `release-json` | github | bool | `true` |
| `draft` | github | bool | `false` |
| `prerelease` | github | `"auto"` or bool | `"auto"`: a prerelease unless the release type is `release` |
| `registry` | registry | URL | the default registry |

A `library`, `lua-library` or `types` package that declares no `registry`
target has one anyway, named `registry`; an addon with a `registry` target
is error E4230 when it publishes. A target without its required field is
error E4002. `"enabled": false` turns a
target off, for example one a workspace shares.

```json
"publish": {
  "targets": {
    "curseforge": { "type": "curseforge", "project-id": 123456 },
    "github": { "type": "github", "repository": "jane/my-addon", "draft": true }
  }
}
```

### `publish.bundle-addons`

A bool or a string array. Default `false`.

Only in a workspace root. The root's targets publish one zip with all member
addons, or with those listed, named after the main one (the first listed, or
the first default member) and with its version and changelog.

### Other `publish` keys

| Key | Type | Default | Meaning |
|-----|------|---------|---------|
| `changelog` | path, `"git"` or `null` | `"CHANGELOG.md"` if that file exists, else `null` | Where the release notes come from: a file, the git history, or none |
| `changelog-format` | `"markdown"`, `"text"`, `"html"` | `"markdown"` | The changelog file's format |
| `release-type` | `"auto"`, `"release"`, `"beta"`, `"alpha"` | `"auto"` | The release type sent to the sites |
| `archive-maps` | bool | `true` | Keep the source maps of published builds, for `wowlua trace` |

## fmt

How `wowlua fmt` and the editor's Format Document lay out your code. See
[VS Code](../guide/vscode.md).

| Key | Type | Default | Meaning |
|-----|------|---------|---------|
| `indent` | `"tab"` or a number | `"tab"` | A tab, or that many spaces (2, 4, 8) |
| `line-width` | number | `100` | The width lines are wrapped at |
| `quote-style` | `"double"`, `"single"`, `"preserve"` | `"double"` | Quotes of string literals |
| `lua` | bool | `false` | Format `.lua` files too |
| `exclude` | glob[] | `[]` | Files left as they are, such as data your tools generate. A file named on the command line is still formatted |

```json
"fmt": { "indent": 4, "exclude": ["src/Data/**"] }
```

## test

Settings of `wowlua test`. The test files themselves are described in
[Testing your addon](../guide/testing.md).

### `test.blizzard-ui`

A bool. Default `false`.

Loads Blizzard's own UI code before your addon, so tests and screenshots see
the default UI around it.

When the flavor's game is installed, wowlua reads Blizzard's code and global
strings from your installation, once per game version, so tests run against
exactly your client's UI. Without the game (in CI, say) it downloads them from
wow-ui-source, at the version wowlua's API declarations come from. Set
`WOWLUA_UI_FROM=upstream` to use the download even with the game installed.

### `test.profile`

A string. Default: none.

The world profile for test files that don't choose one: a file
`tests/profiles/<name>.json` with your character, its gear and bags, and the
world around it. A name with no such file is error E4402.

### `test.coverage`

An object with `lines`, `statements`, `branches`, `functions` and
`conditions` (numbers, in percent: 0 to 100) and `exclude` (globs). Default:
no minimum.

The minimum coverage. A run of the whole suite below one of them fails; a run
of only some tests is not checked. `exclude` leaves files out of the coverage.

```json
"test": {
  "blizzard-ui": true,
  "coverage": { "lines": 80, "exclude": ["src/Debug/**"] }
}
```

### Other `test` keys

| Key | Type | Default | Meaning |
|-----|------|---------|---------|
| `platform` | `"mac"`, `"windows"` | `"mac"` | The machine the emulated client runs on, for answers that differ by machine and `IsMacClient` |
| `screen` | `{ width, height, dpiScale?, uiScale? }` | `{ "width": 1920, "height": 1080 }` | The emulated screen, in pixels. `uiScale` is the game's UI Scale slider with "Use UI Scale" on: 0.65 to 1.15 in steps of 0.01 (65% to 115%); without it, the game scales the UI itself |
| `reporters` | string[] | `["pretty"]` | Default reporters: `pretty`, `junit`, `tap`, `json`, `github`, `lcov`, `cobertura`, `coverage-json`, `html` |
| `addons` | path[] | `[]` | Other addons' folders, each loaded before yours as its own addon |

## Other sections

### `meta`

`meta.runtime` is `"used"` (the default), to write only the `wowlua:meta`
fields your code reads at run time, or `"full"` to write all of them.

### `stdlib`

Settings of standard-library modules, read only when the module is used.
`stdlib.log.history-size` (default `500`, at least 1) is the size of
`wowlua:log`'s history.

### `compat`

Settings for addons adopted from the BigWigs packager: reading `.pkgmeta`
(`pkgmeta`), the packager's keyword substitution (`packager-keywords`), and how
it names, versions and packages releases. `localization` names the package's
locale files for the `@localization(…)@` keyword, as a path with `{locale}`
(`"locales/export/{locale}.lua"`): each keyword is replaced by its locale's
file, and a locale without one stays a comment; it works with or without
`packager-keywords`. List those files in
`build.exclude` if they shouldn't ship. `wowlua init` writes them; see
[Bring your addon](../guide/adopting.md) and
[§9.14](../../spec/toolchain/manifest.md#914-compat).

### `workspace`

Only in a workspace root's manifest. `members` lists the member folders as
globs, `default-members` the members used when no `-p` is given, and
`exclude` folders left out. `external-addons` names AddOns fetched and shipped
as they are. The root's `profiles`, `minify-levels`, `check`, `fmt`, `config`
and `publish` are shared defaults that each member's own values are merged
over. See [§9.15](../../spec/toolchain/manifest.md#915-workspace).
