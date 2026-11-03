//! Playing (decision T535): the mouse and the keyboard as the game takes
//! them, all at once. The frame under the mouse has it: hovered (`OnEnter`,
//! `OnLeave`), clicked when the button is let go where it was pressed,
//! dragged when it registered for the button's drag, its wheel when it
//! takes the wheel. The 3D world has what no frame covers: the unit under
//! the mouse moused over (`mouseover`, its tooltip), a click targeting it
//! (the right button talking to it), a drag turning the camera (the right
//! button the player too), the wheel zooming. Keys go to the edit box with
//! the keyboard focus, else to the frames taking keys and the bindings
//! (`input::key_down`); text to the focused edit box (`input::type_text`).

use wowlua_vm::{Value, Vm};

use crate::units::UnitId;
use crate::widgets::{self, WidgetId};
use crate::{engine, engine_mut};

/// How far the mouse moves while held before it drags (pixels;
/// unverified).
const SLACK: f64 = 4.0;

/// The mouse held down while playing.
pub struct Press {
    right: bool,
    /// The frame it was pressed on; none: the world.
    frame: Option<WidgetId>,
    /// Where it was pressed, and where it was last (screen pixels).
    from: (f64, f64),
    last: (f64, f64),
    /// It moved beyond a click's slack: no click when let go.
    dragged: bool,
    /// The frame's drag under way.
    dragging: Option<widgets::Dragging>,
}

/// The frame under a point of the screen (pixels from the top left) that
/// takes the mouse, not the world's own.
fn frame_at(vm: &mut Vm, x: f64, y: f64) -> Option<WidgetId> {
    crate::scene::hit_test(vm, x, y)
        .filter(|f| engine(vm).world.widgets[f.0].name.as_deref() != Some("WorldFrame"))
}

/// Pixels of the screen in `GetCursorPosition`'s units (bottom left).
fn screen_units(vm: &Vm, x: f64, y: f64) -> (f64, f64) {
    let (k, ui_h) = crate::scene::pixel_scale(vm);
    let ui = widgets::ui_parent_scale(engine(vm));
    (x / k * ui, (ui_h - y / k) * ui)
}

/// Whether a frame registered for `button`'s drag (`RegisterForDrag`).
fn drags_with(vm: &mut Vm, id: WidgetId, button: &str) -> bool {
    let props = engine(vm).world.widgets[id.0].props;
    widgets::stored_args(vm, props, "RegisterForDrag").is_some_and(|args| {
        args.iter().any(|a| match vm.hidden(*a) {
            Value::Str(s) => String::from_utf8_lossy(vm.str_bytes(s)).eq_ignore_ascii_case(button),
            _ => false,
        })
    })
}

/// The unit under the mouse in the world: `mouseover`, its tooltip shown
/// as the client shows it (Blizzard's `GameTooltip` at its default
/// anchor; unverified) and hidden as the mouse leaves it.
fn mouseover(vm: &mut Vm, unit: Option<UnitId>) {
    if engine(vm).units.mouseover == unit {
        return;
    }
    engine_mut(vm).units.mouseover = unit;
    widgets::fire_event(vm, "UPDATE_MOUSEOVER_UNIT", &[]);
    let src: &[u8] = if unit.is_some() {
        b"if GameTooltip and GameTooltip_SetDefaultAnchor then \
          GameTooltip_SetDefaultAnchor(GameTooltip, UIParent) GameTooltip:SetUnit('mouseover') end"
    } else {
        b"if GameTooltip and GameTooltip:GetOwner() == UIParent then GameTooltip:Hide() end"
    };
    if let Ok(f) = vm.load(src, "=mouseover") {
        let _ = vm.pcall(f, &[]);
    }
}

/// The mouse moved to a point (screen pixels from the top left): while
/// held, a drag (past the slack: the frame's, or the camera turning);
/// else onto the frame there, or over the unit there in the world.
pub fn mouse_move(vm: &mut Vm, x: f64, y: f64) {
    let at = screen_units(vm, x, y);
    {
        let input = &mut engine_mut(vm).world.input;
        input.cursor = at;
        input.placed = true;
    }
    if let Some(mut p) = engine_mut(vm).world.input.play.take() {
        let (dx, dy) = (x - p.last.0, y - p.last.1);
        p.last = (x, y);
        if !p.dragged && (x - p.from.0).hypot(y - p.from.1) > SLACK {
            p.dragged = true;
            let button = if p.right { "RightButton" } else { "LeftButton" };
            if let Some(f) = p.frame
                && drags_with(vm, f, button)
            {
                p.dragging = Some(crate::input::as_user(vm, |vm| {
                    widgets::drag_begin(vm, f, button)
                }));
            }
        }
        if p.dragged {
            match (&p.dragging, p.frame) {
                (Some(d), _) => {
                    let from = screen_units(vm, p.from.0, p.from.1);
                    widgets::drag_move(vm, d, at.0 - from.0, at.1 - from.1);
                }
                (None, None) => crate::camera::look(vm, dx, dy, p.right),
                (None, Some(_)) => {}
            }
        }
        engine_mut(vm).world.input.play = Some(p);
        return;
    }
    let frame = frame_at(vm, x, y);
    crate::input::mouse_onto(vm, frame);
    let unit = match frame {
        Some(_) => None,
        None => crate::camera::unit_at(vm, x, y),
    };
    mouseover(vm, unit);
}

/// A mouse button pressed at a point: held until let go.
pub fn mouse_down(vm: &mut Vm, x: f64, y: f64, right: bool) {
    if engine(vm).world.input.play.is_some() {
        return;
    }
    mouse_move(vm, x, y);
    let frame = frame_at(vm, x, y);
    // (the right button held over the world: the turn keys strafe, T536)
    if right && frame.is_none() {
        engine_mut(vm).world.movement.mouselook = true;
    }
    engine_mut(vm).world.input.play = Some(Press {
        right,
        frame,
        from: (x, y),
        last: (x, y),
        dragged: false,
        dragging: None,
    });
}

/// The mouse button let go: a drag ends (`OnDragStop`); a press that did
/// not move clicks the frame it was on, or in the world targets the unit
/// there, the right button talking to it.
pub fn mouse_up(vm: &mut Vm, x: f64, y: f64, right: bool) {
    let Some(p) = engine_mut(vm).world.input.play.take() else {
        return;
    };
    if p.right != right {
        engine_mut(vm).world.input.play = Some(p);
        return;
    }
    if right {
        engine_mut(vm).world.movement.mouselook = false;
    }
    if let Some(d) = &p.dragging {
        crate::input::as_user(vm, |vm| widgets::drag_end(vm, d));
    }
    if p.dragged {
        mouse_move(vm, x, y);
        return;
    }
    let button = if right { "RightButton" } else { "LeftButton" };
    match p.frame {
        Some(f) => {
            let t = Value::Table(engine(vm).world.widgets[f.0].table);
            let b = vm.str(button);
            let click = vm.native("click", crate::input::control_click);
            let _ = vm.pcall(click, &[t, b]);
        }
        None => {
            if let Some(unit) = crate::camera::unit_at(vm, x, y) {
                crate::input::as_user(vm, |vm| {
                    crate::units::set_player_target(vm, Some(unit));
                    if right {
                        crate::quests::talk_to(vm, unit);
                    }
                });
            }
        }
    }
}

/// The mouse wheel turned at a point (`delta` 1 a notch up): the topmost
/// frame there taking the mouse or the wheel (decision T588), it or the
/// nearest of its parents taking the wheel (`OnMouseWheel`), else the
/// camera zooming in (out for a notch down).
pub fn wheel(vm: &mut Vm, x: f64, y: f64, delta: f64) {
    let mut at = crate::scene::wheel_test(vm, x, y)
        .filter(|f| engine(vm).world.widgets[f.0].name.as_deref() != Some("WorldFrame"));
    while let Some(id) = at {
        let props = engine(vm).world.widgets[id.0].props;
        let takes = widgets::stored_args(vm, props, "MouseWheelEnabled")
            .and_then(|v| v.first().copied())
            .is_some_and(|v| v.truthy());
        if takes {
            crate::input::as_user(vm, |vm| {
                widgets::run_script(vm, id, "OnMouseWheel", &[Value::Num(delta)]);
            });
            return;
        }
        at = engine(vm).world.widgets[id.0].parent;
    }
    crate::camera::wheel(vm, -delta);
}
