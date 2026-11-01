//! The Debug Adapter Protocol (spec debugger §18.12.1, decision T313):
//! `wowlua debug` reads DAP messages, turns them into [`Request`]s for a
//! session thread, and its answers and events back into DAP's.

use std::collections::HashMap;
use std::io::{BufRead, BufReader, Read, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc::{Sender, channel};
use std::sync::{Arc, Mutex};

use serde_json::{Value as Json, json};

use crate::protocol::*;
use crate::session::{ChannelLink, ClockMode, Options, Outgoing};

/// What starts sessions: the CLI builds the package and gives the test
/// runner's suite.
pub trait Launcher: Send + Sync + 'static {
    /// The package root (DAP's paths are absolute; the session's relative).
    fn root(&self) -> PathBuf;
    /// Builds the package and runs `session` with its suite, on this
    /// thread (the session's). The suite's `instrument` must be
    /// [`crate::session::instrument`]. An error before the session ran.
    fn run(
        &self,
        opts: &Options,
        session: &mut dyn FnMut(&wowlua_test::Suite, Option<crate::controller::Screenshotter>),
    ) -> Result<(), String>;
    /// Runs a wowlua command in the package (`check`, `fmt`, `build`,
    /// `test` with its arguments) for the MCP tools that are not a
    /// session's (decision T549): whether it succeeded, and what it
    /// printed. A host that runs none answers an error.
    fn command(&self, args: &[String]) -> Result<(bool, String), String> {
        let _ = args;
        Err("this host runs no wowlua commands".into())
    }
}

/// Reads one DAP message (`Content-Length` framed JSON).
pub fn read_message(r: &mut impl BufRead) -> Option<Json> {
    let mut len: Option<usize> = None;
    loop {
        let mut line = String::new();
        if r.read_line(&mut line).ok()? == 0 {
            return None;
        }
        let line = line.trim_end();
        if line.is_empty() {
            if len.is_some() {
                break;
            }
            continue;
        }
        if let Some(v) = line.strip_prefix("Content-Length:") {
            len = v.trim().parse().ok();
        }
    }
    let mut buf = vec![0; len?];
    r.read_exact(&mut buf).ok()?;
    serde_json::from_slice(&buf).ok()
}

pub fn write_message(w: &mut impl Write, msg: &Json) -> std::io::Result<()> {
    let body = serde_json::to_vec(msg)?;
    write!(w, "Content-Length: {}\r\n\r\n", body.len())?;
    w.write_all(&body)?;
    w.flush()
}

/// The DAP side's state: where to write, the requests waiting for their
/// answers.
struct Writer {
    out: Mutex<Box<dyn Write + Send>>,
    seq: AtomicU64,
    /// Our request id → (DAP's request seq, its command).
    pending: Mutex<HashMap<u64, (u64, String)>>,
    root: PathBuf,
}

impl Writer {
    fn send(&self, mut msg: Json) {
        msg["seq"] = json!(self.seq.fetch_add(1, Ordering::Relaxed));
        if let Ok(mut out) = self.out.lock() {
            let _ = write_message(&mut *out, &msg);
        }
    }

    fn respond(&self, request_seq: u64, command: &str, r: Result<Json, String>) {
        let msg = match r {
            Ok(body) => json!({
                "type": "response",
                "request_seq": request_seq,
                "success": true,
                "command": command,
                "body": body,
            }),
            Err(message) => json!({
                "type": "response",
                "request_seq": request_seq,
                "success": false,
                "command": command,
                "message": message,
                "body": { "error": { "id": 1, "format": message } },
            }),
        };
        self.send(msg);
    }

    fn event(&self, event: &str, body: Json) {
        self.send(json!({ "type": "event", "event": event, "body": body }));
    }

    fn abs(&self, rel: &str) -> String {
        native(&self.root.join(rel).to_string_lossy())
    }
}

/// A path with the platform's separators (`addons/Other\\x.lua` on Windows
/// is `addons\\Other\\x.lua`), as editors compare them.
fn native(path: &str) -> String {
    if cfg!(windows) {
        path.replace('/', "\\")
    } else {
        path.to_string()
    }
}

/// A client's path relative to the package root, as the session names
/// sources. Windows' `\\?\` prefix and the case of its letters (VS Code
/// sends `c:\`) do not matter there.
fn rel(root: &Path, path: &str) -> String {
    let slashed = |p: &Path| {
        crate::session::verbatim_free(p)
            .to_string_lossy()
            .replace('\\', "/")
    };
    let p = slashed(Path::new(path));
    let r = slashed(root);
    let r = r.trim_end_matches('/');
    let same = |a: &str, b: &str| {
        if cfg!(windows) {
            a.eq_ignore_ascii_case(b)
        } else {
            a == b
        }
    };
    match p.get(..r.len()) {
        Some(head) if !r.is_empty() && same(head, r) && p[r.len()..].starts_with('/') => {
            p[r.len() + 1..].to_string()
        }
        _ => p,
    }
}

fn breakpoint_json(w: &Writer, b: &BreakpointState, source: Option<&str>) -> Json {
    let mut j = json!({ "id": b.id, "verified": b.verified });
    if let Some(l) = b.line {
        j["line"] = json!(l);
    }
    if let Some(m) = &b.message {
        j["message"] = json!(m);
    }
    if let Some(s) = source {
        j["source"] = json!({ "path": w.abs(s) });
    }
    j
}

fn variable_json(v: &Variable) -> Json {
    let mut ty = v.ty.clone();
    if v.taint != "secure" {
        ty = format!("{ty} · tainted by {}", v.taint);
    }
    let value = if v.secret {
        format!("secret {}", v.value)
    } else {
        v.value.clone()
    };
    json!({
        "name": v.name,
        "value": value,
        "type": ty,
        "variablesReference": v.reference,
    })
}

/// A DAP response body from a session's response.
fn response_body(w: &Writer, command: &str, r: Response, source: Option<&str>) -> Json {
    match r {
        Response::Ok => match command {
            "continue" => json!({ "allThreadsContinued": true }),
            // A slash command typed in the console: it ran.
            "evaluate" => json!({ "result": "", "variablesReference": 0 }),
            _ => json!({}),
        },
        Response::Breakpoints { breakpoints } => json!({
            "breakpoints": breakpoints.iter().map(|b| breakpoint_json(w, b, source)).collect::<Vec<_>>(),
        }),
        Response::StackTrace { frames } => {
            // Code not the package's is shown dimmed, and the editor skips
            // it for the package's frame below; with none below, the editor
            // shows it (a stop in Blizzard's UI as it loads).
            let below = frames.iter().any(|f| f.source.is_some());
            let list: Vec<Json> = frames
                .iter()
                .map(|f| {
                    let mut j = json!({
                        "id": f.id,
                        "name": f.name,
                        "line": f.line,
                        "column": 1,
                    });
                    match (&f.source, &f.chunk) {
                        (Some(s), _) => j["source"] = json!({ "path": w.abs(s), "name": s }),
                        (None, Some(c)) => {
                            // Code not the package's: its file when it has
                            // one on disk (Blizzard's UI, other addons), so
                            // the editor opens it at the line.
                            j["source"] = match &f.file {
                                Some(path) => json!({
                                    "name": path.rsplit(['/', '\\']).next().unwrap_or(c),
                                    "path": native(path),
                                }),
                                None => json!({ "name": c }),
                            };
                            if below {
                                j["source"]["presentationHint"] = json!("deemphasize");
                                j["presentationHint"] = json!("subtle");
                            }
                        }
                        (None, None) => j["presentationHint"] = json!("label"),
                    }
                    j
                })
                .collect();
            json!({ "stackFrames": list, "totalFrames": frames.len() })
        }
        Response::Scopes { scopes } => json!({
            "scopes": scopes.iter().map(|s| json!({
                "name": s.name,
                "variablesReference": s.reference,
                "expensive": s.expensive,
                "presentationHint": if s.name == "Locals" { "locals" } else { "" },
            })).collect::<Vec<_>>(),
        }),
        Response::Variables { variables } => json!({
            "variables": variables.iter().map(variable_json).collect::<Vec<_>>(),
        }),
        Response::Value { variable } => {
            let v = variable_json(&variable);
            match command {
                "evaluate" => json!({
                    "result": v["value"],
                    "type": v["type"],
                    "variablesReference": variable.reference,
                }),
                _ => json!({
                    "value": v["value"],
                    "type": v["type"],
                    "variablesReference": variable.reference,
                }),
            }
        }
        Response::Image {
            png_base64,
            width,
            height,
        } => json!({ "png": png_base64, "width": width, "height": height }),
        Response::Data { data } => data,
        Response::Output { lines } => json!({ "lines": lines }),
    }
}

/// A DAP request as a session request; `Err` answers it here.
fn to_request(
    w: &Writer,
    command: &str,
    args: &Json,
) -> Result<Option<(Request, Option<String>)>, String> {
    let s = |k: &str| args.get(k).and_then(Json::as_str).map(str::to_string);
    Ok(Some(match command {
        "setBreakpoints" => {
            let path = args
                .pointer("/source/path")
                .and_then(Json::as_str)
                .ok_or("setBreakpoints needs source.path")?;
            let source = rel(&w.root, path);
            let breakpoints = args
                .get("breakpoints")
                .and_then(Json::as_array)
                .map(|a| {
                    a.iter()
                        .map(|b| LineBreakpoint {
                            line: b.get("line").and_then(Json::as_u64).unwrap_or(0) as u32,
                            condition: b
                                .get("condition")
                                .and_then(Json::as_str)
                                .map(str::to_string),
                            hit_condition: b
                                .get("hitCondition")
                                .and_then(Json::as_str)
                                .map(str::to_string),
                            log_message: b
                                .get("logMessage")
                                .and_then(Json::as_str)
                                .map(str::to_string),
                        })
                        .collect()
                })
                .unwrap_or_default();
            (
                Request::SetBreakpoints {
                    source: source.clone(),
                    breakpoints,
                },
                Some(source),
            )
        }
        "setFunctionBreakpoints" => (
            Request::SetFunctionBreakpoints {
                breakpoints: args
                    .get("breakpoints")
                    .and_then(Json::as_array)
                    .map(|a| {
                        a.iter()
                            .map(|b| FunctionBreakpoint {
                                name: b
                                    .get("name")
                                    .and_then(Json::as_str)
                                    .unwrap_or("")
                                    .to_string(),
                                condition: b
                                    .get("condition")
                                    .and_then(Json::as_str)
                                    .map(str::to_string),
                            })
                            .collect()
                    })
                    .unwrap_or_default(),
            },
            None,
        ),
        "setExceptionBreakpoints" => (
            Request::SetExceptionBreakpoints {
                filters: args
                    .get("filters")
                    .and_then(Json::as_array)
                    .map(|a| {
                        a.iter()
                            .filter_map(|f| f.as_str().map(str::to_string))
                            .collect()
                    })
                    .unwrap_or_default(),
            },
            None,
        ),
        "configurationDone" => (Request::ConfigurationDone, None),
        "continue" => (Request::Continue, None),
        "next" => (Request::Next, None),
        "stepIn" => (Request::StepIn, None),
        "stepOut" => (Request::StepOut, None),
        "pause" => (Request::Pause, None),
        "stackTrace" => (Request::StackTrace, None),
        "scopes" => (
            Request::Scopes {
                frame: args.get("frameId").and_then(Json::as_u64).unwrap_or(0) as u32,
            },
            None,
        ),
        "variables" => (
            Request::Variables {
                reference: args
                    .get("variablesReference")
                    .and_then(Json::as_u64)
                    .unwrap_or(0) as u32,
            },
            None,
        ),
        "setVariable" => (
            Request::SetVariable {
                reference: args
                    .get("variablesReference")
                    .and_then(Json::as_u64)
                    .unwrap_or(0) as u32,
                name: s("name").unwrap_or_default(),
                value: s("value").unwrap_or_default(),
            },
            None,
        ),
        // A console line starting with `/` is a slash command, as typed in
        // the chat box (`/wdemo`); anything else is Lua.
        "evaluate"
            if s("context").as_deref() != Some("watch")
                && s("context").as_deref() != Some("hover")
                && s("expression").is_some_and(|e| e.trim_start().starts_with('/')) =>
        {
            (
                Request::Input {
                    input: Input::Slash {
                        text: s("expression").unwrap_or_default().trim().to_string(),
                    },
                },
                None,
            )
        }
        "evaluate" => (
            Request::Evaluate {
                expression: s("expression").unwrap_or_default(),
                frame: args.get("frameId").and_then(Json::as_u64).map(|f| f as u32),
                context: match s("context").as_deref() {
                    Some("watch") => EvalContext::Watch,
                    Some("hover") => EvalContext::Hover,
                    _ => EvalContext::Repl,
                },
            },
            None,
        ),
        "terminate" | "disconnect" => (Request::Terminate, None),
        "wowlua/screenshot" => (
            Request::Screenshot {
                frame: s("frame"),
                scale: args.get("scale").and_then(Json::as_f64),
            },
            None,
        ),
        "wowlua/frameTree" => (
            Request::FrameTree {
                root: s("root"),
                hidden: args.get("hidden").and_then(Json::as_bool).unwrap_or(false),
            },
            None,
        ),
        "wowlua/findFrames" => (
            Request::FindFrames {
                query: s("query").unwrap_or_default(),
            },
            None,
        ),
        "wowlua/chatLines" => (
            Request::ChatLines {
                since: args.get("since").and_then(Json::as_u64).unwrap_or(0),
            },
            None,
        ),
        "wowlua/worldPoint" => (
            Request::WorldPoint {
                x: args.get("x").and_then(Json::as_f64).unwrap_or(0.0),
                y: args.get("y").and_then(Json::as_f64).unwrap_or(0.0),
                camera: None,
            },
            None,
        ),
        "wowlua/showWorld" => (
            Request::ShowWorld {
                on: args.get("on").and_then(Json::as_bool).unwrap_or(false),
            },
            None,
        ),
        "wowlua/input" => (
            Request::Input {
                input: serde_json::from_value(args.clone())
                    .map_err(|e| format!("wowlua/input: {e}"))?,
            },
            None,
        ),
        "wowlua/advance" => (
            Request::Advance {
                seconds: args.get("seconds").and_then(Json::as_f64),
                frames: args.get("frames").and_then(Json::as_u64).map(|f| f as u32),
            },
            None,
        ),
        "wowlua/fireEvent" => (
            Request::FireEvent {
                event: s("event").unwrap_or_default(),
                args: args
                    .get("args")
                    .and_then(Json::as_array)
                    .cloned()
                    .unwrap_or_default(),
            },
            None,
        ),
        "wowlua/world" => (
            Request::World {
                script: s("script").unwrap_or_default(),
            },
            None,
        ),
        "wowlua/profile" => match s("action").as_deref() {
            Some("start") => (Request::ProfileStart, None),
            Some("stop") => (Request::ProfileStop, None),
            _ => return Err("wowlua/profile: action \"start\" or \"stop\"".into()),
        },
        _ => return Ok(None),
    }))
}

fn capabilities() -> Json {
    json!({
        "supportsConfigurationDoneRequest": true,
        "supportsFunctionBreakpoints": true,
        "supportsConditionalBreakpoints": true,
        "supportsHitConditionalBreakpoints": true,
        "supportsLogPoints": true,
        "supportsSetVariable": true,
        "supportsEvaluateForHovers": true,
        "supportsTerminateRequest": true,
        "exceptionBreakpointFilters": [
            { "filter": "uncaught", "label": "Errors that reach the error handler", "default": true },
            { "filter": "all", "label": "Every error, caught ones too", "default": false },
            { "filter": "taint", "label": "Execution becomes tainted", "default": false },
            { "filter": "blocked", "label": "A protected action is blocked", "default": true },
        ],
    })
}

/// Serves DAP on a reader and writer until the client disconnects.
pub fn serve(
    input: impl Read + Send + 'static,
    output: Box<dyn Write + Send>,
    launcher: Arc<dyn Launcher>,
) {
    let writer = Arc::new(Writer {
        out: Mutex::new(output),
        seq: AtomicU64::new(1),
        pending: Mutex::new(HashMap::new()),
        root: launcher.root(),
    });
    let mut reader = BufReader::new(input);
    let mut to_session: Option<Sender<(u64, Request)>> = None;
    let interrupt = Arc::new(AtomicBool::new(false));
    let mut next_id: u64 = 1;
    // Sources of setBreakpoints requests waiting, for their answers' paths.
    let sources: Arc<Mutex<HashMap<u64, String>>> = Arc::new(Mutex::new(HashMap::new()));
    let mut session_thread: Option<std::thread::JoinHandle<()>> = None;
    // Closed when the thread that writes the session's events is done.
    let mut forwarded: Option<std::sync::mpsc::Receiver<()>> = None;
    while let Some(msg) = read_message(&mut reader) {
        if msg.get("type").and_then(Json::as_str) != Some("request") {
            continue;
        }
        let seq = msg.get("seq").and_then(Json::as_u64).unwrap_or(0);
        let command = msg
            .get("command")
            .and_then(Json::as_str)
            .unwrap_or("")
            .to_string();
        let args = msg.get("arguments").cloned().unwrap_or(json!({}));
        match command.as_str() {
            "initialize" => {
                writer.respond(seq, &command, Ok(capabilities()));
                continue;
            }
            "threads" => {
                writer.respond(
                    seq,
                    &command,
                    Ok(json!({ "threads": [{ "id": 1, "name": "main" }] })),
                );
                continue;
            }
            "launch" => {
                if to_session.is_some() {
                    writer.respond(seq, &command, Err("a session is running".into()));
                    continue;
                }
                let opts = match Options::from_launch(&args, &writer.root, ClockMode::Realtime) {
                    Ok(o) => o,
                    Err(e) => {
                        writer.respond(seq, &command, Err(e));
                        continue;
                    }
                };
                let (tx, rx) = channel::<(u64, Request)>();
                let (otx, orx) = channel::<Outgoing>();
                to_session = Some(tx);
                // The session's answers and events, to DAP.
                let w = writer.clone();
                let srcs = sources.clone();
                let launch_seq = seq;
                let (done, forwarding) = channel::<()>();
                forwarded = Some(forwarding);
                std::thread::spawn(move || {
                    let _done = done;
                    let mut launched = false;
                    for o in orx {
                        match o {
                            Outgoing::Response(id, r) => {
                                let Some((rseq, cmd)) =
                                    w.pending.lock().ok().and_then(|mut p| p.remove(&id))
                                else {
                                    continue;
                                };
                                let source = srcs.lock().ok().and_then(|mut s| s.remove(&id));
                                let body = r.map(|r| response_body(&w, &cmd, r, source.as_deref()));
                                w.respond(rseq, &cmd, body);
                            }
                            Outgoing::Event(e) => send_event(&w, e, &mut launched, launch_seq),
                        }
                    }
                });
                let launcher = launcher.clone();
                let flag = interrupt.clone();
                let err_tx = otx.clone();
                let builder = std::thread::Builder::new()
                    .name("wowlua session".into())
                    .stack_size(64 << 20);
                session_thread = builder
                    .spawn(move || {
                        let mut rx = Some(rx);
                        let mut otx = Some(otx);
                        let r = launcher.run(&opts, &mut |suite, shot| {
                            let (Some(rx), Some(otx)) = (rx.take(), otx.take()) else {
                                return;
                            };
                            let link = Box::new(ChannelLink::new(rx, otx));
                            crate::session::run(suite, &opts, link, flag.clone(), shot);
                        });
                        if let Err(e) = r {
                            let _ = err_tx.send(Outgoing::Event(Event::Terminated {
                                summary: Some(e),
                                failed: true,
                            }));
                        }
                    })
                    .ok();
                continue;
            }
            _ => {}
        }
        let mapped = to_request(&writer, &command, &args);
        let (req, source) = match mapped {
            Ok(Some(r)) => r,
            Ok(None) => {
                writer.respond(seq, &command, Err(format!("{command} is not supported")));
                continue;
            }
            Err(e) => {
                writer.respond(seq, &command, Err(e));
                continue;
            }
        };
        let Some(tx) = &to_session else {
            if matches!(req, Request::Terminate) {
                writer.respond(seq, &command, Ok(json!({})));
                break;
            }
            writer.respond(seq, &command, Err("no session: launch first".into()));
            continue;
        };
        let id = next_id;
        next_id += 1;
        if let Ok(mut p) = writer.pending.lock() {
            p.insert(id, (seq, command.clone()));
        }
        if let Some(s) = source
            && let Ok(mut m) = sources.lock()
        {
            m.insert(id, s);
        }
        let disconnect = command == "disconnect";
        if tx.send((id, req)).is_err() {
            writer.respond(seq, &command, Ok(json!({})));
            break;
        }
        interrupt.store(true, Ordering::Relaxed);
        if disconnect {
            break;
        }
    }
    // The client went away: the session ends.
    drop(to_session);
    interrupt.store(true, Ordering::Relaxed);
    if let Some(t) = session_thread {
        let _ = t.join();
    }
    // The session's last events (`terminated`) written before the process
    // ends; not for long, should something still hold a sender.
    if let Some(f) = forwarded {
        let _ = f.recv_timeout(std::time::Duration::from_secs(5));
    }
}

fn send_event(w: &Writer, e: Event, launched: &mut bool, launch_seq: u64) {
    match e {
        // (VS Code tells it as the output's "Reloading the UI")
        Event::Reloaded => {}
        Event::Initialized => {
            if !*launched {
                *launched = true;
                w.respond(launch_seq, "launch", Ok(json!({})));
            }
            w.event("initialized", json!({}));
        }
        Event::Stopped {
            reason, text, hit, ..
        } => {
            let mut body = json!({
                "reason": reason.dap(),
                "threadId": 1,
                "allThreadsStopped": true,
            });
            if let Some(t) = text {
                body["text"] = json!(t);
                body["description"] = json!(t);
            }
            if !hit.is_empty() {
                body["hitBreakpointIds"] = json!(hit);
            }
            w.event("stopped", body);
        }
        Event::Continued => w.event(
            "continued",
            json!({ "threadId": 1, "allThreadsContinued": true }),
        ),
        Event::Output { line } => {
            let mut body = json!({ "category": line.category, "output": line.text });
            if let Some(s) = &line.source {
                body["source"] = json!({ "path": w.abs(s) });
            }
            if let Some(l) = line.line {
                body["line"] = json!(l);
            }
            w.event("output", body);
        }
        Event::Breakpoint { breakpoint } => w.event(
            "breakpoint",
            json!({ "reason": "changed", "breakpoint": breakpoint_json(w, &breakpoint, None) }),
        ),
        Event::Terminated { summary, failed } => {
            if !*launched {
                *launched = true;
                w.respond(
                    launch_seq,
                    "launch",
                    Err(summary
                        .clone()
                        .unwrap_or_else(|| "the session failed to start".into())),
                );
            }
            w.event("exited", json!({ "exitCode": i32::from(failed) }));
            w.event("terminated", json!({}));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::rel;
    use std::path::Path;

    #[test]
    fn a_breakpoint_path_is_relative_to_the_root_in_either_spelling() {
        assert_eq!(
            rel(Path::new("/p/addon"), "/p/addon/src/x.lua"),
            "src/x.lua"
        );
        assert_eq!(
            rel(Path::new("/p/addon"), "/p/addons/x.lua"),
            "/p/addons/x.lua"
        );
        // Windows' canonical paths and plain ones alike
        assert_eq!(
            rel(Path::new(r"C:\p\addon"), r"\\?\C:\p\addon\src\x.lua"),
            "src/x.lua"
        );
        assert_eq!(
            rel(Path::new(r"\\?\C:\p\addon"), r"C:\p\addon\src\x.lua"),
            "src/x.lua"
        );
    }
}
