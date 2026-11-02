//! The UI requests (spec debugger §18.9, §18.12): the frame tree, finding
//! frames, input as the user gives it, firing events.

use serde_json::{Value as Json, json};
use wowlua_engine::widgets::{self, WidgetId};
use wowlua_vm::{Value, Vm};

use crate::protocol::Input;

/// The widget a name gives: a global's (`MyFrame`), or a path from one
/// (`MyFrame.title`).
pub fn widget_named(vm: &mut Vm, name: &str) -> Option<WidgetId> {
    let path: Vec<String> = name.split('.').map(str::to_string).collect();
    let v = crate::values::lookup_path(vm, &path)?;
    widgets::widget_of(vm, v)
}

/// Whether the chunk a frame was made in is the client's (none: the
/// engine made it) or Blizzard's UI code.
fn blizzards(chunk: Option<&[u8]>) -> bool {
    let Some(chunk) = chunk else {
        return true;
    };
    let name = String::from_utf8_lossy(chunk).to_ascii_lowercase();
    ["blizzard_", "framexml", "sharedxml"]
        .iter()
        .any(|p| name.contains(p))
}

fn node(vm: &Vm, id: WidgetId, hidden: bool, depth: usize) -> Option<Json> {
    let e = wowlua_engine::engine(vm);
    let w = &e.world.widgets[id.0];
    let visible = widgets::visible(e, id);
    if !hidden && !w.shown {
        return None;
    }
    let mut n = json!({
        "id": id.0,
        "type": w.class,
        "shown": w.shown,
        "visible": visible,
    });
    if let Some(name) = &w.name {
        n["name"] = json!(name);
    }
    // Made by the client or by Blizzard's UI code (decision T496).
    if blizzards(w.created.as_ref().map(|(chunk, _)| &chunk[..])) {
        n["blizzard"] = json!(true);
    }
    // Screen pixels from the top left, as screenshots are.
    if let Some(r) = wowlua_engine::scene::rect_of(vm, id) {
        n["rect"] = json!({
            "x": r.x.round(),
            "y": r.y.round(),
            "width": r.w.round(),
            "height": r.h.round(),
        });
    }
    if let Some(Value::Str(s)) = w.text {
        n["text"] = json!(String::from_utf8_lossy(vm.str_bytes(s)));
    }
    if depth < 64 {
        let children: Vec<Json> = w
            .children
            .iter()
            .filter_map(|&c| node(vm, c, hidden, depth + 1))
            .collect();
        if !children.is_empty() {
            n["children"] = json!(children);
        }
    }
    Some(n)
}

/// The frame tree under `root` (UIParent's, and the frames without a
/// parent, by default).
pub fn frame_tree(vm: &mut Vm, root: Option<&str>, hidden: bool) -> Result<Json, String> {
    if let Some(name) = root {
        let id = widget_named(vm, name).ok_or_else(|| format!("no frame {name}"))?;
        return Ok(node(vm, id, true, 0).unwrap_or(Json::Null));
    }
    let e = wowlua_engine::engine(vm);
    let roots: Vec<WidgetId> = (0..e.world.widgets.len())
        .map(WidgetId)
        .filter(|&id| {
            // Frames without a parent: UIParent and WorldFrame (the engine
            // makes them, not `CreateFrame`) and those made without one.
            let w = &e.world.widgets[id.0];
            w.parent.is_none() && (w.frame || widgets::is_a(e, &w.class, "Frame"))
        })
        .collect();
    let list: Vec<Json> = roots
        .into_iter()
        .filter_map(|id| node(vm, id, hidden, 0))
        .collect();
    Ok(json!(list))
}

/// Frames whose name, type or text contains `query` (any case).
pub fn find_frames(vm: &mut Vm, query: &str) -> Json {
    let q = query.to_lowercase();
    let e = wowlua_engine::engine(vm);
    let mut out = Vec::new();
    for (i, w) in e.world.widgets.iter().enumerate() {
        let text = match w.text {
            Some(Value::Str(s)) => String::from_utf8_lossy(vm.str_bytes(s)).into_owned(),
            _ => String::new(),
        };
        let name = w.name.clone().unwrap_or_default();
        if name.to_lowercase().contains(&q)
            || w.class.to_lowercase() == q
            || text.to_lowercase().contains(&q)
        {
            if let Some(mut n) = node(vm, WidgetId(i), true, 64) {
                n.as_object_mut().map(|o| o.remove("children"));
                if let Some(p) = w.parent {
                    n["parent"] = json!(widgets::widget_label(e, p));
                }
                out.push(n);
            }
            if out.len() >= 200 {
                break;
            }
        }
    }
    json!(out)
}

/// The widget an input acts on: by name, or the topmost mouse-enabled
/// visible frame at a point.
fn target(
    vm: &mut Vm,
    frame: &Option<String>,
    x: Option<f64>,
    y: Option<f64>,
) -> Result<Value, String> {
    let id = match (frame, x, y) {
        (Some(name), _, _) => widget_named(vm, name).ok_or_else(|| format!("no frame {name}"))?,
        (None, Some(x), Some(y)) => wowlua_engine::scene::hit_test(vm, x, y)
            .ok_or_else(|| format!("no mouse-enabled frame at {x}, {y}"))?,
        _ => return Err("a frame name or a point".into()),
    };
    Ok(Value::Table(
        wowlua_engine::engine(vm).world.widgets[id.0].table,
    ))
}

fn call_control(
    vm: &mut Vm,
    name: &'static str,
    f: wowlua_vm::NativeFn,
    args: &[Value],
) -> Result<(), String> {
    let native = vm.native(name, f);
    vm.pcall(native, args)
        .map(|_| ())
        .map_err(|e| crate::values::error_text(vm, e.value))
}

/// Input as the user gives it.
pub fn input(vm: &mut Vm, input: &Input) -> Result<(), String> {
    use wowlua_engine::input as inp;
    match input {
        Input::Click {
            frame,
            x,
            y,
            button,
        } => {
            let t = target(vm, frame, *x, *y)?;
            let b = vm.str(button);
            call_control(vm, "click", inp::control_click, &[t, b])
        }
        Input::Hover { frame, x, y } => {
            let t = if frame.is_none() && x.is_none() {
                Value::Nil
            } else {
                target(vm, frame, *x, *y)?
            };
            call_control(vm, "hover", inp::control_hover, &[t])
        }
        Input::Drag { from, to } => {
            let a = target(vm, &Some(from.clone()), None, None)?;
            let b = target(vm, &Some(to.clone()), None, None)?;
            call_control(vm, "dragTo", wowlua_engine::controls::drag_to, &[a, b])
        }
        Input::DragAt { x, y, dx, dy } => {
            let id = wowlua_engine::scene::hit_test(vm, *x, *y)
                .ok_or_else(|| format!("no mouse-enabled frame at {x}, {y}"))?;
            inp::as_user(vm, |vm| widgets::drag(vm, id, *dx, *dy, 1));
            Ok(())
        }
        Input::Type { text } => {
            let focus = wowlua_engine::engine(vm).world.input.keyboard_focus;
            let Some(id) = focus else {
                return Err("no edit box has the keyboard focus (click one first)".into());
            };
            let t = Value::Table(wowlua_engine::engine(vm).world.widgets[id.0].table);
            let s = vm.str(text);
            call_control(vm, "type", inp::control_type, &[t, s])
        }
        Input::Key { key } => {
            inp::as_user(vm, |vm| inp::press_key(vm, key));
            Ok(())
        }
        Input::KeyDown { key } => {
            inp::as_user(vm, |vm| inp::key_down(vm, key));
            Ok(())
        }
        Input::Look { dx, dy, turn } => {
            wowlua_engine::camera::look(vm, *dx, *dy, *turn);
            Ok(())
        }
        Input::Zoom { steps } => {
            wowlua_engine::camera::wheel(vm, *steps);
            Ok(())
        }
        Input::ClickWorld { x, y, right } => {
            // (a frame there takes the click, as in the game; the world
            // only what no frame covers)
            let frame = wowlua_engine::scene::hit_test(vm, *x, *y).filter(|f| {
                wowlua_engine::engine(vm).world.widgets[f.0].name.as_deref() != Some("WorldFrame")
            });
            if frame.is_some() {
                let t = target(vm, &None, Some(*x), Some(*y))?;
                let b = vm.str(if *right { "RightButton" } else { "LeftButton" });
                return call_control(vm, "click", inp::control_click, &[t, b]);
            }
            if let Some(unit) = wowlua_engine::camera::unit_at(vm, *x, *y) {
                inp::as_user(vm, |vm| {
                    wowlua_engine::units::set_player_target(vm, Some(unit));
                    // (the right button talks to it, as in the game)
                    if *right {
                        wowlua_engine::quests::talk_to(vm, unit);
                    }
                });
            }
            Ok(())
        }
        Input::KeyUp { key } => {
            inp::as_user(vm, |vm| inp::key_up(vm, key));
            Ok(())
        }
        Input::MouseMove { x, y } => {
            wowlua_engine::play::mouse_move(vm, *x, *y);
            Ok(())
        }
        Input::MouseDown { x, y, right } => {
            wowlua_engine::play::mouse_down(vm, *x, *y, *right);
            Ok(())
        }
        Input::MouseUp { x, y, right } => {
            wowlua_engine::play::mouse_up(vm, *x, *y, *right);
            Ok(())
        }
        Input::Wheel { x, y, delta } => {
            wowlua_engine::play::wheel(vm, *x, *y, *delta);
            Ok(())
        }
        Input::Text { text } => {
            inp::type_text(vm, text);
            Ok(())
        }
        Input::Slash { text } => {
            let t = vm.str(text);
            call_control(vm, "slash", wowlua_engine::chat::slash_control, &[t])
        }
        Input::Chat {
            text,
            chat_type,
            target,
        } => {
            let send = vm.get_global("SendChatMessage");
            if send.is_nil() {
                return Err("the client has no SendChatMessage".into());
            }
            let args = [
                vm.str(text),
                vm.str(chat_type),
                Value::Nil,
                target.as_deref().map_or(Value::Nil, |t| vm.str(t)),
            ];
            inp::as_user(vm, |vm| vm.pcall(send, &args))
                .map(|_| ())
                .map_err(|e| crate::values::error_text(vm, e.value))
        }
    }
}

/// The chat windows' lines from line `since` on, and the next line's
/// number (decision T514).
pub fn chat_lines(vm: &Vm, since: u64) -> Json {
    let world = &wowlua_engine::engine(vm).world;
    let lines: Vec<Json> = world
        .chat_log
        .iter()
        .filter(|l| l.seq >= since)
        .map(|l| json!({ "seq": l.seq, "frame": l.frame, "text": l.text, "color": l.color }))
        .collect();
    json!({ "lines": lines, "next": world.chat_log_next })
}

/// Fires a documented event with JSON arguments.
pub fn fire_event(vm: &mut Vm, event: &str, args: &[Json]) -> Result<(), String> {
    let mut values = Vec::new();
    for a in args {
        values.push(match a {
            Json::Null => Value::Nil,
            Json::Bool(b) => Value::Bool(*b),
            Json::Number(n) => Value::Num(n.as_f64().unwrap_or(0.0)),
            Json::String(s) => vm.str(s),
            other => {
                return Err(format!(
                    "event arguments are nil, booleans, numbers or strings, not {other}"
                ));
            }
        });
    }
    widgets::fire_event(vm, event, &values);
    Ok(())
}

/// Standard base64 (for PNGs in JSON).
pub fn base64(bytes: &[u8]) -> String {
    const T: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let b = [
            chunk[0],
            *chunk.get(1).unwrap_or(&0),
            *chunk.get(2).unwrap_or(&0),
        ];
        let n = (u32::from(b[0]) << 16) | (u32::from(b[1]) << 8) | u32::from(b[2]);
        out.push(T[(n >> 18) as usize & 63] as char);
        out.push(T[(n >> 12) as usize & 63] as char);
        out.push(if chunk.len() > 1 {
            T[(n >> 6) as usize & 63] as char
        } else {
            '='
        });
        out.push(if chunk.len() > 2 {
            T[n as usize & 63] as char
        } else {
            '='
        });
    }
    out
}

/// Changes the world: runs `script` (Lua) with `mock` the engine's
/// controls, untainted as the world's own state, as a world profile is
/// applied (decision T401). Other names are the globals.
pub fn world(vm: &mut Vm, script: &str) -> Result<(), String> {
    let controls = wowlua_engine::controls::table(vm);
    let env = vm.new_table();
    let meta = vm.new_table();
    let index = vm.str("__index");
    let globals = Value::Table(vm.globals);
    let _ = vm.heap.table_mut(meta).set(index, globals);
    vm.heap.table_mut(env).meta = Some(meta);
    let key = vm.str("mock");
    let _ = vm.heap.table_mut(env).set(key, Value::Table(controls));
    let saved = *vm.taint();
    {
        let t = vm.taint_mut();
        t.stack = wowlua_vm::Taint::SECURE;
        t.fixed = wowlua_vm::Taint::SECURE;
        t.new_object = wowlua_vm::Taint::SECURE;
        t.new_closure = wowlua_vm::Taint::SECURE;
    }
    let result = vm.load(script.as_bytes(), "=[world]").and_then(|f| {
        if let Value::Func(func) = f {
            vm.set_func_env(func, env);
        }
        vm.call(f, &[]).map(drop).map_err(|e| match e.value {
            Value::Str(s) => String::from_utf8_lossy(vm.str_bytes(s)).into_owned(),
            other => format!("{other:?}"),
        })
    });
    *vm.taint_mut() = saved;
    result
}

// ------------------------------------------------------------ inspector

/// One frame's (or region's) details, for an inspector (decision T484):
/// its strata and level, rectangle, alpha and scale as set and as drawn,
/// anchors, scripts with where their functions are, registered events,
/// regions with their layers, children, and where it was made; `source`
/// names a chunk's source file (the package's).
pub fn inspect(
    vm: &mut Vm,
    id: usize,
    source: &dyn Fn(&[u8]) -> Option<String>,
) -> Result<Json, String> {
    let count = wowlua_engine::engine(vm).world.widgets.len();
    if id >= count {
        return Err(format!("no frame {id}"));
    }
    let wid = WidgetId(id);
    let label = |vm: &Vm, w: WidgetId| widgets::widget_label(wowlua_engine::engine(vm), w);
    let place = |chunk: &[u8], line: u32| -> Json {
        let file = source(chunk).unwrap_or_else(|| {
            String::from_utf8_lossy(chunk.strip_prefix(b"@").unwrap_or(chunk)).into_owned()
        });
        json!({ "file": file, "line": line })
    };
    let (class, name, parent, shown, width, height, scale, alpha, created, children, points) = {
        let e = wowlua_engine::engine(vm);
        let w = &e.world.widgets[id];
        (
            w.class.clone(),
            w.name.clone(),
            w.parent,
            w.shown,
            w.width,
            w.height,
            w.scale,
            w.alpha,
            w.created.clone(),
            w.children.clone(),
            w.points.clone(),
        )
    };
    let visible = widgets::visible(wowlua_engine::engine(vm), wid);
    let mut out = json!({
        "id": id,
        "type": class,
        "label": label(vm, wid),
        "shown": shown,
        "visible": visible,
        "size": { "width": width, "height": height },
        "scale": scale,
        "effectiveScale": widgets::effective_scale(wowlua_engine::engine(vm), wid),
        "alpha": alpha,
        "effectiveAlpha": wowlua_engine::scene::alpha_of(vm, wid),
        "mouse": wowlua_engine::scene::takes_mouse(vm, wid),
    });
    if let Some(n) = name {
        out["name"] = json!(n);
    }
    if let Some(p) = parent {
        out["parent"] = json!({ "id": p.0, "label": label(vm, p) });
    }
    let frame = wowlua_engine::engine(vm).world.widgets[id].frame;
    if frame {
        out["strata"] = json!(widgets::strata_name(vm, wid));
        out["level"] = json!(widgets::frame_level(vm, wid));
    } else {
        let (layer, sublevel) = wowlua_engine::scene::draw_layer(vm, wid);
        out["layer"] = json!(layer);
        out["sublevel"] = json!(sublevel);
    }
    if let Some(r) = wowlua_engine::scene::rect_of(vm, wid) {
        out["rect"] = json!({ "x": r.x, "y": r.y, "width": r.w, "height": r.h });
    }
    out["points"] = Json::Array(
        points
            .iter()
            .map(|p| {
                let relative = match p.relative {
                    Some(Value::Table(_)) | Some(Value::Userdata(_)) => {
                        widgets::widget_of(vm, p.relative.unwrap_or(Value::Nil))
                            .map(|r| label(vm, r))
                    }
                    Some(Value::Str(s)) => Some(String::from_utf8_lossy(vm.str_bytes(s)).into()),
                    _ => None,
                };
                json!({
                    "point": p.point,
                    "relativeTo": relative,
                    "relativePoint": p.relative_point,
                    "x": p.x,
                    "y": p.y,
                })
            })
            .collect(),
    );
    out["scripts"] = Json::Array(scripts_of(vm, wid, &place));
    let mut events: Vec<String> = wowlua_engine::engine(vm).world.widgets[id]
        .events
        .keys()
        .cloned()
        .collect();
    events.sort();
    out["events"] = json!(events);
    let mut regions = Vec::new();
    let mut frames = Vec::new();
    for c in children {
        let is_frame = wowlua_engine::engine(vm).world.widgets[c.0].frame;
        if is_frame {
            frames.push(json!({ "id": c.0, "label": label(vm, c) }));
        } else {
            regions.push(region(vm, c));
        }
    }
    out["regions"] = Json::Array(regions);
    out["children"] = Json::Array(frames);
    if let Some((chunk, line)) = created {
        out["created"] = place(&chunk, line);
    }
    Ok(out)
}

/// A frame's scripts and hooks: each handler and where its function is.
fn scripts_of(vm: &mut Vm, id: WidgetId, place: &dyn Fn(&[u8], u32) -> Json) -> Vec<Json> {
    let (scripts, hooks) = {
        let w = &wowlua_engine::engine(vm).world.widgets[id.0];
        (w.scripts, w.hooks)
    };
    let mut out = Vec::new();
    for (table, hook) in [(scripts, false), (hooks, true)] {
        let entries: Vec<(Value, Value)> = vm.heap.table(table).entries().collect();
        for (k, v) in entries {
            let Value::Str(k) = k else { continue };
            let name = String::from_utf8_lossy(vm.str_bytes(k)).into_owned();
            let funcs: Vec<Value> = match v {
                Value::Table(list) => vm.heap.table(list).entries().map(|(_, f)| f).collect(),
                f => vec![f],
            };
            for f in funcs {
                let Value::Func(f) = f else { continue };
                let at = vm
                    .func_proto(f)
                    .map(|p| place(&p.source, p.line_defined))
                    .unwrap_or(Json::Null);
                out.push(json!({ "name": name, "hook": hook, "at": at }));
            }
        }
    }
    out.sort_by(|a, b| a["name"].as_str().cmp(&b["name"].as_str()));
    out
}

/// A region: its kind, layer, and what it shows (a texture's file or
/// atlas, a font string's text).
fn region(vm: &mut Vm, id: WidgetId) -> Json {
    let (layer, sublevel) = wowlua_engine::scene::draw_layer(vm, id);
    let (class, name, props, text, shown) = {
        let w = &wowlua_engine::engine(vm).world.widgets[id.0];
        (w.class.clone(), w.name.clone(), w.props, w.text, w.shown)
    };
    let mut r = json!({
        "id": id.0,
        "type": class,
        "layer": layer,
        "sublevel": sublevel,
        "shown": shown,
    });
    if let Some(n) = name {
        r["name"] = json!(n);
    }
    let first = |vm: &mut Vm, stem: &str| {
        widgets::stored_args(vm, props, stem).and_then(|v| v.first().copied())
    };
    if let Some(Value::Str(s)) = first(vm, "Atlas") {
        r["atlas"] = json!(String::from_utf8_lossy(vm.str_bytes(s)));
    } else {
        match first(vm, "Texture") {
            Some(Value::Str(s)) => r["texture"] = json!(String::from_utf8_lossy(vm.str_bytes(s))),
            Some(Value::Num(n)) => r["texture"] = json!(n),
            _ => {}
        }
    }
    if let Some(Value::Str(s)) = text {
        r["text"] = json!(String::from_utf8_lossy(vm.str_bytes(s)));
    }
    r
}

/// What draws topmost at a point (screen pixels from the top left), mouse
/// or not: the frame, and the region when a region draws there.
pub fn pick(vm: &mut Vm, x: f64, y: f64) -> Json {
    let scene = wowlua_engine::scene::build(vm);
    let Some(hit) = wowlua_engine::scene::drawn_at(&scene, x, y) else {
        return Json::Null;
    };
    let e = wowlua_engine::engine(vm);
    let w = &e.world.widgets[hit.0];
    if w.frame {
        json!({ "frame": hit.0 })
    } else {
        json!({ "frame": w.parent.map(|p| p.0), "region": hit.0 })
    }
}

// ---------------------------------------------------------------- units

/// The world's units: each one's index, name, tokens, level, class,
/// reaction, health, and where it is (decision T484).
pub fn units(vm: &mut Vm) -> Json {
    // (whether the player sees each unit with a place, and where it faces:
    // what a view of the world shows; T546)
    let count = wowlua_engine::engine(vm).units.list.len();
    let looks: Vec<(Option<bool>, Option<f64>)> = (0..count)
        .map(|i| {
            let id = wowlua_engine::units::UnitId(i);
            let placed = wowlua_engine::character::place_of(vm, id).is_some();
            let sight = (placed && id != wowlua_engine::units::PLAYER)
                .then(|| wowlua_engine::character::in_sight(vm, wowlua_engine::units::PLAYER, id));
            (sight, wowlua_engine::character::unit_facing(vm, id))
        })
        .collect();
    let e = wowlua_engine::engine(vm);
    let u = &e.units;
    let mob = u.target;
    let list = u
        .list
        .iter()
        .enumerate()
        .map(|(i, unit)| {
            let id = wowlua_engine::units::UnitId(i);
            // Its threat on the player's target (decision T494).
            let threat = mob
                .filter(|m| *m != id)
                .and_then(|m| wowlua_engine::units::threat(vm, id, m))
                .map(|(tanking, status, scaled, raw, value)| {
                    json!({ "tanking": tanking, "status": status, "percent": scaled,
                            "raw": raw, "value": value })
                });
            json!({
                "threat": threat,
                "id": i,
                "guid": unit.guid,
                "name": unit.name,
                "tokens": wowlua_engine::units::tokens_of(u, id),
                "level": unit.level,
                "class": unit.class,
                "player": unit.player,
                "reaction": format!("{:?}", unit.reaction).to_lowercase(),
                "health": unit.health,
                "healthMax": unit.health_max,
                "dead": unit.dead,
                "distance": unit.distance,
                "position": unit.world.map(|(y, x)| json!({ "x": x, "y": y })),
                "sight": looks[i].0,
                "facing": looks[i].1,
                "target": u.target == Some(id),
            })
        })
        .collect();
    Json::Array(list)
}

/// The player targets a unit by its index (none: clears the target).
pub fn target_unit(vm: &mut Vm, unit: Option<usize>) -> Result<(), String> {
    let count = wowlua_engine::engine(vm).units.list.len();
    if let Some(i) = unit
        && i >= count
    {
        return Err(format!("no unit {i}"));
    }
    wowlua_engine::units::target(vm, unit.map(wowlua_engine::units::UnitId));
    Ok(())
}

// --------------------------------------------------------------- events

/// The events told since the last read (the log must be on).
pub fn events(vm: &mut Vm) -> Json {
    let list = wowlua_engine::eventlog::take(vm)
        .into_iter()
        .map(|e| {
            json!({
                "seq": e.seq,
                "event": e.name,
                "args": e.args,
                "frames": e.frames,
                "ms": e.nanos.map(|n| n as f64 / 1e6),
            })
        })
        .collect();
    Json::Array(list)
}

// ------------------------------------------------------------ the world

/// The running world as a world profile's JSON (decision T494): the
/// character, its money and place, the other units (the target, focus and
/// group among them), the CVars changed from their defaults. What a
/// profile holds beyond (items, bags, spells, auras, saved variables) is
/// not written.
pub fn world_profile(vm: &mut Vm) -> Json {
    let e = wowlua_engine::engine(vm);
    let u = &e.units;
    let player = &u.list[0];
    let reaction = |r: wowlua_engine::units::Reaction| format!("{r:?}").to_lowercase();
    let character = json!({
        "name": player.name,
        "class": player.class,
        "race": player.race,
        "level": player.level,
        "sex": player.sex,
        "health": player.health,
        "healthMax": player.health_max,
    });
    let c = &e.world.character;
    let pos = &c.position;
    let mut position = json!({ "zone": pos.zone, "subZone": pos.sub_zone });
    if let Some(m) = pos.map_id {
        position["mapID"] = json!(m);
    }
    if let (Some(x), Some(y)) = (pos.x, pos.y) {
        position["x"] = json!(x);
        position["y"] = json!(y);
    }
    let units: Vec<Json> = u
        .list
        .iter()
        .enumerate()
        .skip(1)
        .filter(|(_, unit)| !unit.name.is_empty())
        .map(|(i, unit)| {
            let id = wowlua_engine::units::UnitId(i);
            let mut j = json!({
                "name": unit.name,
                "level": unit.level,
                "class": unit.class,
                "reaction": reaction(unit.reaction),
                "health": unit.health,
                "healthMax": unit.health_max,
            });
            if unit.player {
                j["player"] = json!(true);
                j["race"] = json!(unit.race);
            }
            if unit.dead {
                j["dead"] = json!(true);
            }
            if let Some(d) = unit.distance {
                j["distance"] = json!(d);
            }
            if u.target == Some(id) {
                j["target"] = json!(true);
            }
            if u.focus == Some(id) {
                j["focus"] = json!(true);
            }
            if u.group.contains(&id) {
                j["group"] = json!(if u.raid { "raid" } else { "party" });
            }
            j
        })
        .collect();
    let mut cvars = serde_json::Map::new();
    for (k, v) in &e.world.cvars {
        let default = e
            .world
            .cvar_defaults
            .iter()
            .find(|(d, _)| d.eq_ignore_ascii_case(k))
            .map(|(_, d)| d);
        if default != Some(v) {
            cvars.insert(k.clone(), json!(v));
        }
    }
    let mut out = json!({
        "character": character,
        "money": c.money,
        "position": position,
        "units": units,
    });
    if !cvars.is_empty() {
        out["cvars"] = Json::Object(cvars);
    }
    // (the realm's time of day and the weather: decision T565)
    let minutes = (wowlua_engine::clock::time_of_day(vm) / 60.0).floor() as u32;
    out["time"] = json!(format!("{:02}:{:02}", minutes / 60, minutes % 60));
    let (kind, intensity) = wowlua_engine::engine(vm).world.weather;
    if let Some(name) = ["rain", "snow", "sandstorm"].get((kind as usize).wrapping_sub(1)) {
        out["weather"] = json!({ "kind": name, "intensity": intensity });
    }
    out
}
