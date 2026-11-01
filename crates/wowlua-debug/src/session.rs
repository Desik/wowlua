//! Sessions (spec debugger §18.3): a test file run as `wowlua test` runs
//! it, or the addon loaded with the player logged in and the game's frames
//! running, in real time or as asked; each with a [`Controller`] on every
//! VM it makes. Native: a session runs on its own thread.

use std::cell::RefCell;
use std::rc::Rc;
use std::sync::Arc;
use std::sync::atomic::AtomicBool;
use std::sync::mpsc::{Receiver, RecvTimeoutError, Sender};
use std::time::{Duration, Instant};

use serde_json::Value as Json;
use wowlua_test::{Output, Suite};
use wowlua_vm::Vm;

use crate::controller::{Controller, Link, Screenshotter, Shared, SharedRef};
use crate::protocol::*;

/// What a session runs.
#[derive(Clone, Debug)]
pub enum Target {
    /// A test file (its path relative to the package root) and the tests
    /// whose names contain any of `filters`.
    Test {
        file: String,
        filters: Vec<String>,
    },
    Addon,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ClockMode {
    /// A game frame every 1/60 s of wall time.
    Realtime,
    /// Time moves when asked (`Advance`).
    Manual,
}

#[derive(Clone, Debug)]
pub struct Options {
    pub target: Target,
    pub clock: ClockMode,
    pub stop_on_entry: bool,
    pub debug_blizzard_ui: bool,
    pub flavor: Option<String>,
    /// The world profile (`tests/profiles/<name>.json`) the session starts
    /// in, in place of the manifest's `test.profile`.
    pub profile: Option<String>,
}

/// A path without Windows' verbatim prefix (`\\?\C:\x` is `C:\x`), so
/// a canonical path and a plain one compare.
pub(crate) fn verbatim_free(p: &std::path::Path) -> std::path::PathBuf {
    let text = p.to_string_lossy();
    match text.strip_prefix(r"\\?\") {
        Some(rest) if !rest.starts_with("UNC\\") => std::path::PathBuf::from(rest),
        _ => p.to_path_buf(),
    }
}

impl Options {
    /// From DAP's `launch` arguments: `test` (a path) and `filter`, or
    /// `addon: true`; `clock`, `stopOnEntry`, `debugBlizzardUI`, `flavor`,
    /// `profile`.
    pub fn from_launch(
        args: &Json,
        root: &std::path::Path,
        default_clock: ClockMode,
    ) -> Result<Options, String> {
        let target = match args.get("test").and_then(Json::as_str) {
            Some(test) => {
                // (Windows' `\\?\C:\…` paths and plain ones alike)
                let path = verbatim_free(std::path::Path::new(test));
                let root = verbatim_free(root);
                let rel = path.strip_prefix(&root).unwrap_or(&path);
                Target::Test {
                    file: rel.to_string_lossy().replace('\\', "/"),
                    filters: match args.get("filter") {
                        Some(Json::String(s)) => vec![s.clone()],
                        Some(Json::Array(a)) => a
                            .iter()
                            .filter_map(|v| v.as_str().map(str::to_string))
                            .collect(),
                        _ => Vec::new(),
                    },
                }
            }
            None if args.get("addon").and_then(Json::as_bool) == Some(true) => Target::Addon,
            None => return Err("launch needs `test` (a test file) or `addon: true`".into()),
        };
        let clock = match args.get("clock").and_then(Json::as_str) {
            Some("manual") => ClockMode::Manual,
            Some("realtime") => ClockMode::Realtime,
            Some(other) => {
                return Err(format!(
                    "clock is \"realtime\" or \"manual\", not \"{other}\""
                ));
            }
            None => default_clock,
        };
        Ok(Options {
            target,
            clock,
            stop_on_entry: args
                .get("stopOnEntry")
                .and_then(Json::as_bool)
                .unwrap_or(false),
            debug_blizzard_ui: args
                .get("debugBlizzardUI")
                .and_then(Json::as_bool)
                .unwrap_or(false),
            flavor: args
                .get("flavor")
                .and_then(Json::as_str)
                .map(str::to_string),
            profile: args
                .get("profile")
                .and_then(Json::as_str)
                .filter(|p| !p.is_empty())
                .map(str::to_string),
        })
    }
}

/// What a session sends to its front-end.
#[derive(Debug)]
pub enum Outgoing {
    Response(u64, Result<Response, String>),
    Event(Event),
}

/// A [`Link`] over channels: the front-end's thread on the other side.
pub struct ChannelLink {
    pub rx: Receiver<(u64, Request)>,
    pub tx: Sender<Outgoing>,
    gone: bool,
}

impl ChannelLink {
    pub fn new(rx: Receiver<(u64, Request)>, tx: Sender<Outgoing>) -> ChannelLink {
        ChannelLink {
            rx,
            tx,
            gone: false,
        }
    }
}

impl Link for ChannelLink {
    fn recv(&mut self, timeout: Option<Duration>) -> Option<(u64, Request)> {
        if self.gone {
            return None;
        }
        let r = match timeout {
            None => self.rx.recv().map_err(|_| RecvTimeoutError::Disconnected),
            Some(t) if t.is_zero() => self.rx.try_recv().map_err(|e| match e {
                std::sync::mpsc::TryRecvError::Empty => RecvTimeoutError::Timeout,
                std::sync::mpsc::TryRecvError::Disconnected => RecvTimeoutError::Disconnected,
            }),
            Some(t) => self.rx.recv_timeout(t),
        };
        match r {
            Ok(m) => Some(m),
            Err(RecvTimeoutError::Timeout) => None,
            Err(RecvTimeoutError::Disconnected) => {
                self.gone = true;
                None
            }
        }
    }

    fn respond(&mut self, id: u64, r: Result<Response, String>) {
        let _ = self.tx.send(Outgoing::Response(id, r));
    }

    fn event(&mut self, e: Event) {
        let _ = self.tx.send(Outgoing::Event(e));
    }

    fn gone(&self) -> bool {
        self.gone
    }
}

thread_local! {
    static SESSION: RefCell<Option<SharedRef>> = const { RefCell::new(None) };
    /// Output while the session's state was busy, sent with the next.
    static PENDING: RefCell<Vec<(String, String)>> = const { RefCell::new(Vec::new()) };
}

/// The test runner's `Suite::instrument` for debug sessions: attaches the
/// running session's controller to each VM it makes.
pub fn instrument(vm: &mut Vm) {
    SESSION.with(|s| {
        if let Some(shared) = s.borrow().as_ref() {
            Controller::attach(vm, shared);
        }
    });
}

fn send_output(shared: &SharedRef, category: &str, text: String) {
    match shared.try_borrow_mut() {
        Ok(mut s) => {
            let pending = PENDING.with(|p| std::mem::take(&mut *p.borrow_mut()));
            for (c, t) in pending {
                s.output(&c, &t);
            }
            s.output(category, &text);
        }
        Err(_) => PENDING.with(|p| p.borrow_mut().push((category.to_string(), text))),
    }
}

/// The host's monotonic clock in nanoseconds.
fn host_clock() -> crate::controller::Clock {
    let start = Instant::now();
    Rc::new(move || start.elapsed().as_nanos() as u64)
}

/// Serves the requests that come before the session runs: breakpoints,
/// until `ConfigurationDone`. False when the front-end went away.
fn configure(shared: &SharedRef) -> bool {
    shared.borrow_mut().link.event(Event::Initialized);
    loop {
        let next = shared.borrow_mut().link.recv(None);
        let Some((id, req)) = next else {
            if shared.borrow().link.gone() {
                return false;
            }
            continue;
        };
        let mut s = shared.borrow_mut();
        let r = match req {
            Request::SetBreakpoints {
                source,
                breakpoints,
            } => Ok(Response::Breakpoints {
                breakpoints: s.record_lines(&source, breakpoints),
            }),
            Request::SetFunctionBreakpoints { breakpoints } => Ok(Response::Breakpoints {
                breakpoints: s.record_functions(breakpoints),
            }),
            Request::SetExceptionBreakpoints { filters } => {
                s.record_filters(&filters);
                Ok(Response::Ok)
            }
            Request::ConfigurationDone => {
                s.configured = true;
                s.link.respond(id, Ok(Response::Ok));
                return true;
            }
            Request::Terminate => {
                s.terminate = true;
                s.link.respond(id, Ok(Response::Ok));
                return false;
            }
            Request::StackTrace => Ok(Response::StackTrace { frames: Vec::new() }),
            _ => Err("the session has not started".into()),
        };
        s.link.respond(id, r);
    }
}

pub use crate::controller::FrameHook;

/// Runs a session on this thread until it ends; the suite's `instrument`
/// must be [`instrument`]. Tells the front-end `Terminated` at the end.
pub fn run(
    suite: &Suite,
    opts: &Options,
    link: Box<dyn Link>,
    interrupt: Arc<AtomicBool>,
    screenshot: Option<Screenshotter>,
) {
    run_hosted(suite, opts, link, interrupt, screenshot, None);
}

/// [`run`] with a host's [`FrameHook`] (an addon session's).
pub fn run_hosted(
    suite: &Suite,
    opts: &Options,
    link: Box<dyn Link>,
    interrupt: Arc<AtomicBool>,
    screenshot: Option<Screenshotter>,
    on_frame: Option<FrameHook>,
) {
    let shared = Shared::new(link, wowlua_test::chunk_mapper(suite), host_clock());
    {
        let mut s = shared.borrow_mut();
        s.files = Some(wowlua_test::chunk_file_mapper(suite));
        s.interrupt = interrupt;
        s.screenshot = screenshot;
        s.debug_blizzard_ui = opts.debug_blizzard_ui;
        s.frame_hook = on_frame;
        s.drives_time = matches!(opts.target, Target::Addon);
    }
    if !configure(&shared) {
        shared.borrow_mut().link.event(Event::Terminated {
            summary: None,
            failed: false,
        });
        return;
    }
    if opts.stop_on_entry {
        shared.borrow_mut().stop_on_entry();
    }
    // Which flavor runs: a package has several, the session one.
    let what = match &opts.target {
        Target::Test { file, .. } => file.clone(),
        Target::Addon => "the addon".into(),
    };
    send_output(
        &shared,
        "console",
        format!(
            "Debugging {what} on {} (the launch configuration's \"flavor\" chooses)\n",
            suite.flavor
        ),
    );
    SESSION.with(|s| *s.borrow_mut() = Some(shared.clone()));
    let tee = shared.clone();
    wowlua_test::set_tee(Some(Box::new(move |o: Output| match o {
        // A chat line as the chat frame shows it: no colour codes, links'
        // codes or textures, `|4day:days;` its word.
        Output::Printed(text) => send_output(&tee, "stdout", wowlua_engine::text::plain(&text)),
        Output::Error { message, stack } => {
            send_output(&tee, "stderr", format!("{message}\n{stack}\n"))
        }
    })));
    let (summary, failed) = match &opts.target {
        Target::Test { file, filters } => run_test(suite, file, filters),
        Target::Addon => run_addon(suite, opts, &shared),
    };
    wowlua_test::set_tee(None);
    SESSION.with(|s| *s.borrow_mut() = None);
    let mut s = shared.borrow_mut();
    if let Some(text) = &summary {
        s.output("console", &format!("{text}\n"));
    }
    s.link.event(Event::Terminated { summary, failed });
}

fn run_test(suite: &Suite, file: &str, filters: &[String]) -> (Option<String>, bool) {
    let Some(test) = suite.build.tests.iter().find(|t| t.rel == file) else {
        return (
            Some(format!("{file} is not a test file of the package")),
            true,
        );
    };
    let result = wowlua_test::run_file(suite, test, filters);
    let mut lines = Vec::new();
    if let Some(e) = &result.error {
        lines.push(format!("{file}: {e}"));
    }
    let (mut passed, mut failed, mut skipped) = (0, 0, 0);
    for t in &result.tests {
        match &t.outcome {
            wowlua_test::Outcome::Pass => passed += 1,
            wowlua_test::Outcome::Skip(_) => skipped += 1,
            wowlua_test::Outcome::Fail { message, .. } => {
                failed += 1;
                lines.push(format!("✗ {}: {message}", t.name));
            }
        }
    }
    lines.push(format!(
        "{passed} passed, {failed} failed, {skipped} skipped"
    ));
    (Some(lines.join("\n")), result.failed())
}

const FRAME_MS: f64 = 1000.0 / 60.0;
/// The longest a realtime frame moves the world (milliseconds): a longer
/// stall moves it this far (unverified: the client's own limit).
const MAX_FRAME_MS: f64 = 250.0;
/// How long a realtime frame lasts at least (milliseconds), as the game's
/// settings pace it (decision T538): `maxFPS` in the foreground,
/// `maxFPSBk` in the background (0: no limit), with `vsync` on at most
/// the display's rate (`display_hz`, as the host tells it: T544).
fn frame_ms(vm: &mut wowlua_vm::Vm, background: bool, display_hz: f64) -> f64 {
    let mut number = |name: &str| {
        wowlua_engine::framescript::cvar_get(vm, name).and_then(|v| v.trim().parse::<f64>().ok())
    };
    let limit = if background {
        number("maxFPSBk").or_else(|| number("maxFPS"))
    } else {
        number("maxFPS")
    };
    let vsync = number("vsync").is_none_or(|v| v != 0.0);
    let mut fps: f64 = 1000.0;
    if let Some(l) = limit.filter(|l| *l > 0.0) {
        fps = fps.min(l);
    }
    if vsync {
        fps = fps.min(display_hz);
    }
    1000.0 / fps.max(1.0)
}

/// When a realtime frame starts, for the rate (decision T548): when the
/// last one planned it (`planned`, its start plus a frame), so a wait that
/// ends late does not delay the frames after it, unless the frame is a
/// whole frame late (a slow frame, a stop): then now.
fn frame_start(planned: Option<Instant>, now: Instant, frame: Duration) -> Instant {
    match planned {
        Some(t) if now.saturating_duration_since(t) < frame => t,
        _ => now,
    }
}

fn run_addon(suite: &Suite, opts: &Options, shared: &SharedRef) -> (Option<String>, bool) {
    let mut session = match wowlua_test::AddonSession::open(suite) {
        Ok(s) => s,
        Err(e) => return (Some(e), true),
    };
    let mut last_frame: Option<Instant> = None;
    // the next frame's planned start (T548)
    let mut next_frame: Option<Instant> = None;
    let mut last_clock = opts.clock;
    loop {
        // A `/reload` (`ReloadUI()`) ended the addon's session: the UI loads
        // again, its SavedVariables kept, the debugger on the new one.
        if session.wants_reload() {
            send_output(shared, "console", "Reloading the UI\n".into());
            if let Err(e) = session.reload(suite) {
                return (Some(format!("reloading the UI: {e}")), true);
            }
            shared.borrow_mut().link.event(Event::Reloaded);
        }
        let vm = &mut session.vm;
        vm.with_debugger(|d, vm| d.idle(vm));
        if shared.borrow().terminate || vm.aborted() {
            break;
        }
        // Input and events run here, the debugger attached.
        let queued = std::mem::take(&mut shared.borrow_mut().run_queue);
        for (id, req) in queued {
            let r = match &req {
                Request::Input { input } => crate::ui::input(vm, input).map(|()| Response::Ok),
                Request::FireEvent { event, args } => {
                    crate::ui::fire_event(vm, event, args).map(|()| Response::Ok)
                }
                Request::World { script } => crate::ui::world(vm, script).map(|()| Response::Ok),
                _ => Err("not a request that runs code".into()),
            };
            shared.borrow_mut().link.respond(id, r);
        }
        if shared.borrow().terminate || vm.aborted() {
            break;
        }
        let advance = shared.borrow_mut().advance.take();
        if let Some((id, seconds, frames)) = advance {
            let n = match (frames, seconds) {
                (Some(f), _) => f as usize,
                (None, Some(s)) => (s * 1000.0 / FRAME_MS).ceil() as usize,
                (None, None) => 1,
            };
            wowlua_engine::clock::advance_frames(vm, n, FRAME_MS);
            shared.borrow_mut().link.respond(id, Ok(Response::Ok));
            continue;
        }
        // The clock a front-end chose meanwhile over the launch's; going
        // realtime again counts frames from now.
        let clock = match shared.borrow().manual {
            Some(true) => ClockMode::Manual,
            Some(false) => ClockMode::Realtime,
            None => opts.clock,
        };
        if clock != last_clock {
            last_clock = clock;
            last_frame = None;
            next_frame = None;
        }
        match clock {
            ClockMode::Realtime => {
                // (a frame as long as it really took, as the game's: a slow
                // frame moves the world as far as its time, not less;
                // decision T538)
                let now = Instant::now();
                let ms = last_frame
                    .map_or(FRAME_MS, |t| (now - t).as_secs_f64() * 1000.0)
                    .clamp(1.0, MAX_FRAME_MS);
                last_frame = Some(now);
                wowlua_engine::clock::advance_frames(vm, 1, ms);
                Shared::run_frame_hook(shared, vm);
                let (background, display_hz) = {
                    let s = shared.borrow();
                    (s.background, s.display_hz)
                };
                let frame = Duration::from_secs_f64(frame_ms(vm, background, display_hz) / 1000.0);
                let deadline = frame_start(next_frame, now, frame) + frame;
                next_frame = Some(deadline);
                // Requests that come before the next frame wait for it.
                loop {
                    let now = Instant::now();
                    if now >= deadline {
                        break;
                    }
                    let next = shared.borrow_mut().link.recv(Some(deadline - now));
                    match next {
                        Some(r) => {
                            shared.borrow_mut().deferred.push(r);
                            break;
                        }
                        None if shared.borrow().link.gone() => {
                            shared.borrow_mut().terminate = true;
                            break;
                        }
                        None => {}
                    }
                }
            }
            ClockMode::Manual => {
                Shared::run_frame_hook(shared, vm);
                // Nothing moves until asked.
                let next = shared.borrow_mut().link.recv(None);
                match next {
                    Some(r) => shared.borrow_mut().deferred.push(r),
                    None if shared.borrow().link.gone() => shared.borrow_mut().terminate = true,
                    None => {}
                }
            }
        }
    }
    (Some("the session ended".into()), false)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_verbatim_test_path_is_the_package_s() {
        let root = std::path::Path::new(r"C:\pkg");
        let args = serde_json::json!({ "test": r"\\?\C:\pkg\tests\a.test.wowlua" });
        let opts = Options::from_launch(&args, root, ClockMode::Manual).expect("options");
        match opts.target {
            Target::Test { file, .. } if cfg!(windows) => assert_eq!(file, "tests/a.test.wowlua"),
            Target::Test { .. } => {}
            Target::Addon => panic!("a test"),
        }
        assert_eq!(
            verbatim_free(std::path::Path::new(r"\\?\C:\x")),
            std::path::PathBuf::from(r"C:\x")
        );
    }

    /// A realtime frame lasts as the game's settings say (decisions T538,
    /// T544): `maxFPS`, `maxFPSBk` in the background, at most the
    /// display's rate with `vsync` on.
    #[test]
    fn frames_keep_to_the_settings_and_the_display() {
        let mut vm = wowlua_vm::Vm::wow();
        wowlua_engine::install(&mut vm, "camelot").expect("camelot");
        let set = |vm: &mut wowlua_vm::Vm, name: &str, value: &str| {
            let src = format!("C_CVar.SetCVar('{name}', '{value}')");
            let f = vm.load(src.as_bytes(), "@Test.lua").expect("compiles");
            vm.call(f, &[]).expect("runs");
        };
        let fps = |ms: f64| (1000.0 / ms).round();
        set(&mut vm, "maxFPS", "120");
        set(&mut vm, "maxFPSBk", "30");
        set(&mut vm, "vsync", "1");
        assert_eq!(fps(frame_ms(&mut vm, false, 60.0)), 60.0);
        assert_eq!(fps(frame_ms(&mut vm, false, 144.0)), 120.0);
        assert_eq!(fps(frame_ms(&mut vm, true, 144.0)), 30.0);
        set(&mut vm, "vsync", "0");
        set(&mut vm, "maxFPS", "0");
        assert_eq!(fps(frame_ms(&mut vm, false, 60.0)), 1000.0);
    }

    /// Frames keep their rate though each wait for the next one ends late
    /// (decision T548): 120 frames of a 120 fps limit last a second, not
    /// 120 times a frame and the lateness.
    #[test]
    fn late_waits_do_not_slow_the_frames() {
        let frame = Duration::from_secs_f64(1.0 / 120.0);
        let late = Duration::from_micros(700);
        let t0 = Instant::now();
        let (mut planned, mut now) = (None, t0);
        for _ in 0..120 {
            let start = frame_start(planned, now, frame);
            planned = Some(start + frame);
            now = start + frame + late;
        }
        let took = (planned.expect("planned") - t0).as_secs_f64();
        assert!((took - 1.0).abs() < 0.001, "{took}");
        // a frame a whole frame late starts the count again
        let at = t0 + frame * 3;
        assert_eq!(frame_start(Some(t0), at, frame), at);
    }
}
