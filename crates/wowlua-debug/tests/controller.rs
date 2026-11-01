//! The controller on a VM with the engine (spec debugger §18.4 to §18.7):
//! a scripted front-end answers each stop.

use std::cell::RefCell;
use std::collections::VecDeque;
use std::rc::Rc;

use wowlua_debug::controller::{Controller, Link, Shared, SharedRef};
use wowlua_debug::protocol::*;
use wowlua_vm::{Value, Vm};

#[derive(Default)]
struct Log {
    events: Vec<Event>,
    responses: Vec<(u64, Result<Response, String>)>,
}

/// Requests in order; each stop takes the next ones until one goes on.
struct Script {
    queue: Rc<RefCell<VecDeque<Request>>>,
    log: Rc<RefCell<Log>>,
    next: u64,
}

impl Link for Script {
    fn recv(&mut self, timeout: Option<std::time::Duration>) -> Option<(u64, Request)> {
        // While running, nothing comes in: the script is for stops.
        if timeout.is_some() {
            return None;
        }
        let r = self.queue.borrow_mut().pop_front()?;
        self.next += 1;
        Some((self.next, r))
    }
    fn respond(&mut self, id: u64, r: Result<Response, String>) {
        self.log.borrow_mut().responses.push((id, r));
    }
    fn event(&mut self, e: Event) {
        self.log.borrow_mut().events.push(e);
    }
    fn gone(&self) -> bool {
        self.queue.borrow().is_empty()
    }
}

struct Session {
    vm: Vm,
    shared: SharedRef,
    queue: Rc<RefCell<VecDeque<Request>>>,
    log: Rc<RefCell<Log>>,
}

fn session() -> Session {
    let mut vm = Vm::wow();
    wowlua_engine::install(&mut vm, "standard").expect("the engine");
    vm.set_output(|_| {});
    let queue = Rc::new(RefCell::new(VecDeque::new()));
    let log = Rc::new(RefCell::new(Log::default()));
    let link = Script {
        queue: queue.clone(),
        log: log.clone(),
        next: 0,
    };
    // Chunks named `@src/…` are the package's.
    let shared = Shared::new(
        Box::new(link),
        Box::new(|chunk: &str| chunk.strip_prefix("@src/").map(|p| format!("src/{p}"))),
        Rc::new(|| 0),
    );
    Controller::attach(&mut vm, &shared);
    Session {
        vm,
        shared,
        queue,
        log,
    }
}

impl Session {
    fn script(&self, reqs: Vec<Request>) {
        self.queue.borrow_mut().extend(reqs);
    }

    fn run(&mut self, name: &str, src: &str) {
        let f = self
            .vm
            .load(src.as_bytes(), &format!("@src/{name}"))
            .expect("compiles");
        self.vm.call(f, &[]).expect("runs");
    }

    fn stops(&self) -> Vec<(StopReason, Option<String>)> {
        self.log
            .borrow()
            .events
            .iter()
            .filter_map(|e| match e {
                Event::Stopped { reason, text, .. } => Some((*reason, text.clone())),
                _ => None,
            })
            .collect()
    }

    /// The answers to stack trace requests, in order.
    fn stacks(&self) -> Vec<Vec<StackFrame>> {
        self.log
            .borrow()
            .responses
            .iter()
            .filter_map(|(_, r)| match r {
                Ok(Response::StackTrace { frames }) => Some(frames.clone()),
                _ => None,
            })
            .collect()
    }

    fn values(&self) -> Vec<String> {
        self.log
            .borrow()
            .responses
            .iter()
            .filter_map(|(_, r)| match r {
                Ok(Response::Value { variable }) => Some(variable.value.clone()),
                Err(e) => Some(format!("error: {e}")),
                _ => None,
            })
            .collect()
    }

    fn set_line(&mut self, source: &str, line: u32, f: impl FnOnce(&mut LineBreakpoint)) {
        let mut bp = LineBreakpoint {
            line,
            ..Default::default()
        };
        f(&mut bp);
        self.shared.borrow_mut().record_lines(source, vec![bp]);
    }

    fn set_functions(&mut self, names: &[&str]) {
        let specs = names
            .iter()
            .map(|n| FunctionBreakpoint {
                name: n.to_string(),
                condition: None,
            })
            .collect();
        self.shared.borrow_mut().record_functions(specs);
    }
}

fn eval(e: &str) -> Request {
    Request::Evaluate {
        expression: e.into(),
        frame: Some(1),
        context: EvalContext::Repl,
    }
}

const TWO_HANDLERS: &str = "local a = CreateFrame('Frame', 'FirstFrame')
a:RegisterEvent('BAG_UPDATE')
a:SetScript('OnEvent', function(self, event, bag)
    local seen = bag
    order = (order or '') .. 'a'
end)
local b = CreateFrame('Frame', 'SecondFrame')
b:RegisterEvent('BAG_UPDATE')
b:SetScript('OnEvent', function(self, event, bag)
    order = order .. 'b'
end)
";

/// Stepping into Blizzard's UI code switches while the session runs.
#[test]
fn debugging_blizzards_ui_switches_while_running() {
    let mut s = session();
    s.set_line("src/b.lua", 1, |_| {});
    s.script(vec![
        Request::SetDebugBlizzardUi { on: true },
        Request::Continue,
    ]);
    s.run("b.lua", "local x = 1\n");
    assert!(s.shared.borrow().debug_blizzard_ui);
}

/// A watch only reads (decision T496): pure library functions and the
/// game's queries run; assigning, calling a Lua function or another API
/// function is refused; the console still runs them.
#[test]
fn a_watch_only_reads() {
    let mut s = session();
    s.set_line("src/w.lua", 4, |_| {});
    let watch = |e: &str| Request::Evaluate {
        expression: e.into(),
        frame: Some(1),
        context: EvalContext::Watch,
    };
    s.script(vec![
        watch("string.format('%d', n * 2)"),
        watch("UnitName('player') ~= nil and #list"),
        watch("Bump()"),
        watch("CreateFrame('Frame')"),
        watch("n = 5"),
        watch("table.sort(list)"),
        eval("Bump()"),
        eval("Count"),
        Request::Continue,
    ]);
    s.run(
        "w.lua",
        "function Bump() Count = (Count or 0) + 1 end\nlocal n = 21\nlocal list = { 3, 1 }\nlocal x = n\n",
    );
    let v = s.values();
    assert_eq!(&v[..2], ["\"42\"", "2"], "{v:?}");
    assert!(v[2].contains("does not call the function at"), "{}", v[2]);
    assert!(v[3].contains("does not call CreateFrame()"), "{}", v[3]);
    assert!(v[4].contains("cannot set n"), "{}", v[4]);
    assert!(v[5].contains("does not call sort()"), "{}", v[5]);
    // The console ran it; the watch had not.
    assert_eq!(v[7], "1", "{v:?}");
}

/// A function breakpoint naming where a function is defined (as a profile
/// names it) stops on entry to each of its closures, local ones too
/// (decision T495).
#[test]
fn a_function_breakpoint_by_definition_stops_its_calls() {
    let mut s = session();
    s.set_functions(&["src/defs.lua:1"]);
    s.script(vec![
        Request::StackTrace,
        Request::Continue,
        Request::StackTrace,
        Request::Continue,
    ]);
    s.run(
        "defs.lua",
        "local function sq(x)\n    return x * x\nend\nfunction Twice() return sq(2) + sq(3) end\nTwice()\n",
    );
    let stops = s.stops();
    assert_eq!(stops.len(), 2, "{stops:?}");
    assert!(stops.iter().all(|s| s.0 == StopReason::FunctionBreakpoint));
    let stacks = s.stacks();
    assert_eq!(stacks[0][0].line, 2);
    assert_eq!(stacks[0][0].source.as_deref(), Some("src/defs.lua"));
    assert_eq!(stacks[1][0].line, 2);
}

#[test]
fn an_event_breakpoint_stops_before_its_first_handler() {
    let mut s = session();
    s.set_functions(&["event:BAG_UPDATE"]);
    s.run("handlers.lua", TWO_HANDLERS);
    s.script(vec![
        Request::StackTrace,
        Request::StepIn,
        Request::StackTrace,
        Request::Continue,
    ]);
    wowlua_engine::widgets::fire_event(&mut s.vm, "BAG_UPDATE", &[Value::Num(2.0)]);
    let stops = s.stops();
    assert_eq!(
        stops[0],
        (
            StopReason::FunctionBreakpoint,
            Some("event BAG_UPDATE".into())
        )
    );
    assert_eq!(stops[1].0, StopReason::Step);
    let stacks = s.stacks();
    assert_eq!(stacks[0][0].name, "event BAG_UPDATE");
    assert_eq!(stacks[0][0].kind, "dispatch");
    // Stepped into the first handler's first line.
    assert_eq!(stacks[1][0].line, 4);
    assert_eq!(stacks[1][0].source.as_deref(), Some("src/handlers.lua"));
    assert_eq!(stacks[1][1].name, "FirstFrame:OnEvent");
    assert_eq!(stacks[1][2].name, "event BAG_UPDATE");
}

#[test]
fn stepping_out_of_a_handler_stops_in_the_next() {
    let mut s = session();
    s.set_line("src/handlers.lua", 5, |_| {});
    s.run("handlers.lua", TWO_HANDLERS);
    s.script(vec![
        Request::StepOut,
        Request::StackTrace,
        Request::Continue,
    ]);
    wowlua_engine::widgets::fire_event(&mut s.vm, "BAG_UPDATE", &[Value::Num(1.0)]);
    let stops = s.stops();
    assert_eq!(
        stops.iter().map(|s| s.0).collect::<Vec<_>>(),
        [StopReason::Breakpoint, StopReason::Step]
    );
    let stacks = s.stacks();
    assert_eq!(stacks[0][0].line, 10, "{:#?}", stacks[0]);
    assert_eq!(stacks[0][1].name, "SecondFrame:OnEvent");
}

#[test]
fn hit_conditions_conditions_and_log_points() {
    let mut s = session();
    let src = "for i = 1, 5 do\n  local sq = i * i\n  total = (total or 0) + sq\nend\n";
    s.set_line("src/loop.lua", 3, |b| b.hit_condition = Some("3".into()));
    s.script(vec![eval("i"), Request::Continue]);
    s.run("loop.lua", src);
    assert_eq!(s.values(), ["3"]);
    let mut s = session();
    s.set_line("src/loop.lua", 3, |b| b.condition = Some("sq > 10".into()));
    s.script(vec![
        eval("sq"),
        Request::Continue,
        eval("sq"),
        Request::Continue,
    ]);
    s.run("loop.lua", src);
    assert_eq!(s.values(), ["16", "25"]);
    let mut s = session();
    s.set_line("src/loop.lua", 3, |b| {
        b.log_message = Some("i={i} sq={sq}".into())
    });
    s.run("loop.lua", src);
    assert!(s.stops().is_empty());
    let logged: Vec<String> = s
        .log
        .borrow()
        .events
        .iter()
        .filter_map(|e| match e {
            Event::Output { line } => Some(line.text.clone()),
            _ => None,
        })
        .collect();
    assert_eq!(logged.len(), 5);
    assert_eq!(logged[2], "i=3 sq=9\n");
}

#[test]
fn a_line_without_code_moves_to_the_next() {
    let mut s = session();
    let r = s.shared.borrow_mut().record_lines(
        "src/gap.lua",
        vec![LineBreakpoint {
            line: 2,
            ..Default::default()
        }],
    );
    assert!(!r[0].verified);
    s.script(vec![eval("x"), Request::Continue]);
    s.run("gap.lua", "local x = 1\n\n-- a comment\nx = x + 1\n");
    assert_eq!(s.values(), ["1"]);
    let moved = s.log.borrow().events.iter().any(|e| {
        matches!(e, Event::Breakpoint { breakpoint } if breakpoint.verified && breakpoint.line == Some(4))
    });
    assert!(moved, "{:#?}", s.log.borrow().events);
}

#[test]
fn evaluation_assigns_locals_and_upvalues_of_the_frame() {
    let mut s = session();
    s.set_line("src/assign.lua", 4, |_| {});
    s.script(vec![
        eval("n = n * 10"),
        eval("up = 'changed'"),
        eval("n, up"),
        Request::Continue,
    ]);
    s.run(
        "assign.lua",
        "local up = 'kept'\nlocal function f(n)\n  local m = n\n  result = n .. up\nend\nf(4)\n",
    );
    let values = s.values();
    assert_eq!(values[2], "40, \"changed\"");
    let r = s.vm.get_global("result");
    assert_eq!(String::from_utf8_lossy(&s.vm.raw_tostring(r)), "40changed");
}

#[test]
fn errors_that_reach_the_handler_stop_the_session() {
    let mut s = session();
    s.shared
        .borrow_mut()
        .record_filters(&["uncaught".to_string()]);
    s.run(
        "err.lua",
        "local f = CreateFrame('Frame')\nf:RegisterEvent('BAG_UPDATE')\nf:SetScript('OnEvent', function() local t = nil; return t.x end)\n\
         pcall(error, 'caught')\n",
    );
    s.script(vec![Request::StackTrace, Request::Continue]);
    wowlua_engine::widgets::fire_event(&mut s.vm, "BAG_UPDATE", &[Value::Num(0.0)]);
    let stops = s.stops();
    assert_eq!(stops.len(), 1, "the caught error does not stop: {stops:?}");
    assert_eq!(stops[0].0, StopReason::Exception);
    assert!(
        stops[0].1.as_deref().unwrap().contains("attempt to index"),
        "{stops:?}"
    );
    assert_eq!(s.stacks()[0][0].line, 3);
}

#[test]
fn values_show_their_taint() {
    let mut s = session();
    s.set_line("src/taint.lua", 2, |_| {});
    let f =
        s.vm.load_as(
            b"local mine = { 1 }\nreturn mine\n",
            "@src/taint.lua",
            "Demo",
        )
        .expect("compiles");
    s.script(vec![
        Request::Scopes { frame: 1 },
        Request::Variables { reference: 1 },
        Request::Continue,
    ]);
    s.vm.call(f, &[]).expect("runs");
    let vars = s
        .log
        .borrow()
        .responses
        .iter()
        .find_map(|(_, r)| match r {
            Ok(Response::Variables { variables }) => Some(variables.clone()),
            _ => None,
        })
        .expect("variables");
    let mine = vars.iter().find(|v| v.name == "mine").expect("mine");
    assert_eq!(mine.taint, "Demo");
    let stacks = s.stacks();
    assert!(stacks.is_empty() || stacks[0][0].taint == "Demo");
}

#[test]
fn the_ui_answers_with_frames_and_clicks_at_a_point() {
    let mut s = session();
    s.run(
        "ui.lua",
        "local b = CreateFrame('Button', 'OkButton', UIParent)\nb:SetSize(200, 100)\nb:SetPoint('TOPLEFT', 0, 0)\n\
         b:SetScript('OnClick', function() clicked = (clicked or 0) + 1 end)\n\
         local t = b:CreateFontString(nil, 'OVERLAY')\nt:SetFont('Fonts\\\\FRIZQT__.TTF', 12)\nt:SetAllPoints()\nt:SetText('Okay')\n",
    );
    let found = wowlua_debug::ui::find_frames(&mut s.vm, "okbutton");
    assert_eq!(found[0]["name"], "OkButton");
    let rect = &found[0]["rect"];
    assert_eq!(
        (rect["x"].as_f64(), rect["y"].as_f64()),
        (Some(0.0), Some(0.0))
    );
    // A click in the button's rectangle (screen pixels) clicks it.
    let w = rect["width"].as_f64().unwrap();
    let input = Input::Click {
        frame: None,
        x: Some(w / 2.0),
        y: Some(10.0),
        button: "LeftButton".into(),
    };
    wowlua_debug::ui::input(&mut s.vm, &input).expect("clicked");
    assert!(matches!(s.vm.get_global("clicked"), Value::Num(n) if n == 1.0));
    // The scene has the button's text.
    let scene = wowlua_engine::scene::build(&mut s.vm);
    let text = scene.items.iter().find_map(|i| match i {
        wowlua_engine::scene::Item::Text(t) => Some(t.clone()),
        _ => None,
    });
    let text = text.expect("the label");
    let line: String = text.lines[0].runs.iter().map(|r| r.text.clone()).collect();
    assert_eq!(line, "Okay");
    assert!(text.lines[0].x > 0.0, "centered: {:?}", text.lines[0]);
}

/// A frame its mouse scripts alone make clickable, as the side tabs of the
/// character frame and the group finder (decision T365): `OnMouseUp`
/// turned the mouse on, the click reaches it; a frame without such a
/// script lets the click through.
#[test]
fn a_frame_with_mouse_scripts_takes_clicks_at_a_point() {
    let mut s = session();
    s.run(
        "tab.lua",
        "local tab = CreateFrame('Frame', 'SideTab', UIParent)\ntab:SetSize(50, 50)\ntab:SetPoint('TOPLEFT', 0, 0)\n\
         tab:SetScript('OnMouseUp', function(_, button, inside) upped = button end)\n\
         local plain = CreateFrame('Frame', 'PlainFrame', UIParent)\nplain:SetSize(50, 50)\nplain:SetPoint('TOPLEFT', 100, 0)\n\
         mouse = { tab:IsMouseEnabled(), plain:IsMouseEnabled() }\n",
    );
    let click = |x: f64| Input::Click {
        frame: None,
        x: Some(x),
        y: Some(25.0),
        button: "LeftButton".into(),
    };
    wowlua_debug::ui::input(&mut s.vm, &click(25.0)).expect("clicked the tab");
    match s.vm.get_global("upped") {
        Value::Str(b) => assert_eq!(s.vm.str_bytes(b), b"LeftButton"),
        other => panic!("OnMouseUp did not run: {other:?}"),
    }
    assert_eq!(
        wowlua_engine::scene::hit_test(&mut s.vm, 125.0, 25.0),
        wowlua_engine::scene::hit_test(&mut s.vm, 500.0, 500.0),
        "the plain frame took the mouse"
    );
}

#[test]
fn a_profile_report_has_functions_and_a_speedscope_file() {
    let mut s = session();
    let n = std::cell::Cell::new(0u64);
    s.vm.start_profiling(Box::new(move || {
        n.set(n.get() + 1);
        n.get()
    }));
    s.run(
        "prof.lua",
        "local function sq(x) return x * x end\nlocal s = 0\nfor i = 1, 100 do s = s + sq(i) end\n",
    );
    let p = s.vm.stop_profiling().expect("profiled");
    let r = wowlua_debug::profile::report(&p, &|c| {
        String::from_utf8_lossy(c)
            .strip_prefix("@")
            .map(str::to_string)
    });
    let sq = r["functions"]
        .as_array()
        .unwrap()
        .iter()
        .find(|f| f["name"] == "function src/prof.lua:1")
        .expect("sq");
    assert_eq!(sq["calls"], 100);
    let ss = &r["speedscope"];
    assert_eq!(ss["profiles"].as_array().unwrap().len(), 3);
    assert_eq!(ss["profiles"][0]["unit"], "none");
}

/// The speedscope file keeps the whole profile (decision T495): read back,
/// it has the calls, the dispatches' kinds and each frame's tree; another
/// tool's file reads as a tree of its samples.
#[test]
fn a_saved_profile_reads_back_with_its_frames() {
    use wowlua_debug::profile::Saved;
    use wowlua_vm::debug::DispatchKind;
    let mut s = session();
    s.run(
        "frames.lua",
        "function Tick(n) local x = 0 for i = 1, n do x = x + i end end\n",
    );
    let n = std::cell::Cell::new(0u64);
    s.vm.start_profiling(Box::new(move || {
        n.set(n.get() + 1);
        n.get()
    }));
    let tick = s.vm.get_global("Tick");
    for size in [5.0, 500.0] {
        let frame = s.vm.dispatch_enter(DispatchKind::Frame, || "frame".into());
        let update =
            s.vm.dispatch_enter(DispatchKind::Script, || "MyFrame:OnUpdate".into());
        s.vm.call(tick, &[wowlua_vm::Value::Num(size)])
            .expect("runs");
        s.vm.dispatch_exit(update);
        s.vm.dispatch_exit(frame);
    }
    let p = s.vm.stop_profiling().expect("profiled");
    let map = |c: &[u8]| {
        String::from_utf8_lossy(c)
            .strip_prefix("@")
            .map(str::to_string)
    };
    let saved = Saved::of(&p, &map, "two frames");
    let file = saved.to_speedscope();
    let back = Saved::from_speedscope(&file).expect("reads back");
    assert_eq!(back, saved);
    assert_eq!(back.frames.len(), 2);
    for f in 0..2 {
        let ours: Vec<_> = back
            .frame_tree(f)
            .iter()
            .map(|(n, t)| (n.node, n.calls, t.instructions))
            .collect();
        let vm: Vec<_> = p
            .frame_tree(f)
            .iter()
            .map(|(n, t)| (n.node, n.calls, t.instructions))
            .collect();
        assert_eq!(ours, vm);
    }
    let kinds: Vec<_> = back.keys.iter().filter_map(|k| k.kind.clone()).collect();
    assert!(kinds.contains(&"script".to_string()), "{kinds:?}");
    let tick = back
        .nodes
        .iter()
        .find(|n| back.keys[n.key as usize].name == "function src/frames.lua:1")
        .expect("Tick");
    assert_eq!(tick.calls, 2);
    // Without wowlua's part: the samples' tree, no calls or frames.
    let mut plain = file.clone();
    plain.as_object_mut().unwrap().remove("wowlua");
    let other = Saved::from_speedscope(&plain).expect("reads");
    assert!(other.frames.is_empty());
    let total = |s: &Saved| {
        s.nodes
            .iter()
            .map(|n| n.self_cost.instructions)
            .sum::<u64>()
    };
    assert_eq!(
        total(&other),
        total(&saved) - saved.nodes[0].self_cost.instructions
    );
    assert!(Saved::from_speedscope(&serde_json::json!({ "x": 1 })).is_none());
}
