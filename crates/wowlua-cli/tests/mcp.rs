//! `wowlua mcp` (spec debugger §18.12.2): an agent drives an addon session,
//! and checks, formats and tests the package (decision T549).

use std::fs;
use std::io::{BufRead, BufReader, Write};
use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};

use serde_json::{Value, json};

const ADDON: &str = r#"local f = CreateFrame("Frame", "CounterFrame", UIParent)
f:SetSize(200, 100)
f:SetPoint("CENTER")
local b = CreateFrame("Button", "CounterButton", f)
b:SetSize(100, 30)
b:SetPoint("CENTER")
local label = b:CreateFontString(nil, "OVERLAY")
label:SetFont("Fonts\\FRIZQT__.TTF", 12)
label:SetAllPoints()
label:SetText("Count")
CounterClicks = 0
b:SetScript("OnClick", function(self, button)
    CounterClicks = CounterClicks + 1
    label:SetText("Count " .. CounterClicks)
end)
"#;

struct Mcp {
    child: Child,
    stdin: ChildStdin,
    stdout: BufReader<ChildStdout>,
    id: u64,
}

impl Mcp {
    fn call(&mut self, method: &str, params: Value) -> Value {
        self.id += 1;
        let msg = json!({ "jsonrpc": "2.0", "id": self.id, "method": method, "params": params });
        writeln!(self.stdin, "{msg}").unwrap();
        self.stdin.flush().unwrap();
        let mut line = String::new();
        self.stdout.read_line(&mut line).unwrap();
        let v: Value = serde_json::from_str(&line).unwrap_or_else(|e| panic!("{e}: {line:?}"));
        assert_eq!(v["id"], self.id, "{v:#}");
        v["result"].clone()
    }

    /// A tool's text (all its text parts) and whether it failed.
    fn tool(&mut self, name: &str, args: Value) -> (String, bool, Value) {
        let r = self.call("tools/call", json!({ "name": name, "arguments": args }));
        let text: Vec<String> = r["content"]
            .as_array()
            .unwrap()
            .iter()
            .filter_map(|c| c["text"].as_str().map(str::to_string))
            .collect();
        (text.join("\n"), r["isError"] == true, r)
    }

    fn ok(&mut self, name: &str, args: Value) -> String {
        let (text, failed, _) = self.tool(name, args.clone());
        assert!(!failed, "{name} {args}: {text}");
        text
    }
}

impl Drop for Mcp {
    fn drop(&mut self) {
        let _ = self.child.kill();
    }
}

#[test]
fn an_agent_drives_and_debugs_an_addon() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    fs::write(
        root.join("wowlua.json"),
        r#"{ "package": { "name": "counter", "version": "1.0.0" }, "flavors": { "standard": {} }, "addon": { "title": "Counter" } }"#,
    )
    .unwrap();
    fs::create_dir_all(root.join("src")).unwrap();
    fs::write(root.join("src/counter.lua"), ADDON).unwrap();
    let mut child = Command::new(assert_cmd::cargo::cargo_bin("wowlua"))
        .arg("mcp")
        .current_dir(root)
        .env("NO_COLOR", "1")
        .env("WOWLUA_HOME", root.join(".wowlua-home"))
        .env("WOWLUA_WOW_PATH", root.join("no-wow-here"))
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    let mut m = Mcp {
        stdin: child.stdin.take().unwrap(),
        stdout: BufReader::new(child.stdout.take().unwrap()),
        child,
        id: 0,
    };
    let init = m.call("initialize", json!({ "protocolVersion": "2025-06-18", "capabilities": {}, "clientInfo": { "name": "test", "version": "0" } }));
    assert_eq!(init["serverInfo"]["name"], "wowlua");
    let tools = m.call("tools/list", json!({}));
    assert!(tools["tools"].as_array().unwrap().len() > 20);

    let started = m.ok("start_session", json!({ "addon": true }));
    assert!(
        started.contains("session of the addon: running"),
        "{started}"
    );

    // Where the button is, in screenshot pixels; a click there.
    let found = m.ok("find_frames", json!({ "query": "CounterButton" }));
    let data: Value = serde_json::from_str(found.split("\n(session").next().unwrap()).unwrap();
    let rect = &data[0]["rect"];
    let (x, y) = (
        rect["x"].as_f64().unwrap() + rect["width"].as_f64().unwrap() / 2.0,
        rect["y"].as_f64().unwrap() + rect["height"].as_f64().unwrap() / 2.0,
    );
    // What is there, and the button's details.
    let button = data[0]["id"].as_u64().unwrap();
    let picked = m.ok("pick", json!({ "x": x, "y": y }));
    let picked: Value = serde_json::from_str(picked.split("\n(session").next().unwrap()).unwrap();
    assert_eq!(picked["frame"], button, "{picked}");
    let inspected = m.ok("inspect", json!({ "id": button }));
    assert!(inspected.contains("OnClick"), "{inspected}");
    assert!(inspected.contains("src/counter.lua"), "{inspected}");
    m.ok("click", json!({ "x": x, "y": y }));
    assert_eq!(
        m.ok("eval", json!({ "expression": "CounterClicks" })),
        "1 (number)"
    );
    let tree = m.ok("frame_tree", json!({ "root": "CounterFrame" }));
    assert!(tree.contains("Count 1"), "{tree}");

    // A breakpoint in the handler, hit by a click.
    m.ok(
        "set_breakpoints",
        json!({ "source": "src/counter.lua", "lines": [14] }),
    );
    let clicked = m.ok("click", json!({ "frame": "CounterButton" }));
    assert!(clicked.contains("stopped (breakpoint)"), "{clicked}");
    let stack = m.ok("stack", json!({}));
    assert!(stack.contains("src/counter.lua:14"), "{stack}");
    assert!(
        stack.contains("CounterButton:OnClick (dispatch)"),
        "{stack}"
    );
    let vars = m.ok("variables", json!({}));
    assert!(vars.contains("button = \"LeftButton\""), "{vars}");
    assert_eq!(
        m.ok("eval", json!({ "expression": "CounterClicks", "frame": 1 })),
        "2 (number)"
    );
    let listed = m.ok("breakpoints", json!({}));
    assert!(
        listed.contains("src/counter.lua:14: set, hit 1 times"),
        "{listed}"
    );
    let ran = m.ok("continue", json!({}));
    assert!(ran.starts_with("running"), "{ran}");
    assert_eq!(
        m.ok("eval", json!({ "expression": "CounterClicks" })),
        "2 (number)"
    );
    m.ok(
        "set_breakpoints",
        json!({ "source": "src/counter.lua", "lines": [] }),
    );

    // The profiler: the handler's cost.
    m.ok("profile_start", json!({}));
    for _ in 0..3 {
        m.ok("click", json!({ "frame": "CounterButton" }));
    }
    m.ok("advance", json!({ "frames": 10 }));
    let speedscope = root.join("profile.json");
    let report = m.ok(
        "profile_stop",
        json!({ "speedscope": speedscope.to_string_lossy() }),
    );
    assert!(report.contains("script CounterButton:OnClick"), "{report}");
    assert!(report.contains("function src/counter.lua:12"), "{report}");
    let ss: Value = serde_json::from_slice(&fs::read(&speedscope).unwrap()).unwrap();
    assert_eq!(ss["profiles"].as_array().unwrap().len(), 3);

    // The events told, the units, the screen and the world.
    m.ok("events", json!({ "log": true }));
    m.ok("fire_event", json!({ "event": "PLAYER_REGEN_DISABLED" }));
    let events = m.ok("events", json!({}));
    assert!(events.contains("PLAYER_REGEN_DISABLED"), "{events}");
    let units = m.ok("units", json!({}));
    assert!(units.trim_start().starts_with('['), "{units}");
    // (the UI is 768 high: a 4:3 screen is 1024 wide, a 16:9 one 1365)
    m.ok("set_screen", json!({ "width": 1024, "height": 768 }));
    assert_eq!(
        m.ok(
            "eval",
            json!({ "expression": "math.floor(GetScreenWidth() + 0.5)" })
        ),
        "1024 (number)"
    );
    let world = m.ok("save_world", json!({}));
    let world: Value = serde_json::from_str(world.split("\n(session").next().unwrap()).unwrap();
    assert!(world.is_object(), "{world}");

    // A screenshot, where there is a GPU.
    let (text, failed, raw) = m.tool("screenshot", json!({ "frame": "CounterFrame" }));
    if failed {
        assert!(text.contains("GPU"), "{text}");
    } else {
        assert_eq!(raw["content"][0]["type"], "image");
        assert_eq!(raw["content"][0]["mimeType"], "image/png");
    }
    assert_eq!(m.ok("stop_session", json!({})), "the session ended");
}

const TESTS: &str = r#"import test, expect from "wowlua:test"

@test("counts up")
local function counts()
    expect(1 + 1).toBe(2)
end

@test("counts wrong")
local function wrong()
    expect(1 + 1).toBe(3)
end
"#;

/// The package's commands without a session: check, fmt, test and its
/// coverage (decision T549).
#[test]
fn an_agent_checks_formats_and_tests_the_package() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    fs::write(
        root.join("wowlua.json"),
        r#"{ "package": { "name": "counter", "version": "1.0.0" }, "flavors": { "standard": {} }, "addon": { "title": "Counter" } }"#,
    )
    .unwrap();
    fs::create_dir_all(root.join("src")).unwrap();
    fs::create_dir_all(root.join("tests")).unwrap();
    fs::write(
        root.join("src/counter.wowlua"),
        "local x: number = \"one\"\n",
    )
    .unwrap();
    fs::write(root.join("tests/counter.test.wowlua"), TESTS).unwrap();
    let mut child = Command::new(assert_cmd::cargo::cargo_bin("wowlua"))
        .arg("mcp")
        .current_dir(root)
        .env("NO_COLOR", "1")
        .env("WOWLUA_HOME", root.join(".wowlua-home"))
        .env("WOWLUA_WOW_PATH", root.join("no-wow-here"))
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    let mut m = Mcp {
        stdin: child.stdin.take().unwrap(),
        stdout: BufReader::new(child.stdout.take().unwrap()),
        child,
        id: 0,
    };
    m.call("initialize", json!({ "protocolVersion": "2025-06-18", "capabilities": {}, "clientInfo": { "name": "test", "version": "0" } }));

    // The type error, one line with its place and code.
    let (checked, failed, _) = m.tool("check", json!({}));
    assert!(failed, "{checked}");
    assert!(checked.contains("src/counter.wowlua:1"), "{checked}");
    assert!(checked.contains("E2001"), "{checked}");
    fs::write(
        root.join("src/counter.wowlua"),
        "local   x: number = 1\nprint( x )\n",
    )
    .unwrap();
    let checked = m.ok("check", json!({}));
    assert!(checked.contains("Checked Counter"), "{checked}");

    // Formatting: told, then done.
    let (told, failed, _) = m.tool("fmt", json!({ "check": true }));
    assert!(failed, "{told}");
    assert!(told.contains("counter.wowlua"), "{told}");
    m.ok("fmt", json!({}));
    m.ok("fmt", json!({ "check": true }));
    assert_eq!(
        fs::read_to_string(root.join("src/counter.wowlua")).unwrap(),
        "local x: number = 1\nprint(x)\n"
    );

    // The tests: the counts and the failure, with its place.
    let (ran, failed, _) = m.tool("test", json!({}));
    assert!(failed, "{ran}");
    assert!(ran.starts_with("1 passed, 1 failed, 0 skipped"), "{ran}");
    assert!(ran.contains("failed: counts wrong"), "{ran}");
    assert!(ran.contains("counter.test.wowlua:10"), "{ran}");
    assert!(ran.contains("expected 2 to be 3"), "{ran}");
    let only = m.ok("test", json!({ "filter": "counts up" }));
    assert!(only.starts_with("1 passed, 0 failed"), "{only}");
    let covered = m.ok("test", json!({ "filter": "counts up", "coverage": true }));
    assert!(covered.contains("coverage: lines "), "{covered}");
}

#[test]
fn a_package_that_does_not_load_says_why() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    fs::write(
        root.join("wowlua.json"),
        r#"{ "package": { "name": "counter", "version": "1.0.0" }, "flavors": { "standard": {} }, "addon": { "title": "Counter" },
            "dependencies": { "missing": { "path": "../nope" } } }"#,
    )
    .unwrap();
    let out = Command::new(assert_cmd::cargo::cargo_bin("wowlua"))
        .arg("mcp")
        .current_dir(root)
        .env("NO_COLOR", "1")
        .env("WOWLUA_HOME", root.join(".wowlua-home"))
        .stdin(Stdio::null())
        .output()
        .unwrap();
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(!out.status.success());
    assert!(stderr.contains("E4102"), "{stderr}");
    assert!(!stderr.contains("no wowlua.json found"), "{stderr}");
}
