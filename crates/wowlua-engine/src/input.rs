//! The user's input (spec Tooling §16.1.4, the roadmap's GUI tests): the
//! mouse over a frame (`OnEnter`, `OnLeave`, `GetMouseFoci`, `IsMouseOver`),
//! clicks, an edit box's keyboard focus and typing (`OnChar`,
//! `OnTextChanged` with `userInput`), and keys a focused edit box takes.
//!
//! Verified against the client (`/stdtest engine input`, Forever 70425):
//! `OnEnter(true)` and `OnLeave(true)`; a click's scripts (`OnMouseDown`,
//! `OnMouseUp(button, true)`, then `PreClick`, `OnClick`, `PostClick` with
//! `(button, false)` for an enabled button); `OnChar(char)` then
//! `OnTextChanged(true)` per typed character; `MODIFIER_STATE_CHANGED`
//! with `LSHIFT` and 1 / 0; a click on an edit box focusing it; the focus
//! scripts' `userInput`, true while the user's action runs (also for a
//! `ClearFocus` in `OnEnterPressed`); `GLOBAL_MOUSE_DOWN` before the
//! click's scripts and `GLOBAL_MOUSE_UP` after them, an edit box losing the
//! focus first to a click elsewhere; a slider's thumb dragged (`slide`).
//! Keys (`/stdtest engine keys`): see `press_key`.

use wowlua_vm::stdlib::Ret;
use wowlua_vm::{NativeCall, NativeFn, Value, Vm};

use crate::widgets::{self, MethodFn, WidgetId};
use crate::{engine, engine_mut};

/// Where the mouse and the keyboard are.
#[derive(Default)]
pub struct Input {
    /// The frame under the mouse.
    pub mouse_focus: Option<WidgetId>,
    /// The edit box with the keyboard focus.
    pub keyboard_focus: Option<WidgetId>,
    /// Shift, Ctrl and Alt held (the left keys).
    pub modifiers: [bool; 3],
    /// The user's actions running (a click, typing, a key): scripts' and
    /// focus changes' `userInput`.
    pub user_actions: u32,
    /// Modifier keys held that a keyboard frame kept: its `OnKeyUp` comes
    /// with their release.
    pub held_by: Vec<(String, WidgetId)>,
    /// Where the mouse is on the screen (`GetCursorPosition`, in the
    /// screen's units: a frame's position times its effective scale).
    pub cursor: (f64, f64),
    /// The mouse is on the screen's frames: put there by `setCursorPosition`,
    /// `hover`, a click; off them all at first and after `hover(nil)`
    /// (`IsMouseOver` false everywhere).
    pub placed: bool,
    /// The mouse button of the user's click running (`"LeftButton"`, …):
    /// a modified click's `BUTTONn` part (`IsModifiedClick`).
    pub clicking: Option<String>,
    /// Keys held down (`key_down`): each with the binding it ran.
    pub held: Vec<(String, String)>,
    /// The mouse held down while playing (decision T535).
    pub play: Option<crate::play::Press>,
}

/// `setCursorPosition(x, y)`: the mouse moved there (screen units, as
/// `GetCursorPosition` answers; a frame's point times its effective
/// scale). No events: what is under it is `hover`'s.
pub fn control_set_cursor_position(vm: &mut Vm, c: NativeCall) -> Ret {
    let x = vm.check_number(&c, 1)?;
    let y = vm.check_number(&c, 2)?;
    let input = &mut engine_mut(vm).world.input;
    input.cursor = (x, y);
    input.placed = true;
    Ok(vec![])
}

/// Whether the mouse is over the widget's rectangle grown by the offsets
/// (top, bottom, left, right: in the widget's units, a positive one moving
/// that edge up or right). A widget without points has no rectangle.
fn mouse_inside(vm: &Vm, id: WidgetId, off: [f64; 4]) -> bool {
    let e = engine(vm);
    if !e.world.input.placed || e.world.widgets[id.0].points.is_empty() {
        return false;
    }
    let Some((l, b, w, h)) = widgets::bounds(vm, id, 0) else {
        return false;
    };
    let s = widgets::layout_scale(e, id);
    let ui = widgets::ui_parent_scale(e);
    let (x, y) = (e.world.input.cursor.0 / ui, e.world.input.cursor.1 / ui);
    let [top, bottom, left, right] = off;
    x >= l + left * s && x <= l + w + right * s && y >= b + bottom * s && y <= b + h + top * s
}

/// The cursor onto the widget's middle unless it is over it already (no
/// events; a widget without a rectangle leaves it where it is).
pub fn cursor_onto(vm: &mut Vm, id: WidgetId) {
    if mouse_inside(vm, id, [0.0; 4]) || engine(vm).world.widgets[id.0].points.is_empty() {
        return;
    }
    if let Some((l, b, w, h)) = widgets::bounds(vm, id, 0) {
        let ui = widgets::ui_parent_scale(engine(vm));
        let input = &mut engine_mut(vm).world.input;
        input.cursor = ((l + w / 2.0) * ui, (b + h / 2.0) * ui);
        input.placed = true;
    }
}

/// The mouse moved onto `new` (`None`: off every frame): onto its middle
/// unless it is over it already; `OnLeave` for the frame it left, then
/// `OnEnter` for the new one (`motion` true).
pub(crate) fn mouse_onto(vm: &mut Vm, new: Option<WidgetId>) {
    match new {
        Some(id) => cursor_onto(vm, id),
        None => engine_mut(vm).world.input.placed = false,
    }
    let old = engine(vm).world.input.mouse_focus;
    if old == new {
        return;
    }
    engine_mut(vm).world.input.mouse_focus = new;
    if let Some(o) = old {
        widgets::run_script(vm, o, "OnLeave", &[Value::Bool(true)]);
    }
    if let Some(n) = new {
        widgets::run_script(vm, n, "OnEnter", &[Value::Bool(true)]);
    }
}

/// `GetCursorPosition()`: the mouse's place (`setCursorPosition`; the
/// screen's corner, 0, 0, until it is set).
fn get_cursor_position(vm: &mut Vm, _c: NativeCall) -> Ret {
    let (x, y) = engine(vm).world.input.cursor;
    Ok(vec![Value::Num(x), Value::Num(y)])
}

/// Runs `f` as the user's action.
pub fn as_user<T>(vm: &mut Vm, f: impl FnOnce(&mut Vm) -> T) -> T {
    engine_mut(vm).world.input.user_actions += 1;
    let out = f(vm);
    engine_mut(vm).world.input.user_actions -= 1;
    out
}

const MODIFIERS: [(&str, &str); 3] = [("shift", "LSHIFT"), ("ctrl", "LCTRL"), ("alt", "LALT")];

/// `setModifiers({ shift?, ctrl?, alt? })`: the keys held (the left ones);
/// `MODIFIER_STATE_CHANGED(key, 1 | 0)` for each that changes.
pub fn control_set_modifiers(vm: &mut Vm, c: NativeCall) -> Ret {
    let Value::Table(t) = c.arg(1) else {
        return Err(vm.error("mock.setModifiers: a table of the keys held expected"));
    };
    for (i, (field, key)) in MODIFIERS.iter().enumerate() {
        let k = vm.str(field);
        let held = vm.heap.table(t).get(k).truthy();
        if engine(vm).world.input.modifiers[i] != held {
            engine_mut(vm).world.input.modifiers[i] = held;
            let name = vm.str(key);
            crate::widgets::fire_event(
                vm,
                "MODIFIER_STATE_CHANGED",
                &[name, Value::Num(if held { 1.0 } else { 0.0 })],
            );
            // The key itself, pressed: to the keyboard frames as any key
            // (`/stdtest engine keys`: `OnKeyDown("LSHIFT")`; unverified:
            // after the event, and its release to the frame that kept it).
            if held && engine(vm).world.input.keyboard_focus.is_none() {
                as_user(vm, |vm| {
                    if let Some(id) = key_down_frames(vm, key) {
                        engine_mut(vm)
                            .world
                            .input
                            .held_by
                            .push((key.to_string(), id));
                    }
                });
            } else if !held {
                let kept = engine(vm)
                    .world
                    .input
                    .held_by
                    .iter()
                    .position(|(k, _)| k == key);
                if let Some(at) = kept {
                    let (_, id) = engine_mut(vm).world.input.held_by.remove(at);
                    let name = vm.str(key);
                    as_user(vm, |vm| widgets::run_script(vm, id, "OnKeyUp", &[name]));
                }
            }
        }
    }
    Ok(vec![])
}

fn held(vm: &Vm, i: usize) -> Ret {
    Ok(vec![Value::Bool(engine(vm).world.input.modifiers[i])])
}

fn is_shift_key_down(vm: &mut Vm, _c: NativeCall) -> Ret {
    held(vm, 0)
}

fn is_control_key_down(vm: &mut Vm, _c: NativeCall) -> Ret {
    held(vm, 1)
}

fn is_alt_key_down(vm: &mut Vm, _c: NativeCall) -> Ret {
    held(vm, 2)
}

fn is_modifier_key_down(vm: &mut Vm, _c: NativeCall) -> Ret {
    Ok(vec![Value::Bool(
        engine(vm).world.input.modifiers.iter().any(|m| *m),
    )])
}

fn right_key_down(_vm: &mut Vm, _c: NativeCall) -> Ret {
    Ok(vec![Value::Bool(false)])
}

fn widget_arg(
    vm: &mut Vm,
    c: &NativeCall,
    n: usize,
    what: &str,
) -> Result<WidgetId, wowlua_vm::LuaError> {
    widgets::widget_of(vm, c.arg(n)).ok_or_else(|| vm.error(&format!("{what}: a frame expected")))
}

fn label(vm: &Vm, id: WidgetId) -> String {
    let w = &engine(vm).world.widgets[id.0];
    match &w.name {
        Some(n) => n.clone(),
        None => format!("an unnamed {}", w.class),
    }
}

/// `hover(frame)` (`nil`: away): the mouse onto it (`mouse_onto`).
pub fn control_hover(vm: &mut Vm, c: NativeCall) -> Ret {
    let new = widgets::widget_of(vm, c.arg(1));
    if !c.arg(1).is_nil() && new.is_none() {
        return Err(vm.error("mock.hover: a frame or nil expected"));
    }
    mouse_onto(vm, new);
    Ok(vec![])
}

/// `click(frame, button)`: the user's click on a visible frame: the mouse
/// over it (`OnEnter`), `GLOBAL_MOUSE_DOWN`, `OnMouseDown`,
/// `GLOBAL_MOUSE_UP`, `OnMouseUp`, then a button's click scripts unless it
/// is disabled.
pub fn control_click(vm: &mut Vm, c: NativeCall) -> Ret {
    let id = widget_arg(vm, &c, 1, "mock.click")?;
    if !widgets::visible(engine(vm), id) {
        let name = label(vm, id);
        return Err(vm.error(&format!("mock.click: {name} is not visible")));
    }
    let button = match c.arg(2) {
        Value::Str(s) => Value::Str(s),
        _ => vm.str("LeftButton"),
    };
    // The mouse is over what it clicks.
    mouse_onto(vm, Some(id));
    as_user(vm, |vm| {
        let class = engine(vm).world.widgets[id.0].class.clone();
        let is = |vm: &Vm, kind: &str| {
            widgets::lineage(engine(vm), &class)
                .iter()
                .any(|c| c.eq_ignore_ascii_case(kind))
        };
        let enabled = engine(vm).world.widgets[id.0].enabled;
        let edit_box = is(vm, "EditBox") && enabled;
        // A click elsewhere takes the focus from an edit box, first.
        if !edit_box {
            set_keyboard_focus(vm, None);
        }
        let name = match vm.hidden(button) {
            Value::Str(s) => String::from_utf8_lossy(vm.str_bytes(s)).into_owned(),
            _ => String::new(),
        };
        let outer = engine_mut(vm).world.input.clicking.replace(name.clone());
        // An enabled button's click, down or up as it registered for
        // clicks (`RegisterForClicks`: by default the left button's up
        // click; a secure action button acting on the key down,
        // `ActionButtonUseKeyDown`, registers for both). A check button's
        // state toggles before its scripts run (as the mail's inbox reads
        // it in `OnClick`). Unverified: the down click's place after
        // `OnMouseDown`.
        let clicked = |vm: &mut Vm, down: bool| {
            if !(is(vm, "Button") && enabled) || !widgets::registered_click(vm, id, &name, down) {
                return;
            }
            if is(vm, "CheckButton") {
                let props = engine(vm).world.widgets[id.0].props;
                let checked = widgets::stored_args(vm, props, "Checked")
                    .and_then(|v| v.first().copied())
                    .is_some_and(|v| v.truthy());
                widgets::store(vm, id, "Checked", &[Value::Bool(!checked)]);
            }
            for script in ["PreClick", "OnClick", "PostClick"] {
                widgets::run_script(vm, id, script, &[button, Value::Bool(down)]);
            }
        };
        widgets::fire_event(vm, "GLOBAL_MOUSE_DOWN", &[button]);
        // (the top-level frames it is in come to the front: T586)
        crate::widgetops::raise_clicked(vm, id);
        widgets::run_script(vm, id, "OnMouseDown", &[button]);
        if edit_box {
            user_focus(vm, id);
        }
        clicked(vm, true);
        widgets::run_script(vm, id, "OnMouseUp", &[button, Value::Bool(true)]);
        clicked(vm, false);
        widgets::fire_event(vm, "GLOBAL_MOUSE_UP", &[button]);
        engine_mut(vm).world.input.clicking = outer;
    });
    Ok(vec![])
}

/// `slide(slider, value)`: the slider's thumb dragged to `value` as the
/// user drags it: the mouse down on the thumb (`OnValueChanged(value,
/// true)` for where it is, then `OnMouseDown`), the thumb moved
/// (`OnValueChanged(value, true)`, clamped, snapped only when it obeys
/// its step on drag), the mouse up (`OnMouseUp`), the global mouse events
/// around (`/stdtest engine input`).
pub fn control_slide(vm: &mut Vm, c: NativeCall) -> Ret {
    let id = widget_arg(vm, &c, 1, "mock.slide")?;
    let to = vm.check_number(&c, 2)?;
    if !widgets::visible(engine(vm), id) {
        let name = label(vm, id);
        return Err(vm.error(&format!("mock.slide: {name} is not visible")));
    }
    let class = engine(vm).world.widgets[id.0].class.clone();
    if !widgets::is_a(engine(vm), &class, "Slider") {
        let name = label(vm, id);
        return Err(vm.error(&format!("mock.slide: {name} is not a slider")));
    }
    let left = vm.str("LeftButton");
    mouse_onto(vm, Some(id));
    as_user(vm, |vm| {
        set_keyboard_focus(vm, None);
        widgets::fire_event(vm, "GLOBAL_MOUSE_DOWN", &[left]);
        crate::bars::drag(vm, id, None);
        widgets::run_script(vm, id, "OnMouseDown", &[left]);
        crate::bars::drag(vm, id, Some(to));
        crate::bars::end_drag(vm, id);
        widgets::run_script(vm, id, "OnMouseUp", &[left, Value::Bool(true)]);
        widgets::fire_event(vm, "GLOBAL_MOUSE_UP", &[left]);
    });
    Ok(vec![])
}

/// The keyboard focus let go once its edit box is out of sight (hidden,
/// or a parent hidden or changed): keys reach the game again, as in the
/// client (decision T244; unverified: `OnEditFocusLost` after `OnHide`).
pub fn drop_hidden_focus(vm: &mut Vm) {
    if let Some(id) = engine(vm).world.input.keyboard_focus
        && !widgets::visible(engine(vm), id)
    {
        set_keyboard_focus(vm, None);
    }
}

fn set_keyboard_focus(vm: &mut Vm, new: Option<WidgetId>) {
    let old = engine(vm).world.input.keyboard_focus;
    if old == new {
        return;
    }
    engine_mut(vm).world.input.keyboard_focus = new;
    let user = Value::Bool(engine(vm).world.input.user_actions > 0);
    if let Some(o) = old {
        widgets::run_script(vm, o, "OnEditFocusLost", &[user]);
    }
    if let Some(n) = new {
        widgets::run_script(vm, n, "OnEditFocusGained", &[user]);
    }
}

/// `type(editBox, text)`: focuses the edit box, then types each character:
/// in place of the selection, else at the cursor (`widgetops::insert`),
/// `OnChar(char)`, `OnTextChanged(true)`.
pub fn control_type(vm: &mut Vm, c: NativeCall) -> Ret {
    let id = widget_arg(vm, &c, 1, "mock.type")?;
    let typed = vm.check_str(&c, 2)?;
    let typed = String::from_utf8_lossy(vm.str_bytes(typed)).into_owned();
    if !widgets::visible(engine(vm), id) {
        let name = label(vm, id);
        return Err(vm.error(&format!("mock.type: {name} is not visible")));
    }
    as_user(vm, |vm| {
        user_focus(vm, id);
        for ch in typed.chars() {
            crate::widgetops::insert(vm, id, &ch.to_string());
            let ch = vm.str(&ch.to_string());
            widgets::run_script(vm, id, "OnChar", &[ch]);
            widgets::run_script(vm, id, "OnTextChanged", &[Value::Bool(true)]);
        }
    });
    Ok(vec![])
}

/// Text the user types while playing (decision T535): into the edit box
/// with the keyboard focus, each character's `OnChar` and
/// `OnTextChanged`; nothing without one.
pub fn type_text(vm: &mut Vm, text: &str) {
    let Some(id) = engine(vm).world.input.keyboard_focus else {
        return;
    };
    as_user(vm, |vm| {
        for ch in text.chars() {
            crate::widgetops::insert(vm, id, &ch.to_string());
            let ch = vm.str(&ch.to_string());
            widgets::run_script(vm, id, "OnChar", &[ch]);
            widgets::run_script(vm, id, "OnTextChanged", &[Value::Bool(true)]);
        }
    });
}

/// A key the focused edit box takes, whether it took it: every key while
/// it has the focus (unverified: typing into chat runs no binding),
/// `ENTER`, `ESCAPE`, `TAB` and `SPACE` running its scripts, the arrows
/// as `widgetops::arrow` has them (with the Alt arrow mode, only with Alt
/// held).
pub fn edit_box_key(vm: &mut Vm, key: &str) -> bool {
    let Some(id) = engine(vm).world.input.keyboard_focus else {
        return false;
    };
    if matches!(key, "UP" | "DOWN" | "LEFT" | "RIGHT") {
        return as_user(vm, |vm| crate::widgetops::arrow(vm, id, key));
    }
    // (Backspace and Delete edit the text, decision T535)
    if matches!(key, "BACKSPACE" | "DELETE") {
        as_user(vm, |vm| {
            if crate::widgetops::erase(vm, id, key == "DELETE") {
                widgets::run_script(vm, id, "OnTextChanged", &[Value::Bool(true)]);
            }
        });
        return true;
    }
    let script = match key {
        "ENTER" => "OnEnterPressed",
        "ESCAPE" => "OnEscapePressed",
        "TAB" => "OnTabPressed",
        "SPACE" => "OnSpacePressed",
        _ => return true,
    };
    as_user(vm, |vm| widgets::run_script(vm, id, script, &[]));
    true
}

/// The shown frames taking keys (`EnableKeyboard`, or a key script),
/// the top first: by strata, then level, then the later made. Edit boxes
/// take keys by their focus alone (the chat's box, shown with a key
/// script, keeps no key from the bindings).
fn keyboard_frames(vm: &mut Vm) -> Vec<WidgetId> {
    let mut frames: Vec<(usize, i64, usize)> = Vec::new();
    // Only a widget a keyboard setting was stored for can take keys
    // (`World::keyboard`); the stem interned as when every widget but an
    // edit box was looked at, at the first (nothing here collects it).
    let e = engine(vm);
    if !e
        .world
        .widgets
        .iter()
        .any(|w| !widgets::is_a(e, &w.class, "EditBox"))
    {
        return Vec::new();
    }
    let key = vm.str("KeyboardEnabled");
    let keyboard = engine(vm).world.keyboard.clone();
    for i in keyboard {
        let id = WidgetId(i);
        let e = engine(vm);
        let w = &e.world.widgets[i];
        if widgets::is_a(e, &w.class, "EditBox") {
            continue;
        }
        let props = w.props;
        let on = widgets::stored_args_at(vm, props, key)
            .and_then(|v| v.first().copied())
            .is_some_and(|v| v.truthy());
        if on && widgets::visible(engine(vm), id) {
            let rank = widgets::strata_rank(vm, id);
            let level = widgets::frame_level(vm, id) as i64;
            frames.push((rank, level, i));
        }
    }
    frames.sort_by(|a, b| b.cmp(a));
    frames.into_iter().map(|(_, _, i)| WidgetId(i)).collect()
}

/// Whether a frame has an `OnKeyDown` script or hook.
fn has_key_handler(vm: &mut Vm, id: WidgetId) -> bool {
    let (scripts, hooks) = {
        let w = &engine(vm).world.widgets[id.0];
        (w.scripts, w.hooks)
    };
    let key = vm.heap.intern(b"OnKeyDown");
    !vm.heap.table(scripts).get_str(key).is_nil() || !vm.heap.table(hooks).get_str(key).is_nil()
}

fn propagates(vm: &mut Vm, id: WidgetId) -> bool {
    let props = engine(vm).world.widgets[id.0].props;
    widgets::stored_args(vm, props, "PropagateKeyboardInput")
        .and_then(|v| v.first().copied())
        .is_some_and(|v| v.truthy())
}

/// The modifier keys themselves (no chord of their own).
const MODIFIER_KEYS: &[&str] = &[
    "LSHIFT", "RSHIFT", "LCTRL", "RCTRL", "LALT", "RALT", "LMETA", "RMETA",
];

/// The key with the modifiers held, as bindings name it (`ALT-CTRL-SHIFT-`
/// order, Blizzard's `CreateKeyChordStringUsingMetaKeyState`).
fn chord(vm: &Vm, key: &str) -> String {
    if MODIFIER_KEYS.contains(&key) {
        return key.to_string();
    }
    let [shift, ctrl, alt] = engine(vm).world.input.modifiers;
    let mut out = String::new();
    for (held, name) in [(alt, "ALT-"), (ctrl, "CTRL-"), (shift, "SHIFT-")] {
        if held {
            out.push_str(name);
        }
    }
    out.push_str(key);
    out
}

/// A key pressed on the keyboard frames, the top first: `OnKeyDown(key)`
/// until one does not pass it on; that one, which then has the key.
fn key_down_frames(vm: &mut Vm, key: &str) -> Option<WidgetId> {
    let k = vm.str(key);
    for id in keyboard_frames(vm) {
        // A frame without a key handler lets keys by (`KeybindListener`
        // stays keyboard-enabled after it listened).
        if !has_key_handler(vm, id) {
            continue;
        }
        widgets::run_script(vm, id, "OnKeyDown", &[k]);
        if !propagates(vm, id) {
            return Some(id);
        }
    }
    None
}

/// A key pressed and released (as the user): the focused edit box takes
/// it; else each keyboard frame, the top first, runs `OnKeyDown(key)`
/// until one does not pass it on: that one has the key (its `OnKeyUp`
/// alone runs, and no binding). A key no frame kept runs its chord's
/// command (`chord`; an unbound chord the bare key's), down then up, and
/// no frame's `OnKeyUp`. A propagation turned off in the handler counts
/// for that key; a frame without a key handler lets keys by.
/// (`/stdtest engine keys`, Forever 70425.)
pub fn press_key(vm: &mut Vm, key: &str) {
    if edit_box_key(vm, key) {
        return;
    }
    if let Some(id) = key_down_frames(vm, key) {
        let k = vm.str(key);
        widgets::run_script(vm, id, "OnKeyUp", &[k]);
        return;
    }
    let chord = chord(vm, key);
    let command = crate::bindings::action_for(vm, &chord).or_else(|| {
        (chord != key)
            .then(|| crate::bindings::action_for(vm, key))
            .flatten()
    });
    if let Some(command) = &command {
        crate::bindings::run_command(vm, command, "down");
        crate::bindings::run_command(vm, command, "up");
    }
}

/// A key held down (decision T516): its binding's down now, its up when
/// `key_up` lets it go, as holding a movement key moves the player.
pub fn key_down(vm: &mut Vm, key: &str) {
    if edit_box_key(vm, key) {
        return;
    }
    let chord = chord(vm, key);
    let command = crate::bindings::action_for(vm, &chord).or_else(|| {
        (chord != key)
            .then(|| crate::bindings::action_for(vm, key))
            .flatten()
    });
    if let Some(command) = command {
        crate::bindings::run_command(vm, &command, "down");
        engine_mut(vm)
            .world
            .input
            .held
            .push((key.to_owned(), command));
    }
}

/// A key held down let go: its binding's up.
pub fn key_up(vm: &mut Vm, key: &str) {
    let held = &mut engine_mut(vm).world.input.held;
    let Some(i) = held.iter().position(|(k, _)| k == key) else {
        return;
    };
    let (_, command) = held.remove(i);
    crate::bindings::run_command(vm, &command, "up");
}

/// The user clicks into an edit box: after its text (the engine's click
/// is at the box's middle; unverified where the client puts the cursor),
/// so the cursor goes to the end, nothing selected, before
/// `OnEditFocusGained` (which may select). One with the focus keeps both.
fn user_focus(vm: &mut Vm, id: WidgetId) {
    if engine(vm).world.input.keyboard_focus != Some(id) {
        crate::widgetops::text_set(vm, id);
    }
    set_keyboard_focus(vm, Some(id));
}

fn set_focus(vm: &mut Vm, id: WidgetId, _c: NativeCall) -> Ret {
    set_keyboard_focus(vm, Some(id));
    Ok(vec![])
}

fn clear_focus(vm: &mut Vm, id: WidgetId, _c: NativeCall) -> Ret {
    if engine(vm).world.input.keyboard_focus == Some(id) {
        set_keyboard_focus(vm, None);
    }
    Ok(vec![])
}

fn has_focus(vm: &mut Vm, id: WidgetId, _c: NativeCall) -> Ret {
    Ok(vec![Value::Bool(
        engine(vm).world.input.keyboard_focus == Some(id),
    )])
}

/// `IsMouseOver(top, bottom, left, right)`: the mouse over the region's
/// rectangle grown by the offsets (`mouse_inside`), whatever covers it and
/// whether it is shown (unverified).
fn is_mouse_over(vm: &mut Vm, id: WidgetId, c: NativeCall) -> Ret {
    let mut off = [0.0; 4];
    for (i, o) in off.iter_mut().enumerate() {
        if let Value::Num(n) = c.arg(i + 2) {
            *o = n;
        }
    }
    Ok(vec![Value::Bool(mouse_inside(vm, id, off))])
}

fn focus_value(vm: &mut Vm, id: Option<WidgetId>) -> Value {
    match id {
        Some(w) => Value::Table(engine(vm).world.widgets[w.0].table),
        None => Value::Nil,
    }
}

/// `GetMouseFoci()`: the frames under the mouse (here, one or none).
fn get_mouse_foci(vm: &mut Vm, _c: NativeCall) -> Ret {
    let focus = engine(vm).world.input.mouse_focus;
    let t = vm.new_table();
    if focus.is_some() {
        let v = focus_value(vm, focus);
        let _ = vm.heap.table_mut(t).set(Value::Num(1.0), v);
    }
    Ok(vec![Value::Table(t)])
}

fn get_mouse_focus(vm: &mut Vm, _c: NativeCall) -> Ret {
    let focus = engine(vm).world.input.mouse_focus;
    Ok(vec![focus_value(vm, focus)])
}

fn get_current_keyboard_focus(vm: &mut Vm, _c: NativeCall) -> Ret {
    let focus = engine(vm).world.input.keyboard_focus;
    Ok(vec![focus_value(vm, focus)])
}

pub fn behaviours() -> Vec<(&'static str, NativeFn)> {
    vec![
        ("GetCursorPosition", get_cursor_position),
        ("GetMouseFoci", get_mouse_foci),
        ("GetMouseFocus", get_mouse_focus),
        ("GetCurrentKeyBoardFocus", get_current_keyboard_focus),
        ("IsShiftKeyDown", is_shift_key_down),
        ("IsLeftShiftKeyDown", is_shift_key_down),
        ("IsRightShiftKeyDown", right_key_down),
        ("IsControlKeyDown", is_control_key_down),
        ("IsLeftControlKeyDown", is_control_key_down),
        ("IsRightControlKeyDown", right_key_down),
        ("IsAltKeyDown", is_alt_key_down),
        ("IsLeftAltKeyDown", is_alt_key_down),
        ("IsRightAltKeyDown", right_key_down),
        ("IsModifierKeyDown", is_modifier_key_down),
        ("IsMetaKeyDown", right_key_down),
    ]
}

pub fn methods() -> Vec<(&'static str, MethodFn)> {
    vec![
        ("EditBox:SetFocus", set_focus),
        ("EditBox:ClearFocus", clear_focus),
        ("EditBox:HasFocus", has_focus),
        ("IsMouseOver", is_mouse_over),
    ]
}
