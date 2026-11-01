//! The controller (spec debugger §18.3 to §18.7): the VM's [`Debugger`],
//! one per VM of a session, sharing the session's [`Shared`] state
//! (breakpoints, the step, the link to the front-end) with the others a
//! reload makes and with the session's driver.

use std::cell::RefCell;
use std::collections::{BTreeMap, HashMap};
use std::rc::Rc;
use std::sync::Arc;
use std::sync::atomic::AtomicBool;

use wowlua_vm::debug::{Debugger, Dispatch, DispatchKind, LineCause, Step};
use wowlua_vm::heap::FuncRef;
use wowlua_vm::proto::Proto;
use wowlua_vm::{LuaError, TableRef, Value, Vm};

use crate::protocol::*;
use crate::values::{self, RefTarget};

/// How requests reach a session and its answers and events leave it.
pub trait Link {
    /// The next request: waits for one (`timeout`: at most that long;
    /// `None`: no request in time, or the front-end is gone, see
    /// [`gone`](Self::gone)).
    fn recv(&mut self, timeout: Option<std::time::Duration>) -> Option<(u64, Request)>;
    fn respond(&mut self, id: u64, r: Result<Response, String>);
    fn event(&mut self, e: Event);
    /// The front-end disconnected.
    fn gone(&self) -> bool;
}

/// Chunk name → the package's source path.
pub type Mapper = Box<dyn Fn(&str) -> Option<String>>;

/// Host time in nanoseconds (monotonic): the profiler's.
pub type Clock = Rc<dyn Fn() -> u64>;

/// Draws a screenshot: the PNG, its width and height (the renderer's,
/// given by the host).
pub type Screenshotter =
    Box<dyn FnMut(&mut Vm, Option<&str>, Option<f64>) -> Result<(Vec<u8>, u32, u32), String>>;

/// A line breakpoint and how often it was hit.
pub(crate) struct LineBp {
    pub id: u32,
    pub spec: LineBreakpoint,
    pub hits: u32,
    /// The line it took once its chunk loaded.
    pub actual: Option<u32>,
}

/// What a function breakpoint stops at.
#[derive(Clone, Debug)]
pub(crate) enum FuncTarget {
    /// A Lua or native function at this path from the globals
    /// (`A.b.c`, `A:m`).
    Path(Vec<String>),
    /// The Lua function defined at a line of a source (`src/x.wowlua:560`,
    /// as profiles name it; decision T495).
    Defined {
        source: String,
        line: u32,
    },
    Event(Option<String>),
    Script {
        frame: Option<String>,
        script: String,
    },
    Timer,
}

pub(crate) struct FuncBp {
    pub id: u32,
    pub spec: FunctionBreakpoint,
    pub target: FuncTarget,
    /// How often it stopped.
    pub hits: u32,
}

#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct Filters {
    pub uncaught: bool,
    pub all: bool,
    pub taint: bool,
    pub blocked: bool,
}

/// A step in progress, as the front-end asked it.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Stepping {
    /// Out of a frame the engine called: once the dispatch it is in
    /// ends (fewer dispatches than this), the step becomes "the next line
    /// at the frame's depth".
    pub out_of_dispatch: Option<usize>,
    pub depth: usize,
    pub thread: Option<wowlua_vm::heap::ThreadRef>,
    pub pause: bool,
    /// The session's start (`stopOnEntry`): the package's first line.
    pub entry: bool,
}

/// The session's state every controller and the driver share.
pub struct Shared {
    pub link: Box<dyn Link>,
    /// Chunk name → source path (relative to the package root), for the
    /// package's own files.
    pub mapper: Mapper,
    /// Chunk name → a file on disk, for code that is not the package's
    /// (Blizzard's UI, other addons): editors open it at its frames.
    pub files: Option<Mapper>,
    pub clock: Clock,
    pub screenshot: Option<Screenshotter>,
    /// The flag the front-end's thread sets with each request it sends,
    /// shared by the session's VMs.
    pub interrupt: Arc<AtomicBool>,
    pub(crate) lines: BTreeMap<String, Vec<LineBp>>,
    pub(crate) funcs: Vec<FuncBp>,
    pub(crate) filters: Filters,
    pub(crate) next_id: u32,
    pub(crate) stepping: Option<Stepping>,
    /// Requests a running session serves between frames (its UI's).
    pub deferred: Vec<(u64, Request)>,
    /// Requests that run the addon's code (input, events), for the driver
    /// to run with the debugger attached: their handlers' breakpoints stop.
    pub run_queue: Vec<(u64, Request)>,
    /// An `Advance` the driver runs: its request, seconds, frames.
    pub advance: Option<(u64, Option<f64>, Option<u32>)>,
    /// What was printed and raised, for `Output` requests (MCP).
    pub output: Vec<OutputLine>,
    pub debug_blizzard_ui: bool,
    /// The driver moves time (an addon session); a test session's time is
    /// the test's.
    pub drives_time: bool,
    /// The session should end.
    pub terminate: bool,
    /// The configuration is done (`ConfigurationDone`).
    pub configured: bool,
    /// The session is paused between frames (by the driver).
    pub idle_paused: bool,
    /// The next stop's exception details.
    pub detail: Option<StopDetail>,
    /// The clock a front-end chose while running (`SetClock`): manual or
    /// not, over the launch's.
    pub manual: Option<bool>,
    /// The game in the background (`SetForeground`): its frame rate
    /// `maxFPSBk`'s (decision T538).
    pub background: bool,
    /// The display's refresh rate (`SetDisplayRate`): `vsync`'s cap
    /// (decision T544).
    pub display_hz: f64,
    /// A host's frame hook (`session::run_hosted`): run whenever the
    /// client may have changed, a stop too (decision T494).
    pub frame_hook: Option<FrameHook>,
}

/// What a host runs on the session's thread with the client's VM whenever
/// the client may have changed (after each game frame, an advance,
/// requests served, a stop): Studio builds the scene it draws.
pub type FrameHook = Box<dyn FnMut(&mut Vm)>;

impl Shared {
    /// Runs the host's frame hook, if it has one.
    pub fn run_frame_hook(shared: &SharedRef, vm: &mut Vm) {
        let hook = shared.borrow_mut().frame_hook.take();
        if let Some(mut hook) = hook {
            hook(vm);
            shared.borrow_mut().frame_hook = Some(hook);
        }
    }
}

pub type SharedRef = Rc<RefCell<Shared>>;

impl Shared {
    pub fn new(link: Box<dyn Link>, mapper: Mapper, clock: Clock) -> SharedRef {
        Rc::new(RefCell::new(Shared {
            link,
            mapper,
            files: None,
            clock,
            screenshot: None,
            interrupt: Arc::new(AtomicBool::new(false)),
            lines: BTreeMap::new(),
            funcs: Vec::new(),
            filters: Filters::default(),
            next_id: 1,
            stepping: None,
            deferred: Vec::new(),
            run_queue: Vec::new(),
            advance: None,
            output: Vec::new(),
            debug_blizzard_ui: false,
            drives_time: false,
            terminate: false,
            configured: false,
            idle_paused: false,
            detail: None,
            manual: None,
            background: false,
            display_hz: 60.0,
            frame_hook: None,
        }))
    }

    /// Keeps a source's line breakpoints (placed when its chunks load).
    pub fn record_lines(
        &mut self,
        source: &str,
        specs: Vec<LineBreakpoint>,
    ) -> Vec<BreakpointState> {
        let mut list = Vec::new();
        for spec in specs {
            let id = self.next_id;
            self.next_id += 1;
            list.push(LineBp {
                id,
                spec,
                hits: 0,
                actual: None,
            });
        }
        let states = list.iter().map(state_of).collect();
        self.lines.insert(source.to_string(), list);
        states
    }

    /// Keeps the function breakpoints (resolved as the code defines them).
    pub fn record_functions(&mut self, specs: Vec<FunctionBreakpoint>) -> Vec<BreakpointState> {
        self.funcs.clear();
        let mut out = Vec::new();
        for spec in specs {
            let id = self.next_id;
            self.next_id += 1;
            let target = parse_target(&spec.name);
            out.push(BreakpointState {
                id,
                verified: !matches!(target, FuncTarget::Path(_) | FuncTarget::Defined { .. }),
                line: None,
                message: None,
                hits: 0,
                source: None,
            });
            self.funcs.push(FuncBp {
                id,
                spec,
                target,
                hits: 0,
            });
        }
        out
    }

    pub fn record_filters(&mut self, filters: &[String]) {
        let mut f = Filters::default();
        for name in filters {
            match name.as_str() {
                "uncaught" => f.uncaught = true,
                "all" => f.all = true,
                "taint" => f.taint = true,
                "blocked" => f.blocked = true,
                _ => {}
            }
        }
        self.filters = f;
    }

    /// The session stops at the package's first line.
    pub fn stop_on_entry(&mut self) {
        self.stepping = Some(Stepping {
            out_of_dispatch: None,
            depth: usize::MAX,
            thread: None,
            pause: false,
            entry: true,
        });
    }

    /// Sends output to the front-end (and keeps it for `Output`).
    pub fn output(&mut self, category: &str, text: &str) {
        let line = OutputLine {
            category: category.into(),
            text: text.into(),
            source: None,
            line: None,
        };
        self.output.push(line.clone());
        if self.output.len() > 10_000 {
            self.output.drain(..5_000);
        }
        self.link.event(Event::Output { line });
    }
}

/// What the stopped session's requests refer to: its frames and the
/// values their variables expand to. Valid until it runs on.
pub(crate) struct Paused {
    pub frames: Vec<FrameRef>,
    pub refs: Vec<RefTarget>,
    /// Keeps the values `refs` point to alive while stopped.
    pub pins: TableRef,
}

#[derive(Clone, Debug)]
pub(crate) enum FrameRef {
    /// A Lua frame of the running thread (its index there).
    Lua {
        index: usize,
        func: FuncRef,
    },
    Native,
    Dispatch,
}

/// What a request handled while stopped asks for next.
enum Flow {
    Stay,
    Resume,
}

/// The VM's debugger: one per VM.
pub struct Controller {
    shared: SharedRef,
    /// The package's chunks loaded, by source path: breakpoints mark
    /// their functions.
    chunks: HashMap<String, Vec<Rc<Proto>>>,
    /// Function breakpoints' functions as last resolved.
    resolved: Vec<(u32, FuncRef)>,
    /// Function breakpoints by definition: their prototypes (pointers into
    /// `chunks`) as last resolved.
    resolved_protos: Vec<(u32, usize)>,
}

const PINS_KEY: &str = "wowlua.debugger.pins";

impl Controller {
    /// Attaches a controller to `vm`.
    pub fn attach(vm: &mut Vm, shared: &SharedRef) {
        let c = Controller {
            shared: shared.clone(),
            chunks: HashMap::new(),
            resolved: Vec::new(),
            resolved_protos: Vec::new(),
        };
        vm.attach_debugger(Box::new(c));
        vm.set_interrupt_handle(shared.borrow().interrupt.clone());
        let f = shared.borrow().filters;
        vm.watch_taint(f.taint);
        // A step waiting from before this VM (the entry, a pause).
        if shared.borrow().stepping.is_some() {
            let (thread, _) = vm.depth();
            if let Some(s) = shared.borrow_mut().stepping.as_mut() {
                s.thread = Some(thread);
            }
            vm.set_step(Some(Step {
                thread: None,
                max_depth: usize::MAX,
            }));
        }
    }

    fn shared(&self) -> std::cell::RefMut<'_, Shared> {
        self.shared.borrow_mut()
    }

    // ------------------------------------------------------- breakpoints

    /// The source path of a chunk, if it is the package's.
    fn source_of(&self, chunk: &[u8]) -> Option<String> {
        let chunk = String::from_utf8_lossy(chunk);
        (self.shared.borrow().mapper)(&chunk)
    }

    fn set_line_breakpoints(
        &mut self,
        vm: &mut Vm,
        source: String,
        specs: Vec<LineBreakpoint>,
    ) -> Vec<BreakpointState> {
        self.shared().record_lines(&source, specs);
        // Unmarked first: the functions of the breakpoints taken out.
        if let Some(protos) = self.chunks.get(&source).cloned() {
            for p in &protos {
                vm.mark_breakpoints(p, &mut |_| false);
            }
        }
        self.place(vm, &source);
        let s = self.shared.borrow();
        s.lines[&source].iter().map(state_of).collect()
    }

    /// Places a source's breakpoints on its loaded chunks: each on the
    /// first line with code at or after its own, the functions with one
    /// marked.
    fn place(&mut self, vm: &mut Vm, source: &str) {
        let Some(protos) = self.chunks.get(source).cloned() else {
            return;
        };
        let mut code_lines: Vec<u32> = Vec::new();
        for p in &protos {
            collect_lines(p, &mut code_lines);
        }
        code_lines.sort_unstable();
        code_lines.dedup();
        let mut wanted: Vec<u32> = Vec::new();
        {
            let mut s = self.shared();
            if let Some(list) = s.lines.get_mut(source) {
                for bp in list.iter_mut() {
                    bp.actual = code_lines.iter().copied().find(|&l| l >= bp.spec.line);
                    if let Some(l) = bp.actual {
                        wanted.push(l);
                    }
                }
            }
        }
        for p in &protos {
            vm.mark_breakpoints(p, &mut |q: &Proto| {
                q.lines.iter().any(|l| wanted.contains(l))
            });
        }
    }

    fn set_function_breakpoints(
        &mut self,
        vm: &mut Vm,
        specs: Vec<FunctionBreakpoint>,
    ) -> Vec<BreakpointState> {
        self.shared().record_functions(specs);
        self.resolved.clear();
        vm.set_function_breakpoints([]);
        self.resolve_functions(vm);
        let s = self.shared.borrow();
        s.funcs
            .iter()
            .map(|f| {
                let verified =
                    !matches!(f.target, FuncTarget::Path(_) | FuncTarget::Defined { .. })
                        || self.resolved.iter().any(|(id, _)| *id == f.id)
                        || self.resolved_protos.iter().any(|(id, _)| *id == f.id);
                BreakpointState {
                    id: f.id,
                    verified,
                    line: None,
                    message: (!verified).then(|| format!("{} is not defined yet", f.spec.name)),
                    hits: f.hits,
                    source: None,
                }
            })
            .collect()
    }

    /// Looks the function breakpoints' paths up again (after a load, at a
    /// dispatch): the VM stops in the functions they name now.
    fn resolve_functions(&mut self, vm: &mut Vm) {
        let paths: Vec<(u32, Vec<String>)> = self
            .shared
            .borrow()
            .funcs
            .iter()
            .filter_map(|f| match &f.target {
                FuncTarget::Path(p) => Some((f.id, p.clone())),
                _ => None,
            })
            .collect();
        let mut resolved = Vec::new();
        for (id, path) in paths {
            if let Some(Value::Func(f)) = values::lookup_path(vm, &path) {
                resolved.push((id, f));
            }
        }
        if resolved != self.resolved {
            vm.set_function_breakpoints(resolved.iter().map(|(_, f)| *f));
            self.resolved = resolved;
        }
        // The functions defined at a line: their prototypes in the chunks
        // loaded from that source.
        let defined: Vec<(u32, String, u32)> = self
            .shared
            .borrow()
            .funcs
            .iter()
            .filter_map(|f| match &f.target {
                FuncTarget::Defined { source, line } => Some((f.id, source.clone(), *line)),
                _ => None,
            })
            .collect();
        let mut protos: Vec<(u32, Rc<Proto>)> = Vec::new();
        for (id, source, line) in defined {
            let mut todo: Vec<Rc<Proto>> = self.chunks.get(&source).cloned().unwrap_or_default();
            while let Some(p) = todo.pop() {
                if p.line_defined == line {
                    protos.push((id, p.clone()));
                }
                todo.extend(p.protos.iter().cloned());
            }
        }
        let ptrs: Vec<(u32, usize)> = protos
            .iter()
            .map(|(id, p)| (*id, Rc::as_ptr(p) as usize))
            .collect();
        if ptrs != self.resolved_protos {
            vm.set_proto_breakpoints(protos.iter().map(|(_, p)| p));
            self.resolved_protos = ptrs;
        }
    }

    fn set_exception_breakpoints(&mut self, vm: &mut Vm, filters: &[String]) {
        self.shared().record_filters(filters);
        let taint = self.shared.borrow().filters.taint;
        vm.watch_taint(taint);
    }

    // ---------------------------------------------------------- stopping

    /// Stops: tells the front-end and serves its requests until one runs
    /// the session on.
    fn stop(&mut self, vm: &mut Vm, reason: StopReason, text: Option<String>, hit: Vec<u32>) {
        if vm.aborted() {
            return;
        }
        self.shared().stepping = None;
        vm.set_step(None);
        let mut paused = self.pause_state(vm);
        // The frames requests name (a stack trace builds them again).
        self.stack_trace(vm, &mut paused);
        // The screen as it is at the stop.
        Shared::run_frame_hook(&self.shared, vm);
        let detail = {
            let mut s = self.shared();
            if reason == StopReason::FunctionBreakpoint {
                for f in s.funcs.iter_mut().filter(|f| hit.contains(&f.id)) {
                    f.hits += 1;
                }
            }
            s.detail.take()
        };
        self.shared().link.event(Event::Stopped {
            reason,
            text,
            hit,
            detail,
        });
        loop {
            let next = self.shared().link.recv(None);
            let Some((id, req)) = next else {
                if self.shared.borrow().link.gone() {
                    self.shared().terminate = true;
                    vm.abort();
                    break;
                }
                continue;
            };
            let flow = self.handle(vm, id, req, Some(&mut paused));
            // A terminate resumes only to end: the VM stops at once, not
            // at the next breakpoint.
            if self.shared.borrow().terminate {
                vm.abort();
                break;
            }
            if matches!(flow, Flow::Resume) {
                break;
            }
        }
        self.release(vm);
    }

    fn pause_state(&mut self, vm: &mut Vm) -> Paused {
        let pins = vm.new_table();
        let key = vm.str(PINS_KEY);
        let _ = vm.heap.table_mut(vm.registry).set(key, Value::Table(pins));
        Paused {
            frames: Vec::new(),
            refs: Vec::new(),
            pins,
        }
    }

    fn release(&mut self, vm: &mut Vm) {
        let key = vm.str(PINS_KEY);
        let _ = vm.heap.table_mut(vm.registry).set(key, Value::Nil);
    }

    /// Serves the requests waiting, without stopping (a running session:
    /// the VM polled the interrupt flag, or the driver is between frames).
    pub fn serve_waiting(&mut self, vm: &mut Vm, between_frames: bool) {
        let runs_code = |r: &Request| {
            matches!(
                r,
                Request::Input { .. } | Request::FireEvent { .. } | Request::World { .. }
            )
        };
        if between_frames {
            let deferred = std::mem::take(&mut self.shared().deferred);
            for (id, req) in deferred {
                if runs_code(&req) && self.shared.borrow().drives_time {
                    self.shared().run_queue.push((id, req));
                    continue;
                }
                let _ = self.handle(vm, id, req, None);
            }
        }
        loop {
            let next = self.shared().link.recv(Some(std::time::Duration::ZERO));
            let Some((id, req)) = next else { break };
            if between_frames && runs_code(&req) && self.shared.borrow().drives_time {
                self.shared().run_queue.push((id, req));
                continue;
            }
            match req {
                Request::Pause => {
                    self.shared().link.respond(id, Ok(Response::Ok));
                    self.begin_pause(vm);
                }
                Request::StackTrace => {
                    self.shared()
                        .link
                        .respond(id, Ok(Response::StackTrace { frames: Vec::new() }));
                }
                req @ (Request::Input { .. }
                | Request::FireEvent { .. }
                | Request::World { .. }
                | Request::Screenshot { .. }
                | Request::FrameTree { .. }
                | Request::FindFrames { .. }
                | Request::Evaluate { .. }
                | Request::Inspect { .. }
                | Request::Pick { .. }
                | Request::WorldPoint { .. }
                | Request::Units
                | Request::ChatLines { .. }
                | Request::Target { .. }
                | Request::LogEvents { .. }
                | Request::Events
                | Request::SetScreen { .. }
                | Request::ShowWorld { .. })
                    if !between_frames && self.shared.borrow().drives_time =>
                {
                    self.shared().deferred.push((id, req));
                }
                req => {
                    let _ = self.handle(vm, id, req, None);
                }
            }
            if self.shared.borrow().terminate {
                vm.abort();
                break;
            }
        }
    }

    /// A pause request while running: the next line run stops.
    fn begin_pause(&mut self, vm: &mut Vm) {
        let (thread, _) = vm.depth();
        self.shared().stepping = Some(Stepping {
            out_of_dispatch: None,
            depth: usize::MAX,
            thread: Some(thread),
            pause: true,
            entry: false,
        });
        vm.set_step(Some(Step {
            thread: None,
            max_depth: usize::MAX,
        }));
    }

    /// Serves one request; `paused` while stopped at a Lua stack.
    fn handle(&mut self, vm: &mut Vm, id: u64, req: Request, paused: Option<&mut Paused>) -> Flow {
        let mut flow = Flow::Stay;
        let r: Result<Response, String> = match req {
            Request::SetBreakpoints {
                source,
                breakpoints,
            } => Ok(Response::Breakpoints {
                breakpoints: self.set_line_breakpoints(vm, source, breakpoints),
            }),
            Request::SetFunctionBreakpoints { breakpoints } => Ok(Response::Breakpoints {
                breakpoints: self.set_function_breakpoints(vm, breakpoints),
            }),
            Request::SetExceptionBreakpoints { filters } => {
                self.set_exception_breakpoints(vm, &filters);
                Ok(Response::Ok)
            }
            Request::ConfigurationDone => {
                self.shared().configured = true;
                Ok(Response::Ok)
            }
            Request::Continue => {
                flow = Flow::Resume;
                self.shared().idle_paused = false;
                Ok(Response::Ok)
            }
            Request::Next | Request::StepIn | Request::StepOut => {
                if paused.is_none() && !self.shared.borrow().idle_paused {
                    Err("not stopped".to_string())
                } else {
                    // Between frames every step is into the next line run.
                    let step = if paused.is_some() {
                        req.clone()
                    } else {
                        Request::StepIn
                    };
                    self.begin_step(vm, &step);
                    self.shared().idle_paused = false;
                    flow = Flow::Resume;
                    Ok(Response::Ok)
                }
            }
            // (between frames: the next line run stops, else the next
            // time between frames)
            Request::Pause => {
                if paused.is_none() && !self.shared.borrow().idle_paused {
                    self.begin_pause(vm);
                }
                Ok(Response::Ok)
            }
            Request::StackTrace => match paused {
                Some(p) => Ok(Response::StackTrace {
                    frames: self.stack_trace(vm, p),
                }),
                None => Ok(Response::StackTrace {
                    frames: self.idle_stack(vm),
                }),
            },
            Request::Scopes { frame } => match paused {
                Some(p) => self
                    .scopes(vm, p, frame)
                    .map(|scopes| Response::Scopes { scopes }),
                None => Ok(Response::Scopes { scopes: Vec::new() }),
            },
            Request::Variables { reference } => match paused {
                Some(p) => values::variables(vm, p, reference)
                    .map(|variables| Response::Variables { variables }),
                None => Err("not stopped".into()),
            },
            Request::SetVariable {
                reference,
                name,
                value,
            } => match paused {
                Some(p) => crate::eval::set_variable(vm, p, reference, &name, &value)
                    .map(|variable| Response::Value { variable }),
                None => Err("not stopped".into()),
            },
            Request::Evaluate {
                expression,
                frame,
                context,
            } => {
                let out = crate::eval::evaluate(vm, paused, frame, &expression, context);
                out.map(|variable| Response::Value { variable })
            }
            Request::Screenshot { frame, scale } => self.screenshot(vm, frame.as_deref(), scale),
            Request::FrameTree { root, hidden } => {
                crate::ui::frame_tree(vm, root.as_deref(), hidden)
                    .map(|data| Response::Data { data })
            }
            Request::FindFrames { query } => Ok(Response::Data {
                data: crate::ui::find_frames(vm, &query),
            }),
            Request::Inspect { frame } => {
                let shared = self.shared.clone();
                let source = move |chunk: &[u8]| {
                    let chunk = String::from_utf8_lossy(chunk);
                    (shared.borrow().mapper)(&chunk)
                };
                crate::ui::inspect(vm, frame, &source).map(|data| Response::Data { data })
            }
            Request::WorldPoint { x, y, camera } => Ok(Response::Data {
                data: match match camera {
                    Some(c) => wowlua_engine::camera::ground_seen(vm, x, y, c),
                    None => wowlua_engine::camera::ground_at(vm, x, y),
                } {
                    Some((n, w, z)) => serde_json::json!({ "y": n, "x": w, "z": z }),
                    None => serde_json::Value::Null,
                },
            }),
            Request::Pick { x, y } => Ok(Response::Data {
                data: crate::ui::pick(vm, x, y),
            }),
            Request::Units => Ok(Response::Data {
                data: crate::ui::units(vm),
            }),
            Request::ChatLines { since } => Ok(Response::Data {
                data: crate::ui::chat_lines(vm, since),
            }),
            Request::Target { unit } => crate::ui::target_unit(vm, unit).map(|()| Response::Ok),
            Request::LogEvents { on } => {
                if on {
                    let clock = self.shared.borrow().clock.clone();
                    wowlua_engine::eventlog::start(vm, Some(clock));
                } else {
                    wowlua_engine::eventlog::stop(vm);
                }
                Ok(Response::Ok)
            }
            Request::Events => Ok(Response::Data {
                data: crate::ui::events(vm),
            }),
            Request::SetScreen {
                width,
                height,
                ui_scale,
            } => {
                wowlua_engine::apply_settings(vm, width, height, ui_scale);
                Ok(Response::Ok)
            }
            Request::ShowWorld { on } => {
                wowlua_engine::set_world(vm, on);
                Ok(Response::Ok)
            }
            Request::Input { input } => crate::ui::input(vm, &input).map(|()| Response::Ok),
            Request::FireEvent { event, args } => {
                crate::ui::fire_event(vm, &event, &args).map(|()| Response::Ok)
            }
            Request::World { script } => crate::ui::world(vm, &script).map(|()| Response::Ok),
            Request::Advance { seconds, frames } => {
                if self.shared.borrow().drives_time && paused.is_none() {
                    self.shared().advance = Some((id, seconds, frames));
                    return Flow::Stay;
                }
                Err("time moves only between frames of an addon session".into())
            }
            Request::ProfileStart => {
                let clock = self.shared.borrow().clock.clone();
                vm.start_profiling(Box::new(move || clock()));
                Ok(Response::Ok)
            }
            Request::ProfileStop => match vm.stop_profiling() {
                Some(p) => Ok(Response::Data {
                    data: crate::profile::report(&p, &|chunk| self.source_of(chunk)),
                }),
                None => Err("the profiler is not running".into()),
            },
            Request::Output => {
                let lines = std::mem::take(&mut self.shared().output);
                Ok(Response::Output { lines })
            }
            Request::Breakpoints => {
                let s = self.shared.borrow();
                let mut list: Vec<BreakpointState> = Vec::new();
                for (source, bps) in &s.lines {
                    for b in bps {
                        list.push(BreakpointState {
                            source: Some(source.clone()),
                            ..state_of(b)
                        });
                    }
                }
                for f in &s.funcs {
                    list.push(BreakpointState {
                        id: f.id,
                        verified: self.resolved.iter().any(|(id, _)| *id == f.id)
                            || !matches!(f.target, FuncTarget::Path(_)),
                        line: None,
                        message: None,
                        hits: f.hits,
                        source: None,
                    });
                }
                Ok(Response::Breakpoints { breakpoints: list })
            }
            Request::SetClock { manual } => {
                self.shared().manual = Some(manual);
                Ok(Response::Ok)
            }
            Request::SetForeground { on } => {
                self.shared().background = !on;
                Ok(Response::Ok)
            }
            Request::SetDisplayRate { hz } => {
                if hz.is_finite() && hz >= 1.0 {
                    self.shared().display_hz = hz.min(1000.0);
                }
                Ok(Response::Ok)
            }
            Request::SetDebugBlizzardUi { on } => {
                self.shared().debug_blizzard_ui = on;
                Ok(Response::Ok)
            }
            Request::SaveWorld => Ok(Response::Data {
                data: crate::ui::world_profile(vm),
            }),
            Request::Terminate => {
                let mut s = self.shared();
                s.terminate = true;
                s.idle_paused = false;
                flow = Flow::Resume;
                Ok(Response::Ok)
            }
        };
        let resumed = matches!(flow, Flow::Resume) && !self.shared.borrow().terminate;
        self.shared().link.respond(id, r);
        if resumed {
            self.shared().link.event(Event::Continued);
        }
        flow
    }

    fn screenshot(
        &mut self,
        vm: &mut Vm,
        frame: Option<&str>,
        scale: Option<f64>,
    ) -> Result<Response, String> {
        let shot = self.shared().screenshot.take();
        let Some(mut shot) = shot else {
            return Err("this session cannot draw screenshots (no renderer)".into());
        };
        let r = shot(vm, frame, scale);
        self.shared().screenshot = Some(shot);
        let (png, width, height) = r?;
        Ok(Response::Image {
            png_base64: crate::ui::base64(&png),
            width,
            height,
        })
    }

    fn begin_step(&mut self, vm: &mut Vm, req: &Request) {
        let (thread, depth) = vm.depth();
        let dispatches = vm.dispatches().len();
        // The frame stepped from: the innermost Lua frame.
        let top_is_handler = vm.lua_frames().first().is_some_and(|f| {
            vm.dispatches()
                .last()
                .is_some_and(|d| d.thread == thread && d.frames == f.index)
        });
        let (vm_step, stepping) = match req {
            Request::StepIn => (
                Step {
                    thread: None,
                    max_depth: usize::MAX,
                },
                Stepping {
                    out_of_dispatch: None,
                    depth: usize::MAX,
                    thread: Some(thread),
                    pause: false,
                    entry: false,
                },
            ),
            Request::StepOut if top_is_handler => (
                // Out of a handler: the next handler of the dispatch.
                Step {
                    thread: Some(thread),
                    max_depth: depth.saturating_sub(1),
                },
                Stepping {
                    out_of_dispatch: Some(dispatches),
                    depth,
                    thread: Some(thread),
                    pause: false,
                    entry: false,
                },
            ),
            Request::StepOut => (
                Step {
                    thread: Some(thread),
                    max_depth: depth.saturating_sub(1),
                },
                Stepping {
                    out_of_dispatch: None,
                    depth: depth.saturating_sub(1),
                    thread: Some(thread),
                    pause: false,
                    entry: false,
                },
            ),
            _ => (
                Step {
                    thread: Some(thread),
                    max_depth: depth,
                },
                Stepping {
                    out_of_dispatch: None,
                    depth,
                    thread: Some(thread),
                    pause: false,
                    entry: false,
                },
            ),
        };
        self.shared().stepping = Some(stepping);
        vm.set_step(Some(vm_step));
    }

    // ------------------------------------------------------ inspection

    fn stack_trace(&mut self, vm: &mut Vm, p: &mut Paused) -> Vec<StackFrame> {
        p.frames.clear();
        let mut out = Vec::new();
        let (thread, _) = vm.depth();
        let lua = vm.lua_frames();
        let dispatches: Vec<Dispatch> = vm
            .dispatches()
            .iter()
            .filter(|d| d.thread == thread)
            .cloned()
            .collect();
        let mut push = |p: &mut Paused, f: StackFrame, r: FrameRef| {
            p.frames.push(r);
            let mut f = f;
            f.id = p.frames.len() as u32;
            out.push(f);
        };
        let dispatch_frame = |d: &Dispatch| StackFrame {
            id: 0,
            name: match d.kind {
                DispatchKind::Script | DispatchKind::Update => d.name.to_string(),
                k => format!("{} {}", k.label(), d.name),
            },
            kind: "dispatch".into(),
            source: None,
            chunk: None,
            file: None,
            line: 0,
            taint: "secure".into(),
        };
        let native_frame = |name: &str| StackFrame {
            id: 0,
            name: format!("[C] {name}"),
            kind: "native".into(),
            source: None,
            chunk: None,
            file: None,
            line: 0,
            taint: "secure".into(),
        };
        let top = lua.first().map_or(0, |f| f.index + 1);
        for d in dispatches.iter().rev().filter(|d| d.frames >= top) {
            push(p, dispatch_frame(d), FrameRef::Dispatch);
        }
        for f in &lua {
            for n in f.natives_above.iter().rev() {
                push(p, native_frame(n), FrameRef::Native);
            }
            let line = f.proto.lines.get(f.pc).copied().unwrap_or(0);
            let source = self.source_of(&f.proto.source);
            let chunk = source.is_none().then(|| {
                String::from_utf8_lossy(&wowlua_vm::vm::chunkid(&f.proto.source)).into_owned()
            });
            let file = if source.is_none() {
                let s = self.shared.borrow();
                s.files
                    .as_ref()
                    .and_then(|m| m(&String::from_utf8_lossy(&f.proto.source)))
            } else {
                None
            };
            let name = match vm.frame_name(f.index) {
                Some((_, n)) => n,
                None if f.proto.line_defined == 0 => "main chunk".into(),
                None => match dispatches.iter().rev().find(|d| d.frames == f.index) {
                    Some(d) => d.name.to_string(),
                    None => format!(
                        "function <{}:{}>",
                        source.as_deref().or(chunk.as_deref()).unwrap_or("?"),
                        f.proto.line_defined
                    ),
                },
            };
            let taint = values::taint_text(vm, vm.func_taint(f.func));
            push(
                p,
                StackFrame {
                    id: 0,
                    name,
                    kind: "lua".into(),
                    source,
                    chunk,
                    file,
                    line,
                    taint,
                },
                FrameRef::Lua {
                    index: f.index,
                    func: f.func,
                },
            );
            for d in dispatches.iter().rev().filter(|d| d.frames == f.index) {
                push(p, dispatch_frame(d), FrameRef::Dispatch);
            }
        }
        out
    }

    /// The stack between frames: the game is idle.
    fn idle_stack(&mut self, _vm: &mut Vm) -> Vec<StackFrame> {
        vec![StackFrame {
            id: 1,
            name: "(between frames)".into(),
            kind: "dispatch".into(),
            source: None,
            chunk: None,
            file: None,
            line: 0,
            taint: "secure".into(),
        }]
    }

    fn scopes(&mut self, vm: &mut Vm, p: &mut Paused, frame: u32) -> Result<Vec<Scope>, String> {
        let Some(f) = p.frames.get(frame.saturating_sub(1) as usize).cloned() else {
            return Err(format!("no frame {frame}"));
        };
        let FrameRef::Lua { index, func } = f else {
            return Ok(Vec::new());
        };
        let mut scopes = vec![Scope {
            name: "Locals".into(),
            reference: values::add_ref(p, RefTarget::Locals(index)),
            expensive: false,
        }];
        if !vm.frame_varargs(index).is_empty() {
            scopes.push(Scope {
                name: "Varargs".into(),
                reference: values::add_ref(p, RefTarget::Varargs(index)),
                expensive: false,
            });
        }
        if !vm.upvalues(func).is_empty() {
            scopes.push(Scope {
                name: "Upvalues".into(),
                reference: values::add_ref(p, RefTarget::Upvalues(func)),
                expensive: false,
            });
        }
        let env = vm.func_env(func);
        values::pin(vm, p, Value::Table(env));
        scopes.push(Scope {
            name: "Globals".into(),
            reference: values::add_ref(p, RefTarget::Table(env)),
            expensive: true,
        });
        Ok(scopes)
    }

    // -------------------------------------------------------- deciding

    /// Whether the running frame's code is steppable: the package's, or
    /// any with `debugBlizzardUI`.
    fn steppable(&self, vm: &Vm) -> bool {
        if self.shared.borrow().debug_blizzard_ui {
            return true;
        }
        match vm.lua_frames().first() {
            Some(f) => self.source_of(&f.proto.source).is_some(),
            None => false,
        }
    }

    fn check_line(&mut self, vm: &mut Vm, line: u32, cause: LineCause) {
        let frames = vm.lua_frames();
        let Some(top) = frames.first() else { return };
        let source = self.source_of(&top.proto.source);
        if cause == LineCause::FunctionEntry {
            let proto = Rc::as_ptr(&top.proto) as usize;
            let hit: Vec<u32> = self
                .resolved
                .iter()
                .filter(|(_, f)| *f == top.func)
                .map(|(id, _)| *id)
                .chain(
                    self.resolved_protos
                        .iter()
                        .filter(|(_, p)| *p == proto)
                        .map(|(id, _)| *id),
                )
                .collect();
            if !hit.is_empty() && self.function_condition_holds(vm, &hit) {
                self.stop(vm, StopReason::FunctionBreakpoint, None, hit);
                return;
            }
        }
        // A step's end.
        let stepping = self.shared.borrow().stepping;
        if let Some(st) = stepping {
            let (thread, depth) = vm.depth();
            let reached = st.out_of_dispatch.is_none()
                && (st.pause
                    || st.depth == usize::MAX
                    || (Some(thread) == st.thread && depth <= st.depth));
            if reached && (st.pause || self.steppable(vm)) {
                let reason = if st.entry {
                    StopReason::Entry
                } else if st.pause {
                    StopReason::Pause
                } else {
                    StopReason::Step
                };
                self.stop(vm, reason, None, Vec::new());
                return;
            }
        }
        // A line breakpoint.
        let Some(source) = source else { return };
        let candidates: Vec<(u32, LineBreakpoint)> = {
            let s = self.shared.borrow();
            match s.lines.get(&source) {
                Some(list) => list
                    .iter()
                    .filter(|b| b.actual == Some(line))
                    .map(|b| (b.id, b.spec.clone()))
                    .collect(),
                None => Vec::new(),
            }
        };
        let mut hit = Vec::new();
        for (id, spec) in candidates {
            if let Some(c) = &spec.condition {
                match crate::eval::evaluate(vm, None, None, c, EvalContext::Repl) {
                    Ok(v) if v.value != "nil" && v.value != "false" => {}
                    Ok(_) => continue,
                    Err(e) => {
                        self.shared()
                            .output("console", &format!("breakpoint condition `{c}`: {e}\n"));
                    }
                }
            }
            let hits = {
                let mut s = self.shared();
                let bp = s
                    .lines
                    .get_mut(&source)
                    .and_then(|l| l.iter_mut().find(|b| b.id == id));
                match bp {
                    Some(b) => {
                        b.hits += 1;
                        b.hits
                    }
                    None => continue,
                }
            };
            if let Some(h) = &spec.hit_condition
                && !hit_condition(h, hits)
            {
                continue;
            }
            if let Some(msg) = &spec.log_message {
                let text = crate::eval::log_message(vm, msg);
                let mut s = self.shared();
                let line_out = OutputLine {
                    category: "console".into(),
                    text: format!("{text}\n"),
                    source: Some(source.clone()),
                    line: Some(line),
                };
                s.output.push(line_out.clone());
                s.link.event(Event::Output { line: line_out });
                continue;
            }
            hit.push(id);
        }
        if !hit.is_empty() {
            self.stop(vm, StopReason::Breakpoint, None, hit);
        }
    }

    fn function_condition_holds(&mut self, vm: &mut Vm, ids: &[u32]) -> bool {
        let conds: Vec<Option<String>> = self
            .shared
            .borrow()
            .funcs
            .iter()
            .filter(|f| ids.contains(&f.id))
            .map(|f| f.spec.condition.clone())
            .collect();
        conds.iter().any(|c| match c {
            None => true,
            Some(c) => crate::eval::evaluate(vm, None, None, c, EvalContext::Repl)
                .is_ok_and(|v| v.value != "nil" && v.value != "false"),
        })
    }

    fn check_dispatch(&mut self, vm: &mut Vm, d: &Dispatch) {
        let hit: Vec<u32> = self
            .shared
            .borrow()
            .funcs
            .iter()
            .filter(|f| match (&f.target, d.kind) {
                (FuncTarget::Event(None), DispatchKind::Event) => true,
                (FuncTarget::Event(Some(e)), DispatchKind::Event) => **e == *d.name,
                (
                    FuncTarget::Script { frame, script },
                    DispatchKind::Script | DispatchKind::Update,
                ) => match d.name.rsplit_once(':') {
                    Some((f, s)) => s == script && frame.as_deref().is_none_or(|n| n == f),
                    None => false,
                },
                (FuncTarget::Timer, DispatchKind::Timer) => true,
                _ => false,
            })
            .map(|f| f.id)
            .collect();
        if !hit.is_empty() {
            let text = format!("{} {}", d.kind.label(), d.name);
            self.stop(vm, StopReason::FunctionBreakpoint, Some(text), hit);
        }
    }

    /// Called by the VM's load hook: the chunk's breakpoints are placed.
    fn loaded(&mut self, vm: &mut Vm, proto: &Rc<Proto>) {
        if let Some(source) = self.source_of(&proto.source) {
            self.chunks
                .entry(source.clone())
                .or_default()
                .push(proto.clone());
            let before: Vec<BreakpointState> = self
                .shared
                .borrow()
                .lines
                .get(&source)
                .map(|l| l.iter().map(state_of).collect())
                .unwrap_or_default();
            self.place(vm, &source);
            let after: Vec<BreakpointState> = self
                .shared
                .borrow()
                .lines
                .get(&source)
                .map(|l| l.iter().map(state_of).collect())
                .unwrap_or_default();
            for (a, b) in after.into_iter().zip(before) {
                if a != b {
                    self.shared()
                        .link
                        .event(Event::Breakpoint { breakpoint: a });
                }
            }
        }
        if !self.shared.borrow().funcs.is_empty() {
            self.resolve_functions(vm);
        }
    }
}

impl Debugger for Controller {
    fn line(&mut self, vm: &mut Vm, line: u32, cause: LineCause) {
        self.check_line(vm, line, cause);
    }

    fn api_call(&mut self, vm: &mut Vm, f: FuncRef) {
        let hit: Vec<u32> = self
            .resolved
            .iter()
            .filter(|(_, g)| *g == f)
            .map(|(id, _)| *id)
            .collect();
        if !hit.is_empty() && self.function_condition_holds(vm, &hit) {
            let name = vm.native_name(f).to_string();
            self.stop(
                vm,
                StopReason::FunctionBreakpoint,
                Some(format!("[C] {name}")),
                hit,
            );
        }
    }

    fn error(&mut self, vm: &mut Vm, error: &LuaError, caught: bool) {
        let f = self.shared.borrow().filters;
        if f.all || (f.uncaught && !caught) {
            let msg = values::error_text(vm, error.value);
            self.shared().detail = Some(StopDetail::Error {
                message: msg.clone(),
                // (the error's own traceback is written as it unwinds:
                // here, the stack it was raised in)
                stack: if error.traceback.is_empty() {
                    vm.traceback(0)
                } else {
                    error.traceback.clone()
                },
                caught,
            });
            self.stop(vm, StopReason::Exception, Some(msg), Vec::new());
        }
    }

    fn load(&mut self, vm: &mut Vm, proto: &Rc<Proto>) {
        self.loaded(vm, proto);
    }

    fn dispatch(&mut self, vm: &mut Vm, d: &Dispatch, enter: bool) {
        if enter {
            if !self.shared.borrow().funcs.is_empty() {
                self.resolve_functions(vm);
                self.check_dispatch(vm, d);
            }
            return;
        }
        // Out of a handler: once its dispatch ended, the next line at
        // its depth.
        let stepping = self.shared.borrow().stepping;
        if let Some(st) = stepping
            && let Some(level) = st.out_of_dispatch
            && vm.dispatches().len() < level
        {
            self.shared().stepping = Some(Stepping {
                out_of_dispatch: None,
                ..st
            });
            vm.set_step(Some(Step {
                thread: st.thread,
                max_depth: st.depth,
            }));
        }
    }

    fn interrupted(&mut self, vm: &mut Vm) {
        self.serve_waiting(vm, false);
    }

    fn tainted(&mut self, vm: &mut Vm, by: &str) {
        if self.shared.borrow().filters.taint && self.steppable(vm) {
            let report = vm.execution_report();
            self.shared().detail = Some(StopDetail::Taint {
                by: by.to_string(),
                report,
            });
            self.stop(
                vm,
                StopReason::Exception,
                Some(format!("execution tainted by {by}")),
                Vec::new(),
            );
        }
    }

    fn blocked(&mut self, vm: &mut Vm, action: &str, addon: &str, report: &str) {
        if self.shared.borrow().filters.blocked {
            let text = format!("{addon} was blocked from {action}\n{report}");
            self.shared().detail = Some(StopDetail::Blocked {
                addon: addon.to_string(),
                action: action.to_string(),
                report: report.to_string(),
            });
            self.stop(vm, StopReason::Exception, Some(text), Vec::new());
        }
    }

    fn idle(&mut self, vm: &mut Vm) {
        self.serve_waiting(vm, true);
        // A pause between frames: the session waits here.
        let pausing = self.shared.borrow().stepping.is_some_and(|s| s.pause);
        if pausing {
            self.shared().stepping = None;
            vm.set_step(None);
            self.shared().idle_paused = true;
            self.shared().link.event(Event::Stopped {
                reason: StopReason::Pause,
                text: Some("between frames".into()),
                hit: Vec::new(),
                detail: None,
            });
        }
        while self.shared.borrow().idle_paused && !self.shared.borrow().terminate {
            let next = self.shared().link.recv(None);
            let Some((id, req)) = next else {
                if self.shared.borrow().link.gone() {
                    self.shared().terminate = true;
                }
                continue;
            };
            let _ = self.handle(vm, id, req, None);
        }
    }
}

fn state_of(b: &LineBp) -> BreakpointState {
    BreakpointState {
        id: b.id,
        verified: b.actual.is_some(),
        line: Some(b.actual.unwrap_or(b.spec.line)),
        message: b
            .actual
            .is_none()
            .then(|| "not loaded yet, or no code at or after this line".to_string()),
        hits: b.hits,
        source: None,
    }
}

fn collect_lines(p: &Proto, out: &mut Vec<u32>) {
    out.extend_from_slice(&p.lines);
    for c in &p.protos {
        collect_lines(c, out);
    }
}

fn parse_target(name: &str) -> FuncTarget {
    let name = name.trim();
    if let Some(e) = name.strip_prefix("event:") {
        return FuncTarget::Event((e != "*").then(|| e.to_string()));
    }
    if let Some(s) = name.strip_prefix("script:") {
        return match s.rsplit_once(':') {
            Some((frame, script)) => FuncTarget::Script {
                frame: Some(frame.to_string()),
                script: script.to_string(),
            },
            None => FuncTarget::Script {
                frame: None,
                script: s.to_string(),
            },
        };
    }
    if name == "timer" {
        return FuncTarget::Timer;
    }
    if let Some((source, line)) = name.rsplit_once(':')
        && (source.ends_with(".lua") || source.ends_with(".wowlua"))
        && let Ok(line) = line.parse()
    {
        return FuncTarget::Defined {
            source: source.to_string(),
            line,
        };
    }
    FuncTarget::Path(name.split(['.', ':']).map(str::to_string).collect())
}

/// Whether the `hits`-th hit passes a hit condition.
pub(crate) fn hit_condition(cond: &str, hits: u32) -> bool {
    let c = cond.trim();
    let (op, n) = if let Some(n) = c.strip_prefix(">=") {
        (">=", n)
    } else if let Some(n) = c.strip_prefix("<=") {
        ("<=", n)
    } else if let Some(n) = c.strip_prefix("==") {
        ("==", n)
    } else if let Some(n) = c.strip_prefix('>') {
        (">", n)
    } else if let Some(n) = c.strip_prefix('<') {
        ("<", n)
    } else if let Some(n) = c.strip_prefix('%') {
        ("%", n)
    } else {
        ("==", c)
    };
    let Ok(n) = n.trim().parse::<u32>() else {
        return true;
    };
    match op {
        ">=" => hits >= n,
        "<=" => hits <= n,
        ">" => hits > n,
        "<" => hits < n,
        "%" => n > 0 && hits.is_multiple_of(n),
        _ => hits == n,
    }
}
