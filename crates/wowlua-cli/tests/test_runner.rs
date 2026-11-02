//! `wowlua test` (spec Tooling §16.1): test files of a package run in the
//! emulated client, per flavor.

use assert_cmd::Command;
use std::fs;
use std::path::Path;

fn wowlua(dir: &Path) -> Command {
    // The examples are in the repository: their home goes elsewhere.
    let repo = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let home = if dir.starts_with(&repo) {
        std::env::temp_dir().join(format!("wowlua-home-{}", std::process::id()))
    } else {
        dir.join(".wowlua-home")
    };
    let mut cmd = Command::cargo_bin("wowlua").unwrap();
    cmd.current_dir(dir)
        .env("NO_COLOR", "1")
        .env("WOWLUA_HOME", home)
        .env_remove("WOWLUA_WOW_PATH")
        .env_remove("WOWLUA_STDLIB_DIR");
    cmd
}

/// A package `Demo` (`flavors`: the manifest's entries) with these files.
fn project(flavors: &str, files: &[(&str, &str)]) -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    let manifest = format!(
        r#"{{ "package": {{ "name": "demo", "version": "1.0.0" }}, "flavors": {{ {flavors} }},
  "addon": {{ "title": "Demo", "saved-variables": ["DemoDB"] }} }}"#
    );
    fs::write(dir.path().join("wowlua.json"), manifest).unwrap();
    for (path, text) in files {
        let path = dir.path().join(path);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, text).unwrap();
    }
    dir
}

/// Runs `wowlua test` with `args`: success and stdout + stderr.
fn test(dir: &Path, args: &[&str]) -> (bool, String) {
    let out = wowlua(dir).arg("test").args(args).output().unwrap();
    let text = format!(
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    (out.status.success(), text)
}

const TRACKER: &str = r#"
export class Tracker
    kills: number = 0
    static function create(): Tracker
        return Tracker()
    end
    function record(n: number?)
        self.kills = self.kills + (n ?? 1)
    end
end
export function double(x: number): number
    return x * 2
end
"#;

#[test]
fn runs_tests_hooks_and_cases() {
    let dir = project(
        r#""standard": {}"#,
        &[
            ("src/tracker.wowlua", TRACKER),
            (
                "tests/tracker.test.wowlua",
                r#"
import test, beforeEach, afterEach, beforeAll, afterAll, expect, cases, skip from "wowlua:test"
import Tracker, double from "/tracker"

local tracker: Tracker
local order: string[] = {}

@beforeAll
local function first() order[#order + 1] = "all" end
@beforeEach
local function setup()
    order[#order + 1] = "each"
    tracker = Tracker.create()
end
@afterEach
local function teardown() order[#order + 1] = "after" end
@afterAll
local function last() print(table.concat(order, " ")) end

@test("records kills")
local function recordsKills()
    tracker:record(2)
    expect(tracker.kills).toBe(2)
    expect(tracker.kills).never.toBe(3)
end

@test("doubles")
@cases({ { 1, 2 }, { 2, 4 } })
local function doubles(n: number, expected: number)
    expect(double(n)).toBe(expected)
end

@skip("not yet")
@test("later")
local function later() end

@test
local function deepEquality()
    expect({ a = { 1, 2 }, b = "x" }).toEqual({ b = "x", a = { 1, 2 } })
    expect("hello").toContain("ell")
    expect({ 1, 2, 3 }).toHaveLength(3)
    expect(0.1 + 0.2).toBeCloseTo(0.3)
    expect(function() error("boom") end).toThrow("boom")
end
"#,
            ),
        ],
    );
    let (ok, out) = test(dir.path(), &[]);
    assert!(ok, "{out}");
    for line in [
        "PASS tests/tracker.test.wowlua (standard",
        "✓ records kills",
        "✓ doubles [1, 2]",
        "✓ doubles [2, 4]",
        "- later (not yet)",
        "✓ deepEquality",
        "4 passed, 0 failed, 1 skipped (1 file)",
    ] {
        assert!(out.contains(line), "missing {line:?} in:\n{out}");
    }
}

#[test]
fn failures_name_the_source_line_and_fail_the_run() {
    let dir = project(
        r#""standard": {}"#,
        &[
            ("src/tracker.wowlua", TRACKER),
            (
                "tests/fail.test.wowlua",
                r#"import test, expect from "wowlua:test"
import double from "/tracker"

@test("wrong")
local function wrong()
    expect(double(2)).toBe(5)
end

@test("raises")
local function raises()
    local t: any = nil
    print(t.field)
end
"#,
            ),
        ],
    );
    let (ok, out) = test(dir.path(), &[]);
    assert!(!ok, "{out}");
    assert!(out.contains("expected 4 to be 5"), "{out}");
    assert!(out.contains("at tests/fail.test.wowlua:6"), "{out}");
    assert!(out.contains("attempt to index"), "{out}");
    assert!(out.contains("at tests/fail.test.wowlua:12"), "{out}");
    assert!(out.contains("0 passed, 2 failed"), "{out}");
}

#[test]
fn filter_only_and_flavors_select_tests() {
    let dir = project(
        r#""standard": {}, "vanilla": {}"#,
        &[
            (
                "tests/a.test.wowlua",
                r#"import test, expect, flavors from "wowlua:test"

@test("everywhere")
local function everywhere() expect(1).toBe(1) end

@test("retail only")
@flavors("standard")
local function retail() expect(1).toBe(1) end
"#,
            ),
            (
                "tests/b.test.wowlua",
                r#"import test, only, expect from "wowlua:test"

@test("not this")
local function notThis() expect(1).toBe(2) end

@only
@test("this")
local function this() expect(1).toBe(1) end
"#,
            ),
        ],
    );
    let (ok, out) = test(dir.path(), &[]);
    assert!(ok, "{out}");
    // a: 2 tests on standard, 1 on classic; b: 1 per flavor.
    assert!(
        out.contains("5 passed, 0 failed, 0 skipped (4 files)"),
        "{out}"
    );
    assert!(!out.contains("not this"), "{out}");
    let (ok, out) = test(dir.path(), &["everywhere", "--flavor", "vanilla"]);
    assert!(ok, "{out}");
    assert!(
        out.contains("1 passed, 0 failed, 0 skipped (1 file)"),
        "{out}"
    );
}

#[test]
fn lifecycle_mocks_helpers_and_the_world() {
    let dir = project(
        r#""standard": {}"#,
        &[
            (
                "src/main.wowlua",
                r#"local addonName: string = ...
local f = CreateFrame("Frame")
f:RegisterEvent("ADDON_LOADED")
f:RegisterEvent("PLAYER_LOGIN")
f:SetScript("OnEvent", function(_: Frame, event: string, name: string?)
    if event == "ADDON_LOADED" and name == addonName then
        DemoDB = DemoDB or { logins = 0 }
    elseif event == "PLAYER_LOGIN" then
        DemoDB.logins = DemoDB.logins + 1
        print("hello " .. (MockedName ?? "?"))
    end
end)
"#,
            ),
            (
                "tests/mocks/name.wowlua",
                "MockedName = \"from the mock\"\n",
            ),
            (
                "tests/helpers/numbers.wowlua",
                "export function answer(): number\n    return 42\nend\n",
            ),
            (
                "tests/auto.test.wowlua",
                r#"import test, expect from "wowlua:test"
import * as mock from "wowlua:mock"
import answer from "./helpers/numbers"

@test("logged in once, mocks first")
local function loggedIn()
    expect(DemoDB.logins).toBe(1)
    expect(mock.printed()).toContain("hello from the mock")
    expect(answer()).toBe(42)
end

@test("spies and stubs are restored")
local function stubs()
    local spy = mock.stub(_G, "GetTime", function(): number return 5 end)
    expect(GetTime()).toBe(5)
    expect(spy).toHaveBeenCalledTimes(1)
end

@test("after a stub")
local function afterStub()
    expect(GetTime()).never.toBe(5)
end

@test("timers and waiting")
local function waiting()
    local spy = mock.spy()
    C_Timer.NewTicker(1, spy)
    mock.advanceTime(3.5)
    expect(spy).toHaveBeenCalledTimes(3)
    local fired = false
    C_Timer.After(2, function() fired = true end)
    mock.waitSeconds(2.5)
    expect(fired).toBe(true)
    C_Timer.After(1, function() mock.fireEvent("PLAYER_REGEN_ENABLED") end)
    mock.waitFor("PLAYER_REGEN_ENABLED")
end

@test("a wait times out")
local function timesOut()
    mock.waitUntil(function(): boolean return false end, "nothing")
end
"#,
            ),
            (
                "tests/manual.test.wowlua",
                r#"import test, expect, config from "wowlua:test"
import * as mock from "wowlua:mock"
config({ lifecycle = "manual" })

@test("logs in itself")
local function manual()
    expect(DemoDB).toBeNil()
    mock.login()
    expect(DemoDB.logins).toBe(1)
end
"#,
            ),
        ],
    );
    let (ok, out) = test(dir.path(), &[]);
    assert!(!ok, "{out}");
    assert!(
        out.contains("timed out after 10 s waiting for nothing"),
        "{out}"
    );
    assert!(out.contains("5 passed, 1 failed"), "{out}");
}

#[test]
fn errors_reaching_the_handler_fail_unless_read() {
    let dir = project(
        r#""standard": {}"#,
        &[(
            "tests/errors.test.wowlua",
            r#"import test, expect from "wowlua:test"
import * as mock from "wowlua:mock"

local function failLater()
    C_Timer.After(1, function() error("late failure") end)
    mock.advanceTime(1)
end

@test("unread")
local function unread() failLater() end

@test("read")
local function read()
    failLater()
    expect(mock.errors()[1]).toContain("late failure")
end
"#,
        )],
    );
    let (ok, out) = test(dir.path(), &[]);
    assert!(!ok, "{out}");
    assert!(out.contains("an error reached the error handler"), "{out}");
    assert!(out.contains("1 passed, 1 failed"), "{out}");
}

/// An error the client drops without a report (a `secureexecuterange`
/// callback's) reaches no error handler, as in the client, but fails the
/// test: the code failed.
#[test]
fn errors_the_client_drops_fail_the_test() {
    let dir = project(
        r#""standard": {}"#,
        &[(
            "tests/dropped.test.wowlua",
            r#"import test, expect from "wowlua:test"
import * as mock from "wowlua:mock"

@test("dropped")
local function dropped()
    local n = 0
    secureexecuterange({ 1, 2 }, function() n = n + 1 error("in range") end)
    expect(n).toBe(2)
    expect(#mock.errors()).toBe(0)
end

@test("fine")
local function fine()
    secureexecuterange({ 1 }, function() end)
end
"#,
        )],
    );
    let (ok, out) = test(dir.path(), &[]);
    assert!(!ok, "{out}");
    assert!(
        out.contains("an error was dropped without a report"),
        "{out}"
    );
    assert!(out.contains("secureexecuterange dropped: "), "{out}");
    assert!(out.contains("in range"), "{out}");
    assert!(out.contains("1 passed, 1 failed"), "{out}");
}

#[test]
fn test_modules_are_only_for_test_files() {
    let dir = project(
        r#""standard": {}"#,
        &[(
            "src/main.wowlua",
            "import expect from \"wowlua:test\"\nexpect(1).toBe(1)\n",
        )],
    );
    let out = wowlua(dir.path()).arg("check").output().unwrap();
    let text = String::from_utf8_lossy(&out.stderr);
    assert!(!out.status.success());
    assert!(text.contains("E8108"), "{text}");
}

#[test]
fn test_files_stay_out_of_builds() {
    let dir = project(
        r#""standard": {}"#,
        &[
            ("src/main.wowlua", "print(1)\n"),
            (
                "src/main.test.wowlua",
                "import test from \"wowlua:test\"\n@test\nlocal function t() end\n",
            ),
            ("tests/helper.wowlua", "export const x = 1\n"),
        ],
    );
    wowlua(dir.path()).arg("build").assert().success();
    let out = dir.path().join("dist/dev/Demo");
    let toc = fs::read_to_string(out.join("Demo.toc")).unwrap();
    assert!(!toc.contains("test"), "{toc}");
    assert!(!out.join("src/main.test.lua").exists());
    assert!(!out.join("wowlua/std/test.lua").exists());
    let (ok, out) = test(dir.path(), &[]);
    assert!(ok, "{out}");
    assert!(out.contains("PASS src/main.test.wowlua"), "{out}");
}

#[test]
fn a_new_addon_passes_its_tests() {
    let dir = tempfile::tempdir().unwrap();
    wowlua(dir.path())
        .args(["new", "greeter", "--name", "Greeter"])
        .assert()
        .success();
    let (ok, out) = test(&dir.path().join("greeter"), &[]);
    assert!(ok, "{out}");
    assert!(out.contains("✓ greets at login"), "{out}");
}

/// `test.blizzard-ui`: Blizzard's UI code (from `cargo xtask types fetch`,
/// skipped without it) loads before the addon; `print` goes through its
/// print handler.
#[test]
fn blizzard_ui_loads_before_the_addon() {
    let source = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/types-src");
    if !source.join("camelot/Interface").exists() {
        eprintln!("skipped: no Blizzard UI code (run `cargo xtask types fetch`)");
        return;
    }
    let dir = project(
        r#""camelot": {}"#,
        &[
            ("src/main.wowlua", "print(\"loaded\", 1)\n"),
            (
                "tests/ui.test.wowlua",
                r#"import test, expect from "wowlua:test"
import * as mock from "wowlua:mock"

@test("Blizzard's frames")
local function frames()
    expect(GameTooltip).never.toBeNil()
    local b = CreateFrame("Button", nil, UIParent, "UIPanelButtonTemplate")
    b:SetText("Hi")
    expect(b:GetText()).toBe("Hi")
    expect(mock.printed()).toContain("loaded 1")
    GameTooltip:SetOwner(UIParent, "ANCHOR_NONE")
    GameTooltip:AddLine("Hello")
    GameTooltip:AddDoubleLine("Left", "Right")
    GameTooltip:Show()
    expect(GameTooltip).toHaveLines({ "Hello", { "Left", "Right" } })
end
"#,
            ),
        ],
    );
    let manifest = dir.path().join("wowlua.json");
    let text = fs::read_to_string(&manifest).unwrap();
    fs::write(
        &manifest,
        text.replacen('{', r#"{ "test": { "blizzard-ui": true },"#, 1),
    )
    .unwrap();
    let out = wowlua(dir.path())
        .arg("test")
        .env("WOWLUA_UI_SOURCE", &source)
        .output()
        .unwrap();
    let text = format!(
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(out.status.success(), "{text}");
    assert!(text.contains("1 passed, 0 failed"), "{text}");
    assert!(!text.contains("raised"), "{text}");
}

/// Test bodies are the addon's code, tainted as its main chunk is; a
/// Blizzard addon loaded while the addon loads stays Blizzard's secure code
/// (camelot's UI code, skipped without it).
#[test]
fn test_bodies_run_with_the_addons_taint() {
    let source = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/types-src");
    if !source.join("camelot/Interface").exists() {
        eprintln!("skipped: no Blizzard UI code (run `cargo xtask types fetch`)");
        return;
    }
    let dir = project(
        r#""camelot": {}"#,
        &[
            (
                "src/main.wowlua",
                r#"export const chunkSecure = issecure()
_G.DemoChunkGlobal = 1
C_AddOns.LoadAddOn("Blizzard_MacroUI")
"#,
            ),
            (
                "tests/taint.test.wowlua",
                r#"import test, expect from "wowlua:test"
import chunkSecure from "/main"

@test("taint")
local function taint()
    _G.DemoBodyGlobal = 1
    expect(chunkSecure).toBe(false)
    expect(issecure()).toBe(false)
    expect(issecurevariable("DemoChunkGlobal")).toBe(false)
    expect(issecurevariable("DemoBodyGlobal")).toBe(false)
    expect(issecurevariable("MacroFrame_Show")).toBe(true)
end
"#,
            ),
        ],
    );
    let manifest = dir.path().join("wowlua.json");
    let text = fs::read_to_string(&manifest).unwrap();
    fs::write(
        &manifest,
        text.replacen('{', r#"{ "test": { "blizzard-ui": true },"#, 1),
    )
    .unwrap();
    let out = wowlua(dir.path())
        .arg("test")
        .env("WOWLUA_UI_SOURCE", &source)
        .output()
        .unwrap();
    let text = format!(
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(out.status.success(), "{text}");
    assert!(text.contains("1 passed, 0 failed"), "{text}");
}

/// `wowlua:aura` against Blizzard's own aura containers (camelot's UI
/// code, skipped without it): `hasContainers` finds the template, the
/// container has every method the stdlib's containers call, and one made
/// while the addon loads takes its groups without an error (the engine's
/// intrinsic stays Blizzard's code).
#[test]
fn aura_containers_in_blizzard_ui() {
    let source = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/types-src");
    if !source.join("camelot/Interface").exists() {
        eprintln!("skipped: no Blizzard UI code (run `cargo xtask types fetch`)");
        return;
    }
    let dir = project(
        r#""camelot": {}"#,
        &[
            (
                "src/main.wowlua",
                r#"import * as aura from "wowlua:aura"
export const has = aura.hasContainers()
export const early = aura.container(UIParent, {
    unit = "player",
    groups = { { key = "buffs", filter = "HELPFUL", max = 4 }, { key = "debuffs", filter = "HARMFUL", max = 2, size = 24 } },
})
"#,
            ),
            (
                "tests/aura.test.wowlua",
                r#"import test, expect from "wowlua:test"
import has, early from "/main"

@test("the engine's containers")
local function containers()
    expect(has).toBe(true)
    const made = early.frame :: any
    expect(made:GetAuraGroupFrameCount("buffs") > 0).toBe(true)
    expect(made:GetAuraGroupFrameCount("debuffs") > 0).toBe(true)
    const engine = CreateFrame("AuraContainer", nil, UIParent, "CustomAuraContainerTemplate") :: any
    for _, method in ipairs({
        "AddAuraGroup", "SetAuraGroupMaxFrameCount", "SetAuraGroupCandidateFilters",
        "SetAuraGroupFilterString", "SetAuraGroupLayout", "GetAuraGroupFrameCount",
        "GetAuraGroupFrame", "SetUnit", "UpdateAllAuras", "SetFlowLayoutAnchorPoint",
        "SetFlowLayoutGrowthDirection", "SetFlowLayoutMaximumLineSize",
    }) do
        expect(type(engine[method])).toBe("function")
    end
end
"#,
            ),
        ],
    );
    let manifest = dir.path().join("wowlua.json");
    let text = fs::read_to_string(&manifest).unwrap();
    fs::write(
        &manifest,
        text.replacen('{', r#"{ "test": { "blizzard-ui": true },"#, 1),
    )
    .unwrap();
    let out = wowlua(dir.path())
        .arg("test")
        .env("WOWLUA_UI_SOURCE", &source)
        .output()
        .unwrap();
    let text = format!(
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(out.status.success(), "{text}");
    assert!(text.contains("1 passed, 0 failed"), "{text}");
}

/// `mock.slash` with Blizzard's chat (decision T233): the line goes through
/// the chat's edit box as typed and sent, so Blizzard's own commands run
/// (it moves them out of `SlashCmdList` into its hash tables, where the
/// engine's lookup never found them), an addon's command too, and an
/// unknown one gets the client's help line.
#[test]
fn slash_through_blizzards_chat() {
    let source = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/types-src");
    if !source.join("camelot/Interface").exists() {
        eprintln!("skipped: no Blizzard UI code (run `cargo xtask types fetch`)");
        return;
    }
    let dir = project(
        r#""camelot": {}"#,
        &[
            (
                "src/main.wowlua",
                "SLASH_DEMOSLASH1 = \"/demoslash\"\nSlashCmdList.DEMOSLASH = function(msg) DEMO_SLASHED = msg end\n",
            ),
            (
                "tests/slash.test.wowlua",
                r#"import test, expect from "wowlua:test"
import * as mock from "wowlua:mock"

@test("typed into the chat")
local function typed()
    local other = mock.spawnUnit({ name = "Otherplayer", player = true, level = 60, reaction = "friendly" })
    other:setDistance(3)
    mock.player:target(other)
    expect(C_AddOns.IsAddOnLoaded("Blizzard_InspectUI")).toBe(false)
    mock.slash("/inspect")
    expect(C_AddOns.IsAddOnLoaded("Blizzard_InspectUI")).toBe(true)
    mock.slash("/demoslash hello")
    expect((_G :: any).DEMO_SLASHED).toBe("hello")
    mock.slash("/nosuchcommand")
    local printed = mock.printed()
    expect(printed[#printed]).toBe(HELP_TEXT_SIMPLE)
end
"#,
            ),
        ],
    );
    let manifest = dir.path().join("wowlua.json");
    let text = fs::read_to_string(&manifest).unwrap();
    fs::write(
        &manifest,
        text.replacen('{', r#"{ "test": { "blizzard-ui": true },"#, 1),
    )
    .unwrap();
    let out = wowlua(dir.path())
        .arg("test")
        .env("WOWLUA_UI_SOURCE", &source)
        .output()
        .unwrap();
    let text = format!(
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(out.status.success(), "{text}");
    assert!(text.contains("1 passed, 0 failed"), "{text}");
}

/// Typed matchers (decision T214) and cases (T215) are checked when the
/// tests compile.
#[test]
fn matchers_and_cases_are_type_checked() {
    let dir = project(
        r#""standard": {}"#,
        &[(
            "tests/typed.test.wowlua",
            r#"import test, expect, cases from "wowlua:test"
import * as mock from "wowlua:mock"

@test
local function wrong()
    local count = 3
    expect(count).toBe("3")
    expect(count).toHaveLength(3)
    local s = mock.spy(function(_a: number, _b: string) end)
    expect(s).toHaveBeenCalledWith("x", 1)
end

@test
@cases({ { 1, 2 }, { "a", 2 }, { 1 }, { 1, 2, 3 } })
local function sums(a: number, b: number)
    expect(a + b).never.toBe(0)
end
"#,
        )],
    );
    let (ok, out) = test(dir.path(), &[]);
    assert!(!ok, "{out}");
    for wanted in [
        "`\"3\"` is not assignable to `number`",
        "`toHaveLength` needs a string or a table; the value is `number`",
        "`\"x\"` is not assignable to `number`",
        "case 2: `\"a\"` is not assignable to `number` (parameter `a`)",
        "case 3 has no value for `b`",
        "case 4 has 3 values; the test takes 2",
    ] {
        assert!(out.contains(wanted), "missing {wanted:?} in:\n{out}");
    }
}

/// A profile's `powers` (decision T512): every power of the class, each
/// by token, the shown one by `powerType` or the class's.
#[test]
fn world_profile_powers() {
    let profile = r#"{
  "character": { "class": "DRUID", "race": "NightElf", "level": 60, "powerType": 0, "maxPower": 2000,
                 "powers": { "COMBO_POINTS": { "current": 2 }, "ENERGY": { "current": 40 } } },
  "units": [ { "name": "Rogue", "player": true, "class": "ROGUE", "reaction": "friendly", "target": true,
               "powers": { "COMBO_POINTS": { "current": 3, "max": 6 } } } ]
}"#;
    let dir = project(
        r#""camelot": {}"#,
        &[
            ("src/main.wowlua", "\n"),
            ("tests/profiles/druid.json", profile),
            (
                "tests/powers.test.wowlua",
                r#"import test, expect, config from "wowlua:test"
import * as mock from "wowlua:mock"
config({ profile = "druid" })

@test("every power of the class")
local function powers()
    local kind, token = UnitPowerType("player")
    expect(kind).toBe(0)
    expect(token).toBe("MANA")
    expect(UnitPowerMax("player")).toBe(2000)
    expect(UnitPower("player", Enum.PowerType.ComboPoints)).toBe(2)
    expect(UnitPowerMax("player", Enum.PowerType.ComboPoints)).toBe(5)
    expect(UnitPower("player", Enum.PowerType.Energy)).toBeSecret()
    expect(UnitHasPowerType("player", Enum.PowerType.Rage)).toBe(true)
    expect(UnitPower("player", Enum.PowerType.HolyPower)).toBe(0)
    local _, rogue = UnitPowerType("target")
    expect(rogue).toBe("ENERGY")
    expect(UnitPower("target", Enum.PowerType.ComboPoints)).toBe(3)
    expect(UnitPowerMax("target", Enum.PowerType.ComboPoints)).toBe(6)
    mock.player:update({ powers = { COMBO_POINTS = { current = 4 } } })
    expect(UnitPower("player", Enum.PowerType.ComboPoints)).toBe(4)
end
"#,
            ),
        ],
    );
    let (ok, out) = test(dir.path(), &[]);
    assert!(ok, "{out}");
    assert!(out.contains("1 passed, 0 failed"), "{out}");
}

/// A world profile's units as NPCs (decision T530): a vendor talked to
/// opens its window, its gossip's option opens its goods, a flight master
/// has a way to fly.
#[test]
fn world_profile_npcs() {
    let profile = r#"{
  "items": { "159": { "name": "Refreshing Spring Water", "quality": 1 } },
  "units": [
    { "name": "Innkeeper", "reaction": "friendly", "distance": 3, "target": true,
      "vendor": { "items": [ { "id": 159, "price": 25, "stack": 5 } ], "repairs": true } },
    { "name": "Guard", "reaction": "friendly", "distance": 4, "focus": true,
      "vendor": { "items": [ { "id": 159 } ] },
      "gossip": { "text": "Hail.", "options": [ { "name": "Your goods?", "opens": "vendor" } ] } },
    { "name": "Gryphon Master", "reaction": "friendly", "distance": 5, "service": "flightmaster" }
  ]
}"#;
    let dir = project(
        r#""camelot": {}"#,
        &[
            ("src/main.wowlua", "\n"),
            ("tests/profiles/town.json", profile),
            (
                "tests/npcs.test.wowlua",
                r#"import test, expect, config from "wowlua:test"
import * as mock from "wowlua:mock"
config({ profile = "town" })

@test("a vendor opens its window when talked to")
local function vendor()
    local npc = mock.unitOf("target")
    if npc then
        mock.player:interact(npc)
    end
    mock.advanceFrames(60, 1000 / 60)
    expect(GetMerchantNumItems()).toBe(1)
    expect(CanMerchantRepair()).toBe(true)
end

@test("the guard's gossip opens its goods")
local function guard()
    local npc = mock.unitOf("focus")
    if npc then
        mock.player:interact(npc)
    end
    mock.advanceFrames(60, 1000 / 60)
    expect(C_GossipInfo.GetText()).toBe("Hail.")
    expect(#C_GossipInfo.GetOptions()).toBe(1)
end
"#,
            ),
        ],
    );
    let (ok, out) = test(dir.path(), &[]);
    assert!(ok, "{out}");
    assert!(out.contains("2 passed, 0 failed"), "{out}");
}

/// A built-in world profile (decision T533): a package without one of
/// that name names wowlua's (`northshire`), its check finds it there, and
/// a test starts in it: the fresh character with its money, beside test
/// vendors, trainers and services.
#[test]
fn the_built_in_northshire_profile() {
    let dir = project(
        r#""camelot": {}"#,
        &[
            ("src/main.wowlua", "\n"),
            (
                "tests/npcs.test.wowlua",
                r#"import test, expect, config from "wowlua:test"
import * as mock from "wowlua:mock"
config({ profile = "northshire" })

@test("northshire's cloth vendor sells its gear and repairs")
local function cloth()
    expect(GetMoney()).toBe(500000)
    expect(UnitClass("player")).toBe("Warrior")
    local npc = mock.unitAt(1)
    mock.player:target(npc)
    expect(UnitName("target")).toBe("Clara")
    local y, x = UnitPosition("target")
    mock.moveTo({ world = { y = y - 1.5, x = x, z = 0, instanceID = 0 }, facing = 0 })
    if npc then
        mock.player:interact(npc)
    end
    mock.advanceFrames(60, 1000 / 60)
    expect(GetMerchantNumItems()).toBe(18)
    expect(CanMerchantRepair()).toBe(true)
end

@test("northshire's warrior trainer offers its training")
local function trainer()
    local npc = mock.unitAt(9)
    mock.player:target(npc)
    expect(UnitName("target")).toBe("Garrick")
    local y, x = UnitPosition("target")
    mock.moveTo({ world = { y = y - 1.5, x = x, z = 0, instanceID = 0 }, facing = 0 })
    if npc then
        mock.player:interact(npc)
    end
    mock.advanceFrames(60, 1000 / 60)
    expect(C_GossipInfo.GetOptions()[1].name).toBe("I would like to train.")
end
"#,
            ),
        ],
    );
    let out = wowlua(dir.path()).arg("check").output().unwrap();
    let text =
        String::from_utf8_lossy(&out.stderr).into_owned() + &String::from_utf8_lossy(&out.stdout);
    assert!(out.status.success() && !text.contains("E4402"), "{text}");
    let (ok, out) = test(dir.path(), &[]);
    assert!(ok, "{out}");
    assert!(out.contains("2 passed, 0 failed"), "{out}");
}

/// World profiles (decision T213): the character, gear, bags, money,
/// place, the units around, CVars, bindings and saved variables, loaded
/// before the addon; changes fire the client's events.
#[test]
fn world_profiles() {
    let profile = r#"{
  "character": { "name": "Thrall", "realm": "Realm", "class": "SHAMAN", "race": "Orc", "level": 60, "health": 3000, "maxHealth": 3000 },
  "money": 123456,
  "items": { "19019": { "name": "Thunderfury, Blessed Blade of the Windseeker", "quality": 5, "icon": 135349 }, "159": { "name": "Refreshing Spring Water", "quality": 1 } },
  "equipment": { "MainHandSlot": 19019 },
  "bags": { "0": { "slots": 16, "items": { "1": { "id": 159, "count": 5 } } } },
  "position": { "mapID": 1453, "zone": "Stormwind City", "subZone": "Trade District", "x": 0.5, "y": 0.6 },
  "units": [ { "name": "Hogger", "level": 11, "reaction": "hostile", "distance": 12, "target": true, "nameplate": true } ],
  "cvars": { "nameplateShowEnemies": "1" },
  "trained": [403, 529],
  "savedVariables": { "DemoDB": { "logins": 41 } }
}"#;
    let dir = project(
        r#""standard": {}"#,
        &[
            (
                "src/main.wowlua",
                r#"local f = CreateFrame("Frame")
f:RegisterEvent("PLAYER_LOGIN")
f:SetScript("OnEvent", function()
    DemoDB.logins = DemoDB.logins + 1
    print(UnitName("player"), GetMoney(), GetZoneText(), (UnitName("target")))
end)
"#,
            ),
            ("tests/profiles/stormwind.json", profile),
            (
                "tests/world.test.wowlua",
                r#"import test, expect, config from "wowlua:test"
import * as mock from "wowlua:mock"
config({ profile = "stormwind" })

@test("the profile is the world")
local function world()
    expect(DemoDB.logins).toBe(42)
    expect(mock.printed()).toContain("Thrall 123456 Stormwind City Hogger")
    expect(UnitClass("player")).toBe("Shaman")
    expect(GetInventoryItemID("player", 16)).toBe(19019)
    expect(GetInventoryItemLink("player", 16)).toContain("[Thunderfury, Blessed Blade of the Windseeker]")
    expect(C_Container.GetContainerItemInfo(0, 1).stackCount).toBe(5)
    expect(C_Container.GetContainerNumFreeSlots(0)).toBe(15)
    expect(GetSubZoneText()).toBe("Trade District")
    expect(C_Map.GetBestMapForUnit("player")).toBe(1453)
    expect(CheckInteractDistance("target", 2)).toBe(false)
    expect(CheckInteractDistance("target", 1)).toBe(true)
    expect(C_CVar.GetCVar("nameplateShowEnemies")).toBe("1")
    -- Its nameplate came with the world, after the login.
    expect(UnitName("nameplate1")).toBe("Hogger")
    -- Trained: known; others not.
    expect(C_SpellBook.IsSpellKnown(403)).toBe(true)
    expect(IsPlayerSpell(529)).toBe(true)
    expect(C_SpellBook.IsSpellKnown(8042)).toBe(false)
end

@test("spells learned and forgotten")
local function learned()
    local seen: string[] = {}
    local f = CreateFrame("Frame")
    for _, e in ipairs({ "LEARNED_SPELL_IN_SKILL_LINE", "SPELLS_CHANGED" }) do
        f:RegisterEvent(e)
    end
    f:SetScript("OnEvent", function(_: Frame, e: string) seen[#seen + 1] = e end)
    mock.player:learn(8042)
    -- Known already: nothing told.
    mock.player:learn({ 8042, 403 })
    expect(C_SpellBook.IsSpellKnown(8042)).toBe(true)
    mock.player:unlearn(403)
    expect(C_SpellBook.IsSpellKnown(403)).toBe(false)
    expect(seen).toEqual({ "LEARNED_SPELL_IN_SKILL_LINE", "SPELLS_CHANGED", "SPELLS_CHANGED" })
end

@test("changes fire events")
local function changes()
    local seen: string[] = {}
    local f = CreateFrame("Frame")
    for _, e in ipairs({ "PLAYER_MONEY", "PLAYER_EQUIPMENT_CHANGED", "BAG_UPDATE", "ZONE_CHANGED", "ZONE_CHANGED_NEW_AREA" }) do
        f:RegisterEvent(e)
    end
    f:SetScript("OnEvent", function(_: Frame, e: string) seen[#seen + 1] = e end)
    mock.player:setMoney(10)
    mock.player:equip(16, nil)
    mock.setBag(0, { slots = 16, items = {} })
    mock.moveTo({ subZone = "Old Town" })
    mock.moveTo({ zone = "Elwynn Forest", mapID = 1429 })
    expect(seen).toEqual({ "PLAYER_MONEY", "PLAYER_EQUIPMENT_CHANGED", "BAG_UPDATE", "BAG_UPDATE", "ZONE_CHANGED", "ZONE_CHANGED_NEW_AREA" })
    expect(GetMoney()).toBe(10)
    expect(GetInventoryItemID("player", 16)).toBeNil()
end
"#,
            ),
        ],
    );
    let (ok, out) = test(dir.path(), &[]);
    assert!(ok, "{out}");
    assert!(out.contains("3 passed, 0 failed"), "{out}");
}

/// Widget matchers (decision T217) and custom matchers (T220), typed.
#[test]
fn widget_and_custom_matchers() {
    let dir = project(
        r#""standard": {}"#,
        &[
            (
                "tests/matchers.wowlua",
                r#"import matcher from "wowlua:test"

export const toBeEven = matcher("toBeEven", function(n: number, by: number?): (boolean, string)
    return n % (by ?? 2) == 0, `expected {n} to be even`
end)
"#,
            ),
            (
                "tests/ui.test.wowlua",
                r#"import test, expect from "wowlua:test"
import toBeEven from "./matchers"

@test("frames")
local function frames()
    local parent = CreateFrame("Frame", nil, UIParent)
    parent:SetSize(200, 100)
    parent:SetPoint("CENTER")
    local child = CreateFrame("Frame", nil, parent)
    child:SetPoint("TOPLEFT", parent, "TOPLEFT", 4, -4)
    local label = parent:CreateFontString(nil, "OVERLAY")
    label:SetFont("Fonts\\FRIZQT__.TTF", 12, "")
    label:SetText("Hello")
    expect(parent).toBeShown()
    expect(parent).toBeVisible()
    expect(parent).toHaveSize(200, 100)
    expect(parent).toHaveText("Hello")
    expect(label).toHaveText("Hello")
    expect(child).toBeAnchoredTo(parent, "TOPLEFT")
    expect(child).never.toBeAnchoredTo(UIParent)
    parent:Hide()
    expect(child).toBeShown()
    expect(child).never.toBeVisible()
end

@test("custom")
local function customMatcher()
    expect(4).toBeEven()
    expect(9).toBeEven(3)
    expect(3).never.toBeEven()
end

@test("a custom failure")
local function customFailure()
    expect(3).toBeEven()
end
"#,
            ),
        ],
    );
    let (ok, out) = test(dir.path(), &[]);
    assert!(!ok, "{out}");
    assert!(out.contains("2 passed, 1 failed"), "{out}");
    assert!(out.contains("expected 3 to be even"), "{out}");
    // Typed: the value against the matcher's first parameter, the
    // arguments against the rest.
    fs::write(
        dir.path().join("tests/ui.test.wowlua"),
        r#"import test, expect from "wowlua:test"
import toBeEven from "./matchers"

@test
local function typed()
    expect("x").toBeEven()
    expect(4).toBeEven("two")
    expect(4).toBeShown()
end
"#,
    )
    .unwrap();
    let (ok, out) = test(dir.path(), &[]);
    assert!(!ok, "{out}");
    assert!(
        out.contains("`toBeEven` takes `number`; the value is `\"x\"`"),
        "{out}"
    );
    assert!(
        out.contains("`\"two\"` is not assignable to `number?`"),
        "{out}"
    );
    assert!(
        out.contains("`toBeShown` needs a widget; the value is `4`"),
        "{out}"
    );
    assert!(!out.contains("unused import"), "{out}");
}

/// Wildcards in expected values (decision T261): `anything` matches any
/// value but nil, `ofType(kind)` any value of that type, in
/// `toHaveBeenCalledWith` and `toEqual`, and messages name them.
#[test]
fn wildcards_match_any_value() {
    let dir = project(
        r#""standard": {}"#,
        &[(
            "tests/wild.test.wowlua",
            r#"import test, expect, anything, ofType from "wowlua:test"
import * as mock from "wowlua:mock"

@test("matched")
local function matched()
    local seen = mock.spy(function(...: any) end)
    seen(1, CreateFrame("Frame"))
    expect(seen).toHaveBeenCalledWith(1, anything)
    expect(seen).toHaveBeenCalledWith(1, ofType("table"))
    expect(seen).never.toHaveBeenCalledWith(1, ofType("string"))
    expect(seen).never.toHaveBeenCalledWith(anything, nil)
    expect({ id = 7, at = GetTime() }).toEqual({ id = 7, at = ofType("number") })
    expect({ id = 7 }).never.toEqual({ id = 7, at = anything })
end

@test("a failure names the wildcard")
local function named()
    local seen = mock.spy(function(...: any) end)
    seen(nil)
    expect(seen).toHaveBeenCalledWith(anything)
end
"#,
        )],
    );
    let (ok, out) = test(dir.path(), &[]);
    assert!(!ok, "{out}");
    assert!(out.contains("1 passed, 1 failed"), "{out}");
    assert!(
        out.contains("expected a call with (anything); its calls: (nil)"),
        "{out}"
    );
}

/// Snapshots (decision T218): written when missing, compared after, shown
/// as a diff when they differ, rewritten with `-u`.
#[test]
fn snapshots() {
    let file = |kills: u32| {
        format!(
            r#"import test, expect from "wowlua:test"

@test("state")
local function state()
    expect({{ kills = {kills}, name = "x", list = {{ 1, 2 }} }}).toMatchSnapshot()
end

@test("frame")
local function frame()
    local f = CreateFrame("Frame", "SnapFrame", UIParent)
    f:SetSize(100, 50)
    f:SetPoint("TOPLEFT", 10, -20)
    local t = f:CreateFontString(nil, "OVERLAY")
    t:SetFont("Fonts\\FRIZQT__.TTF", 12, "")
    t:SetText("Hi")
    expect(f).toMatchSnapshot("layout")
end
"#
        )
    };
    let dir = project(r#""standard": {}"#, &[("tests/snap.test.wowlua", &file(1))]);
    let (ok, out) = test(dir.path(), &[]);
    assert!(ok, "{out}");
    assert!(out.contains("snapshots: 2 written"), "{out}");
    let snap =
        fs::read_to_string(dir.path().join("tests/__snapshots__/snap.test.wowlua.snap")).unwrap();
    for line in [
        "# state › 1",
        "| {",
        "|   kills = 1,",
        "|   name = \"x\",",
        "# frame › layout",
        "| Frame SnapFrame",
        "|   size 100 x 50",
        "|   TOPLEFT on UIParent TOPLEFT (10, -20)",
        "|   FontString <FontString>",
        "|     text \"Hi\"",
    ] {
        assert!(snap.contains(line), "missing {line:?} in:\n{snap}");
    }
    // The same again: they match.
    let (ok, out) = test(dir.path(), &[]);
    assert!(ok && !out.contains("snapshots:"), "{out}");
    // A change fails with a diff; -u takes it.
    fs::write(dir.path().join("tests/snap.test.wowlua"), file(2)).unwrap();
    let (ok, out) = test(dir.path(), &[]);
    assert!(!ok, "{out}");
    assert!(out.contains("- stored, + now"), "{out}");
    assert!(
        out.contains("-   kills = 1,") && out.contains("+   kills = 2,"),
        "{out}"
    );
    let (ok, out) = test(dir.path(), &["-u"]);
    assert!(ok, "{out}");
    assert!(out.contains("snapshots: 1 updated"), "{out}");
    // A snapshot no test matches goes with -u.
    fs::write(
        dir.path().join("tests/snap.test.wowlua"),
        file(2).replace("toMatchSnapshot(\"layout\")", "toBeShown()"),
    )
    .unwrap();
    let (ok, out) = test(dir.path(), &["-u"]);
    assert!(ok, "{out}");
    assert!(out.contains("snapshots: 1 removed"), "{out}");
}

/// Compile-time tests (decision T219): `@expectError` passes when the
/// function gives exactly these diagnostics, which are dropped; otherwise
/// E8122.
#[test]
fn compile_time_tests() {
    let body = |codes: &str| {
        format!(
            r#"import test, expect, expectError from "wowlua:test"

local function record(n: number): number
    return n
end

@expectError({codes})
local function wrongArgument()
    record("two")
end

@test
local function runs()
    expect(record(2)).toBe(2)
end
"#
        )
    };
    let dir = project(
        r#""standard": {}"#,
        &[("tests/types.test.wowlua", &body("\"E2001\""))],
    );
    let (ok, out) = test(dir.path(), &[]);
    assert!(ok, "{out}");
    assert!(out.contains("✓ wrongArgument"), "{out}");
    assert!(out.contains("2 passed, 0 failed"), "{out}");
    fs::write(
        dir.path().join("tests/types.test.wowlua"),
        body("\"E2005\""),
    )
    .unwrap();
    let (ok, out) = test(dir.path(), &[]);
    assert!(!ok, "{out}");
    assert!(
        out.contains("`wrongArgument` should not compile with E2005; it gives E2001"),
        "{out}"
    );
}

/// `mock.partial` gives a fake record typed by where it goes, so a stub
/// can return it (decision S42).
#[test]
fn mock_partial_record() {
    let dir = project(
        r#""standard": {}"#,
        &[
            ("src/main.wowlua", "print(1)\n"),
            (
                "tests/partial.test.wowlua",
                r#"import test, expect from "wowlua:test"
import * as mock from "wowlua:mock"

@test
local function partial()
    local info: BNetAccountInfo = mock.partial({ bnetAccountID = 7, accountName = "|Kq1|k" })
    expect(info.bnetAccountID).toBe(7)
    local n: number = info.bnetAccountID
    expect(n).toBe(7)
end
"#,
            ),
        ],
    );
    let (ok, out) = test(dir.path(), &[]);
    assert!(ok, "{out}");
    assert!(out.contains("1 passed, 0 failed"), "{out}");
}

/// A `mock.partial` fake passes the runtime check of a function taking the
/// record it stands for; a table that only claims to be one still fails it.
#[test]
fn mock_partial_passes_runtime_checks() {
    let dir = project(
        r#""standard": {}"#,
        &[
            (
                "src/main.wowlua",
                r#"export type Entry = { kills: number, name: string }
export function total(e: Entry): number
    return e.kills
end
"#,
            ),
            (
                "tests/partial.test.wowlua",
                r#"import test, expect from "wowlua:test"
import * as mock from "wowlua:mock"
import total from "/main"

@test
local function partial()
    expect(total(mock.partial({ kills = 3 }))).toBe(3)
    local claimed: any = { kills = 3 }
    expect(function()
        total(claimed)
    end).toThrow("bad argument #1 'e' to 'total'")
end
"#,
            ),
        ],
    );
    let (ok, out) = test(dir.path(), &[]);
    assert!(ok, "{out}");
    assert!(out.contains("1 passed, 0 failed"), "{out}");
}

/// `import internal` (decision T223): a module's file-level locals and
/// types in test files only (E8119 elsewhere); the addon build is
/// unchanged.
#[test]
fn import_internal() {
    let tracker = r#"
type Entry = { kills: number }

local function parse(text: string): number
    return tonumber(text) or 0
end

const LIMIT = 10

local cache: { [string]: Entry } = {}

export function record(name: string, text: string)
    cache[name] = { kills = math.min(parse(text), LIMIT) }
end

export function limit(): number return LIMIT end
"#;
    let dir = project(
        r#""standard": {}"#,
        &[
            ("src/tracker.wowlua", tracker),
            (
                "tests/tracker.test.wowlua",
                r#"import test, expect from "wowlua:test"
import record from "/tracker"
import internal parse, LIMIT, cache, Entry from "/tracker"

@test
local function internals()
    expect(parse("12")).toBe(12)
    expect(LIMIT).toBe(10)
    record("hogger", "99")
    local e: Entry = cache["hogger"]
    expect(e.kills).toBe(10)
end
"#,
            ),
        ],
    );
    let (ok, out) = test(dir.path(), &[]);
    assert!(ok, "{out}");
    assert!(out.contains("1 passed, 0 failed"), "{out}");
    // Typed: the local's type.
    fs::write(
        dir.path().join("tests/tracker.test.wowlua"),
        "import internal parse from \"/tracker\"\nlocal s: string = parse(\"1\")\nprint(s)\n",
    )
    .unwrap();
    let (ok, out) = test(dir.path(), &[]);
    assert!(!ok, "{out}");
    assert!(
        out.contains("`number` is not assignable to `string`"),
        "{out}"
    );
    // An export read through it keeps its type (decision L66).
    fs::write(
        dir.path().join("tests/tracker.test.wowlua"),
        "import internal limit from \"/tracker\"\nlocal s: string = limit()\nprint(s)\n",
    )
    .unwrap();
    let (ok, out) = test(dir.path(), &[]);
    assert!(!ok, "{out}");
    assert!(
        out.contains("`number` is not assignable to `string`"),
        "{out}"
    );
    // Not in the addon's own files.
    fs::write(
        dir.path().join("tests/tracker.test.wowlua"),
        "import test from \"wowlua:test\"\n@test\nlocal function t() end\n",
    )
    .unwrap();
    fs::write(
        dir.path().join("src/main.wowlua"),
        "import internal parse from \"./tracker\"\nprint(parse(\"1\"))\n",
    )
    .unwrap();
    let out = wowlua(dir.path()).arg("check").output().unwrap();
    let text = String::from_utf8_lossy(&out.stderr);
    assert!(text.contains("E8119"), "{text}");
    // The addon build has no internals.
    fs::remove_file(dir.path().join("src/main.wowlua")).unwrap();
    wowlua(dir.path()).arg("build").assert().success();
    let built = fs::read_to_string(dir.path().join("dist/dev/Demo/src/tracker.lua")).unwrap();
    assert!(!built.contains("#internal"), "{built}");
}

/// `mock.module` (decision T224): the module's exports replaced for every
/// importer, the rest kept; replacements checked against the exports.
#[test]
fn mock_module() {
    let dir = project(
        r#""standard": {}"#,
        &[
            (
                "src/clock.wowlua",
                "export function now(): number\n    return 1\nend\nexport function zone(): string\n    return \"here\"\nend\n",
            ),
            (
                "src/report.wowlua",
                "import now, zone from \"./clock\"\nexport function line(): string\n    return zone() .. \" at \" .. now()\nend\n",
            ),
            (
                "tests/report.test.wowlua",
                r#"import test, expect from "wowlua:test"
import * as mock from "wowlua:mock"
import * as clock from "/clock"
import line from "/report"

@test("the real one")
local function real()
    expect(line()).toBe("here at 1")
end

@test("replaced")
local function replaced()
    mock.module(clock, { now = function(): number return 42 end })
    expect(line()).toBe("here at 42")
end
"#,
            ),
        ],
    );
    let (ok, out) = test(dir.path(), &[]);
    assert!(ok, "{out}");
    assert!(out.contains("2 passed, 0 failed"), "{out}");
    fs::write(
        dir.path().join("tests/report.test.wowlua"),
        r#"import * as mock from "wowlua:mock"
import * as clock from "/clock"
mock.module(clock, { now = function(): string return "x" end, later = 1 })
"#,
    )
    .unwrap();
    let (ok, out) = test(dir.path(), &[]);
    assert!(!ok, "{out}");
    assert!(out.contains("the module has no export `later`"), "{out}");
    assert!(out.contains("is not assignable to `() -> number`"), "{out}");
}

/// The group finder's other listings (`mock.defineLFGListing`): an addon's
/// search from a click finds them; their changes and delistings come as
/// `LFG_LIST_SEARCH_RESULT_UPDATED` for the last search's results; a failed
/// search as `LFG_LIST_SEARCH_FAILED`.
#[test]
fn group_finder_listings() {
    let dir = project(
        r#""camelot": {}"#,
        &[
            (
                "src/main.wowlua",
                r#"-- Searches the dungeons' listings from its button; tells what it hears.
export const heard: string[] = {}
export const button = CreateFrame("Button", "DemoSearchButton", UIParent)
button:SetSize(100, 20)
button:SetPoint("CENTER")
button:SetScript("OnClick", function()
    C_LFGList.Search(2, 0, 0, nil, false, nil, { 799 })
end)
local f = CreateFrame("Frame")
f:RegisterEvent("LFG_LIST_SEARCH_RESULTS_RECEIVED")
f:RegisterEvent("LFG_LIST_SEARCH_RESULT_UPDATED")
f:RegisterEvent("LFG_LIST_SEARCH_FAILED")
f:SetScript("OnEvent", function(_: Frame, event: string, arg: any)
    if event == "LFG_LIST_SEARCH_RESULTS_RECEIVED" then
        local _, results = C_LFGList.GetSearchResults()
        for _, id in ipairs(results) do
            local info = C_LFGList.GetSearchResultInfo(id)
            heard[#heard + 1] = `{info.leaderName}: {info.comment}`
        end
    elseif event == "LFG_LIST_SEARCH_RESULT_UPDATED" then
        local info = C_LFGList.GetSearchResultInfo(arg)
        heard[#heard + 1] = `{info.leaderName} {info.isDelisted and "delisted" or info.comment}`
    else
        heard[#heard + 1] = `failed: {arg}`
    end
end)
"#,
            ),
            (
                "tests/lfg.test.wowlua",
                r#"import test, expect from "wowlua:test"
import * as mock from "wowlua:mock"
import heard, button from "/main"

@test("other groups' listings")
local function listings()
    local group = mock.defineLFGListing({ activities = { 799 }, comment = "LF healer", members = {
        { name = "Tamsin Ashford", class = "WARRIOR", level = 18, role = "TANK" },
        { name = "Otherplayer", class = "MAGE", level = 17, role = "DAMAGER" },
    } })
    expect(mock.lfgSearchResultID(group)).toBe(nil)
    mock.click(button)
    mock.waitSeconds(1)
    expect(heard).toEqual({ "Tamsin Ashford: LF healer" })
    expect(mock.lfgSearchResultID(group)).toBe(1)
    expect(mock.updateLFGListing(group, { comment = "LF tank" })).toBe(true)
    expect(mock.removeLFGListing(group)).toBe(true)
    expect(heard).toEqual({ "Tamsin Ashford: LF healer", "Tamsin Ashford LF tank", "Tamsin Ashford delisted" })
    mock.failLFGSearch("throttled")
    mock.click(button)
    mock.waitSeconds(1)
    expect(heard[4]).toBe("failed: throttled")
    expect(mock.lfgSearchResultID(group)).toBe(nil)
end
"#,
            ),
        ],
    );
    let (ok, out) = test(dir.path(), &[]);
    assert!(ok, "{out}");
    assert!(out.contains("1 passed, 0 failed"), "{out}");
}

/// `test.addons`: other addon folders load before the addon, each with
/// its name, its own addon table, its SavedVariables and its
/// `ADDON_LOADED` (the addon's last); an addon's dependencies first; the
/// flavor's TOC; a load-on-demand one with `C_AddOns.LoadAddOn`; one for
/// another game left out. The addon itself is known as its TOC says. Their
/// files are named as the client names them (`Interface/AddOns/<Name>/`).
#[test]
fn other_addons() {
    const RECORD: &str = "local name, ns = ...\nns.who = name\nOrder = Order or {}\ntable.insert(Order, name)\nNS = NS or {}\nNS[name] = ns\nStacks = Stacks or {}\nStacks[name] = debugstack(1, 1, 0)\n";
    let dir = project(
        r#""standard": {}"#,
        &[
            (
                "src/main.wowlua",
                "local name: string = ...\nconst G = _G :: any\nG.Order = G.Order ?? {}\ntable.insert(G.Order, name)\n",
            ),
            (
                "addons/Lazy/Lazy.toc",
                "## Title: Lazy One\n## LoadOnDemand: 1\n## Dependencies: Base\nlazy.lua\n",
            ),
            ("addons/Lazy/lazy.lua", RECORD),
            (
                "addons/Child/Child.toc",
                "## Title: Not this one\nwrong.lua\n",
            ),
            (
                "addons/Child/Child_Mainline.toc",
                "## Title: Child\n## X-Answer: 42\n## Dependencies: Base\n## SavedVariables: ChildDB\nchild.lua\n",
            ),
            (
                "addons/Child/wrong.lua",
                "error('the plain TOC was read')\n",
            ),
            ("addons/Child/child.lua", RECORD),
            ("addons/Base/Base.toc", "## Title: Base\nbase.lua\n"),
            ("addons/Base/base.lua", RECORD),
            (
                "addons/Elsewhere/Elsewhere.toc",
                "## AllowLoadGameType: vanilla\nelsewhere.lua\n",
            ),
            (
                "addons/Elsewhere/elsewhere.lua",
                "error('loaded for another game')\n",
            ),
            (
                "tests/others.test.wowlua",
                r#"import test, expect from "wowlua:test"
import * as mock from "wowlua:mock"

const G = _G :: any
local told: string[] = {}
local f = CreateFrame("Frame")
f:RegisterEvent("ADDON_LOADED")
f:SetScript("OnEvent", function(_, _, name: string)
    table.insert(told, name)
end)

@test("each loads first, as its own addon")
local function each()
    expect(G.Order).toEqual({ "Base", "Child", "Demo" })
    expect(told).toEqual({ "Base", "Child", "Demo" })
    expect(G.NS.Base.who).toBe("Base")
    expect(G.NS.Child.who).toBe("Child")
    expect(G.NS.Base == G.NS.Child).toBe(false)
    expect(C_AddOns.GetNumAddOns()).toBe(4)
    expect(C_AddOns.GetAddOnMetadata("Child", "X-Answer")).toBe("42")
    expect(C_AddOns.GetAddOnMetadata("Demo", "Title")).toBe("Demo")
    expect(C_AddOns.GetAddOnMetadata("Demo", "Version")).toBe("1.0.0-dev")
    expect(C_AddOns.IsAddOnLoaded("Demo")).toBe(true)
    expect(C_AddOns.LoadAddOn("Demo")).toBe(true)
    expect(G.Order).toEqual({ "Base", "Child", "Demo" })
    -- Not for this game: missing, as an unknown name (the full probe)
    expect(select(5, C_AddOns.GetAddOnInfo("Elsewhere"))).toBe("MISSING")
    expect(C_AddOns.IsAddOnLoaded("Child")).toBe(true)
    expect(C_AddOns.IsAddOnLoaded("Lazy")).toBe(false)
    expect(C_AddOns.IsAddOnLoadOnDemand("Lazy")).toBe(true)
    expect(select(5, C_AddOns.GetAddOnInfo("Lazy"))).toBe("DEMAND_LOADED")
    -- An addon folder's files have IDs, files it lacks and absent addons none
    expect(GetFileIDFromPath("Interface\\AddOns\\Base\\base.lua") ~= nil).toBe(true)
    expect(GetFileIDFromPath("Interface/AddOns/BASE/Base") ~= nil).toBe(true)
    expect(GetFileIDFromPath("Interface\\AddOns\\Base\\missing.tga")).toBe(nil)
    expect(GetFileIDFromPath("Interface\\AddOns\\Atlas\\Images\\Map")).toBe(nil)
    expect(GetFileIDFromPath("Interface\\AddOns\\Demo\\media\\logo.tga") ~= nil).toBe(true)
    expect(C_AddOns.LoadAddOn("Lazy")).toBe(true)
    expect(select(5, C_AddOns.GetAddOnInfo("Lazy"))).toBe("")
    expect(G.Order).toEqual({ "Base", "Child", "Demo", "Lazy" })
    expect(told).toEqual({ "Base", "Child", "Demo", "Lazy" })
    expect(G.NS.Lazy.who).toBe("Lazy")
    expect(C_AddOns.IsAddOnLoaded("Lazy")).toBe(true)
    expect(string.find(G.Stacks.Base, "[Interface/AddOns/Base/base.lua]:", 1, true)).toBe(1)
    expect(string.find(G.Stacks.Lazy, "[Interface/AddOns/Lazy/lazy.lua]:", 1, true)).toBe(1)
end

@test("their SavedVariables survive a reload")
local function saved()
    if mock.reloads() == 0 then
        G.ChildDB = { n = 7 }
        mock.reloadUI()
    end
    expect(G.ChildDB.n).toBe(7)
    expect(G.Order).toEqual({ "Base", "Child", "Demo" })
end
"#,
            ),
        ],
    );
    let manifest = fs::read_to_string(dir.path().join("wowlua.json")).unwrap();
    let manifest = manifest.replacen(
        "{ \"package\"",
        r#"{ "test": { "addons": ["addons/Lazy", "addons/Child", "addons/Base", "addons/Elsewhere"] }, "package""#,
        1,
    );
    fs::write(dir.path().join("wowlua.json"), manifest).unwrap();
    let (ok, out) = test(dir.path(), &[]);
    assert!(ok, "{out}");
    assert!(out.contains("2 passed, 0 failed"), "{out}");
}

/// The addon's own files, the test files and the runtime are named as the
/// client names them too (`Interface/AddOns/Demo/src/main.lua`): stacks
/// and error messages carry those names, and the failures show the source
/// paths, also for a name Lua shortened with `...` at the front.
#[test]
fn chunk_names_as_the_client() {
    let dir = project(
        r#""standard": {}"#,
        &[
            (
                "src/main.wowlua",
                "const G = _G :: any\nG.MainStack = debugstack(1, 1, 0)\nG.Fail = function() error(\"from src\") end\n",
            ),
            (
                "tests/names.test.wowlua",
                r#"import test, expect from "wowlua:test"

const G = _G :: any

@test("as in the client")
local function named()
    expect(G.MainStack).toBe("[Interface/AddOns/Demo/src/main.lua]:2: in main chunk\n")
    expect(select(2, pcall(G.Fail))).toBe("Interface/AddOns/Demo/src/main.lua:3: from src")
    expect(string.find(debugstack(1, 1, 0), "[Interface/AddOns/Demo/tests/names.test.lua]:9:", 1, true)).toBe(1)
    expect(select(2, pcall(error, "here", 1))).toBe("here")
end

@test("an addon file's failure")
local function fails()
    G.Fail()
end
"#,
            ),
            (
                "tests/some/deeply/nested/a_rather_long_file_name.test.wowlua",
                r#"import test, expect from "wowlua:test"

@test("a shortened name's failure")
local function shortened()
    expect((select(2, pcall(function() error("x") end)))).toBe(".../some/deeply/nested/a_rather_long_file_name.test.lua:5: x")
    error("boom")
end
"#,
            ),
        ],
    );
    let (ok, out) = test(dir.path(), &[]);
    assert!(!ok, "{out}");
    assert!(out.contains("1 passed, 2 failed"), "{out}");
    assert!(
        out.contains("from src\n       at src/main.wowlua:3"),
        "{out}"
    );
    assert!(
        out.contains(
            "boom\n       at tests/some/deeply/nested/a_rather_long_file_name.test.wowlua:6"
        ),
        "{out}"
    );
    assert!(!out.contains("Interface/AddOns"), "{out}");
}

/// `mock.reloadUI()`: the test runs again in a fresh VM with the
/// SavedVariables as the client writes them (`/stdtest sv`), logged in as
/// a reload; the file's next test runs where the file was.
#[test]
fn reload_ui() {
    let dir = project(
        r#""camelot": {}"#,
        &[
            ("src/main.wowlua", "_G.Loaded = (_G.Loaded or 0) + 1\n"),
            (
                "tests/reload.test.wowlua",
                r#"import test, expect, afterEach from "wowlua:test"
import * as mock from "wowlua:mock"

local entering: any[] = {}
local f = CreateFrame("Frame")
f:RegisterEvent("PLAYER_ENTERING_WORLD")
f:SetScript("OnEvent", function(_, _, initial: boolean, reloading: boolean)
    entering = { initial, reloading }
end)
local teardowns = 0

@afterEach
local function count()
    teardowns = teardowns + 1
end

@test("the SavedVariables survive a reload")
local function survives()
    const db = _G :: any
    if mock.reloads() == 0 then
        expect(db.DemoDB).toBeNil()
        expect(entering).toEqual({ true, false })
        local shared = { tag = "shared" }
        db.DemoDB = {
            count = 1,
            third = 1 / 3,
            seventeen = 0.12345678901234567,
            fn = print,
            frame = CreateFrame("Frame"),
            nested = { deep = true },
            keys = { [1.5] = "float", [true] = "true", [-1] = "negative" },
            a = shared,
            b = shared,
            time = GetTime(),
        }
        mock.reloadUI()
        error("not reached")
    end
    expect(mock.reloads()).toBe(1)
    expect(db.Loaded).toBe(1)
    expect(entering).toEqual({ false, true })
    expect(db.DemoDB.count).toBe(1)
    expect(db.DemoDB.third).toBe(1 / 3)
    expect(db.DemoDB.seventeen).toBe(0.1234567890123457)
    expect(db.DemoDB.fn).toBeNil()
    expect(db.DemoDB.frame[0]).toBeNil()
    expect(db.DemoDB.nested.deep).toBe(true)
    expect(db.DemoDB.keys).toEqual({ [1.5] = "float", [true] = "true", [-1] = "negative" })
    expect(db.DemoDB.a == db.DemoDB.b).toBe(false)
    expect(db.DemoDB.a.tag).toBe("shared")
    expect(GetTime() >= db.DemoDB.time).toBe(true)
end

@test("twice")
local function twice()
    const db = _G :: any
    db.DemoDB.passes = db.DemoDB.passes ?? {}
    table.insert(db.DemoDB.passes, mock.reloads())
    if mock.reloads() < 2 then
        mock.reloadUI()
    end
    expect(db.DemoDB.passes).toEqual({ 0, 1, 2 })
end

@test("the next test runs where the file was")
local function after()
    expect(mock.reloads()).toBe(0)
    expect((_G :: any).DemoDB.count).toBe(1)
    -- Each test's first pass ran its teardown in this VM.
    expect(teardowns).toBe(2)
end
"#,
            ),
        ],
    );
    let (ok, out) = test(dir.path(), &[]);
    assert!(ok, "{out}");
    assert!(out.contains("3 passed, 0 failed"), "{out}");
}

/// The addon's own `ReloadUI()` (Questie's `/questie tracker hide`) ends
/// the session there: `PLAYER_LOGOUT`, then nothing of it runs (its
/// timers, its `OnUpdate`); the test's pass ends at its next wait and runs
/// again after the reload, as at `mock.reloadUI()`.
#[test]
fn the_addons_reload_ui() {
    let dir = project(
        r#""camelot": {}"#,
        &[
            (
                "src/main.wowlua",
                r#"local f = CreateFrame("Frame")
f:RegisterEvent("PLAYER_LOGOUT")
f:SetScript("OnEvent", function()
    _G.DemoDB = _G.DemoDB or {}
    _G.DemoDB.loggedOut = (_G.DemoDB.loggedOut or 0) + 1
end)
f:SetScript("OnUpdate", function() _G.Updates = (_G.Updates or 0) + 1 end)
SLASH_DEMORELOAD1 = "/demoreload"
SlashCmdList.DEMORELOAD = function() C_UI.Reload() end
"#,
            ),
            (
                "tests/reload.test.wowlua",
                r#"import test, expect from "wowlua:test"
import * as mock from "wowlua:mock"

@test("the addon reloads the UI")
local function reloads()
    const g = _G :: any
    if mock.reloads() == 0 then
        mock.waitFrames(2)
        mock.slash("/demoreload")
        local updates = g.Updates
        mock.advanceFrames(10)
        expect(g.Updates).toBe(updates)
        mock.waitFrames(1)
        error("not reached")
    end
    expect(mock.reloads()).toBe(1)
    expect(g.DemoDB.loggedOut).toBe(1)
end

@test("the next test runs where the file was")
local function after()
    expect(mock.reloads()).toBe(0)
    local updates = (_G :: any).Updates
    mock.waitFrames(1)
    expect((_G :: any).Updates).toBe(updates + 1)
end
"#,
            ),
        ],
    );
    let (ok, out) = test(dir.path(), &[]);
    assert!(ok, "{out}");
    assert!(out.contains("2 passed, 0 failed"), "{out}");
}

/// GUI tests act as the user: hover, click, type and keys.
#[test]
fn gui_input() {
    let dir = project(
        r#""standard": {}"#,
        &[(
            "tests/gui.test.wowlua",
            r#"import test, expect from "wowlua:test"
import * as mock from "wowlua:mock"

@test("hover and click")
local function hoverClick()
    local seen: string[] = {}
    local b = CreateFrame("Button", "GuiButton", UIParent)
    b:SetSize(80, 20)
    b:SetPoint("CENTER")
    for _, s in ipairs({ "OnEnter", "OnLeave", "OnMouseDown", "OnMouseUp", "PreClick", "OnClick", "PostClick" }) do
        b:SetScript(s, function() seen[#seen + 1] = s end)
    end
    mock.hover(b)
    expect(b:IsMouseOver()).toBe(true)
    expect(GetMouseFoci()[1]).toBe(b)
    mock.click(b)
    mock.hover(nil)
    expect(seen).toEqual({ "OnEnter", "OnMouseDown", "OnMouseUp", "PreClick", "OnClick", "PostClick", "OnLeave" })
    -- The mouse comes back to click it again.
    b:Disable()
    seen = {}
    mock.click(b)
    expect(seen).toEqual({ "OnEnter", "OnMouseDown", "OnMouseUp" })
    b:Hide()
    expect(function() mock.click(b) end).toThrow("GuiButton is not visible")
end

@test("type and press enter")
local function typing()
    local box = CreateFrame("EditBox", nil, UIParent)
    local changes = 0
    local entered: (string | Secret<string>)? = nil
    box:SetScript("OnTextChanged", function(_: EditBox, user: boolean) if user then changes = changes + 1 end end)
    box:SetScript("OnEnterPressed", function(self: EditBox) entered = self:GetText() end)
    mock.typeText(box, "hello")
    expect(box:HasFocus()).toBe(true)
    expect(changes).toBe(5)
    mock.pressKey("ENTER")
    expect(entered).toBe("hello")
end
"#,
        )],
    );
    let (ok, out) = test(dir.path(), &[]);
    assert!(ok, "{out}");
    assert!(out.contains("2 passed, 0 failed"), "{out}");
}

/// A multi-line `return f(…)` runs to its closing line: the `RETURN` Lua
/// puts after the tail call never runs and is not counted (decision T260).
#[test]
fn coverage_counts_a_multi_line_tail_call_as_run() {
    let dir = project(
        r#""standard": {}"#,
        &[
            (
                "src/wrap.wowlua",
                r#"local function make(t: { [string]: number }): number
    return t.a
end

export function wrapped(): number
    return make({
        a = 1,
    })
end
"#,
            ),
            (
                "tests/wrap.test.wowlua",
                r#"import test, expect from "wowlua:test"
import wrapped from "/wrap"

@test
local function wraps()
    expect(wrapped()).toBe(1)
end
"#,
            ),
        ],
    );
    let (ok, out) = test(dir.path(), &["--coverage", "--reporter", "pretty,lcov"]);
    assert!(ok, "{out}");
    let lcov = fs::read_to_string(dir.path().join("dist/test/lcov.info")).unwrap();
    assert!(lcov.contains("DA:6,1"), "{lcov}");
    assert!(!lcov.contains("DA:8,0"), "{lcov}");
    assert!(out.contains("coverage: lines 100.0% (6/6)"), "{out}");
}

/// The HTML coverage report in a workspace: titled by the root package and
/// with its sources, not the last member's (two addons, the root first).
#[test]
fn coverage_html_in_a_workspace() {
    let dir = tempfile::tempdir().unwrap();
    for (path, text) in [
        (
            "wowlua.json",
            r#"{ "package": { "name": "main", "version": "1.0.0" }, "addon": { "title": "Main" },
  "workspace": { "members": ["plugins/*"] } }"#,
        ),
        (
            "src/main.wowlua",
            "export function twice(n: number): number\n    return n * 2\nend\n",
        ),
        (
            "tests/main.test.wowlua",
            "import test, expect from \"wowlua:test\"\nimport twice from \"/main\"\n\n@test\nlocal function doubles()\n    expect(twice(2)).toBe(4)\nend\n",
        ),
        (
            "plugins/Zed/wowlua.json",
            r#"{ "package": { "name": "zed", "version": "1.0.0" }, "addon": { "title": "Zed" } }"#,
        ),
        ("plugins/Zed/src/zed.wowlua", "print(\"zed\")\n"),
    ] {
        let path = dir.path().join(path);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, text).unwrap();
    }
    let (ok, out) = test(dir.path(), &["--coverage", "--reporter", "html"]);
    assert!(ok, "{out}");
    let html = fs::read_to_string(dir.path().join("dist/test/coverage.html")).unwrap();
    assert!(html.contains("<title>Main coverage</title>"), "{html}");
    assert!(
        html.contains("return n * 2"),
        "the root package's source is missing"
    );
}

/// Coverage (decision T188): counted by the VM in the addon's own files,
/// with reports and thresholds.
#[test]
fn coverage_reports_and_thresholds() {
    let dir = project(
        r#""standard": {}"#,
        &[
            (
                "src/math.wowlua",
                r#"export function sign(n: number): number
    if n > 0 then
        return 1
    elseif n < 0 then
        return -1
    end
    return 0
end

export function unused(): number
    return 42
end

-- wowlua: coverage ignore
local function debugOnly() print("never") end
"#,
            ),
            (
                "tests/math.test.wowlua",
                r#"import test, expect from "wowlua:test"
import sign from "/math"

@test
local function positive()
    expect(sign(5)).toBe(1)
end
"#,
            ),
        ],
    );
    let (ok, out) = test(
        dir.path(),
        &[
            "--coverage",
            "--reporter",
            "pretty,lcov,cobertura,coverage-json,html,junit,tap,json",
        ],
    );
    assert!(ok, "{out}");
    assert!(out.contains("coverage: lines"), "{out}");
    assert!(out.contains("src/math.wowlua: lines"), "{out}");
    let report = dir.path().join("dist/test");
    let lcov = fs::read_to_string(report.join("lcov.info")).unwrap();
    for line in [
        "SF:src/math.wowlua",
        "FN:1,sign#0",
        "FNDA:1,sign#0",
        "FNDA:0,unused#0",
        "DA:11,0",
        "BRDA:2,0,",
    ] {
        assert!(lcov.contains(line), "missing {line:?} in:\n{lcov}");
    }
    // The ignored function is not counted.
    assert!(!lcov.contains("debugOnly"), "{lcov}");
    assert!(!lcov.contains("SF:tests/"), "{lcov}");
    let html = fs::read_to_string(report.join("coverage.html")).unwrap();
    assert!(html.contains("<title>Demo coverage</title>") && html.contains("prefers-color-scheme"));
    // the page draws its views from the data: every file's counts, its
    // functions by name and its source, under wowlua's mark
    for part in [
        "\"p\":\"src/math.wowlua\"",
        "\"unused\"",
        "export function unused(): number",
        "Where to test next",
        "class=\"brand\"",
        "rel=\"icon\"",
    ] {
        assert!(html.contains(part), "missing {part:?} in the HTML report");
    }
    // no `<` inside the data: a source's `</script>` can't end the script
    let data = html.split("id=\"data\">").nth(1).unwrap();
    let data = data.split("</script>").next().unwrap();
    assert!(!data.contains('<'), "{data}");
    assert!(
        fs::read_to_string(report.join("cobertura.xml"))
            .unwrap()
            .contains("filename=\"src/math.wowlua\"")
    );
    assert!(
        fs::read_to_string(report.join("coverage.json"))
            .unwrap()
            .contains("\"functions\"")
    );
    assert!(
        fs::read_to_string(report.join("junit.xml"))
            .unwrap()
            .contains("<testcase name=\"positive\"")
    );
    assert!(
        fs::read_to_string(report.join("results.tap"))
            .unwrap()
            .contains("ok 1 - tests/math.test.wowlua (standard) › positive")
    );
    assert!(
        fs::read_to_string(report.join("results.json"))
            .unwrap()
            .contains("\"status\": \"passed\"")
    );
    // A minimum the run does not reach fails it.
    let manifest = dir.path().join("wowlua.json");
    let text = fs::read_to_string(&manifest).unwrap();
    fs::write(
        &manifest,
        text.replacen('{', r#"{ "test": { "coverage": { "functions": 90 } },"#, 1),
    )
    .unwrap();
    let (ok, out) = test(dir.path(), &["--coverage"]);
    assert!(!ok, "{out}");
    assert!(
        out.contains("functions coverage 50.0% is below the minimum 90%"),
        "{out}"
    );
    // A run with a filter checks part of the suite: no minimum applies.
    let (ok, out) = test(dir.path(), &["--coverage", "math"]);
    assert!(ok, "{out}");
    assert!(!out.contains("is below the minimum"), "{out}");
}

/// GitHub annotations for failures.
#[test]
fn github_reporter() {
    let dir = project(
        r#""standard": {}"#,
        &[(
            "tests/a.test.wowlua",
            "import test, expect from \"wowlua:test\"\n@test(\"bad\")\nlocal function bad()\n    expect(1).toBe(2)\nend\n",
        )],
    );
    let (ok, out) = test(dir.path(), &["--reporter", "github"]);
    assert!(!ok, "{out}");
    assert!(
        out.contains(
            "::error file=tests/a.test.wowlua,line=4,title=bad (standard)::expected 1 to be 2"
        ),
        "{out}"
    );
}

/// Taint in tests (spec Tooling §16.1.4): a protected action refused
/// because of taint fails the test with its taint chain unless the test
/// expects it (`toBeBlocked`); `toBeSecure` tells untainted variables.
#[test]
fn taint_in_tests() {
    let dir = project(
        r#""standard": {}"#,
        &[(
            "tests/taint.test.wowlua",
            r#"import test, expect from "wowlua:test"
import * as mock from "wowlua:mock"

local function protectedFrame(name: string): Frame
    local f = CreateFrame("Button", name, UIParent, "SecureActionButtonTemplate")
    mock.player:enterCombat()
    return f
end

@test("expected")
local function expected()
    local f = protectedFrame("ProtA")
    expect(function() f:Hide() end).toBeBlocked()
    mock.player:leaveCombat()
end

@test("unexpected")
local function unexpected()
    TargetUnit("player")
end

@test("variables")
local function variables()
    expect("UIParent").toBeSecure()
    _G.MyAddonGlobal = 1
    expect("MyAddonGlobal").never.toBeSecure()
end
"#,
        )],
    );
    let (ok, out) = test(dir.path(), &[]);
    assert!(!ok, "{out}");
    assert!(out.contains("✓ expected"), "{out}");
    assert!(out.contains("✓ variables"), "{out}");
    assert!(out.contains("✗ unexpected"), "{out}");
    assert!(out.contains("because of taint"), "{out}");
    assert!(out.contains("2 passed, 1 failed"), "{out}");
}

/// Secret values in tests (spec Tooling §16.1.3): tests run tainted, so
/// the framework never reads a secret. What needs no value passes or fails
/// as it should; secrets show as `<secret>` in names, messages and
/// snapshots.
#[test]
fn secret_values_in_matchers_spies_snapshots_and_waits() {
    let dir = project(
        r#""standard": {}"#,
        &[(
            "tests/secret.test.wowlua",
            r#"import test, expect, cases, describe from "wowlua:test"
import * as mock from "wowlua:mock"

local n: any = mock.secret(5)
local s: any = mock.secret("Hogger")

@test("matchers that need no value")
local function noValue()
    expect(n).toBeSecret()
    expect(5).never.toBeSecret()
    expect(n).never.toBeNil()
    expect(s).toBeTruthy()
    expect(n).never.toBe("x")
    expect({ hp = n }).never.toEqual({ hp = "x" })
    expect({ n, 1 }).toContain(1)
    expect(function() error(s) end).toThrow()
end

@test("spies record secrets")
local function spies()
    local spy = mock.spy()
    spy(n, 1)
    expect(spy).toHaveBeenCalledTimes(1)
    expect(spy).never.toHaveBeenCalledWith(n, 2)
end

@test("snapshots")
local function snapshots()
    local f = CreateFrame("Frame", "SecretFrame", UIParent)
    f:SetSize(10, 10)
    local label = f:CreateFontString(nil, "OVERLAY")
    label:SetFont("Fonts\\FRIZQT__.TTF", 12, "")
    label:SetText(s)
    -- A table whose security keeps addons out is not looked into either.
    local guarded = { 1 }
    local client: any = _G
    client.settablesecurity(guarded, 0)
    expect({ hp = n, frame = f, guarded = guarded }).toMatchSnapshot()
end

@test("waits")
local function waits()
    C_Timer.After(0.1, function() mock.fireEvent("UNIT_HEALTH", mock.secret("target")) end)
    expect(mock.waitFor("UNIT_HEALTH")).toBeSecret()
    expect(mock.waitUntil(function() return s end)).toBeSecret()
end

@test("cases")
@cases({ { mock.secret(1), 1 }, mock.secret({ 2 }) })
local function secretCases(a: any, b: any)
    expect(a).toBeSecret()
end

describe(s, function()
    test("inside")(function() expect(1).toBe(1) end)
end)
"#,
        )],
    );
    let (ok, out) = test(dir.path(), &[]);
    assert!(ok, "{out}");
    for line in [
        "✓ matchers that need no value",
        "✓ spies record secrets",
        "✓ cases [<secret>, 1]",
        "✓ cases <secret>",
        "✓ <secret> › inside",
        "7 passed, 0 failed",
    ] {
        assert!(out.contains(line), "missing {line:?} in:\n{out}");
    }
    let snap = fs::read_to_string(
        dir.path()
            .join("tests/__snapshots__/secret.test.wowlua.snap"),
    )
    .unwrap();
    for line in [
        "|   hp = <secret>,",
        "|     text <secret>",
        "|   guarded = <inaccessible table>,",
    ] {
        assert!(snap.contains(line), "missing {line:?} in:\n{snap}");
    }
    // What needs a secret's value fails, negated or not, and says so.
    fs::write(
        dir.path().join("tests/secret.test.wowlua"),
        r#"import test, expect, matcher from "wowlua:test"
import * as mock from "wowlua:mock"

local n: any = mock.secret(5)
local s: any = mock.secret("Hogger")
local b: any = mock.secret(true)

const toBeFive = matcher("toBeFive", function(v: any): (boolean, string) return v, "not five" end)

@test local function toBe() expect(n).toBe(5) end
@test local function notToBe() expect(n).never.toBe(5) end
@test local function truthy() expect(b).toBeTruthy() end
@test local function equal() expect({ hp = n }).toEqual({ hp = 5 }) end
@test local function contain() expect(s).toContain("Hog") end
@test local function length() expect({ 1 }).toHaveLength(n) end
@test local function called()
    local spy = mock.spy()
    spy(n)
    expect(spy).toHaveBeenCalledWith(5)
end
@test local function text()
    local label = UIParent:CreateFontString(nil, "OVERLAY")
    label:SetFont("Fonts\\FRIZQT__.TTF", 12, "")
    label:SetText(s)
    expect(label).toHaveText("Hogger")
end
@test local function raised() error(s) end
@test local function throws() expect(function() error(s) end).toThrow("Hog") end
@test local function custom() expect(b).toBeFive() end
@test local function waitsUntil() mock.waitUntil(function() return b end) end
@test local function waitsFor()
    C_Timer.After(0.1, function() mock.player:setMoney(5) end)
    mock.waitFor("PLAYER_MONEY", function() return b end)
end
"#,
    )
    .unwrap();
    let (ok, out) = test(dir.path(), &[]);
    assert!(!ok, "{out}");
    assert!(out.contains("0 passed, 13 failed"), "{out}");
    let tell = ": cannot tell with a secret value (tests run tainted, like the addon)";
    for line in [
        format!("expected <secret> to be 5{tell}"),
        format!("expected <secret> not to be 5{tell}"),
        format!("expected <secret> to be truthy{tell}"),
        format!("expected {{ hp = <secret> }} to equal {{ hp = 5 }}{tell}"),
        "toContain: received a secret value, which it cannot read".into(),
        "toHaveLength: an argument is a secret value, which it cannot read".into(),
        format!("expected a call with (5); its calls: (<secret>){tell}"),
        format!("expected the text \"Hogger\", got <secret>{tell}"),
        "the test raised a secret value as its error".into(),
        format!("expected an error containing \"Hog\"; it raised <secret>{tell}"),
        "the matcher toBeFive answered with a secret value".into(),
        "mock.waitUntil: the condition is a secret boolean".into(),
        "mock.waitFor: the predicate for PLAYER_MONEY returned a secret boolean".into(),
        "at tests/secret.test.wowlua:10".into(),
    ] {
        assert!(out.contains(&line), "missing {line:?} in:\n{out}");
    }
    // The framework itself never errors on a secret.
    assert!(!out.contains("attempt to"), "{out}");
}

/// Casting, modifier keys, load-on-demand addons and libraries.
#[test]
fn casting_modifiers_addons_libraries() {
    let dir = project(
        r#""standard": {}"#,
        &[(
            "tests/world.test.wowlua",
            r#"import test, expect from "wowlua:test"
import * as mock from "wowlua:mock"

@test("a cast runs and ends")
local function cast()
    mock.defineSpell(116, { name = "Frostbolt", iconID = 135846, castTime = 2000 })
    local seen: string[] = {}
    local f = CreateFrame("Frame")
    for _, e in ipairs({ "UNIT_SPELLCAST_START", "UNIT_SPELLCAST_SUCCEEDED", "UNIT_SPELLCAST_STOP", "UNIT_SPELLCAST_INTERRUPTED" }) do
        f:RegisterUnitEvent(e, "player")
    end
    f:SetScript("OnEvent", function(_: Frame, e: string) seen[#seen + 1] = e end)
    mock.player:castSpell(116)
    local name, _, icon, startMS, endMS = UnitCastingInfo("player")
    expect(name).toBe("Frostbolt")
    expect(icon).toBe(135846)
    expect(endMS - startMS).toBe(2000)
    mock.advanceTime(2)
    expect(UnitCastingInfo("player")).toBeNil()
    mock.player:castSpell(116)
    mock.player:interruptCast()
    expect(seen).toEqual({
        "UNIT_SPELLCAST_START", "UNIT_SPELLCAST_SUCCEEDED", "UNIT_SPELLCAST_STOP",
        "UNIT_SPELLCAST_START", "UNIT_SPELLCAST_INTERRUPTED", "UNIT_SPELLCAST_STOP",
    })
end

@test("modifiers")
local function modifiers()
    expect(IsShiftKeyDown()).toBe(false)
    mock.setModifiers({ shift = true })
    expect(IsShiftKeyDown()).toBe(true)
    expect(IsModifierKeyDown()).toBe(true)
    mock.setModifiers({})
    expect(IsModifierKeyDown()).toBe(false)
end

@test("addons and libraries")
local function addonsAndLibraries()
    local loaded = false
    mock.defineAddon("Extra", { loadOnDemand = true, load = function() loaded = true end })
    mock.loadAddon("Extra")
    expect(loaded).toBe(true)
    mock.provideLibrary("LibThing-1.0", { answer = 42 })
    local lib: any = LibStub("LibThing-1.0")
    expect(lib.answer).toBe(42)
end
"#,
        )],
    );
    let (ok, out) = test(dir.path(), &[]);
    assert!(ok, "{out}");
    assert!(out.contains("3 passed, 0 failed"), "{out}");
}

/// The examples' tests: the addons with Blizzard's UI (skipped without
/// the UI code of `cargo xtask types fetch`), on every flavor they list
/// (their tests are camelot's alone, `flavor-files`).
#[test]
fn example_tests() {
    example("core-demo", "8 passed, 0 failed");
    example("widget-demo", "3 passed, 0 failed");
}

/// The typed-demo example's tests: modules, world profiles, compile-time
/// tests, every flavor of a shared-mode package (no Blizzard UI needed).
#[test]
fn typed_demo_example() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/typed-demo");
    let (ok, out) = test(&root, &[]);
    assert!(ok, "{out}");
    assert!(out.contains("36 passed, 0 failed"), "{out}");
}

fn example(name: &str, summary: &str) {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let source = root.join("target/types-src");
    if !source.join("camelot/Interface").exists() {
        eprintln!("skipped: no Blizzard UI code (run `cargo xtask types fetch`)");
        return;
    }
    let out = wowlua(&root.join("examples").join(name))
        .arg("test")
        .env("WOWLUA_UI_SOURCE", &source)
        .output()
        .unwrap();
    let text = format!(
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(out.status.success(), "{name}: {text}");
    assert!(text.contains(summary), "{name}: {text}");
}

/// A world profile's bag item nothing has data for loads with Blizzard's
/// UI without an error: its bag slot has no item info, as in the client
/// while the item's data has not arrived (its reagent bag tutorial reads a
/// slot's link).
#[test]
fn blizzard_ui_with_a_bag_item_without_data() {
    let source = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/types-src");
    if !source.join("camelot/Interface").exists() {
        eprintln!("skipped: no Blizzard UI code (run `cargo xtask types fetch`)");
        return;
    }
    let dir = project(
        r#""camelot": {}"#,
        &[
            ("src/main.wowlua", "print(\"loaded\")\n"),
            (
                "tests/profiles/bags.json",
                r#"{ "bags": { "0": { "slots": 16, "items": { "1": { "id": 159, "count": 5 } } } } }"#,
            ),
            (
                "tests/bags.test.wowlua",
                r#"import test, expect from "wowlua:test"

@test("the slot has no info")
local function slot()
    expect(C_Container.GetContainerItemID(0, 1)).toBe(159)
    expect(C_Container.GetContainerItemInfo(0, 1)).toBeNil()
end
"#,
            ),
        ],
    );
    let manifest = dir.path().join("wowlua.json");
    let text = fs::read_to_string(&manifest).unwrap();
    fs::write(
        &manifest,
        text.replacen(
            '{',
            r#"{ "test": { "blizzard-ui": true, "profile": "bags" },"#,
            1,
        ),
    )
    .unwrap();
    let out = wowlua(dir.path())
        .arg("test")
        .env("WOWLUA_UI_SOURCE", &source)
        .output()
        .unwrap();
    let text = format!(
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(out.status.success(), "{text}");
    assert!(text.contains("1 passed, 0 failed"), "{text}");
}

/// Blizzard's UI through the world's changes, without an error (it runs
/// without one in the client): casts, money, gear, bags, auras in and out
/// of combat (their secret updates), a target's death.
#[test]
fn blizzard_ui_through_world_changes() {
    let source = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/types-src");
    if !source.join("camelot/Interface").exists() {
        eprintln!("skipped: no Blizzard UI code (run `cargo xtask types fetch`)");
        return;
    }
    let dir = project(
        r#""camelot": {}"#,
        &[
            ("src/main.wowlua", "print(\"loaded\")\n"),
            (
                "tests/world.test.wowlua",
                r#"import test, expect from "wowlua:test"
import * as mock from "wowlua:mock"

@test("the world changes")
local function world()
    mock.nextFrame(60)
    local p = mock.player :: any
    p:castSpell(647, { castTime = 1 })
    p:castSpell(647, { castTime = 1 })
    mock.advanceTime(2)
    p:castSpell(19750, { castTime = 0 })
    p:castSpell(5143, { castTime = 1, channel = true })
    p:interruptCast()
    p:setMoney(40000)
    p:equip(16, { id = 6948 })
    p:equip(16, nil)
    mock.setBag(0, { slots = 16, items = { [1] = { id = 6948 } } })
    mock.setBag(2, { slots = 6, items = {} })
    mock.setBag(2, nil)
    local id = p:applyAura({ spellId = 20217, name = "Blessing of Kings", duration = 300 })
    mock.nextFrame(5)
    p:removeAura(id)
    local hogger = mock.spawnUnit({ name = "Hogger", level = 11, reaction = "hostile", health = 666 })
    p:target(hogger)
    p:enterCombat()
    mock.nextFrame(30)
    local id2 = p:applyAura({ spellId = 20217, name = "Blessing of Kings", duration = 300 })
    hogger:applyAura({ spellId = 772, name = "Rend", duration = 15, harmful = true, source = "player" })
    mock.nextFrame(5)
    p:removeAura(id2)
    hogger:castSpell(133, { castTime = 1 })
    mock.advanceTime(0.5)
    hogger:kill({ by = mock.player, lootable = true })
    mock.nextFrame(5)
    p:leaveCombat()
    mock.nextFrame(30)
    expect(UnitExists("target")).toBe(false)
    -- Soft targets.
    local wolf = mock.spawnUnit({ name = "Swoop", level = 4, reaction = "hostile", health = 50 })
    local friend = mock.spawnUnit({ name = "Harn Longcast", level = 30, reaction = "friendly" })
    p:softTarget(wolf)
    p:softTarget(friend)
    p:softTarget(friend, "interact")
    mock.nextFrame(10)
    p:softTarget(nil, "enemy")
    p:softTarget(nil, "friend")
    p:softTarget(nil, "interact")
    -- A vendor: the merchant frame opens, buys, sells and buys back.
    mock.defineItem(6256, { name = "Fishing Pole", icon = 132932, quality = 1 })
    mock.defineItem(6529, { name = "Shiny Bauble", icon = 134335, quality = 1, sellPrice = 12, stackCount = 20 })
    mock.openMerchant({ name = "Harn Longcast", items = { { id = 6256, price = 23 }, { id = 6529, price = 50 } } })
    mock.nextFrame(10)
    expect(MerchantFrame).toBeShown()
    BuyMerchantItem(2, 1)
    mock.waitSeconds(0.5)
    mock.sell(0, 2)
    mock.waitSeconds(0.5)
    BuybackItem(1)
    mock.waitSeconds(0.5)
    mock.closeMerchant()
    mock.nextFrame(10)
    expect(MerchantFrame).never.toBeShown()
end
"#,
            ),
        ],
    );
    let manifest = dir.path().join("wowlua.json");
    let text = fs::read_to_string(&manifest).unwrap();
    fs::write(
        &manifest,
        text.replacen('{', r#"{ "test": { "blizzard-ui": true },"#, 1),
    )
    .unwrap();
    let out = wowlua(dir.path())
        .arg("test")
        .env("WOWLUA_UI_SOURCE", &source)
        .output()
        .unwrap();
    let text = format!(
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(out.status.success(), "{text}");
    assert!(text.contains("1 passed, 0 failed"), "{text}");
}

/// A test file run with Blizzard's UI loaded: its output, after asserting
/// it passed (`summary`).
fn blizzard_ui_test(file: &str, source_text: &str, summary: &str) {
    let source = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/types-src");
    if !source.join("camelot/Interface").exists() {
        eprintln!("skipped: no Blizzard UI code (run `cargo xtask types fetch`)");
        return;
    }
    let dir = project(
        r#""camelot": {}"#,
        &[
            ("src/main.wowlua", "print(\"loaded\")\n"),
            (file, source_text),
        ],
    );
    let manifest = dir.path().join("wowlua.json");
    let text = fs::read_to_string(&manifest).unwrap();
    fs::write(
        &manifest,
        text.replacen('{', r#"{ "test": { "blizzard-ui": true },"#, 1),
    )
    .unwrap();
    let out = wowlua(dir.path())
        .arg("test")
        .env("WOWLUA_UI_SOURCE", &source)
        .output()
        .unwrap();
    let text = format!(
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(out.status.success(), "{text}");
    assert!(text.contains(summary), "{text}");
}

/// A group's fight's helpers: a member of a class (a player character at
/// the player's level, in the group), buffs from one on all, debuffs from
/// several on each mob. (No recording: a synthetic fight; what the engine
/// does is what its modules model, unverified beyond them.)
const GROUP_FIGHT: &str = r#"import test, expect from "wowlua:test"
import * as mock from "wowlua:mock"

local function member(name: string, class: string, classID: number, powerType: number, raid: boolean?): any
    local u = mock.spawnUnit({
        name = name, class = class, classID = classID, level = 20, reaction = "friendly", player = true,
        health = 900, maxHealth = 1000, powerType = powerType, power = 500, maxPower = 1000,
    })
    mock.group:add(u, raid)
    return u
end

local function mobs(n: number): any[]
    local out = {}
    for i = 1, n do
        out[i] = mock.spawnUnit({ name = `Defias Thug {i}`, level = 19, reaction = "hostile", health = 800, maxHealth = 800 })
        out[i]:showNameplate()
        mock.nextFrame(2)
        -- Blizzard's nameplate for it.
        local plate = C_NamePlate.GetNamePlateForUnit(`nameplate{i}`) :: any
        expect(plate.UnitFrame.unit).toBe(`nameplate{i}`)
        expect(plate.UnitFrame.name:GetText()).toBe(`Defias Thug {i}`)
    end
    expect(#C_NamePlate.GetNamePlates()).toBe(n)
    return out
end

local function auras(unit: string, filter: string): number
    local n = 0
    while C_UnitAuras.GetAuraDataByIndex(unit, n + 1, filter) do n = n + 1 end
    return n
end

--- The whole group's fight: buffs from a caster on everyone, debuffs from
--- each member on every mob, casts, hits and heals, a member's death,
--- target switches among the group and the mobs.
local function fight(p: any, group: any[], enemies: any[])
    local everyone = { p }
    for _, u in ipairs(group) do everyone[#everyone + 1] = u end
    local buffs = {
        { spellId = 1243, name = "Power Word: Fortitude", duration = 1800 },
        { spellId = 1459, name = "Arcane Intellect", duration = 1800 },
        { spellId = 1126, name = "Mark of the Wild", duration = 1800 },
        { spellId = 19740, name = "Blessing of Might", duration = 300 },
        { spellId = 139, name = "Renew", duration = 15 },
    }
    for i, buff in ipairs(buffs) do
        local caster = group[((i - 1) % #group) + 1]
        for _, u in ipairs(everyone) do
            u:applyAura({ spellId = buff.spellId, name = buff.name, duration = buff.duration, source = caster })
        end
        mock.nextFrame(2)
    end
    local debuffs = {
        { spellId = 772, name = "Rend", duration = 15 },
        { spellId = 589, name = "Shadow Word: Pain", duration = 18, dispelName = "Magic" },
        { spellId = 172, name = "Corruption", duration = 18, dispelName = "Magic" },
        { spellId = 8921, name = "Moonfire", duration = 12, dispelName = "Magic" },
        { spellId = 1978, name = "Serpent Sting", duration = 15, dispelName = "Poison" },
        { spellId = 7386, name = "Sunder Armor", duration = 30, applications = 3 },
    }
    expect(auras("player", "HELPFUL")).toBe(#buffs)
    for i = 1, #group do
        local token = IsInRaid() and `raid{i + 1}` or `party{i}`
        expect(auras(token, "HELPFUL")).toBe(#buffs)
    end
    p:target(enemies[1])
    mock.nextFrame(10)
    local targetFrame = (_G :: any).TargetFrame
    expect(targetFrame:IsShown()).toBe(true)
    expect(UnitName("target")).toBe("Defias Thug 1")
    for i, u in ipairs(group) do
        local d = debuffs[((i - 1) % #debuffs) + 1]
        for _, mob in ipairs(enemies) do
            mob:applyAura({ spellId = d.spellId, name = d.name, duration = d.duration, harmful = true,
                source = u, dispelName = d.dispelName, applications = d.applications })
        end
        u:castSpell(116, { castTime = 0.5 })
        mock.advanceTime(0.2)
        mock.nextFrame(3)
    end
    -- (Tainted code reads no aura in combat: counted before the pull.)
    for i = 1, #enemies do
        expect(auras(`nameplate{i}`, "HARMFUL")).toBe(#group)
    end
    p:enterCombat()
    mock.nextFrame(10)
    -- A debuff on a member, cured.
    local curse = group[1]:applyAura({ spellId = 18266, name = "Curse of Agony", duration = 24, harmful = true,
        dispelName = "Curse", source = enemies[2] })
    mock.nextFrame(5)
    group[1]:removeAura(curse)
    -- Hits and heals.
    for round = 1, 3 do
        for _, mob in ipairs(enemies) do mob:damage(120) end
        for _, u in ipairs(everyone) do u:damage(60 * round) end
        mock.nextFrame(5)
        for _, u in ipairs(everyone) do u:heal(80) end
        mock.nextFrame(5)
    end
    -- Target switches: each member, each mob, a member's target.
    for _, u in ipairs(group) do
        p:target(u)
        mock.nextFrame(2)
    end
    for _, mob in ipairs(enemies) do
        p:target(mob)
        mock.nextFrame(2)
    end
    p:focus(group[1])
    mock.nextFrame(3)
    expect((_G :: any).FocusFrame:IsShown()).toBe(true)
    -- A member dies; the mobs die.
    group[#group]:kill()
    mock.nextFrame(5)
    for _, mob in ipairs(enemies) do
        mob:kill({ by = p, lootable = true })
        mock.nextFrame(3)
    end
    p:leaveCombat()
    mock.nextFrame(30)
    -- The dead member's buffs gone with it.
    local dead = IsInRaid() and `raid{#group + 1}` or `party{#group}`
    expect(auras(dead, "HELPFUL")).toBe(0)
    expect(auras("player", "HELPFUL")).toBe(#buffs)
end
"#;

/// A party's fight with Blizzard's UI, without an error: four members of
/// four classes, buffs on all, debuffs from each on three mobs, casts,
/// hits, heals, a death, target switches.
#[test]
fn blizzard_ui_through_a_party_fight() {
    let test = format!(
        "{GROUP_FIGHT}{}",
        r#"
@test("a party's fight")
local function party()
    mock.nextFrame(60)
    local p = mock.player :: any
    local group = {
        member("Arnhild", "PRIEST", 5, 0),
        member("Brenna", "MAGE", 8, 0),
        member("Corvin", "ROGUE", 4, 3),
        member("Dalla", "DRUID", 11, 0),
    }
    mock.nextFrame(30)
    expect(IsInGroup()).toBe(true)
    expect(IsInRaid()).toBe(false)
    expect(GetNumGroupMembers()).toBe(5)
    expect(UnitName("party1")).toBe("Arnhild")
    expect(UnitName("party4")).toBe("Dalla")
    -- Blizzard's party frames.
    local frames = (_G :: any).PartyFrame
    expect(frames.MemberFrame1:IsShown()).toBe(true)
    expect(frames.MemberFrame1.name:GetText()).toBe("Arnhild")
    expect(frames.MemberFrame4.name:GetText()).toBe("Dalla")
    fight(p, group, mobs(3))
    expect(UnitIsDead("party4")).toBe(true)
end
"#
    );
    blizzard_ui_test("tests/party.test.wowlua", &test, "1 passed, 0 failed");
}

/// A raid's fight with Blizzard's UI, without an error: ten members, every
/// class once (a second warrior), buffs on all, debuffs from each on four
/// mobs, casts, hits, heals, a death, target switches.
#[test]
fn blizzard_ui_through_a_raid_fight() {
    let test = format!(
        "{GROUP_FIGHT}{}",
        r#"
@test("a raid's fight")
local function raid()
    mock.nextFrame(60)
    local p = mock.player :: any
    local group = {
        member("Arnhild", "PALADIN", 2, 0, true),
        member("Brenna", "HUNTER", 3, 0, true),
        member("Corvin", "ROGUE", 4, 3, true),
        member("Dalla", "PRIEST", 5, 0, true),
        member("Eskil", "SHAMAN", 7, 0, true),
        member("Freya", "MAGE", 8, 0, true),
        member("Gunnar", "WARLOCK", 9, 0, true),
        member("Hilde", "DRUID", 11, 0, true),
        member("Ivar", "WARRIOR", 1, 1, true),
    }
    mock.nextFrame(30)
    expect(IsInRaid()).toBe(true)
    expect(GetNumGroupMembers()).toBe(10)
    expect(UnitName("raid2")).toBe("Arnhild")
    -- Blizzard's raid frames: two groups of five; the party frames hidden.
    for i, name in ipairs({ "Player", "Arnhild", "Brenna", "Corvin", "Dalla", "Eskil", "Freya", "Gunnar", "Hilde", "Ivar" }) do
        local frame = (_G :: any)[`CompactRaidGroup{math.floor((i - 1) / 5) + 1}Member{(i - 1) % 5 + 1}`]
        expect(frame:IsVisible()).toBe(true)
        expect(frame.unit).toBe(`raid{i}`)
        expect(frame.name:GetText()).toBe(name)
    end
    expect((_G :: any).PartyFrame.MemberFrame1:IsShown()).toBe(false)
    fight(p, group, mobs(4))
    expect(UnitIsDead("raid10")).toBe(true)
    local name, _, subgroup, _, _, file, _, online, dead = GetRaidRosterInfo(10)
    expect(name).toBe("Ivar")
    expect(subgroup).toBe(2)
    expect(file).toBe("WARRIOR")
    expect(online).toBe(true)
    expect(dead).toBe(true)
end
"#
    );
    blizzard_ui_test("tests/raid.test.wowlua", &test, "1 passed, 0 failed");
}

/// A dungeon's boss fight with Blizzard's UI, without an error: a party of
/// five in an instance (`IsInInstance`, `GetInstanceInfo`), the boss
/// engaged (`boss1`, Blizzard's boss frame, its cast bar), debuffs on the
/// party, hits and heals, a member's death and resurrection, the encounter
/// won (the boss frame gone), the boss's loot window, the player's death
/// and a priest's resurrection taken through Blizzard's popup. (No
/// recording: the instance, the encounter and the resurrection are
/// modelled, unverified.)
#[test]
fn blizzard_ui_through_a_dungeon_boss() {
    blizzard_ui_test(
        "tests/dungeon.test.wowlua",
        r#"import test, expect from "wowlua:test"
import * as mock from "wowlua:mock"

local function member(name: string, class: string, classID: number, powerType: number, role: "TANK" | "HEALER" | "DAMAGER"): any
    local u = mock.spawnUnit({
        name = name, class = class, classID = classID, level = 20, reaction = "friendly", player = true,
        health = 900, maxHealth = 1000, powerType = powerType, power = 500, maxPower = 1000,
    })
    mock.group:add(u)
    u:setRole(role)
    return u
end

@test("a dungeon's boss")
local function dungeon()
    mock.nextFrame(60)
    local p = mock.player :: any
    p:setRole("TANK")
    local group = {
        member("Arnhild", "PRIEST", 5, 0, "HEALER"),
        member("Brenna", "MAGE", 8, 0, "DAMAGER"),
        member("Corvin", "ROGUE", 4, 3, "DAMAGER"),
        member("Dalla", "HUNTER", 3, 0, "DAMAGER"),
    }
    mock.nextFrame(30)
    expect(GetNumGroupMembers()).toBe(5)
    -- Into the dungeon.
    mock.enterInstance("party", { name = "The Deadmines", instanceID = 36, difficultyID = 1, maxPlayers = 5 })
    mock.nextFrame(30)
    local inInstance, kind = IsInInstance()
    expect(inInstance).toBe(true)
    expect(kind).toBe("party")
    local name, instanceType, difficultyID, _, maxPlayers = GetInstanceInfo()
    expect(name).toBe("The Deadmines")
    expect(instanceType).toBe("party")
    expect(difficultyID).toBe(1)
    expect(maxPlayers).toBe(5)
    -- The boss, engaged.
    local boss = mock.spawnUnit({ name = "Edwin VanCleef", level = 21, reaction = "hostile", health = 5000,
        maxHealth = 5000, classification = "elite", powerType = 0, power = 1000, maxPower = 1000 })
    boss:showNameplate()
    mock.startEncounter({ id = 2973, name = "Edwin VanCleef", difficultyID = 1, groupSize = 5, bosses = { boss } })
    mock.nextFrame(10)
    -- (On a restricted map, the boss's name is secret to the test's code.)
    expect(UnitExists("boss1")).toBe(true)
    expect(issecretvalue(UnitName("boss1"))).toBe(true)
    expect(C_InstanceEncounter.IsEncounterInProgress()).toBe(true)
    local bossFrame = (_G :: any).Boss1TargetFrame
    expect(bossFrame).toBeShown()
    expect(bossFrame.unit).toBe("boss1")
    p:target(boss)
    p:enterCombat()
    for _, u in ipairs(group) do u:enterCombat() end
    boss:enterCombat()
    p:startAttack()
    mock.nextFrame(10)
    -- Its cast, shown on the boss frame's bar.
    boss:castSpell(5165, { castTime = 2 })
    mock.nextFrame(10)
    expect(bossFrame.spellbar).toBeShown()
    mock.waitSeconds(4)
    expect(bossFrame.spellbar).never.toBeShown()
    -- Its debuffs on the party, one cured.
    local curses = {}
    for i, u in ipairs(group) do
        curses[i] = u:applyAura({ spellId = 6136, name = "Chilled", duration = 10, harmful = true,
            dispelName = "Magic", source = boss })
    end
    mock.nextFrame(5)
    group[2]:removeAura(curses[2])
    mock.nextFrame(5)
    -- The fight: swings, hits and heals.
    for _ = 1, 3 do
        p:swing(boss, 120, { power = 10 })
        boss:swing(p, 80, { victimPower = 5 })
        for _, u in ipairs(group) do
            u:hit(boss, 200, { school = 4 })
            u:damage(150)
        end
        mock.nextFrame(10)
        for _, u in ipairs(group) do u:heal(100) end
        mock.advanceTime(1)
        mock.nextFrame(10)
    end
    -- A member's death and resurrection.
    group[3]:kill()
    mock.nextFrame(10)
    expect(UnitIsDead("party3")).toBe(true)
    group[1]:castSpell(2006, { castTime = 2 })
    mock.advanceTime(2.5)
    group[1]:resurrect(group[3], { health = 350 })
    mock.nextFrame(10)
    expect(UnitIsDead("party3")).toBe(false)
    -- The boss dies: the encounter won, the frames gone, the loot.
    mock.defineItem(5191, { name = "Cruel Barb", icon = 135321, quality = 3, classID = 2, subclassID = 7 })
    boss:kill({ by = p, loot = { { id = 5191 } } })
    mock.nextFrame(5)
    mock.endEncounter(true)
    mock.nextFrame(10)
    expect(C_InstanceEncounter.IsEncounterInProgress()).toBe(false)
    expect(UnitExists("boss1")).toBe(false)
    expect(bossFrame).never.toBeShown()
    p:leaveCombat()
    for _, u in ipairs(group) do u:leaveCombat() end
    mock.nextFrame(30)
    p:loot(boss)
    mock.waitSeconds(1)
    expect((_G :: any).LootFrame).toBeShown()
    expect(GetNumLootItems()).toBe(1)
    mock.waitSeconds(1)
    CloseLoot()
    mock.waitSeconds(1)
    expect((_G :: any).LootFrame).never.toBeShown()
    -- The player dies in the next pull and takes a resurrection.
    local add = mock.spawnUnit({ name = "Defias Pirate", level = 20, reaction = "hostile", health = 800, maxHealth = 800 })
    p:target(add)
    p:enterCombat()
    add:swing(p, 50)
    mock.nextFrame(5)
    p:die()
    mock.nextFrame(10)
    expect(UnitIsDead("player")).toBe(true)
    local G = _G :: any
    expect(G.StaticPopup_Visible("DEATH") ~= nil).toBe(true)
    -- The priest's offer: Blizzard's popup, accepted.
    group[1]:resurrect(p)
    mock.waitSeconds(0.5)
    expect(ResurrectGetOfferer()).toBe("Arnhild")
    local popup = G.StaticPopup_FindVisible("RESURRECT_NO_TIMER")
    expect(popup ~= nil).toBe(true)
    mock.waitSeconds(5)
    mock.click(popup:GetButton1())
    mock.waitSeconds(1)
    expect(UnitIsDead("player")).toBe(false)
    expect(G.StaticPopup_Visible("DEATH")).toBe(nil)
    expect(G.StaticPopup_Visible("RESURRECT_NO_TIMER")).toBe(nil)
    mock.leaveInstance()
    mock.nextFrame(30)
    expect(IsInInstance()).toBe(false)
end
"#,
        "1 passed, 0 failed",
    );
}

/// A hunter's pet with Blizzard's UI, without an error: the pet frame, its
/// happiness, the pet bar (a spell, the commands), the pet sent to attack
/// and called back, hits, a buff on it, its happiness and experience
/// changing, dismissed. (No recording: the pet is modelled, unverified.)
#[test]
fn blizzard_ui_through_a_hunters_pet() {
    blizzard_ui_test(
        "tests/hunter.test.wowlua",
        r#"import test, expect from "wowlua:test"
import * as mock from "wowlua:mock"

@test("a hunter's pet in a fight")
local function hunter()
    mock.setPlayer({ name = "Tamsin", level = 20, class = "HUNTER", race = "Dwarf", health = 500, maxHealth = 500,
        powerType = 0, power = 600, maxPower = 600 })
    mock.nextFrame(60)
    local p = mock.player :: any
    local G = _G :: any
    expect(G.PetFrame).never.toBeShown()
    expect(G.PetActionBar).never.toBeShown()
    -- The wolf tamed: its frame, its bar, its happiness.
    local wolf = mock.spawnUnit({ name = "Fang", level = 19, reaction = "friendly", health = 400, maxHealth = 400,
        powerType = 2, power = 100, maxPower = 100, creatureType = "Beast", creatureTypeID = 1,
        creatureFamily = "Wolf", creatureFamilyID = 1 })
    mock.defineSpell(17253, { name = "Bite", icon = 132278 })
    mock.setPet(wolf, { happiness = 3, loyalty = "Loyalty Level 2", xp = 300, xpMax = 1200,
        actions = { [4] = { spell = 17253, name = "Bite", icon = 132278, autocast = true } } })
    mock.nextFrame(10)
    expect(UnitExists("pet")).toBe(true)
    expect(UnitName("pet")).toBe("Fang")
    expect(UnitIsUnit("pet", "pet")).toBe(true)
    expect(G.PetFrame).toBeShown()
    expect(G.PetName:GetText()).toBe("Fang")
    expect(G.PetFrameHappiness).toBeShown()
    expect(G.PetFrameHappiness.Texture:GetAtlas()).toBe("UI-PetHappiness")
    expect(G.PetActionBar).toBeShown()
    expect(G.PetActionButton1.tooltipName).toBe(G.PET_ACTION_ATTACK)
    expect(G.PetActionButton2:GetChecked()).toBe(true)
    expect(G.PetActionButton4.tooltipName).toBe("Bite")
    -- The fight: the pet sent in, hits taken and dealt, a buff on it.
    local boar = mock.spawnUnit({ name = "Rockhide Boar", level = 18, reaction = "hostile", health = 450,
        maxHealth = 450 })
    boar:showNameplate()
    p:target(boar)
    p:enterCombat()
    wolf:enterCombat()
    mock.click(G.PetActionButton1)
    mock.nextFrame(5)
    expect(IsPetAttackActive()).toBe(true)
    expect(G.PetAttackModeTexture).toBeShown()
    for _ = 1, 3 do
        wolf:swing(boar, 40, { power = 5 })
        wolf:hit(boar, 30, { school = 1 })
        boar:swing(wolf, 25)
        p:hit(boar, 60, { school = 1 })
        mock.nextFrame(10)
        mock.advanceTime(1)
    end
    local mend = wolf:applyAura({ spellId = 136, name = "Mend Pet", duration = 15, source = p })
    mock.nextFrame(5)
    -- Unhappy now: its indicator follows.
    mock.setPet(wolf, { happiness = 1 })
    mock.nextFrame(5)
    expect(select(1, C_PetInfo.GetPetHappiness())).toBe(1)
    expect(G.PetFrameHappiness).toBeShown()
    expect(G.PetFrameHappiness.Texture:GetAtlas()).toBe("UI-PetMad")
    boar:kill({ by = p })
    mock.nextFrame(5)
    mock.click(G.PetActionButton2)
    mock.nextFrame(5)
    expect(IsPetAttackActive()).toBe(false)
    expect(G.PetAttackModeTexture).never.toBeShown()
    wolf:removeAura(mend)
    wolf:leaveCombat()
    p:leaveCombat()
    -- Its experience grows.
    mock.setPet(wolf, { xp = 900 })
    mock.nextFrame(10)
    expect(select(1, GetPetExperience())).toBe(900)
    -- Dismissed: the frame and the bar gone.
    mock.setPet(nil)
    mock.nextFrame(10)
    expect(UnitExists("pet")).toBe(false)
    expect(G.PetFrame).never.toBeShown()
    expect(G.PetActionBar).never.toBeShown()
end
"#,
        "1 passed, 0 failed",
    );
}

/// A town session with Blizzard's UI, without an error: a vendor (bought,
/// sold, bought back, repaired), the bank, the mailbox (a letter read, its
/// item taken), the auction house (a search), a class trainer and a flight
/// master's map through their gossip.
#[test]
fn blizzard_ui_through_a_town_session() {
    blizzard_ui_test(
        "tests/town.test.wowlua",
        r#"import test, expect from "wowlua:test"
import * as mock from "wowlua:mock"

-- A frame's descendant button showing `text` (a gossip option, a row).
local function button(root: any, text: string, depth: number?): any
    local d = depth or 0
    if d > 12 or not root:IsVisible() then return nil end
    for _, r in ipairs({ root:GetRegions() }) do
        if r:IsObjectType("FontString") and r:GetText() == text then return root end
    end
    for _, c in ipairs({ root:GetChildren() }) do
        local found = button(c, text, d + 1)
        if found then return found end
    end
    return nil
end

@test("a town session")
local function town()
    mock.nextFrame(60)
    local p = mock.player :: any
    local G = _G :: any
    p:setMoney(500000)
    mock.defineItem(6256, { name = "Fishing Pole", icon = 132932, quality = 1, sellPrice = 5 })
    mock.defineItem(6529, { name = "Shiny Bauble", icon = 134335, quality = 1, sellPrice = 12, stackCount = 20 })
    mock.defineItem(2770, { name = "Copper Ore", icon = 134566, quality = 1, sellPrice = 5, stackCount = 20 })
    mock.setBag(0, { slots = 16, items = { [1] = { id = 6529, count = 5 }, [2] = { id = 2770, count = 4 } } })
    mock.defineItem(2488, { name = "Gladius", icon = 135321, quality = 1, classID = 2, subclassID = 7, sellPrice = 9 })
    p:equip(16, { id = 2488 })
    p:setDurability(16, 20, 40)

    -- The vendor: bought, sold, bought back, repaired.
    mock.openMerchant({ name = "Harn Longcast", canRepair = true, repairCost = 250,
        items = { { id = 6256, price = 23 }, { id = 6529, price = 50 } } })
    mock.waitSeconds(0.5)
    expect(G.MerchantFrame).toBeShown()
    expect(G.MerchantItem1Name:GetText()).toBe("Fishing Pole")
    BuyMerchantItem(1, 1)
    mock.waitSeconds(0.5)
    mock.sell(0, 1)
    mock.waitSeconds(0.5)
    expect(C_Container.GetContainerItemID(0, 1)).toBe(nil)
    expect(GetMoney()).toBe(500000 - 23 + 60)
    BuybackItem(1)
    mock.waitSeconds(0.5)
    -- The pole bought (23 copper), the baubles sold and bought back.
    expect(GetMoney()).toBe(500000 - 23)
    expect(C_Container.GetContainerItemID(0, 1)).toBe(6529)
    expect(C_Container.GetContainerItemID(0, 3)).toBe(6256)
    expect(GetRepairAllCost()).toBe(250)
    expect(G.MerchantRepairAllButton).toBeShown()
    mock.click(G.MerchantRepairAllButton)
    mock.waitSeconds(0.5)
    expect(GetMoney()).toBe(500000 - 23 - 250)
    expect(select(1, GetInventoryItemDurability(16))).toBe(40)
    mock.closeMerchant()
    mock.waitSeconds(0.5)
    expect(G.MerchantFrame).never.toBeShown()

    -- The bank.
    local banker = mock.spawnUnit({ name = "Olstan Ironsworn", level = 45, reaction = "friendly" })
    mock.defineBank(banker, { nextTabCost = 10000, tabs = {
        { name = "Tab 1", icon = 134400, slots = 48, items = { [1] = { id = 2770, count = 6 } } },
    } })
    banker:setDistance(3)
    p:target(banker)
    p:interact(banker)
    mock.waitSeconds(1)
    expect(G.BankFrame).toBeShown()
    expect(C_Container.GetContainerItemID(6, 1)).toBe(2770)
    expect(C_Container.GetContainerNumSlots(6)).toBe(48)
    HideUIPanel(G.BankFrame)
    mock.waitSeconds(0.5)
    expect(G.BankFrame).never.toBeShown()
    p:target(nil)

    -- The mailbox: a letter read, its item taken.
    local mailbox = mock.spawnUnit({ name = "Mailbox", level = 1, reaction = "friendly" })
    mock.defineMailbox(mailbox, { newMail = true, senders = { "Eldra Hollowtree" }, letters = {
        { stationeryIcon = 134939, sender = "Eldra Hollowtree", subject = "Ore for you",
          daysLeft = 29, textCreated = true, text = "Use it well.", items = { { id = 2770, count = 2 } } },
    } })
    mailbox:setDistance(2)
    p:interact(mailbox)
    mock.waitSeconds(1)
    expect(G.MailFrame).toBeShown()
    expect(GetInboxNumItems()).toBe(1)
    mock.click(G.MailItem1Button)
    mock.waitSeconds(1)
    expect(G.OpenMailFrame).toBeShown()
    expect(G.OpenMailSubject:GetText()).toBe("Ore for you")
    expect(G.OpenMailSender.Name:GetText()).toBe("Eldra Hollowtree")
    mock.click(G.OpenMailFrame.OpenMailAttachments[1])
    mock.waitSeconds(1.5)
    -- The ore taken: four in the bag before, six now; the letter kept (its text).
    expect(C_Item.GetItemCount(2770)).toBe(6)
    expect(GetInboxNumItems()).toBe(1)
    HideUIPanel(G.MailFrame)
    mock.waitSeconds(0.5)
    expect(G.MailFrame).never.toBeShown()

    -- The auction house: a search, closed with Escape.
    local auctioneer = mock.spawnUnit({ name = "Auctioneer Fitch", level = 55, reaction = "friendly" })
    mock.defineAuctionHouse(auctioneer, {
        browse = { { itemKey = { battlePetSpeciesID = 0, itemID = 2770, itemLevel = 10, itemSuffix = 0 },
            minPrice = 30, totalQuantity = 40, containsOwnerItem = false } },
        commodities = {}, items = {}, deposit = 100, nextAuctionID = 1000,
    })
    auctioneer:setDistance(3)
    p:target(auctioneer)
    p:interact(auctioneer)
    mock.waitSeconds(1)
    local ah = G.AuctionHouseFrame
    expect(ah).toBeShown()
    mock.click(ah.SearchBar.SearchButton)
    mock.waitSeconds(1)
    expect(ah.BrowseResultsFrame.ItemList.ScrollBox:GetDataProviderSize()).toBe(1)
    mock.pressKey("ESCAPE")
    mock.waitSeconds(0.5)
    expect(ah).never.toBeShown()
    p:target(nil)

    -- The class trainer, through its gossip.
    local trainer = mock.spawnUnit({ name = "Wilhelm Fairbanks", level = 40, reaction = "friendly",
        class = "WARRIOR", classID = 1 })
    mock.defineGossip(trainer, { text = "Strength and honour.", options = {
        { name = "I require warrior training.", icon = 132058, gossipOptionID = 96654, opens = "trainer" },
    } })
    mock.defineTrainer(trainer, { class = "WARRIOR", greeting = "Ready to train?" })
    trainer:setDistance(3)
    p:target(trainer)
    p:interact(trainer)
    mock.waitSeconds(1)
    local gossip = G.GossipFrame
    expect(gossip).toBeShown()
    local option = button(gossip, "I require warrior training.")
    expect(option ~= nil).toBe(true)
    mock.click(option)
    mock.waitSeconds(1)
    local classTrainer = G.ClassTrainerFrame
    expect(classTrainer).toBeShown()
    expect(GetNumTrainerServices() > 0).toBe(true)
    HideUIPanel(classTrainer)
    mock.waitSeconds(0.5)
    expect(classTrainer).never.toBeShown()
    p:target(nil)

    -- The flight master's map.
    local flight = mock.spawnUnit({ name = "Gryth Skyfeather", level = 55, reaction = "friendly" })
    mock.defineGossip(flight, { text = "Where to?", options = {
        { name = "I need a ride.", icon = 132057, gossipOptionID = 30000, opens = "taxi" },
    } })
    mock.defineTaxi(flight, { mapID = 1415, nodes = {
        { name = "Stormwind, Elwynn", nodeID = 2, x = 0.4, y = 0.7, state = 0, cost = 0 },
        { name = "Ironforge, Dun Morogh", nodeID = 6, x = 0.45, y = 0.5, state = 1, cost = 500, routes = 1, hops = { 1, 2 } },
    } })
    flight:setDistance(3)
    p:target(flight)
    p:interact(flight)
    mock.waitSeconds(1)
    local option2 = button(G.GossipFrame, "I need a ride.")
    expect(option2 ~= nil).toBe(true)
    mock.click(option2)
    mock.waitSeconds(1)
    local taxi = G.TaxiFrame
    expect(taxi).toBeShown()
    expect(NumTaxiNodes()).toBe(2)
    expect(G.TaxiButton2).toBeShown()
    HideUIPanel(taxi)
    mock.waitSeconds(0.5)
    expect(taxi).never.toBeShown()
    p:target(nil)
    mock.waitSeconds(1)
end
"#,
        "1 passed, 0 failed",
    );
}

/// Levels gained with Blizzard's UI, without an error: a level 1 warrior
/// hunts wolves to level 4 (the player frame's level, the experience bar,
/// the chat's level and stat lines), then learns the new level's spell at
/// the trainer.
#[test]
fn blizzard_ui_through_level_ups() {
    blizzard_ui_test(
        "tests/levels.test.wowlua",
        r#"import test, expect from "wowlua:test"
import * as mock from "wowlua:mock"

local function chatHas(text: string): boolean
    local frame = (_G :: any).ChatFrame1
    for i = 1, frame:GetNumMessages() do
        local line = frame:GetMessageInfo(i)
        if type(line) == "string" and line:find(text, 1, true) then return true end
    end
    return false
end

-- Blizzard's experience bar (the status tracking bars' one shown).
local function expBar(): any
    for _, f in ipairs(mock.frames()) do
        local frame = f :: any
        if frame.GetLevelData == (_G :: any).ExpBarMixin.GetLevelData and frame:IsVisible() then return frame end
    end
    return nil
end

@test("levels gained")
local function levels()
    mock.setPlayer({ name = "Brannoc", level = 1, class = "WARRIOR", race = "Human", health = 60, maxHealth = 60,
        powerType = 1, power = 0, maxPower = 100 })
    mock.nextFrame(60)
    local p = mock.player :: any
    p:setStats({ xp = 0, xpMax = 400 })
    mock.defineLevel(2, { xpMax = 900, health = 19, power = 0, talents = 0, pvpTalentSlots = 0,
        strength = 1, agility = 1, stamina = 1, intellect = 0, spirit = 0 })
    mock.defineLevel(3, { xpMax = 1400, health = 20, power = 0, talents = 0, pvpTalentSlots = 0,
        strength = 1, agility = 1, stamina = 2, intellect = 0, spirit = 1 })
    mock.defineLevel(4, { xpMax = 2100, health = 21, power = 0, talents = 0, pvpTalentSlots = 0,
        strength = 2, agility = 1, stamina = 1, intellect = 0, spirit = 0 })
    mock.nextFrame(10)
    expect(UnitLevel("player")).toBe(1)
    expect((_G :: any).PlayerLevelText:GetText()).toBe("1")
    local bar = expBar()
    expect(bar ~= nil).toBe(true)
    -- Wolves hunted: each kill's experience; every third a level.
    local level = 1
    for i = 1, 9 do
        local wolf = mock.spawnUnit({ name = `Young Wolf {i}`, level = 1, reaction = "hostile", health = 40, maxHealth = 40,
            creatureType = "Beast", creatureTypeID = 1 })
        wolf:showNameplate()
        p:target(wolf)
        p:startAttack()
        mock.waitSeconds(0.5)
        p:swing(wolf, 15, { power = 5 })
        wolf:swing(p, 3)
        mock.waitSeconds(1.5)
        local xp = ({ [1] = 400, [2] = 900, [3] = 1400 })[level] / 3
        p:swing(wolf, 25, { power = 5, kill = { xp = math.ceil(xp) } })
        mock.waitSeconds(1)
        p:stopAttack()
        p:leaveCombat()
        wolf:hideNameplate()
        mock.waitSeconds(1)
        if i % 3 == 0 then
            level = level + 1
            expect(UnitLevel("player")).toBe(level)
            expect((_G :: any).PlayerLevelText:GetText()).toBe(tostring(level))
            expect(chatHas(`[Level {level}]`)).toBe(true)
        end
        -- The bar shows the experience of the level.
        expect(bar.StatusBar:GetValue()).toBe(UnitXP("player"))
        expect(select(2, bar.StatusBar:GetMinMaxValues())).toBe(UnitXPMax("player"))
    end
    expect(UnitLevel("player")).toBe(4)
    expect(UnitXPMax("player")).toBe(2100)
    expect(UnitHealthMax("player")).toBe(60 + 19 + 20 + 21)
    -- At the trainer: a spell the new level opened, learned.
    p:setMoney(10000)
    local trainer = mock.spawnUnit({ name = "Wilhelm Fairbanks", level = 40, reaction = "friendly", class = "WARRIOR", classID = 1 })
    mock.defineGossip(trainer, { text = "Strength and honour.", options = {
        { name = "I require warrior training.", icon = 132058, gossipOptionID = 96654, opens = "trainer" },
    } })
    mock.defineTrainer(trainer, { class = "WARRIOR", greeting = "Ready to train?" })
    trainer:setDistance(3)
    p:target(trainer)
    p:interact(trainer)
    mock.waitSeconds(1)
    C_GossipInfo.SelectOptionByIndex(0)
    mock.waitSeconds(1)
    expect((_G :: any).ClassTrainerFrame).toBeShown()
    -- Charge (level 4) available now, learned.
    local charge = 0
    for i = 1, GetNumTrainerServices() do
        local name, rank = GetTrainerServiceInfo(i)
        if name == "Charge" and rank == "available" then charge = i end
    end
    expect(GetTrainerServiceLevelReq(charge)).toBe(4)
    local row = (_G :: any).ClassTrainerFrame.ScrollBox:FindFrameByPredicate(function(_, d: any): boolean
        local data = d and (d.data or d)
        return data ~= nil and data.skillIndex == charge
    end)
    expect(row ~= nil).toBe(true)
    mock.click(row)
    mock.waitSeconds(0.2)
    expect((_G :: any).ClassTrainerTrainButton:IsEnabled()).toBe(true)
    expect(IsPlayerSpell(100)).toBe(false)
    mock.click((_G :: any).ClassTrainerTrainButton)
    mock.waitSeconds(1)
    expect(chatHas("You have learned a new ability: ")).toBe(true)
    expect(chatHas("[Charge]")).toBe(true)
    expect(select(2, GetTrainerServiceInfo(charge))).toBe("used")
    -- known from now on
    expect(IsPlayerSpell(100)).toBe(true)
    expect(C_SpellBook.IsSpellKnown(100)).toBe(true)
    HideUIPanel((_G :: any).ClassTrainerFrame)
    mock.waitSeconds(0.5)
    expect((_G :: any).ClassTrainerFrame).never.toBeShown()
end
"#,
        "1 passed, 0 failed",
    );
}

/// Every panel's key binding with Blizzard's UI, opened and closed, as the
/// fresh character (the default) and as the recorded one (its profile):
/// not a single Blizzard error, nothing refused.
#[test]
fn blizzard_ui_every_panel_binding() {
    let source = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/types-src");
    if !source.join("camelot/Interface").exists() {
        eprintln!("skipped: no Blizzard UI code (run `cargo xtask types fetch`)");
        return;
    }
    let panels = |profile: &str| {
        format!(
            r#"import test, expect, config from "wowlua:test"
import * as mock from "wowlua:mock"
{profile}
const BINDINGS = {{
    "TOGGLEGAMEMENU", "TOGGLEBACKPACK", "TOGGLEBAG1", "TOGGLEBAG2", "TOGGLEBAG3", "TOGGLEBAG4", "OPENALLBAGS",
    "TOGGLECHARACTER0", "TOGGLECHARACTER2", "TOGGLECURRENCY", "TOGGLECHARACTER4", "TOGGLESPELLBOOK",
    "TOGGLEPROFESSIONBOOK", "TOGGLEPETBOOK", "TOGGLETALENTS", "TOGGLECOOLDOWNVIEWERSETTINGS", "TOGGLEQUESTLOG",
    "TOGGLEWORLDMAP", "TOGGLEWORLDMAPSIZE", "TOGGLEMINIMAP", "TOGGLEBATTLEFIELDMINIMAP", "TOGGLEWORLDSTATESCORES",
    "TOGGLEGUILDTAB", "TOGGLESOCIAL", "TOGGLEFRIENDSTAB", "TOGGLEWHOTAB", "TOGGLECHATTAB", "TOGGLERAIDTAB",
    "TOGGLECOLLECTIONS", "TOGGLECOLLECTIONSMOUNTJOURNAL", "TOGGLECOLLECTIONSPETJOURNAL", "TOGGLECOLLECTIONSTOYBOX",
    "TOGGLECOLLECTIONSWARDROBE", "TOGGLEUI", "TOGGLEFPS",
}}

@test("every panel, opened and closed")
local function panels()
    mock.nextFrame(60)
    for _, name in ipairs(BINDINGS) do
        mock.pressBinding(name)
        mock.nextFrame(5)
        mock.pressBinding(name)
        mock.nextFrame(3)
    end
    expect(#mock.refusals()).toBe(0)
end
"#
        )
    };
    let recorded = fs::read_to_string(
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../wowlua-engine/tests/client/forever-camelot-character.json"),
    )
    .unwrap();
    let fresh = panels("");
    let as_recorded = panels(r#"config({ profile = "recorded" })"#);
    let dir = project(
        r#""camelot": {}"#,
        &[
            ("src/main.wowlua", "print(\"loaded\")\n"),
            ("tests/fresh.test.wowlua", &fresh),
            ("tests/recorded.test.wowlua", &as_recorded),
            ("tests/profiles/recorded.json", &recorded),
        ],
    );
    let manifest = dir.path().join("wowlua.json");
    let text = fs::read_to_string(&manifest).unwrap();
    fs::write(
        &manifest,
        text.replacen('{', r#"{ "test": { "blizzard-ui": true },"#, 1),
    )
    .unwrap();
    let out = wowlua(dir.path())
        .arg("test")
        .env("WOWLUA_UI_SOURCE", &source)
        .output()
        .unwrap();
    let text = format!(
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(out.status.success(), "{text}");
    assert!(text.contains("2 passed, 0 failed"), "{text}");
}

/// Every control in every panel with Blizzard's UI, as the fresh and the
/// recorded character: buttons hovered, left- and right-clicked (each tab's
/// content too), the menus and dialogs they raise explored, edit boxes
/// typed into, scroll areas wheeled; spells, actions, bag and equipped
/// items dragged. Not a single Blizzard error, nothing refused.
#[test]
fn blizzard_ui_every_button_in_every_panel() {
    let source = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/types-src");
    if !source.join("camelot/Interface").exists() {
        eprintln!("skipped: no Blizzard UI code (run `cargo xtask types fetch`)");
        return;
    }
    let clicks = |profile: &str| {
        format!(
            r#"import test, expect, config from "wowlua:test"
import * as mock from "wowlua:mock"
{profile}

const BINDINGS = {{
    "TOGGLEGAMEMENU", "TOGGLEBACKPACK", "OPENALLBAGS",
    "TOGGLECHARACTER0", "TOGGLECHARACTER2", "TOGGLECURRENCY", "TOGGLECHARACTER4", "TOGGLESPELLBOOK",
    "TOGGLEPROFESSIONBOOK", "TOGGLEPETBOOK", "TOGGLETALENTS", "TOGGLECOOLDOWNVIEWERSETTINGS", "TOGGLEQUESTLOG",
    "TOGGLEWORLDMAP", "TOGGLEBATTLEFIELDMINIMAP", "TOGGLEWORLDSTATESCORES",
    "TOGGLEGUILDTAB", "TOGGLESOCIAL", "TOGGLEWHOTAB", "TOGGLECHATTAB", "TOGGLERAIDTAB",
    "TOGGLECOLLECTIONS", "TOGGLECOLLECTIONSMOUNTJOURNAL", "TOGGLECOLLECTIONSPETJOURNAL", "TOGGLECOLLECTIONSTOYBOX",
    "TOGGLECOLLECTIONSWARDROBE",
}}

const G = _G :: any

--- The visible top-level frames (under UIParent; menus under WorldFrame).
local function shown(): {{ [any]: boolean }}
    const out: {{ [any]: boolean }} = {{}}
    for _, parent in ipairs({{ G.UIParent, G.WorldFrame }}) do
        for _, f in ipairs({{ parent:GetChildren() }}) do
            if f ~= G.UIParent and not f:IsForbidden() and f:IsVisible() then
                out[f] = true
            end
        end
    end
    return out
end

--- A close button: used last, so the rest of its frame is explored.
local function closes(name: string): boolean
    return name:find("Close") ~= nil or name:find("Cancel") ~= nil
end

local function descendants(f: any, out: any[])
    for _, c in ipairs({{ f:GetChildren() }}) do
        if not c:IsForbidden() then
            out[#out + 1] = c
            descendants(c, out)
        end
    end
end

local function label(f: any): string
    const name = f:GetName()
    if name then
        return name
    end
    const parent = f:GetParent()
    if parent == nil then
        return "?"
    end
    for k, v in pairs(parent) do
        if v == f and type(k) == "string" then
            return `{{label(parent)}}.{{k}}`
        end
    end
    return `{{label(parent)}}.?`
end

local function isRoot(roots: any[], f: any): boolean
    for _, r in ipairs(roots) do
        if r == f then
            return true
        end
    end
    return false
end

local function anyVisible(roots: any[]): boolean
    for _, r in ipairs(roots) do
        if r:IsVisible() then
            return true
        end
    end
    return false
end

local function hidePanel(f: any)
    G.HideUIPanel(f)
    if f:IsVisible() then
        f:Hide()
    end
end

@test("every control in every panel: left and right clicks, menus, typing, wheels")
local function deep()
    mock.nextFrame(60)
    local report: string[] = {{}}
    const failures: string[] = {{}}
    local total = 0
    local function check(what: string, n: number)
        const errs = mock.errors()
        if #errs > n then
            failures[#failures + 1] = `{{what}}: {{tostring(errs[n + 1])}}`
        end
    end
    for _, binding in ipairs(BINDINGS) do
        const before = shown()
        mock.pressBinding(binding)
        mock.nextFrame(5)
        const roots: any[] = {{}}
        for f in pairs(shown()) do
            if not before[f] then
                roots[#roots + 1] = f
            end
        end
        const done: {{ [any]: boolean }} = {{}}
        local actions = 0
        while actions < 500 and anyVisible(roots) do
            -- The next control not yet used, in the panel as it is now.
            const frames: any[] = {{}}
            for _, r in ipairs(roots) do
                if r:IsVisible() then
                    descendants(r, frames)
                end
            end
            local next: any = nil
            for _, f in ipairs(frames) do
                if not done[f] and f:IsVisible() then
                    if f:IsObjectType("Button") or f:IsObjectType("EditBox") or f:IsObjectType("ScrollFrame") or f.GetScrollPercentage ~= nil then
                        next = f
                        break
                    end
                end
            end
            if next == nil then
                break
            end
            done[next] = true
            const name = label(next)
            if name:find("Logout") or name:find("Quit") or name:find("ExitGame") then
                continue
            end
            actions += 1
            local n = #mock.errors()
            if next:IsObjectType("EditBox") then
                if next:IsEnabled() then
                    mock.typeText(next, "ab")
                    mock.nextFrame(1)
                    mock.pressKey("ENTER")
                    mock.nextFrame(1)
                    if next:HasFocus() then
                        mock.pressKey("ESCAPE")
                        mock.nextFrame(1)
                    end
                    check(`{{binding}} / type {{name}}`, n)
                end
            elseif next:IsObjectType("Button") then
                if next:IsEnabled() then
                    const shownBefore = shown()
                    mock.hover(next)
                    mock.click(next)
                    mock.nextFrame(2)
                    check(`{{binding}} / {{name}}`, n)
                    -- What the click raised (a menu, a dialog): its buttons
                    -- clicked, then it is closed.
                    for f in pairs(shown()) do
                        if not shownBefore[f] and not isRoot(roots, f) and f:IsVisible() then
                            local k = 0
                            const usedInner: {{ [any]: boolean }} = {{}}
                            -- Its controls, looking again after each (a tab,
                            -- a category shows more); close buttons last.
                            while k < 60 and f:IsVisible() do
                                const inner: any[] = {{}}
                                descendants(f, inner)
                                local pick: any = nil
                                local closer: any = nil
                                for _, b in ipairs(inner) do
                                    if not usedInner[b] and b:IsVisible() and b:IsObjectType("Button") and b:IsEnabled() then
                                        if closes(label(b)) then
                                            closer = closer ?? b
                                        else
                                            pick = b
                                            break
                                        end
                                    end
                                end
                                pick = pick ?? closer
                                if pick == nil then
                                    break
                                end
                                usedInner[pick] = true
                                k += 1
                                actions += 1
                                n = #mock.errors()
                                mock.click(pick)
                                mock.nextFrame(1)
                                check(`{{binding}} / {{name}} -> {{label(pick)}}`, n)
                            end
                            if f:IsVisible() then
                                hidePanel(f)
                            end
                        end
                    end
                    -- The right button, where the control still shows.
                    if next:IsVisible() and next:IsEnabled() then
                        n = #mock.errors()
                        mock.click(next, "RightButton")
                        mock.nextFrame(2)
                        check(`{{binding}} / right {{name}}`, n)
                        for f in pairs(shown()) do
                            if not before[f] and not isRoot(roots, f) and f:IsVisible() then
                                hidePanel(f)
                            end
                        end
                    end
                    mock.hover(nil)
                end
            else
                mock.wheel(next, -1)
                mock.wheel(next, 1)
                mock.nextFrame(1)
                check(`{{binding}} / wheel {{name}}`, n)
            end
            -- Back to the panel if the action closed it.
            if not anyVisible(roots) then
                mock.pressBinding(binding)
                mock.nextFrame(3)
            end
        end
        total += actions
        report[#report + 1] = `{{binding}}: {{actions}} actions`
        const n = #mock.errors()
        mock.pressBinding(binding)
        mock.nextFrame(2)
        for f in pairs(shown()) do
            if not before[f] and f:IsVisible() then
                hidePanel(f)
            end
        end
        mock.nextFrame(3)
        check(`{{binding}} closing`, n)
    end
    -- Drags: spells onto the action bar, actions onto actions, bag items
    -- between slots, equipped items onto their slots.
    local drags = 0
    local function dragged(what: string, from: any, to: any)
        const n = #mock.errors()
        mock.dragTo(from, to)
        mock.nextFrame(1)
        drags += 1
        check(`drag {{what}}`, n)
        G.ClearCursor()
        mock.nextFrame(1)
    end
    local function withScript(root: any, script: string): any[]
        const all: any[] = {{}}
        descendants(root, all)
        const out: any[] = {{}}
        for _, f in ipairs(all) do
            if f:IsVisible() and f:HasScript(script) and f:GetScript(script) ~= nil then
                out[#out + 1] = f
            end
        end
        return out
    end
    mock.pressBinding("TOGGLESPELLBOOK")
    mock.nextFrame(5)
    for i, b in ipairs(withScript(G.PlayerSpellsFrame, "OnDragStart")) do
        if i <= 20 then
            dragged(`spell {{label(b)}} -> ActionButton{{(i % 12) + 1}}`, b, G[`ActionButton{{(i % 12) + 1}}`])
        end
    end
    mock.pressBinding("TOGGLESPELLBOOK")
    mock.nextFrame(3)
    for i = 1, 11 do
        dragged(`ActionButton{{i}} -> ActionButton{{i + 1}}`, G[`ActionButton{{i}}`], G[`ActionButton{{i + 1}}`])
    end
    mock.pressBinding("OPENALLBAGS")
    mock.nextFrame(5)
    for _, root in ipairs({{ G.ContainerFrameCombinedBags, G.ContainerFrame1, G.ContainerFrame2 }}) do
        if root and root:IsVisible() then
            const slots = withScript(root, "OnDragStart")
            for i, b in ipairs(slots) do
                if i <= 20 then
                    dragged(`bag {{label(b)}} -> {{label(slots[#slots - i + 1])}}`, b, slots[#slots - i + 1])
                end
            end
        end
    end
    mock.pressBinding("OPENALLBAGS")
    mock.nextFrame(3)
    mock.pressBinding("TOGGLECHARACTER0")
    mock.nextFrame(5)
    for _, b in ipairs(withScript(G.PaperDollItemsFrame ?? G.CharacterFrame, "OnDragStart")) do
        dragged(`equipped {{label(b)}} onto itself`, b, b)
    end
    mock.pressBinding("TOGGLECHARACTER0")
    mock.nextFrame(3)
    -- Not a single error; nothing refused.
    if #failures > 0 then
        error(table.concat(failures, "\n"), 0)
    end
    expect(#mock.refusals()).toBe(0)
end
"#
        )
    };
    let recorded = fs::read_to_string(
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../wowlua-engine/tests/client/forever-camelot-character.json"),
    )
    .unwrap();
    let fresh = clicks("");
    let as_recorded = clicks(r#"config({ profile = "recorded" })"#);
    let dir = project(
        r#""camelot": {}"#,
        &[
            ("src/main.wowlua", "print(\"loaded\")\n"),
            ("tests/fresh.test.wowlua", &fresh),
            ("tests/recorded.test.wowlua", &as_recorded),
            ("tests/profiles/recorded.json", &recorded),
        ],
    );
    let manifest = dir.path().join("wowlua.json");
    let text = fs::read_to_string(&manifest).unwrap();
    fs::write(
        &manifest,
        text.replacen('{', r#"{ "test": { "blizzard-ui": true },"#, 1),
    )
    .unwrap();
    let out = wowlua(dir.path())
        .arg("test")
        .env("WOWLUA_UI_SOURCE", &source)
        .output()
        .unwrap();
    let text = format!(
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(out.status.success(), "{text}");
    assert!(text.contains("2 passed, 0 failed"), "{text}");
}

/// The guild's panels with Blizzard's UI (the communities frame's guild):
/// as the default character, its leader (the recorded guild: 27 members),
/// as an Initiate of a guild of two and outside a guild; each display
/// mode, a member's details, the guild control panel (the leader's):
/// not a single Blizzard error, nothing refused.
#[test]
fn blizzard_ui_guild_panels() {
    let source = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/types-src");
    if !source.join("camelot/Interface").exists() {
        eprintln!("skipped: no Blizzard UI code (run `cargo xtask types fetch`)");
        return;
    }
    let tests = r#"import test, expect from "wowlua:test"
import * as mock from "wowlua:mock"

const G = _G :: any
const MODES = { "ROSTER", "GUILD_INFO", "GUILD_BENEFITS", "GUILD_PREFERRED_PLAY_SETTINGS", "CHAT", "ROSTER" }

--- The guild's panels toured; the roster's size and whether the rank
--- editor's button showed.
local function tour(): (number, boolean)
    mock.pressBinding("TOGGLEGUILDTAB")
    mock.nextFrame(10)
    const f = G.CommunitiesFrame
    expect(f).toBeShown()
    expect(f:GetSelectedClubId()).toBe(C_Club.GetGuildClubId())
    for _, mode in ipairs(MODES) do
        f:SetDisplayMode(G.COMMUNITIES_FRAME_DISPLAY_MODES[mode])
        mock.nextFrame(5)
    end
    const size = f.MemberList.ScrollBox:GetDataProvider():GetSize()
    const row = f.MemberList.ScrollBox:FindFrameByPredicate(function(_, d: any): boolean
        return d ~= nil and d.memberInfo ~= nil and not d.memberInfo.isSelf
    end)
    expect(row ~= nil).toBe(true)
    mock.click(row)
    mock.nextFrame(3)
    expect(f.GuildMemberDetailFrame).toBeShown()
    const control = f.CommunitiesControlFrame.GuildControlButton
    const shown = control:IsShown()
    if shown then
        mock.click(control)
        mock.nextFrame(5)
        expect(G.GuildControlUI).toBeShown()
    end
    mock.pressBinding("TOGGLEGUILDTAB")
    mock.nextFrame(5)
    expect(#mock.errors()).toBe(0)
    expect(#mock.refusals()).toBe(0)
    return size, shown
end

@test("the leader of the recorded guild")
local function leader()
    mock.nextFrame(60)
    expect(IsGuildLeader()).toBe(true)
    local size, control = tour()
    expect(size).toBe(27)
    expect(control).toBe(true)
end

@test("an Initiate")
local function initiate()
    mock.joinGuild({ rank = 4, members = { { name = "Otherplayer", lastName = "Othername", rank = 0, online = true, class = "MAGE", level = 60 } } })
    mock.nextFrame(60)
    local size, control = tour()
    expect(size).toBe(2)
    expect(control).toBe(false)
end

@test("no guild")
local function none()
    mock.leaveGuild()
    mock.nextFrame(60)
    mock.pressBinding("TOGGLEGUILDTAB")
    mock.nextFrame(10)
    expect(IsInGuild()).toBe(false)
    mock.pressBinding("TOGGLEGUILDTAB")
    mock.nextFrame(5)
    expect(#mock.errors()).toBe(0)
end
"#;
    let dir = project(
        r#""camelot": {}"#,
        &[
            ("src/main.wowlua", "print(\"loaded\")\n"),
            ("tests/guild.test.wowlua", tests),
        ],
    );
    let manifest = dir.path().join("wowlua.json");
    let text = fs::read_to_string(&manifest).unwrap();
    fs::write(
        &manifest,
        text.replacen('{', r#"{ "test": { "blizzard-ui": true },"#, 1),
    )
    .unwrap();
    let out = wowlua(dir.path())
        .arg("test")
        .env("WOWLUA_UI_SOURCE", &source)
        .output()
        .unwrap();
    let text = format!(
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(out.status.success(), "{text}");
    assert!(text.contains("3 passed, 0 failed"), "{text}");
}

/// The settings, every one as the player uses them: each category opened
/// from the game menu; each row's controls clicked (checkboxes both ways,
/// tabs, expanders), each menu's every entry chosen, dialogs confirmed,
/// sliders dragged and wheeled; each key binding bound by clicking its button and
/// pressing a chord, checked, unbound by a right click; every row of every
/// list reached, the page found again when a control leaves it; sliders'
/// thumbs dragged to each end; Defaults confirmed. Not a single Blizzard error, nothing refused.
#[test]
fn blizzard_ui_every_setting() {
    let source = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/types-src");
    if !source.join("camelot/Interface").exists() {
        eprintln!("skipped: no Blizzard UI code (run `cargo xtask types fetch`)");
        return;
    }
    let settings = |profile: &str| {
        format!(
            r#"import test, expect, config from "wowlua:test"
import * as mock from "wowlua:mock"
{profile}

const G = _G :: any

local function descendants(f: any, out: any[])
    for _, c in ipairs({{ f:GetChildren() }}) do
        if not c:IsForbidden() then
            out[#out + 1] = c
            descendants(c, out)
        end
    end
end

--- The visible frames directly under WorldFrame and UIParent.
local function topLevel(): {{ [any]: boolean }}
    const out: {{ [any]: boolean }} = {{}}
    for _, parent in ipairs({{ G.UIParent, G.WorldFrame }}) do
        for _, f in ipairs({{ parent:GetChildren() }}) do
            if f ~= G.UIParent and not f:IsForbidden() and f:IsVisible() then
                out[f] = true
            end
        end
    end
    return out
end

local function buttonsIn(f: any): any[]
    const all: any[] = {{}}
    descendants(f, all)
    const out: any[] = {{}}
    for _, b in ipairs(all) do
        if b:IsVisible() and b:IsObjectType("Button") and b:IsEnabled() then
            out[#out + 1] = b
        end
    end
    return out
end

--- A key binding row's binding (its initializer's index), if it is one.
local function bindingOf(f: any): number?
    const init = f.initializer
    return init and init.data and init.data.bindingIndex
end

--- The chord the binding rows get: no default binds it.
const CHORD = "CTRL-SHIFT-F9"

@test("every setting: categories, checkboxes, every dropdown option, sliders, key bindings, buttons")
local function settings()
    mock.nextFrame(60)
    const failures: string[] = {{}}
    local function check(what: string, n: number)
        const errs = mock.errors()
        if #errs > n then
            failures[#failures + 1] = `{{what}}: {{tostring(errs[n + 1]):sub(1, 240)}}`
        end
    end
    const panel = G.SettingsPanel
    const list = panel.CategoryList
    const settingsList = panel.Container.SettingsList
    -- Open the settings as the player does: the game menu's Options (the
    -- menu key first closes the windows open, one at a time).
    local function openSettings()
        for _ = 1, 6 do
            if panel:IsVisible() then
                return
            end
            if G.GameMenuFrame:IsVisible() then
                for _, b in ipairs(buttonsIn(G.GameMenuFrame)) do
                    if b:GetText() == G.GAMEMENU_OPTIONS or b:GetText() == G.SETTINGS_TITLE then
                        mock.click(b)
                    end
                end
            else
                mock.pressBinding("TOGGLEGAMEMENU")
            end
            mock.nextFrame(3)
        end
    end
    --- The menu open (Blizzard's menu manager), if any.
    local function openMenu(): any
        return G.Menu.GetManager():GetOpenMenu()
    end
    --- The category list's row for `element`, scrolled to with the wheel.
    local function rowOf(element: any): any
        for _ = 1, 60 do
            const f = list.ScrollBox:FindFrame(element)
            if f and f:IsVisible() then
                return f
            end
            -- Up first if it is above what shows, else down.
            const index = list.ScrollBox:FindElementDataIndex(element) or 0
            local first = math.huge
            list.ScrollBox:ForEachFrame(function(r: any)
                first = math.min(first, list.ScrollBox:FindElementDataIndex(r:GetElementData()) or math.huge)
            end)
            mock.wheel(list.ScrollBox, index < first and 1 or -1)
            mock.nextFrame(1)
        end
        return nil
    end
    --- The category list's element for `category` as it is now (opening
    --- a category from a button, `Settings.OpenToCategory`, lays the list
    --- out anew), else `element`.
    local function elementOf(element: any, category: any): any
        local found = nil
        list.ScrollBox:GetDataProvider():ForEach(function(e: any)
            if not found and e.data and e.data.category == category then
                found = e
            end
        end)
        return found or element
    end
    --- Selects a category by clicking its row: whether it is shown.
    local function select(element: any, category: any): boolean
        openSettings()
        const row = rowOf(elementOf(element, category))
        if not row then
            return false
        end
        if panel:GetCurrentCategory() ~= category then
            const n = #mock.errors()
            mock.click(row)
            mock.nextFrame(3)
            check(`category {{category:GetName()}}`, n)
        end
        return panel:IsVisible() and panel:GetCurrentCategory() == category
    end
    openSettings()
    assert(panel:IsVisible(), "the settings opened")
    const stats = {{ headers = 0, categories = 0, controls = 0, options = 0, sliders = 0, bindings = 0 }}

    --- The dialogs an action raised (new frames, and any confirmation
    --- shown): their first button (accept) clicked, Escape if they stay.
    local function answerDialogs(before: {{ [any]: boolean }}, key: string)
        const dialogs: {{ [any]: boolean }} = {{}}
        for dialog in pairs(topLevel()) do
            if not before[dialog] and dialog ~= panel then
                dialogs[dialog] = true
            end
        end
        for i = 1, 4 do
            const popup = G[`StaticPopup{{i}}`]
            if popup and popup:IsVisible() then
                dialogs[popup] = true
            end
        end
        for dialog in pairs(dialogs) do
            if dialog:IsVisible() then
                const first = buttonsIn(dialog)[1]
                if first then
                    const n = #mock.errors()
                    mock.click(first)
                    mock.nextFrame(1)
                    check(`{{key}}: dialog {{tostring(dialog.which or dialog:GetDebugName())}}`, n)
                end
                if dialog:IsVisible() then
                    mock.pressKey("ESCAPE")
                    mock.nextFrame(1)
                end
            end
        end
    end

    --- Uses every control of `category`'s rows, scrolling down its list;
    --- comes back to the page (and the place in it) when a control leaves.
    local function useSettings(element: any, category: any)
        const name = category:GetName()
        const done: {{ [any]: boolean }} = {{}}
                local scrolled = 0
        --- Whether the page is still this category's; if not, back to it.
        local function stayed(what: string): boolean
            if panel:IsVisible() and panel:GetCurrentCategory() == category then
                return true
            end
            -- Whatever it opened, closed (a dialog, a frame), as the user
            -- presses Escape.
            for _ = 1, 3 do
                if panel:IsVisible() then
                    break
                end
                mock.pressKey("ESCAPE")
                mock.nextFrame(2)
            end
            if not select(element, category) then
                const shown: string[] = {{}}
                for f in pairs(topLevel()) do
                    shown[#shown + 1] = tostring(f:GetDebugName())
                end
                failures[#failures + 1] = `{{name}}: could not come back after {{what}} (panel {{tostring(panel:IsVisible())}}, menu {{tostring(G.GameMenuFrame:IsVisible())}}, current {{tostring(panel:GetCurrentCategory() and panel:GetCurrentCategory():GetName())}}, shown {{table.concat(shown, " ")}})`
                return false
            end
            for _ = 1, scrolled do
                mock.wheel(settingsList.ScrollBox, -1)
            end
            mock.nextFrame(1)
            return false
        end
        --- A binding row: CHORD bound by clicking its button and pressing
        --- the keys, then unbound by a right click.
        local function useBinding(f: any, key: string): boolean
            const action = G.GetBinding(bindingOf(f))
            const button = f.Button1
            if not (button and button:IsVisible() and button:IsEnabled()) then
                return true
            end
            stats.bindings += 1
            -- Binding replaces the action's keys: they are put back after.
            const original = {{ G.GetBindingKey(action) }}
            local n = #mock.errors()
            mock.click(button)
            mock.setModifiers({{ ctrl = true, shift = true }})
            mock.pressKey("F9")
            mock.setModifiers({{}})
            mock.nextFrame(1)
            check(`{{key}}: bind`, n)
            local keys = table.concat({{ G.GetBindingKey(action) }}, ",")
            if not keys:find(CHORD, 1, true) then
                failures[#failures + 1] = `{{key}}: {{CHORD}} not bound to {{action}} (keys {{keys}})`
            end
            n = #mock.errors()
            mock.click(button, "RightButton")
            mock.nextFrame(1)
            check(`{{key}}: unbind`, n)
            keys = table.concat({{ G.GetBindingKey(action) }}, ",")
            if keys:find(CHORD, 1, true) then
                failures[#failures + 1] = `{{key}}: {{CHORD}} still bound to {{action}} after a right click`
            end
            for _, k in ipairs(original) do
                G.SetBinding(k, action)
            end
            return stayed(`{{key}} binding`)
        end
        --- Every control of a row, in rounds until none is new (tabs and
        --- expanders show more): whether the page stayed.
        local function useRow(f: any, key: string): boolean
            const clicked: {{ [any]: boolean }} = {{}}
            for _ = 1, 6 do
                local more = false
                for _, b in ipairs(buttonsIn(f)) do
                    const parent = b:GetParent()
                    if bindingOf(parent) then
                        if not clicked[parent] then
                            clicked[parent] = true
                            more = true
                            if not useBinding(parent, `{{key}}/{{G.GetBinding(bindingOf(parent))}}`) then
                                return false
                            end
                        end
                    elseif b:IsVisible() and b:IsEnabled() and not clicked[b] then
                        clicked[b] = true
                        more = true
                        stats.controls += 1
                        const before = topLevel()
                        local n = #mock.errors()
                        mock.click(b)
                        mock.nextFrame(1)
                        check(`{{key}}: click`, n)
                        -- A menu it opened (a dropdown's, a context menu):
                        -- every entry chosen, the menu opened again by the
                        -- same click before each; closed with Escape.
                        if openMenu() then
                            const count = #buttonsIn(openMenu())
                            for i = 1, count do
                                if not openMenu() then
                                    mock.click(b)
                                    mock.nextFrame(1)
                                end
                                const menu = openMenu()
                                const entry = menu and buttonsIn(menu)[i]
                                if not entry then
                                    break
                                end
                                stats.options += 1
                                n = #mock.errors()
                                mock.click(entry)
                                mock.nextFrame(1)
                                check(`{{key}}: option {{i}}`, n)
                                if not stayed(`{{key}} option {{i}}`) then
                                    return false
                                end
                            end
                            if openMenu() then
                                mock.pressKey("ESCAPE")
                                mock.nextFrame(1)
                            end
                            if openMenu() then
                                failures[#failures + 1] = `{{key}}: Escape left its menu open`
                            end
                        end
                        answerDialogs(before, key)
                        if not stayed(`{{key}} {{tostring(b:GetDebugName())}}`) then
                            return false
                        end
                    end
                end
                if not more then
                    break
                end
            end
            -- Its sliders: dragged and wheeled.
            const all: any[] = {{}}
            descendants(f, all)
            for _, s in ipairs(all) do
                if s:IsObjectType("Slider") and s:IsVisible() and s:IsEnabled() then
                    stats.sliders += 1
                    const n = #mock.errors()
                    -- Its thumb dragged to each end and the middle; the
                    -- wheel both ways.
                    local lo, hi = s:GetMinMaxValues()
                    for _, v in ipairs({{ hi, lo, (lo + hi) / 2 }}) do
                        mock.slide(s, v)
                        mock.nextFrame(1)
                    end
                    mock.wheel(s, 1)
                    mock.wheel(s, -1)
                    mock.nextFrame(1)
                    check(`{{key}}: slider`, n)
                end
            end
            return stayed(`{{key}} sliders`)
        end
        for _ = 1, 400 do
            const frames: any[] = {{}}
            settingsList.ScrollBox:ForEachFrame(function(f: any)
                frames[#frames + 1] = f
            end)
            local again = false
            for _, f in ipairs(frames) do
                const data = f.GetElementData and f:GetElementData()
                if f:IsVisible() and data and not done[data] then
                    done[data] = true
                    const label = data.data and data.data.name or data:GetTemplate()
                    if not useRow(f, `{{name}}/{{tostring(label)}}`) then
                        -- Back on the page: its frames are new.
                        again = true
                        break
                    end
                end
            end
            if not again then
                const box = settingsList.ScrollBox
                if not box:HasScrollableExtent() or box:GetScrollPercentage() >= 1 then
                    break
                end
                mock.wheel(box, -1)
                scrolled += 1
                mock.nextFrame(1)
            end
        end
        -- The rows the list held that no pass reached.
        const missed: string[] = {{}}
        const provider = settingsList.ScrollBox:GetDataProvider()
        if provider then
            provider:ForEach(function(e: any)
                if not done[e] then
                    missed[#missed + 1] = tostring(e.data and e.data.name or e:GetTemplate())
                end
            end)
        end
        if #missed > 0 then
            failures[#failures + 1] = `{{name}}: rows not reached: {{table.concat(missed, ", ")}}`
        end
    end

    -- Every category in the list, by its data (rows are made anew when the
    -- panel opens again); headers collapsed and expanded again.
    const elements: any[] = {{}}
    list.ScrollBox:GetDataProvider():ForEach(function(e: any)
        elements[#elements + 1] = e
    end)
    for _, element in ipairs(elements) do
        const category = element.data and element.data.category
        openSettings()
        if category then
            stats.categories += 1
            if select(element, category) then
                useSettings(element, category)
            else
                failures[#failures + 1] = `category {{category:GetName()}}: could not select it`
            end
        else
            const row = rowOf(element)
            if row then
                stats.headers += 1
                const n = #mock.errors()
                mock.click(row)
                mock.nextFrame(2)
                mock.click(rowOf(element))
                mock.nextFrame(2)
                check(`header {{tostring(row.Label and row.Label:GetText())}}`, n)
            end
        end
    end
    -- Defaults (confirmed), then close.
    openSettings()
    for _, b in ipairs(buttonsIn(panel)) do
        if b:GetText() == G.SETTINGS_DEFAULTS then
            const before = topLevel()
            const n = #mock.errors()
            mock.click(b)
            mock.nextFrame(2)
            for dialog in pairs(topLevel()) do
                if not before[dialog] and dialog ~= panel then
                    const first = buttonsIn(dialog)[1]
                    if first then
                        mock.click(first)
                        mock.nextFrame(2)
                    end
                end
            end
            check("defaults", n)
        end
    end
    mock.pressKey("ESCAPE")
    mock.nextFrame(2)
    print(`headers {{stats.headers}}, categories {{stats.categories}}, controls {{stats.controls}}, menu options {{stats.options}}, sliders {{stats.sliders}}, key bindings {{stats.bindings}}`)
    -- Not a single error; nothing refused.
    if #failures > 0 then
        error(table.concat(failures, "\n"), 0)
    end
    expect(#mock.refusals()).toBe(0)
end
"#
        )
    };
    let recorded = fs::read_to_string(
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../wowlua-engine/tests/client/forever-camelot-character.json"),
    )
    .unwrap();
    let fresh = settings("");
    let as_recorded = settings(r#"config({ profile = "recorded" })"#);
    let dir = project(
        r#""camelot": {}"#,
        &[
            ("src/main.wowlua", "print(\"loaded\")\n"),
            ("tests/fresh.test.wowlua", &fresh),
            ("tests/recorded.test.wowlua", &as_recorded),
            ("tests/profiles/recorded.json", &recorded),
        ],
    );
    let manifest = dir.path().join("wowlua.json");
    let text = fs::read_to_string(&manifest).unwrap();
    fs::write(
        &manifest,
        text.replacen('{', r#"{ "test": { "blizzard-ui": true },"#, 1),
    )
    .unwrap();
    let out = wowlua(dir.path())
        .arg("test")
        .env("WOWLUA_UI_SOURCE", &source)
        .output()
        .unwrap();
    let text = format!(
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(out.status.success(), "{text}");
    assert!(text.contains("2 passed, 0 failed"), "{text}");
}

/// Tooltips with Blizzard's UI: the engine's tooltip data from the tests'
/// game data, Blizzard's tooltip code, and an addon's post-call.
/// `mock.tooltipLines` reads what a tooltip shows (left or right texts),
/// `mock.textOf` what a font string shows (decision S52);
/// `mock.sentAddonMessages` gives typed messages (decision S47).
#[test]
fn tooltip_lines_and_sent_messages() {
    let dir = project(
        r#""camelot": {}"#,
        &[
            ("src/main.wowlua", "print(\"demo\")\n"),
            (
                "tests/tip.test.wowlua",
                r#"import test, expect from "wowlua:test"
import * as mock from "wowlua:mock"

@test("the lines a tooltip shows; the messages sent")
local function lines()
    local tip = CreateFrame("GameTooltip", "DemoTip", UIParent)
    tip:SetOwner(UIParent, "ANCHOR_NONE")
    tip:AddLine("Title")
    tip:AddDoubleLine("Left", "Right")
    expect(mock.tooltipLines(tip)).toEqual({ "Title", "Left" })
    expect(mock.tooltipLines(tip, true)).toEqual({ "", "Right" })
    local label = UIParent:CreateFontString(nil, "OVERLAY")
    label:SetFont("Fonts\\FRIZQT__.TTF", 12, "")
    expect(mock.textOf(label)).toBe("")
    label:SetText("Shown")
    expect(mock.textOf(label)).toBe("Shown")
    tip:Hide()
    C_ChatInfo.RegisterAddonMessagePrefix("Demo")
    C_ChatInfo.SendAddonMessage("Demo", "hello", "WHISPER", "Ann")
    local sent = mock.sentAddonMessages()
    local last = sent[#sent]
    expect(last.prefix).toBe("Demo")
    expect(last.message).toBe("hello")
    expect(last.chatType).toBe("WHISPER")
    expect(last.target).toBe("Ann")
end
"#,
            ),
        ],
    );
    let (ok, out) = test(dir.path(), &[]);
    assert!(ok, "{out}");
}

#[test]
fn tooltips_with_blizzard_ui() {
    let source = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/types-src");
    if !source.join("camelot/Interface").exists() {
        eprintln!("skipped: no Blizzard UI code (run `cargo xtask types fetch`)");
        return;
    }
    let dir = project(
        r#""camelot": {}"#,
        &[
            (
                "src/main.wowlua",
                r#"TooltipDataProcessor.AddTooltipPostCall(Enum.TooltipDataType.Item, function(tip: any, data: any)
    tip:AddLine("Demo: item " .. data.id)
end)
"#,
            ),
            (
                "tests/tip.test.wowlua",
                r#"import test, expect from "wowlua:test"
import * as mock from "wowlua:mock"

@test("an item tooltip with the addon's line")
local function item()
    mock.defineItem(1, { name = "Not loaded" }, { cached = false })
    expect(mock.showTooltip("item", { id = 1 })[1]).toContain("Retrieving item information")
    mock.defineItem(19019, { name = "Thunderfury, Blessed Blade of the Windseeker", quality = 5, itemLevel = 80 }, { cached = true })
    -- No item level line (WoW Forever's item tooltips have none); the
    -- issue reporter's line, as on units, the item's link known.
    local lines = mock.showTooltip("item", { id = 19019 })
    expect(lines).toEqual({
        "Thunderfury, Blessed Blade of the Windseeker",
        " ",
        "|c0042b1fePress F6 to submit an issue for this Item",
        "Demo: item 19019",
    })
    expect(GameTooltip).toBeShown()
end

@test("a unit tooltip")
local function unit()
    local hogger = mock.spawnUnit({ name = "Hogger", level = 11, reaction = "hostile" })
    mock.player:target(hogger)
    -- The beta client's issue reporter (Blizzard_PTRFeedback, loaded on
    -- beta and PTR clients only) adds its line to unit tooltips: its key,
    -- F6 by default (its binding in the "PTR" category, as `GetBinding`
    -- answers it).
    expect(mock.showTooltip("unit", { unit = "target" })).toEqual({
        "Hogger",
        "Level 11",
        " ",
        "|c0042b1fePress F6 to submit an issue for this Creature",
    })
end
"#,
            ),
        ],
    );
    let manifest = dir.path().join("wowlua.json");
    let text = fs::read_to_string(&manifest).unwrap();
    fs::write(
        &manifest,
        text.replacen('{', r#"{ "test": { "blizzard-ui": true },"#, 1),
    )
    .unwrap();
    let out = wowlua(dir.path())
        .arg("test")
        .env("WOWLUA_UI_SOURCE", &source)
        .output()
        .unwrap();
    let text = format!(
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(out.status.success(), "{text}");
    assert!(text.contains("2 passed, 0 failed"), "{text}");
}

/// Blizzard's UI with these test files: success and stdout + stderr, or
/// `None` when the UI code is not on disk.
fn blizzard_ui_files(files: &[(&str, &str)]) -> Option<(bool, String)> {
    let source = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/types-src");
    if !source.join("camelot/Interface").exists() {
        eprintln!("skipped: no Blizzard UI code (run `cargo xtask types fetch`)");
        return None;
    }
    let mut all = vec![("src/main.wowlua", "print(\"loaded\")\n")];
    all.extend_from_slice(files);
    let dir = project(r#""camelot": {}"#, &all);
    let manifest = dir.path().join("wowlua.json");
    let text = fs::read_to_string(&manifest).unwrap();
    fs::write(
        &manifest,
        text.replacen('{', r#"{ "test": { "blizzard-ui": true },"#, 1),
    )
    .unwrap();
    let out = wowlua(dir.path())
        .arg("test")
        .env("WOWLUA_UI_SOURCE", &source)
        .output()
        .unwrap();
    let text = format!(
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    Some((out.status.success(), text))
}

/// In-game regression (spec emulator §17.8): SuuUI stood Blizzard's XP and
/// reputation bar containers down by reparenting them under a hidden frame;
/// opening Edit Mode then raised a Lua error (SuuUI's changelog:
/// `StatusTrackingManager.lua:228`), as the container asks its parent,
/// which it takes for `StatusTrackingBarManager`, for `CheckForLayoutChange`.
/// The repro fails the same way; SuuUI's fix (the containers kept under
/// the manager, the manager under the hidden frame) opens and closes Edit
/// Mode without an error.
#[test]
fn suuui_reparented_status_bars_break_edit_mode() {
    let Some((ok, text)) = blizzard_ui_files(&[
        (
            "tests/reparented.test.wowlua",
            r#"import test from "wowlua:test"
import * as mock from "wowlua:mock"

-- SuuUI before its fix (minimal repro): Blizzard's XP and reputation bar
-- containers stood down as any other replaced frame, their events off,
-- hidden and reparented under a hidden frame.
@test("reparented status-bar containers, then Edit Mode")
local function reparented()
    mock.nextFrame(60)
    local hidden = CreateFrame("Frame", nil, UIParent)
    hidden:Hide()
    StatusTrackingBarManager:UnregisterAllEvents()
    for _, name in ipairs({ "MainStatusTrackingBarContainer", "SecondaryStatusTrackingBarContainer" }) do
        local container = (_G :: any)[name]
        container:UnregisterAllEvents()
        container:Hide()
        container:SetParent(hidden)
    end
    -- The user opens Edit Mode from the game menu.
    mock.pressBinding("TOGGLEGAMEMENU")
    mock.nextFrame(5)
    local button: any = nil
    for _, b in ipairs((_G :: any).GameMenuFrame.buttons) do
        if b:GetText() == HUD_EDIT_MODE_MENU then button = b end
    end
    mock.click(button)
    mock.nextFrame(10)
end
"#,
        ),
        (
            "tests/kept.test.wowlua",
            r#"import test, expect from "wowlua:test"
import * as mock from "wowlua:mock"

local function editModeButton(): any
    mock.pressBinding("TOGGLEGAMEMENU")
    mock.nextFrame(5)
    for _, b in ipairs((_G :: any).GameMenuFrame.buttons) do
        if b:GetText() == HUD_EDIT_MODE_MENU then return b end
    end
    return nil
end

-- SuuUI's fix: the containers stay under StatusTrackingBarManager (Edit
-- Mode asks their parent for the layout), only their events off; the
-- manager goes under the hidden frame, which hides them with it.
@test("containers kept under the manager, Edit Mode opened and closed")
local function kept()
    mock.nextFrame(60)
    local hidden = CreateFrame("Frame", nil, UIParent)
    hidden:Hide()
    for _, name in ipairs({ "MainStatusTrackingBarContainer", "SecondaryStatusTrackingBarContainer" }) do
        local container = (_G :: any)[name]
        container:UnregisterAllEvents()
    end
    StatusTrackingBarManager:UnregisterAllEvents()
    StatusTrackingBarManager:Hide()
    StatusTrackingBarManager:SetParent(hidden)
    mock.click(editModeButton())
    mock.nextFrame(10)
    expect(EditModeManagerFrame).toBeShown()
    expect(EditModeManagerFrame:IsEditModeActive()).toBe(true)
    expect(MainStatusTrackingBarContainer:GetParent()).toBe(StatusTrackingBarManager)
    expect(MainStatusTrackingBarContainer:IsVisible()).toBe(false)
    mock.pressKey("ESCAPE")
    mock.nextFrame(10)
    expect(EditModeManagerFrame).never.toBeShown()
    expect(#mock.errors()).toBe(0)
end
"#,
        ),
    ]) else {
        return;
    };
    assert!(!ok, "{text}");
    assert!(
        text.contains("an error reached the error handler: ...d_StatusTrackingBar/Shared/StatusTrackingManager.lua:228: attempt to call a nil value"),
        "{text}"
    );
    assert!(text.contains("1 passed, 1 failed"), "{text}");
    assert!(text.contains("PASS tests/kept.test.wowlua"), "{text}");
}

/// In-game regression (spec emulator §17.8, the taint met in combat):
/// SuuUI's bag button called Blizzard's `ToggleAllBags`, whose code wrote
/// the bag frames' fields as SuuUI's (its taint probe found
/// `ContainerFrame1.size`, `Items`, `AddSlotsButton`); in combat Escape,
/// which closes the bags first, read them and Blizzard's panel manager
/// would not close the spells window ("Interface action failed", no
/// `ADDON_ACTION_BLOCKED`). The repro shows the same; SuuUI's fix (its bags
/// keep their own state through hooks, never calling Blizzard's bag
/// functions) leaves Escape free to close it.
#[test]
fn suuui_bag_calls_taint_escape_in_combat() {
    let Some((ok, text)) = blizzard_ui_files(&[
        (
            "tests/called.test.wowlua",
            r#"import test, expect from "wowlua:test"
import * as mock from "wowlua:mock"

local function chatHas(text: string): boolean
    local frame = (_G :: any).ChatFrame1
    for i = 1, frame:GetNumMessages() do
        local line = frame:GetMessageInfo(i)
        if type(line) == "string" and line:find(text, 1, true) then return true end
    end
    return false
end

-- SuuUI before its fix (minimal repro): its micro menu's bag button
-- called Blizzard's `ToggleAllBags`, whose code then wrote the bag
-- frames' fields (`size`, `Items`, `AddSlotsButton`) as the addon's.
@test("the addon's ToggleAllBags, then Escape in combat")
local function called()
    mock.setBag(1, { slots = 10 })
    mock.nextFrame(60)
    ToggleAllBags()
    mock.nextFrame(5)
    expect(issecurevariable(ContainerFrame2, "Items")).toBe(false)
    -- The user opens the spellbook, a fight starts, the user presses Escape.
    mock.pressBinding("TOGGLESPELLBOOK")
    mock.nextFrame(5)
    expect(PlayerSpellsFrame).toBeShown()
    mock.player:enterCombat()
    mock.nextFrame(5)
    mock.pressKey("ESCAPE")
    mock.nextFrame(5)
    -- As in the game: Escape closed the bags first, reading their fields
    -- tainted it, and Blizzard's panel manager refused to close the
    -- window ("Interface action failed", no ADDON_ACTION_BLOCKED).
    expect(PlayerSpellsFrame).toBeShown()
    expect(chatHas(INTERFACE_ACTION_BLOCKED)).toBe(true)
    expect(#mock.refusals()).toBe(0)
end
"#,
        ),
        (
            "tests/own.test.wowlua",
            r#"import test, expect from "wowlua:test"
import * as mock from "wowlua:mock"

local function chatHas(text: string): boolean
    local frame = (_G :: any).ChatFrame1
    for i = 1, frame:GetNumMessages() do
        local line = frame:GetMessageInfo(i)
        if type(line) == "string" and line:find(text, 1, true) then return true end
    end
    return false
end

-- SuuUI's fix: its bags keep their own open state, following Blizzard's
-- bag calls through hooks, and never call them.
@test("the addon's own bag state, then Escape in combat")
local function own()
    mock.setBag(1, { slots = 10 })
    mock.nextFrame(60)
    local bags = CreateFrame("Frame", nil, UIParent)
    bags:Hide()
    hooksecurefunc("ToggleAllBags", function() bags:SetShown(not bags:IsShown()) end)
    -- The addon's bag button opens its own window only.
    bags:Show()
    -- The user's bag key opens Blizzard's bags too.
    mock.pressBinding("OPENALLBAGS")
    mock.nextFrame(5)
    expect(ContainerFrame2).toBeShown()
    expect(issecurevariable(ContainerFrame2, "Items")).toBe(true)
    mock.pressBinding("TOGGLESPELLBOOK")
    mock.nextFrame(5)
    expect(PlayerSpellsFrame).toBeShown()
    mock.player:enterCombat()
    mock.nextFrame(5)
    mock.pressKey("ESCAPE")
    mock.nextFrame(5)
    expect(PlayerSpellsFrame).never.toBeShown()
    expect(ContainerFrame2).never.toBeShown()
    expect(chatHas(INTERFACE_ACTION_BLOCKED)).toBe(false)
end
"#,
        ),
    ]) else {
        return;
    };
    assert!(ok, "{text}");
    assert!(text.contains("2 passed, 0 failed"), "{text}");
}

// --------------------------------------------- SuuUI's test findings (0.9.1)

/// `spawnUnit`'s declared `healthMax` and `powerMax` set the maximums, and the
/// types know every unit field and method the engine has: a strict test
/// uses them (decision T232).
#[test]
fn unit_data_and_methods_as_declared() {
    let dir = project(
        r#""camelot": {}"#,
        &[
            (
                "src/main.wowlua",
                "print(\"loaded\")
",
            ),
            (
                "tests/units.test.wowlua",
                r##"--!strict
import test, expect from "wowlua:test"
import * as mock from "wowlua:mock"

@test("the declared fields")
local function fields()
    local wolf = mock.spawnUnit({ name = "Wolf", reaction = "friendly", health = 800, healthMax = 1000, power = 5, powerMax = 50,
        classification = "elite", creatureType = "Beast", creatureTypeID = 1, creatureFamily = "Wolf", creatureFamilyID = 1 })
    mock.player:target(wolf)
    expect(UnitHealthMax("target")).toBe(mock.secret(1000))
    expect(UnitPowerMax("target")).toBe(mock.secret(50))
    expect(UnitClassification("target")).toBe("elite")
    expect(UnitCreatureFamily("target")).toBe("Wolf")
    local old = mock.spawnUnit({ name = "Old", reaction = "friendly", health = 10, maxHealth = 20, maxPower = 30 })
    mock.player:target(old)
    expect(UnitHealthMax("target")).toBe(mock.secret(20))
    expect(UnitPowerMax("target")).toBe(mock.secret(30))
    local other = mock.spawnUnit({ name = "Otherplayer", player = true, factionGroup = "Alliance", pvp = true })
    mock.player:target(other)
    expect(UnitFactionGroup("target")).toBe("Alliance")
    expect(UnitIsPVP("target")).toBe(true)
end

@test("the declared methods")
local function methods()
    local other = mock.spawnUnit({ name = "Otherplayer", player = true, level = 60, reaction = "friendly" })
    other:setDistance(3)
    other:setInspect({ itemLevel = 50, items = { [1] = { link = "|cnIQ1:|Hitem:6948::::::::60:::::::::|h[Hearthstone]|h|r", cached = true } } })
    mock.player:target(other)
    NotifyInspect("target")
    mock.advanceTime(2)
    expect(GetInventoryItemID("target", 1)).toBe(6948)
    local wolf = mock.spawnUnit({ name = "Wolf", reaction = "hostile" })
    wolf:setThreat(5)
    wolf:setThreat(nil)
end
"##,
            ),
        ],
    );
    let (ok, text) = test(dir.path(), &[]);
    assert!(ok, "{text}");
    assert!(text.contains("2 passed, 0 failed"), "{text}");
}

/// `mock.setField` sets any field, `nil` too, until the test ends and puts it
/// back secure, where an assignment from the test leaves it tainted
/// (decision T230).
#[test]
fn set_field_puts_the_field_back_secure() {
    let dir = project(
        r#""camelot": {}"#,
        &[
            (
                "src/main.wowlua",
                "print(\"loaded\")
",
            ),
            (
                "tests/fields.test.wowlua",
                r##"import test, expect from "wowlua:test"
import * as mock from "wowlua:mock"

@test("taken away")
local function away()
    mock.setField(_G, "GetNumGroupMembers", nil)
    expect(rawget(_G, "GetNumGroupMembers")).toBeNil()
    mock.setField(C_Timer, "NewTimer", 5)
    expect((C_Timer :: any).NewTimer).toBe(5)
end

@test("back, secure")
local function back()
    expect(type(GetNumGroupMembers)).toBe("function")
    expect("GetNumGroupMembers").toBeSecure()
    expect(type(C_Timer.NewTimer)).toBe("function")
    expect(C_Timer).toBeSecure("NewTimer")
end

@test("assigned back instead")
local function assigned()
    local G: any = _G
    local saved = G.GetTime
    G.GetTime = nil
    G.GetTime = saved
end

@test("tainted after the assignment")
local function taintedAfter()
    expect("GetTime").never.toBeSecure()
end

@test("not a table")
local function notTable()
    expect(function() mock.setField(1 :: any, "x", 1) end).toThrow("mock.setField: a table expected, got number")
end
"##,
            ),
        ],
    );
    let (ok, text) = test(dir.path(), &[]);
    assert!(ok, "{text}");
    assert!(text.contains("5 passed, 0 failed"), "{text}");
}

/// SuuUI's test findings: the mock library and the matchers (R2, R5, R6,
/// R7, R8, R11, R12): a fake's global restored secure, `mock.errors()` per
/// test, leaving a group, secrets compared by their values, a matcher's
/// message for a value that is not a frame, `UNIT_NAME_UPDATE` from
/// `setName`, a stub on a widget.
#[test]
fn suuui_findings_mock_library() {
    let dir = project(
        r#""camelot": {}"#,
        &[
            ("src/main.wowlua", "print(\"loaded\")\n"),
            (
                "tests/mock.test.wowlua",
                r#"import test, expect from "wowlua:test"
import * as mock from "wowlua:mock"

@test("a fake")
local function faked()
    mock.fake("GetNumGroupMembers", function(): number return 3 end)
    expect(GetNumGroupMembers()).toBe(3)
end

@test("restored secure")
local function restored()
    expect("GetNumGroupMembers").toBeSecure()
end

@test("an error")
local function anError()
    local f = CreateFrame("Frame")
    f:SetScript("OnEvent", function() error("boom") end)
    f:RegisterEvent("PLAYER_REGEN_DISABLED")
    mock.fireEvent("PLAYER_REGEN_DISABLED")
    expect(#mock.errors()).toBe(1)
end

@test("the errors are the test's")
local function ownErrors()
    expect(#mock.errors()).toBe(0)
end

@test("leaving the group")
local function leaving()
    local friend = mock.spawnUnit({ name = "Friend", player = true })
    mock.group:add(friend)
    expect(IsInGroup()).toBe(true)
    C_PartyInfo.LeaveParty()
    mock.advanceTime(1)
    expect(IsInGroup()).toBe(false)
    mock.group:add(friend)
    mock.group:remove(friend)
    expect(IsInGroup()).toBe(false)
end

@test("secrets by their values")
local function secrets()
    local s = mock.spy()
    s("player", mock.secret(true))
    expect(s).toHaveBeenCalledWith("player", mock.secret(true))
    expect(s).never.toHaveBeenCalledWith("player", mock.secret(false))
end

@test("not a frame")
local function notAFrame()
    local t: { Frame } = {}
    expect(t[1]).never.toBeShown()
end

@test("a name change")
local function nameChange()
    local seen: string[] = {}
    local f = CreateFrame("Frame")
    f:RegisterEvent("UNIT_NAME_UPDATE")
    f:SetScript("OnEvent", function(_f: Frame, _e: string, unit: string) seen[#seen + 1] = unit end)
    mock.player:setName("Other")
    mock.player:setName("Again", nil, { quiet = true })
    expect(seen).toEqual({ "player" })
    expect((UnitName("player"))).toBe("Again")
end

@test("a widget's stub")
local function widgetStub()
    local b = CreateFrame("Button", nil, UIParent)
    local s = mock.stub(b, "SetText")
    b:SetText("x")
    expect(s).toHaveBeenCalledWith(b, "x")
end

@test("a call that leaves out an optional argument")
local function leftOut()
    local s = mock.spy(function(_name: string, _member: string?) end)
    s("guildInvite")
    expect(s).toHaveBeenCalledWith("guildInvite")
end
"#,
            ),
            (
                "tests/unselected.test.wowlua",
                "error(\"loaded though no filter selects it\")\n",
            ),
        ],
    );
    // Filtered: a file no filter can select is not loaded.
    let (ok, text) = test(dir.path(), &["mock.test"]);
    assert!(!ok, "{text}");
    assert!(!text.contains("loaded though"), "{text}");
    assert!(text.contains("9 passed, 1 failed"), "{text}");
    let (ok, text) = test(dir.path(), &[]);
    assert!(!ok, "{text}");
    assert!(text.contains("loaded though"), "{text}");
    assert!(text.contains("9 passed, 1 failed"), "{text}");
    assert!(text.contains("✗ not a frame"), "{text}");
    assert!(text.contains("expected a frame, got nil"), "{text}");
}

/// SuuUI's test findings: the emulated client (R4, R9, V1, V2, V4, V5, V6,
/// V8, V9): clicks as registered, a ticker due at an advance's end, sizes
/// that follow anchors, `SetAlphaFromBoolean`, `OnVerticalScroll`, scales
/// and alphas ignoring the parent's, an atlas gone with a file, a secret
/// text's width.
#[test]
fn suuui_findings_emulated_client() {
    let dir = project(
        r#""camelot": {}"#,
        &[
            ("src/main.wowlua", "print(\"loaded\")\n"),
            (
                "tests/client.test.wowlua",
                r#"import test, expect from "wowlua:test"
import * as mock from "wowlua:mock"
import every from "wowlua:timer"

@test("clicks as registered")
local function clicks()
    local b = CreateFrame("Button", nil, UIParent)
    b:SetSize(20, 20); b:SetPoint("CENTER")
    local downs: any[] = {}
    b:SetScript("OnClick", function(_b: Button, button: string, down: boolean) downs[#downs + 1] = `{button} {down}` end)
    mock.click(b)
    mock.click(b, "RightButton")
    b:RegisterForClicks("AnyDown", "AnyUp")
    mock.click(b, "RightButton")
    expect(downs).toEqual({ "LeftButton false", "RightButton true", "RightButton false" })
end

@test("a clock moved by fractions")
local function fractions()
    for _ = 1, 37 do mock.advanceTime(0.1) end
    mock.advanceTime(0.033)
end

@test("a tick due at the advance's end")
local function tickAtEnd()
    local n = 0
    every(1, function() n += 1 end)
    mock.advanceTime(1)
    mock.advanceTime(1)
    expect(n).toBe(2)
end

@test("a size that follows the anchors")
local function anchoredSize()
    local holder = CreateFrame("Frame", nil, UIParent); holder:SetPoint("TOPLEFT"); holder:SetSize(100, 100)
    local inner = CreateFrame("Frame", nil, holder)
    local sizes: number[] = {}
    inner:SetScript("OnSizeChanged", function(_f: Frame, w: number) sizes[#sizes + 1] = w end)
    inner:SetAllPoints(); mock.advanceFrames(1)
    holder:SetWidth(150); mock.advanceFrames(1)
    mock.advanceFrames(1)
    expect(sizes).toEqual({ 100, 150 })
end

@test("alpha from a boolean")
local function alphaFromBoolean()
    local f = CreateFrame("Frame")
    f:SetAlphaFromBoolean(true, 0, 1)
    expect(f:GetAlpha()).toBe(0)
    f:SetAlphaFromBoolean(false)
    expect(f:GetAlpha()).toBe(0)
    f:SetAlphaFromBoolean(true)
    expect(f:GetAlpha()).toBe(1)
    f:SetAlphaFromBoolean(mock.secret(false), 0, 1)
    expect(issecretvalue(f:GetAlpha())).toBe(true)
end

@test("a scroll runs OnVerticalScroll")
local function scrolls()
    local s = CreateFrame("ScrollFrame", nil, UIParent); s:SetSize(200, 100)
    local c = CreateFrame("Frame", nil, s); c:SetSize(200, 400); s:SetScrollChild(c)
    local offsets: number[] = {}
    s:HookScript("OnVerticalScroll", function(_s: ScrollFrame, offset: number) offsets[#offsets + 1] = offset end)
    s:SetVerticalScroll(50)
    s:SetVerticalScroll(50)
    expect(offsets).toEqual({ 50 })
    expect(s:GetVerticalScroll()).toBe(50)
end

@test("ignoring the parent's scale and alpha")
local function ignoring()
    local outer = CreateFrame("Frame", nil, UIParent); outer:SetScale(2)
    local inner = CreateFrame("Frame", nil, outer)
    inner:SetIgnoreParentScale(true)
    inner:SetIgnoreParentAlpha(true)
    expect(inner:IsIgnoringParentScale()).toBe(true)
    expect(inner:IsIgnoringParentAlpha()).toBe(true)
    expect(inner:GetEffectiveScale()).toBe(1)
end

@test("a file clears the atlas")
local function atlasCleared()
    local t = CreateFrame("Frame"):CreateTexture()
    t:SetAtlas("ui-hud-minus")
    t:SetTexture("Interface\\Buttons\\UI-PlusButton-Up")
    expect((t :: any):GetAtlas()).toBe(nil)
    local b = CreateFrame("Button", nil, UIParent)
    b:SetNormalAtlas("ui-hud-minus")
    b:SetNormalTexture("Interface\\Buttons\\UI-PlusButton-Up")
    expect((b:GetNormalTexture() :: any):GetAtlas()).toBe(nil)
end

@test("a secret text's width")
local function secretWidth()
    local fs = CreateFrame("Frame", nil, UIParent):CreateFontString(nil, "OVERLAY")
    fs:SetFont("Fonts/FRIZQT__.TTF", 12, "")
    fs:SetText(mock.secret("Hidden"))
    expect(issecretvalue(fs:GetUnboundedStringWidth())).toBe(true)
    fs:SetText("Shown")
    expect(issecretvalue(fs:GetUnboundedStringWidth())).toBe(false)
end
"#,
            ),
        ],
    );
    let (ok, text) = test(dir.path(), &[]);
    assert!(ok, "{text}");
    assert!(text.contains("9 passed, 0 failed"), "{text}");
}

/// SuuUI's test findings: the runner and the checker (R1, R13, K1, K2,
/// K3): a test file that does not compile is left out and the others run
/// (a filtered run only notes it), several filters, `wowlua check` checks
/// the test side, `@cases` typed by the parameters, a local only tests
/// read through `import internal` is not unused, and a misplaced
/// annotation list is E2060.
#[test]
fn suuui_findings_runner_and_checker() {
    let dir = project(
        r#""camelot": {}"#,
        &[
            (
                "src/main.wowlua",
                "local hidden = 1\nlocal unused = 2\nexport function f(): number return 1 end\n",
            ),
            (
                "tests/a.test.wowlua",
                r#"import test, expect, cases from "wowlua:test"
import internal hidden from "/main"

type D = { state: "idle" | "results" }

@test("cases typed by the parameters")
@cases({ { { state = "idle" } } })
local function typed(d: D) expect(d.state).toBe("idle") end

@test("an internal read")
local function internalRead() expect(hidden).toBe(1) end
"#,
            ),
            (
                "tests/b.test.wowlua",
                r#"import test, expect from "wowlua:test"
@test("b runs")
local function b() expect(1).toBe(1) end
"#,
            ),
            (
                "tests/broken.test.wowlua",
                r#"import test from "wowlua:test"
@test("broken")
local function broken()
    local a, b: any, any = 1, 2
    print(a, b)
end
"#,
            ),
        ],
    );
    let (ok, text) = test(dir.path(), &[]);
    assert!(!ok, "{text}");
    assert!(text.contains("error[E2060]"), "{text}");
    assert!(text.contains("FAIL tests/broken.test.wowlua"), "{text}");
    assert!(text.contains("it does not compile"), "{text}");
    assert!(text.contains("3 passed, 0 failed"), "{text}");
    assert!(text.contains("unused local `unused`"), "{text}");
    assert!(!text.contains("unused local `hidden`"), "{text}");
    assert!(!text.contains("aborting"), "{text}");
    // Filtered: the broken file only noted; several filters.
    let (ok, text) = test(dir.path(), &["a.test", "b runs"]);
    assert!(ok, "{text}");
    assert!(text.contains("3 passed, 0 failed"), "{text}");
    assert!(text.contains("Skipped tests/broken.test.wowlua"), "{text}");
    // `wowlua check` checks the test side.
    let out = wowlua(dir.path()).arg("check").output().unwrap();
    let text = String::from_utf8_lossy(&out.stderr).into_owned();
    assert!(!out.status.success(), "{text}");
    assert!(text.contains("error[E2060]"), "{text}");
    assert!(!text.contains("unused local `hidden`"), "{text}");
}

/// SuuUI's test findings with Blizzard's UI (R4, V11): a secure action
/// button registered for down and up clicks acts on the down click
/// (`ActionButtonUseKeyDown`, on by default), and a menu's submenu opens
/// on hover once frames pass (the menu's mouse timer is an `OnUpdate`;
/// `mock.advanceTime` runs no frames).
#[test]
fn suuui_findings_with_blizzard_ui() {
    let Some((ok, text)) = blizzard_ui_files(&[(
        "tests/ui.test.wowlua",
        r#"import test, expect from "wowlua:test"
import * as mock from "wowlua:mock"

local function rowWithText(text: string): any
    for _, f in ipairs(mock.frames()) do
        local fr: any = f
        if not fr:IsForbidden() and fr:IsVisible() then
            for _, r in ipairs({ fr:GetRegions() }) do
                local reg: any = r
                if reg:GetObjectType() == "FontString" and reg:GetText() == text then return fr end
            end
        end
    end
    return nil
end

@test("a secure button acting on the key down")
local function keyDown()
    local b = CreateFrame("Button", nil, UIParent, "SecureActionButtonTemplate")
    b:RegisterForClicks("AnyDown", "AnyUp")
    b:SetAttribute("type", "click"); b:SetAttribute("clickbutton", (_G :: any).CharacterMicroButton)
    b:SetSize(20, 20); b:SetPoint("CENTER"); b:Show()
    mock.click(b)
    expect((_G :: any).CharacterFrame).toBeShown()
end

@test("a submenu on hover")
local function submenu()
    local owner = CreateFrame("Frame", nil, UIParent); owner:SetSize(10, 10); owner:SetPoint("CENTER")
    ;(_G :: any).MenuUtil.CreateContextMenu(owner, function(_o: any, root: any)
        local parent = root:CreateButton("Parent")
        parent:CreateButton("Child", function() end)
    end)
    mock.hover(rowWithText("Parent"))
    mock.advanceFrames(30)
    expect(rowWithText("Child") ~= nil).toBe(true)
end

@test("a Shift-click is a chat link: CHATLINK's SHIFT-BUTTON1 by the click's button")
local function chatLink()
    local b = CreateFrame("Button", nil, UIParent)
    b:SetSize(20, 20); b:SetPoint("CENTER")
    b:RegisterForClicks("AnyUp")
    local seen: any[] = {}
    b:SetScript("OnClick", function(_b: Button, button: string) seen[#seen + 1] = `{button} {IsModifiedClick("CHATLINK")}` end)
    expect(GetModifiedClick("CHATLINK")).toBe("SHIFT-BUTTON1")
    mock.click(b)
    mock.setModifiers({ shift = true })
    mock.click(b)
    mock.click(b, "RightButton")
    mock.setModifiers({ shift = false })
    expect(seen).toEqual({ "LeftButton false", "LeftButton true", "RightButton false" })
end
"#,
    )]) else {
        return;
    };
    assert!(ok, "{text}");
    assert!(text.contains("3 passed, 0 failed"), "{text}");
}

/// A world profile's collections (decision T235): the mount journal, the
/// toy box and the wardrobe answer from its `mounts`, `toys` and
/// `appearances`, typed for strict tests, and stay drivable.
#[test]
fn world_profile_collections() {
    let profile = r#"{
  "items": { "801": { "name": "Toy Drum", "icon": 134001, "quality": 1 } },
  "mounts": { "11": { "name": "Brown Horse", "spellID": 458, "icon": 132261, "favorite": true }, "12": { "name": "Zebra", "spellID": 459, "collected": false } },
  "toys": { "801": { "favorite": true } },
  "appearances": { "collected": [21598], "favorites": [11257] }
}"#;
    let dir = project(
        r#""camelot": {}"#,
        &[
            ("src/main.wowlua", "print(\"loaded\")\n"),
            ("tests/profiles/collector.json", profile),
            (
                "tests/collections.test.wowlua",
                r#"import test, expect, config from "wowlua:test"
import * as mock from "wowlua:mock"
config({ profile = "collector" })

@test("the profile's collections")
local function collections()
    expect(C_MountJournal.GetNumMounts()).toBe(2)
    expect(C_MountJournal.GetNumDisplayedMounts()).toBe(1)
    expect((C_MountJournal.GetDisplayedMountInfo(1))).toBe("Brown Horse")
    expect(select(11, C_MountJournal.GetMountInfoByID(12))).toBe(false)
    expect(PlayerHasToy(801)).toBe(true)
    expect(select(2, C_ToyBox.GetToyInfo(801))).toBe("Toy Drum")
    expect(C_ToyBox.GetIsFavorite(801)).toBe(true)
    expect(C_TransmogCollection.PlayerKnowsSource(21598)).toBe(true)
    expect(C_TransmogCollection.GetIsAppearanceFavorite(11257)).toBe(true)
end

@test("changed during the test")
local function changed()
    mock.setMounts({ [20] = { name = "Swift Ram", spellID = 460, usable = false } })
    mock.setToys({ [801] = { collected = false } })
    mock.setAppearances({ collected = {} })
    expect(C_MountJournal.GetNumMounts()).toBe(1)
    expect(select(5, C_MountJournal.GetMountInfoByID(20))).toBe(false)
    expect(PlayerHasToy(801)).toBe(false)
    expect(C_TransmogCollection.PlayerKnowsSource(21598)).toBe(false)
end
"#,
            ),
        ],
    );
    let (ok, out) = test(dir.path(), &[]);
    assert!(ok, "{out}");
    assert!(out.contains("2 passed, 0 failed"), "{out}");
}

/// The machine the client runs on (decision T236): the manifest's
/// `test.platform` for every file, a file's `config({ platform })` over it.
#[test]
fn test_platform_from_manifest_and_config() {
    let dir = project(
        r#""camelot": {}"#,
        &[
            ("src/main.wowlua", "print(\"loaded\")\n"),
            (
                "tests/windows.test.wowlua",
                r#"import test, expect from "wowlua:test"

@test("the manifest's platform")
local function windows()
    expect(IsWindowsClient()).toBe(true)
    expect(IsMacClient()).toBe(false)
end
"#,
            ),
            (
                "tests/mac.test.wowlua",
                r#"import test, expect, config from "wowlua:test"
config({ platform = "mac" })

@test("a file's own platform")
local function mac()
    expect(IsMacClient()).toBe(true)
end
"#,
            ),
        ],
    );
    let manifest = r#"{ "package": { "name": "demo", "version": "1.0.0" }, "flavors": { "camelot": {} },
  "addon": { "title": "Demo" }, "test": { "platform": "windows" } }"#;
    fs::write(dir.path().join("wowlua.json"), manifest).unwrap();
    let (ok, out) = test(dir.path(), &[]);
    assert!(ok, "{out}");
    assert!(out.contains("2 passed, 0 failed"), "{out}");
}

/// The screen the client draws on (decision T237): the manifest's
/// `test.screen`, a file's `config({ screen })` over it; UIParent's size,
/// the DPI scale and the text's measures follow it (as recorded on
/// Windows at 1600x900 and 2560x1440).
#[test]
fn test_screen_from_manifest_and_config() {
    let dir = project(
        r#""camelot": {}"#,
        &[
            ("src/main.wowlua", "print(\"loaded\")\n"),
            (
                "tests/laptop.test.wowlua",
                r#"import test, expect from "wowlua:test"

@test("the manifest's screen")
local function laptop()
    expect(GetScreenWidth()).toBe(1365.3333740234375)
    expect(GetScreenHeight()).toBe(768)
    expect(GetScreenDPIScale()).toBe(2.4000000953674316)
    local w, h = GetPhysicalScreenSize()
    expect(w).toBe(1600)
    local f = UIParent:CreateFontString()
    f:SetFont("Fonts\FRIZQT__.TTF", 12, "")
    f:SetText("Hello World")
    -- at 14 pixels: its advances and the last glyph's width, 85 pixels
    expect(f:GetUnboundedStringWidth()).toBe(72.533332824707031)
    expect(f:GetStringHeight()).toBe(11.946666717529297)
end
"#,
            ),
            (
                "tests/large.test.wowlua",
                r#"import test, expect, config from "wowlua:test"
config({ screen = { width = 2560, height = 1440, dpiScale = 1.5 } })

@test("a file's own screen")
local function large()
    expect(GetScreenWidth()).toBe(1706.666748046875)
    expect(GetScreenHeight()).toBe(960)
end
"#,
            ),
        ],
    );
    let manifest = r#"{ "package": { "name": "demo", "version": "1.0.0" }, "flavors": { "camelot": {} },
  "addon": { "title": "Demo" }, "test": { "screen": { "width": 1600, "height": 900, "dpiScale": 2.4 } } }"#;
    fs::write(dir.path().join("wowlua.json"), manifest).unwrap();
    let (ok, out) = test(dir.path(), &[]);
    assert!(ok, "{out}");
    assert!(out.contains("2 passed, 0 failed"), "{out}");
}

/// A world profile's `screen` (decision T482): over the manifest's
/// `test.screen`, under a file's `config({ screen })`; its UI scale turns
/// "Use UI Scale" on, as the client's CVars read.
#[test]
fn test_screen_from_the_world_profile() {
    let dir = project(
        r#""camelot": {}"#,
        &[
            ("src/main.wowlua", "print(\"loaded\")\n"),
            (
                "tests/profiles/big.json",
                r#"{ "screen": { "width": 2560, "height": 1440, "uiScale": 0.8 } }"#,
            ),
            (
                "tests/big.test.wowlua",
                r#"import test, expect, config from "wowlua:test"
config({ profile = "big" })

@test("the profile's screen")
local function big()
    local w, h = GetPhysicalScreenSize()
    expect(w).toBe(2560)
    expect(UIParent:GetScale()).toBe(0.80000001192092896)
    expect(C_CVar.GetCVar("useUiScale")).toBe("1")
end
"#,
            ),
            (
                "tests/own.test.wowlua",
                r#"import test, expect, config from "wowlua:test"
config({ profile = "big", screen = { width = 1280, height = 720 } })

@test("a file's own screen over its profile's")
local function own()
    local w, h = GetPhysicalScreenSize()
    expect(w).toBe(1280)
end
"#,
            ),
            (
                "tests/manifest.test.wowlua",
                r#"import test, expect from "wowlua:test"

@test("the manifest's screen without a profile")
local function manifest()
    local w, h = GetPhysicalScreenSize()
    expect(w).toBe(1600)
end
"#,
            ),
        ],
    );
    let manifest = r#"{ "package": { "name": "demo", "version": "1.0.0" }, "flavors": { "camelot": {} },
  "addon": { "title": "Demo" }, "test": { "screen": { "width": 1600, "height": 900 } } }"#;
    fs::write(dir.path().join("wowlua.json"), manifest).unwrap();
    let (ok, out) = test(dir.path(), &[]);
    assert!(ok, "{out}");
    assert!(out.contains("3 passed, 0 failed"), "{out}");
}

/// A collection while a test waits (a timer's, between its steps) keeps
/// the running test: only the runner held it, and the next step read a
/// freed table ("stale handle").
#[test]
fn a_collection_while_a_test_waits_keeps_it() {
    let dir = project(
        r#""camelot": {}"#,
        &[(
            "tests/gc.test.wowlua",
            r#"
import test, expect from "wowlua:test"
import * as mock from "wowlua:mock"

@test("waits through collections")
local function waits()
    local seen: number[] = {}
    for i = 1, 5 do
        C_Timer.After(0.1, function() collectgarbage("collect") end)
        mock.waitSeconds(0.2)
        seen[#seen + 1] = i
    end
    expect(#seen).toBe(5)
end
"#,
        )],
    );
    let (ok, out) = test(dir.path(), &[]);
    assert!(ok, "{out}");
}

/// Code examples in doc comments run as tests (decision T222): named after
/// what they document, reported as their source's, a failure and a type
/// error at the comment's line; `norun` ones and other languages left
/// alone; `path::name` runs one.
#[test]
fn doc_comment_examples_run_as_tests() {
    let src = "--- Adds two numbers.\n---\n--- ```wowlua\n--- expect(add(1, 2)).toBe(3)\n--- ```\n---\n--- ```wowlua\n--- expect(add(2, 2)).toBe(5)\n--- ```\n---\n--- ```lua\n--- add(\"not run\")\n--- ```\nexport function add(a: number, b: number): number\n    return a + b\nend\n\n--- ```wowlua norun\n--- double(\"x\")\n--- ```\nexport function double(n: number): number\n    return n * 2\nend\n";
    let dir = project(r#""standard": {}"#, &[("src/math.wowlua", src)]);
    let (ok, out) = test(dir.path(), &[]);
    assert!(!ok, "{out}");
    assert!(out.contains("src/math.wowlua"), "{out}");
    assert!(out.contains("✓ add example\n"), "{out}");
    assert!(out.contains("✗ add example 2"), "{out}");
    assert!(out.contains("at src/math.wowlua:8"), "{out}");
    assert!(!out.contains("double example"), "{out}");
    // one, exactly
    let (_, out) = test(dir.path(), &["src/math.wowlua::add example"]);
    assert!(out.contains("1 passed, 0 failed"), "{out}");
    // a type error: at the comment's line and column; the other tests run
    std::fs::write(
        dir.path().join("src/math.wowlua"),
        src.replace("expect(add(2, 2)).toBe(5)", "local s: string = add(2, 2)"),
    )
    .unwrap();
    let (ok, out) = test(dir.path(), &[]);
    assert!(!ok, "{out}");
    assert!(out.contains("src/math.wowlua:8:11"), "{out}");
    assert!(out.contains("does not compile"), "{out}");
}

/// `wowlua data pull` (spec CLI §12.1, decision T203) reads a flavor's game
/// data into wowlua's cache, and `wowlua test --game-data` answers from it
/// without the installation: an item the test does not define has the
/// client's name. Skipped without a WoW installation with camelot's client.
#[test]
fn pulled_game_data_answers_game_data_tests() {
    let install = std::env::var_os("WOWLUA_WOW_PATH")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| "/Applications/World of Warcraft".into());
    if !install.join("_forever_").is_dir() {
        eprintln!("no WoW installation with camelot's client: skipped");
        return;
    }
    let dir = project(
        "\"camelot\": {}",
        &[
            ("src/main.wowlua", "print(1)\n"),
            (
                "tests/item.test.wowlua",
                "import test, expect from \"wowlua:test\"\n@test(\"the client's item\")\nlocal function clientItem()\n    expect(C_Item.GetItemNameByID(277714)).toBe(\"Spectral Bear Cub\")\nend\n",
            ),
        ],
    );
    let nowhere = dir.path().join("no-installation");
    // Without pulled data or an installation the item is unknown.
    wowlua(dir.path())
        .env("WOWLUA_WOW_PATH", &nowhere)
        .args(["test", "--game-data"])
        .assert()
        .failure()
        .stderr(predicates::str::contains("no game data pulled for camelot"));
    wowlua(dir.path())
        .args(["data", "pull", "--wow"])
        .arg(&install)
        .assert()
        .success()
        .stderr(predicates::str::contains("Pulled camelot game data"));
    assert!(
        dir.path()
            .join(".wowlua-home/data/camelot/.pulled")
            .is_file()
    );
    wowlua(dir.path())
        .env("WOWLUA_WOW_PATH", &nowhere)
        .args(["test", "--game-data"])
        .assert()
        .success();
    // Without `--game-data` a test sees only what it defines.
    wowlua(dir.path())
        .env("WOWLUA_WOW_PATH", &nowhere)
        .arg("test")
        .assert()
        .failure();
}

/// `wowlua data pull` names what it cannot read.
#[test]
fn data_pull_reports_a_missing_installation_or_flavor() {
    let dir = project("\"camelot\": {}", &[("src/main.wowlua", "print(1)\n")]);
    wowlua(dir.path())
        .args(["data", "pull", "--wow"])
        .arg(dir.path().join("nowhere"))
        .assert()
        .failure()
        .stderr(predicates::str::contains("does not exist"));
    wowlua(dir.path())
        .args(["data", "pull", "--flavor", "nope", "--wow"])
        .arg(dir.path())
        .assert()
        .failure()
        .stderr(predicates::str::contains("unknown flavor `nope`"));
}

/// `@bench` functions (decision T221) are run by `wowlua bench`, in the
/// build the profile optimizes and without optimizations, after the file's
/// `@beforeAll` hooks and with `@beforeEach` and `@afterEach` around each;
/// `wowlua test` leaves them out. A benchmark that fails fails the run.
#[test]
fn bench_runs_benchmarks_that_test_leaves_out() {
    let dir = project(
        r#""standard": {}"#,
        &[
            ("src/tracker.wowlua", TRACKER),
            (
                "tests/tracker.test.wowlua",
                r#"
import test, bench, beforeAll, beforeEach, expect, skip from "wowlua:test"
import Tracker, double from "/tracker"

local tracker: Tracker
local setUp = 0

@beforeAll
local function setup()
    tracker = Tracker.create()
end

@beforeEach
local function each()
    setUp = setUp + 1
end

@test("doubles")
local function doubles()
    expect(double(2)).toBe(4)
end

@bench("records kills")
local function records()
    for _ = 1, 10 do
        tracker:record(2)
    end
end

@bench
local function doubling()
    double(setUp)
end

@bench("not yet")
@skip("too slow")
local function slow()
    error("never runs")
end
"#,
            ),
        ],
    );
    let (ok, out) = test(dir.path(), &[]);
    assert!(ok, "{out}");
    assert!(out.contains("doubles"), "{out}");
    assert!(!out.contains("records kills"), "{out}");
    let bench = |args: &[&str]| {
        let out = wowlua(dir.path())
            .arg("bench")
            .args(["--samples", "2", "--batch-ms", "1"])
            .args(args)
            .output()
            .unwrap();
        (
            out.status.success(),
            String::from_utf8_lossy(&out.stdout).into_owned(),
            String::from_utf8_lossy(&out.stderr).into_owned(),
        )
    };
    let (ok, stdout, stderr) = bench(&["--json"]);
    assert!(ok, "{stdout}{stderr}");
    let json: serde_json::Value = serde_json::from_str(&stdout).expect("JSON");
    assert_eq!(json["profile"], "release");
    let benches = json["files"][0]["benches"].as_array().unwrap();
    let names: Vec<&str> = benches
        .iter()
        .map(|b| b["name"].as_str().unwrap())
        .collect();
    assert_eq!(names, ["records kills", "doubling", "not yet"]);
    for b in &benches[..2] {
        assert!(b["unoptimized"]["nanos"].as_f64().unwrap() > 0.0, "{b}");
        assert!(b["optimized"]["nanos"].as_f64().unwrap() > 0.0, "{b}");
        assert!(b["speedup"].as_f64().unwrap() > 0.0, "{b}");
        assert!(b["calls"].as_u64().unwrap() >= 1, "{b}");
    }
    assert_eq!(benches[2]["skipped"], "too slow");
    // The table, one benchmark by a filter.
    let (ok, stdout, stderr) = bench(&["doubling"]);
    assert!(ok, "{stdout}{stderr}");
    assert!(
        stdout.contains("tests/tracker.test.wowlua (standard)"),
        "{stdout}"
    );
    assert!(stdout.contains("doubling"), "{stdout}");
    assert!(!stdout.contains("records kills"), "{stdout}");
    assert!(
        stdout.contains("unoptimized") && stdout.contains("release"),
        "{stdout}"
    );
    // A benchmark that fails fails the run, with its error.
    fs::write(
        dir.path().join("tests/broken.test.wowlua"),
        "import bench from \"wowlua:test\"\n@bench(\"breaks\")\nlocal function breaks()\n    error(\"broken benchmark\")\nend\n",
    )
    .unwrap();
    let (ok, stdout, _) = bench(&["breaks"]);
    assert!(!ok, "{stdout}");
    assert!(stdout.contains("broken benchmark"), "{stdout}");
    // An unknown profile is E4006.
    let (ok, _, stderr) = bench(&["--profile", "nope"]);
    assert!(!ok);
    assert!(stderr.contains("E4006"), "{stderr}");
}
