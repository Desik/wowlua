//! `wowlua test` (spec Tooling §16.1): a test build's test files run in the
//! engine, each file in a fresh VM, in parallel. A file's VM loads the
//! runtime, the project mocks, the whole addon in its TOC's order, then the
//! test file with its helpers (§16.1.2); its tests run as `wowlua:test`
//! registered them, each in a coroutine the runner steps, advancing the
//! game's frames while a test waits.

pub mod bench;
mod blizzard;
#[cfg(feature = "camera")]
pub mod camera;
pub mod coverage;
/// Blizzard's UI code read from the WoW installation (decision T553).
#[cfg(feature = "native")]
pub mod installed;
mod natives;
pub mod profile;
pub mod reports;
pub mod runs;
mod saved;
pub mod scenario;
pub mod screenshots;
mod snapshots;

pub use snapshots::Summary as SnapshotSummary;

pub use blizzard::{
    INSTALLED_STRINGS, UiSource, cached_ui_source, commits as ui_commits, forget_ui_source,
    ui_source, ui_source_in, upstream_ui_source,
};
#[cfg(feature = "native")]
pub use blizzard::{extract_installed, installed_client};

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use wowlua_driver::{BuildOutput, TestFile, TestRole};
use wowlua_vm::{TableRef, Value, Vm};

/// The machine the emulated client runs on (decision T236).
pub use wowlua_engine::Platform;

/// The screen the emulated client draws on (decision T237).
pub use wowlua_engine::Screen;

/// What runs after each test (`Suite::after_test`).
pub type AfterTest = dyn Fn(&mut Vm, &str) + Send + Sync;

/// What a test run needs of a test build.
pub struct Suite<'a> {
    pub build: &'a BuildOutput,
    /// The flavor the build is for.
    pub flavor: String,
    /// The package root and its `build.src`, to show source paths.
    pub root: PathBuf,
    pub src: String,
    /// `test.blizzard-ui`: Blizzard's UI code, loaded before the addon.
    pub blizzard_ui: Option<UiSource>,
    /// `test.profile`: the world profile of files that name none.
    pub profile: Option<String>,
    /// A world profile's JSON text, over `profile` for files that name
    /// none (a playground run's, decision T401).
    pub profile_text: Option<String>,
    /// `test.platform`: the machine of files that name none (decision T236).
    pub platform: wowlua_engine::Platform,
    /// `test.screen`: the screen of files that name none (decision T237).
    pub screen: wowlua_engine::Screen,
    /// `test.addons`: other addons' folders, loaded before the addon.
    pub addons: Vec<PathBuf>,
    /// `-u`: rewrite the snapshots that differ.
    pub update_snapshots: bool,
    /// `--coverage`: count what the addon's files run.
    pub coverage: bool,
    /// Called with each VM the run makes, once the engine is installed
    /// and before anything loads: a debugger or the profiler attaches here
    /// (spec debugger §18.3).
    pub instrument: Option<&'a (dyn Fn(&mut Vm) + Sync)>,
    /// What draws screenshots (`toMatchScreenshot`, `mock.screenshot`).
    pub camera: Option<std::sync::Arc<dyn screenshots::Camera>>,
    /// Answer from the installation's own data where a test defines
    /// nothing (`wowlua test --game-data`, decision T366); off, a test
    /// gives the same result on every machine.
    pub game_data: bool,
    /// The 3D world on (decision T522): the character and the units on
    /// the installation's ground, the world behind the UI in screenshots
    /// (`--world`); off by default.
    pub world: bool,
    /// Called after each test with its VM and name, before the next
    /// (`wowlua screenshot`).
    pub after_test: Option<std::sync::Arc<AfterTest>>,
    /// Called when a test file's tests ran, with its VM and path.
    pub after_file: Option<std::sync::Arc<AfterTest>>,
}

fn widgets_keep(vm: &mut Vm, v: Value) {
    wowlua_engine::widgets::keep(vm, v);
}

/// One test file's run for one flavor.
#[derive(Clone, Debug)]
pub struct FileResult {
    /// The test file, relative to the package root.
    pub file: String,
    pub flavor: String,
    pub tests: Vec<TestResult>,
    /// The file could not run: an error while loading the addon, the
    /// mocks or the test file.
    pub error: Option<String>,
    /// What the file's code printed.
    pub printed: String,
    pub seconds: f64,
    pub snapshots: SnapshotSummary,
    /// With `--coverage`: what the file's tests ran of the addon.
    pub coverage: Option<coverage::Report>,
}

impl FileResult {
    pub fn failed(&self) -> bool {
        self.error.is_some()
            || self
                .tests
                .iter()
                .any(|t| matches!(t.outcome, Outcome::Fail { .. }))
    }
}

#[derive(Clone, Debug)]
pub struct TestResult {
    pub name: String,
    pub outcome: Outcome,
    pub seconds: f64,
    /// With `--coverage`: the addon's functions the test called, by
    /// source and the line each is defined on (decision T481).
    pub functions: Vec<(String, u32)>,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Outcome {
    Pass,
    /// The message and where it happened (source locations).
    Fail {
        message: String,
        stack: String,
    },
    Skip(String),
}

/// Runs the suite's test files whose path or tests' names contain
/// any of `filters` (all without one), on `jobs` threads.
pub fn run(suite: &Suite, filters: &[String], jobs: usize) -> Vec<FileResult> {
    let never = std::sync::atomic::AtomicBool::new(false);
    run_with(suite, filters, jobs, &never, &|_| {})
}

/// [`run`], telling `on_file` each file's result as it finishes, and
/// running no more files once `stop` is set (those left are not in the
/// results): an editor's progress and its stop button (wowlua studio).
pub fn run_with(
    suite: &Suite,
    filters: &[String],
    jobs: usize,
    stop: &std::sync::atomic::AtomicBool,
    on_file: &(dyn Fn(&FileResult) + Sync),
) -> Vec<FileResult> {
    let files: Vec<&TestFile> = suite
        .build
        .tests
        .iter()
        .filter(|t| t.role == TestRole::Test)
        .filter(|t| selectable(suite, t, filters))
        .collect();
    let next = std::sync::atomic::AtomicUsize::new(0);
    let results = std::sync::Mutex::new(Vec::new());
    std::thread::scope(|scope| {
        for _ in 0..jobs.clamp(1, files.len().max(1)) {
            let builder = std::thread::Builder::new().stack_size(64 << 20);
            let _ = builder.spawn_scoped(scope, || {
                loop {
                    if stop.load(std::sync::atomic::Ordering::Relaxed) {
                        break;
                    }
                    let i = next.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                    let Some(file) = files.get(i) else { break };
                    let result = run_file(suite, file, filters);
                    on_file(&result);
                    if let Ok(mut r) = results.lock() {
                        r.push((i, result));
                    }
                }
            });
        }
    });
    let mut results = results.into_inner().unwrap_or_default();
    results.sort_by_key(|(i, _)| *i);
    results
        .into_iter()
        .map(|(_, r)| r)
        .filter(|r| r.error.is_some() || !r.tests.is_empty() || filters.is_empty())
        .collect()
}

/// Whether `filters` may select a test of `file`: one is in its path, or
/// anywhere in its source (a test's name is written there: `@test("…")`,
/// the function's name, a group's). A file no filter can select is not
/// loaded: each is a whole emulated client, Blizzard's UI and all.
fn selectable(suite: &Suite, file: &TestFile, filters: &[String]) -> bool {
    if filters.is_empty() {
        return true;
    }
    // `path::name`: that file only
    if filters.iter().any(|f| {
        f.split_once("::")
            .is_some_and(|(p, _)| is_path(&file.rel, p))
    }) {
        return true;
    }
    let loose: Vec<&String> = filters.iter().filter(|f| !f.contains("::")).collect();
    if loose.iter().any(|f| file.rel.contains(f.as_str())) {
        return true;
    }
    match wowlua_host::read_to_string(suite.root.join(&file.rel)) {
        Ok(text) => loose.iter().any(|f| text.contains(f.as_str())),
        Err(_) => !loose.is_empty(),
    }
}

/// Whether test file `rel` is the one `path` names (the same path, or `path`
/// ends with it: an absolute one).
fn is_path(rel: &str, path: &str) -> bool {
    let path = path.replace('\\', "/");
    // (a source's examples are named by that source)
    let source = wowlua_driver::examples::source_of(rel);
    [rel, source.as_str()]
        .iter()
        .any(|r| path == *r || path.ends_with(&format!("/{r}")))
}

/// Whether filter `f` selects the test `name` of file `file` (a case of a
/// `@cases` test: `declared` is the test's own name): `path::name` that
/// test exactly (every case of a test of that name), anything else when the
/// name or path contains it.
pub fn selects(f: &str, file: &str, name: &str, declared: &str) -> bool {
    match f.split_once("::") {
        Some((path, exact)) => is_path(file, path) && (name == exact || declared == exact),
        None => name.contains(f) || file.contains(f),
    }
}

/// What the addon's files' chunk names start with: the client names an
/// addon's files `Interface/AddOns/<Name>/<path>` (`debugstack` in a
/// recording: `Interface/AddOns/StdTest/src/engineprobe.lua:806`).
fn chunk_prefix(build: &BuildOutput) -> String {
    format!("Interface/AddOns/{}/", build.addon_name)
}

/// Loads files of the addon with the names the client gives them
/// ([`chunk_prefix`]); what loads later (Blizzard's addons on demand) is
/// named as before.
fn named_as_client(vm: &mut Vm, build: &BuildOutput, load: impl FnOnce(&mut Vm)) {
    let saved = wowlua_engine::engine_mut(vm)
        .world
        .ui
        .chunk_prefix
        .replace(chunk_prefix(build));
    load(vm);
    wowlua_engine::engine_mut(vm).world.ui.chunk_prefix = saved;
}

/// Whether coverage counts a chunk: the addon's own files (`src/…`), not
/// test files.
fn counted(prefix: &str, chunk: &str) -> bool {
    chunk
        .strip_prefix(prefix)
        .is_some_and(|rest| rest.starts_with("src/") && !rest.ends_with(".test.lua"))
}

/// The source path of an output file (`src/x.lua` → `src/x.wowlua`), by
/// its path in the AddOn folder or its chunk name.
fn source_of(suite: &Suite, output: &str) -> String {
    let output = output
        .strip_prefix(chunk_prefix(suite.build).as_str())
        .unwrap_or(output);
    source_path(&suite.root, &suite.src, output)
}

/// [`source_of`] by the package root and its `build.src`.
fn source_path(root: &Path, src: &str, output: &str) -> String {
    let src = src.trim_end_matches('/');
    let (prefix, rest) = if let Some(rest) = output.strip_prefix("src/") {
        (
            if src.is_empty() || src == "." {
                String::new()
            } else {
                format!("{src}/")
            },
            rest,
        )
    } else if let Some(rest) = output.strip_prefix("tests/") {
        ("tests/".to_string(), rest)
    } else {
        return output.to_string();
    };
    let stem = rest.strip_suffix(".lua").unwrap_or(rest);
    let wowlua = format!("{prefix}{stem}.wowlua");
    // a doc comment's examples: the source they are written in (their
    // lines are its)
    if wowlua.ends_with(wowlua_driver::examples::SUFFIX) {
        return wowlua_driver::examples::source_of(&wowlua);
    }
    if wowlua_host::exists(root.join(&wowlua)) {
        wowlua
    } else {
        format!("{prefix}{rest}")
    }
}

/// The first source location in a stack (`tests/x.test.wowlua:12`) that
/// is not the standard library's.
pub fn location(stack: &str) -> Option<String> {
    let re = regex::Regex::new(r"[\w./@-]+\.(?:wowlua|lua):\d+").expect("a valid pattern");
    re.find_iter(stack)
        .map(|m| m.as_str())
        .find(|l| !l.starts_with("wowlua/"))
        .map(str::to_string)
}

/// A failure's message without the location an error put first, and the
/// location: that one, else the stack's.
pub fn split_failure(message: &str, stack: &str) -> (String, Option<String>) {
    let re = regex::Regex::new(r"^([\w./@-]+\.(?:wowlua|lua):\d+): ").expect("a valid pattern");
    if let Some(c) = re.captures(message)
        && !c[1].starts_with("wowlua/")
    {
        return (message[c[0].len()..].to_string(), Some(c[1].to_string()));
    }
    (message.to_string(), location(stack))
}

/// Error messages and stacks with source paths: the test profile keeps
/// lines, so `Interface/AddOns/Demo/src/x.lua:12` is `src/x.wowlua:12`.
/// A chunk name Lua shortened (`...ns/Demo/src/x.lua`, past 52 bytes) is
/// the addon's file it ends, when only one does; other addons' files stay.
fn to_sources(suite: &Suite, text: &str) -> String {
    let re = regex::Regex::new(r"\[?(\.\.\.)?([\w./@-]+\.lua)\]?(:\d+)").expect("a valid pattern");
    let prefix = chunk_prefix(suite.build);
    let own = |path: &str| -> Option<String> {
        if let Some(rest) = path.strip_prefix(prefix.as_str()) {
            return Some(rest.to_string());
        }
        ["src/", "tests/", "wowlua/"]
            .iter()
            .any(|p| path.starts_with(p))
            .then(|| path.to_string())
    };
    let ending = |tail: &str| -> Option<String> {
        let files = suite.build.files.keys();
        let tests = suite.build.tests.iter().map(|t| &t.path);
        let mut found = files
            .chain(tests)
            .filter(|f| f.ends_with(".lua") && format!("{prefix}{f}").ends_with(tail));
        let first = found.next()?;
        found.all(|f| f == first).then(|| first.clone())
    };
    re.replace_all(text, |c: &regex::Captures| {
        let path = if c.get(1).is_some() {
            ending(&c[2])
        } else {
            own(&c[2])
        };
        match path {
            Some(path) => format!("{}{}", source_of(suite, &path), &c[3]),
            None => c[0].to_string(),
        }
    })
    .into_owned()
}

fn get(vm: &mut Vm, t: TableRef, key: &str) -> Value {
    let k = vm.heap.intern(key.as_bytes());
    vm.heap.table(t).get_str(k)
}

fn set(vm: &mut Vm, t: TableRef, key: &str, v: Value) {
    let k = vm.heap.intern(key.as_bytes());
    vm.heap.table_mut(t).set_str(k, v);
}

fn text(vm: &mut Vm, v: Value) -> String {
    match v {
        Value::Str(s) => String::from_utf8_lossy(vm.str_bytes(s)).into_owned(),
        Value::Nil => String::new(),
        other => vm
            .tostring(other)
            .ok()
            .map(|s| text(vm, s))
            .unwrap_or_default(),
    }
}

/// A Lua error's message (a test failure's own, for tables).
fn error_text(vm: &mut Vm, e: &wowlua_vm::LuaError) -> String {
    text(vm, e.value)
}

/// The output paths a file needs loaded first, itself last.
fn closure<'a>(build: &'a BuildOutput, path: &'a str, out: &mut Vec<&'a str>) {
    if out.contains(&path) {
        return;
    }
    if let Some(t) = build.tests.iter().find(|t| t.path == path) {
        // Placed before its imports are, so a cycle ends.
        out.push(path);
        let at = out.len() - 1;
        for d in &t.deps {
            closure(build, d, out);
        }
        let me = out.remove(at);
        out.push(me);
    }
}

/// Runs one test file (its tests whose names contain any of `filters`)
/// on this thread.
pub fn run_file(suite: &Suite, file: &TestFile, filters: &[String]) -> FileResult {
    let started = wowlua_host::Stopwatch::start();
    let rel = file.rel.clone();
    let mut result = FileResult {
        // (a source's examples are reported as that source's)
        file: wowlua_driver::examples::source_of(&rel),
        flavor: suite.flavor.clone(),
        tests: Vec::new(),
        error: None,
        printed: String::new(),
        seconds: 0.0,
        snapshots: SnapshotSummary::default(),
        coverage: None,
    };
    snapshots::begin(&suite.root.join(&file.rel), suite.update_snapshots);
    screenshots::begin(
        &suite.root.join(&file.rel),
        suite.update_snapshots,
        suite.camera.clone(),
    );
    natives::reset();
    match load(suite, file, None) {
        Ok(mut state) => {
            run_tests(suite, file, &mut state, filters, &mut result);
            if let Some(f) = &suite.after_file {
                f(&mut state.vm, &file.rel);
            }
            if let Some(mut report) = coverage_of(suite, &state.vm) {
                for extra in &state.reloaded_coverage {
                    report.merge(extra);
                }
                result.coverage = Some(report);
            }
            let all_ran = filters.is_empty() && result.error.is_none();
            match snapshots::finish(all_ran) {
                Ok(mut s) => {
                    // Screenshots count with the snapshots.
                    let (written, updated) = screenshots::finish();
                    s.written += written;
                    s.updated += updated;
                    result.snapshots = s
                }
                Err(e) => result.error = Some(format!("writing the snapshots: {e}")),
            }
        }
        Err(e) => result.error = Some(to_sources(suite, &e)),
    }
    result.printed = natives::take_printed();
    result.seconds = started.elapsed().as_secs_f64();
    result
}

/// How often each function of a VM's run was called so far (by its
/// coverage's prototypes), to tell what a test called.
fn entry_counts(vm: &Vm) -> Vec<u32> {
    vm.coverage.as_ref().map_or_else(Vec::new, |cov| {
        cov.protos
            .iter()
            .map(|c| c.hits.first().copied().unwrap_or(0))
            .collect()
    })
}

/// The addon's functions called since `before` (a test's): by source and
/// the line each is defined on. A VM a test made anew (`mock.reloadUI`)
/// counts from nothing.
fn functions_ran(suite: &Suite, vm: &Vm, before: &[u32]) -> Vec<(String, u32)> {
    let Some(cov) = vm.coverage.as_ref() else {
        return Vec::new();
    };
    let prefix = chunk_prefix(suite.build);
    let mut out: Vec<(String, u32)> = Vec::new();
    for (i, counts) in cov.protos.iter().enumerate() {
        let now = counts.hits.first().copied().unwrap_or(0);
        if counts.main || now <= before.get(i).copied().unwrap_or(0) {
            continue;
        }
        let chunk = String::from_utf8_lossy(&counts.proto.source);
        let chunk = chunk.strip_prefix('@').unwrap_or(&chunk);
        if !counted(&prefix, chunk) {
            continue;
        }
        let entry = (source_of(suite, chunk), counts.proto.line_defined);
        if !out.contains(&entry) {
            out.push(entry);
        }
    }
    out.sort();
    out
}

/// The counts of a VM's run, by source file.
fn coverage_of(suite: &Suite, vm: &Vm) -> Option<coverage::Report> {
    let cov = vm.coverage.as_ref()?;
    let prefix = chunk_prefix(suite.build);
    let map = |chunk: &str| -> Option<String> {
        counted(&prefix, chunk).then(|| source_of(suite, chunk))
    };
    Some(coverage::collect(vm, cov, &suite.root, &map))
}

/// A test file's VM, loaded: the registry `wowlua:test` filled.
struct Loaded {
    vm: Vm,
    tests: TableRef,
    /// The variables the TOC saves.
    saved: Vec<String>,
    /// The coverage of the VMs `mock.reloadUI` made and dropped.
    reloaded_coverage: Vec<coverage::Report>,
}

/// After `mock.reloadUI`: the pass, and the clock's time, which goes on.
struct Reload {
    pass: usize,
    now: f64,
    elapsed_ms: f64,
}

/// What fails a test on an error the client drops without a report
/// (`secureexecuterange`'s callbacks): code that failed, though no error
/// handler heard it.
const DROPPED_FAILURE: &str = "an error was dropped without a report (as the client drops it)";

fn load(suite: &Suite, file: &TestFile, reload: Option<&Reload>) -> Result<Loaded, String> {
    let (vm, tests, saved) = load_vm(suite, Some(file), reload)?;
    Ok(Loaded {
        vm,
        tests: tests.ok_or("the test file does not import `wowlua:test`")?,
        saved,
        reloaded_coverage: Vec::new(),
    })
}

/// The addon loaded as the client loads it, without a test file, the
/// player logged in (a debug session of the addon, spec debugger §18.3).
pub fn open_addon(suite: &Suite) -> Result<Vm, String> {
    natives::reset();
    load_vm(suite, None, None)
        .map(|(vm, _, _)| vm)
        .map_err(|e| to_sources(suite, &e))
}

/// A debug session of the addon (spec debugger §18.3): its VM, and the
/// variables its TOC saves, so a `/reload` loads it again with them.
pub struct AddonSession {
    pub vm: Vm,
    saved: Vec<String>,
    reloads: usize,
}

impl AddonSession {
    /// The addon loaded as the client loads it, the player logged in.
    pub fn open(suite: &Suite) -> Result<AddonSession, String> {
        natives::reset();
        let (vm, _, saved) = load_vm(suite, None, None).map_err(|e| to_sources(suite, &e))?;
        Ok(AddonSession {
            vm,
            saved,
            reloads: 0,
        })
    }

    /// The addon's UI asked to reload (`ReloadUI()`, `/reload`): its
    /// session ended (`PLAYER_LOGOUT`).
    pub fn wants_reload(&self) -> bool {
        wowlua_engine::engine(&self.vm).world.logged_out
    }

    /// Loads the UI again as the client does after a `/reload` (as
    /// `mock.reloadUI` does in a test): a fresh VM, the SavedVariables
    /// kept, the clock going on, the reload's login.
    pub fn reload(&mut self, suite: &Suite) -> Result<(), String> {
        let saved = saved::capture(&mut self.vm, &self.saved);
        let reload = Reload {
            pass: self.reloads + 1,
            now: self.vm.clock.now,
            elapsed_ms: self.vm.clock.elapsed_ms,
        };
        natives::keep_saved(saved);
        let (vm, _, names) =
            load_vm(suite, None, Some(&reload)).map_err(|e| to_sources(suite, &e))?;
        self.vm = vm;
        self.saved = names;
        self.reloads += 1;
        Ok(())
    }
}

/// The source path (`src/x.wowlua`, `tests/x.test.wowlua`) of a chunk
/// name of the package's own files; `None` for others' (Blizzard's,
/// other addons').
pub fn source_of_chunk(suite: &Suite, chunk: &str) -> Option<String> {
    let chunk = chunk.strip_prefix('@').unwrap_or(chunk);
    let prefix = chunk_prefix(suite.build);
    let rest = chunk.strip_prefix(prefix.as_str())?;
    if rest.starts_with("wowlua/") {
        return None;
    }
    Some(source_of(suite, rest))
}

/// Chunk name → source path.
pub type ChunkMapper = Box<dyn Fn(&str) -> Option<String> + Send + Sync>;

/// [`source_of_chunk`] as a function that owns what it needs (a debug
/// session's map from chunk names to sources).
pub fn chunk_mapper(suite: &Suite) -> ChunkMapper {
    let prefix = chunk_prefix(suite.build);
    let root = suite.root.clone();
    let src = suite.src.clone();
    Box::new(move |chunk: &str| {
        let chunk = chunk.strip_prefix('@').unwrap_or(chunk);
        let rest = chunk.strip_prefix(prefix.as_str())?;
        if rest.starts_with("wowlua/") {
            return None;
        }
        Some(source_path(&root, &src, rest))
    })
}

/// Chunk name → the file on disk of code that is not the package's: a
/// `test.addons` folder's (`Interface/AddOns/<Name>/…`), else Blizzard's
/// UI in the local cache; absolute (a debugger's frames open it).
pub fn chunk_file_mapper(suite: &Suite) -> ChunkMapper {
    let addons = suite.addons.clone();
    let interface = suite.blizzard_ui.as_ref().map(|u| u.interface.clone());
    Box::new(move |chunk: &str| {
        let chunk = chunk.strip_prefix('@').unwrap_or(chunk).replace('\\', "/");
        let at = chunk.find("Interface/AddOns/")?;
        let rest = &chunk[at + "Interface/AddOns/".len()..];
        let (name, file) = rest.split_once('/')?;
        let found = addons
            .iter()
            .find(|d| {
                d.file_name()
                    .is_some_and(|n| n.to_string_lossy().eq_ignore_ascii_case(name))
            })
            .and_then(|d| wowlua_engine::bindings::find_addon_file(d, file, &[]))
            .or_else(|| {
                let dir = interface.as_ref()?.join("AddOns").join(name);
                wowlua_engine::bindings::find_addon_file(&dir, file, &[])
            })?;
        Some(found.to_string_lossy().into_owned())
    })
}

/// What a test run sends on as it happens: printed lines and errors
/// that reached the error handler (a debug session's console).
pub use natives::{Output, set_tee};

/// The screen of the world profile a load applies (the file's, the
/// given one, the suite's), when it has a `screen` part (decision T482).
fn profile_screen(suite: &Suite, source: &[u8]) -> Result<Option<wowlua_engine::Screen>, String> {
    let named = profile::named_in(source);
    if let (None, Some(text)) = (&named, &suite.profile_text) {
        return profile::screen_of(text, "given");
    }
    let Some(name) = named.or_else(|| suite.profile.clone()) else {
        return Ok(None);
    };
    // (a profile that can't be read is told when it is applied)
    match profile::named(&suite.root, &suite.flavor, &name) {
        Ok((text, label)) => profile::screen_of(&text, &label),
        Err(_) => Ok(None),
    }
}

/// A VM loaded for `file` (or the addon alone), with `wowlua:test`'s
/// registry when a test file loaded, and the variables the TOC saves.
fn load_vm(
    suite: &Suite,
    file: Option<&TestFile>,
    reload: Option<&Reload>,
) -> Result<(Vm, Option<TableRef>, Vec<String>), String> {
    let build = suite.build;
    let dir = &build.dir;
    let name = build.addon_name.clone();
    let mut vm = Vm::wow();
    wowlua_engine::install(&mut vm, &suite.flavor)?;
    if let Some(data) = suite
        .camera
        .as_ref()
        .filter(|_| suite.game_data)
        .and_then(|c| c.client_data())
    {
        wowlua_engine::clientdata::set(&mut vm, data);
    }
    wowlua_engine::set_world(&mut vm, suite.world);
    // The machine the client runs on (decision T236): the file's
    // `config({ platform = … })`, else the manifest's `test.platform`.
    let source = file
        .and_then(|f| wowlua_host::read(suite.root.join(&f.rel)).ok())
        .unwrap_or_default();
    let platform = match profile::config_text(&source, "platform") {
        Some(name) => wowlua_engine::Platform::parse(&name).ok_or_else(|| {
            format!("config: platform \"{name}\" is not one of \"mac\", \"windows\"")
        })?,
        None => suite.platform,
    };
    wowlua_engine::set_platform(&mut vm, platform);
    // The screen it draws on (decision T237): the file's `config({ screen
    // = { width, height, dpiScale, uiScale } })`, else the world profile's
    // (T482), else the manifest's.
    let screen = match profile::config_numbers(&source, "screen") {
        Some(fields) => {
            let get = |k: &str| fields.iter().find(|(n, _)| n == k).map(|(_, v)| *v);
            let (Some(width), Some(height)) = (get("width"), get("height")) else {
                return Err("config: screen needs a width and a height".into());
            };
            wowlua_engine::Screen {
                width,
                height,
                dpi_scale: get("dpiScale").unwrap_or(1.0),
                ui_scale: get("uiScale"),
            }
        }
        None => profile_screen(suite, &source)?.unwrap_or(suite.screen),
    };
    wowlua_engine::set_screen(&mut vm, screen);
    if let Some(instrument) = suite.instrument {
        instrument(&mut vm);
    }
    if let Some(r) = reload {
        vm.clock.now = r.now;
        vm.clock.elapsed_ms = r.elapsed_ms;
    }
    if suite.coverage {
        let prefix = chunk_prefix(build);
        vm.enable_coverage(Box::new(move |chunk: &[u8]| {
            counted(
                &prefix,
                &String::from_utf8_lossy(chunk.strip_prefix(b"@").unwrap_or(chunk)),
            )
        }));
    }
    if let Some(ui) = &suite.blizzard_ui {
        let report = wowlua_engine::ui::load(&mut vm, &ui.interface, ui.strings.as_deref());
        if !report.errors.is_empty() {
            return Err(format!(
                "Blizzard's UI code raised {} errors while loading:\n{}",
                report.errors.len(),
                report.errors.join("\n")
            ));
        }
    }
    vm.set_output(natives::print_sink);
    natives::install_print(&mut vm);
    vm.error_handler = vm.native("errorhandler", natives::error_handler);
    vm.on_dropped_error = Some(natives::dropped_error);
    // The addon table, with the runtime's table holding the mock's engine.
    let ns = vm.new_table();
    let wl = vm.new_table();
    set(&mut vm, ns, "__wowlua", Value::Table(wl));
    let controls = wowlua_engine::controls::table(&mut vm);
    natives::extend(&mut vm, controls, &name);
    set(&mut vm, wl, "mockEngine", Value::Table(controls));
    if let Some(r) = reload {
        set(&mut vm, wl, "reloads", Value::Num(r.pass as f64));
    }
    wowlua_engine::widgets::keep(&mut vm, Value::Table(ns));
    let table = Value::Table(ns);
    // The world profile, before the addon (decision T213).
    let named = profile::named_in(&source);
    if let (None, Some(text)) = (&named, &suite.profile_text) {
        profile::apply_text(&mut vm, controls, text, "given")?;
    } else if let Some(name) = named.or_else(|| suite.profile.clone()) {
        let (text, label) = profile::named(&suite.root, &suite.flavor, &name)?;
        profile::apply_text(&mut vm, controls, &text, &label)?;
    } else if let Some(text) = profile::default_for(&suite.flavor) {
        profile::apply_text(&mut vm, controls, text, "of the fresh character")?;
    }

    // The addon's key bindings, which the client reads at startup.
    let bindings = dir.join("Bindings.xml");
    natives::set_has_bindings(wowlua_host::exists(&bindings));
    if wowlua_host::exists(&bindings) {
        wowlua_engine::bindings::load_bindings_xml(&mut vm, &bindings, Some(&name))?;
    }
    let toc = toc_path(build, &suite.flavor).ok_or("the build has no TOC file")?;
    let toc_text = wowlua_host::read_to_string(dir.join(&toc)).map_err(|e| e.to_string())?;
    let mut saved = saved::names(&wowlua_toc::parse(&toc_text));
    let listed: Vec<String> = wowlua_toc::parse(&toc_text)
        .files
        .iter()
        .map(|f| f.path.replace('\\', "/"))
        .collect();
    let mut loaded: HashSet<String> = HashSet::new();
    let errors_before = |vm: &Vm| wowlua_engine::engine(vm).world.ui.errors.len();
    let check = |vm: &mut Vm, from: usize, what: &str| -> Result<(), String> {
        let errors = &wowlua_engine::engine(vm).world.ui.errors;
        if errors.len() > from {
            return Err(format!("{what}: {}", errors[from..].join("\n")));
        }
        Ok(())
    };
    // The TOC's files up to the runtime (locale files come first), then
    // the mocks (with what they import), then the rest of the addon.
    if let Some(runtime) = listed.iter().position(|p| p == "wowlua/runtime.lua") {
        let before = errors_before(&vm);
        for path in &listed[..=runtime] {
            named_as_client(&mut vm, build, |vm| {
                wowlua_engine::ui::load_user_file(vm, dir, path, &name, table)
            });
            loaded.insert(path.clone());
        }
        check(&mut vm, before, "loading the runtime")?;
    }
    let before = errors_before(&vm);
    for mock in build.tests.iter().filter(|t| t.role == TestRole::Mock) {
        let mut order = Vec::new();
        closure(build, &mock.path, &mut order);
        for path in order {
            if loaded.insert(path.to_string()) {
                named_as_client(&mut vm, build, |vm| {
                    wowlua_engine::ui::load_user_file(vm, dir, path, &name, table)
                });
            }
        }
    }
    check(&mut vm, before, "loading the mocks")?;
    // The other addons (`test.addons`), as the client loads them first.
    for addon in other_addons(suite)? {
        saved.extend(addon.saved.iter().cloned());
        let before = errors_before(&vm);
        load_other_addon(&mut vm, &addon)?;
        check(
            &mut vm,
            before,
            &format!("loading the addon {}", addon.name),
        )?;
    }
    // The addon itself, known to the addon functions as its TOC says.
    let own = wowlua_engine::ui::read_user_toc(&suite.flavor, &name, dir, &dir.join(&toc))?;
    wowlua_engine::bindings::add_own_addon(&mut vm, &own, table, wowlua_host::exists(&bindings));
    let before = errors_before(&vm);
    named_as_client(&mut vm, build, |vm| {
        wowlua_engine::ui::load_user_addon(vm, dir, &dir.join(&toc), &name, table, &loaded)
    });
    loaded.extend(listed);
    check(&mut vm, before, "loading the addon")?;
    let Some(file) = file else {
        natives::login(&mut vm, &name);
        return Ok((vm, None, saved));
    };
    let before = errors_before(&vm);
    let mut order = Vec::new();
    closure(build, &file.path, &mut order);
    for path in order {
        if loaded.insert(path.to_string()) {
            named_as_client(&mut vm, build, |vm| {
                wowlua_engine::ui::load_user_file(vm, dir, path, &name, table)
            });
        }
    }
    check(&mut vm, before, "loading the test file")?;
    let tests = match get(&mut vm, wl, "tests") {
        Value::Table(t) => t,
        _ => return Ok((vm, None, saved)),
    };
    // The lifecycle (step 5), unless the file drives it.
    let settings = match get(&mut vm, tests, "settings") {
        Value::Table(t) => Some(t),
        _ => None,
    };
    let manual = settings.is_some_and(|s| {
        let v = get(&mut vm, s, "lifecycle");
        text(&mut vm, v) == "manual"
    });
    if !manual {
        natives::login(&mut vm, &name);
    }
    Ok((vm, Some(tests), saved))
}

/// The `test.addons` the client loads in this game, in its load order.
fn other_addons(suite: &Suite) -> Result<Vec<wowlua_engine::ui::UserAddon>, String> {
    let mut list = Vec::new();
    for dir in &suite.addons {
        let addon = wowlua_engine::ui::read_user_addon(&suite.flavor, dir)
            .map_err(|e| format!("test.addons: {e}"))?;
        // The client leaves out an addon for other games (unverified:
        // whether its addon list shows it).
        if addon.allowed {
            list.push(addon);
        }
    }
    Ok(wowlua_engine::ui::user_addon_order(list))
}

/// One of the `test.addons`, known to the client with its own addon table:
/// its `Bindings.xml` read, then its files (unless on demand), after the
/// Blizzard addons it depends on.
fn load_other_addon(vm: &mut Vm, addon: &wowlua_engine::ui::UserAddon) -> Result<(), String> {
    let table = Value::Table(vm.new_table());
    let bindings = addon.dir.join("Bindings.xml");
    if wowlua_host::exists(&bindings) {
        wowlua_engine::bindings::load_bindings_xml(vm, &bindings, Some(&addon.name))?;
    }
    wowlua_engine::bindings::add_user_addon(vm, addon, table, wowlua_host::exists(&bindings));
    if addon.on_demand {
        return Ok(());
    }
    for dep in &addon.deps {
        wowlua_engine::ui::load_addon(vm, dep);
    }
    wowlua_engine::ui::load_other_user_addon(vm, &addon.dir, &addon.toc, &addon.name, table);
    Ok(())
}

/// The TOC the client would read for the flavor: the flavor's own
/// (`<Name>_Mainline.toc`), else the plain one.
fn toc_path(build: &BuildOutput, flavor: &str) -> Option<String> {
    let tocs: Vec<&str> = build
        .files
        .keys()
        .filter(|p| !p.contains('/') && p.to_ascii_lowercase().ends_with(".toc"))
        .map(String::as_str)
        .collect();
    let suffixes = wowlua_flavors::table()
        .clients
        .iter()
        .find(|c| c.flavor == flavor)
        .map(|c| c.suffixes.clone())
        .unwrap_or_default();
    for suffix in suffixes {
        if let Some(t) = tocs.iter().find(|t| {
            t.to_ascii_lowercase()
                .ends_with(&format!("{}.toc", suffix.to_ascii_lowercase()))
        }) {
            return Some(t.to_string());
        }
    }
    let plain = format!("{}.toc", build.addon_name);
    tocs.iter()
        .find(|t| t.eq_ignore_ascii_case(&plain))
        .or(tocs.first())
        .map(|t| t.to_string())
}

/// The longest a single wait may take, in frames (a test's timeout).
fn timeout_of(vm: &mut Vm, tests: TableRef) -> f64 {
    let f = get(vm, tests, "timeout");
    vm.call(f, &[])
        .ok()
        .and_then(|r| match r.first() {
            Some(Value::Num(n)) => Some(*n),
            _ => None,
        })
        .unwrap_or(10.0)
}

const FRAME_MS: f64 = 1000.0 / 60.0;

/// Steps a job until it finishes (a reload outside a test fails it).
fn drive(vm: &mut Vm, tests: TableRef, job: Value) -> Outcome {
    drive_inner(vm, tests, job).unwrap_or_else(|| Outcome::Fail {
        message: "mock.reloadUI runs only in a test".into(),
        stack: String::new(),
    })
}

/// Steps a job until it finishes (`None`: at `mock.reloadUI`; resumed,
/// the job ends the pass and runs its teardown).
fn drive_inner(vm: &mut Vm, tests: TableRef, job: Value) -> Option<Outcome> {
    // Only this function holds the job (a Lua table): kept where the
    // collector sees it, or a collection in a timer between steps frees it
    // ("stale handle" when the next step reads it; the default UI's every
    // window, run in one test).
    wowlua_engine::widgets::keep_as(vm, "wowlua.test.job", job);
    let step = get(vm, tests, "step");
    loop {
        let r = match vm.call(step, &[job]) {
            Ok(r) => r,
            Err(e) => {
                return Some(Outcome::Fail {
                    message: error_text(vm, &e),
                    stack: e.traceback.clone(),
                });
            }
        };
        let status = r.first().map(|&v| text(vm, v)).unwrap_or_default();
        match status.as_str() {
            "pass" => return Some(Outcome::Pass),
            "wait" => {
                let kind = r.get(1).map(|&v| text(vm, v)).unwrap_or_default();
                if kind == "reload" {
                    return None;
                }
                let amount = match r.get(2) {
                    Some(Value::Num(n)) => *n,
                    _ => 1.0,
                };
                let frames = if kind == "seconds" {
                    (amount * 1000.0 / FRAME_MS).ceil()
                } else {
                    amount.ceil()
                };
                let limit = timeout_of(vm, tests) * 1000.0 / FRAME_MS;
                if frames > limit.ceil() {
                    return Some(Outcome::Fail {
                        message: format!(
                            "a wait of {amount} {kind} is longer than the test's timeout"
                        ),
                        stack: String::new(),
                    });
                }
                let target = wowlua_engine::clock::now(vm) + amount;
                wowlua_engine::clock::advance_frames(vm, frames.max(0.0) as usize, FRAME_MS);
                // Frames of 1000/60 ms add up to a hair less than the
                // seconds asked for: the clock reaches them.
                let now = wowlua_engine::clock::now(vm);
                if kind == "seconds" && now < target {
                    wowlua_engine::clock::advance(vm, target - now);
                }
            }
            _ => {
                let message = r.get(1).map(|&v| text(vm, v)).unwrap_or_default();
                let stack = r.get(2).map(|&v| text(vm, v)).unwrap_or_default();
                return Some(Outcome::Fail { message, stack });
            }
        }
    }
}

fn start(
    vm: &mut Vm,
    tests: TableRef,
    kind: &str,
    i: Option<usize>,
    case: Option<usize>,
) -> Result<Value, String> {
    let f = get(vm, tests, "start");
    let kind = vm.str(kind);
    let args = [
        kind,
        i.map_or(Value::Nil, |i| Value::Num(i as f64)),
        case.map_or(Value::Nil, |c| Value::Num(c as f64)),
    ];
    match vm.call(f, &args) {
        Ok(r) => Ok(r.first().copied().unwrap_or(Value::Nil)),
        Err(e) => Err(error_text(vm, &e)),
    }
}

/// One planned test: its number, case and name.
struct Planned {
    index: usize,
    case: Option<usize>,
    name: String,
    skip: Option<String>,
}

fn plan(
    vm: &mut Vm,
    tests: TableRef,
    flavor: &str,
    file: &str,
    filters: &[String],
) -> Vec<Planned> {
    let count = get(vm, tests, "count");
    let n = match vm.call(count, &[]).ok().and_then(|r| r.first().copied()) {
        Some(Value::Num(n)) => n as usize,
        _ => 0,
    };
    let info = get(vm, tests, "info");
    let case_name = get(vm, tests, "caseName");
    let flavor_v = vm.str(flavor);
    let mut planned = Vec::new();
    let mut any_only = false;
    let mut entries = Vec::new();
    for i in 1..=n {
        let Ok(r) = vm.call(info, &[Value::Num(i as f64), flavor_v]) else {
            continue;
        };
        let name = r.first().map(|&v| text(vm, v)).unwrap_or_default();
        let skip = match r.get(3) {
            Some(&v @ Value::Str(_)) => Some(text(vm, v)),
            _ => None,
        };
        let only = matches!(r.get(4), Some(Value::Bool(true)));
        let runs = matches!(r.get(5), Some(Value::Bool(true)));
        let cases = match r.get(6) {
            Some(Value::Num(c)) => *c as usize,
            _ => 0,
        };
        any_only |= only;
        entries.push((i, name, skip, only, runs, cases));
    }
    for (i, name, skip, only, runs, cases) in entries {
        if !runs || (any_only && !only) {
            continue;
        }
        let declared = name.clone();
        let names: Vec<(Option<usize>, String)> = if cases == 0 {
            vec![(None, name)]
        } else {
            (1..=cases)
                .map(|c| {
                    let n = vm
                        .call(case_name, &[Value::Num(i as f64), Value::Num(c as f64)])
                        .ok()
                        .and_then(|r| r.first().copied())
                        .map(|v| text(vm, v))
                        .unwrap_or_default();
                    (Some(c), n)
                })
                .collect()
        };
        for (case, name) in names {
            if !filters.is_empty() && !filters.iter().any(|f| selects(f, file, &name, &declared)) {
                continue;
            }
            planned.push(Planned {
                index: i,
                case,
                name,
                skip: skip.clone(),
            });
        }
    }
    planned
}

/// The most passes a test may reload into.
const MAX_RELOADS: usize = 10;

fn failed(message: String) -> Outcome {
    Outcome::Fail {
        message,
        stack: String::new(),
    }
}

/// A test, with its passes after `mock.reloadUI`: each in a fresh VM with
/// the SavedVariables kept, the clock going on; the file's VM stays the
/// first pass's.
fn run_one(suite: &Suite, file: &TestFile, state: &mut Loaded, p: &Planned) -> Outcome {
    // `mock.errors()` lists this test's errors (its `@beforeEach` hooks' too).
    natives::begin_test();
    let tests = state.tests;
    let job = match start(&mut state.vm, tests, "test", Some(p.index), p.case) {
        Ok(job) => job,
        Err(e) => return failed(e),
    };
    if let Some(o) = drive_inner(&mut state.vm, tests, job) {
        return o;
    }
    // The first pass ends: its teardown runs in the file's VM. An addon's
    // `ReloadUI()` ended its session (`PLAYER_LOGOUT`, nothing running
    // after): the file's next tests go on in it as after `mock.reloadUI`.
    let mut saved = saved::capture(&mut state.vm, &state.saved);
    wowlua_engine::engine_mut(&mut state.vm).world.logged_out = false;
    let mut clock = (state.vm.clock.now, state.vm.clock.elapsed_ms);
    if let Some(o @ Outcome::Fail { .. }) = drive_inner(&mut state.vm, tests, job) {
        return o;
    }
    let mut pass = 0;
    let outcome = loop {
        pass += 1;
        if pass > MAX_RELOADS {
            break failed(format!(
                "the test reloaded the UI more than {MAX_RELOADS} times"
            ));
        }
        natives::keep_saved(saved.clone());
        let reload = Reload {
            pass,
            now: clock.0,
            elapsed_ms: clock.1,
        };
        let mut fresh = match load(suite, file, Some(&reload)) {
            Ok(l) => l,
            Err(e) => break failed(format!("reloading the UI: {e}")),
        };
        let pending = natives::unread_errors();
        if !pending.is_empty() {
            break failed(format!(
                "an error reached the error handler while reloading:\n{}",
                pending.join("\n")
            ));
        }
        let dropped = natives::unread_dropped();
        if !dropped.is_empty() {
            break failed(format!(
                "{DROPPED_FAILURE} while reloading:\n{}",
                dropped.join("\n")
            ));
        }
        let tests = fresh.tests;
        let vm = &mut fresh.vm;
        let ended = match start(vm, tests, "beforeAll", None, None) {
            Ok(job) => match drive(vm, tests, job) {
                Outcome::Fail { message, .. } => Some(failed(format!(
                    "@beforeAll failed after the reload: {message}"
                ))),
                _ => None,
            },
            Err(e) => Some(failed(e)),
        };
        let ended = ended.or_else(|| {
            let job = match start(vm, tests, "test", Some(p.index), p.case) {
                Ok(job) => job,
                Err(e) => return Some(failed(e)),
            };
            let done = drive_inner(vm, tests, job);
            if done.is_none() {
                saved = saved::capture(vm, &fresh.saved);
                clock = (vm.clock.now, vm.clock.elapsed_ms);
                if let Some(o @ Outcome::Fail { .. }) = drive_inner(vm, tests, job) {
                    return Some(o);
                }
            }
            done
        });
        if let Ok(job) = start(vm, tests, "afterAll", None, None) {
            let _ = drive(vm, tests, job);
        }
        if let Some(report) = coverage_of(suite, &fresh.vm) {
            state.reloaded_coverage.push(report);
        }
        if let Some(o) = ended {
            break o;
        }
    };
    // A reload's SavedVariables a manual lifecycle never logged in with.
    natives::drop_saved();
    outcome
}

fn run_tests(
    suite: &Suite,
    file: &TestFile,
    state: &mut Loaded,
    filters: &[String],
    result: &mut FileResult,
) {
    let tests = state.tests;
    let vm = &mut state.vm;
    let planned = plan(vm, tests, &suite.flavor, &result.file, filters);
    if planned.is_empty() {
        return;
    }
    let fail_all = |result: &mut FileResult, planned: &[Planned], message: String| {
        for p in planned {
            result.tests.push(TestResult {
                name: p.name.clone(),
                outcome: Outcome::Fail {
                    message: message.clone(),
                    stack: String::new(),
                },
                seconds: 0.0,
                functions: Vec::new(),
            });
        }
    };
    // Errors and refusals so far (loading, logging in) belong to no test:
    // they fail the file, Blizzard's code's included (it runs without an
    // error in the client).
    let _ = natives::unread_refusals(vm);
    let pending = natives::unread_errors();
    if !pending.is_empty() {
        result.error = Some(to_sources(
            suite,
            &format!(
                "an error reached the error handler while loading:\n{}",
                pending.join("\n")
            ),
        ));
        return;
    }
    let dropped = natives::unread_dropped();
    if !dropped.is_empty() {
        result.error = Some(to_sources(
            suite,
            &format!("{DROPPED_FAILURE} while loading:\n{}", dropped.join("\n")),
        ));
        return;
    }
    match start(vm, tests, "beforeAll", None, None) {
        Ok(job) => {
            if let Outcome::Fail { message, .. } = drive(vm, tests, job) {
                fail_all(
                    result,
                    &planned,
                    to_sources(suite, &format!("@beforeAll failed: {message}")),
                );
                return;
            }
        }
        Err(e) => {
            fail_all(result, &planned, e);
            return;
        }
    }
    for p in &planned {
        if let Some(reason) = &p.skip {
            result.tests.push(TestResult {
                name: p.name.clone(),
                outcome: Outcome::Skip(reason.clone()),
                seconds: 0.0,
                functions: Vec::new(),
            });
            continue;
        }
        let started = wowlua_host::Stopwatch::start();
        let before = suite.coverage.then(|| entry_counts(&state.vm));
        let mut outcome = run_one(suite, file, state, p);
        let functions = before
            .map(|b| functions_ran(suite, &state.vm, &b))
            .unwrap_or_default();
        let vm = &mut state.vm;
        if let Some(f) = &suite.after_test {
            f(vm, &p.name);
        }
        // An error no test read (`mock.errors()`) fails the test, and so
        // does a protected action refused because of taint (with its taint
        // chain) that no test expected (`toBeBlocked`, `mock.refusals()`).
        let unread = natives::unread_errors();
        let dropped = natives::unread_dropped();
        let refused = natives::unread_refusals(vm);
        if outcome == Outcome::Pass && !unread.is_empty() {
            outcome = Outcome::Fail {
                message: format!("an error reached the error handler: {}", unread.join("\n")),
                stack: String::new(),
            };
        } else if outcome == Outcome::Pass && !dropped.is_empty() {
            outcome = Outcome::Fail {
                message: format!("{DROPPED_FAILURE}: {}", dropped.join("\n")),
                stack: String::new(),
            };
        } else if outcome == Outcome::Pass && !refused.is_empty() {
            let (action, chain) = &refused[0];
            outcome = Outcome::Fail {
                message: format!("the client refused {action} because of taint:\n{chain}"),
                stack: String::new(),
            };
        }
        if let Outcome::Fail { message, stack } = outcome {
            outcome = Outcome::Fail {
                message: to_sources(suite, &message),
                stack: to_sources(suite, &stack),
            };
        }
        result.tests.push(TestResult {
            name: p.name.clone(),
            outcome,
            seconds: started.elapsed().as_secs_f64(),
            functions,
        });
    }
    let vm = &mut state.vm;
    if let Ok(job) = start(vm, tests, "afterAll", None, None)
        && let Outcome::Fail { message, .. } = drive(vm, tests, job)
    {
        result.error = Some(to_sources(suite, &format!("@afterAll failed: {message}")));
    }
}

/// Where test builds go: `<out>/.test/<flavor>/<Name>`.
pub fn build_dir(out: &Path, flavor: &str, addon: &str) -> PathBuf {
    out.join(".test").join(flavor).join(addon)
}

#[cfg(test)]
mod filter_tests {
    use super::selects;

    #[test]
    fn a_filter_selects_by_text_or_exactly() {
        let a = "tests/a.test.wowlua";
        assert!(selects("count", a, "counts clicks", "counts clicks"));
        assert!(selects("a.test", a, "anything", "anything"));
        assert!(selects(
            "tests/a.test.wowlua::counts",
            a,
            "counts",
            "counts"
        ));
        assert!(!selects(
            "tests/a.test.wowlua::counts",
            a,
            "counts clicks",
            "counts clicks"
        ));
        assert!(!selects(
            "tests/b.test.wowlua::counts",
            a,
            "counts",
            "counts"
        ));
        // every case of a `@cases` test
        assert!(selects(
            "tests/a.test.wowlua::adds",
            a,
            "adds [1, 2]",
            "adds"
        ));
        // an absolute path ends with the file's
        assert!(selects(
            "/home/u/addon/tests/a.test.wowlua::counts",
            a,
            "counts",
            "counts"
        ));
    }
}
