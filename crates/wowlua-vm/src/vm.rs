//! The virtual machine: loading, calls, the interpreter loop, metamethods
//! and errors. Lua-to-Lua calls never recurse on the Rust stack; a native
//! function that calls Lua (pcall, sort comparators, metamethods) runs a
//! nested loop, which is where Lua 5.1 draws its C-call boundary too.

use std::rc::Rc;

use crate::compiler;
use crate::error::LuaError;
use crate::function::{Args, Function, LuaFunction, NativeCall, NativeFn, NativeFunction, Upval};
use crate::heap::{FuncRef, Heap, StrRef, TableRef, ThreadRef, UpvalRef};
use crate::number::{fmt_num, str2num};
use crate::proto::{ArithOp, Op, Proto, Rk};
use crate::table::Table;
use crate::taint::{Origin, Slot, Taint, TaintState};
use crate::thread::{Frame, Status, Thread, ThreadState};
use crate::value::Value;

/// `LUAI_MAXCALLS`: a thread's call-info array (one slot per Lua or native
/// call, and a base one) grows by doubling from `BASIC_CI_SIZE`; growing
/// past this size raises "stack overflow", so the first overflow comes at
/// 16384 calls. After it, the array is cut back to this size, and the
/// collector may halve it (`ThreadState::shrink_ci`).
pub(crate) const MAXCALLS: usize = 20000;
/// Nested native-to-Lua calls (`LUAI_MAXCCALLS`).
pub(crate) const MAXCCALLS: usize = 200;
/// `__index`/`__newindex` chains (`MAXTAGLOOP`).
const MAXTAGLOOP: usize = 100;
const LFIELDS_PER_FLUSH: usize = 50;

macro_rules! names {
    ($($field:ident = $text:literal),* $(,)?) => {
        /// Interned metamethod and library names.
        #[derive(Clone, Copy)]
        pub struct Names { $(pub $field: StrRef),* }
        impl Names {
            fn new(heap: &mut Heap) -> Names {
                Names { $($field: heap.intern($text.as_bytes())),* }
            }
            pub fn all(&self) -> Vec<StrRef> {
                vec![$(self.$field),*]
            }
        }
    };
}

names! {
    index = "__index",
    newindex = "__newindex",
    call = "__call",
    add = "__add",
    sub = "__sub",
    mul = "__mul",
    div = "__div",
    mod_ = "__mod",
    pow = "__pow",
    unm = "__unm",
    len = "__len",
    lt = "__lt",
    le = "__le",
    eq = "__eq",
    concat = "__concat",
    gc = "__gc",
    mode = "__mode",
    metatable = "__metatable",
    tostring = "__tostring",
    tostring_fn = "tostring",
    n = "n",
}

/// A native function on the call stack (for levels and messages).
#[derive(Clone, Copy)]
pub(crate) struct CEntry {
    /// Lua frames below it.
    pub at_frames: usize,
    /// The stack slot of the called function (its arguments follow).
    pub slot: usize,
    pub func: FuncRef,
    /// Called directly by a Lua instruction (`CALL`, `TFORLOOP`).
    pub from_lua: bool,
}

/// Named values: a level's locals or upvalues.
pub type Variables = Vec<(String, Value)>;

/// One level of the call stack, as `lua_getinfo` describes it.
#[derive(Clone, Debug)]
pub struct Level {
    pub short_src: Vec<u8>,
    pub source: Vec<u8>,
    /// -1 when unknown (native functions, tail calls).
    pub currentline: i64,
    pub linedefined: i64,
    /// "main", "Lua", "C" or "tail".
    pub what: &'static str,
    /// How the caller named it: ("global" | "local" | "method" | "field" |
    /// "upvalue", name).
    pub name: Option<(&'static str, String)>,
    pub(crate) entry: Option<LevelEntry>,
}

#[derive(Clone, Copy, Debug)]
pub(crate) enum LevelEntry {
    Lua(usize),
    Native(usize),
}

/// One entry of the combined Lua and native call stack.
pub(crate) enum StackEntry {
    Lua(usize),
    Native(usize),
}

/// What the client changed in Lua 5.1's rules ([`Vm::wow`]); all off in
/// stock Lua.
#[derive(Clone, Copy, Debug, Default)]
pub struct Rules {
    /// Division and modulo check their number operands ("Division by
    /// zero", elune's `LUA_EXCEPTFPESTRICT`).
    pub fpe_strict: bool,
    /// The same for numbers coerced from strings (`LUA_EXCEPTFPECOERCE`).
    pub fpe_coerce: bool,
    /// Integer arguments must fit 32 bits (`LUA_EXCEPTOVERFLOW`).
    pub int_overflow: bool,
    /// `string.format` as the client's (positional arguments, `%F`).
    pub client_format: bool,
    /// `xpcall(f, handler, ...)` passes the extra arguments to `f`.
    pub xpcall_args: bool,
    /// The client's error messages: calling a non-function is "attempt to
    /// call a nil value", assigning through one "attempt to perform indexed
    /// assignment on".
    pub client_errors: bool,
    /// `string.upper`/`lower` map UTF-8 characters, not only ASCII.
    pub utf8_case: bool,
    /// The client's Lua runs with its API assertions on, and a C function
    /// gets `LUA_MINSTACK` = 100 free slots: where its results overflow
    /// them (`pcall` returning `#args + 100` or more values), the
    /// assertion `L->top < L->ci->top` crashes the game. The VM raises an
    /// error naming the call instead (decision T194).
    pub assertions: bool,
}

/// The VM's clock: virtual, so runs repeat exactly (spec emulator §17.2).
#[derive(Clone, Copy, Debug)]
pub struct Clock {
    /// Seconds since the epoch (`time()`).
    pub now: f64,
    /// Local time minus UTC, in seconds (`date` without `!`).
    pub utc_offset: i64,
    /// Milliseconds since the session began (`debugprofilestop`).
    pub elapsed_ms: f64,
    pub(crate) profile_start: f64,
}

impl Default for Clock {
    fn default() -> Clock {
        Clock {
            // 2026-01-01 12:00:00 UTC: noon, the world's time of day as the
            // realm's (decision T555).
            now: 1_767_268_800.0,
            utc_offset: 0,
            elapsed_ms: 0.0,
            profile_start: 0.0,
        }
    }
}

/// A call's check: why the VM refuses to call a function, or nothing.
pub type CallGuard = dyn Fn(&Vm, FuncRef) -> Option<String>;

pub struct Vm {
    /// The client's changes to Lua's rules.
    pub rules: Rules,
    /// Taints for a native's results, set by the native just before it
    /// returns (else its results take the write taint).
    pub result_taints: Vec<Taint>,
    taint_names: Vec<Rc<str>>,
    taint_ids: std::collections::HashMap<Rc<str>, Taint>,
    pub clock: Clock,
    /// `geterrorhandler`/`seterrorhandler` and the print handler (WoW).
    pub error_handler: Value,
    /// Told of each error a client function drops without a report (the
    /// error and the stack where it was raised): `secureexecuterange`'s
    /// callbacks fail without a trace in the client, but the host's tests
    /// see them (code that fails is a failure even where nobody hears it).
    pub on_dropped_error: Option<fn(&mut Vm, String, String)>,
    pub heap: Heap,
    pub(crate) cur: ThreadState,
    pub(crate) current: ThreadRef,
    pub(crate) main: ThreadRef,
    pub globals: TableRef,
    pub registry: TableRef,
    pub string_meta: Option<TableRef>,
    pub names: Names,
    pub(crate) c_depth: usize,
    /// The running native took a secret's value: its results are secret.
    pub(crate) secret_used: bool,
    /// Where taint came from (`crate::origin`).
    pub(crate) origins: crate::origin::Origins,
    /// `WOWLUA_TAINT_TRACE`: log where execution becomes tainted
    /// (`crate::origin`); `1` or `all` for every site, else only sites
    /// whose source contains the text.
    pub taint_trace: Option<String>,
    /// The host's state (the engine's, spec emulator §17.6), for its
    /// natives.
    pub host: Option<Box<dyn std::any::Any>>,
    /// The host's answer to whether an object keeps secrets its layout
    /// passes from untrusted code (`hooksecurefunc`'s hooks on it: the
    /// engine's `UntrustedLayoutScriptExecution`).
    pub untrusted_layout_forbidden: Option<fn(&Vm, Value) -> bool>,
    /// The running native may take secrets' values even while tainted
    /// (`tostring`, `string.format`, `strjoin`: their results are secret).
    pub(crate) secret_reveal: bool,
    pub(crate) cstack: Vec<CEntry>,
    pub(crate) errfunc: Option<Value>,
    /// The error handler proxy (`error_handler_proxy`), made once and kept
    /// alive: a handler saved while a nested call from the engine or a
    /// `pcall` runs is held only by Rust, and the collector freed a fresh
    /// one under AtlasLoot's login (a stale handle when it was restored).
    pub(crate) errfunc_proxy: Option<Value>,
    in_errfunc: bool,
    output: Output,
    /// Coverage counts, when on ([`Vm::enable_coverage`]).
    pub coverage: Option<crate::coverage::Coverage>,
    /// The counts' index of the running frame's function.
    pub(crate) cov_frame: Option<usize>,
    /// Which hooks the running frame's instructions take (`crate::debug`):
    /// 0 unless something listens.
    pub(crate) hook_mask: u8,
    /// The hook bits every frame takes (profiling, a debugger attached).
    pub(crate) hook_base: u8,
    /// Anything listens: frames work their hook bits out.
    pub(crate) hooks_on: bool,
    /// Calls and returns are reported (profiling, function breakpoints).
    pub(crate) call_hooks: bool,
    /// Asked before every call: a reason refuses it (a debugger's watch
    /// that only reads).
    pub(crate) call_guard: Option<Box<CallGuard>>,
    /// Instructions run while hooks were on.
    pub(crate) instructions: u64,
    /// `pcall`s, `xpcall`s and `coroutine.resume`s running: an error
    /// raised now is caught.
    pub(crate) protected: u32,
    pub(crate) aborted: bool,
    pub(crate) debugger: Option<Box<dyn crate::debug::Debugger>>,
    pub(crate) dbg: Option<Box<crate::debug::DebugState>>,
    pub(crate) profiler: Option<Box<crate::profile::Profiler>>,
    /// Values kept alive outside the heap (host references).
    pub(crate) pinned: Vec<Value>,
    pub(crate) gc_threshold: usize,
    pub(crate) gc_stopped: bool,
    /// `collectgarbage("setpause")`, percent.
    pub(crate) gc_pause: usize,
    /// `collectgarbage("setstepmul")`, percent.
    pub(crate) gc_stepmul: usize,
    /// Work left in the cycle `collectgarbage("step")` emulates.
    pub(crate) gc_cycle_work: Option<usize>,
    /// Userdata marked for finalization (their `__gc` ran or is pending).
    pub(crate) finalized: std::collections::HashSet<crate::heap::UdRef>,
    /// Userdata whose `__gc` is pending, in order.
    pub(crate) to_finalize: std::collections::VecDeque<crate::heap::UdRef>,
    pub(crate) in_finalizer: bool,
    /// Nested `execute` runs (for the yield boundary).
    pub(crate) run_depth: usize,
    /// The run a resumed coroutine's frames execute in.
    pub(crate) co_base: usize,
    /// The yield in progress: its call site and values.
    pub(crate) yield_site: Option<(usize, i32)>,
    pub(crate) yield_values: Vec<Slot>,
    /// Threads that resumed the running one (kept alive).
    pub(crate) resume_chain: Vec<ThreadRef>,
    pub(crate) rand_state: u32,
    /// Collections run so far.
    pub gc_cycles: usize,
}

/// Where `print` writes.
type Output = Box<dyn FnMut(&[u8])>;

impl Default for Vm {
    fn default() -> Self {
        Vm::new()
    }
}

/// A native call's arguments, their taints and their origins, in order
/// (up to [`Args`]' size in one pass over the slots, in place).
fn split_slots(slots: &[Slot]) -> (Args<Value>, Args<Taint>, Args<Origin>) {
    const N: usize = 6;
    if slots.len() > N {
        return (
            slots.iter().map(|s| s.v).collect(),
            slots.iter().map(|s| s.t).collect(),
            slots.iter().map(|s| s.o).collect(),
        );
    }
    let mut v = [Value::Nil; N];
    let mut t = [Taint::SECURE; N];
    let mut o = [Origin::NONE; N];
    for (i, s) in slots.iter().enumerate() {
        (v[i], t[i], o[i]) = (s.v, s.t, s.o);
    }
    let n = slots.len();
    (
        Args::from_buf_and_len(v, n),
        Args::from_buf_and_len(t, n),
        Args::from_buf_and_len(o, n),
    )
}

impl Vm {
    /// A VM with Lua 5.1's base library and the standard libraries.
    pub fn new() -> Vm {
        let mut heap = Heap::default();
        let names = Names::new(&mut heap);
        let globals = heap.alloc_table(Table::new(0, 64));
        let registry = heap.alloc_table(Table::new(0, 8));
        let main = heap.alloc_thread(Thread {
            status: Status::Running,
            state: ThreadState::default(),
            start: None,
            yield_site: None,
            globals,
        });
        let mut vm = Vm {
            rules: Rules::default(),
            result_taints: Vec::new(),
            taint_names: Vec::new(),
            taint_ids: std::collections::HashMap::new(),
            clock: Clock::default(),
            error_handler: Value::Nil,
            heap,
            cur: ThreadState::default(),
            current: main,
            main,
            globals,
            registry,
            string_meta: None,
            names,
            c_depth: 0,
            secret_used: false,
            origins: Default::default(),
            taint_trace: std::env::var("WOWLUA_TAINT_TRACE")
                .ok()
                .filter(|v| !v.is_empty() && v != "0"),
            host: None,
            untrusted_layout_forbidden: None,
            secret_reveal: false,
            cstack: Vec::new(),
            errfunc: None,
            errfunc_proxy: None,
            in_errfunc: false,
            on_dropped_error: None,
            coverage: None,
            cov_frame: None,
            hook_mask: 0,
            hook_base: 0,
            hooks_on: false,
            call_hooks: false,
            call_guard: None,
            instructions: 0,
            protected: 0,
            aborted: false,
            debugger: None,
            dbg: None,
            profiler: None,
            output: Box::new(|bytes| {
                use std::io::Write;
                let _ = std::io::stdout().write_all(bytes);
            }),
            pinned: Vec::new(),
            gc_threshold: 4 << 20,
            gc_stopped: false,
            gc_pause: 200,
            gc_stepmul: 200,
            gc_cycle_work: None,
            finalized: std::collections::HashSet::new(),
            to_finalize: std::collections::VecDeque::new(),
            in_finalizer: false,
            run_depth: 0,
            co_base: 0,
            yield_site: None,
            yield_values: Vec::new(),
            resume_chain: Vec::new(),
            rand_state: 1,
            gc_cycles: 0,
        };
        vm.cur.stack.resize(64, Slot::NIL);
        crate::stdlib::open(&mut vm);
        vm
    }

    /// Where `print` writes.
    pub fn set_output(&mut self, out: impl FnMut(&[u8]) + 'static) {
        self.output = Box::new(out);
    }

    pub(crate) fn write_output(&mut self, bytes: &[u8]) {
        (self.output)(bytes);
    }

    // ------------------------------------------------------------- strings

    pub fn str(&mut self, s: &str) -> Value {
        Value::Str(self.heap.intern(s.as_bytes()))
    }

    pub fn bytes(&mut self, b: &[u8]) -> Value {
        Value::Str(self.heap.intern(b))
    }

    pub fn str_bytes(&self, s: StrRef) -> &[u8] {
        self.heap.str_bytes(s)
    }

    // ----------------------------------------------------------- functions

    pub fn native(&mut self, name: &'static str, f: NativeFn) -> Value {
        self.native_with(name, f, Vec::new())
    }

    pub fn native_with(&mut self, name: &'static str, f: NativeFn, upvals: Vec<Value>) -> Value {
        let slots = upvals.into_iter().map(Slot::secure).collect();
        self.native_slots(name, f, slots)
    }

    /// A native with upvalues that keep their taints.
    pub fn native_slots(&mut self, name: &'static str, f: NativeFn, upvals: Vec<Slot>) -> Value {
        let env = self.globals;
        let taint = self.cur.taint.alloc(true);
        Value::Func(self.heap.alloc_func(Function::Native(NativeFunction {
            f,
            name,
            upvals: upvals.iter().map(|s| s.v).collect(),
            upval_taints: upvals.iter().map(|s| s.t).collect(),
            env,
            taint,
        })))
    }

    /// The taint of an addon (or any name).
    pub fn taint_named(&mut self, name: &str) -> Taint {
        if let Some(&t) = self.taint_ids.get(name) {
            return t;
        }
        self.taint_names.push(Rc::from(name));
        let t = Taint(self.taint_names.len() as u32);
        self.taint_ids.insert(Rc::from(name), t);
        t
    }

    /// A taint's name; none for secure.
    pub fn taint_name(&self, t: Taint) -> Option<String> {
        if t.is_secure() {
            return None;
        }
        self.taint_names
            .get(t.0 as usize - 1)
            .map(|n| n.to_string())
    }

    pub fn set_global(&mut self, name: &str, v: Value) {
        let k = self.heap.intern(name.as_bytes());
        let g = self.globals;
        self.heap.table_mut(g).set_str(k, v);
    }

    /// A global, raw, but through the globals' table-valued `__index`
    /// (an environment reading through to another, as WoW's secure one).
    pub fn get_global(&mut self, name: &str) -> Value {
        let k = self.heap.intern(name.as_bytes());
        let mut t = self.globals;
        for _ in 0..8 {
            let v = self.heap.table(t).get_str(k);
            if !v.is_nil() {
                return v;
            }
            let next = self
                .heap
                .table(t)
                .meta
                .map(|m| self.heap.table(m).get_str(self.names.index));
            match next {
                Some(Value::Table(n)) => t = n,
                _ => return Value::Nil,
            }
        }
        Value::Nil
    }

    pub fn new_table(&mut self) -> TableRef {
        self.heap.alloc_table(Table::default())
    }

    /// Compiles a chunk. `chunkname` follows Lua: `@file`, `=name`, or the
    /// source itself.
    pub fn load(&mut self, src: &[u8], chunkname: &str) -> Result<Value, String> {
        self.load_raw(src, chunkname.as_bytes())
            .map_err(|e| String::from_utf8_lossy(&e).into_owned())
    }

    /// Loads code as an addon's (`luaL_loadbufferas`): its functions and
    /// constants carry the addon's taint, so running them taints execution.
    pub fn load_as(&mut self, src: &[u8], chunkname: &str, addon: &str) -> Result<Value, String> {
        let saved = self.cur.taint.saved();
        let taint = self.taint_named(addon);
        self.cur.taint.stack = taint;
        let r = self.load(src, chunkname);
        self.cur.taint.restore(saved);
        r
    }

    /// [`load`](Self::load) on bytes, as Lua strings are: the error message
    /// keeps the chunk name's and the source's bytes.
    pub fn load_raw(&mut self, src: &[u8], chunkname: &[u8]) -> Result<Value, Vec<u8>> {
        let mut message = chunkid_sized(chunkname, 80);
        // Lua 5.1 skips a first line starting with `#` in files only;
        // `loadstring` does not, so neither do we.
        let file = wowlua_span::SourceDb::new().add("", "");
        let parse = wowlua_syntax::parse(src, file);
        let options = compiler::Options {
            c_depth: self.c_depth,
            ..Default::default()
        };
        let compiled = compiler::compile_with(
            &mut self.heap,
            parse.view(),
            src,
            Rc::from(chunkname),
            options,
        );
        if let Some(d) = parse.diagnostics.iter().find(|d| d.is_error()) {
            let span = d.labels.first().map(|l| l.span);
            let line = span.map_or(1, |s| line_of(src, s.start as usize));
            // Lua compiles while it parses: a limit met before the syntax
            // error is the error.
            let earlier = compiled.as_ref().err().filter(|e| (e.line as usize) < line);
            match earlier {
                Some(e) => {
                    message.extend_from_slice(format!(":{}: {}", e.line, e.message).as_bytes())
                }
                None => {
                    message.extend_from_slice(format!(":{line}: ").as_bytes());
                    message.extend_from_slice(&raw_near(&d.message, span, src));
                }
            }
            return Err(message);
        }
        let mut proto = compiled.map_err(|e| {
            message.extend_from_slice(format!(":{}: {}", e.line, e.message).as_bytes());
            message
        })?;
        // The constants take the taint of the code loading them.
        set_const_taint(&mut proto, self.cur.taint.write());
        if let Some(c) = self.coverage.as_mut() {
            c.register(&proto);
        }
        if self.debugger.is_some() {
            self.load_hook(&proto);
        }
        let env = self.globals;
        let taint = self.cur.taint.alloc(true);
        Ok(Value::Func(self.heap.alloc_func(Function::Lua(
            LuaFunction {
                proto,
                upvals: Box::new([]),
                env,
                taint,
            },
        ))))
    }

    // --------------------------------------------------------------- calls

    fn ensure_stack(&mut self, size: usize) {
        if self.cur.stack.len() < size {
            let new = (size + 64).max(self.cur.stack.len() * 2);
            self.cur.stack.resize(new, Slot::NIL);
        }
    }

    /// A native's results as they are, taints included: sets
    /// [`result_taints`](Self::result_taints) and returns the values.
    pub fn return_slots(&mut self, slots: Vec<Slot>) -> Vec<Value> {
        self.result_taints.clear();
        self.result_taints.extend(slots.iter().map(|s| s.t));
        slots.into_iter().map(|s| s.v).collect()
    }

    /// The running thread's taint.
    pub fn taint(&self) -> &TaintState {
        &self.cur.taint
    }

    pub fn taint_mut(&mut self) -> &mut TaintState {
        &mut self.cur.taint
    }

    /// A register's value.
    fn reg(&self, i: usize) -> Value {
        self.cur.stack[i].v
    }

    /// Writes a value made now (`setnvalue` and the like).
    fn set_fresh(&mut self, i: usize, v: Value) {
        self.cur.stack[i] = self.cur.taint.fresh(v);
    }

    /// Copies a stored value into a register (`setobj2s`).
    fn set_copy(&mut self, i: usize, s: Slot) {
        self.cur.stack[i] = self.cur.taint.copy(s);
        if self.cur.taint.tainted_by.is_some() {
            self.settle_origin();
        }
    }

    /// Calls `f` with `args`, as a native pushes them (values made now);
    /// returns all results. Errors unwind the call.
    pub fn call(&mut self, f: Value, args: &[Value]) -> Result<Vec<Value>, LuaError> {
        let f = self.cur.taint.fresh(f);
        let args: Vec<Slot> = args.iter().map(|&v| self.cur.taint.fresh(v)).collect();
        Ok(self
            .call_slots(f, &args)?
            .into_iter()
            .map(|s| s.v)
            .collect())
    }

    /// The environment of the function `level` calls up from a native (1:
    /// the native's caller), or the globals.
    pub fn caller_env(&self, level: usize) -> TableRef {
        match self.stack_entries().get(level) {
            Some(StackEntry::Lua(fi)) => self.heap.func(self.cur.frames[*fi].closure).env(),
            _ => self.globals,
        }
    }

    /// The chunk name (`@Interface/AddOns/…/file.lua`) of the Lua function
    /// `level` calls up from a native (1: the native's caller).
    pub fn caller_source(&self, level: usize) -> Option<Vec<u8>> {
        match self.stack_entries().get(level) {
            Some(StackEntry::Lua(fi)) => match self.heap.func(self.cur.frames[*fi].closure) {
                Function::Lua(f) => Some(f.proto.source.to_vec()),
                _ => None,
            },
            _ => None,
        }
    }

    /// The chunk name and line where the nearest Lua function, `level` or
    /// more calls up from a native (1: the native's caller), is now (natives
    /// between skipped: a template's, a wrapper's).
    pub fn caller_place(&self, level: usize) -> Option<(Rc<[u8]>, u32)> {
        self.stack_entries()
            .iter()
            .skip(level)
            .find_map(|e| match e {
                StackEntry::Lua(fi) => {
                    let f = &self.cur.frames[*fi];
                    let line = f
                        .proto
                        .lines
                        .get(f.pc.saturating_sub(1))
                        .copied()
                        .unwrap_or(0);
                    Some((f.proto.source.clone(), line))
                }
                _ => None,
            })
    }

    /// Sets the environment of the Lua function `level` calls up from a
    /// native (1: the native's caller); false if that is no Lua function.
    pub fn set_caller_env(&mut self, level: usize, env: TableRef) -> bool {
        match self.stack_entries().get(level) {
            Some(StackEntry::Lua(fi)) => {
                let f = self.cur.frames[*fi].closure;
                self.heap.func_mut(f).set_env(env);
                true
            }
            _ => false,
        }
    }

    /// Whether the Lua function `level` calls up from a native (1: the
    /// native's caller) is secure code (loaded untainted); false for a
    /// native caller or none.
    pub fn caller_is_secure(&self, level: usize) -> bool {
        match self.stack_entries().get(level) {
            Some(StackEntry::Lua(fi)) => self
                .heap
                .func(self.cur.frames[*fi].closure)
                .taint()
                .is_secure(),
            _ => false,
        }
    }

    /// How many calls deep the running coroutine is.
    pub fn call_depth(&self) -> usize {
        self.cur.frames.len()
    }

    /// Calls `f` with `args` as they are (taints included); returns the
    /// result slots as the callee left them.
    pub fn call_slots(&mut self, f: Slot, args: &[Slot]) -> Result<Vec<Slot>, LuaError> {
        let func = self.cur.top.max(self.frame_top());
        self.ensure_stack(func + args.len() + 1);
        self.cur.stack[func] = f;
        self.cur.stack[func + 1..func + 1 + args.len()].copy_from_slice(args);
        let saved_frames = self.cur.frames.len();
        let saved_c = self.cstack.len();
        let saved_depth = self.c_depth;
        let saved_top = self.cur.top;
        match self.call_at(func, args.len(), -1, false) {
            Ok(()) => {
                let results = self.cur.stack[func..self.cur.top].to_vec();
                self.cur.top = saved_top;
                Ok(results)
            }
            Err(mut e) => {
                if self.debugger.is_some() {
                    self.error_hook(&mut e);
                }
                self.unwind(saved_frames, saved_c, func);
                if self.call_hooks {
                    self.return_hook(saved_frames + saved_c);
                }
                self.c_depth = saved_depth;
                self.cur.top = saved_top;
                Err(e)
            }
        }
    }

    /// Like [`call`](Self::call), but errors are returned without running
    /// the error handler of an enclosing `xpcall` (Lua's `lua_pcall` with no
    /// handler).
    pub fn pcall(&mut self, f: Value, args: &[Value]) -> Result<Vec<Value>, LuaError> {
        let saved = self.errfunc.take();
        let r = self.call(f, args);
        self.errfunc = saved;
        r
    }

    /// Restores the stack after an error: frames, native entries and open
    /// upvalues above `level`.
    pub(crate) fn unwind(&mut self, frames: usize, cstack: usize, level: usize) {
        self.cur.frames.truncate(frames);
        self.cstack.truncate(cstack);
        // After an overflow, the array goes back to `LUAI_MAXCALLS` slots
        // (`restore_stack_limit`).
        if self.cur.ci_size > MAXCALLS && self.cur.ci_used() + 1 < MAXCALLS {
            self.cur.ci_size = MAXCALLS;
        }
        self.close_upvals(level);
    }

    /// The first free slot above the running frame.
    fn frame_top(&self) -> usize {
        match self.cur.frames.last() {
            Some(f) => f.base + f.proto.max_stack as usize,
            None => 0,
        }
    }

    /// Calls the function at `func` with `nargs` arguments above it, leaving
    /// the results at `func` (`nresults` of them, or all with -1 and `top`
    /// set). A nested run: counts toward the C-call limit.
    pub(crate) fn call_at(
        &mut self,
        func: usize,
        nargs: usize,
        nresults: i32,
        from_lua: bool,
    ) -> Result<(), LuaError> {
        self.c_depth += 1;
        if self.c_depth >= MAXCCALLS {
            if self.c_depth == MAXCCALLS {
                self.c_depth -= 1;
                return Err(self.rt_error("C stack overflow"));
            }
            if self.c_depth >= MAXCCALLS + (MAXCCALLS >> 3) {
                self.c_depth -= 1;
                let v = self.str("error in error handling");
                return Err(LuaError::new(v));
            }
        }
        let r = self.run_call(func, nargs, nresults, from_lua);
        self.c_depth -= 1;
        r
    }

    /// Runs a call to its end in a nested `execute`, without counting it.
    pub(crate) fn run_call(
        &mut self,
        func: usize,
        nargs: usize,
        nresults: i32,
        from_lua: bool,
    ) -> Result<(), LuaError> {
        match self.precall(func, nargs, nresults, from_lua) {
            Ok(true) => {
                self.cur.frames.last_mut().expect("frame").entry = true;
                self.execute()
            }
            Ok(false) => Ok(()),
            Err(e) => Err(e),
        }
    }

    /// Takes a call-info slot for a call (`inc_ci`): a full array doubles,
    /// and past `LUAI_MAXCALLS` the call fails, once more while handling
    /// that with "error in error handling" (`growCI`).
    fn enter_ci(&mut self) -> Result<(), LuaError> {
        let size = self.cur.ci_size();
        if 1 + self.cur.ci_used() < size {
            return Ok(());
        }
        if size > MAXCALLS {
            let v = self.str("error in error handling");
            return Err(LuaError::new(v));
        }
        self.cur.ci_size = size * 2;
        if self.cur.ci_size > MAXCALLS {
            return Err(self.rt_error("stack overflow"));
        }
        Ok(())
    }

    /// Prepares a call. Lua functions get a frame (true); natives run now.
    fn precall(
        &mut self,
        func: usize,
        nargs: usize,
        nresults: i32,
        from_lua: bool,
    ) -> Result<bool, LuaError> {
        let mut nargs = nargs;
        let f = match self.reg(func) {
            Value::Func(f) => f,
            v @ Value::Secret(_) if self.can_access_secrets() => {
                return self.secret_call(func, v, nargs, nresults, from_lua);
            }
            other => {
                // `__call`
                let tm = self.get_tm_slot(other, self.names.call);
                let Value::Func(f) = tm.v else {
                    return Err(self.call_error(func));
                };
                self.ensure_stack(func + nargs + 2);
                for i in (func..=func + nargs).rev() {
                    let s = self.cur.stack[i];
                    self.set_copy(i + 1, s);
                }
                self.set_copy(func, tm);
                nargs += 1;
                f
            }
        };
        if let Some(guard) = &self.call_guard
            && let Some(why) = guard(self, f)
        {
            return Err(self.error(&why));
        }
        self.enter_ci()?;
        // Calls clear the fixed taint (`luaD_precall`); entering a Lua
        // frame sets it again.
        self.cur.taint.fixed = Taint::SECURE;
        match self.heap.func(f) {
            Function::Lua(lf) => {
                let proto = lf.proto.clone();
                let np = proto.num_params as usize;
                let (base, nvarargs) = if proto.is_vararg {
                    // Fixed parameters move above the actual arguments; the
                    // extra arguments stay below the frame (adjust_varargs).
                    let top = func + 1 + nargs;
                    self.ensure_stack(top + np + proto.max_stack as usize + 1);
                    let mut actual = nargs;
                    while actual < np {
                        self.set_fresh(func + 1 + actual, Value::Nil);
                        actual += 1;
                    }
                    let fixed = func + 1;
                    let base = func + 1 + actual;
                    for i in 0..np {
                        let s = self.cur.stack[fixed + i];
                        self.set_copy(base + i, s);
                        self.set_fresh(fixed + i, Value::Nil);
                    }
                    (base, actual - np)
                } else {
                    (func + 1, 0)
                };
                let top = base + proto.max_stack as usize;
                self.ensure_stack(top + 1);
                // Missing parameters and the other registers start nil.
                let first_nil = if proto.is_vararg {
                    base + np
                } else {
                    base + nargs.min(np)
                };
                let nil = self.cur.taint.fresh(Value::Nil);
                for slot in &mut self.cur.stack[first_nil..top] {
                    *slot = nil;
                }
                self.cur.frames.push(Frame {
                    closure: f,
                    proto,
                    func,
                    base,
                    pc: 0,
                    nresults,
                    nvarargs,
                    tail: false,
                    entry: false,
                    secrets_dropped: false,
                    hook_pc: usize::MAX,
                });
                if self.call_hooks {
                    self.call_hook(f, false);
                }
                Ok(true)
            }
            Function::Native(nf) => {
                let (native, upvals, upval_taints) =
                    (nf.f, nf.upvals.clone(), nf.upval_taints.clone());
                let (args, taints, origins) =
                    split_slots(&self.cur.stack[func + 1..func + 1 + nargs]);
                // Nested calls from the native go above its arguments.
                self.cur.top = func + 1 + nargs;
                self.cstack.push(CEntry {
                    at_frames: self.cur.frames.len(),
                    slot: func,
                    func: f,
                    from_lua,
                });
                if self.call_hooks {
                    self.call_hook(f, true);
                }
                self.result_taints.clear();
                self.cur.natives += 1;
                let outer_used = std::mem::replace(&mut self.secret_used, false);
                let outer_reveal = std::mem::replace(&mut self.secret_reveal, false);
                let r = native(
                    self,
                    NativeCall {
                        args,
                        taints,
                        origins,
                        upvals,
                        upval_taints,
                    },
                );
                self.cur.natives -= 1;
                self.cstack.pop();
                if self.call_hooks {
                    self.return_hook(self.cur.frames.len() + self.cstack.len());
                }
                if self.cur.taint.tainted_by.is_some() {
                    self.settle_origin();
                }
                let used = std::mem::replace(&mut self.secret_used, outer_used);
                self.secret_reveal = outer_reveal;
                let results = match r {
                    // What a native computed from a secret is secret.
                    Ok(results) if used => {
                        results.into_iter().map(|v| self.secret_wrap(v)).collect()
                    }
                    Ok(results) => results,
                    Err(e) => {
                        self.result_taints.clear();
                        if e.yielded {
                            // The results of this call come with the resume.
                            self.yield_site = Some((func, nresults));
                        }
                        return Err(e);
                    }
                };
                // Values a native made take the write taint; it may give
                // others' own (`Vm::result_taints`).
                let taints = std::mem::take(&mut self.result_taints);
                let write = self.cur.taint.write();
                let origin = if write.is_secure() {
                    Origin::NONE
                } else {
                    self.cur.taint.origin
                };
                // Into the caller's registers (`luaD_poscall`), each with
                // its taint.
                let n = if nresults < 0 {
                    results.len()
                } else {
                    nresults as usize
                };
                self.ensure_stack(func + n + 1);
                for i in 0..n {
                    match results.get(i) {
                        Some(&v) => {
                            let t = taints.get(i).copied().unwrap_or(write);
                            self.set_copy(func + i, Slot { v, t, o: origin });
                        }
                        None => self.set_fresh(func + i, Value::Nil),
                    }
                }
                if nresults < 0 {
                    self.cur.top = func + n;
                }
                // The buffer back, empty, for the next call's.
                self.result_taints = taints;
                self.result_taints.clear();
                Ok(false)
            }
        }
    }

    /// A secret callee called by code that may use secrets (decision T195):
    /// the value it hides is called, in a nested run, and its results are
    /// secret again, as everything secure code computes from a secret.
    /// Blizzard's target frame needs it: `AuraUtil.SetAuraBorderColor`
    /// indexes `DEBUFF_DISPLAY_INFO` with an aura's secret dispel type and
    /// calls the secret color's `GetRGBA`. Unverified: addon code always
    /// runs tainted, so no probe sees a secure call of a secret.
    fn secret_call(
        &mut self,
        func: usize,
        callee: Value,
        nargs: usize,
        nresults: i32,
        from_lua: bool,
    ) -> Result<bool, LuaError> {
        self.cur.stack[func].v = self.hidden(callee);
        self.call_at(func, nargs, -1, from_lua)?;
        let n = self.cur.top - func;
        for i in func..func + n {
            let v = self.cur.stack[i].v;
            self.cur.stack[i].v = self.secret_wrap(v);
        }
        if nresults < 0 {
            return Ok(false);
        }
        let wanted = nresults as usize;
        self.ensure_stack(func + wanted + 1);
        for i in n..wanted {
            self.set_fresh(func + i, Value::Nil);
        }
        Ok(false)
    }

    /// Returns from the top frame with the `n` values at `from`; true if it
    /// was the entry frame of the current `execute`.
    fn poscall(&mut self, from: usize, n: usize) -> bool {
        if self.call_hooks {
            self.return_hook(self.cur.frames.len() + self.cstack.len() - 1);
        }
        let frame = self.cur.frames.pop().expect("frame");
        let dest = frame.func;
        let wanted = frame.nresults;
        let count = if wanted < 0 { n } else { wanted as usize };
        self.ensure_stack(dest + count + 1);
        for i in 0..count {
            if i < n {
                let s = self.cur.stack[from + i];
                self.set_copy(dest + i, s);
            } else {
                self.set_fresh(dest + i, Value::Nil);
            }
        }
        if wanted < 0 {
            self.cur.top = dest + n;
        }
        frame.entry
    }

    // ------------------------------------------------------------ upvalues

    fn find_upval(&mut self, idx: usize) -> UpvalRef {
        let open = &self.cur.open_upvals;
        match open.binary_search_by_key(&idx, |&(i, _)| i) {
            Ok(i) => open[i].1,
            Err(at) => {
                let u = self.heap.alloc_upval(Upval::Open {
                    thread: self.current,
                    idx,
                });
                self.cur.open_upvals.insert(at, (idx, u));
                u
            }
        }
    }

    pub(crate) fn close_upvals(&mut self, level: usize) {
        while let Some(&(idx, u)) = self.cur.open_upvals.last() {
            if idx < level {
                break;
            }
            let v = self.cur.stack[idx];
            *self.heap.upval_mut(u) = Upval::Closed(v);
            self.cur.open_upvals.pop();
        }
    }

    pub(crate) fn upval_get(&self, u: UpvalRef) -> Value {
        self.upval_slot(u).v
    }

    pub(crate) fn upval_slot(&self, u: UpvalRef) -> Slot {
        match *self.heap.upval(u) {
            Upval::Closed(v) => v,
            Upval::Open { thread, idx } => {
                if thread == self.current {
                    self.cur.stack[idx]
                } else {
                    self.heap.thread(thread).state.stack[idx]
                }
            }
        }
    }

    /// Stores into an upvalue, taint and all (`setobj2uv`).
    pub(crate) fn upval_set(&mut self, u: UpvalRef, v: Slot) {
        match *self.heap.upval(u) {
            Upval::Closed(_) => *self.heap.upval_mut(u) = Upval::Closed(v),
            Upval::Open { thread, idx } => {
                if thread == self.current {
                    self.cur.stack[idx] = v;
                } else {
                    self.heap.thread_mut(thread).state.stack[idx] = v;
                }
            }
        }
    }

    // ---------------------------------------------------------- the loop

    fn rk(&self, base: usize, k: &[Value], rk: Rk) -> Value {
        match rk {
            Rk::R(r) => self.cur.stack[base + r as usize].v,
            Rk::K(i) => k[i as usize],
        }
    }

    /// An RK operand with its taint (constants: the load taint).
    fn rk_slot(&self, base: usize, proto: &Proto, rk: Rk) -> Slot {
        match rk {
            Rk::R(r) => self.cur.stack[base + r as usize],
            Rk::K(i) => Slot {
                v: proto.constants[i as usize],
                t: proto.const_taint,
                o: Origin::NONE,
            },
        }
    }

    /// Runs frames until the entry frame returns.
    pub(crate) fn execute(&mut self) -> Result<(), LuaError> {
        self.run_depth += 1;
        let cov = self.cov_frame;
        let r = self.run();
        self.cov_frame = cov;
        // The caller's frame works its hook bits out again.
        if self.hooks_on {
            self.hook_mask |= crate::debug::HOOK_RECALC;
        }
        self.run_depth -= 1;
        r
    }

    /// Counts what the chunks `filter` accepts (by chunk name) run from
    /// now on ([`crate::coverage`]).
    pub fn enable_coverage(&mut self, filter: crate::coverage::ChunkFilter) {
        self.coverage = Some(crate::coverage::Coverage::new(filter));
        self.hooks_changed();
    }

    fn run(&mut self) -> Result<(), LuaError> {
        // The running frame's prototype, kept from one frame to the next
        // when it is the same function's (a recursion, a loop's calls).
        let mut proto = self.cur.frames[self.cur.frames.len() - 1].proto.clone();
        'frame: loop {
            let fi = self.cur.frames.len() - 1;
            let (closure, base) = {
                let f = &self.cur.frames[fi];
                if !Rc::ptr_eq(&proto, &f.proto) {
                    proto = f.proto.clone();
                }
                (f.closure, f.base)
            };
            // (Re)entering a Lua frame: its closure's taint taints the
            // execution and fixes it while the frame runs.
            let closure_taint = self.heap.func(closure).taint();
            self.cur.taint.fixed = Taint::SECURE;
            self.cur.taint.read(closure_taint);
            self.settle_origin();
            self.cur.taint.fixed = closure_taint;
            self.frame_hooks(&proto);
            let k = &proto.constants[..];
            let code = &proto.code[..];
            loop {
                let pc = self.cur.frames[fi].pc;
                let op = code[pc];
                self.cur.frames[fi].pc = pc + 1;
                if self.hook_mask != 0 {
                    self.instructions += 1;
                    if self.hook_mask & !crate::debug::HOOK_COUNT != 0 {
                        self.instr_hook(fi, pc, &proto)?;
                    }
                }
                match op {
                    Op::Move { a, b } => {
                        let s = self.cur.stack[base + b as usize];
                        self.set_copy(base + a as usize, s);
                    }
                    Op::LoadK { a, k: i } => {
                        let s = self.rk_slot(base, &proto, Rk::K(i));
                        self.set_copy(base + a as usize, s);
                    }
                    Op::LoadBool { a, value, skip } => {
                        self.set_fresh(base + a as usize, Value::Bool(value));
                        if skip {
                            self.cur.frames[fi].pc += 1;
                        }
                    }
                    Op::LoadNil { a, b } => {
                        for r in a..=b {
                            self.set_fresh(base + r as usize, Value::Nil);
                        }
                    }
                    Op::GetUpval { a, u } => {
                        let uv = self.closure_upval(closure, u);
                        let s = self.upval_slot(uv);
                        self.set_copy(base + a as usize, s);
                    }
                    Op::GetGlobal { a, k: i } => {
                        let env = Value::Table(self.heap.func(closure).env());
                        let s = self.index_slot(env, k[i as usize], None)?;
                        self.set_copy(base + a as usize, s);
                    }
                    Op::GetTable { a, b, c } => {
                        let t = self.reg(base + b as usize);
                        let key = self.rk(base, k, c);
                        let s = match t {
                            Value::Table(tr) if !matches!(key, Value::Secret(_)) => {
                                let s = self.heap.table(tr).get_slot(key);
                                if s.v.is_nil() && self.heap.table(tr).meta.is_some() {
                                    self.index_slot(t, key, Some(b))?
                                } else {
                                    s
                                }
                            }
                            _ => self.index_slot(t, key, Some(b))?,
                        };
                        self.set_copy(base + a as usize, s);
                    }
                    Op::SetGlobal { a, k: i } => {
                        let env = Value::Table(self.heap.func(closure).env());
                        let v = self.cur.stack[base + a as usize];
                        self.set_index_slot(env, k[i as usize], v, None)?;
                    }
                    Op::SetUpval { a, u } => {
                        let uv = self.closure_upval(closure, u);
                        let mut v = self.cur.stack[base + a as usize];
                        if !v.t.is_secure() {
                            v.o = self.record_write(v.t, v.o);
                        }
                        self.upval_set(uv, v);
                    }
                    Op::SetTable { a, b, c } => {
                        let t = self.reg(base + a as usize);
                        let key = self.rk(base, k, b);
                        let v = self.rk_slot(base, &proto, c);
                        self.set_index_slot(t, key, v, Some(a))?;
                    }
                    Op::NewTable { a, narr, nhash } => {
                        let t = self
                            .heap
                            .alloc_table(Table::new(narr as usize, nhash as usize));
                        self.set_fresh(base + a as usize, Value::Table(t));
                        self.gc_step()?;
                    }
                    Op::SelfOp { a, b, c } => {
                        let obj = self.cur.stack[base + b as usize];
                        let key = self.rk(base, k, c);
                        self.set_copy(base + a as usize + 1, obj);
                        let s = self.index_slot(obj.v, key, Some(b))?;
                        self.set_copy(base + a as usize, s);
                    }
                    Op::Arith { op, a, b, c } => {
                        let x = self.rk(base, k, b);
                        let y = self.rk(base, k, c);
                        if self.rules.fpe_strict
                            && matches!(op, ArithOp::Div | ArithOp::Mod)
                            && let (Value::Num(x), Value::Num(y)) = (x, y)
                        {
                            // Numbers only: with a metamethod, `t / 0` calls it.
                            self.check_fp(Some(x), Some(y))?;
                        }
                        match (x, y) {
                            (Value::Num(x), Value::Num(y)) => {
                                self.set_fresh(base + a as usize, Value::Num(arith(op, x, y)));
                            }
                            _ => {
                                let xs = self.rk_slot(base, &proto, b);
                                let ys = self.rk_slot(base, &proto, c);
                                let s = self.arith_slow(op, xs, ys, b, c)?;
                                self.set_copy(base + a as usize, s);
                            }
                        }
                    }
                    Op::Unm { a, b } => {
                        let x = self.reg(base + b as usize);
                        match x {
                            Value::Num(n) => self.set_fresh(base + a as usize, Value::Num(-n)),
                            _ => {
                                let s = self.unm_slow(x, b)?;
                                self.set_copy(base + a as usize, s);
                            }
                        }
                    }
                    Op::Not { a, b } => {
                        let x = self.reg(base + b as usize);
                        let t = match x {
                            Value::Secret(_) => self.secret_truthy(x, "negate", Some(b))?,
                            _ => x.truthy(),
                        };
                        self.set_fresh(base + a as usize, Value::Bool(!t));
                    }
                    Op::Len { a, b } => {
                        let x = self.reg(base + b as usize);
                        let s = self.len_slot(x, Some(b))?;
                        self.set_copy(base + a as usize, s);
                    }
                    Op::Concat { a, b, c } => {
                        let s = self.concat_range(base, b, c)?;
                        self.set_copy(base + a as usize, s);
                        self.gc_step()?;
                    }
                    Op::Jmp { sbx } => {
                        self.jump(fi, sbx);
                    }
                    Op::Eq { a, b, c } => {
                        let x = self.rk(base, k, b);
                        let y = self.rk(base, k, c);
                        let r = self.equals_at(x, y, (rk_reg(b), rk_reg(c)))?;
                        self.cond_jump(fi, &proto, r == a);
                    }
                    Op::Lt { a, b, c } => {
                        let x = self.rk(base, k, b);
                        let y = self.rk(base, k, c);
                        let r = self.less_than_at(x, y, (rk_reg(b), rk_reg(c)))?;
                        self.cond_jump(fi, &proto, r == a);
                    }
                    Op::Le { a, b, c } => {
                        let x = self.rk(base, k, b);
                        let y = self.rk(base, k, c);
                        let r = self.less_equal_at(x, y, (rk_reg(b), rk_reg(c)))?;
                        self.cond_jump(fi, &proto, r == a);
                    }
                    Op::Test { a, c } => {
                        let x = self.reg(base + a as usize);
                        let t = match x {
                            Value::Secret(_) => {
                                self.secret_truthy(x, "perform boolean test on", Some(a))?
                            }
                            _ => x.truthy(),
                        };
                        self.cond_jump(fi, &proto, t == c);
                    }
                    Op::TestSet { a, b, c } => {
                        let s = self.cur.stack[base + b as usize];
                        let t = match s.v {
                            Value::Secret(_) => {
                                self.secret_truthy(s.v, "perform boolean test on", Some(b))?
                            }
                            _ => s.v.truthy(),
                        };
                        if t == c {
                            self.set_copy(base + a as usize, s);
                            self.cond_jump(fi, &proto, true);
                        } else {
                            self.cond_jump(fi, &proto, false);
                        }
                    }
                    Op::Call { a, b, c } => {
                        let func = base + a as usize;
                        let nargs = if b == 0 {
                            self.cur.top - func - 1
                        } else {
                            b as usize - 1
                        };
                        let nresults = c as i32 - 1;
                        if self.precall(func, nargs, nresults, true)? {
                            continue 'frame;
                        }
                    }
                    Op::TailCall { a, b } => {
                        let func = base + a as usize;
                        let nargs = if b == 0 {
                            self.cur.top - func - 1
                        } else {
                            b as usize - 1
                        };
                        self.close_upvals(base);
                        let cur_func = self.cur.frames[fi].func;
                        let nresults = self.cur.frames[fi].nresults;
                        let entry = self.cur.frames[fi].entry;
                        if matches!(self.callee_kind(func), Some(true)) {
                            // Move the callee and arguments down over this
                            // frame and replace it.
                            for i in 0..=nargs {
                                let s = self.cur.stack[func + i];
                                self.set_copy(cur_func + i, s);
                            }
                            // Lua takes the callee's slot before it drops
                            // the caller's.
                            self.enter_ci()?;
                            if self.call_hooks {
                                self.return_hook(self.cur.frames.len() + self.cstack.len() - 1);
                            }
                            self.cur.frames.pop();
                            self.precall(cur_func, nargs, nresults, true)?;
                            let f = self.cur.frames.last_mut().expect("frame");
                            f.tail = true;
                            f.entry = entry;
                            continue 'frame;
                        }
                        // A native (or `__call`) callee: call it here; the
                        // following RETURN returns its results.
                        if self.precall(func, nargs, -1, true)? {
                            continue 'frame;
                        }
                    }
                    Op::Return { a, b } => {
                        let from = base + a as usize;
                        let n = if b == 0 {
                            self.cur.top - from
                        } else {
                            b as usize - 1
                        };
                        self.close_upvals(base);
                        if self.poscall(from, n) {
                            return Ok(());
                        }
                        continue 'frame;
                    }
                    Op::ForLoop { a, sbx } => {
                        let ra = base + a as usize;
                        let (Value::Num(idx), Value::Num(limit), Value::Num(step)) =
                            (self.reg(ra), self.reg(ra + 1), self.reg(ra + 2))
                        else {
                            unreachable!("for loop prepared")
                        };
                        let idx = idx + step;
                        if if step > 0.0 {
                            idx <= limit
                        } else {
                            limit <= idx
                        } {
                            self.jump(fi, sbx);
                            self.set_fresh(ra, Value::Num(idx));
                            self.set_fresh(ra + 3, Value::Num(idx));
                        }
                    }
                    Op::ForPrep { a, sbx } => {
                        let ra = base + a as usize;
                        let init = self.tonumber(self.reg(ra));
                        let limit = self.tonumber(self.reg(ra + 1));
                        let step = self.tonumber(self.reg(ra + 2));
                        let Some(init) = init else {
                            return Err(self.rt_error("'for' initial value must be a number"));
                        };
                        let Some(limit) = limit else {
                            return Err(self.rt_error("'for' limit must be a number"));
                        };
                        let Some(step) = step else {
                            return Err(self.rt_error("'for' step must be a number"));
                        };
                        // `tonumber` converts in place: a number stays as it
                        // is, a converted string becomes a value made now.
                        for (r, x) in [(ra + 1, limit), (ra + 2, step)] {
                            if !matches!(self.reg(r), Value::Num(_)) {
                                self.set_fresh(r, Value::Num(x));
                            }
                        }
                        self.set_fresh(ra, Value::Num(init - step));
                        self.jump(fi, sbx);
                    }
                    Op::TForLoop { a, c } => {
                        let ra = base + a as usize;
                        let cb = ra + 3;
                        for i in (0..3).rev() {
                            let s = self.cur.stack[ra + i];
                            self.set_copy(cb + i, s);
                        }
                        self.call_at(cb, 2, c as i32, true)?;
                        let s = self.cur.stack[cb];
                        if !s.v.is_nil() {
                            self.set_copy(cb - 1, s);
                        } else {
                            self.cur.frames[fi].pc += 1;
                        }
                    }
                    Op::SetList { a, b, c } => {
                        let ra = base + a as usize;
                        let n = if b == 0 {
                            self.cur.top - ra - 1
                        } else {
                            b as usize
                        };
                        let Value::Table(t) = self.reg(ra) else {
                            unreachable!("SETLIST on a table")
                        };
                        let first = (c as usize - 1) * LFIELDS_PER_FLUSH;
                        let last = first + n;
                        let table = self.heap.table_mut(t);
                        if last > table.array_len() {
                            table.resize_array(last);
                        }
                        for i in (1..=n).rev() {
                            let v = self.cur.stack[ra + i];
                            self.heap.table_mut(t).set_int(first + i, v);
                        }
                    }
                    Op::Close { a } => {
                        self.close_upvals(base + a as usize);
                    }
                    Op::Closure { a, proto: pi } => {
                        let p = proto.protos[pi as usize].clone();
                        let mut upvals = Vec::with_capacity(p.upvals.len());
                        for d in &p.upvals {
                            upvals.push(match *d {
                                crate::proto::UpvalDesc::Local(r) => {
                                    self.find_upval(base + r as usize)
                                }
                                crate::proto::UpvalDesc::Upval(u) => self.closure_upval(closure, u),
                            });
                        }
                        let env = self.heap.func(closure).env();
                        let taint = self.cur.taint.alloc(true);
                        let f = self.heap.alloc_func(Function::Lua(LuaFunction {
                            proto: p,
                            upvals: upvals.into_boxed_slice(),
                            env,
                            taint,
                        }));
                        self.set_fresh(base + a as usize, Value::Func(f));
                        self.gc_step()?;
                    }
                    Op::Vararg { a, b } => {
                        let n = self.cur.frames[fi].nvarargs;
                        let ra = base + a as usize;
                        let wanted = if b == 0 {
                            self.ensure_stack(ra + n + 1);
                            self.cur.top = ra + n;
                            n
                        } else {
                            b as usize - 1
                        };
                        for j in 0..wanted {
                            if j < n {
                                let s = self.cur.stack[base - n + j];
                                self.set_copy(ra + j, s);
                            } else {
                                self.set_fresh(ra + j, Value::Nil);
                            }
                        }
                    }
                }
            }
        }
    }

    fn jump(&mut self, fi: usize, sbx: i32) {
        let f = &mut self.cur.frames[fi];
        f.pc = (f.pc as i64 + i64::from(sbx)) as usize;
    }

    /// After a test: take the following jump if `taken`, else skip it.
    fn cond_jump(&mut self, fi: usize, proto: &Proto, taken: bool) {
        let pc = self.cur.frames[fi].pc;
        if let Some(ci) = self.cov_frame
            && let Some(c) = self.coverage.as_mut()
        {
            c.branch(ci, pc - 1, taken);
        }
        if taken {
            let Op::Jmp { sbx } = proto.code[pc] else {
                unreachable!("a test is followed by a jump")
            };
            self.cur.frames[fi].pc = (pc as i64 + 1 + i64::from(sbx)) as usize;
        } else {
            self.cur.frames[fi].pc = pc + 1;
        }
    }

    fn closure_upval(&self, closure: FuncRef, u: u8) -> UpvalRef {
        match self.heap.func(closure) {
            Function::Lua(f) => f.upvals[u as usize],
            Function::Native(_) => unreachable!("native closures have no upvalue cells"),
        }
    }

    /// Whether the value at `slot` is a Lua function (true), a native
    /// (false), or not a function (None).
    fn callee_kind(&self, slot: usize) -> Option<bool> {
        match self.cur.stack[slot].v {
            Value::Func(f) => Some(matches!(self.heap.func(f), Function::Lua(_))),
            _ => None,
        }
    }

    // ---------------------------------------------------------- metatables

    pub fn get_metatable(&self, v: Value) -> Option<TableRef> {
        match v {
            Value::Table(t) => self.heap.table(t).meta,
            Value::Userdata(u) => self.heap.userdata(u).meta,
            Value::Str(_) => self.string_meta,
            _ => None,
        }
    }

    pub fn get_tm(&self, v: Value, event: StrRef) -> Value {
        self.get_tm_slot(v, event).v
    }

    /// A metamethod with the taint of its metatable field.
    pub fn get_tm_slot(&self, v: Value, event: StrRef) -> Slot {
        match self.get_metatable(v) {
            Some(m) => self.heap.table(m).get_str_slot(event),
            None => Slot::NIL,
        }
    }

    /// Calls a metamethod with arguments; returns its first result.
    pub(crate) fn call_tm(&mut self, tm: Value, args: &[Value]) -> Result<Value, LuaError> {
        let tm = Slot::secure(tm);
        let args: Vec<Slot> = args.iter().map(|&v| Slot::secure(v)).collect();
        Ok(self.call_tm_slots(tm, &args)?.v)
    }

    /// `callTMres`: the metamethod and its arguments are copied to the stack
    /// (tainted ones taint the execution); returns the first result.
    pub(crate) fn call_tm_slots(&mut self, tm: Slot, args: &[Slot]) -> Result<Slot, LuaError> {
        let func = self.cur.top.max(self.frame_top());
        self.ensure_stack(func + args.len() + 2);
        self.set_copy(func, tm);
        for (i, &a) in args.iter().enumerate() {
            self.set_copy(func + 1 + i, a);
        }
        self.call_at(func, args.len(), 1, false)?;
        Ok(self.cur.stack[func])
    }

    /// `t[key]` with `__index` (`luaV_gettable`). `reg` names the operand in
    /// errors (the register of `t` in the running Lua frame).
    pub fn index(&mut self, t: Value, key: Value, reg: Option<u8>) -> Result<Value, LuaError> {
        Ok(self.index_slot(t, key, reg)?.v)
    }

    /// [`index`](Self::index) with the result's taint.
    pub fn index_slot(&mut self, t: Value, key: Value, reg: Option<u8>) -> Result<Slot, LuaError> {
        if matches!(t, Value::Secret(_)) || matches!(key, Value::Secret(_)) {
            return self.secret_index(t, key, reg);
        }
        let mut t = Slot::secure(t);
        let mut reg = reg;
        for _ in 0..MAXTAGLOOP {
            let tm = if let Value::Table(tr) = t.v {
                let table = self.heap.table(tr);
                let v = table.get_slot(key);
                if !v.v.is_nil() {
                    return Ok(v);
                }
                match table.meta {
                    None => return Ok(v),
                    Some(m) => {
                        let tm = self.heap.table(m).get_str_slot(self.names.index);
                        if tm.v.is_nil() {
                            return Ok(v);
                        }
                        tm
                    }
                }
            } else {
                let tm = self.get_tm_slot(t.v, self.names.index);
                if tm.v.is_nil() {
                    return Err(self.type_error(t.v, "index", reg));
                }
                tm
            };
            if matches!(tm.v, Value::Func(_)) {
                return self.call_tm_slots(tm, &[t, Slot::secure(key)]);
            }
            t = tm;
            reg = None;
        }
        Err(self.rt_error("loop in gettable"))
    }

    /// `t[key] = v` with `__newindex` (`luaV_settable`), `v` as a value
    /// made now.
    pub fn set_index(
        &mut self,
        t: Value,
        key: Value,
        v: Value,
        reg: Option<u8>,
    ) -> Result<(), LuaError> {
        let v = self.cur.taint.fresh(v);
        self.set_index_slot(t, key, v, reg)
    }

    /// [`set_index`](Self::set_index) storing `v`'s taint as it is.
    pub fn set_index_slot(
        &mut self,
        t: Value,
        key: Value,
        v: Slot,
        reg: Option<u8>,
    ) -> Result<(), LuaError> {
        if matches!(t, Value::Secret(_)) || matches!(key, Value::Secret(_)) {
            return self.secret_set_index(t, key, v, reg);
        }
        let mut t = Slot::secure(t);
        let mut reg = reg;
        for _ in 0..MAXTAGLOOP {
            let tm = if let Value::Table(tr) = t.v {
                // The key is checked first, even with `__newindex`.
                if let Err(e) = crate::table::check_key(key) {
                    return Err(self.rt_error(e.message()));
                }
                let table = self.heap.table(tr);
                let existing = table.get(key);
                let tm = match table.meta {
                    Some(m) if existing.is_nil() => {
                        self.heap.table(m).get_str_slot(self.names.newindex)
                    }
                    _ => Slot::NIL,
                };
                if tm.v.is_nil() {
                    let mut v = v;
                    if !v.t.is_secure() {
                        v.o = self.record_write(v.t, v.o);
                    }
                    return self
                        .heap
                        .table_mut(tr)
                        .set_slot(key, v)
                        .map_err(|e| self.rt_error(e.message()));
                }
                tm
            } else {
                let tm = self.get_tm_slot(t.v, self.names.newindex);
                if tm.v.is_nil() {
                    let op = if self.rules.client_errors {
                        "perform indexed assignment on"
                    } else {
                        "index"
                    };
                    return Err(self.type_error(t.v, op, reg));
                }
                tm
            };
            // The metamethod's taint taints the execution.
            self.cur.taint.read(tm.t);
            self.settle_origin();
            if matches!(tm.v, Value::Func(_)) {
                let func = self.cur.top.max(self.frame_top());
                self.ensure_stack(func + 4);
                self.set_copy(func, tm);
                self.set_copy(func + 1, t);
                self.set_copy(func + 2, Slot::secure(key));
                self.set_copy(func + 3, v);
                return self.call_at(func, 3, 0, false);
            }
            t = tm;
            reg = None;
        }
        Err(self.rt_error("loop in settable"))
    }

    // ---------------------------------------------------------- arithmetic

    pub fn tonumber(&self, v: Value) -> Option<f64> {
        match v {
            Value::Num(n) => Some(n),
            Value::Str(s) => str2num(self.heap.str_bytes(s)),
            _ => None,
        }
    }

    fn arith_tm(&self, op: ArithOp) -> StrRef {
        match op {
            ArithOp::Add => self.names.add,
            ArithOp::Sub => self.names.sub,
            ArithOp::Mul => self.names.mul,
            ArithOp::Div => self.names.div,
            ArithOp::Mod => self.names.mod_,
            ArithOp::Pow => self.names.pow,
        }
    }

    fn arith_slow(
        &mut self,
        op: ArithOp,
        xs: Slot,
        ys: Slot,
        rb: Rk,
        rc: Rk,
    ) -> Result<Slot, LuaError> {
        let (x, y) = (xs.v, ys.v);
        if let (Some(a), Some(b)) = (self.tonumber(x), self.tonumber(y)) {
            let v = self.arith_num(op, a, b)?;
            return Ok(self.cur.taint.fresh(v));
        }
        if self.secret_usable(x, y) {
            let (hx, hy) = (self.hidden(x), self.hidden(y));
            let r = self.arith_values(op, hx, hy)?;
            let r = self.secret_wrap(r);
            return Ok(self.cur.taint.fresh(r));
        }
        let event = self.arith_tm(op);
        let mut tm = self.get_tm_slot(x, event);
        if tm.v.is_nil() {
            tm = self.get_tm_slot(y, event);
        }
        if tm.v.is_nil() {
            // The first operand that is not a number is blamed.
            let (bad, reg) = if self.tonumber(x).is_none() {
                (x, rb)
            } else {
                (y, rc)
            };
            let reg = match reg {
                Rk::R(r) => Some(r),
                Rk::K(_) => None,
            };
            return Err(self.type_error(bad, "perform arithmetic on", reg));
        }
        self.call_tm_slots(tm, &[xs, ys])
    }

    /// Arithmetic on numbers, where WoW's rules apply (S2 hooks them in).
    /// Arithmetic on numbers coerced from strings (or from native code).
    pub(crate) fn arith_num(&mut self, op: ArithOp, a: f64, b: f64) -> Result<Value, LuaError> {
        if self.rules.fpe_coerce && matches!(op, ArithOp::Div | ArithOp::Mod) {
            self.check_fp(Some(a), Some(b))?;
        }
        Ok(Value::Num(arith(op, a, b)))
    }

    /// The client's checks of a division's operands (elune's `checkfp`).
    fn check_fp(&mut self, b: Option<f64>, c: Option<f64>) -> Result<(), LuaError> {
        let msg = if c == Some(0.0) {
            "Division by zero"
        } else if b.is_some_and(f64::is_nan) {
            "Numerator is not a number"
        } else if c.is_some_and(f64::is_nan) {
            "Denominator is not a number"
        } else if b.is_some_and(f64::is_infinite) && c.is_some_and(f64::is_infinite) {
            "Infinity divided by infinity"
        } else {
            return Ok(());
        };
        Err(self.rt_error(msg))
    }

    /// The client's 32-bit range check of integer arguments (`lua_tolong`,
    /// `lua_tointeger`).
    pub fn check_overflow(&mut self, x: f64) -> Result<(), LuaError> {
        if self.rules.int_overflow
            && (x.is_nan() || x < f64::from(i32::MIN) || x > f64::from(i32::MAX))
        {
            let msg = format!(
                "integer overflow attempting to store {}",
                crate::number::fmt_num(x)
            );
            return Err(self.rt_error(&msg));
        }
        Ok(())
    }

    /// Arithmetic from native code (no operand names).
    pub fn arith_values(&mut self, op: ArithOp, x: Value, y: Value) -> Result<Value, LuaError> {
        if let (Value::Num(a), Value::Num(b)) = (x, y) {
            return self.arith_num(op, a, b);
        }
        let (x, y) = (self.cur.taint.fresh(x), self.cur.taint.fresh(y));
        Ok(self.arith_slow(op, x, y, Rk::K(0), Rk::K(0))?.v)
    }

    fn unm_slow(&mut self, x: Value, reg: u8) -> Result<Slot, LuaError> {
        if let Some(n) = self.tonumber(x) {
            return Ok(self.cur.taint.fresh(Value::Num(-n)));
        }
        if self.secret_usable(x, Value::Nil) {
            let h = self.hidden(x);
            let r = self.arith_values(ArithOp::Sub, Value::Num(0.0), h)?;
            let r = self.secret_wrap(r);
            return Ok(self.cur.taint.fresh(r));
        }
        let tm = self.get_tm_slot(x, self.names.unm);
        if tm.v.is_nil() {
            return Err(self.type_error(x, "perform arithmetic on", Some(reg)));
        }
        let xs = self.cur.stack[self.cur.frames.last().map_or(0, |f| f.base) + reg as usize];
        self.call_tm_slots(tm, &[xs, xs])
    }

    /// `#v` (Lua 5.1: `__len` only for userdata).
    pub fn len(&mut self, v: Value, reg: Option<u8>) -> Result<Value, LuaError> {
        Ok(self.len_slot(v, reg)?.v)
    }

    fn len_slot(&mut self, v: Value, reg: Option<u8>) -> Result<Slot, LuaError> {
        if self.secret_usable(v, Value::Nil) {
            let h = self.hidden(v);
            let r = self.len(h, None)?;
            let r = self.secret_wrap(r);
            return Ok(self.cur.taint.fresh(r));
        }
        match v {
            Value::Table(t) => {
                let n = self.heap.table(t).border() as f64;
                Ok(self.cur.taint.fresh(Value::Num(n)))
            }
            Value::Str(s) => {
                let n = self.heap.str_bytes(s).len() as f64;
                Ok(self.cur.taint.fresh(Value::Num(n)))
            }
            _ => {
                let tm = self.get_tm_slot(v, self.names.len);
                if tm.v.is_nil() {
                    return Err(self.type_error(v, "get length of", reg));
                }
                let vs = self.cur.taint.fresh(v);
                self.call_tm_slots(tm, &[vs, Slot::NIL])
            }
        }
    }

    /// `R(b) .. … .. R(c)`, right to left, with `__concat` (`luaV_concat`).
    fn concat_range(&mut self, base: usize, b: u8, c: u8) -> Result<Slot, LuaError> {
        let mut top = base + c as usize + 1;
        let mut total = c as usize - b as usize + 1;
        while total > 1 {
            let xslot = self.cur.stack[top - 2];
            let yslot = self.cur.stack[top - 1];
            let (x, y) = (xslot.v, yslot.v);
            if matches!(x, Value::Secret(_)) || matches!(y, Value::Secret(_)) {
                // Secret strings and numbers join into a secret string, even
                // while tainted; anything else blames the secret.
                let r = self.secret_concat(x, y, top - 2 - base, top - 1 - base)?;
                self.set_fresh(top - 2, r);
                top -= 1;
                total -= 1;
                continue;
            }
            let xs = self.is_strlike(x);
            let ys = self.is_strlike(y);
            if !xs || !ys {
                let mut tm = self.get_tm_slot(x, self.names.concat);
                if tm.v.is_nil() {
                    tm = self.get_tm_slot(y, self.names.concat);
                }
                if tm.v.is_nil() {
                    let (bad, slot) = if xs { (y, top - 1) } else { (x, top - 2) };
                    let reg = (slot - base) as u8;
                    return Err(self.type_error(bad, "concatenate", Some(reg)));
                }
                let r = self.call_tm_slots(tm, &[xslot, yslot])?;
                self.set_copy(top - 2, r);
                top -= 1;
                total -= 1;
                continue;
            }
            // Collect as many string-like values as possible.
            let mut n = 1;
            while n < total && self.is_strlike(self.reg(top - n - 1)) {
                n += 1;
            }
            let mut buf = Vec::new();
            for i in (top - n)..top {
                let v = self.reg(i);
                self.append_strlike(&mut buf, v);
            }
            let s = self.heap.intern(&buf);
            self.set_fresh(top - n, Value::Str(s));
            top -= n - 1;
            total -= n - 1;
        }
        Ok(self.cur.stack[top - 1])
    }

    fn is_strlike(&self, v: Value) -> bool {
        matches!(v, Value::Str(_) | Value::Num(_))
    }

    pub(crate) fn append_strlike(&self, buf: &mut Vec<u8>, v: Value) {
        match v {
            Value::Str(s) => buf.extend_from_slice(self.heap.str_bytes(s)),
            Value::Num(n) => buf.extend_from_slice(fmt_num(n).as_bytes()),
            _ => unreachable!("string-like"),
        }
    }

    /// Concatenates two values from native code.
    pub fn concat_values(&mut self, x: Value, y: Value) -> Result<Value, LuaError> {
        if matches!(x, Value::Secret(_)) || matches!(y, Value::Secret(_)) {
            return self.secret_concat_values(x, y, (None, None));
        }
        if self.is_strlike(x) && self.is_strlike(y) {
            let mut buf = Vec::new();
            self.append_strlike(&mut buf, x);
            self.append_strlike(&mut buf, y);
            return Ok(Value::Str(self.heap.intern(&buf)));
        }
        let mut tm = self.get_tm(x, self.names.concat);
        if tm.is_nil() {
            tm = self.get_tm(y, self.names.concat);
        }
        if tm.is_nil() {
            let bad = if self.is_strlike(x) { y } else { x };
            return Err(self.type_error(bad, "concatenate", None));
        }
        self.call_tm(tm, &[x, y])
    }

    // --------------------------------------------------------- comparison

    /// `__eq` applies to two tables or two userdata with the same handler.
    pub fn equals(&mut self, x: Value, y: Value) -> Result<bool, LuaError> {
        self.equals_at(x, y, (None, None))
    }

    /// [`equals`](Self::equals) naming the operands' registers in errors.
    pub(crate) fn equals_at(
        &mut self,
        x: Value,
        y: Value,
        regs: (Option<u8>, Option<u8>),
    ) -> Result<bool, LuaError> {
        if matches!(x, Value::Secret(_)) || matches!(y, Value::Secret(_)) {
            return self.secret_equals(x, y, regs);
        }
        if x.raw_eq(y) {
            return Ok(true);
        }
        let same_kind = matches!(
            (x, y),
            (Value::Table(_), Value::Table(_)) | (Value::Userdata(_), Value::Userdata(_))
        );
        if !same_kind {
            return Ok(false);
        }
        let tm = self.comp_tm(x, y, self.names.eq);
        if tm.is_nil() {
            return Ok(false);
        }
        let r = self.call_tm(tm, &[x, y])?;
        Ok(r.truthy())
    }

    /// A comparison handler both operands share (`get_compTM`).
    fn comp_tm(&self, x: Value, y: Value, event: StrRef) -> Value {
        let (Some(m1), m2) = (self.get_metatable(x), self.get_metatable(y)) else {
            return Value::Nil;
        };
        let tm1 = self.heap.table(m1).get_str(event);
        if tm1.is_nil() {
            return Value::Nil;
        }
        if m2 == Some(m1) {
            return tm1;
        }
        let Some(m2) = m2 else { return Value::Nil };
        let tm2 = self.heap.table(m2).get_str(event);
        if tm1.raw_eq(tm2) { tm1 } else { Value::Nil }
    }

    fn order_tm(&mut self, x: Value, y: Value, event: StrRef) -> Result<Option<bool>, LuaError> {
        let tm1 = self.get_tm(x, event);
        if tm1.is_nil() {
            return Ok(None);
        }
        let tm2 = self.get_tm(y, event);
        if !tm1.raw_eq(tm2) {
            return Ok(None);
        }
        let r = self.call_tm(tm1, &[x, y])?;
        Ok(Some(r.truthy()))
    }

    fn order_error(&mut self, x: Value, y: Value) -> LuaError {
        let (t1, t2) = (x.type_name(), y.type_name());
        if t1 == t2 {
            self.rt_error(&format!("attempt to compare two {t1} values"))
        } else {
            self.rt_error(&format!("attempt to compare {t1} with {t2}"))
        }
    }

    pub fn less_than(&mut self, x: Value, y: Value) -> Result<bool, LuaError> {
        self.less_than_at(x, y, (None, None))
    }

    pub(crate) fn less_than_at(
        &mut self,
        x: Value,
        y: Value,
        regs: (Option<u8>, Option<u8>),
    ) -> Result<bool, LuaError> {
        if matches!(x, Value::Secret(_)) || matches!(y, Value::Secret(_)) {
            if self.can_access_secrets() {
                let (hx, hy) = (self.hidden(x), self.hidden(y));
                return self.less_than(hx, hy);
            }
            return Err(self.blame_secret(x, y, "compare", regs));
        }
        match (x, y) {
            (Value::Num(a), Value::Num(b)) => Ok(a < b),
            (Value::Str(a), Value::Str(b)) => Ok(self.heap.str_bytes(a) < self.heap.str_bytes(b)),
            _ if x.type_name() == y.type_name() => match self.order_tm(x, y, self.names.lt)? {
                Some(r) => Ok(r),
                None => Err(self.order_error(x, y)),
            },
            _ => Err(self.order_error(x, y)),
        }
    }

    pub fn less_equal(&mut self, x: Value, y: Value) -> Result<bool, LuaError> {
        self.less_equal_at(x, y, (None, None))
    }

    pub(crate) fn less_equal_at(
        &mut self,
        x: Value,
        y: Value,
        regs: (Option<u8>, Option<u8>),
    ) -> Result<bool, LuaError> {
        if matches!(x, Value::Secret(_)) || matches!(y, Value::Secret(_)) {
            if self.can_access_secrets() {
                let (hx, hy) = (self.hidden(x), self.hidden(y));
                return self.less_equal(hx, hy);
            }
            return Err(self.blame_secret(x, y, "compare", regs));
        }
        match (x, y) {
            (Value::Num(a), Value::Num(b)) => Ok(a <= b),
            (Value::Str(a), Value::Str(b)) => Ok(self.heap.str_bytes(a) <= self.heap.str_bytes(b)),
            _ if x.type_name() == y.type_name() => {
                if let Some(r) = self.order_tm(x, y, self.names.le)? {
                    return Ok(r);
                }
                // Lua 5.1: `a <= b` as `not (b < a)`.
                match self.order_tm(y, x, self.names.lt)? {
                    Some(r) => Ok(!r),
                    None => Err(self.order_error(x, y)),
                }
            }
            _ => Err(self.order_error(x, y)),
        }
    }

    // ------------------------------------------------------------ strings

    /// `tostring(v)` without `__tostring`.
    pub fn raw_tostring(&mut self, v: Value) -> Vec<u8> {
        match v {
            Value::Nil => b"nil".to_vec(),
            Value::Bool(b) => {
                if b {
                    b"true".to_vec()
                } else {
                    b"false".to_vec()
                }
            }
            Value::Num(n) => fmt_num(n).into_bytes(),
            Value::Str(s) => self.heap.str_bytes(s).to_vec(),
            other => {
                let addr = Heap::address(other).unwrap_or(0);
                format!("{}: 0x{addr:08x}", other.type_name()).into_bytes()
            }
        }
    }

    /// `tostring(v)` with `__tostring`.
    pub fn tostring(&mut self, v: Value) -> Result<Value, LuaError> {
        let tm = self.get_tm(v, self.names.tostring);
        if !tm.is_nil() {
            return self.call_tm(tm, &[v]);
        }
        if let Value::Str(_) = v {
            return Ok(v);
        }
        let b = self.raw_tostring(v);
        Ok(self.bytes(&b))
    }

    // ------------------------------------------------------------- errors

    /// Raises `v` as an error: runs the active `xpcall` handler here, where
    /// the stack still shows the error.
    pub fn raise(&mut self, v: Value) -> LuaError {
        let Some(handler) = self.errfunc else {
            return LuaError::new(v);
        };
        if self.in_errfunc {
            let msg = self.str("error in error handling");
            return LuaError::new(msg);
        }
        self.in_errfunc = true;
        let r = self.call(handler, &[v]);
        self.in_errfunc = false;
        match r {
            Ok(results) => LuaError::new(results.first().copied().unwrap_or_default()),
            Err(_) => {
                let msg = self.str("error in error handling");
                LuaError::new(msg)
            }
        }
    }

    /// A runtime error with the position of the running Lua function.
    pub fn rt_error(&mut self, msg: &str) -> LuaError {
        let mut text = self.where_(0);
        text.extend_from_slice(msg.as_bytes());
        let v = self.bytes(&text);
        self.raise(v)
    }

    /// Asks `guard` before every call from now on (none: no more): a reason
    /// it gives refuses the call with that error.
    pub fn set_call_guard(&mut self, guard: Option<Box<CallGuard>>) {
        self.call_guard = guard;
    }

    /// An error from native code with the position of its caller
    /// (`luaL_error`).
    pub fn error(&mut self, msg: &str) -> LuaError {
        let mut text = self.where_(1);
        text.extend_from_slice(msg.as_bytes());
        let v = self.bytes(&text);
        self.raise(v)
    }

    fn type_error(&mut self, v: Value, op: &str, reg: Option<u8>) -> LuaError {
        if let Value::Secret(_) = v {
            return self.secret_error(v, op, reg);
        }
        let t = v.type_name();
        let named = reg.and_then(|r| self.reg_name(r));
        match named {
            Some((kind, name)) => {
                self.rt_error(&format!("attempt to {op} {kind} '{name}' (a {t} value)"))
            }
            None => self.rt_error(&format!("attempt to {op} a {t} value")),
        }
    }

    fn call_error(&mut self, func: usize) -> LuaError {
        let v = self.cur.stack[func].v;
        // The register of the callee in the running frame, if it is one.
        let reg = self.cur.frames.last().and_then(|f| {
            if func >= f.base && func < f.base + f.proto.max_stack as usize {
                Some((func - f.base) as u8)
            } else {
                None
            }
        });
        let reg = if self.running_native() { None } else { reg };
        if let Value::Secret(_) = v {
            return self.secret_error(v, "call", reg);
        }
        // The client reports the missing `__call` metamethod: always "a nil
        // value", never a name.
        if self.rules.client_errors {
            return self.rt_error("attempt to call a nil value");
        }
        self.type_error(v, "call", reg)
    }

    /// Whether a native function is running above the top Lua frame.
    pub(crate) fn running_native(&self) -> bool {
        self.cstack
            .last()
            .is_some_and(|c| c.at_frames == self.cur.frames.len())
    }

    /// The running functions, innermost first.
    pub(crate) fn stack_entries(&self) -> Vec<StackEntry> {
        let mut out = Vec::new();
        let mut ci = self.cstack.len();
        let mut fi = self.cur.frames.len();
        loop {
            // Natives above Lua frame `fi - 1` come first (innermost).
            if ci > 0 && self.cstack[ci - 1].at_frames == fi {
                ci -= 1;
                out.push(StackEntry::Native(ci));
                continue;
            }
            if fi == 0 {
                break;
            }
            fi -= 1;
            out.push(StackEntry::Lua(fi));
            if self.cur.frames[fi].entry {
                // Natives that called into this nested run sit below it.
                continue;
            }
        }
        out
    }

    /// The call stack as `lua_getstack` and `lua_getinfo` see it, innermost
    /// first (level 0 is the running function); a tail call adds a level.
    pub fn levels(&self) -> Vec<Level> {
        let entries = self.stack_entries();
        let mut out = Vec::new();
        for (i, e) in entries.iter().enumerate() {
            let tail = matches!(e, StackEntry::Lua(fi) if self.cur.frames[*fi].tail);
            // The name comes from the caller's call instruction, when the
            // caller is Lua code (`getfuncname`).
            let name = match entries.get(i + 1) {
                Some(StackEntry::Lua(ci)) if !tail => {
                    let f = &self.cur.frames[*ci];
                    let pc = f.pc.saturating_sub(1);
                    match f.proto.code.get(pc) {
                        Some(
                            Op::Call { a, .. } | Op::TailCall { a, .. } | Op::TForLoop { a, .. },
                        ) => obj_name(&self.heap, &f.proto, pc, *a),
                        _ => None,
                    }
                }
                _ => None,
            };
            match e {
                StackEntry::Lua(fi) => {
                    let f = &self.cur.frames[*fi];
                    out.push(Level {
                        short_src: chunkid(&f.proto.source),
                        source: f.proto.source.to_vec(),
                        currentline: f
                            .proto
                            .lines
                            .get(f.pc.saturating_sub(1))
                            .map_or(-1, |&l| i64::from(l)),
                        linedefined: i64::from(f.proto.line_defined),
                        what: if f.proto.line_defined == 0 {
                            "main"
                        } else {
                            "Lua"
                        },
                        name,
                        entry: Some(LevelEntry::Lua(*fi)),
                    });
                    if tail {
                        out.push(Level {
                            short_src: b"(tail call)".to_vec(),
                            source: b"=(tail call)".to_vec(),
                            currentline: -1,
                            linedefined: -1,
                            what: "tail",
                            name: None,
                            entry: None,
                        });
                    }
                }
                StackEntry::Native(ci) => out.push(Level {
                    short_src: b"[C]".to_vec(),
                    source: b"=[C]".to_vec(),
                    currentline: -1,
                    linedefined: -1,
                    what: "C",
                    name,
                    entry: Some(LevelEntry::Native(*ci)),
                }),
            }
        }
        out
    }

    /// A function's environment.
    pub fn func_env(&self, f: FuncRef) -> TableRef {
        match self.heap.func(f) {
            Function::Lua(l) => l.env,
            Function::Native(n) => n.env,
        }
    }

    pub fn set_func_env(&mut self, f: FuncRef, env: TableRef) {
        match self.heap.func_mut(f) {
            Function::Lua(l) => l.env = env,
            Function::Native(n) => n.env = env,
        }
    }

    /// The levels of another thread: its Lua frames (a suspended
    /// coroutine keeps no native calls).
    pub fn thread_levels(&mut self, t: ThreadRef) -> Vec<Level> {
        let state = std::mem::take(&mut self.heap.thread_mut(t).state);
        let own = std::mem::replace(&mut self.cur, state);
        let cstack = std::mem::take(&mut self.cstack);
        let levels = self.levels();
        self.cstack = cstack;
        let state = std::mem::replace(&mut self.cur, own);
        self.heap.thread_mut(t).state = state;
        // Its levels name no entries in the running thread.
        levels
            .into_iter()
            .map(|l| Level { entry: None, ..l })
            .collect()
    }

    /// The local variables of a level, as `lua_getlocal` names them
    /// (`(*temporary)` for the other slots of its frame), then its function's
    /// upvalues (`lua_getupvalue`).
    pub fn level_variables(&self, level: &Level) -> (Variables, Variables) {
        let mut locals = Vec::new();
        let mut upvalues = Vec::new();
        let Some(entry) = level.entry else {
            return (locals, upvalues);
        };
        // Where the next (inner) call's function sits: the frame ends there.
        let limit_after = |fi_or_slot: usize, is_native: bool| -> usize {
            let next_native = self
                .cstack
                .iter()
                .filter(|c| {
                    if is_native {
                        c.slot > fi_or_slot
                    } else {
                        c.at_frames > fi_or_slot
                    }
                })
                .map(|c| c.slot)
                .min();
            let next_lua = if is_native {
                self.cur
                    .frames
                    .iter()
                    .map(|f| f.func)
                    .filter(|&f| f > fi_or_slot)
                    .min()
            } else {
                self.cur.frames.get(fi_or_slot + 1).map(|f| f.func)
            };
            match (next_native, next_lua) {
                (Some(a), Some(b)) => a.min(b),
                (Some(a), None) | (None, Some(a)) => a,
                (None, None) => self.cur.top,
            }
        };
        match entry {
            LevelEntry::Lua(fi) => {
                let f = &self.cur.frames[fi];
                let pc = f.pc.saturating_sub(1);
                let limit = limit_after(fi, false);
                let active: Vec<&str> = f
                    .proto
                    .locvars
                    .iter()
                    .take_while(|v| v.start_pc <= pc)
                    .filter(|v| pc < v.end_pc)
                    .map(|v| &*v.name)
                    .collect();
                let mut n = 0;
                while f.base + n < limit {
                    let name = active.get(n).map_or("(*temporary)", |s| s);
                    locals.push((name.to_string(), self.cur.stack[f.base + n].v));
                    n += 1;
                }
                if let Function::Lua(l) = self.heap.func(f.closure) {
                    for (i, &u) in l.upvals.iter().enumerate() {
                        let name = f.proto.upval_names.get(i).map_or("", |n| n);
                        upvalues.push((name.to_string(), self.upval_get(u)));
                    }
                }
            }
            LevelEntry::Native(ci) => {
                let c = &self.cstack[ci];
                let base = c.slot + 1;
                let limit = limit_after(c.slot, true);
                for slot in base..limit {
                    locals.push(("(*temporary)".to_string(), self.cur.stack[slot].v));
                }
                if let Function::Native(n) = self.heap.func(c.func) {
                    upvalues.extend(n.upvals.iter().map(|&v| (String::new(), v)));
                }
            }
        }
        (locals, upvalues)
    }

    /// `chunk:line: ` of the function at `level` (0: the running one), or
    /// nothing for a native function (`luaL_where`).
    pub fn where_(&self, level: usize) -> Vec<u8> {
        match self.stack_entries().get(level) {
            Some(StackEntry::Lua(fi)) => {
                let f = &self.cur.frames[*fi];
                let line = f
                    .proto
                    .lines
                    .get(f.pc.saturating_sub(1))
                    .copied()
                    .unwrap_or(0);
                let mut w = chunkid(&f.proto.source);
                w.extend_from_slice(format!(":{line}: ").as_bytes());
                w
            }
            _ => Vec::new(),
        }
    }

    /// The kind and name of register `reg` in the running Lua frame
    /// (`getobjname`).
    pub(crate) fn reg_name(&self, reg: u8) -> Option<(&'static str, String)> {
        let f = self.cur.frames.last()?;
        obj_name(&self.heap, &f.proto, f.pc.saturating_sub(1), reg)
    }

    /// The name the running native function was called by, and whether it
    /// was called as a method (`getfuncname`).
    pub(crate) fn native_call_name(&self) -> Option<(&'static str, String)> {
        let c = self.cstack.last()?;
        if !c.from_lua || c.at_frames == 0 {
            return None;
        }
        let f = &self.cur.frames[c.at_frames - 1];
        let pc = f.pc.saturating_sub(1);
        match f.proto.code[pc] {
            Op::Call { a, .. } | Op::TailCall { a, .. } | Op::TForLoop { a, .. } => {
                obj_name(&self.heap, &f.proto, pc, a)
            }
            _ => None,
        }
    }

    /// The name the running native function was called by, as Lua's debug
    /// information sees the call site; none through `pcall` or from native
    /// code.
    pub fn called_as(&self) -> Option<String> {
        self.native_call_name().map(|(_, name)| name)
    }

    /// `bad argument #n to 'name' (msg)` (`luaL_argerror`).
    pub fn arg_error(&mut self, narg: usize, msg: &str) -> LuaError {
        let mut narg = narg;
        let named = self.native_call_name();
        if let Some(("method", name)) = &named {
            narg -= 1;
            if narg == 0 {
                return self.error(&format!("calling '{name}' on bad self ({msg})"));
            }
        }
        let name = named.map(|(_, n)| n).unwrap_or_else(|| "?".to_string());
        self.error(&format!("bad argument #{narg} to '{name}' ({msg})"))
    }

    /// The client's documented API's own argument error: "bad argument #N
    /// to '?' (…)", with neither the caller's position nor the function's
    /// name, even for a direct call (`/stdtest argcheck`).
    pub fn api_arg_error(&mut self, narg: usize, msg: &str) -> LuaError {
        let v = self.str(&format!("bad argument #{narg} to '?' ({msg})"));
        self.raise(v)
    }

    pub fn type_arg_error(&mut self, narg: usize, expected: &str, got: Option<Value>) -> LuaError {
        let got = match got {
            None => "no value",
            Some(v) => v.type_name(),
        };
        self.arg_error(narg, &format!("{expected} expected, got {got}"))
    }

    /// `msg` with the stack (`debug.traceback` / `debugstack`).
    pub fn traceback(&self, level: usize) -> String {
        let mut out = String::from("stack traceback:");
        for e in self.stack_entries().iter().skip(level) {
            match e {
                StackEntry::Lua(fi) => {
                    let f = &self.cur.frames[*fi];
                    let line = f
                        .proto
                        .lines
                        .get(f.pc.saturating_sub(1))
                        .copied()
                        .unwrap_or(0);
                    let source = String::from_utf8_lossy(&chunkid(&f.proto.source)).into_owned();
                    let what = if f.proto.line_defined == 0 {
                        "in main chunk".to_string()
                    } else {
                        format!("in function <{source}:{}>", f.proto.line_defined)
                    };
                    out.push_str(&format!("\n\t{source}:{line}: {what}"));
                    if f.tail {
                        out.push_str("\n\t(tail call): ?");
                    }
                }
                StackEntry::Native(ci) => {
                    let name = match self.heap.func(self.cstack[*ci].func) {
                        Function::Native(n) => n.name,
                        Function::Lua(_) => "?",
                    };
                    out.push_str(&format!("\n\t[C]: in function '{name}'"));
                }
            }
        }
        out
    }

    /// The addon a C error's "Lua Taint:" line names: none while execution
    /// is secure, else the taint of the innermost Lua function (the code
    /// that made the failing call) when it is tainted, else the execution's.
    /// The user's client (build 70450, ElvUI): ElvUI's Minimap.lua calls
    /// `FontString:SetText` with a string ElvUI_Libraries' `utf8sub` made,
    /// after a C call (`SetTextColor`), under ElvUI_Libraries'
    /// CallbackHandler; the execution's taint is then ElvUI_Libraries (a
    /// read after a C call taints it, `luaD_precall` having cleared the
    /// fixed taint), and the client names ElvUI. Unverified: one sample;
    /// the client may instead keep the fixed taint across C calls.
    pub fn error_taint(&self) -> Taint {
        let stack = self.cur.taint.stack;
        if stack.is_secure() {
            return stack;
        }
        match self
            .cur
            .frames
            .last()
            .map(|f| self.heap.func(f.closure).taint())
        {
            Some(t) if !t.is_secure() => t,
            _ => stack,
        }
    }
}

/// Number arithmetic as Lua 5.1 defines it (`luai_num*`).
/// Sets a freshly compiled proto's (and its children's) constant taint.
fn set_const_taint(proto: &mut Rc<Proto>, taint: Taint) {
    if let Some(p) = Rc::get_mut(proto) {
        p.const_taint = taint;
        for child in &mut p.protos {
            set_const_taint(child, taint);
        }
    }
}

pub fn arith(op: ArithOp, a: f64, b: f64) -> f64 {
    match op {
        ArithOp::Add => a + b,
        ArithOp::Sub => a - b,
        ArithOp::Mul => a * b,
        ArithOp::Div => a / b,
        ArithOp::Mod => a - (a / b).floor() * b,
        ArithOp::Pow => a.powf(b),
    }
}

/// The line of a byte offset (1-based).
fn line_of(src: &[u8], offset: usize) -> usize {
    let starts = compiler::line_starts(src);
    starts.partition_point(|&s| s <= offset.min(src.len()))
}

/// Lua 5.1's `luaO_chunkid` for runtime positions (`LUA_IDSIZE` 60), on
/// bytes.
pub fn chunkid(source: &[u8]) -> Vec<u8> {
    chunkid_sized(source, 60)
}

/// `luaO_chunkid` with a buffer of `IDSIZE` bytes: syntax errors use the
/// lexer's `MAXSRC` (80).
#[allow(non_snake_case)]
pub fn chunkid_sized(source: &[u8], IDSIZE: usize) -> Vec<u8> {
    if let Some(rest) = source.strip_prefix(b"=") {
        return rest[..rest.len().min(IDSIZE - 1)].to_vec();
    }
    if let Some(rest) = source.strip_prefix(b"@") {
        let max = IDSIZE - " '...' ".len() - 1;
        if rest.len() > max {
            return [b"...", &rest[rest.len() - max..]].concat();
        }
        return rest.to_vec();
    }
    let first_line_len = source
        .iter()
        .position(|&b| b == b'\n' || b == b'\r')
        .unwrap_or(source.len());
    let max = IDSIZE - " [string \"...\"] ".len() - 1;
    let len = first_line_len.min(max);
    if len < source.len() {
        [b"[string \"", &source[..len], b"...\"]"].concat()
    } else {
        [b"[string \"", source, b"\"]"].concat()
    }
}

/// A parser message whose `near '…'` token text was decoded lossily: the
/// token's own bytes, as Lua quotes them.
fn raw_near(message: &str, span: Option<wowlua_span::Span>, src: &[u8]) -> Vec<u8> {
    let raw = span.and_then(|s| src.get(s.start as usize..s.end as usize));
    if let (Some((head, quoted)), Some(raw)) = (message.rsplit_once(" near '"), raw) {
        let text = quoted.strip_suffix('\'').unwrap_or(quoted);
        if !raw.is_empty() && String::from_utf8_lossy(raw).starts_with(text) {
            return [head.as_bytes(), b" near '", raw, b"'"].concat();
        }
    }
    message.as_bytes().to_vec()
}

/// The name of register `reg` at instruction `pc` (`getobjname`).
pub(crate) fn obj_name(
    heap: &Heap,
    p: &Proto,
    pc: usize,
    reg: u8,
) -> Option<(&'static str, String)> {
    if let Some(name) = p.local_name(reg, pc) {
        return Some(("local", name.to_string()));
    }
    // A constant string key's text, else "?" (`kname`).
    let kname = |c: Rk| match c {
        Rk::K(k) => match p.constants[k as usize] {
            Value::Str(s) => String::from_utf8_lossy(heap.str_bytes(s)).into_owned(),
            _ => "?".to_string(),
        },
        Rk::R(_) => "?".to_string(),
    };
    match symbexec(p, pc, reg)? {
        Op::GetGlobal { k, .. } => Some(("global", kname(Rk::K(k)))),
        Op::Move { a, b } if b < a => obj_name(heap, p, pc, b),
        Op::GetTable { c, .. } => Some(("field", kname(c))),
        Op::GetUpval { u, .. } => Some((
            "upvalue",
            p.upval_names
                .get(u as usize)
                .map_or_else(|| "?".to_string(), |n| n.to_string()),
        )),
        Op::SelfOp { c, .. } => Some(("method", kname(c))),
        _ => None,
    }
}

/// The last instruction before `lastpc` that set `reg` (`symbexec`).
fn symbexec(p: &Proto, lastpc: usize, reg: u8) -> Option<Op> {
    let mut last: Option<usize> = None;
    let mut pc = 0;
    while pc < lastpc {
        let op = p.code[pc];
        match op {
            Op::LoadNil { a, b } => {
                if a <= reg && reg <= b {
                    last = Some(pc);
                }
            }
            Op::SelfOp { a, .. } => {
                if reg == a || reg == a + 1 {
                    last = Some(pc);
                }
            }
            Op::TForLoop { a, .. } => {
                if reg >= a + 2 {
                    last = Some(pc);
                }
            }
            Op::Call { a, .. } | Op::TailCall { a, .. } => {
                if reg >= a {
                    last = Some(pc);
                }
            }
            Op::Jmp { sbx } | Op::ForLoop { sbx, .. } | Op::ForPrep { sbx, .. } => {
                if let Op::ForLoop { a, .. } | Op::ForPrep { a, .. } = op
                    && a == reg
                {
                    last = Some(pc);
                }
                let dest = pc as i64 + 1 + i64::from(sbx);
                if (pc as i64) < dest && dest <= lastpc as i64 {
                    pc = dest as usize;
                    continue;
                }
            }
            Op::Move { a, .. }
            | Op::LoadK { a, .. }
            | Op::LoadBool { a, .. }
            | Op::GetUpval { a, .. }
            | Op::GetGlobal { a, .. }
            | Op::GetTable { a, .. }
            | Op::NewTable { a, .. }
            | Op::Arith { a, .. }
            | Op::Unm { a, .. }
            | Op::Not { a, .. }
            | Op::Len { a, .. }
            | Op::Concat { a, .. }
            // TEST counts too: its A mode is set in Lua 5.1's opmodes.
            | Op::Test { a, .. }
            | Op::TestSet { a, .. }
            | Op::Closure { a, .. }
            | Op::Vararg { a, .. }
                if a == reg =>
            {
                last = Some(pc);
            }
            _ => {}
        }
        pc += 1;
    }
    last.map(|pc| p.code[pc])
}

/// The register an RK operand names, if it is one.
fn rk_reg(rk: Rk) -> Option<u8> {
    match rk {
        Rk::R(r) => Some(r),
        Rk::K(_) => None,
    }
}
