//! Blizzard's UI code (spec emulator §17.7, decision T186): the flavor's
//! Blizzard addons loaded as secure code before the addon under test, in
//! the client's load order, their XML built into widgets.
//!
//! The startup set is the client's (`/stdtest engine`: the 201 addons WoW
//! Forever had loaded when StdTest loaded): every Blizzard addon allowed in
//! the game and for the flavor's game type, read from the client's
//! suffixed TOC first, not load-on-demand unless a startup addon depends on
//! it, beta- and PTR-only ones on such clients. Their order is inferred
//! from what Blizzard's code needs at load (unverified beyond that):
//! Blizzard_SharedXMLBase and Blizzard_SharedXML, then the secure
//! environment's addons, then the `LoadFirst` ones, then the rest, by
//! name, each after the addons it depends on and right before the startup
//! addons that load with it (`LoadWith`).
//!
//! `UseSecureEnvironment` addons run with globals of their own, reading
//! through to the addons' (theirs stay out of them: WoW Forever's addons
//! see none of Blizzard_CommunitiesSecure's); `[LoadIntoEnvironment
//! global]` files and `SwapToGlobalEnvironment()` write to the addons'.
//! What Blizzard_EnvironmentCleanup removes from the addons' globals stays
//! in the secure ones.

pub mod build;
pub mod xml;

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::rc::Rc;

use wowlua_vm::{Value, Vm};

use crate::{engine, engine_mut};
use xml::Element;

/// The UI code's templates, intrinsic frame types and load errors.
#[derive(Default)]
pub struct Registry {
    /// Virtual elements by lower-case name.
    pub templates: HashMap<String, Rc<Element>>,
    /// The file each template came from (for errors).
    pub template_files: HashMap<String, String>,
    /// The globals each template was defined with: its mixins and scripts
    /// resolve there (a secure addon's template on an addon's frame).
    pub template_envs: HashMap<String, wowlua_vm::TableRef>,
    /// Intrinsic frame types by lower-case tag: the base class and element.
    pub intrinsics: HashMap<String, (String, Rc<Element>)>,
    /// Errors while loading: files that did not parse, Lua errors.
    pub errors: Vec<String>,
    /// The addons loaded, in order.
    pub loaded: Vec<String>,
    /// The addons whose bootstrap files ran, lower-case.
    pub bootstrapped: HashSet<String>,
    /// The secure addons' globals (`UseSecureEnvironment`), once made.
    pub secure_env: Option<wowlua_vm::TableRef>,
    /// An intrinsic type's element is being applied (its scripts stay).
    pub applying_intrinsic: bool,
    /// The files' results, by path (`import`): see `build::module_key`.
    pub modules: HashMap<String, Value>,
    /// Blizzard's errors as its handler logged them, stacks included
    /// (`C_Log.LogErrorMessage`).
    pub error_stacks: Vec<String>,
    /// The addon whose UI code is loading (not Blizzard's): its Lua and
    /// XML scripts compile as its code, carrying its taint.
    pub code_of: Option<String>,
    /// What an addon's files' chunk names start with while it loads: the
    /// `test.addons`' `Interface/AddOns/<Name>/`, as the client names them
    /// (`debugstack` in a recording: `Interface/AddOns/StdTest/src/
    /// engineprobe.lua:806`; libraries find their folder there). The test
    /// runner sets it for the addon tested too.
    pub chunk_prefix: Option<String>,
    /// The addon each of its templates came from, by lower-case name: their
    /// scripts are its code wherever they are applied.
    pub template_addons: HashMap<String, String>,
    /// Templates and intrinsic types declared under `<ScopedModifier
    /// allowUntaintedCreation="true">`, by lower-case name: frames made
    /// from them are built untainted, whoever calls `CreateFrame`.
    pub untainted_creation: HashSet<String>,
    /// Inside such a modifier while the UI code loads.
    in_untainted_creation: bool,
    /// `CreateFrame` running its new frame's `OnLoad` and `OnShow`: within
    /// the caller's execution, its taint going on (decision T243).
    pub creating: bool,
    /// Inside a `<ScopedModifier forbidden="true">` (or `fullLockdown`)
    /// while the UI code loads.
    pub in_forbidden: bool,
    /// Building a file's own (not virtual) elements as the UI code loads.
    pub building_file: u32,
    /// The secure delegates `<Mixin secureDelegates="true">` made, by the
    /// function they call: every frame shares them (each kept, so the
    /// function it holds is never collected).
    pub mixin_delegates: HashMap<wowlua_vm::FuncRef, Value>,
    /// Every Blizzard addon of the UI code, by lower-case name: what
    /// `C_AddOns.LoadAddOn` can load later.
    available: HashMap<String, Addon>,
    /// The folder holding `Interface`, once the code is loaded.
    root: Option<PathBuf>,
}

/// What loading the UI code did.
pub struct Report {
    pub addons: Vec<String>,
    pub errors: Vec<String>,
}

/// The game types a flavor's Blizzard code loads for: its id and family.
struct Target {
    game_types: Vec<String>,
    family: String,
    game: String,
    /// The client's TOC suffixes, in lookup order (`_Camelot`, `_Forever`,
    /// `_Mainline`).
    suffixes: Vec<String>,
    /// A beta or PTR client (`## OnlyBetaAndPTR` addons load): WoW Forever
    /// runs from `_forever_`.
    beta: bool,
}

impl Target {
    fn new(flavor: &str) -> Target {
        let table = wowlua_flavors::table();
        let family = table
            .flavor(flavor)
            .map(|f| f.family.name().to_string())
            .unwrap_or_else(|| "mainline".into());
        let cap = |s: &str| {
            let mut c = s.chars();
            c.next()
                .map(|f| f.to_uppercase().collect::<String>() + c.as_str())
                .unwrap_or_default()
        };
        let client = table.clients.iter().find(|c| c.flavor == flavor);
        Target {
            game_types: vec![flavor.to_string(), family.clone()],
            game: cap(flavor),
            family: cap(&family),
            suffixes: client.map(|c| c.suffixes.clone()).unwrap_or_default(),
            beta: client.is_some_and(|c| {
                c.folders
                    .iter()
                    .any(|f| f.contains("_beta_") || f.contains("ptr_"))
            }),
        }
    }

    /// Whether a line's or directive's inline conditions hold in the game
    /// (not the glue screens).
    fn allows(&self, conditions: &[wowlua_toc::InlineDirective]) -> bool {
        conditions.iter().all(|c| {
            let any = |list: &[String]| {
                list.iter()
                    .any(|v| self.game_types.iter().any(|g| g.eq_ignore_ascii_case(v)))
            };
            match c.name.to_ascii_lowercase().as_str() {
                "allowloadgametype" => any(&c.values),
                "excludeloadgametype" => !any(&c.values),
                "allowload" => c.values.iter().any(|v| {
                    let v = v.to_ascii_lowercase();
                    v == "game" || v == "both"
                }),
                "allowloadtextlocale" => c.values.iter().any(|v| v == "enUS"),
                _ => true,
            }
        })
    }

    /// `[Family]`, `[Game]` and `[TextLocale]` in a file line.
    fn path(&self, line: &str) -> String {
        line.replace("[Family]", &self.family)
            .replace("[Game]", &self.game)
            .replace("[TextLocale]", "enUS")
            .replace('\\', "/")
    }
}

/// One Blizzard addon: its directory, TOC and dependencies.
#[derive(Clone)]
struct Addon {
    name: String,
    dir: PathBuf,
    toc: wowlua_toc::ParsedToc,
    deps: Vec<String>,
    load_first: bool,
    /// `LoadWith`: the addons it loads with, lowercased (a startup addon
    /// loads right after them: Classic's Blizzard_UIParentPanelManager
    /// with Blizzard_UIParent, before the LoadFirst addons that use it).
    load_with: Vec<String>,
    /// Load-on-demand: loaded at startup only as a startup addon's
    /// dependency.
    on_demand: bool,
    /// `UseSecureEnvironment`: its globals are not the addons' (WoW
    /// Forever's main environment has none of Blizzard_CommunitiesSecure's).
    secure: bool,
}

fn directive<'a>(toc: &'a wowlua_toc::ParsedToc, t: &Target, key: &str) -> Vec<&'a str> {
    toc.directives
        .iter()
        .filter(|d| d.key.eq_ignore_ascii_case(key) && t.allows(&d.conditions))
        .map(|d| d.value.as_str())
        .collect()
}

/// The addons the game loads at startup, in load order, and every addon
/// it could load (by lower-case name).
fn startup_addons(interface: &Path, t: &Target) -> (Vec<Addon>, HashMap<String, Addon>) {
    let mut all: HashMap<String, Addon> = HashMap::new();
    let Ok(dirs) = wowlua_host::read_dir(interface.join("AddOns")) else {
        return (Vec::new(), HashMap::new());
    };
    for dir in dirs {
        let Some(name) = dir.file_name().and_then(|n| n.to_str()).map(str::to_string) else {
            continue;
        };
        // The client's suffixed TOC first (`Blizzard_WorldMap_Mainline.toc`).
        let text = t
            .suffixes
            .iter()
            .map(|s| format!("{name}{s}.toc"))
            .chain(std::iter::once(format!("{name}.toc")))
            .find_map(|f| wowlua_host::read_to_string(dir.join(f)).ok());
        let Some(text) = text else {
            continue;
        };
        let toc = wowlua_toc::parse(&text);
        let on = |v: &str| v.trim() == "1" || v.trim().eq_ignore_ascii_case("true");
        let on_demand = directive(&toc, t, "LoadOnDemand").iter().any(|v| on(v));
        // `AllowLoad` names where the addon loads (default: both).
        let allow: Vec<String> = toc
            .directives
            .iter()
            .filter(|d| d.key.eq_ignore_ascii_case("AllowLoad") && t.allows(&d.conditions))
            .map(|d| d.value.trim().to_ascii_lowercase())
            .collect();
        if !allow.is_empty() && !allow.iter().any(|a| a == "game" || a == "both") {
            continue;
        }
        let gated = |key: &str| {
            toc.directives
                .iter()
                .any(|d| d.key.eq_ignore_ascii_case(key))
        };
        let allowed_type = toc
            .directives
            .iter()
            .filter(|d| d.key.eq_ignore_ascii_case("AllowLoadGameType"))
            .all(|d| {
                d.value.split(',').any(|v| {
                    t.game_types
                        .iter()
                        .any(|g| g.eq_ignore_ascii_case(v.trim()))
                })
            });
        let excluded_type = toc
            .directives
            .iter()
            .filter(|d| d.key.eq_ignore_ascii_case("ExcludeLoadGameType"))
            .any(|d| {
                d.value.split(',').any(|v| {
                    t.game_types
                        .iter()
                        .any(|g| g.eq_ignore_ascii_case(v.trim()))
                })
            });
        if !allowed_type || excluded_type || (gated("OnlyBetaAndPTR") && !t.beta) {
            continue;
        }
        let mut deps = Vec::new();
        for key in [
            "Dep",
            "Dependencies",
            "RequiredDep",
            "RequiredDeps",
            "OptionalDep",
            "OptionalDeps",
        ] {
            for v in directive(&toc, t, key) {
                deps.extend(
                    v.split(',')
                        .map(|d| d.trim().to_string())
                        .filter(|d| !d.is_empty()),
                );
            }
        }
        let load_first = directive(&toc, t, "LoadFirst").iter().any(|v| on(v));
        let load_with = directive(&toc, t, "LoadWith")
            .iter()
            .flat_map(|v| v.split(','))
            .map(|d| d.trim().to_ascii_lowercase())
            .filter(|d| !d.is_empty())
            .collect();
        let secure = directive(&toc, t, "UseSecureEnvironment")
            .iter()
            .any(|v| on(v));
        all.insert(
            name.to_ascii_lowercase(),
            Addon {
                name,
                dir,
                toc,
                deps,
                load_first,
                load_with,
                on_demand,
                secure,
            },
        );
    }
    let mut names: Vec<String> = all.keys().cloned().collect();
    names.sort();
    let mut order: Vec<String> = Vec::new();
    let mut seen: HashSet<String> = HashSet::new();
    fn visit(
        n: &str,
        all: &HashMap<String, Addon>,
        names: &[String],
        seen: &mut HashSet<String>,
        order: &mut Vec<String>,
    ) {
        if !seen.insert(n.to_string()) {
            return;
        }
        if let Some(a) = all.get(n) {
            for d in &a.deps {
                visit(&d.to_ascii_lowercase(), all, names, seen, order);
            }
            order.push(n.to_string());
            // the startup addons that load with it, right after
            for w in names {
                if !all[w].on_demand && all[w].load_with.iter().any(|l| l == n) {
                    visit(w, all, names, seen, order);
                }
            }
        }
    }
    // The shared base first: LoadFirst addons need it without naming it
    // (Blizzard_CatalogShopSharedTemplates calls CreateAtlasMarkup and reads
    // SecondsFormatterConstants at load, Blizzard_Game EventUtil).
    for base in ["blizzard_sharedxmlbase", "blizzard_sharedxml"] {
        if all.contains_key(base) {
            visit(base, &all, &names, &mut seen, &mut order);
        }
    }
    // Then the secure environment's addons (Blizzard_UnitFrame builds
    // Blizzard_AuraContainer's aura containers at load, naming no
    // dependency on it).
    for n in &names {
        if all[n].secure && !all[n].on_demand {
            visit(n, &all, &names, &mut seen, &mut order);
        }
    }
    for first in [true, false] {
        for n in &names {
            if all[n].load_first == first && !all[n].on_demand {
                visit(n, &all, &names, &mut seen, &mut order);
            }
        }
    }
    let startup = order.iter().filter_map(|n| all.get(n).cloned()).collect();
    (startup, all)
}

/// A TOC line's file and whether it is marked `[Bootstrap]`: loaded at
/// startup for a load-on-demand addon (its `…_LoadUI` functions).
fn bootstrap(path: &str) -> (String, bool) {
    let trimmed = path.trim_end();
    match trimmed
        .to_ascii_lowercase()
        .strip_suffix("[bootstrap]")
        .map(str::len)
    {
        Some(len) => (trimmed[..len].trim_end().to_string(), true),
        None => (trimmed.to_string(), false),
    }
}

/// Loads an addon's files (all, but its bootstrap ones only if they have
/// not run; or only those), then fires `ADDON_LOADED` unless only
/// bootstrapping. A startup addon's bootstrap files run with the rest
/// (Blizzard_GuildControlUI's `GuildControlUI_LoadUI`).
fn load_addon_files(vm: &mut Vm, root: &Path, t: &Target, addon: &Addon, only_bootstrap: bool) {
    if addon
        .name
        .eq_ignore_ascii_case("Blizzard_EnvironmentCleanup")
    {
        keep_for_secure_code(vm, &addon.dir);
    }
    // A secure addon's code runs with the secure environment as its globals.
    let main_globals = vm.globals;
    if addon.secure {
        vm.globals = secure_env(vm);
    }
    // Blizzard's code and file names, also when an addon's file loading
    // calls `C_AddOns.LoadAddOn` (it would otherwise compile as that
    // addon's code, tainted, and be named inside its folder).
    let ui = &mut engine_mut(vm).world.ui;
    let code_of = ui.code_of.take();
    let chunk_prefix = ui.chunk_prefix.take();
    load_addon_files_in(vm, root, t, addon, only_bootstrap);
    let ui = &mut engine_mut(vm).world.ui;
    ui.code_of = code_of;
    ui.chunk_prefix = chunk_prefix;
    vm.globals = main_globals;
}

/// The globals of Blizzard's secure addons: their own table, reading
/// through to the main environment (unverified: the client may give them
/// separate copies of shared code), with the secure-only namespaces their
/// code calls (`C_UnitAurasPrivate`; functions without behaviour).
fn secure_env(vm: &mut Vm) -> wowlua_vm::TableRef {
    if let Some(env) = engine(vm).world.ui.secure_env {
        return env;
    }
    let main = vm.globals;
    let (env, meta) = (vm.new_table(), vm.new_table());
    let index = vm.names.index;
    vm.heap.table_mut(meta).set_str(index, Value::Table(main));
    vm.heap.table_mut(env).meta = Some(meta);
    let g = vm.heap.intern(b"_G");
    vm.heap.table_mut(env).set_str(g, Value::Table(env));
    // No private auras or anchors: the lists empty.
    fn empty_list(vm: &mut Vm, _c: wowlua_vm::NativeCall) -> wowlua_vm::stdlib::Ret {
        Ok(vec![Value::Table(vm.new_table())])
    }
    for (ns_name, functions) in SECURE_NAMESPACES {
        let ns = vm.new_table();
        for f in *functions {
            let name = vm.str(&format!("{ns_name}.{f}"));
            let behaviour: wowlua_vm::NativeFn =
                if f.starts_with("GetAll") || *f == "GetPrivateAuraAnchors" {
                    empty_list
                } else {
                    crate::undocumented_function
                };
            let native = vm.native_with(f, behaviour, vec![name]);
            let k = vm.heap.intern(f.as_bytes());
            vm.heap.table_mut(ns).set_str(k, native);
        }
        let k = vm.heap.intern(ns_name.as_bytes());
        vm.heap.table_mut(env).set_str(k, Value::Table(ns));
    }
    // The documented namespaces the addons' globals lack (`C_PingSecure`).
    for (k, v) in engine(vm).secure_namespaces.clone() {
        vm.heap.table_mut(env).set_str(k, v);
    }
    // `secretunwrap(...)`: the values themselves (the addons' globals lack
    // it, as recorded; Blizzard's auras key their frames by it).
    let unwrap = vm.native("secretunwrap", secret_unwrap);
    let k = vm.heap.intern(b"secretunwrap");
    vm.heap.table_mut(env).set_str(k, unwrap);
    // `GetForbiddenObjectTable(object)` (documented `SecureOnly`): an
    // object's forbidden partition. Partitions are not modelled, so an
    // object is its own (the aura containers' inbound checks compare the
    // two).
    let forbidden = vm.native("GetForbiddenObjectTable", forbidden_object_table);
    let k = vm.heap.intern(b"GetForbiddenObjectTable");
    vm.heap.table_mut(env).set_str(k, forbidden);
    crate::widgets::keep(vm, Value::Table(env));
    engine_mut(vm).world.ui.secure_env = Some(env);
    env
}

fn forbidden_object_table(vm: &mut Vm, c: wowlua_vm::NativeCall) -> wowlua_vm::stdlib::Ret {
    let object = c.arg(1);
    if crate::widgets::widget_of(vm, object).is_none() {
        return Err(vm.error("Usage: local forbiddenTable = GetForbiddenObjectTable(object)"));
    }
    Ok(vec![object])
}

/// `secretunwrap(...)`: its arguments, secrets revealed.
fn secret_unwrap(vm: &mut Vm, c: wowlua_vm::NativeCall) -> wowlua_vm::stdlib::Ret {
    Ok(c.args.iter().map(|&v| vm.hidden(v)).collect())
}

/// What Blizzard_EnvironmentCleanup removes from the addons' globals
/// (`X = nil;`, `Enum.X = nil;` in its files) the secure environment keeps:
/// secure code goes on using `CreateSecureDelegate`, `Enum.StoreError`.
fn keep_for_secure_code(vm: &mut Vm, dir: &Path) {
    let env = secure_env(vm);
    let main = engine(vm).main_globals;
    let mut files = Vec::new();
    lua_files_in(dir, &mut files);
    for file in files {
        let Ok(text) = wowlua_host::read_to_string(&file) else {
            continue;
        };
        for line in text.lines() {
            let Some(name) = line.trim().strip_suffix("= nil;").map(str::trim) else {
                continue;
            };
            match name.split_once('.') {
                None => {
                    let k = vm.heap.intern(name.as_bytes());
                    let v = vm.heap.table(main).get_str(k);
                    if !v.is_nil() {
                        vm.heap.table_mut(env).set_str(k, v);
                    }
                }
                Some(("Enum", field)) => {
                    let enum_key = vm.heap.intern(b"Enum");
                    let Value::Table(main_enum) = vm.heap.table(main).get_str(enum_key) else {
                        continue;
                    };
                    // The secure code's own Enum, reading through.
                    let secure_enum = match vm.heap.table(env).get_str(enum_key) {
                        Value::Table(t) => t,
                        _ => {
                            let (t, meta) = (vm.new_table(), vm.new_table());
                            let index = vm.names.index;
                            vm.heap
                                .table_mut(meta)
                                .set_str(index, Value::Table(main_enum));
                            vm.heap.table_mut(t).meta = Some(meta);
                            vm.heap.table_mut(env).set_str(enum_key, Value::Table(t));
                            t
                        }
                    };
                    let k = vm.heap.intern(field.as_bytes());
                    let v = vm.heap.table(main_enum).get_str(k);
                    if !v.is_nil() {
                        vm.heap.table_mut(secure_enum).set_str(k, v);
                    }
                }
                _ => {}
            }
        }
    }
}

fn lua_files_in(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = wowlua_host::read_dir(dir) else {
        return;
    };
    for p in entries {
        if wowlua_host::is_dir(&p) {
            lua_files_in(&p, out);
        } else if p.extension().is_some_and(|x| x.eq_ignore_ascii_case("lua")) {
            out.push(p);
        }
    }
}

/// The secure code's undocumented namespaces: the functions Blizzard's
/// code calls of them (a scan of wow-ui-source 1.60.1 (70425)).
const SECURE_NAMESPACES: &[(&str, &[&str])] = &[
    (
        "C_AuthChallenge",
        &[
            "Cancel",
            "DidChallengeSucceed",
            "OnTabPressed",
            "SetFrame",
            "Submit",
        ],
    ),
    (
        "C_StoreSecure",
        &[
            "AckFailure",
            "ClearPreGeneratedExternalTransactionID",
            "GetBnetTransferInfo",
            "GetCharacterInfoByGUID",
            "GetCharactersForRealm",
            "GetConfirmationInfo",
            "GetCurrencyID",
            "GetCurrencyInfo",
            "GetEligibleRacesForVASService",
            "GetEntryInfo",
            "GetFailureInfo",
            "GetLastProductListResponseError",
            "GetProductGroupInfo",
            "GetProductGroups",
            "GetProductInfo",
            "GetProductList",
            "GetProducts",
            "GetPurchaseList",
            "GetRealmList",
            "GetUnrevokedBoostInfo",
            "GetVASCompletionInfo",
            "GetVASErrors",
            "GetVASGuildMasterInfoForCharacterByGUID",
            "GetVASRealmList",
            "GetVasServiceType",
            "GetWoWAccountGUIDFromName",
            "HasDistributionList",
            "HasDynamicPriceData",
            "HasProductList",
            "HasPurchaseInProgress",
            "HasPurchaseList",
            "IsAvailable",
            "IsDynamicBundle",
            "IsRegionLocked",
            "OpenNydusLink",
            "PurchaseProduct",
            "PurchaseProductConfirm",
            "PurchaseVASProduct",
            "RequestAllDynamicPriceInfo",
            "RequestCharacterGuildFollowInfo",
            "RequestRealmGuildMasterInfo",
            "SetDisconnectOnLogout",
            "SetVASProductReady",
            "ValidateBnetTransfer",
        ],
    ),
    (
        "C_UnitAurasPrivate",
        &[
            "AddPrivateAuraUpdateCallback",
            "AnchorPrivateAura",
            "GetAllPrivateAuraInstanceIDs",
            "GetAllPrivateAuras",
            "GetAuraDataByAuraInstanceIDPrivate",
            "GetPrivateAuraAnchors",
            "GetPrivateAuraCasterGUID",
            "IsPrivateAuraFilteredOutByInstanceID",
            "RemovePrivateAuraUpdateCallback",
            "SetPrivateAuraAnchorAddedCallback",
            "SetPrivateAuraAnchorRemovedCallback",
            "SetPrivateRaidBossMessageCallback",
            "SetPrivateWarningTextFrame",
        ],
    ),
    (
        "C_WowTokenSecure",
        &[
            "CanRedeemForBalance",
            "CancelRedeem",
            "ConfirmBuyToken",
            "ConfirmSellToken",
            "GetBalanceRedeemAmount",
            "GetBalanceRedemptionInfo",
            "GetGameTimeRedemptionInfo",
            "GetPriceLockDuration",
            "GetRemainingGameTime",
            "GetTokenCount",
            "IsRedemptionStillValid",
            "RedeemToken",
            "RedeemTokenConfirm",
            "SetBalanceAmountString",
            "WillKickFromWorld",
        ],
    ),
];

fn load_addon_files_in(vm: &mut Vm, root: &Path, t: &Target, addon: &Addon, only_bootstrap: bool) {
    let table = Value::Table(vm.new_table());
    let key = addon.name.to_ascii_lowercase();
    let bootstrapped = !engine_mut(vm).world.ui.bootstrapped.insert(key);
    for file in &addon.toc.files {
        if !t.allows(&file.conditions) {
            continue;
        }
        let (rel, marked) = bootstrap(&file.path);
        let is_bootstrap = marked
            || file
                .conditions
                .iter()
                .any(|c| c.name.eq_ignore_ascii_case("Bootstrap"));
        if (only_bootstrap && !is_bootstrap) || (is_bootstrap && bootstrapped) {
            continue;
        }
        let path = resolve(&addon.dir, &t.path(&rel));
        // `[LoadIntoEnvironment global]`: a secure addon's file for the
        // addons' globals (Blizzard_AuraContainerInbound.lua); `secure`: an
        // addon's file for the secure ones (TargetFrameAuraContainer.lua).
        let into = |env: &str| {
            file.conditions.iter().any(|c| {
                c.name.eq_ignore_ascii_case("LoadIntoEnvironment")
                    && c.values.iter().any(|v| v.eq_ignore_ascii_case(env))
            })
        };
        let globals = vm.globals;
        if into("global") {
            vm.globals = engine(vm).main_globals;
        } else if into("secure") {
            vm.globals = secure_env(vm);
        }
        load_file(vm, root, &path, &addon.name, table);
        vm.globals = globals;
    }
    if only_bootstrap {
        return;
    }
    engine_mut(vm).world.ui.loaded.push(addon.name.clone());
    let name = vm.str(&addon.name);
    crate::widgets::fire_event(vm, "ADDON_LOADED", &[name, Value::Bool(false)]);
}

/// `C_AddOns.LoadAddOn(name)` for Blizzard's addons: their dependencies,
/// then their files. `Some(true)` once loaded (or if it was), `Some(false)`
/// for an addon the code does not have, `None` without the UI code.
pub fn load_addon(vm: &mut Vm, name: &str) -> Option<bool> {
    let root = engine(vm).world.ui.root.clone()?;
    let key = name.to_ascii_lowercase();
    if engine(vm)
        .world
        .ui
        .loaded
        .iter()
        .any(|l| l.eq_ignore_ascii_case(name))
    {
        return Some(true);
    }
    let Some(addon) = engine(vm).world.ui.available.get(&key).cloned() else {
        return Some(false);
    };
    for dep in &addon.deps {
        if engine(vm)
            .world
            .ui
            .available
            .contains_key(&dep.to_ascii_lowercase())
        {
            load_addon(vm, dep);
        }
    }
    let t = Target::new(&engine(vm).flavor.clone());
    load_addon_files(vm, &root, &t, &addon, false);
    Some(true)
}

/// An addon folder a test loads besides the addon under test (the
/// manifest's `test.addons`), as the client reads it for the flavor.
#[derive(Clone, Debug)]
pub struct UserAddon {
    /// The folder's name, the addon's.
    pub name: String,
    pub dir: PathBuf,
    /// The TOC the client reads: the flavor's own (`<Name>_Mainline.toc`),
    /// else `<Name>.toc`.
    pub toc: PathBuf,
    /// The TOC's metadata (`## Title: …`), its inline conditions met.
    pub metadata: Vec<(String, String)>,
    /// `Dependencies`, `RequiredDeps` (and their short forms).
    pub deps: Vec<String>,
    /// `LoadOnDemand: 1`: known, loaded by `C_AddOns.LoadAddOn`.
    pub on_demand: bool,
    /// `SavedVariables` and `SavedVariablesPerCharacter`.
    pub saved: Vec<String>,
    /// The client loads it in this game (`AllowLoadGameType`,
    /// `ExcludeLoadGameType`, `AllowLoad`).
    pub allowed: bool,
}

/// Reads an addon folder as the client does for `flavor`: the flavor's
/// TOC (the suffixes the Blizzard addons' are read with), else the plain
/// one. An error if the folder has neither.
pub fn read_user_addon(flavor: &str, dir: &Path) -> Result<UserAddon, String> {
    let t = Target::new(flavor);
    let name = dir
        .file_name()
        .and_then(|n| n.to_str())
        .map(str::to_string)
        .ok_or_else(|| format!("{}: not an addon folder", dir.display()))?;
    let toc = t
        .suffixes
        .iter()
        .map(|s| format!("{name}{s}.toc"))
        .chain(std::iter::once(format!("{name}.toc")))
        .map(|f| resolve(dir, &f))
        .find(|p| wowlua_host::is_file(p))
        .ok_or_else(|| format!("{}: no {name}.toc for {flavor}", dir.display()))?;
    read_user_toc(flavor, &name, dir, &toc)
}

/// An addon named `name` in `dir` as its TOC `toc` describes it for
/// `flavor` (see [`read_user_addon`]).
pub fn read_user_toc(
    flavor: &str,
    name: &str,
    dir: &Path,
    toc: &Path,
) -> Result<UserAddon, String> {
    let t = Target::new(flavor);
    let name = name.to_string();
    let toc = toc.to_path_buf();
    let text = wowlua_host::read_to_string(&toc).map_err(|e| format!("{}: {e}", toc.display()))?;
    let parsed = wowlua_toc::parse(&text);
    let on = |v: &str| v.trim() == "1" || v.trim().eq_ignore_ascii_case("true");
    let list = |keys: &[&str]| -> Vec<String> {
        keys.iter()
            .flat_map(|k| directive(&parsed, &t, k))
            .flat_map(|v| v.split(','))
            .map(|n| n.trim().to_string())
            .filter(|n| !n.is_empty())
            .collect()
    };
    let in_game = |key: &str| {
        parsed
            .directives
            .iter()
            .filter(|d| d.key.eq_ignore_ascii_case(key))
            .map(|d| {
                d.value.split(',').any(|v| {
                    t.game_types
                        .iter()
                        .any(|g| g.eq_ignore_ascii_case(v.trim()))
                })
            })
            .collect::<Vec<bool>>()
    };
    let allow_load: Vec<String> = directive(&parsed, &t, "AllowLoad")
        .iter()
        .map(|v| v.trim().to_ascii_lowercase())
        .collect();
    let allowed = in_game("AllowLoadGameType").iter().all(|b| *b)
        && !in_game("ExcludeLoadGameType").iter().any(|b| *b)
        && (allow_load.is_empty() || allow_load.iter().any(|a| a == "game" || a == "both"));
    let metadata = parsed
        .directives
        .iter()
        .filter(|d| t.allows(&d.conditions))
        .map(|d| (d.key.clone(), d.value.trim().to_string()))
        .collect();
    Ok(UserAddon {
        deps: list(&["Dep", "Dependencies", "RequiredDep", "RequiredDeps"]),
        on_demand: directive(&parsed, &t, "LoadOnDemand").iter().any(|v| on(v)),
        saved: list(&["SavedVariables", "SavedVariablesPerCharacter"]),
        allowed,
        metadata,
        name,
        dir: dir.to_path_buf(),
        toc,
    })
}

/// The client's load order of addons: as given, each after those of its
/// dependencies the list has (a cycle's first stays first).
pub fn user_addon_order(addons: Vec<UserAddon>) -> Vec<UserAddon> {
    fn visit(i: usize, all: &[UserAddon], seen: &mut Vec<bool>, order: &mut Vec<usize>) {
        if seen[i] {
            return;
        }
        seen[i] = true;
        for d in &all[i].deps {
            if let Some(j) = all.iter().position(|a| a.name.eq_ignore_ascii_case(d)) {
                visit(j, all, seen, order);
            }
        }
        order.push(i);
    }
    let mut seen = vec![false; addons.len()];
    let mut order = Vec::new();
    for i in 0..addons.len() {
        visit(i, &addons, &mut seen, &mut order);
    }
    order.into_iter().map(|i| addons[i].clone()).collect()
}

/// Whether the loaded UI code has a Blizzard addon of this name (loaded or
/// loadable).
pub fn is_blizzard_addon(vm: &Vm, name: &str) -> bool {
    engine(vm)
        .world
        .ui
        .available
        .contains_key(&name.to_ascii_lowercase())
}

/// A Blizzard addon of the loaded UI code as `C_AddOns` tells it: its
/// name, its TOC's title, whether it loads on demand, its required
/// dependencies (in the TOC's order).
pub struct BlizzardAddon {
    pub name: String,
    pub title: Option<String>,
    pub on_demand: bool,
    pub deps: Vec<String>,
}

/// The loaded UI code's Blizzard addon of this name, any case.
pub fn blizzard_addon(vm: &Vm, name: &str) -> Option<BlizzardAddon> {
    let a = engine(vm)
        .world
        .ui
        .available
        .get(&name.to_ascii_lowercase())?;
    let title = a
        .toc
        .directives
        .iter()
        .find(|d| d.key.eq_ignore_ascii_case("Title"))
        .map(|d| d.value.clone());
    Some(BlizzardAddon {
        name: a.name.clone(),
        title,
        on_demand: a.on_demand,
        deps: a.deps.clone(),
    })
}

/// Loads an addon of the user's from its folder as the client does: the
/// files its TOC lists, in order, as the addon's code (its taint), with
/// `table` as the addon table; `skip`: files (relative to `dir`) already
/// loaded. Errors go to [`Registry::errors`]; `ADDON_LOADED` is not fired.
pub fn load_user_addon(
    vm: &mut Vm,
    dir: &Path,
    toc: &Path,
    name: &str,
    table: Value,
    skip: &HashSet<String>,
) {
    let text = match wowlua_host::read_to_string(toc) {
        Ok(t) => t,
        Err(e) => {
            engine_mut(vm)
                .world
                .ui
                .errors
                .push(format!("{}: {e}", toc.display()));
            return;
        }
    };
    let parsed = wowlua_toc::parse(&text);
    let t = Target::new(&engine(vm).flavor.clone());
    let saved = engine_mut(vm).world.ui.code_of.replace(name.to_string());
    for file in &parsed.files {
        if !t.allows(&file.conditions) {
            continue;
        }
        let rel = t.path(&file.path);
        if skip.contains(&rel) {
            continue;
        }
        let path = resolve(dir, &rel);
        load_file(vm, dir, &path, name, table);
    }
    engine_mut(vm).world.ui.code_of = saved;
}

/// Another addon of the user's (one of the `test.addons`): as
/// [`load_user_addon`], its files named as the client names them
/// (`Interface/AddOns/<name>/<file>`).
pub fn load_other_user_addon(vm: &mut Vm, dir: &Path, toc: &Path, name: &str, table: Value) {
    let saved = engine_mut(vm)
        .world
        .ui
        .chunk_prefix
        .replace(format!("Interface/AddOns/{name}/"));
    load_user_addon(vm, dir, toc, name, table, &HashSet::new());
    engine_mut(vm).world.ui.chunk_prefix = saved;
}

/// One file of an addon of the user's (`rel`: relative to `dir`), as its
/// code: see [`load_user_addon`].
pub fn load_user_file(vm: &mut Vm, dir: &Path, rel: &str, name: &str, table: Value) {
    let saved = engine_mut(vm).world.ui.code_of.replace(name.to_string());
    let path = resolve(dir, rel);
    load_file(vm, dir, &path, name, table);
    engine_mut(vm).world.ui.code_of = saved;
}

/// Loads one UI file (`.lua` or `.xml`) of an addon.
fn load_file(vm: &mut Vm, interface_root: &Path, path: &Path, addon: &str, addon_table: Value) {
    // A session ended (a debugger's terminate): nothing more loads.
    if vm.aborted() {
        return;
    }
    let shown = path
        .strip_prefix(interface_root)
        .unwrap_or(path)
        .to_string_lossy()
        .replace('\\', "/");
    let shown = match &engine(vm).world.ui.chunk_prefix {
        Some(prefix) => format!("{prefix}{shown}"),
        None => shown,
    };
    let ext = path
        .extension()
        .and_then(|e| e.to_str())
        .map(str::to_ascii_lowercase);
    match ext.as_deref() {
        Some("lua") => build::run_lua(vm, path, &shown, addon, addon_table),
        Some("xml") => {
            let text = match wowlua_host::read_to_string(path) {
                Ok(t) => t,
                Err(e) => {
                    engine_mut(vm).world.ui.errors.push(format!("{shown}: {e}"));
                    return;
                }
            };
            let root = match xml::parse(&text) {
                Ok(r) => r,
                Err(e) => {
                    engine_mut(vm).world.ui.errors.push(format!("{shown}: {e}"));
                    return;
                }
            };
            let dir = path.parent().unwrap_or(Path::new("")).to_path_buf();
            let cx = build::Context { file: &shown };
            process_ui(vm, interface_root, &dir, &cx, &root, addon, addon_table);
        }
        _ => {}
    }
}

/// A file a `<Script>` or `<Include>` names, relative to the XML file's
/// directory (any case, as on Windows).
fn resolve(dir: &Path, file: &str) -> PathBuf {
    let rel = wowlua_toc::split_conditions(file).0.replace('\\', "/");
    let direct = dir.join(&rel);
    if wowlua_host::exists(&direct) {
        return direct;
    }
    // Case-insensitive lookup, one component at a time.
    let mut cur = dir.to_path_buf();
    for part in rel.split('/') {
        let next = wowlua_host::read_dir(&cur).ok().and_then(|entries| {
            entries.into_iter().find(|p| {
                p.file_name()
                    .and_then(|n| n.to_str())
                    .is_some_and(|n| n.eq_ignore_ascii_case(part))
            })
        });
        match next {
            Some(p) => cur = p,
            None => return direct,
        }
    }
    cur
}

/// A `<Ui>` element's children, in order.
fn process_ui(
    vm: &mut Vm,
    interface_root: &Path,
    dir: &Path,
    cx: &build::Context,
    root: &Rc<Element>,
    addon: &str,
    addon_table: Value,
) {
    for el in &root.children {
        match el.tag.to_ascii_lowercase().as_str() {
            "script" => {
                if let Some(file) = el.attr("file") {
                    let t = Target::new(&engine(vm).flavor);
                    if t.allows(&wowlua_toc::split_conditions(file).1) {
                        load_file(vm, interface_root, &resolve(dir, file), addon, addon_table);
                    }
                } else if !el.text.trim().is_empty() {
                    let src = format!("{}{}", "\n".repeat(el.line.saturating_sub(1)), el.text);
                    match build::load_chunk(vm, src.as_bytes(), &format!("@{}", cx.file)) {
                        Ok(f) => {
                            if let Err(e) = vm.call_secure(f, &[]) {
                                let msg = match e.value {
                                    Value::Str(s) => {
                                        String::from_utf8_lossy(vm.str_bytes(s)).into_owned()
                                    }
                                    _ => "?".into(),
                                };
                                engine_mut(vm).world.ui.errors.push(msg);
                            }
                        }
                        Err(e) => engine_mut(vm).world.ui.errors.push(e),
                    }
                }
            }
            "include" => {
                if let Some(file) = el.attr("file") {
                    let t = Target::new(&engine(vm).flavor);
                    if t.allows(&wowlua_toc::split_conditions(file).1) {
                        load_file(vm, interface_root, &resolve(dir, file), addon, addon_table);
                    }
                }
            }
            "font" => font(vm, el),
            "fontfamily" => {
                // The family's first member's font stands for it.
                if let Some(name) = el.attr("name")
                    && let Some(f) = el.child("Member").and_then(|m| m.child("Font"))
                {
                    font_named(vm, name, f);
                }
            }
            "scopedmodifier" => {
                // `allowUntaintedCreation` (Blizzard_AuraContainer's
                // intrinsic and its custom container template): what is
                // declared within is built untainted (see
                // `widgets::create_frame`). Blizzard's code only (an
                // addon's XML would make its own code secure; unverified).
                let ui = &mut engine_mut(vm).world.ui;
                let outer = ui.in_untainted_creation;
                let outer_forbidden = ui.in_forbidden;
                ui.in_untainted_creation |=
                    el.flag("allowUntaintedCreation") && ui.code_of.is_none();
                ui.in_forbidden |= el.flag("forbidden") || el.flag("fullLockdown");
                process_ui(vm, interface_root, dir, cx, el, addon, addon_table);
                let ui = &mut engine_mut(vm).world.ui;
                ui.in_untainted_creation = outer;
                ui.in_forbidden = outer_forbidden;
            }
            _ => {
                if (el.flag("virtual") || el.flag("intrinsic"))
                    && let Some(name) = el.attr("name")
                {
                    let globals = vm.globals;
                    let ui = &mut engine_mut(vm).world.ui;
                    if ui.in_untainted_creation {
                        ui.untainted_creation.insert(name.to_ascii_lowercase());
                    }
                    ui.template_files
                        .insert(name.to_ascii_lowercase(), cx.file.to_string());
                    ui.template_envs.insert(name.to_ascii_lowercase(), globals);
                    if let Some(addon) = ui.code_of.clone() {
                        ui.template_addons.insert(name.to_ascii_lowercase(), addon);
                    }
                }
                build::top_level(vm, cx, el);
            }
        }
    }
}

/// `<Font name=… inherits=… font=… height=… outline=…>`: a font object.
fn font(vm: &mut Vm, el: &Rc<Element>) {
    if let Some(name) = el.attr("name") {
        font_named(vm, name, el);
    }
}

fn font_named(vm: &mut Vm, name: &str, el: &Element) {
    crate::engine_mut(vm).world.scene_epoch += 1;
    let f = crate::widgets::create(vm, "Font", Some(name.to_string()), None);
    let call = |vm: &mut Vm, method: &str, args: &[Value]| {
        let key = vm.str(method);
        if let Ok(m) = vm.index(f, key, None)
            && !m.is_nil()
        {
            let mut a = vec![f];
            a.extend_from_slice(args);
            // A font the engine cannot build is the loader's error.
            if let Err(e) = vm.call(m, &a) {
                let msg = match e.value {
                    Value::Str(s) => String::from_utf8_lossy(vm.str_bytes(s)).into_owned(),
                    _ => "?".into(),
                };
                engine_mut(vm)
                    .world
                    .ui
                    .errors
                    .push(format!("{name} {method}: {msg}"));
            }
        }
    };
    // What the font inherits: the parent font's face, color, shadow and
    // whether the user's text scale may scale it.
    if let Some(parent) = el.attr("inherits") {
        let p = vm.get_global(parent);
        if !p.is_nil() {
            call(vm, "SetFontObject", &[p]);
            if let (Some(pid), Some(id)) = (
                crate::widgets::widget_of(vm, p),
                crate::widgets::widget_of(vm, f),
            ) {
                crate::widgets::inherit_font_style(vm, pid, id);
            }
        }
    }
    if let Some(v) = el.attr("canBeUserScaled")
        && let Some(id) = crate::widgets::widget_of(vm, f)
    {
        let props = engine(vm).world.widgets[id.0].props;
        let k = vm.str("canBeUserScaled");
        let _ = vm
            .heap
            .table_mut(props)
            .set(k, Value::Bool(v.eq_ignore_ascii_case("true")));
    }
    if let Some(file) = el.attr("font") {
        let height = el
            .number("height")
            .or_else(|| {
                el.child("FontHeight")
                    .and_then(|h| h.child("AbsValue"))
                    .and_then(|v| v.number("val"))
            })
            .unwrap_or(12.0);
        let file = vm.str(file);
        let flags = vm.str(&xml_font_flags(el));
        call(vm, "SetFont", &[file, Value::Num(height), flags]);
    }
    // The XML's colors, in eight bits each as the client keeps them.
    if let Some(c) = el.child("Color") {
        let rgba = xml_color(vm, c);
        call(vm, "SetTextColor", &rgba);
    }
    if let Some(shadow) = el.child("Shadow") {
        // `<Shadow x y>` or `<Shadow><Offset x y/>` (or its AbsDimension).
        let offset = shadow.child("Offset").map(|o| {
            let d = o.child("AbsDimension").map(|d| d.as_ref()).unwrap_or(o);
            (d.number("x").unwrap_or(0.0), d.number("y").unwrap_or(0.0))
        });
        let (x, y) = offset.unwrap_or((
            shadow.number("x").unwrap_or(0.0),
            shadow.number("y").unwrap_or(0.0),
        ));
        call(vm, "SetShadowOffset", &[Value::Num(x), Value::Num(y)]);
        let color = match shadow.child("Color") {
            Some(c) => xml_color(vm, c),
            None => [
                Value::Num(0.0),
                Value::Num(0.0),
                Value::Num(0.0),
                Value::Num(1.0),
            ],
        };
        call(vm, "SetShadowColor", &color);
    }
    // Its justification and spacing (`QuestFontLeft`'s `justifyH="LEFT"`:
    // the gossip window's options were centered without it).
    for (attr, method) in [("justifyH", "SetJustifyH"), ("justifyV", "SetJustifyV")] {
        if let Some(v) = el.attr(attr) {
            let v = vm.str(v);
            call(vm, method, &[v]);
        }
    }
    if let Some(n) = el.number("spacing") {
        call(vm, "SetSpacing", &[Value::Num(n)]);
    }
}

/// An XML font's flags as the client reports them (`GetFont`,
/// `GetFontInfo`: `/stdtest engine answers`): `outline="NORMAL"` is
/// OUTLINE, `"THICK"` OUTLINE and THICKOUTLINE; then SLUG, FILTER and
/// FIXEDHEIGHT (`fixedSize`), joined with ", " (MONOCHROME's place
/// unverified).
pub(crate) fn xml_font_flags(el: &Element) -> String {
    let mut flags = Vec::new();
    match el.attr("outline").map(str::to_ascii_uppercase).as_deref() {
        Some("NORMAL") => flags.push("OUTLINE"),
        Some("THICK") => flags.extend(["OUTLINE", "THICKOUTLINE"]),
        _ => {}
    }
    if el.flag("monochrome") {
        flags.push("MONOCHROME");
    }
    if el.flag("slug") {
        flags.push("SLUG");
    }
    if el.flag("filter") {
        flags.push("FILTER");
    }
    if el.flag("fixedSize") {
        flags.push("FIXEDHEIGHT");
    }
    flags.join(", ")
}

/// An XML `<Color r g b a/>` (or `color="WHITE_FONT_COLOR"`, a color
/// object's): r, g, b and a (1 when not given), each in eight bits.
pub(crate) fn xml_color(vm: &mut Vm, el: &Element) -> [Value; 4] {
    let mut c = [
        el.number("r").unwrap_or(0.0),
        el.number("g").unwrap_or(0.0),
        el.number("b").unwrap_or(0.0),
        el.number("a").unwrap_or(1.0),
    ];
    if let Some(name) = el.attr("color")
        && let Value::Table(t) = vm.get_global(name)
    {
        for (i, k) in ["r", "g", "b", "a"].iter().enumerate() {
            let key = vm.str(k);
            if let Value::Num(n) = vm.heap.table(t).get(key) {
                c[i] = n;
            }
        }
    }
    c.map(|v| Value::Num(crate::widgets::byte_float(vm, v)))
}

/// The client's global strings: the file's `NAME = "text";` lines (a line
/// whose name is not an identifier is skipped).
fn load_global_strings(vm: &mut Vm, path: &Path) {
    let text = match wowlua_host::read_to_string(path) {
        Ok(t) => t,
        Err(e) => {
            engine_mut(vm)
                .world
                .ui
                .errors
                .push(format!("GlobalStrings.lua: {e}"));
            return;
        }
    };
    let ident = |s: &str| {
        let mut c = s.chars();
        c.next()
            .is_some_and(|f| f.is_ascii_alphabetic() || f == '_')
            && c.all(|ch| ch.is_ascii_alphanumeric() || ch == '_')
    };
    let src: String = text
        .lines()
        .filter(|l| l.split_once(" = ").is_some_and(|(name, _)| ident(name)))
        .flat_map(|l| [l, "\n"])
        .collect();
    match vm.load(src.as_bytes(), "@GlobalStrings.lua") {
        Ok(f) => {
            if let Err(e) = vm.call(f, &[]) {
                let msg = match e.value {
                    Value::Str(s) => String::from_utf8_lossy(vm.str_bytes(s)).into_owned(),
                    _ => "?".into(),
                };
                engine_mut(vm).world.ui.errors.push(msg);
            }
        }
        Err(e) => engine_mut(vm).world.ui.errors.push(e),
    }
}

/// Loads the flavor's Blizzard addons from `interface` (a folder holding
/// `AddOns/Blizzard_*`), as secure code, after the client's global strings
/// (`GlobalStrings.lua`: `NAME = "text"` lines) if given; `ADDON_LOADED`
/// fires for each addon.
/// `CreateSecureDelegate(f)`: a C function calling `f` (Blizzard's code
/// only, while it loads).
fn create_secure_delegate(vm: &mut Vm, c: wowlua_vm::NativeCall) -> wowlua_vm::stdlib::Ret {
    let f = c.arg(1);
    if !matches!(f, Value::Func(_)) {
        return Err(vm.error("Usage: CreateSecureDelegate(func)"));
    }
    Ok(vec![secure_delegate(vm, f, false)])
}

/// A secure delegate of `f`: as the client documents `CreateSecureDelegate`,
/// it "invokes the original function in a protected call context with
/// secure execution taint, and supports passing and returning values
/// directly". The arguments go in as values of that secure execution (were
/// they to keep a tainted caller's taint, the first one read would taint
/// it again), and the caller's taint is back after, the results taking it.
/// A mixin's delegates (`<Mixin secureDelegates="true">`) stay at the
/// caller's taint when a tainted caller passes a secret: Blizzard's
/// ScriptErrorsFrame says secure mixin methods "drop their ability to
/// elevate to an untainted state if any input parameter is secret from a
/// tainted context", unlike a delegate of its own. Unverified: errors
/// reach the caller (Blizzard's inbound methods validate with `assert`).
pub fn secure_delegate(vm: &mut Vm, f: Value, mixin: bool) -> Value {
    // The delegate is Blizzard's, whoever's frame needs it made.
    let saved = vm.taint().saved();
    vm.taint_mut().stack = wowlua_vm::Taint::SECURE;
    let d = vm.native_with("delegate", call_delegate, vec![f, Value::Bool(mixin)]);
    vm.taint_mut().restore(saved);
    d
}

fn call_delegate(vm: &mut Vm, c: wowlua_vm::NativeCall) -> wowlua_vm::stdlib::Ret {
    let f = c.upvals.first().copied().unwrap_or(Value::Nil);
    let mixin = c.upvals.get(1).copied().is_some_and(Value::truthy);
    let tainted = !vm.taint().stack.is_secure();
    if mixin && tainted && c.args.iter().any(|v| matches!(v, Value::Secret(_))) {
        return vm.call(f, &c.args);
    }
    let saved = vm.taint().saved();
    vm.taint_mut().stack = wowlua_vm::Taint::SECURE;
    let args: Vec<wowlua_vm::Slot> = c.args.iter().map(|&v| wowlua_vm::Slot::secure(v)).collect();
    let r = vm.call_slots(wowlua_vm::Slot::secure(f), &args);
    vm.taint_mut().restore(saved);
    let results = r?;
    Ok(results.into_iter().map(|s| s.v).collect())
}

/// What starts an error the client dropped without a report in the
/// engine's list (`secureexecuterange`'s callbacks: the client runs on
/// silently, but code that failed is a failure all the same).
pub const DROPPED: &str = "secureexecuterange dropped: ";

/// `Vm::on_dropped_error`: the error in the engine's list, marked
/// (`DROPPED`), with its stack.
pub fn dropped_error(vm: &mut Vm, message: String, stack: String) {
    let ui = &mut engine_mut(vm).world.ui;
    ui.errors.push(format!("{DROPPED}{message}"));
    ui.error_stacks.push(format!("{DROPPED}{message}\n{stack}"));
}

pub fn load(vm: &mut Vm, interface: &Path, global_strings: Option<&Path>) -> Report {
    let flavor = engine(vm).flavor.clone();
    let t = Target::new(&flavor);
    let root = interface.parent().unwrap_or(interface).to_path_buf();
    // Errors of the code's handlers while it loads are the report's too.
    let saved_handler = vm.error_handler;
    let recorder = vm.native("errorhandler", |vm, c| {
        let msg = match c.arg(1) {
            Value::Str(s) => String::from_utf8_lossy(vm.str_bytes(s)).into_owned(),
            _ => "UNKNOWN ERROR".into(),
        };
        engine_mut(vm).world.ui.errors.push(msg);
        Ok(vec![])
    });
    vm.error_handler = recorder;
    // The VM's stand-in for what RestrictedExecution.lua does to the string
    // metatable (`__metatable = string`) gives way to the real code.
    if let Some(meta) = vm.string_meta {
        let k = vm.heap.intern(b"__metatable");
        vm.heap.table_mut(meta).set_str(k, Value::Nil);
    }
    if let Some(path) = global_strings {
        load_global_strings(vm, path);
    }
    // What only Blizzard's code sees; its EnvironmentCleanup.lua removes it.
    for s in engine(vm).secure_tables.clone() {
        vm.heap.table_mut(s.root).set_str(s.name, s.table);
    }
    let delegate = vm.native("CreateSecureDelegate", create_secure_delegate);
    vm.set_global("CreateSecureDelegate", delegate);
    let (startup, all) = startup_addons(interface, &t);
    {
        let ui = &mut engine_mut(vm).world.ui;
        ui.available = all.clone();
        ui.root = Some(root.clone());
    }
    for addon in &startup {
        load_addon_files(vm, &root, &t, addon, false);
    }
    // The load-on-demand addons' bootstrap files (order unverified: after
    // the startup addons, by name).
    let mut rest: Vec<&Addon> = all
        .values()
        .filter(|a| !startup.iter().any(|s| s.name == a.name))
        .collect();
    rest.sort_by(|a, b| a.name.cmp(&b.name));
    for addon in rest {
        load_addon_files(vm, &root, &t, addon, true);
    }
    // Blizzard's key bindings, which the client reads at startup: the
    // game's (`Bindings_Camelot.xml`), then each addon's `Bindings.xml`
    // (by name; order unverified), Blizzard's code (secure).
    let mut binding_files = vec![
        interface
            .join("AddOns/Blizzard_FrameXML")
            .join(format!("Bindings_{}.xml", t.game)),
    ];
    let mut names: Vec<&String> = all.keys().collect();
    names.sort();
    binding_files.extend(names.into_iter().map(|n| all[n].dir.join("Bindings.xml")));
    for path in binding_files.into_iter().filter(|p| wowlua_host::exists(p)) {
        if let Err(e) = crate::bindings::load_bindings_xml(vm, &path, None) {
            engine_mut(vm).world.ui.errors.push(e);
        }
    }
    // Blizzard's code may have set its own handler meanwhile.
    if let (Value::Func(a), Value::Func(b)) = (vm.error_handler, recorder)
        && a == b
    {
        vm.error_handler = saved_handler;
    }
    let ui = &engine(vm).world.ui;
    Report {
        addons: ui.loaded.clone(),
        errors: ui.errors.clone(),
    }
}
