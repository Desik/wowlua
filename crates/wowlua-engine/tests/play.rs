//! Playing (decision T535): the mouse and the keyboard as the game takes
//! them, all at once: the frame under the mouse hovered, clicked, dragged
//! and scrolled; the world under it moused over, clicked and turned; text
//! and keys into the focused edit box.

use wowlua_engine::play;
use wowlua_vm::{Value, Vm};

fn vm() -> Vm {
    let mut vm = Vm::wow();
    wowlua_engine::install(&mut vm, "camelot").expect("installs");
    wowlua_engine::controls::install(&mut vm, "mock");
    vm
}

fn run(vm: &mut Vm, src: &str) -> Vec<String> {
    let f = vm.load(src.as_bytes(), "@Test.lua").expect("compiles");
    let text = |vm: &Vm, v: Value| match vm.hidden(v) {
        Value::Str(s) => String::from_utf8_lossy(vm.str_bytes(s)).into_owned(),
        Value::Num(n) => format!("{}", (n * 100.0).round() / 100.0),
        Value::Bool(b) => format!("{b}"),
        Value::Nil => "nil".into(),
        other => format!("{other:?}"),
    };
    match vm.call(f, &[]) {
        Ok(r) => r.into_iter().map(|v| text(vm, v)).collect(),
        Err(e) => panic!("{}", text(vm, e.value)),
    }
}

/// The screen's middle (pixels).
fn middle(vm: &Vm) -> (f64, f64) {
    let s = &wowlua_engine::engine(vm).screen;
    (s.width / 2.0, s.height / 2.0)
}

/// A 100-unit button in the screen's middle, what happens to it in `Log`.
const BUTTON: &str = "Log = {}
local b = CreateFrame('Button', 'Thing', UIParent)
b:SetSize(100, 100)
b:SetPoint('CENTER')
b:RegisterForClicks('AnyUp')
b:SetScript('OnEnter', function() Log[#Log + 1] = 'enter' end)
b:SetScript('OnLeave', function() Log[#Log + 1] = 'leave' end)
b:SetScript('OnClick', function(_, button) Log[#Log + 1] = 'click ' .. button end)
b:SetScript('OnMouseWheel', function(_, d) Log[#Log + 1] = 'wheel ' .. d end)
";

#[test]
fn the_frame_under_the_mouse_is_hovered_clicked_and_scrolled() {
    let mut vm = vm();
    run(&mut vm, BUTTON);
    let (x, y) = middle(&vm);
    play::mouse_move(&mut vm, x, y);
    play::mouse_down(&mut vm, x, y, false);
    play::mouse_up(&mut vm, x, y, false);
    play::mouse_down(&mut vm, x, y, true);
    play::mouse_up(&mut vm, x, y, true);
    play::wheel(&mut vm, x, y, 1.0);
    play::mouse_move(&mut vm, 5.0, 5.0);
    let out = run(&mut vm, "return table.concat(Log, ', ')");
    assert_eq!(
        out,
        ["enter, click LeftButton, click RightButton, wheel 1, leave"]
    );
}

/// A scroll area taking only the wheel over a window taking the mouse
/// (SuuUI's settings navigation, its entries not taking the mouse): the
/// wheel turns it, not the window nor the camera (decision T588).
#[test]
fn the_wheel_finds_a_frame_taking_only_the_wheel() {
    let mut vm = vm();
    run(
        &mut vm,
        "Log = {}
local window = CreateFrame('Frame', 'Window', UIParent)
window:SetSize(300, 300)
window:SetPoint('CENTER')
window:EnableMouse(true)
local nav = CreateFrame('ScrollFrame', nil, window)
nav:SetAllPoints()
nav:EnableMouseWheel(true)
nav:SetScript('OnMouseWheel', function(_, d) Log[#Log + 1] = 'nav ' .. d end)
local entry = CreateFrame('Button', nil, nav)
entry:SetSize(100, 20)
entry:SetPoint('CENTER')",
    );
    let (x, y) = middle(&vm);
    play::wheel(&mut vm, x, y, -1.0);
    let out = run(&mut vm, "return table.concat(Log, ', ')");
    assert_eq!(out, ["nav -1"]);
}

#[test]
fn a_frame_registered_for_dragging_follows_the_mouse() {
    let mut vm = vm();
    run(
        &mut vm,
        "local f = CreateFrame('Frame', 'Mover', UIParent)
f:SetSize(100, 100)
f:SetPoint('CENTER')
f:EnableMouse(true)
f:SetMovable(true)
f:RegisterForDrag('LeftButton')
f:SetScript('OnDragStart', function(self) self:StartMoving() end)
f:SetScript('OnDragStop', function(self) self:StopMovingOrSizing() Stopped = true end)",
    );
    let (x, y) = middle(&vm);
    let before = run(&mut vm, "return Mover:GetLeft()")[0]
        .parse::<f64>()
        .unwrap();
    play::mouse_down(&mut vm, x, y, false);
    play::mouse_move(&mut vm, x + 20.0, y);
    let during = run(&mut vm, "return Mover:GetLeft()")[0]
        .parse::<f64>()
        .unwrap();
    play::mouse_move(&mut vm, x + 60.0, y);
    play::mouse_up(&mut vm, x + 60.0, y, false);
    let out = run(&mut vm, "return Mover:GetLeft(), tostring(Stopped)");
    let after = out[0].parse::<f64>().unwrap();
    // (it moves with the mouse while held, and stays where let go)
    assert!(
        during > before && after > during,
        "{before} {during} {after}"
    );
    assert_eq!(out[1], "true");
}

#[test]
fn typing_goes_into_the_focused_edit_box() {
    let mut vm = vm();
    run(
        &mut vm,
        "local e = CreateFrame('EditBox', 'Box', UIParent)
e:SetSize(200, 30)
e:SetPoint('CENTER')
e:SetAutoFocus(false)
Moved = false
mock.defineBinding('TEST_FORWARD', function() Moved = true end)
SetBinding('W', 'TEST_FORWARD')",
    );
    let (x, y) = middle(&vm);
    play::mouse_down(&mut vm, x, y, false);
    play::mouse_up(&mut vm, x, y, false);
    wowlua_engine::input::type_text(&mut vm, "hiw");
    wowlua_engine::input::key_down(&mut vm, "W");
    wowlua_engine::input::key_up(&mut vm, "W");
    wowlua_engine::input::key_down(&mut vm, "BACKSPACE");
    let out = run(&mut vm, "return Box:GetText(), tostring(Moved)");
    // (the W typed is text, not the binding)
    assert_eq!(out, ["hi", "false"]);
}

#[test]
fn the_world_under_the_mouse_is_moused_over_clicked_and_zoomed() {
    let mut vm = vm();
    run(
        &mut vm,
        "mock.moveTo({ world = { y = 0, x = 0, z = 0 }, facing = 0 })
Wolf = mock.spawnUnit({ name = 'Wolf', reaction = 'hostile', world = { y = 10, x = 0, z = 0 } })
Talked = false",
    );
    // (the wolf: straight ahead, under the screen's middle a little)
    let spot = (0..200)
        .map(|i| f64::from(i) * 5.0)
        .find_map(|y| {
            let (x, _) = middle(&vm);
            (wowlua_engine::camera::unit_at(&mut vm, x, y).is_some()).then_some((x, y))
        })
        .expect("the wolf on the screen");
    play::mouse_move(&mut vm, spot.0, spot.1);
    let over = run(&mut vm, "return (UnitName('mouseover'))");
    play::mouse_down(&mut vm, spot.0, spot.1, false);
    play::mouse_up(&mut vm, spot.0, spot.1, false);
    let zoom = run(&mut vm, "return GetCameraZoom()")[0]
        .parse::<f64>()
        .unwrap();
    play::wheel(&mut vm, spot.0, 5.0, 1.0);
    let nearer = run(&mut vm, "return GetCameraZoom()")[0]
        .parse::<f64>()
        .unwrap();
    let out = run(&mut vm, "return (UnitName('target'))");
    assert_eq!(over, ["Wolf"]);
    assert_eq!(out, ["Wolf"]);
    assert!(nearer < zoom, "{zoom} {nearer}");
}

/// `GetFramerate` answers the frames a second the engine runs at
/// (decision T538): the last second's frames over their time.
#[test]
fn the_framerate_is_the_engines() {
    let mut vm = vm();
    wowlua_engine::clock::advance_frames(&mut vm, 60, 1000.0 / 30.0);
    let slow = run(&mut vm, "return math.floor(GetFramerate() + 0.5)");
    wowlua_engine::clock::advance_frames(&mut vm, 120, 1000.0 / 60.0);
    let fast = run(&mut vm, "return math.floor(GetFramerate() + 0.5)");
    assert_eq!([slow, fast], [["30"], ["60"]]);
}
