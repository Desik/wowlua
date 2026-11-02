//! UI XML into widgets, as the client builds it: virtual elements become
//! templates (`inherits`, `CreateFrame`'s template), intrinsic ones frame
//! types; other elements become widgets, their templates applied first,
//! then their own attributes, mixins, children and scripts; `OnLoad` runs
//! once a widget and its children are built.
//!
//! Unverified (no recording): the order within an element (attributes,
//! mixins, key values, children, scripts), `$parent` naming the nearest
//! named ancestor, script parameter names, and which attributes call which
//! method.

use std::path::Path;
use std::rc::Rc;

use wowlua_vm::{TableRef, Value, Vm};

use super::xml::Element;
use crate::widgets::{self, WidgetId};
use crate::{engine, engine_mut};

/// Region tags and the class they make.
const REGIONS: &[(&str, &str)] = &[
    ("Texture", "Texture"),
    ("FontString", "FontString"),
    ("MaskTexture", "MaskTexture"),
    ("Line", "Line"),
    // A model scene's actor template (`<Actor virtual="true">`).
    ("Actor", "ModelSceneFrameActor"),
];

/// Child elements that are a texture the widget uses, and its setter.
const TEXTURE_SLOTS: &[(&str, &str)] = &[
    ("NormalTexture", "SetNormalTexture"),
    ("PushedTexture", "SetPushedTexture"),
    ("DisabledTexture", "SetDisabledTexture"),
    ("HighlightTexture", "SetHighlightTexture"),
    ("CheckedTexture", "SetCheckedTexture"),
    ("DisabledCheckedTexture", "SetDisabledCheckedTexture"),
    ("ThumbTexture", "SetThumbTexture"),
    ("BarTexture", "SetStatusBarTexture"),
    // A color select's.
    ("ColorWheelTexture", "SetColorWheelTexture"),
    ("ColorWheelThumbTexture", "SetColorWheelThumbTexture"),
    ("ColorValueTexture", "SetColorValueTexture"),
    ("ColorValueThumbTexture", "SetColorValueThumbTexture"),
    ("ColorAlphaTexture", "SetColorAlphaTexture"),
    ("ColorAlphaThumbTexture", "SetColorAlphaThumbTexture"),
];

/// Font object slots (`<NormalFont style="GameFontNormal"/>`).
const FONT_SLOTS: &[(&str, &str)] = &[
    ("NormalFont", "SetNormalFontObject"),
    ("HighlightFont", "SetHighlightFontObject"),
    ("DisabledFont", "SetDisabledFontObject"),
];

/// Boolean attributes that call a method with `true`.
const FLAG_ATTRS: &[(&str, &str)] = &[
    ("enableMouse", "EnableMouse"),
    ("enableMouseClicks", "SetMouseClickEnabled"),
    ("enableMouseMotion", "SetMouseMotionEnabled"),
    ("enableKeyboard", "EnableKeyboard"),
    ("movable", "SetMovable"),
    ("resizable", "SetResizable"),
    ("toplevel", "SetToplevel"),
    ("clampedToScreen", "SetClampedToScreen"),
    ("dontSavePosition", "SetDontSavePosition"),
    ("autoFocus", "SetAutoFocus"),
    ("multiLine", "SetMultiLine"),
    ("wordwrap", "SetWordWrap"),
    ("setToFinalAlpha", "SetToFinalAlpha"),
    ("propagateKeyboardInput", "SetPropagateKeyboardInput"),
    ("obeyStepOnDrag", "SetObeyStepOnDrag"),
    ("reverseFill", "SetReverseFill"),
    ("rotatesTexture", "SetRotatesTexture"),
    ("useParentLevel", "SetUsingParentLevel"),
    // Blizzard's scroll boxes (`ScrollBoxBaseTemplate`) clip what scrolls
    // in them: the Settings panel's graphics quality controls ran past the
    // panel without it.
    ("clipChildren", "SetClipsChildren"),
];

/// String and number attributes passed to a setter.
const VALUE_ATTRS: &[(&str, &str)] = &[
    ("frameStrata", "SetFrameStrata"),
    ("frameLevel", "SetFrameLevel"),
    ("justifyH", "SetJustifyH"),
    ("justifyV", "SetJustifyV"),
    ("file", "SetTexture"),
    ("atlas", "SetAtlas"),
    ("alphaMode", "SetBlendMode"),
    ("texelSnappingBias", "SetTexelSnappingBias"),
    ("letters", "SetMaxLetters"),
    // SuuUI probe chat: ChatFrame1EditBox kept 32 (its template's).
    ("historyLines", "SetHistoryLines"),
    ("alpha", "SetAlpha"),
    ("scale", "SetScale"),
    ("orientation", "SetOrientation"),
    ("valueStep", "SetValueStep"),
    ("stepsPerPage", "SetStepsPerPage"),
    // Animations.
    ("duration", "SetDuration"),
    ("order", "SetOrder"),
    ("startDelay", "SetStartDelay"),
    ("endDelay", "SetEndDelay"),
    ("smoothing", "SetSmoothing"),
    ("fromAlpha", "SetFromAlpha"),
    ("toAlpha", "SetToAlpha"),
    ("looping", "SetLooping"),
    ("childKey", "SetChildKey"),
    ("targetKey", "SetTargetKey"),
    ("flipBookRows", "SetFlipBookRows"),
    ("flipBookColumns", "SetFlipBookColumns"),
    ("flipBookFrames", "SetFlipBookFrames"),
    ("flipBookFrameWidth", "SetFlipBookFrameWidth"),
    ("flipBookFrameHeight", "SetFlipBookFrameHeight"),
];

/// Parameters of inline script bodies (`...` after them, always).
fn script_params(script: &str) -> &'static str {
    match script {
        "OnEvent" => "self, event, ...",
        "OnUpdate" => "self, elapsed, ...",
        "OnClick" | "PreClick" | "PostClick" => "self, button, down, ...",
        "OnEnter" | "OnLeave" => "self, motion, ...",
        "OnMouseDown" | "OnMouseUp" | "OnDragStart" => "self, button, ...",
        "OnMouseWheel" => "self, delta, ...",
        "OnValueChanged" => "self, value, userInput, ...",
        "OnTextChanged" => "self, userInput, ...",
        "OnSizeChanged" => "self, width, height, ...",
        "OnAttributeChanged" => "self, name, value, ...",
        "OnKeyDown" | "OnKeyUp" => "self, key, ...",
        "OnChar" => "self, text, ...",
        _ => "self, ...",
    }
}

/// A Lua error's message.
fn message(vm: &mut Vm, v: Value) -> String {
    match v {
        Value::Str(s) => String::from_utf8_lossy(vm.str_bytes(s)).into_owned(),
        other => vm
            .tostring(other)
            .ok()
            .and_then(|s| match s {
                Value::Str(s) => Some(String::from_utf8_lossy(vm.str_bytes(s)).into_owned()),
                _ => None,
            })
            .unwrap_or_else(|| "?".into()),
    }
}

/// Where the loader is: the file it reads, for chunk names and errors.
pub struct Context<'a> {
    pub file: &'a str,
}

impl Context<'_> {
    fn error(&self, vm: &mut Vm, line: usize, msg: String) {
        let e = format!("{}:{line}: {msg}", self.file);
        engine_mut(vm).world.ui.errors.push(e);
    }
}

/// The class an element's tag makes, if it is a widget or region.
pub fn class_of(vm: &Vm, tag: &str) -> Option<String> {
    if let Some((_, class)) = REGIONS.iter().find(|(t, _)| t.eq_ignore_ascii_case(tag)) {
        return Some(class.to_string());
    }
    let e = engine(vm);
    if let Some((base, _)) = e.world.ui.intrinsics.get(&tag.to_ascii_lowercase()) {
        return Some(base.clone());
    }
    // Animation groups and animations (`<Alpha>`, `<Translation>`).
    if let Some(class) = crate::widgets::canonical_class(e, tag)
        && (class == "AnimationGroup" || crate::widgets::is_a(e, &class, "Animation"))
    {
        return Some(class);
    }
    e.api
        .classes
        .iter()
        .find(|(_, c)| {
            c.widget
                .as_deref()
                .is_some_and(|w| w.eq_ignore_ascii_case(tag))
        })
        .map(|(name, _)| name.clone())
}

/// A top-level widget element of a `<Ui>`: a template if virtual (and a
/// frame type if intrinsic), else a widget.
pub fn top_level(vm: &mut Vm, cx: &Context, el: &Rc<Element>) {
    let Some(class) = class_of(vm, &el.tag) else {
        return;
    };
    if el.flag("virtual") || el.flag("intrinsic") {
        if let Some(name) = el.attr("name") {
            let key = name.to_ascii_lowercase();
            let ui = &mut engine_mut(vm).world.ui;
            if el.flag("intrinsic") {
                ui.intrinsics.insert(key, (class, el.clone()));
            } else {
                ui.templates.insert(key, el.clone());
            }
        }
        return;
    }
    let parent = el.attr("parent").and_then(|p| {
        let g = vm.get_global(p);
        widgets::widget_of(vm, g)
    });
    engine_mut(vm).world.ui.building_file += 1;
    build(vm, cx, el, parent);
    engine_mut(vm).world.ui.building_file -= 1;
}

/// A widget a secure addon's file builds as it loads is forbidden only
/// within a `<ScopedModifier forbidden="true">` (or under a forbidden
/// parent): Blizzard wraps the store's, the ping system's, the token's
/// and the checkout's frames in one, and leaves the catalog shop's out,
/// which ElvUI skins (`CatalogShopFrame:StripTextures()`) at every login,
/// error-free in the client as far as known. Unverified by a probe;
/// frames secure code makes at run time stay forbidden.
fn unforbid_file_widget(vm: &mut Vm, id: WidgetId, parent: Option<WidgetId>) {
    let e = engine(vm);
    if e.world.ui.building_file == 0 || !e.world.widgets[id.0].forbidden {
        return;
    }
    let parent_forbidden = parent.is_some_and(|p| e.world.widgets[p.0].forbidden);
    if !e.world.ui.in_forbidden && !parent_forbidden {
        engine_mut(vm).world.widgets[id.0].forbidden = false;
    }
}

/// The name an element gives its widget, `$parent` replaced by the nearest
/// named ancestor's name.
fn resolve_name(vm: &Vm, name: &str, parent: Option<WidgetId>) -> String {
    if !name.contains("$parent") && !name.contains("$Parent") {
        return name.to_string();
    }
    let e = engine(vm);
    let mut cur = parent;
    let mut pname = String::new();
    while let Some(p) = cur {
        if let Some(n) = &e.world.widgets[p.0].name {
            pname = n.clone();
            break;
        }
        cur = e.world.widgets[p.0].parent;
    }
    name.replace("$parent", &pname).replace("$Parent", &pname)
}

/// Builds the widget an element describes under `parent`, then runs its
/// `OnLoad`.
pub fn build(
    vm: &mut Vm,
    cx: &Context,
    el: &Rc<Element>,
    parent: Option<WidgetId>,
) -> Option<Value> {
    let class = class_of(vm, &el.tag)?;
    // A child marked virtual is still built, but its name makes no global
    // (SuuUI probe bankSlots: camelot's BankFrame.xml marks its purchase
    // button so; `BankFrame.BankPanel.PurchaseButton` is a shown Button,
    // `BankFramePurchaseButton` nil). Unverified: its `GetName`, and
    // whether the name also makes a template.
    let name = el
        .attr("name")
        .filter(|_| !el.flag("virtual"))
        .map(|n| resolve_name(vm, n, parent));
    // The client's UIParent is the screen the engine already has.
    let existing = name
        .as_deref()
        .filter(|n| *n == "UIParent")
        .map(|n| vm.get_global(n))
        .filter(|v| widgets::widget_of(vm, *v).is_some());
    let frame = match existing {
        Some(f) => f,
        None => widgets::create(vm, &class, name, parent),
    };
    let id = widgets::widget_of(vm, frame)?;
    if existing.is_none() {
        unforbid_file_widget(vm, id, parent);
    }
    engine_mut(vm).world.widgets[id.0].frame = true;
    crate::widgets::reorder(crate::engine_mut(vm));
    // An intrinsic type's own element comes first.
    let intrinsic = engine(vm)
        .world
        .ui
        .intrinsics
        .get(&el.tag.to_ascii_lowercase())
        .map(|(_, e)| e.clone());
    if let Some(i) = intrinsic {
        apply_as_intrinsic(vm, cx, frame, id, &i);
    }
    apply(vm, cx, frame, id, el);
    fill_parent_if_unplaced(vm, cx, el, frame, id);
    widgets::run_script(vm, id, "OnLoad", &[]);
    // Built visible: shown (unverified that the client runs OnShow here;
    // Blizzard's PersonalResourceDisplay sets itself up in OnShow).
    if widgets::visible(engine(vm), id) {
        widgets::run_script(vm, id, "OnShow", &[]);
    }
    Some(frame)
}

/// Applies a template by name to a widget (`CreateFrame`'s template, or
/// `inherits`); false if there is no such template.
pub fn apply_template(vm: &mut Vm, frame: Value, name: &str) -> bool {
    apply_template_in(vm, frame, name, Phase::Own)
        && apply_template_in(vm, frame, name, Phase::Children)
}

fn apply_template_in(vm: &mut Vm, frame: Value, name: &str, phase: Phase) -> bool {
    let Some(t) = engine(vm)
        .world
        .ui
        .templates
        .get(&name.to_ascii_lowercase())
        .cloned()
    else {
        return false;
    };
    let Some(id) = widgets::widget_of(vm, frame) else {
        return false;
    };
    let file = engine(vm)
        .world
        .ui
        .template_files
        .get(&name.to_ascii_lowercase())
        .cloned();
    let cx = Context {
        file: file.as_deref().unwrap_or("?"),
    };
    let env = engine(vm)
        .world
        .ui
        .template_envs
        .get(&name.to_ascii_lowercase())
        .copied();
    let globals = vm.globals;
    if let Some(env) = env {
        vm.globals = env;
    }
    // A template's scripts are its addon's code (or Blizzard's).
    let addon = engine(vm)
        .world
        .ui
        .template_addons
        .get(&name.to_ascii_lowercase())
        .cloned();
    let code_of = std::mem::replace(&mut engine_mut(vm).world.ui.code_of, addon);
    apply_in(vm, &cx, frame, id, &t, phase);
    engine_mut(vm).world.ui.code_of = code_of;
    vm.globals = globals;
    true
}

/// An intrinsic frame type's element onto a frame `CreateFrame` made.
pub fn apply_intrinsic(vm: &mut Vm, frame: Value, el: &Rc<Element>) {
    let Some(id) = widgets::widget_of(vm, frame) else {
        return;
    };
    // As a template's: its file, its globals, and its scripts its addon's
    // code (or Blizzard's), whoever creates the frame. An addon creating
    // Blizzard's `AuraContainer` while it loads would otherwise compile
    // the intrinsic's `method` handlers as its own code, and the
    // container's `OnLoad_Intrinsic` would run tainted (the client: SuuUI
    // makes aura containers while it loads, without an error).
    let key = el.attr("name").map(str::to_ascii_lowercase);
    let ui = &engine(vm).world.ui;
    let file = key.as_ref().and_then(|k| ui.template_files.get(k)).cloned();
    let env = key.as_ref().and_then(|k| ui.template_envs.get(k)).copied();
    let addon = key
        .as_ref()
        .and_then(|k| ui.template_addons.get(k))
        .cloned();
    let cx = Context {
        file: file.as_deref().unwrap_or("?"),
    };
    let globals = vm.globals;
    if let Some(env) = env {
        vm.globals = env;
    }
    let code_of = std::mem::replace(&mut engine_mut(vm).world.ui.code_of, addon);
    apply_as_intrinsic(vm, &cx, frame, id, el);
    engine_mut(vm).world.ui.code_of = code_of;
    vm.globals = globals;
}

/// An intrinsic type's element: its scripts stay, whatever the widget sets.
fn apply_as_intrinsic(vm: &mut Vm, cx: &Context, frame: Value, id: WidgetId, el: &Rc<Element>) {
    let was = std::mem::replace(&mut engine_mut(vm).world.ui.applying_intrinsic, true);
    apply(vm, cx, frame, id, el);
    engine_mut(vm).world.ui.applying_intrinsic = was;
}

/// Calls `frame:method(args…)`, if the widget has the method; an error is
/// the loader's.
fn call_method(vm: &mut Vm, cx: &Context, line: usize, frame: Value, method: &str, args: &[Value]) {
    let key = vm.str(method);
    let f = match vm.index(frame, key, None) {
        Ok(f) if !f.is_nil() => f,
        _ => return,
    };
    let mut call = vec![frame];
    call.extend_from_slice(args);
    if let Err(e) = vm.call(f, &call) {
        let msg = message(vm, e.value);
        cx.error(vm, line, format!("{method}: {msg}"));
    }
}

fn table_of(v: Value) -> Option<TableRef> {
    match v {
        Value::Table(t) => Some(t),
        _ => None,
    }
}

/// The secure delegate of a mixin's function, made once and shared.
fn mixin_delegate(vm: &mut Vm, f: wowlua_vm::FuncRef) -> Value {
    if let Some(&d) = engine(vm).world.ui.mixin_delegates.get(&f) {
        return d;
    }
    let d = super::secure_delegate(vm, Value::Func(f), true);
    widgets::keep(vm, d);
    engine_mut(vm).world.ui.mixin_delegates.insert(f, d);
    d
}

/// Which part of an element `apply_in` applies: its attributes, mixins and
/// key values (the whole template chain's, first), or its children and
/// scripts (a child's `OnLoad` sees the parent's key values: Blizzard's
/// `ButtonControllerMixin` reads `atlasName` of a derived template).
#[derive(Clone, Copy, PartialEq)]
enum Phase {
    Own,
    Children,
}

/// An element's attributes, mixins, children and scripts onto a widget,
/// after the templates it inherits.
fn apply(vm: &mut Vm, cx: &Context, frame: Value, id: WidgetId, el: &Rc<Element>) {
    apply_in(vm, cx, frame, id, el, Phase::Own);
    apply_in(vm, cx, frame, id, el, Phase::Children);
}

fn apply_in(vm: &mut Vm, cx: &Context, frame: Value, id: WidgetId, el: &Rc<Element>, phase: Phase) {
    crate::widgets::mark(crate::engine_mut(vm), id);
    let is_font_string = engine(vm).world.widgets[id.0].class == "FontString";
    for t in el.attr("inherits").unwrap_or("").split(',') {
        let t = t.trim();
        if t.is_empty() {
            continue;
        }
        // A font string inherits a font object's font.
        let g = vm.get_global(t);
        let is_font = widgets::widget_of(vm, g)
            .is_some_and(|w| engine(vm).world.widgets[w.0].class == "Font");
        if is_font && (is_font_string || el.tag.eq_ignore_ascii_case("EditBox")) {
            if phase == Phase::Own {
                call_method(vm, cx, el.line, frame, "SetFontObject", &[g]);
            }
            continue;
        }
        if !apply_template_in(vm, frame, t, phase) && phase == Phase::Own {
            engine_mut(vm)
                .world
                .warnings
                .push(format!("{}:{}: no template \"{t}\"", cx.file, el.line));
        }
    }
    if phase == Phase::Children {
        for child in el
            .children
            .iter()
            .filter(|c| !c.tag.eq_ignore_ascii_case("KeyValues"))
        {
            apply_child(vm, cx, frame, id, child);
        }
        // A button's text, once its font string is there.
        if let Some(text) = el.attr("text")
            && engine(vm).world.widgets[id.0].class != "FontString"
        {
            let g = vm.get_global(text);
            let v = if matches!(g, Value::Str(_)) {
                g
            } else {
                vm.str(text)
            };
            call_method(vm, cx, el.line, frame, "SetText", &[v]);
        }
        return;
    }
    let table = table_of(frame).expect("a widget is a table");
    for attr in ["mixin", "secureMixin"] {
        for m in el.attr(attr).unwrap_or("").split(',') {
            let m = m.trim();
            if m.is_empty() {
                continue;
            }
            match vm.get_global(m) {
                Value::Table(src) => {
                    let mut k = Value::Nil;
                    while let Some((key, v)) = vm.heap.table(src).next(k).ok().flatten() {
                        let _ = vm.heap.table_mut(table).set(key, v);
                        k = key;
                    }
                }
                // Not defined: skipped, without an error (recorded: the
                // camelot tutorial's keyboard frame, its mixins' file
                // missing from the client, loads without one).
                _ => engine_mut(vm)
                    .world
                    .warnings
                    .push(format!("{}:{}: mixin {m} is not a table", cx.file, el.line)),
            }
        }
    }
    // `<Mixins><Mixin key="X" [source="secure"]/></Mixins>`: from the
    // globals, or the secure environment's; with `secureDelegates`, its
    // functions as secure delegates (`super::secure_delegate`: the custom
    // aura container's inbound API, called by addons). Partitions are not
    // modelled.
    for list in el
        .children
        .iter()
        .filter(|c| c.tag.eq_ignore_ascii_case("Mixins"))
    {
        for m in &list.children {
            let Some(key) = m.attr("key") else { continue };
            let secure = m
                .attr("source")
                .is_some_and(|s| s.eq_ignore_ascii_case("secure"));
            let delegates = m.flag("secureDelegates");
            let globals = vm.globals;
            if secure && let Some(env) = engine(vm).world.ui.secure_env {
                vm.globals = env;
            }
            let src = vm.get_global(key);
            vm.globals = globals;
            match src {
                Value::Table(src) => {
                    let mut k = Value::Nil;
                    while let Some((key, v)) = vm.heap.table(src).next(k).ok().flatten() {
                        let v =
                            match v {
                                Value::Func(f) if delegates => mixin_delegate(vm, f),
                                // The private mixin shares the inbound one's
                                // functions (both made from a shared mixin): in
                                // the client they land in another partition, the
                                // delegate staying what addons call.
                                Value::Func(f)
                                    if engine(vm).world.ui.mixin_delegates.get(&f).is_some_and(
                                        |d| d.raw_eq(vm.heap.table(table).get(key)),
                                    ) =>
                                {
                                    k = key;
                                    continue;
                                }
                                v => v,
                            };
                        let _ = vm.heap.table_mut(table).set(key, v);
                        k = key;
                    }
                }
                _ => cx.error(vm, m.line, format!("mixin {key} is not a table")),
            }
        }
    }
    // A template's `parent` for a widget its element gave none
    // (`ContainerFrameTemplate`'s `ContainerFrameContainer`).
    if let Some(p) = el.attr("parent")
        && engine(vm).world.widgets[id.0].parent.is_none()
    {
        let g = vm.get_global(p);
        if widgets::widget_of(vm, g).is_some() {
            call_method(vm, cx, el.line, frame, "SetParent", &[g]);
        }
    }
    if let Some(key) = el.attr("parentKey")
        && let Some(p) = engine(vm).world.widgets[id.0].parent
    {
        let pt = engine(vm).world.widgets[p.0].table;
        let k = vm.str(key);
        let _ = vm.heap.table_mut(pt).set(k, frame);
    }
    if let Some(key) = el.attr("parentArray")
        && let Some(p) = engine(vm).world.widgets[id.0].parent
    {
        let pt = engine(vm).world.widgets[p.0].table;
        let k = vm.str(key);
        let list = match vm.heap.table(pt).get(k) {
            Value::Table(t) => t,
            _ => {
                let t = vm.new_table();
                let _ = vm.heap.table_mut(pt).set(k, Value::Table(t));
                t
            }
        };
        let n = vm.heap.table(list).border() as f64 + 1.0;
        let _ = vm.heap.table_mut(list).set(Value::Num(n), frame);
    }
    if el.flag("hidden") {
        engine_mut(vm).world.widgets[id.0].shown = false;
    }
    // Explicitly protected, as frames from a secure template are
    // (`IsProtected` true, true; `/stdtest engine`): Blizzard's restricted
    // environment hands out managed environments to those alone, and runs
    // a group header's configuration on children of an addon's templates
    // that inherit SecureFrameTemplate (oUF's headers, error-free in the
    // client).
    if el.flag("protected") {
        let w = &mut engine_mut(vm).world.widgets[id.0];
        w.protected = true;
        w.explicitly_protected = true;
    }
    if let Some(n) = el.number("id") {
        call_method(vm, cx, el.line, frame, "SetID", &[Value::Num(n)]);
    }
    // An element's own `<Anchors>` take the place of its `setAllPoints`, as
    // in game (the player frame's rest icon, 20x20 at its anchor).
    let anchored = el.child("Anchors").is_some_and(|a| !a.children.is_empty());
    if el.flag("setAllPoints") && !anchored {
        call_method(vm, cx, el.line, frame, "SetAllPoints", &[]);
    }
    for (attr, method) in FLAG_ATTRS {
        if el.flag(attr) {
            call_method(vm, cx, el.line, frame, method, &[Value::Bool(true)]);
        }
    }
    // Switches given either way (`snapToPixelGrid="false"`).
    for (attr, method) in [
        ("snapToPixelGrid", "SetSnapToPixelGrid"),
        ("horizTile", "SetHorizTile"),
        ("vertTile", "SetVertTile"),
    ] {
        if let Some(v) = el.attr(attr) {
            let on = v.trim().eq_ignore_ascii_case("true");
            call_method(vm, cx, el.line, frame, method, &[Value::Bool(on)]);
        }
    }
    // A texture's `rotation` is in degrees (`SetRotation` takes radians):
    // the whole texture turns about its center (Blizzard's group finder
    // divider, an 11 by 454 strip at 90, lies across the list's top).
    if let Some(deg) = el.number("rotation") {
        let class = engine(vm).world.widgets[id.0].class.clone();
        if widgets::is_a(engine(vm), &class, "Texture") {
            let rad = Value::Num(deg.to_radians());
            call_method(vm, cx, el.line, frame, "SetRotation", &[rad]);
        }
    }
    for (attr, method) in VALUE_ATTRS {
        if let Some(v) = el.attr(attr) {
            let v = match v.trim().parse::<f64>() {
                Ok(n) => Value::Num(n),
                Err(_) => vm.str(v),
            };
            // `useAtlasSize` sizes the texture as its atlas is
            // (`SetAtlas(atlas, true)`).
            if *attr == "atlas" && el.flag("useAtlasSize") {
                call_method(vm, cx, el.line, frame, method, &[v, Value::Bool(true)]);
                continue;
            }
            call_method(vm, cx, el.line, frame, method, &[v]);
        }
    }
    // `registerForClicks="LeftButtonUp, RightButtonUp"`: the clicks, one
    // argument each.
    if let Some(list) = el.attr("registerForClicks") {
        let clicks: Vec<Value> = list
            .split(',')
            .map(str::trim)
            .filter(|c| !c.is_empty())
            .map(|c| vm.str(c))
            .collect();
        call_method(vm, cx, el.line, frame, "RegisterForClicks", &clicks);
    }
    // A texture's `alpha` is its vertex color's too (SuuUI probe trees: the
    // character window's side tabs' `TabGlow`, `alpha="0"`, the model
    // controls' highlights, `alpha="0.4"`, and a statistics row's texture,
    // `alpha="0.3"`, report it as `GetVertexColor`'s alpha, their color
    // white).
    // Unverified: `GetAlpha` (kept as `SetAlpha` set it), and the alpha
    // `SetVertexColor` set before.
    if let Some(a) = el.number("alpha")
        && matches!(
            engine(vm).world.widgets[id.0].class.as_str(),
            "Texture" | "MaskTexture" | "Line"
        )
    {
        let props = engine(vm).world.widgets[id.0].props;
        let mut rgba = widgets::stored_args(vm, props, "VertexColor")
            .filter(|v| v.len() == 4)
            .unwrap_or_else(|| vec![Value::Num(1.0); 4]);
        rgba[3] = Value::Num(a);
        widgets::store(vm, id, "VertexColor", &rgba);
    }
    // A slider's or a status bar's range, then its value.
    if el.attr("minValue").is_some() || el.attr("maxValue").is_some() {
        let (min, max) = (
            el.number("minValue").unwrap_or(0.0),
            el.number("maxValue").unwrap_or(0.0),
        );
        call_method(
            vm,
            cx,
            el.line,
            frame,
            "SetMinMaxValues",
            &[Value::Num(min), Value::Num(max)],
        );
    }
    if let Some(v) = el.number("defaultValue") {
        call_method(vm, cx, el.line, frame, "SetValue", &[Value::Num(v)]);
    }
    // `font`: a font object's name (`GameFontHighlightRight`), else a file.
    if let Some(font) = el.attr("font")
        && !el.tag.eq_ignore_ascii_case("Font")
    {
        let g = vm.get_global(font);
        if widgets::widget_of(vm, g).is_some_and(|w| engine(vm).world.widgets[w.0].class == "Font")
        {
            call_method(vm, cx, el.line, frame, "SetFontObject", &[g]);
        } else {
            let height = el
                .child("FontHeight")
                .and_then(|h| h.child("AbsValue"))
                .and_then(|v| v.number("val"))
                .unwrap_or(12.0);
            let (file, flags) = (vm.str(font), vm.str(&super::xml_font_flags(el)));
            call_method(
                vm,
                cx,
                el.line,
                frame,
                "SetFont",
                &[file, Value::Num(height), flags],
            );
        }
    }
    if let Some(text) = el.attr("text")
        && engine(vm).world.widgets[id.0].class == "FontString"
    {
        // A global string's name or the text itself. A font string's is
        // set as XML sets it, before its font may come (a button's
        // `NormalFont` follows its `ButtonText`).
        let g = vm.get_global(text);
        let v = if matches!(g, Value::Str(_)) {
            g
        } else {
            vm.str(text)
        };
        crate::widgets::store_text(vm, id, Some(v));
    }
    for child in el
        .children
        .iter()
        .filter(|c| c.tag.eq_ignore_ascii_case("KeyValues"))
    {
        apply_child(vm, cx, frame, id, child);
    }
}

/// A `<Size>`'s or `<Offset>`'s dimensions: `x`/`y` or an `<AbsDimension>`.
/// Whether a widget shows text (a font object, a font string).
fn is_texture(vm: &Vm, id: WidgetId) -> bool {
    let e = engine(vm);
    let class = &e.world.widgets[id.0].class;
    widgets::is_a(e, class, "Texture") || widgets::is_a(e, class, "Line")
}

fn is_text(vm: &Vm, id: WidgetId) -> bool {
    let class = &engine(vm).world.widgets[id.0].class;
    class == "Font" || class == "FontString"
}

fn dims(el: &Element) -> (Option<f64>, Option<f64>) {
    let d = el.child("AbsDimension").map(|d| d.as_ref()).unwrap_or(el);
    (d.number("x"), d.number("y"))
}

/// The widget `relativeKey="$parent.$parent.Key"` names, from `frame`.
fn relative_key(vm: &mut Vm, frame: Value, path: &str) -> Value {
    let mut cur = frame;
    for part in path.split('.') {
        cur = if part.eq_ignore_ascii_case("$parent") {
            match widgets::widget_of(vm, cur).and_then(|w| engine(vm).world.widgets[w.0].parent) {
                Some(p) => Value::Table(engine(vm).world.widgets[p.0].table),
                None => return Value::Nil,
            }
        } else {
            let k = vm.str(part);
            match vm.index(cur, k, None) {
                Ok(v) => v,
                Err(_) => return Value::Nil,
            }
        };
        if cur.is_nil() {
            return cur;
        }
    }
    cur
}

fn apply_child(vm: &mut Vm, cx: &Context, frame: Value, id: WidgetId, child: &Rc<Element>) {
    let tag = child.tag.as_str();
    match tag.to_ascii_lowercase().as_str() {
        "size" => {
            let (x, y) = dims(child);
            match (x, y) {
                (Some(x), Some(y)) => {
                    call_method(
                        vm,
                        cx,
                        child.line,
                        frame,
                        "SetSize",
                        &[Value::Num(x), Value::Num(y)],
                    );
                }
                (Some(x), None) => {
                    call_method(vm, cx, child.line, frame, "SetWidth", &[Value::Num(x)])
                }
                (None, Some(y)) => {
                    call_method(vm, cx, child.line, frame, "SetHeight", &[Value::Num(y)])
                }
                (None, None) => {}
            }
        }
        "anchors" => {
            for a in &child.children {
                let point = a.attr("point").unwrap_or("TOPLEFT");
                let rel = if let Some(key) = a.attr("relativeKey") {
                    relative_key(vm, frame, key)
                } else if let Some(name) = a.attr("relativeTo") {
                    let parent = engine(vm).world.widgets[id.0].parent;
                    let name = resolve_name(vm, name, parent);
                    vm.get_global(&name)
                } else {
                    match engine(vm).world.widgets[id.0].parent {
                        Some(p) => Value::Table(engine(vm).world.widgets[p.0].table),
                        None => Value::Nil,
                    }
                };
                let rp = a.attr("relativePoint").unwrap_or(point).to_string();
                let (mut x, mut y) = (a.number("x"), a.number("y"));
                if let Some(o) = a.child("Offset") {
                    let (ox, oy) = dims(o);
                    x = x.or(ox);
                    y = y.or(oy);
                }
                let p = vm.str(point);
                let rp = vm.str(&rp);
                call_method(
                    vm,
                    cx,
                    a.line,
                    frame,
                    "SetPoint",
                    &[
                        p,
                        rel,
                        rp,
                        Value::Num(x.unwrap_or(0.0)),
                        Value::Num(y.unwrap_or(0.0)),
                    ],
                );
            }
        }
        "layers" => {
            // A region takes its `<Layer>`'s level (ARTWORK when none) and
            // sublevel, its own `textureSubLevel` first (SuuUI probe
            // trees: the client's regions report them, a `<HighlightTexture>`
            // HIGHLIGHT, set elsewhere).
            for layer in &child.children {
                let level = layer.attr("level").unwrap_or("ARTWORK").to_string();
                for region in &layer.children {
                    let built = build(vm, cx, region, Some(id));
                    let sublevel = region
                        .number("textureSubLevel")
                        .or_else(|| layer.number("textureSubLevel"))
                        .unwrap_or(0.0);
                    if let Some(rid) = built.and_then(|r| widgets::widget_of(vm, r)) {
                        let l = vm.str(&level);
                        widgets::store(vm, rid, "DrawLayer", &[l, Value::Num(sublevel)]);
                    }
                }
            }
        }
        "frames" => {
            for f in &child.children {
                build(vm, cx, f, Some(id));
            }
        }
        "animations" => {
            for g in &child.children {
                build(vm, cx, g, Some(id));
            }
        }
        // A mask's textures (`<MaskedTexture childKey="icon"/>`, or a
        // `target` by name): it masks each.
        "maskedtextures" => {
            let parent = engine(vm).world.widgets[id.0]
                .parent
                .map(|p| Value::Table(engine(vm).world.widgets[p.0].table));
            for m in &child.children {
                let target = match (m.attr("childKey"), m.attr("target"), parent) {
                    (Some(key), _, Some(p)) => relative_key(vm, p, key),
                    (None, Some(name), _) => {
                        let parent_id = engine(vm).world.widgets[id.0].parent;
                        let name = resolve_name(vm, name, parent_id);
                        vm.get_global(&name)
                    }
                    _ => Value::Nil,
                };
                if widgets::widget_of(vm, target).is_some() {
                    call_method(vm, cx, m.line, target, "AddMaskTexture", &[frame]);
                }
            }
        }
        // A texture's coordinates: its edges, or its corners
        // (`<Rect ULx=… LRy=…/>`).
        "texcoords" => {
            let args: Vec<Value> = match child.child("Rect") {
                Some(r) => ["ULx", "ULy", "LLx", "LLy", "URx", "URy", "LRx", "LRy"]
                    .iter()
                    .map(|k| Value::Num(r.number(k).unwrap_or(0.0)))
                    .collect(),
                None => [("left", 0.0), ("right", 1.0), ("top", 0.0), ("bottom", 1.0)]
                    .iter()
                    .map(|(k, d)| Value::Num(child.number(k).unwrap_or(*d)))
                    .collect(),
            };
            call_method(vm, cx, child.line, frame, "SetTexCoord", &args);
        }
        // A texture's color: its tint when it shows a file or an atlas,
        // else its plain color (`SetColorTexture`). Blizzard's flat panel
        // backgrounds (`FlatPanelBackgroundTemplate`, the Settings panel's)
        // tint corner atlases and fill their edges by
        // `<Color color="PANEL_BACKGROUND_COLOR"/>`: without it the panel
        // had no background, what lay behind it showing through.
        "color" if is_texture(vm, id) => {
            let rgba = super::xml_color(vm, child);
            let props = engine(vm).world.widgets[id.0].props;
            let shows_file = ["Texture", "Atlas"]
                .iter()
                .any(|k| widgets::stored_args(vm, props, k).is_some_and(|v| !v.is_empty()));
            let method = if shows_file {
                "SetVertexColor"
            } else {
                "SetColorTexture"
            };
            call_method(vm, cx, child.line, frame, method, &rgba);
        }
        // A texture's gradient (`<Gradient orientation><MinColor/>
        // <MaxColor/></Gradient>`): Blizzard's help tips' dark gold boxes
        // (a white texture shaded so) were white without it.
        "gradient" if is_texture(vm, id) => {
            let orientation = vm.str(child.attr("orientation").unwrap_or("HORIZONTAL"));
            let mut args = vec![orientation];
            for tag in ["MinColor", "MaxColor"] {
                match child.child(tag) {
                    Some(c) => args.extend(super::xml_color(vm, c)),
                    None => args.extend([Value::Num(1.0); 4]),
                }
            }
            widgets::store(vm, id, "Gradient", &args);
        }
        // A font's or font string's text color and shadow.
        "color" if is_text(vm, id) => {
            let rgba = super::xml_color(vm, child);
            call_method(vm, cx, child.line, frame, "SetTextColor", &rgba);
        }
        "shadow" if is_text(vm, id) => {
            let (x, y) = match child.child("Offset") {
                Some(o) => dims(o),
                None => (child.number("x"), child.number("y")),
            };
            call_method(
                vm,
                cx,
                child.line,
                frame,
                "SetShadowOffset",
                &[Value::Num(x.unwrap_or(0.0)), Value::Num(y.unwrap_or(0.0))],
            );
            let rgba = match child.child("Color") {
                Some(c) => super::xml_color(vm, c),
                None => [
                    Value::Num(0.0),
                    Value::Num(0.0),
                    Value::Num(0.0),
                    Value::Num(1.0),
                ],
            };
            call_method(vm, cx, child.line, frame, "SetShadowColor", &rgba);
        }
        "barcolor" => {
            let rgba = super::xml_color(vm, child);
            call_method(vm, cx, child.line, frame, "SetStatusBarColor", &rgba);
        }
        "scrollchild" => {
            for f in &child.children {
                if let Some(c) = build(vm, cx, f, Some(id)) {
                    call_method(vm, cx, f.line, frame, "SetScrollChild", &[c]);
                }
            }
        }
        "keyvalues" => {
            let table = table_of(frame).expect("a widget is a table");
            for kv in &child.children {
                let Some(key) = kv.attr("key") else { continue };
                let raw = kv.attr("value").unwrap_or("");
                let v = match kv.attr("type").map(str::to_ascii_lowercase).as_deref() {
                    Some("number") => raw.trim().parse().map(Value::Num).unwrap_or(Value::Nil),
                    Some("boolean") => Value::Bool(raw.eq_ignore_ascii_case("true")),
                    Some("global") => global_path(vm, raw.trim()),
                    Some("nil") => Value::Nil,
                    _ => vm.str(raw),
                };
                let k = match kv.attr("keyType").map(str::to_ascii_lowercase).as_deref() {
                    Some("number") => key.trim().parse().map(Value::Num).unwrap_or(Value::Nil),
                    _ => vm.str(key),
                };
                if !k.is_nil() {
                    let _ = vm.heap.table_mut(table).set(k, v);
                }
            }
        }
        "attributes" => {
            let attributes = engine(vm).world.widgets[id.0].attributes;
            for a in &child.children {
                let Some(name) = a.attr("name") else { continue };
                let raw = a.attr("value").unwrap_or("");
                let v = match a.attr("type").map(str::to_ascii_lowercase).as_deref() {
                    Some("number") => raw.trim().parse().map(Value::Num).unwrap_or(Value::Nil),
                    Some("boolean") => Value::Bool(raw.eq_ignore_ascii_case("true")),
                    Some("global") => vm.get_global(raw),
                    Some("nil") => Value::Nil,
                    _ => vm.str(raw),
                };
                let k = vm.heap.intern(name.to_lowercase().as_bytes());
                vm.heap.table_mut(attributes).set_str(k, v);
            }
        }
        "scripts" => {
            for s in &child.children {
                set_script(vm, cx, id, s);
            }
        }
        // The template's forbidden aspects (`DisableUntrustedLayoutScriptsTemplate`).
        "forbiddenaspects" => {
            let flags = child
                .children
                .iter()
                .filter_map(|a| a.attr("aspect"))
                .fold(0, |f, name| f | crate::protection::aspect_flag(name));
            crate::protection::add_aspects(vm, id, flags);
        }
        "buttontext" => {
            if let Some(fs) = build_as(vm, cx, child, "FontString", Some(id)) {
                call_method(vm, cx, child.line, frame, "SetFontString", &[fs]);
            }
        }
        // A frame's own font (an edit box's, a message frame's): the
        // element's font object, font and justification. Unverified: its
        // nested height, color and shadow.
        "fontstring"
            if [
                "EditBox",
                "MessageFrame",
                "ScrollingMessageFrame",
                "SimpleHTML",
            ]
            .iter()
            .any(|c| {
                let class = engine(vm).world.widgets[id.0].class.clone();
                crate::widgets::is_a(engine(vm), &class, c)
            }) =>
        {
            if let Some(inherits) = child.attr("inherits") {
                let font = vm.get_global(inherits);
                if !font.is_nil() {
                    call_method(vm, cx, child.line, frame, "SetFontObject", &[font]);
                }
            }
            if let Some(file) = child.attr("font") {
                let file = vm.str(file);
                let size = child
                    .attr("size")
                    .and_then(|s| s.parse::<f64>().ok())
                    .map_or(Value::Num(12.0), Value::Num);
                let flags = vm.str(child.attr("outline").unwrap_or(""));
                call_method(vm, cx, child.line, frame, "SetFont", &[file, size, flags]);
            }
            // SimpleHTML justifies by text type (unverified for this
            // element).
            let html = engine(vm).world.widgets[id.0].class == "SimpleHTML";
            for (attr, method) in [("justifyH", "SetJustifyH"), ("justifyV", "SetJustifyV")] {
                if html {
                    break;
                }
                if let Some(v) = child.attr(attr) {
                    let v = vm.str(v);
                    call_method(vm, cx, child.line, frame, method, &[v]);
                }
            }
        }
        _ if engine(vm).world.widgets[id.0].class == "AnimationGroup"
            && class_of(vm, tag)
                .is_some_and(|c| crate::widgets::is_a(engine(vm), &c, "Animation")) =>
        {
            build(vm, cx, child, Some(id));
        }
        _ => {
            if let Some((slot, method)) = TEXTURE_SLOTS
                .iter()
                .find(|(t, _)| t.eq_ignore_ascii_case(tag))
            {
                if let Some(tex) = build_as(vm, cx, child, "Texture", Some(id)) {
                    // A button state's texture in its state's layer.
                    if let Some(state) = slot.strip_suffix("Texture").filter(|s| {
                        [
                            "Normal",
                            "Pushed",
                            "Disabled",
                            "Highlight",
                            "Checked",
                            "DisabledChecked",
                        ]
                        .contains(s)
                    }) && let Some(tid) = widgets::widget_of(vm, tex)
                    {
                        let layer = vm.str(crate::buttons::state_layer(state));
                        widgets::store(vm, tid, "DrawLayer", &[layer, Value::Num(0.0)]);
                    }
                    call_method(vm, cx, child.line, frame, method, &[tex]);
                }
            } else if let Some((_, method)) =
                FONT_SLOTS.iter().find(|(t, _)| t.eq_ignore_ascii_case(tag))
                && let Some(style) = child.attr("style")
            {
                let font = vm.get_global(style);
                if !font.is_nil() {
                    call_method(vm, cx, child.line, frame, method, &[font]);
                }
            }
        }
    }
}

/// Builds an element under a tag of its own (`<NormalTexture>`) as a
/// region of `class`.
fn build_as(
    vm: &mut Vm,
    cx: &Context,
    el: &Rc<Element>,
    class: &str,
    parent: Option<WidgetId>,
) -> Option<Value> {
    let name = el.attr("name").map(|n| resolve_name(vm, n, parent));
    let region = widgets::create(vm, class, name, parent);
    let id = widgets::widget_of(vm, region)?;
    unforbid_file_widget(vm, id, parent);
    apply(vm, cx, region, id, el);
    fill_parent_if_unplaced(vm, cx, el, region, id);
    Some(region)
}

/// A texture the XML does not anchor fills its parent (SuuUI probe trees:
/// such textures, slot icons, glows, a button's state textures, report
/// `TOPLEFT` on their parent's and its size; textures made from Lua stay
/// unplaced), a sized one in a layer too (decision T326: Blizzard's group
/// finder category buttons, a `ResizeLayoutFrame`, measure their
/// atlas-sized cover so); a thumb not (the slider places it,
/// `bars::thumb_place`). Unverified: a sized one in the client,
/// font strings, lines, and whether the client places them at creation or
/// later. A font string so too (Blizzard's `ScrollingFontTemplate` anchors
/// none: the character window's side panes showed no description; in game
/// it shows), but a button's `ButtonText`, which the button centers.
fn fill_parent_if_unplaced(vm: &mut Vm, cx: &Context, el: &Element, region: Value, id: WidgetId) {
    let w = &engine(vm).world.widgets[id.0];
    if !matches!(w.class.as_str(), "Texture" | "MaskTexture" | "FontString")
        || w.parent.is_none()
        || !w.points.is_empty()
        || el.tag.eq_ignore_ascii_case("ButtonText")
    {
        return;
    }
    // A thumb the slider (or color select) places itself.
    if el.tag.to_ascii_lowercase().ends_with("thumbtexture") {
        return;
    }
    call_method(vm, cx, el.line, region, "SetAllPoints", &[]);
}

/// Compiles UI code: as the loading addon's ([`Registry::code_of`]), or
/// as Blizzard's, secure whatever the running code's taint (an addon's
/// `C_AddOns.LoadAddOn` or `CreateFrame` with Blizzard's template).
///
/// [`Registry::code_of`]: super::Registry::code_of
pub(crate) fn load_chunk(vm: &mut Vm, src: &[u8], chunk: &str) -> Result<Value, String> {
    match engine(vm).world.ui.code_of.clone() {
        Some(addon) => vm.load_as(src, chunk, &addon),
        None => {
            let saved = vm.taint().saved();
            vm.taint_mut().stack = wowlua_vm::Taint::SECURE;
            let r = vm.load(src, chunk);
            vm.taint_mut().restore(saved);
            r
        }
    }
}

/// Compiles Lua for a script handler; an error is the loader's.
fn compile(vm: &mut Vm, cx: &Context, line: usize, src: &str) -> Option<Value> {
    let chunk = format!("@{}", cx.file);
    // Blizzard's handler is made secure too, whoever makes the frame (an
    // addon's `CreateFrame` with `SecureHandlerClickTemplate`: its OnClick
    // runs the snippet on the user's click).
    // An addon's handler carries its taint, but making it leaves the running
    // execution's alone (Blizzard's secure group header applying an addon's
    // template with an inline `OnLoad`, then running its
    // `initialConfigFunction` for the new button).
    let blizzard = engine(vm).world.ui.code_of.is_none();
    match load_chunk(vm, src.as_bytes(), &chunk) {
        Ok(f) => match if blizzard {
            vm.call_secure(f, &[])
        } else {
            let saved = vm.taint().saved();
            let r = vm.call(f, &[]);
            vm.taint_mut().restore(saved);
            r
        } {
            Ok(r) => r.first().copied(),
            Err(e) => {
                let msg = message(vm, e.value);
                cx.error(vm, line, msg);
                None
            }
        },
        Err(e) => {
            cx.error(vm, line, e);
            None
        }
    }
}

/// One `<Scripts>` child: a handler from `function`, `method` or the body,
/// replacing the script or, with `inherit`, running with it (an
/// intrinsic's `intrinsicOrder` handlers are kept apart).
fn set_script(vm: &mut Vm, cx: &Context, id: WidgetId, s: &Element) {
    let name = s.tag.as_str();
    let handler = if let Some(f) = s.attr("function") {
        let g = vm.get_global(f);
        if g.is_nil() {
            return;
        }
        g
    } else if let Some(m) = s.attr("method") {
        // A method the frame lacks does nothing (recorded: the camelot
        // tutorial's keyboard frame, its mixin's file missing from the
        // client, loads without an error).
        let src = format!(
            "return function(self, ...) local f = self[{m:?}] if f ~= nil then return f(self, ...) end end"
        );
        match compile(vm, cx, s.line, &src) {
            Some(f) => f,
            None => return,
        }
    } else {
        // Lines as in the file. An empty element is a handler doing
        // nothing, replacing the inherited one (ElvUI's popup template
        // "removes the scripts added from GameDialogMixin" with empty
        // ones and logs in without StaticPopup_OnShow's error in the
        // client; Blizzard's artifact frame swallows the wheel with
        // `<OnMouseWheel />`; unverified whether GetScript answers it).
        let src = format!(
            "{}return function({}) {} end",
            "\n".repeat(s.line.saturating_sub(1)),
            script_params(name),
            s.text
        );
        match compile(vm, cx, s.line, &src) {
            Some(f) => f,
            None => return,
        }
    };
    // An intrinsic type's own handlers stay, whatever the widget sets
    // (`ScrollingMessageFrame`'s `OnPreLoad`, before the frame's OnLoad).
    // Without an order, before (`DropdownButton`'s `OnLoad_Intrinsic`;
    // unverified).
    let in_intrinsic = engine(vm).world.ui.applying_intrinsic;
    if let Some(order) = s
        .attr("intrinsicOrder")
        .or(in_intrinsic.then_some("precall"))
    {
        let props = engine(vm).world.widgets[id.0].props;
        let key = vm.str(&widgets::intrinsic_key(&order.to_ascii_lowercase(), name));
        let _ = vm.heap.table_mut(props).set(key, handler);
        engine_mut(vm).world.widgets[id.0].intrinsic_scripts = true;
        // (Its handlers count for every script: `has_handler`.)
        engine_mut(vm).world.updates(id.0);
        return;
    }
    let order = s.attr("inherit").map(str::to_ascii_lowercase);
    let scripts = engine(vm).world.widgets[id.0].scripts;
    let key = vm.heap.intern(name.as_bytes());
    let existing = vm.heap.table(scripts).get_str(key);
    // `inherit="prepend"`: the inherited handler first, then this one;
    // `"append"`: this one, then the inherited (Blizzard's chat edit box
    // sets `chatFrame` in an appended OnLoad its inherited OnLoad reads).
    let f = match (order.as_deref(), existing) {
        (Some("prepend"), Value::Func(_)) => chain(vm, existing, handler),
        (Some("append"), Value::Func(_)) => chain(vm, handler, existing),
        _ => handler,
    };
    vm.heap.table_mut(scripts).set_str(key, f);
    crate::widgets::script_stored(vm, id, name);
    crate::widgets::input_script_set(vm, id, name);
}

/// A handler running `a`, then `b`.
fn chain(vm: &mut Vm, a: Value, b: Value) -> Value {
    let src = "local a, b = ...\nreturn function(...) a(...) return b(...) end";
    match vm.load(src.as_bytes(), "=[engine]") {
        Ok(f) => vm
            .call(f, &[a, b])
            .ok()
            .and_then(|r| r.first().copied())
            .unwrap_or(b),
        Err(_) => b,
    }
}

/// Loads a Lua file of Blizzard's code as secure code, with the addon's
/// name and table as its arguments.
pub fn run_lua(vm: &mut Vm, path: &Path, chunk: &str, addon: &str, addon_table: Value) {
    let src = match wowlua_host::read(path) {
        Ok(s) => s,
        // A TOC's file that is not there: skipped, as the client skips it
        // (it only logs; unverified wording).
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            engine_mut(vm)
                .world
                .warnings
                .push(format!("{chunk}: no such file"));
            return;
        }
        Err(e) => {
            engine_mut(vm).world.ui.errors.push(format!("{chunk}: {e}"));
            return;
        }
    };
    let f = match load_chunk(vm, &src, &format!("@{chunk}")) {
        Ok(f) => f,
        Err(e) => {
            engine_mut(vm).world.ui.errors.push(e);
            return;
        }
    };
    let name = vm.str(addon);
    match vm.call_secure(f, &[name, addon_table]) {
        // A file's result is its module (`import(".Name")`).
        Ok(results) => {
            if let Some(&v) = results.first()
                && !v.is_nil()
            {
                crate::widgets::keep(vm, v);
                let key = module_key(chunk.strip_suffix(".lua").unwrap_or(chunk));
                engine_mut(vm).world.ui.modules.insert(key, v);
            }
        }
        Err(e) => {
            let msg = message(vm, e.value);
            engine_mut(vm).world.ui.errors.push(msg);
        }
    }
}

/// A module's key: its path without the extension, `/`-separated,
/// lower-case.
pub fn module_key(path: &str) -> String {
    path.replace('\\', "/").to_ascii_lowercase()
}

/// `import(".Name")`, `import(".Dir.Name")`: the result of the file
/// `Name.lua` (in `Dir`) next to the calling file, which loaded before;
/// `import("AddOn.Dir.Name")`: the file `Dir/Name.lua` of the addon
/// `AddOn` (Forever 70707's `FrameControlsManager.lua` imports
/// `Blizzard_GamepadSmartNavigation.SecureHelper`, an addon it depends on;
/// decision T576). Unverified: the error for a module not there.
pub fn import(vm: &mut Vm, c: wowlua_vm::NativeCall) -> wowlua_vm::stdlib::Ret {
    let name = vm.check_str(&c, 1)?;
    let name = String::from_utf8_lossy(vm.str_bytes(name)).into_owned();
    let found = match name.strip_prefix('.') {
        Some(rel) => {
            let source = vm
                .caller_source(1)
                .map(|s| String::from_utf8_lossy(&s).into_owned())
                .unwrap_or_default();
            let file = source
                .strip_prefix('@')
                .unwrap_or(&source)
                .replace('\\', "/");
            let dir = file.rsplit_once('/').map_or("", |(d, _)| d);
            let key = module_key(&format!("{dir}/{}", rel.replace('.', "/")));
            engine(vm).world.ui.modules.get(&key).copied()
        }
        None => {
            // The addon's folder, wherever the files' names start.
            let rel = format!("addons/{}", module_key(&name.replace('.', "/")));
            let modules = &engine(vm).world.ui.modules;
            modules
                .iter()
                .find(|(key, _)| {
                    key.strip_suffix(rel.as_str())
                        .is_some_and(|head| head.is_empty() || head.ends_with('/'))
                })
                .map(|(_, &v)| v)
        }
    };
    match found {
        Some(v) => Ok(vec![v]),
        None => Err(vm.error(&format!("import: no module '{name}'"))),
    }
}

/// `C_XMLUtil.GetTemplateInfo(name)`: a template's widget type, size (its
/// own or inherited `<Size>`), key values, what it inherits and where it is
/// defined; nothing for a name that is no template.
pub fn template_info(vm: &mut Vm, c: wowlua_vm::NativeCall) -> wowlua_vm::stdlib::Ret {
    let name = vm.check_str(&c, 1)?;
    let name = String::from_utf8_lossy(vm.str_bytes(name)).to_ascii_lowercase();
    // A template the client lacks: nothing (the full probe, 1.60.1).
    let Some(el) = engine(vm).world.ui.templates.get(&name).cloned() else {
        return Ok(vec![]);
    };
    // The size: this template's, else the first inherited one's.
    fn size_of(vm: &Vm, el: &Element, depth: usize) -> Option<(f64, f64)> {
        if let Some(s) = el.child("Size") {
            let (x, y) = dims(s);
            return Some((x.unwrap_or(0.0), y.unwrap_or(0.0)));
        }
        if depth > 16 {
            return None;
        }
        el.attr("inherits")?.split(',').find_map(|t| {
            let t = engine(vm)
                .world
                .ui
                .templates
                .get(&t.trim().to_ascii_lowercase())?;
            size_of(vm, t, depth + 1)
        })
    }
    let (w, h) = size_of(vm, &el, 0).unwrap_or((0.0, 0.0));
    let class = class_of(vm, &el.tag).unwrap_or_else(|| el.tag.clone());
    let file = engine(vm)
        .world
        .ui
        .template_files
        .get(&name)
        .cloned()
        .unwrap_or_default();
    let t = vm.new_table();
    let kvs = vm.new_table();
    if let Some(list) = el.child("KeyValues") {
        for (i, kv) in list.children.iter().enumerate() {
            let entry = vm.new_table();
            for (field, v) in [
                ("key", kv.attr("key").unwrap_or("")),
                ("keyType", kv.attr("keyType").unwrap_or("string")),
                ("type", kv.attr("type").unwrap_or("string")),
                ("value", kv.attr("value").unwrap_or("")),
            ] {
                let k = vm.str(field);
                let v = vm.str(v);
                let _ = vm.heap.table_mut(entry).set(k, v);
            }
            let _ = vm
                .heap
                .table_mut(kvs)
                .set(Value::Num((i + 1) as f64), Value::Table(entry));
        }
    }
    let fields = [
        ("type", vm.str(&class)),
        ("width", Value::Num(w)),
        ("height", Value::Num(h)),
        ("keyValues", Value::Table(kvs)),
        ("sourceLocation", vm.str(&file)),
    ];
    for (k, v) in fields {
        let k = vm.str(k);
        let _ = vm.heap.table_mut(t).set(k, v);
    }
    if let Some(inherits) = el.attr("inherits") {
        let k = vm.str("inherits");
        let v = vm.str(inherits);
        let _ = vm.heap.table_mut(t).set(k, v);
    }
    Ok(vec![Value::Table(t)])
}

/// A global by its dotted path (`MoneyDenominationDisplayType.Copper`).
fn global_path(vm: &mut Vm, path: &str) -> Value {
    let mut parts = path.split('.');
    let mut v = vm.get_global(parts.next().unwrap_or(""));
    for part in parts {
        let Value::Table(t) = v else {
            return Value::Nil;
        };
        let k = vm.str(part);
        v = vm.heap.table(t).get(k);
    }
    v
}
