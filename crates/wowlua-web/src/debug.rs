//! The debugger in the page (spec M14 S4, decision T397): the M12
//! controller on the running addon's VMs, its requests and events those
//! of `wowlua_debug::protocol` (`{ id, command, … }` in, `{ id, response }`,
//! `{ id, error }` and `{ event, … }` out). Stopped, the controller waits
//! for the next request in its [`Link`]: the page's link blocks the
//! worker on a shared mailbox (`Atomics.wait`), so the page itself never
//! blocks.

use serde_json::{Value as Json, json};
use std::cell::RefCell;
use std::collections::VecDeque;
use std::rc::Rc;
use wowlua_debug::protocol::{FunctionBreakpoint, LineBreakpoint};
use wowlua_debug::{Controller, Event, Link, Request, Response, Shared, SharedRef};
use wowlua_vm::Vm;

thread_local! {
    /// The debugging session of the running addon, if it is debugged.
    static SESSION: RefCell<Option<SharedRef>> = const { RefCell::new(None) };
}

/// The test runner's `instrument`: attaches the session's controller to
/// each VM the run makes (a `/reload` makes another).
pub fn instrument(vm: &mut Vm) {
    SESSION.with(|s| {
        if let Some(shared) = s.borrow().as_ref() {
            Controller::attach(vm, shared);
        }
    });
}

/// What a debugged run starts with: `{ breakpoints: { "<source>": [{ line,
/// condition, hitCondition, logMessage }] }, functionBreakpoints: [{ name,
/// condition }], exceptions: ["uncaught", "all", "taint", "blocked"],
/// stopOnEntry }`; sources are the package's paths (`src/main.wowlua`).
pub struct Setup {
    pub link: Box<dyn Link>,
    pub settings: Json,
}

/// Starts the session: the controller's state with the setup's
/// breakpoints, configured (no handshake: the page sent them with the run).
pub fn start(
    setup: Setup,
    mapper: wowlua_debug::controller::Mapper,
    files: wowlua_debug::controller::Mapper,
) -> Result<SharedRef, String> {
    let watch = wowlua_host::Stopwatch::start();
    let clock: wowlua_debug::Clock = Rc::new(move || watch.elapsed().as_nanos() as u64);
    let shared = Shared::new(setup.link, mapper, clock);
    {
        let mut s = shared.borrow_mut();
        s.files = Some(files);
        s.drives_time = true;
        s.configured = true;
        let settings = &setup.settings;
        if let Some(map) = settings["breakpoints"].as_object() {
            for (source, list) in map {
                let specs: Vec<LineBreakpoint> = serde_json::from_value(list.clone())
                    .map_err(|e| format!("breakpoints of {source}: {e}"))?;
                s.record_lines(source, specs);
            }
        }
        if !settings["functionBreakpoints"].is_null() {
            let specs: Vec<FunctionBreakpoint> =
                serde_json::from_value(settings["functionBreakpoints"].clone())
                    .map_err(|e| format!("function breakpoints: {e}"))?;
            s.record_functions(specs);
        }
        if let Some(list) = settings["exceptions"].as_array() {
            let filters: Vec<String> = list
                .iter()
                .filter_map(|f| f.as_str().map(str::to_string))
                .collect();
            s.record_filters(&filters);
        }
        if settings["stopOnEntry"].as_bool() == Some(true) {
            s.stop_on_entry();
        }
    }
    SESSION.with(|s| *s.borrow_mut() = Some(shared.clone()));
    Ok(shared)
}

/// Ends the session: tells the page, and no VM made later is attached.
pub fn end(shared: &SharedRef, summary: Option<String>) {
    SESSION.with(|s| *s.borrow_mut() = None);
    if let Ok(mut s) = shared.try_borrow_mut() {
        s.link.event(Event::Terminated {
            summary,
            failed: false,
        });
    }
}

/// A request as the page writes it: `{ id, command, … }`.
pub fn parse_request(text: &str) -> Option<(u64, Request)> {
    let value: Json = serde_json::from_str(text).ok()?;
    let id = value["id"].as_u64()?;
    let request: Request = serde_json::from_value(value).ok()?;
    Some((id, request))
}

pub fn response_json(id: u64, r: &Result<Response, String>) -> Json {
    match r {
        Ok(response) => json!({ "id": id, "response": response }),
        Err(error) => json!({ "id": id, "error": error }),
    }
}

/// A link over queues: requests given beforehand, everything sent kept.
/// Natively, the tests' front-end: its requests are for when the session
/// waits (stopped); a wait with no request left is the front-end gone.
#[derive(Clone, Default)]
pub struct QueueLink {
    pub requests: Rc<RefCell<VecDeque<(u64, Request)>>>,
    pub sent: Rc<RefCell<Vec<Json>>>,
    gone: Rc<RefCell<bool>>,
}

impl Link for QueueLink {
    fn recv(&mut self, timeout: Option<std::time::Duration>) -> Option<(u64, Request)> {
        if timeout.is_some() {
            return None;
        }
        let next = self.requests.borrow_mut().pop_front();
        if next.is_none() {
            *self.gone.borrow_mut() = true;
        }
        next
    }

    fn respond(&mut self, id: u64, r: Result<Response, String>) {
        self.sent.borrow_mut().push(response_json(id, &r));
    }

    fn event(&mut self, e: Event) {
        self.sent.borrow_mut().push(json!(e));
    }

    fn gone(&self) -> bool {
        *self.gone.borrow()
    }
}

/// The page's link (the worker's side of its mailbox, `web/worker.js`):
/// `wowluaDebugRecv(timeoutMs)` waits up to `timeoutMs` (-1: until one
/// comes) and gives the next request's text; `wowluaDebugSend(text)` posts
/// a response or event to the page.
#[cfg(target_arch = "wasm32")]
pub struct PageLink;

#[cfg(target_arch = "wasm32")]
mod page {
    use wasm_bindgen::prelude::*;

    #[wasm_bindgen]
    extern "C" {
        #[wasm_bindgen(js_name = wowluaDebugRecv)]
        pub fn recv(timeout_ms: f64) -> Option<String>;
        #[wasm_bindgen(js_name = wowluaDebugSend)]
        pub fn send(text: &str);
    }
}

#[cfg(target_arch = "wasm32")]
impl Link for PageLink {
    fn recv(&mut self, timeout: Option<std::time::Duration>) -> Option<(u64, Request)> {
        let text = page::recv(timeout.map_or(-1.0, |t| t.as_secs_f64() * 1000.0))?;
        match parse_request(&text) {
            Some(r) => Some(r),
            None => {
                page::send(&json!({ "error": format!("not a request: {text}") }).to_string());
                None
            }
        }
    }

    fn respond(&mut self, id: u64, r: Result<Response, String>) {
        page::send(&response_json(id, &r).to_string());
    }

    fn event(&mut self, e: Event) {
        page::send(&json!(e).to_string());
    }

    fn gone(&self) -> bool {
        false
    }
}
