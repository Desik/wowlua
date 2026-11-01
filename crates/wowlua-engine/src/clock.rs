//! The clock and timers (spec emulator §17.6): `GetTime` and `C_Timer` on
//! the VM's virtual clock, which tests advance (`mock.advanceTime`), and
//! `OnUpdate` per simulated frame (`mock.nextFrame`).
//!
//! Unverified until probed: `GetTime()` starts at 0; a timer handle is a
//! table with `Cancel`, `IsCancelled` and `Invoke`; within a frame, due
//! timers run before the `OnUpdate` scripts.

use wowlua_vm::debug::DispatchKind;
use wowlua_vm::heap::Userdata;
use wowlua_vm::stdlib::Ret;
use wowlua_vm::{NativeCall, TableRef, Value, Vm};

use crate::widgets::{self, WidgetId};
use crate::{engine, engine_mut};

/// A timer's payload in its handle.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TimerId(pub usize);

pub struct Timer {
    pub due: f64,
    pub every: f64,
    /// Runs left (`f64::INFINITY` for a ticker without a count).
    pub left: f64,
    pub cancelled: bool,
    /// The engine's (the server's answer), not an addon's.
    pub engine: bool,
    /// The callback and the handle it gets (none for `C_Timer.After`), kept
    /// in a registry table.
    pub slot: TableRef,
}

#[derive(Default)]
pub struct Timers {
    pub list: Vec<Timer>,
    /// The timers still to fire, by index (finished ones leave it: the
    /// list keeps every timer for its handle).
    pub active: Vec<usize>,
    handle_meta: Option<TableRef>,
    handle_methods: Option<TableRef>,
    /// While due timers run (`advance`): the time they run up to.
    advancing: Option<f64>,
    /// The last frame's seconds (`GetTickTime`), none before the first.
    pub tick_time: f64,
    /// The last second's frames' seconds, the newest last (`GetFramerate`,
    /// decision T538).
    pub recent: std::collections::VecDeque<f64>,
    /// Game frames run so far (the debugger's and profiler's frame names).
    pub frames_run: u64,
}

/// The server's round trip: a request's answer comes this much later (the
/// player's ping; 0.4 s unless a test sets it, near the recordings' ~0.3
/// s on WoW Forever's US servers).
pub fn server_delay(vm: &Vm) -> f64 {
    engine(vm).world.latency.unwrap_or(0.4)
}

/// Seconds since the session began (`GetTime`), in whole milliseconds as
/// the client's (recorded: `GetTime()`'s three decimals).
pub fn now(vm: &Vm) -> f64 {
    vm.clock.elapsed_ms.round() / 1000.0
}

fn get_time(vm: &mut Vm, _c: NativeCall) -> Ret {
    Ok(vec![Value::Num(now(vm))])
}

/// The handles' metatable: `Cancel`, `IsCancelled`, `Invoke`.
fn handle_meta(vm: &mut Vm) -> TableRef {
    if let Some(t) = engine(vm).timers.handle_meta {
        return t;
    }
    let methods = vm.new_table();
    for (name, f) in [
        ("Cancel", cancel as wowlua_vm::NativeFn),
        ("IsCancelled", is_cancelled),
        ("Invoke", invoke),
    ] {
        let native = vm.native(name, f);
        let key = vm.heap.intern(name.as_bytes());
        vm.heap.table_mut(methods).set_str(key, native);
    }
    crate::widgets::keep(vm, Value::Table(methods));
    engine_mut(vm).timers.handle_methods = Some(methods);
    // Handles are userdata whose metatable is hidden (`/stdtest engine`:
    // no methods table to read; that `__metatable` is false is unverified).
    // A handle keeps the fields written to it: warcraft.wiki.gg's
    // `API_C_Timer.NewTimer` ("user-defined fields written to handles can
    // be used to supply additional data"; patch 10.0.0), and Details!'s
    // Forever build stores its ticker's callback on the handle at login
    // (unverified by a probe).
    let meta = vm.new_table();
    for (name, f) in [
        ("__index", handle_index as wowlua_vm::NativeFn),
        ("__newindex", handle_newindex),
    ] {
        let native = vm.native(name, f);
        let key = vm.heap.intern(name.as_bytes());
        vm.heap.table_mut(meta).set_str(key, native);
    }
    let hidden = vm.heap.intern(b"__metatable");
    vm.heap.table_mut(meta).set_str(hidden, Value::Bool(false));
    crate::widgets::keep(vm, Value::Table(meta));
    engine_mut(vm).timers.handle_meta = Some(meta);
    meta
}

/// A handle's own fields (made on the first write), in its slot.
fn handle_fields(vm: &mut Vm, id: TimerId, make: bool) -> Option<TableRef> {
    let slot = engine(vm).timers.list[id.0].slot;
    match vm.heap.table(slot).get(Value::Num(3.0)) {
        Value::Table(t) => Some(t),
        _ if make => {
            let t = vm.new_table();
            let _ = vm
                .heap
                .table_mut(slot)
                .set(Value::Num(3.0), Value::Table(t));
            Some(t)
        }
        _ => None,
    }
}

/// `handle[key]`: a method, else a field written to it.
fn handle_index(vm: &mut Vm, c: NativeCall) -> Ret {
    let key = c.arg(2);
    if let Some(methods) = engine(vm).timers.handle_methods {
        let m = vm.heap.table(methods).get(key);
        if !m.is_nil() {
            return Ok(vec![m]);
        }
    }
    let v = match timer_of(vm, c.arg(1)).and_then(|id| handle_fields(vm, id, false)) {
        Some(t) => vm.heap.table(t).get(key),
        None => Value::Nil,
    };
    Ok(vec![v])
}

/// `handle[key] = value`: kept on the handle.
fn handle_newindex(vm: &mut Vm, c: NativeCall) -> Ret {
    if let Some(t) = timer_of(vm, c.arg(1)).and_then(|id| handle_fields(vm, id, true)) {
        let (k, v) = (c.arg(2), c.arg(3));
        if k.is_nil() {
            return Err(vm.error("table index is nil"));
        }
        let _ = vm.heap.table_mut(t).set(k, v);
    }
    Ok(vec![])
}

fn timer_of(vm: &Vm, v: Value) -> Option<TimerId> {
    let Value::Userdata(u) = v else { return None };
    vm.heap.userdata(u).data.downcast_ref::<TimerId>().copied()
}

fn cancel(vm: &mut Vm, c: NativeCall) -> Ret {
    if let Some(id) = timer_of(vm, c.arg(1)) {
        engine_mut(vm).timers.list[id.0].cancelled = true;
    }
    Ok(vec![])
}

fn is_cancelled(vm: &mut Vm, c: NativeCall) -> Ret {
    let cancelled = timer_of(vm, c.arg(1)).is_some_and(|id| engine(vm).timers.list[id.0].cancelled);
    Ok(vec![Value::Bool(cancelled)])
}

fn invoke(vm: &mut Vm, c: NativeCall) -> Ret {
    if let Some(id) = timer_of(vm, c.arg(1)) {
        fire(vm, id);
    }
    Ok(vec![])
}

/// Runs `callback` once after `seconds` (the engine's own timers: data
/// arriving from the server).
pub fn after_engine(vm: &mut Vm, seconds: f64, callback: Value) {
    schedule(vm, seconds, callback, 1.0, false);
    // Due at a whole millisecond, as the client's clock tells its packets
    // (a round trip from a recorded moment lands on the recorded moment,
    // not a hair after it), and run at that time within a frame
    // (`advance_frames`).
    if let Some(t) = engine_mut(vm).timers.list.last_mut() {
        t.due = (t.due * 1000.0).round() / 1000.0;
        t.engine = true;
    }
}

/// Schedules `callback` after `seconds`, `runs` times; returns its handle
/// when `handle`.
fn schedule(vm: &mut Vm, seconds: f64, callback: Value, runs: f64, handle: bool) -> Value {
    let id = TimerId(engine(vm).timers.list.len());
    let slot = vm.new_table();
    crate::widgets::keep(vm, Value::Table(slot));
    let _ = vm.heap.table_mut(slot).set(Value::Num(1.0), callback);
    let mut out = Value::Nil;
    if handle {
        let meta = handle_meta(vm);
        let ud = vm.heap.alloc_userdata(Userdata {
            serial: 0,
            meta: Some(meta),
            env: None,
            data: Box::new(id),
        });
        let _ = vm
            .heap
            .table_mut(slot)
            .set(Value::Num(2.0), Value::Userdata(ud));
        out = Value::Userdata(ud);
    }
    let mut due = now(vm) + seconds.max(0.0);
    // A timer a running timer sets without a delay waits for the next
    // run of timers (the next frame), so one that sets itself again does
    // not hold time still (unverified: the client's order).
    if seconds <= 0.0
        && let Some(target) = engine(vm).timers.advancing
    {
        due = due.max(target + NEXT_RUN);
    }
    let index = engine(vm).timers.list.len();
    engine_mut(vm).timers.active.push(index);
    engine_mut(vm).timers.list.push(Timer {
        due,
        every: seconds.max(0.0),
        left: runs,
        cancelled: false,
        engine: false,
        slot,
    });
    out
}

fn after(vm: &mut Vm, c: NativeCall) -> Ret {
    let s = vm.check_number(&c, 1)?;
    schedule(vm, s, c.arg(2), 1.0, false);
    Ok(vec![])
}

fn new_timer(vm: &mut Vm, c: NativeCall) -> Ret {
    let s = vm.check_number(&c, 1)?;
    Ok(vec![schedule(vm, s, c.arg(2), 1.0, true)])
}

fn new_ticker(vm: &mut Vm, c: NativeCall) -> Ret {
    let s = vm.check_number(&c, 1)?;
    let runs = match c.arg(3) {
        Value::Nil => f64::INFINITY,
        _ => vm.check_number(&c, 3)?,
    };
    Ok(vec![schedule(vm, s, c.arg(2), runs, true)])
}

/// Runs a timer's callback once (as the client's C code calls it).
fn fire(vm: &mut Vm, id: TimerId) {
    let slot = engine(vm).timers.list[id.0].slot;
    let callback = vm.heap.table(slot).get(Value::Num(1.0));
    let handle = vm.heap.table(slot).get(Value::Num(2.0));
    let args: Vec<Value> = if handle.is_nil() {
        vec![]
    } else {
        vec![handle]
    };
    let d = vm.dispatch_enter(DispatchKind::Timer, || "timer".to_string());
    vm.call_from_engine(callback, &args);
    vm.dispatch_exit(d);
}

/// Moves the clock by `ms` milliseconds (and the wall clock with it).
fn tick(vm: &mut Vm, ms: f64) {
    // The moment's end: its batched events (`batch`).
    if ms > 0.0 {
        crate::batch::flush(vm);
    }
    vm.clock.elapsed_ms += ms;
    vm.clock.now += ms / 1000.0;
}

/// Moves the clock by `seconds` without running anything (the client while
/// it loads: `/stdtest engine startup`, no timer or OnUpdate before the
/// loading screen goes).
pub fn jump(vm: &mut Vm, seconds: f64) {
    tick(vm, seconds.max(0.0) * 1000.0);
}

/// Timers overdue after loading come due now, once (a ticker does not
/// catch up: recorded one tick of a 0.5 s ticker after a 7 s load).
pub fn catch_up(vm: &mut Vm) {
    let now = now(vm);
    for t in engine_mut(vm).timers.list.iter_mut() {
        if t.due < now {
            t.due = now;
        }
    }
}

/// A time in whole microseconds. Timers come due by it, so the noise of
/// their sums (`now + 1`, a ticker's `due + every`: 4.733000000000001)
/// does not put one a hair past the end of the advance it is due in.
fn micros(seconds: f64) -> i64 {
    (seconds * 1e6).round() as i64
}

/// When a timer set or set again without a delay during a run of timers
/// is due: just after the run's end (the next run), never within it.
const NEXT_RUN: f64 = 1e-6;

/// Advances the clock by `seconds`, running due timers in the order they
/// come due (each at its time).
pub fn advance(vm: &mut Vm, seconds: f64) {
    // After `ReloadUI` the session's Lua is gone (the client loads a new
    // one): time passes, nothing of it runs.
    if engine(vm).world.logged_out {
        jump(vm, seconds);
        return;
    }
    // The clock's own milliseconds (`now` rounds them): timers due by
    // the time read at the end.
    let end_ms = vm.clock.elapsed_ms + seconds.max(0.0) * 1000.0;
    let target = end_ms.round() / 1000.0;
    let outer = engine_mut(vm).timers.advancing.replace(target);
    loop {
        let timers = &mut engine_mut(vm).timers;
        let list = &timers.list;
        timers
            .active
            .retain(|&i| !list[i].cancelled && list[i].left > 0.0);
        let next = timers
            .active
            .iter()
            .map(|&i| (i, &timers.list[i]))
            .filter(|(_, t)| micros(t.due) <= micros(target))
            .min_by(|a, b| a.1.due.total_cmp(&b.1.due).then(a.0.cmp(&b.0)))
            .map(|(i, t)| (TimerId(i), t.due));
        let Some((id, due)) = next else { break };
        // At its time, but not past the advance's end (a timer due within
        // the rounding of the end runs at the end: the clock does not
        // drift ahead of the steps' times).
        let due_ms = (due * 1000.0).min(end_ms);
        if due_ms > vm.clock.elapsed_ms {
            tick(vm, due_ms - vm.clock.elapsed_ms);
        }
        {
            // A ticker without an interval fires once a frame (once this
            // advance), not again and again at the same moment.
            let t = &mut engine_mut(vm).timers.list[id.0];
            t.left -= 1.0;
            t.due = if t.every > 0.0 {
                t.due + t.every
            } else {
                target + NEXT_RUN
            };
        }
        fire(vm, id);
    }
    engine_mut(vm).timers.advancing = outer;
    if end_ms > vm.clock.elapsed_ms {
        tick(vm, end_ms - vm.clock.elapsed_ms);
    }
}

/// `n` frames of `ms` milliseconds: each moves the clock, runs the
/// `OnSizeChanged` the last frame's resizing owes (before `OnUpdate`,
/// `/stdtest engine`), every visible widget's
/// `OnUpdate` (with the frame's seconds), the `OnTextChanged` the frame's
/// `SetText` owes (those before it and in `OnUpdate`), then the due
/// timers, those an `OnUpdate` just set included (`/stdtest engine`:
/// "update1 after0 after0 (set in update1) update2"; "update1 … changed
/// after0"). The engine's own timers (the server's answers) run at their
/// own times during the frame, before it ends: a packet's events at the
/// moment it comes, as the recordings time them (the client's frames are
/// not the engine's 60 a second; the profession's casts, recorded to the
/// millisecond, start at their recorded times).
pub fn advance_frames(vm: &mut Vm, n: usize, ms: f64) {
    for _ in 0..n {
        if engine(vm).world.logged_out {
            jump(vm, ms / 1000.0);
            continue;
        }
        let end_ms = vm.clock.elapsed_ms + ms;
        engine_mut(vm).timers.frames_run += 1;
        let number = engine(vm).timers.frames_run;
        let d = vm.dispatch_enter(DispatchKind::Frame, || format!("frame {number}"));
        engine_answers(vm, end_ms);
        tick(vm, end_ms - vm.clock.elapsed_ms);
        engine_mut(vm).timers.tick_time = ms / 1000.0;
        {
            let recent = &mut engine_mut(vm).timers.recent;
            recent.push_back(ms / 1000.0);
            while recent.len() > 1 && recent.iter().sum::<f64>() - recent[0] >= 1.0 {
                recent.pop_front();
            }
        }
        widgets::run_size_changes(vm);
        let count = engine(vm).world.widgets.len();
        let elapsed = Value::Num(ms / 1000.0);
        // The widgets given an `OnUpdate`, in creation order (one given it
        // during the frame too), not those made in it. The name interned as
        // when each widget was looked at: before the first, and after each
        // `OnUpdate` (a collection in it may have freed the string, and
        // interning it again counts towards the next). Between those, the
        // handler's lookup finds the name interned and changes nothing: it
        // waits until the widget shows.
        if count > 0 {
            vm.heap.intern(b"OnUpdate");
        }
        // (A widget given one before the one at `at` moves it on: the list
        // is caught up to `next`.)
        let mut next = 0;
        let mut at = 0;
        loop {
            let updating = &engine(vm).world.updating;
            while updating.get(at).is_some_and(|&i| i < next) {
                at += 1;
            }
            let Some(&i) = updating.get(at).filter(|&&i| i < count) else {
                break;
            };
            next = i + 1;
            let id = WidgetId(i);
            if widgets::visible(engine(vm), id) && widgets::has_handler(vm, id, "OnUpdate") {
                widgets::run_script(vm, id, "OnUpdate", &[elapsed]);
                if next < count {
                    vm.heap.intern(b"OnUpdate");
                }
            }
        }
        widgets::run_text_changes(vm);
        crate::animations::tick(vm, ms / 1000.0);
        crate::movement::tick(vm, ms / 1000.0);
        crate::unitacts::tick(vm);
        crate::camera::tick(vm, ms / 1000.0);
        crate::spellinfo::range_checks_tick(vm);
        crate::units::plates_tick(vm);
        crate::curves::update_text_bindings(vm);
        crate::bars::update_timers(vm);
        advance(vm, 0.0);
        vm.dispatch_exit(d);
    }
}

/// The engine's timers (the server's answers) due before the frame's end,
/// each at its time.
fn engine_answers(vm: &mut Vm, end_ms: f64) {
    loop {
        let timers = &mut engine_mut(vm).timers;
        let list = &timers.list;
        timers
            .active
            .retain(|&i| !list[i].cancelled && list[i].left > 0.0);
        let next = timers
            .active
            .iter()
            .map(|&i| (i, &timers.list[i]))
            .filter(|(_, t)| t.engine && t.due * 1000.0 < end_ms)
            .min_by(|a, b| a.1.due.total_cmp(&b.1.due).then(a.0.cmp(&b.0)))
            .map(|(i, t)| (TimerId(i), t.due));
        let Some((id, due)) = next else { break };
        if due * 1000.0 > vm.clock.elapsed_ms {
            tick(vm, due * 1000.0 - vm.clock.elapsed_ms);
        }
        engine_mut(vm).timers.list[id.0].left -= 1.0;
        fire(vm, id);
    }
}

/// `GetTickTime()`: the frame's seconds, as its `OnUpdate` got them
/// (unverified: before the first frame, 0).
fn get_tick_time(vm: &mut Vm, _c: NativeCall) -> Ret {
    Ok(vec![Value::Num(engine(vm).timers.tick_time)])
}

/// The frames a second the engine runs at (decision T538): the last
/// second's frames over their time, as fast as a realtime session's
/// frames really come (60 before any frame; unverified: the client's own
/// averaging).
pub fn framerate(vm: &Vm) -> f64 {
    let recent = &engine(vm).timers.recent;
    let seconds: f64 = recent.iter().sum();
    if recent.is_empty() || seconds <= 0.0 {
        return 60.0;
    }
    recent.len() as f64 / seconds
}

// ------------------------------------------------------- timed signal maps

/// `C_Timer.NewTimedSignalMap(callback)`: keys scheduled to signal at a
/// time, as Blizzard's documentation describes the client's
/// `TimedSignalMap`: when a key's time arrives it leaves the map and the
/// callback gets it (from the client's C code). Each key runs on a timer
/// of the clock's. Unverified: `GetNextSignal` on an empty map returns
/// nothing; keys due at the same moment signal in the order they were set.
fn new_timed_signal_map(vm: &mut Vm, c: NativeCall) -> Ret {
    let callback = c.arg(1);
    if !matches!(callback, Value::Func(_)) {
        return Err(vm.error("Usage: local signalMap = C_Timer.NewTimedSignalMap(callback)"));
    }
    let map = widgets::create(vm, "TimedSignalMap", None, None);
    let id = widgets::widget_of(vm, map).expect("a signal map");
    set_map_prop(vm, id, "callback", callback);
    let signals = Value::Table(vm.new_table());
    set_map_prop(vm, id, "signals", signals);
    Ok(vec![map])
}

fn map_prop(vm: &mut Vm, id: WidgetId, key: &str) -> Value {
    let props = engine(vm).world.widgets[id.0].props;
    let k = vm.str(key);
    vm.heap.table(props).get(k)
}

fn set_map_prop(vm: &mut Vm, id: WidgetId, key: &str, v: Value) {
    let props = engine(vm).world.widgets[id.0].props;
    let k = vm.str(key);
    let _ = vm.heap.table_mut(props).set(k, v);
}

/// The map's scheduled keys: key to `{ timer index, time }`.
fn signals(vm: &mut Vm, id: WidgetId) -> TableRef {
    match map_prop(vm, id, "signals") {
        Value::Table(t) => t,
        _ => {
            let t = vm.new_table();
            set_map_prop(vm, id, "signals", Value::Table(t));
            t
        }
    }
}

/// A scheduled key's timer index and time.
fn signal(vm: &mut Vm, id: WidgetId, key: f64) -> Option<(usize, f64)> {
    let t = signals(vm, id);
    let Value::Table(entry) = vm.heap.table(t).get(Value::Num(key)) else {
        return None;
    };
    let num = |vm: &Vm, i: f64| match vm.heap.table(entry).get(Value::Num(i)) {
        Value::Num(n) => n,
        _ => 0.0,
    };
    Some((num(vm, 1.0) as usize, num(vm, 2.0)))
}

fn cancel_signal_of(vm: &mut Vm, id: WidgetId, key: f64) {
    if let Some((timer, _)) = signal(vm, id, key) {
        engine_mut(vm).timers.list[timer].cancelled = true;
        let t = signals(vm, id);
        let _ = vm.heap.table_mut(t).set(Value::Num(key), Value::Nil);
    }
}

/// Schedules `key` at `time` (replacing its schedule).
fn signal_at_time(vm: &mut Vm, id: WidgetId, key: f64, time: f64) {
    cancel_signal_of(vm, id, key);
    let timer = engine(vm).timers.list.len();
    let map = Value::Table(engine(vm).world.widgets[id.0].table);
    let due = vm.native_with(
        "signal",
        signal_due,
        vec![map, Value::Num(key), Value::Num(timer as f64)],
    );
    schedule(vm, time - now(vm), due, 1.0, false);
    let entry = vm.new_table();
    let _ = vm
        .heap
        .table_mut(entry)
        .set(Value::Num(1.0), Value::Num(timer as f64));
    let _ = vm
        .heap
        .table_mut(entry)
        .set(Value::Num(2.0), Value::Num(time));
    let t = signals(vm, id);
    let _ = vm
        .heap
        .table_mut(t)
        .set(Value::Num(key), Value::Table(entry));
}

/// A key's timer came due: it leaves the map, and the callback gets it.
fn signal_due(vm: &mut Vm, c: NativeCall) -> Ret {
    let up = |i: usize| c.upvals.get(i).copied().unwrap_or(Value::Nil);
    let (Some(id), Value::Num(key), Value::Num(timer)) =
        (widgets::widget_of(vm, up(0)), up(1), up(2))
    else {
        return Ok(vec![]);
    };
    if signal(vm, id, key).map(|(t, _)| t) != Some(timer as usize) {
        return Ok(vec![]);
    }
    let t = signals(vm, id);
    let _ = vm.heap.table_mut(t).set(Value::Num(key), Value::Nil);
    let callback = map_prop(vm, id, "callback");
    vm.call(callback, &[Value::Num(key)])?;
    Ok(vec![])
}

fn signal_key(vm: &mut Vm, c: &NativeCall) -> Result<f64, wowlua_vm::LuaError> {
    vm.check_number(c, 2)
}

fn signal_at(vm: &mut Vm, id: WidgetId, c: NativeCall) -> Ret {
    let key = signal_key(vm, &c)?;
    let time = vm.check_number(&c, 3)?;
    signal_at_time(vm, id, key, time);
    Ok(vec![])
}

fn signal_after(vm: &mut Vm, id: WidgetId, c: NativeCall) -> Ret {
    let key = signal_key(vm, &c)?;
    let seconds = vm.check_number(&c, 3)?;
    let time = now(vm) + seconds;
    signal_at_time(vm, id, key, time);
    Ok(vec![])
}

fn cancel_signal(vm: &mut Vm, id: WidgetId, c: NativeCall) -> Ret {
    let key = signal_key(vm, &c)?;
    cancel_signal_of(vm, id, key);
    Ok(vec![])
}

/// The scheduled keys, in key order.
fn signal_keys(vm: &mut Vm, id: WidgetId) -> Vec<f64> {
    let t = signals(vm, id);
    let mut keys = Vec::new();
    let mut k = Value::Nil;
    while let Some((key, _)) = vm.heap.table(t).next(k).ok().flatten() {
        if let Value::Num(n) = key {
            keys.push(n);
        }
        k = key;
    }
    keys.sort_by(f64::total_cmp);
    keys
}

fn cancel_all_signals(vm: &mut Vm, id: WidgetId, _c: NativeCall) -> Ret {
    for key in signal_keys(vm, id) {
        cancel_signal_of(vm, id, key);
    }
    Ok(vec![])
}

fn has_signal(vm: &mut Vm, id: WidgetId, c: NativeCall) -> Ret {
    let key = signal_key(vm, &c)?;
    Ok(vec![Value::Bool(signal(vm, id, key).is_some())])
}

fn get_signal_time(vm: &mut Vm, id: WidgetId, c: NativeCall) -> Ret {
    let key = signal_key(vm, &c)?;
    Ok(vec![
        signal(vm, id, key).map_or(Value::Nil, |(_, t)| Value::Num(t)),
    ])
}

fn get_signal_count(vm: &mut Vm, id: WidgetId, _c: NativeCall) -> Ret {
    Ok(vec![Value::Num(signal_keys(vm, id).len() as f64)])
}

fn get_next_signal(vm: &mut Vm, id: WidgetId, _c: NativeCall) -> Ret {
    let mut next: Option<(f64, f64, usize)> = None;
    for key in signal_keys(vm, id) {
        if let Some((timer, time)) = signal(vm, id, key)
            && next.is_none_or(|(_, t, i)| (time, timer) < (t, i))
        {
            next = Some((key, time, timer));
        }
    }
    Ok(match next {
        Some((key, time, _)) => vec![Value::Num(key), Value::Num(time)],
        None => vec![],
    })
}

/// The documented functions this module gives behaviour.
pub fn behaviours() -> Vec<(&'static str, wowlua_vm::NativeFn)> {
    vec![
        ("GetTime", get_time),
        ("GetTickTime", get_tick_time),
        ("GetTimePreciseSec", get_time),
        ("C_Timer.After", after),
        ("C_Timer.NewTimer", new_timer),
        ("C_Timer.NewTicker", new_ticker),
        ("C_Timer.NewTimedSignalMap", new_timed_signal_map),
    ]
}

/// The timed signal maps' methods.
pub fn methods() -> Vec<(&'static str, widgets::MethodFn)> {
    vec![
        ("TimedSignalMap:SignalAt", signal_at as widgets::MethodFn),
        ("TimedSignalMap:SignalAfter", signal_after),
        ("TimedSignalMap:CancelSignal", cancel_signal),
        ("TimedSignalMap:CancelAllSignals", cancel_all_signals),
        ("TimedSignalMap:HasSignal", has_signal),
        ("TimedSignalMap:GetSignalTime", get_signal_time),
        ("TimedSignalMap:GetSignalCount", get_signal_count),
        ("TimedSignalMap:GetNextSignal", get_next_signal),
    ]
}

/// Seconds into the realm's day: the clock's local time of day (decision
/// T555).
pub fn time_of_day(vm: &Vm) -> f64 {
    (vm.clock.now + vm.clock.utc_offset as f64).rem_euclid(86_400.0)
}

/// Sets the realm's time of day, the day kept: the clock jumps (its timers
/// and `GetTime` do not move; decision T555).
pub fn set_time_of_day(vm: &mut Vm, seconds: f64) {
    let now = vm.clock.now;
    vm.clock.now = now - time_of_day(vm) + seconds.rem_euclid(86_400.0);
}
