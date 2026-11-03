//! Widget methods with no result whose effect the widget's getters show:
//! until these, the engine took them for setters without a getter and did
//! nothing (decision T289).
//!
//! - `Frame:Raise()` and `Frame:Lower()`: the frame's level above (below)
//!   every other shown frame's in its strata, as `GetFrameLevel` answers;
//!   a top-level frame raised so as it shows and as it is clicked (T586).
//! - An edit box's selection: `HighlightText([start [, end]])` selects
//!   (the whole text by default; `HighlightText(0, 0)` nothing),
//!   `ClearHighlightText()` drops it; `Insert(text)` puts the text in place
//!   of the selection, else at the cursor, the cursor after it. What the
//!   user types (`mock.type`) goes there too; `SetText` puts the cursor at
//!   the end and drops the selection, as the user's click or typing into a
//!   box without the focus does before its `OnEditFocusGained`.
//! - An edit box's history: `AddHistoryLine(text)` keeps the line (as many
//!   as `SetHistoryLines` allows), `ClearHistory()` forgets them, and the
//!   up and down arrows step through them (`arrow`).
//! - `AdjustPointsOffset(x, y)`: every anchor moved by (x, y);
//!   `ClearPointsOffset()`: every anchor's offset 0.
//! - The clears: a button's
//!   `ClearNormalTexture()` and the other states' (the texture emptied and
//!   hidden, still the button's: decision T576), `FontString:ClearText()`, `ClearParentKey()`,
//!   `ResetTexCoord()`, `StopAnimating()` (every animation group of the
//!   region stopped).
//!
//! Unverified, none recorded: which frames `Raise` counts (the shown ones
//! of the strata here) and the level cap (10000), `Insert`'s
//! `OnTextChanged` (`false`, the frame's update after, as `SetText`'s),
//! a selection given backwards, a box without `SetHistoryLines` keeping
//! no line, the arrows' order (`OnArrowPressed` first), `ClearPointsOffset` zeroing the offsets
//! `SetPoint` gave.

use wowlua_vm::stdlib::Ret;
use wowlua_vm::{NativeCall, Value, Vm};

use crate::widgets::{self, MethodFn, WidgetId};
use crate::{engine, engine_mut};

/// The level a frame can have at most.
const MAX_LEVEL: f64 = 10000.0;

/// The selection's stem (the engine's own: no setter has it).
const HIGHLIGHT: &str = "\u{0}Highlight";

/// Drops what setter `stem` stored.
fn unset(vm: &mut Vm, id: WidgetId, stem: &str) {
    crate::widgets::mark(crate::engine_mut(vm), id);
    let props = engine(vm).world.widgets[id.0].props;
    let key = vm.str(stem);
    let _ = vm.heap.table_mut(props).set(key, Value::Nil);
}

// --------------------------------------------------------------- levels

/// Whether `id` is `root` or one of its descendants.
fn under(vm: &Vm, mut id: WidgetId, root: WidgetId) -> bool {
    let e = engine(vm);
    loop {
        if id == root {
            return true;
        }
        match e.world.widgets[id.0].parent {
            Some(p) => id = p,
            None => return false,
        }
    }
}

/// The levels of the other shown frames in `id`'s strata (not its own
/// children's).
fn other_levels(vm: &mut Vm, id: WidgetId) -> Vec<f64> {
    let rank = widgets::strata_rank(vm, id);
    let candidates: Vec<WidgetId> = {
        let e = engine(vm);
        (0..e.world.widgets.len())
            .map(WidgetId)
            .filter(|&w| {
                let class = &e.world.widgets[w.0].class;
                widgets::is_a(e, class, "Frame") && widgets::visible(e, w)
            })
            .collect()
    };
    let mut out = Vec::new();
    for w in candidates {
        if under(vm, w, id) || widgets::strata_rank(vm, w) != rank {
            continue;
        }
        out.push(widgets::frame_level(vm, w));
    }
    out
}

/// `Frame:Raise()`: above every other shown frame of its strata, its
/// descendants with levels of their own raised with it, as
/// `SetFrameLevel` moves them: a top-level window raised as it opens
/// keeps its panes above its background (the spellbook's
/// `PlayerSpellsFrame`, raised to 501, its `SpellBookFrame` at 100 was
/// drawn under it; the recorded tree has the pane above the window).
fn raise(vm: &mut Vm, id: WidgetId, _c: NativeCall) -> Ret {
    raise_frame(vm, id);
    Ok(vec![])
}

/// A frame raised as `Raise` raises it.
pub(crate) fn raise_frame(vm: &mut Vm, id: WidgetId) {
    let own = widgets::frame_level(vm, id);
    let top = other_levels(vm, id).into_iter().fold(f64::MIN, f64::max);
    if top >= own {
        let level = (top + 1.0).min(MAX_LEVEL);
        widgets::set_level(vm, id, level);
    }
}

/// Whether a frame is top-level (`SetToplevel(true)`, `toplevel="true"`).
fn toplevel(vm: &mut Vm, id: WidgetId) -> bool {
    let props = engine(vm).world.widgets[id.0].props;
    widgets::stored_args(vm, props, "Toplevel")
        .and_then(|v| v.first().copied())
        .is_some_and(|v| v.truthy())
}

/// A top-level frame raised as it shows (decision T586: "when clicked with
/// the mouse or when shown").
pub(crate) fn raise_shown(vm: &mut Vm, id: WidgetId) {
    if toplevel(vm, id) {
        raise_frame(vm, id);
    }
}

/// The top-level frames a click on `id` raises (decision T586): it and the
/// top-level frames it is in, the outermost first. Unverified: whether a
/// click in a top-level frame's child raises it (here it does, as a click
/// on a window's button brings the window forward).
pub(crate) fn raise_clicked(vm: &mut Vm, id: WidgetId) {
    let mut chain = Vec::new();
    let mut at = Some(id);
    while let Some(w) = at {
        chain.push(w);
        at = engine(vm).world.widgets[w.0].parent;
    }
    for w in chain.into_iter().rev() {
        if toplevel(vm, w) {
            raise_frame(vm, w);
        }
    }
}

/// `Frame:Lower()`: below every other shown frame of its strata (0 at the
/// lowest).
fn lower(vm: &mut Vm, id: WidgetId, _c: NativeCall) -> Ret {
    let own = widgets::frame_level(vm, id);
    let bottom = other_levels(vm, id).into_iter().fold(f64::MAX, f64::min);
    if bottom <= own {
        let level = (bottom - 1.0).max(0.0);
        widgets::set_level(vm, id, level);
    }
    Ok(vec![])
}

// ------------------------------------------------------------ edit boxes

fn text_of(vm: &Vm, id: WidgetId) -> String {
    match engine(vm).world.widgets[id.0].text {
        Some(Value::Str(s)) => String::from_utf8_lossy(vm.heap.str_bytes(s)).into_owned(),
        _ => String::new(),
    }
}

fn first_number(vm: &mut Vm, id: WidgetId, stem: &str) -> Option<f64> {
    let props = engine(vm).world.widgets[id.0].props;
    match widgets::stored_args(vm, props, stem).and_then(|v| v.first().copied()) {
        Some(Value::Num(n)) => Some(n),
        _ => None,
    }
}

/// A byte offset in `text`, moved back to a character's start.
fn at_char(text: &str, mut at: usize) -> usize {
    at = at.min(text.len());
    while !text.is_char_boundary(at) {
        at -= 1;
    }
    at
}

/// The cursor's byte offset (`SetCursorPosition`'s, else the text's end).
fn cursor(vm: &mut Vm, id: WidgetId, text: &str) -> usize {
    match first_number(vm, id, "CursorPosition") {
        Some(n) if n >= 0.0 => at_char(text, n as usize),
        Some(_) => 0,
        None => text.len(),
    }
}

/// The selection's byte range, if any text is selected.
fn selection(vm: &mut Vm, id: WidgetId, text: &str) -> Option<(usize, usize)> {
    let props = engine(vm).world.widgets[id.0].props;
    let v = widgets::stored_args(vm, props, HIGHLIGHT)?;
    let (Some(Value::Num(a)), Some(Value::Num(b))) = (v.first(), v.get(1)) else {
        return None;
    };
    let (a, b) = (at_char(text, *a as usize), at_char(text, *b as usize));
    (a < b).then_some((a, b))
}

/// `HighlightText([start [, end]])`: selects from `start` (0) to `end`
/// (the text's end; a negative one too), byte offsets as the cursor's.
fn highlight_text(vm: &mut Vm, id: WidgetId, c: NativeCall) -> Ret {
    let len = text_of(vm, id).len() as f64;
    let start = match vm.hidden(c.arg(2)) {
        Value::Num(n) => n.max(0.0),
        _ => 0.0,
    };
    let end = match vm.hidden(c.arg(3)) {
        Value::Num(n) if n >= 0.0 => n.min(len),
        _ => len,
    };
    let (start, end) = if start <= end {
        (start, end)
    } else {
        (end, start)
    };
    widgets::store(vm, id, HIGHLIGHT, &[Value::Num(start), Value::Num(end)]);
    Ok(vec![])
}

fn clear_highlight_text(vm: &mut Vm, id: WidgetId, _c: NativeCall) -> Ret {
    unset(vm, id, HIGHLIGHT);
    Ok(vec![])
}

/// Puts `text` into the edit box as the user's typing and `Insert` do: in
/// place of the selection, else at the cursor; the cursor after it.
pub fn insert(vm: &mut Vm, id: WidgetId, text: &str) {
    let mut now = text_of(vm, id);
    let (from, to) = match selection(vm, id, &now) {
        Some(range) => range,
        None => {
            let at = cursor(vm, id, &now);
            (at, at)
        }
    };
    now.replace_range(from..to, text);
    let v = vm.str(&now);
    widgets::store_text(vm, id, Some(v));
    unset(vm, id, HIGHLIGHT);
    let at = (from + text.len()) as f64;
    widgets::store(vm, id, "CursorPosition", &[Value::Num(at)]);
}

/// Backspace (`forward`: Delete) in an edit box: the selection gone, else
/// the character before (after) the cursor; whether the text changed.
pub fn erase(vm: &mut Vm, id: WidgetId, forward: bool) -> bool {
    let mut now = text_of(vm, id);
    let (from, to) = match selection(vm, id, &now) {
        Some(range) if range.0 != range.1 => range,
        _ => {
            let at = cursor(vm, id, &now);
            let near = if forward {
                now[at..].chars().next().map(|c| (at, at + c.len_utf8()))
            } else {
                now[..at]
                    .chars()
                    .next_back()
                    .map(|c| (at - c.len_utf8(), at))
            };
            match near {
                Some(range) => range,
                None => return false,
            }
        }
    };
    now.replace_range(from..to, "");
    let v = vm.str(&now);
    widgets::store_text(vm, id, Some(v));
    unset(vm, id, HIGHLIGHT);
    widgets::store(vm, id, "CursorPosition", &[Value::Num(from as f64)]);
    true
}

/// The text set: the cursor at its end, nothing selected.
pub fn text_set(vm: &mut Vm, id: WidgetId) {
    unset(vm, id, HIGHLIGHT);
    unset(vm, id, "CursorPosition");
}

/// `EditBox:Insert(text)`: the text in, its `OnTextChanged(false)` in the
/// frame's update, as `SetText`'s.
fn insert_text(vm: &mut Vm, id: WidgetId, c: NativeCall) -> Ret {
    let text = match vm.hidden(c.arg(2)) {
        Value::Str(s) => String::from_utf8_lossy(vm.str_bytes(s)).into_owned(),
        v @ Value::Num(_) => match vm.tostring(v) {
            Ok(Value::Str(s)) => String::from_utf8_lossy(vm.str_bytes(s)).into_owned(),
            _ => String::new(),
        },
        _ => String::new(),
    };
    insert(vm, id, &text);
    if !engine(vm).world.text_set.contains(&id) {
        engine_mut(vm).world.text_set.push(id);
    }
    Ok(vec![])
}

// --------------------------------------------------------------- history

/// The lines `AddHistoryLine` kept, oldest first, and where the arrow keys
/// are in them (the engine's own stems).
const HISTORY: &str = "\u{0}History";
const HISTORY_AT: &str = "\u{0}HistoryAt";

fn history(vm: &mut Vm, id: WidgetId) -> Vec<Value> {
    let props = engine(vm).world.widgets[id.0].props;
    widgets::stored_args(vm, props, HISTORY).unwrap_or_default()
}

/// `AddHistoryLine(text)`: kept, as many lines as `SetHistoryLines` allows
/// (the oldest go first; none without it).
fn add_history_line(vm: &mut Vm, id: WidgetId, c: NativeCall) -> Ret {
    let max = first_number(vm, id, "HistoryLines").unwrap_or(0.0).max(0.0) as usize;
    let line = match vm.hidden(c.arg(2)) {
        v @ Value::Str(_) => v,
        _ => return Ok(vec![]),
    };
    let mut lines = history(vm, id);
    lines.push(line);
    let extra = lines.len().saturating_sub(max);
    lines.drain(..extra);
    widgets::store(vm, id, HISTORY, &lines);
    unset(vm, id, HISTORY_AT);
    Ok(vec![])
}

/// `ClearHistory()`: no lines kept.
fn clear_history(vm: &mut Vm, id: WidgetId, _c: NativeCall) -> Ret {
    unset(vm, id, HISTORY);
    unset(vm, id, HISTORY_AT);
    Ok(vec![])
}

/// An arrow key (`UP`, `DOWN`, `LEFT`, `RIGHT`) the focused edit box takes,
/// whether it took it: with `SetAltArrowKeyMode(true)` only with Alt held.
/// It runs `OnArrowPressed(key)`; left and right move the cursor a
/// character (the selection gone), up and down step through the history
/// lines, the newest first, down past the newest an empty text
/// (`OnTextChanged(true)`).
pub fn arrow(vm: &mut Vm, id: WidgetId, key: &str) -> bool {
    let props = engine(vm).world.widgets[id.0].props;
    let alt_mode = widgets::stored_args(vm, props, "AltArrowKeyMode")
        .and_then(|v| v.first().copied())
        .is_some_and(|v| v.truthy());
    if alt_mode && !engine(vm).world.input.modifiers[2] {
        return false;
    }
    let k = vm.str(key);
    widgets::run_script(vm, id, "OnArrowPressed", &[k]);
    let text = text_of(vm, id);
    match key {
        "LEFT" | "RIGHT" => {
            let at = cursor(vm, id, &text);
            let moved = if key == "LEFT" {
                text[..at]
                    .chars()
                    .next_back()
                    .map_or(at, |c| at - c.len_utf8())
            } else {
                text[at..].chars().next().map_or(at, |c| at + c.len_utf8())
            };
            unset(vm, id, HIGHLIGHT);
            widgets::store(vm, id, "CursorPosition", &[Value::Num(moved as f64)]);
        }
        _ => {
            let lines = history(vm, id);
            if lines.is_empty() {
                return true;
            }
            let n = lines.len();
            let at = first_number(vm, id, HISTORY_AT).map_or(n, |a| a as usize);
            let next = match key {
                "UP" if at > 0 => at - 1,
                "DOWN" if at < n => at + 1,
                _ => return true,
            };
            widgets::store(vm, id, HISTORY_AT, &[Value::Num(next as f64)]);
            let line = lines.get(next).copied().unwrap_or_else(|| vm.str(""));
            widgets::store_text(vm, id, Some(line));
            text_set(vm, id);
            widgets::run_script(vm, id, "OnTextChanged", &[Value::Bool(true)]);
        }
    }
    true
}

// --------------------------------------------------------------- points

/// `AdjustPointsOffset(x, y)`: every anchor's offset moved by (x, y).
fn adjust_points_offset(vm: &mut Vm, id: WidgetId, c: NativeCall) -> Ret {
    let dx = vm.check_number(&c, 2)?;
    let dy = vm.check_number(&c, 3)?;
    for p in &mut engine_mut(vm).world.widgets[id.0].points {
        p.x += dx;
        p.y += dy;
    }
    widgets::layout_changed(vm);
    Ok(vec![])
}

/// `ClearPointsOffset()`: every anchor's offset 0.
fn clear_points_offset(vm: &mut Vm, id: WidgetId, _c: NativeCall) -> Ret {
    for p in &mut engine_mut(vm).world.widgets[id.0].points {
        p.x = 0.0;
        p.y = 0.0;
    }
    widgets::layout_changed(vm);
    Ok(vec![])
}

// --------------------------------------------------------------- clears

fn same(a: Value, b: Value) -> bool {
    match (a, b) {
        (Value::Table(x), Value::Table(y)) => x == y,
        (Value::Userdata(x), Value::Userdata(y)) => x == y,
        _ => false,
    }
}

/// A button's `Clear…Texture()`: the state's texture emptied (no file or
/// atlas, hidden) but still the button's, `Get…Texture` answering it:
/// Forever 70707's item buttons mask their normal texture after their
/// `OnLoad`, in which the professions' recraft slot clears it (decision
/// T576; unverified, tests/ingame/PROBES.md "The Camelot update").
fn clear_state_texture(vm: &mut Vm, id: WidgetId, state: &str) -> Ret {
    crate::widgets::mark(crate::engine_mut(vm), id);
    let stem = format!("{state}Texture");
    let props = engine(vm).world.widgets[id.0].props;
    let region = widgets::stored_args(vm, props, &stem)
        .and_then(|v| v.first().copied())
        .and_then(|v| widgets::widget_of(vm, v));
    if let Some(r) = region {
        engine_mut(vm).world.widgets[r.0].shown = false;
        widgets::unstore(vm, r, "Texture");
        widgets::unstore(vm, r, "Atlas");
    }
    unset(vm, id, &format!("{state}Atlas"));
    Ok(vec![])
}

fn clear_normal_texture(vm: &mut Vm, id: WidgetId, _c: NativeCall) -> Ret {
    clear_state_texture(vm, id, "Normal")
}

fn clear_pushed_texture(vm: &mut Vm, id: WidgetId, _c: NativeCall) -> Ret {
    clear_state_texture(vm, id, "Pushed")
}

fn clear_disabled_texture(vm: &mut Vm, id: WidgetId, _c: NativeCall) -> Ret {
    clear_state_texture(vm, id, "Disabled")
}

fn clear_highlight_texture(vm: &mut Vm, id: WidgetId, _c: NativeCall) -> Ret {
    clear_state_texture(vm, id, "Highlight")
}

/// `FontString:ClearText()`: no text (`GetText` nil).
fn clear_text(vm: &mut Vm, id: WidgetId, _c: NativeCall) -> Ret {
    widgets::store_text(vm, id, None);
    Ok(vec![])
}

/// `ClearParentKey()`: the parent no longer holds it under its key.
fn clear_parent_key(vm: &mut Vm, id: WidgetId, _c: NativeCall) -> Ret {
    let (parent, me) = {
        let e = engine(vm);
        (e.world.widgets[id.0].parent, widgets::value_of(e, id))
    };
    if let Some(p) = parent {
        let pt = engine(vm).world.widgets[p.0].table;
        let keys: Vec<Value> = vm
            .heap
            .table(pt)
            .entries()
            .filter(|(k, v)| matches!(k, Value::Str(_)) && same(*v, me))
            .map(|(k, _)| k)
            .collect();
        for k in keys {
            let _ = vm.heap.table_mut(pt).set(k, Value::Nil);
        }
    }
    unset(vm, id, "ParentKey");
    Ok(vec![])
}

/// `ResetTexCoord()`: the whole texture (`GetTexCoord`'s 0, 0, 0, 1, 1, 0,
/// 1, 1).
fn reset_tex_coord(vm: &mut Vm, id: WidgetId, _c: NativeCall) -> Ret {
    unset(vm, id, "TexCoord");
    Ok(vec![])
}

/// `StopAnimating()`: every animation group of the region stopped.
fn stop_animating(vm: &mut Vm, id: WidgetId, _c: NativeCall) -> Ret {
    let groups: Vec<WidgetId> = {
        let e = engine(vm);
        e.world.widgets[id.0]
            .children
            .iter()
            .copied()
            .filter(|g| e.world.widgets[g.0].class == "AnimationGroup")
            .collect()
    };
    for g in groups {
        crate::animations::stop_group(vm, g);
    }
    Ok(vec![])
}

// ------------------------------------------------- nothing kept or drawn

/// Methods of what the engine draws or keeps nothing of: model cameras,
/// lights and animation kits, the browser and the shop's
/// checkout, housing's pins, the minimap's blips and pings, movies, unit
/// position frames, vector graphics and blobs, the input language, focus
/// and mouse simulation, roles and access restrictions, and a single
/// animation played on its own (unverified: whether its group plays).
/// They change nothing a getter reads; a getter among them answers its
/// documented empty values (`widgets::stored`).
pub const EMPTY_METHODS: &[&str] = &[
    "Animation:Pause",
    "Animation:Play",
    "Animation:Restart",
    "Animation:Stop",
    "Blob:DrawAll",
    "Blob:DrawBlob",
    "Blob:DrawNone",
    "Browser:ClearFocus",
    "Browser:CopyExternalLink",
    "Browser:DeleteCookies",
    "Browser:NavigateBack",
    "Browser:NavigateForward",
    "Browser:NavigateHome",
    "Browser:NavigateReload",
    "Browser:NavigateStop",
    "Browser:OpenExternalLink",
    "Browser:OpenTicket",
    "Button:RegisterForMouse",
    "CharacterModelBase:ApplySpellVisualKit",
    "CharacterModelBase:CanSetUnit",
    "CharacterModelBase:FreezeAnimation",
    "CharacterModelBase:PlayAnimKit",
    "CharacterModelBase:RefreshCamera",
    "CharacterModelBase:RefreshUnit",
    "CharacterModelBase:StopAnimKit",
    "CharacterModelBase:ZeroCachedCenterXY",
    "CinematicModel:EquipItem",
    "CinematicModel:InitializeCamera",
    "CinematicModel:InitializePanCamera",
    "CinematicModel:RefreshCamera",
    "CinematicModel:StartPan",
    "CinematicModel:StopPan",
    "CinematicModel:UnequipItems",
    "ColorSelect:ClearColorWheelTexture",
    "Cooldown:Pause",
    "Cooldown:Resume",
    "EditBox:ResetInputMode",
    "EditBox:ToggleInputLanguage",
    "FontString:OnColorsUpdated",
    "Frame:AbortDrag",
    "Frame:AddRoleset",
    "Frame:CreateVectorGraphics",
    "Frame:DesaturateHierarchy",
    "Frame:RegisterUnitEventCallback",
    "Frame:RemoveRoleset",
    "Frame:ResizeToBoundsRect",
    "Frame:RotateTextures",
    "FrameScriptObject:AddAccessRestrictions",
    "FrameScriptObject:AddSecretAspect",
    "FrameScriptObject:HasSecretValues",
    "HousingFixturePointFrame:HasAttachedFixture",
    "HousingFixturePointFrame:Select",
    "HousingLayoutPinFrame:CanMove",
    "HousingLayoutPinFrame:CanRemove",
    "HousingLayoutPinFrame:CanRotate",
    "HousingLayoutPinFrame:Drag",
    "HousingLayoutPinFrame:GetDoorConnectionInfo",
    "HousingLayoutPinFrame:GetPinType",
    "HousingLayoutPinFrame:GetRoomGUID",
    "HousingLayoutPinFrame:GetRoomName",
    "HousingLayoutPinFrame:Select",
    "Minimap:PingLocation",
    "Minimap:UpdateBlips",
    "Minimap:UpdateMouseoverAtPoint",
    "Model:AdvanceTime",
    "Model:ClearTransform",
    "Model:ReplaceIconTexture",
    "Model:UseModelCenterToTransform",
    "ModelFFX:AddCharacterLight",
    "ModelFFX:AddLight",
    "ModelFFX:AddPetLight",
    "ModelFFX:ResetLights",
    "ModelSceneFrame:TakeActor",
    "ModelSceneFrameActor:DressPlayerSlot",
    "ModelSceneFrameActor:ResetNextHandSlot",
    "ModelSceneFrameActor:UseUnitSheatheCategory",
    "ModelSceneFrameActorBase:PlayAnimationKit",
    "ModelSceneFrameActorBase:StopAnimationKit",
    "Movie:StartMovie",
    "Movie:StartMovieByName",
    "Movie:StopMovie",
    "NamePlate:ClearAllHitTestPoints",
    "OffScreenFrame:Flush",
    "QuestPOI:GetNumTooltips",
    "QuestPOI:GetTooltipIndex",
    "QuestPOI:UpdateMouseOverTooltip",
    "ScenarioPOI:GetScenarioTooltipText",
    "ScenarioPOI:UpdateMouseOverTooltip",
    "ScriptRegion:FocusEnter",
    "ScriptRegion:FocusExit",
    "ScriptRegion:MouseDown",
    "ScriptRegion:MouseUp",
    "ScrollFrame:UpdateScrollChildRect",
    "SimpleCheckout:CancelOpenCheckout",
    "SimpleCheckout:ClearFocus",
    "SimpleCheckout:CloseCheckout",
    "SimpleCheckout:CopyExternalLink",
    "SimpleCheckout:OpenCheckout",
    "SimpleCheckout:OpenExternalLink",
    "TabardModelBase:CycleVariation",
    "TabardModelBase:GetLowerEmblemTexture",
    "TabardModelBase:GetUpperEmblemTexture",
    "TabardModelBase:InitializeTabardColors",
    "TabardModelBase:Save",
    "TextureBase:ClearSVG",
    "Tooltip:AddFontStrings",
    "UnitPositionFrame:AddUnit",
    "UnitPositionFrame:ClearUnits",
    "UnitPositionFrame:FinalizeUnits",
    "UnitPositionFrame:StartPlayerPing",
    "UnitPositionFrame:StopPlayerPing",
    "VectorGraphics:ClearSVG",
    "VectorGraphics:GetSVGFileID",
    "VectorGraphics:HasSVG",
];

/// `ClearAttributes()`: every attribute gone.
fn clear_attributes(vm: &mut Vm, id: WidgetId, _c: NativeCall) -> Ret {
    let fresh = vm.new_table();
    engine_mut(vm).world.widgets[id.0].attributes = fresh;
    Ok(vec![])
}

/// `ClearScripts()`: every script handler gone (the hooks too).
fn clear_scripts(vm: &mut Vm, id: WidgetId, _c: NativeCall) -> Ret {
    let (scripts, hooks) = (vm.new_table(), vm.new_table());
    let w = &mut engine_mut(vm).world.widgets[id.0];
    w.scripts = scripts;
    w.hooks = hooks;
    Ok(vec![])
}

/// `LockHighlight()` / `UnlockHighlight()`: as `SetHighlightLocked`
/// (`IsHighlightLocked` reads it; the engine draws nothing yet).
fn lock_highlight(vm: &mut Vm, id: WidgetId, _c: NativeCall) -> Ret {
    widgets::store(vm, id, "HighlightLocked", &[Value::Bool(true)]);
    Ok(vec![])
}

fn unlock_highlight(vm: &mut Vm, id: WidgetId, _c: NativeCall) -> Ret {
    widgets::store(vm, id, "HighlightLocked", &[Value::Bool(false)]);
    Ok(vec![])
}

/// `RegisterForDrag(...)`: the buttons kept.
fn register_for_drag(vm: &mut Vm, id: WidgetId, c: NativeCall) -> Ret {
    let args: Vec<Value> = c.args.iter().skip(1).copied().collect();
    widgets::store(vm, id, "RegisterForDrag", &args);
    Ok(vec![])
}

/// `AnimationGroup:RemoveAnimations()`: its animations gone (`GetAnimations`
/// none, its duration 0).
fn remove_animations(vm: &mut Vm, id: WidgetId, _c: NativeCall) -> Ret {
    let gone: Vec<WidgetId> = {
        let e = engine(vm);
        e.world.widgets[id.0]
            .children
            .iter()
            .copied()
            .filter(|c| widgets::is_a(e, &e.world.widgets[c.0].class, "Animation"))
            .collect()
    };
    let e = engine_mut(vm);
    e.world.widgets[id.0].children.retain(|c| !gone.contains(c));
    for a in gone {
        e.world.widgets[a.0].parent = None;
    }
    Ok(vec![])
}

/// `ModelSceneFrame:GetDrawLayer()`: as set, else a fresh model scene's
/// recorded `ARTWORK`, 0.
fn model_scene_draw_layer(vm: &mut Vm, id: WidgetId, _c: NativeCall) -> Ret {
    let props = engine(vm).world.widgets[id.0].props;
    if let Some(v) = widgets::stored_args(vm, props, "DrawLayer") {
        return Ok(v);
    }
    let layer = vm.str("ARTWORK");
    Ok(vec![layer, Value::Num(0.0)])
}

pub fn methods() -> Vec<(&'static str, MethodFn)> {
    vec![
        ("Frame:Raise", raise),
        ("Frame:Lower", lower),
        ("EditBox:HighlightText", highlight_text),
        ("EditBox:ClearHighlightText", clear_highlight_text),
        ("EditBox:Insert", insert_text),
        ("EditBox:AddHistoryLine", add_history_line),
        ("EditBox:ClearHistory", clear_history),
        ("AdjustPointsOffset", adjust_points_offset),
        ("ClearPointsOffset", clear_points_offset),
        ("Button:ClearNormalTexture", clear_normal_texture),
        ("Button:ClearPushedTexture", clear_pushed_texture),
        ("Button:ClearDisabledTexture", clear_disabled_texture),
        ("Button:ClearHighlightTexture", clear_highlight_texture),
        ("FontString:ClearText", clear_text),
        ("ClearParentKey", clear_parent_key),
        ("ResetTexCoord", reset_tex_coord),
        ("StopAnimating", stop_animating),
        ("Frame:ClearAttributes", clear_attributes),
        ("ScriptRegion:ClearScripts", clear_scripts),
        ("Frame:LockHighlight", lock_highlight),
        ("Frame:UnlockHighlight", unlock_highlight),
        ("Frame:RegisterForDrag", register_for_drag),
        ("AnimationGroup:RemoveAnimations", remove_animations),
        ("ModelSceneFrame:GetDrawLayer", model_scene_draw_layer),
    ]
}
