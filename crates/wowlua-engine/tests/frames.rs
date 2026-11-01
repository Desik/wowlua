//! Frames, scripts and events in the engine.

#![allow(clippy::disallowed_methods)] // (tests set up their files on the disk)

use wowlua_vm::{Value, Vm};

fn vm() -> Vm {
    let mut vm = Vm::wow();
    wowlua_engine::install(&mut vm, "camelot").expect("installs");
    vm
}

fn text(vm: &Vm, v: Value) -> String {
    match v {
        Value::Str(s) => String::from_utf8_lossy(vm.str_bytes(s)).into_owned(),
        Value::Num(n) => n.to_string(),
        other => format!("{other:?}"),
    }
}

/// Runs `src` as addon code; its results as text.
fn run(vm: &mut Vm, src: &str) -> Vec<String> {
    let f = vm
        .load_as(src.as_bytes(), "@MyAddon/main.lua", "MyAddon")
        .expect("compiles");
    match vm.call(f, &[]) {
        Ok(r) => r.into_iter().map(|v| text(vm, v)).collect(),
        Err(e) => panic!("{}", text(vm, e.value)),
    }
}

#[test]
fn frames_are_tables_with_their_class_methods() {
    let mut vm = vm();
    let r = run(
        &mut vm,
        "local f = CreateFrame('Frame', 'MyFrame', UIParent)
local b = CreateFrame('button', nil, f)
return type(f), type(f[0]), f == MyFrame and 'named' or 'not', f:GetName(), f:GetObjectType(), b:GetObjectType(),
    tostring(b:IsObjectType('Frame')), tostring(b:GetParent() == f), tostring(f:IsShown()), type(f.Show), tostring(f.Show == b.Show)",
    );
    assert_eq!(
        r,
        [
            "table", "userdata", "named", "MyFrame", "Frame", "Button", "true", "true", "true",
            "function", "false"
        ]
    );
}

#[test]
fn events_reach_registered_frames_in_order() {
    let mut vm = vm();
    run(
        &mut vm,
        "Seen = {}
for i = 1, 3 do
    local f = CreateFrame('Frame')
    f:SetScript('OnEvent', function(self, event, arg) Seen[#Seen + 1] = i .. event .. tostring(arg) end)
    if i ~= 2 then f:RegisterEvent('PLAYER_LOGIN') end
    if i == 2 then f:RegisterUnitEvent('UNIT_HEALTH', 'target') end
end",
    );
    let unit = vm.str("player");
    wowlua_engine::widgets::fire_event(&mut vm, "PLAYER_LOGIN", &[]);
    wowlua_engine::widgets::fire_event(&mut vm, "UNIT_HEALTH", &[unit]);
    let target = vm.str("target");
    wowlua_engine::widgets::fire_event(&mut vm, "UNIT_HEALTH", &[target]);
    let r = run(&mut vm, "return table.concat(Seen, ',')");
    assert_eq!(r, ["1PLAYER_LOGINnil,3PLAYER_LOGINnil,2UNIT_HEALTHtarget"]);
}

#[test]
fn region_count_tracks_regions_not_child_frames() {
    let mut vm = vm();
    let r = run(
        &mut vm,
        "local f = CreateFrame('Frame')
local other = CreateFrame('Frame')
local empty = f:GetNumRegions()
local child = CreateFrame('Frame', nil, f)
child:CreateTexture()
local texture = f:CreateTexture()
f:CreateFontString()
f:CreateMaskTexture()
f:CreateLine()
texture:Hide()
local populated = f:GetNumRegions()
local enumerated = select('#', f:GetRegions())
texture:SetParent(other)
return empty, populated, enumerated, f:GetNumRegions(), other:GetNumRegions(), f:GetNumChildren()",
    );
    assert_eq!(r, ["0", "4", "4", "3", "1", "1"]);
}

#[test]
fn a_failing_handler_does_not_stop_the_others() {
    let mut vm = vm();
    run(
        &mut vm,
        "Errors, Ran = {}, {}
seterrorhandler(function(e) Errors[#Errors + 1] = e end)
for i = 1, 2 do
    local f = CreateFrame('Frame')
    f:RegisterEvent('PLAYER_LOGIN')
    f:SetScript('OnEvent', function() Ran[#Ran + 1] = i if i == 1 then error('boom') end end)
end",
    );
    wowlua_engine::widgets::fire_event(&mut vm, "PLAYER_LOGIN", &[]);
    let r = run(&mut vm, "return #Ran, #Errors, Errors[1]");
    assert_eq!(r[0], "2");
    assert_eq!(r[1], "1");
    assert!(r[2].ends_with("boom"), "{}", r[2]);
}

#[test]
fn draw_layer_switches_filter_the_scene_and_can_be_reenabled() {
    let mut vm = vm();
    run(
        &mut vm,
        "LayerFrame = CreateFrame('Frame', nil, UIParent)
LayerFrame:SetSize(100, 100)
LayerFrame:SetPoint('CENTER')
local function texture(frame, layer)
    local t = frame:CreateTexture(nil, layer)
    t:SetAllPoints()
    t:SetColorTexture(1, 1, 1)
    return t
end
LayerBackground = texture(LayerFrame, 'BACKGROUND')
LayerArtwork = texture(LayerFrame, 'ARTWORK')
LayerChild = CreateFrame('Frame', nil, LayerFrame)
LayerChild:SetAllPoints()
LayerChild:SetUsingParentLevel(true)
texture(LayerChild, 'BACKGROUND')
NewLayerTexture = texture",
    );
    let quads = |vm: &mut Vm| {
        wowlua_engine::scene::build(vm)
            .items
            .iter()
            .filter(|item| matches!(item, wowlua_engine::scene::Item::Quad(_)))
            .count()
    };
    assert_eq!(quads(&mut vm), 3);
    run(&mut vm, "LayerFrame:DisableDrawLayer('BACKGROUND')");
    assert_eq!(quads(&mut vm), 2);
    let shown = run(&mut vm, "return tostring(LayerBackground:IsShown())");
    assert_eq!(shown, ["true"]);
    run(
        &mut vm,
        "NewLayerTexture(LayerFrame, 'BACKGROUND')
LayerFrame:DisableDrawLayer('BACKGROUND')
LayerBackground:Show()",
    );
    assert_eq!(quads(&mut vm), 2);
    run(&mut vm, "LayerArtwork:SetDrawLayer('BACKGROUND')");
    assert_eq!(quads(&mut vm), 1);
    run(&mut vm, "LayerFrame:EnableDrawLayer('BACKGROUND')");
    assert_eq!(quads(&mut vm), 4);
    run(&mut vm, "LayerBackground:Hide()");
    assert_eq!(quads(&mut vm), 3);
    run(&mut vm, "LayerFrame:DisableDrawLayer('BACKGROUND')");
    assert_eq!(quads(&mut vm), 1);
    run(&mut vm, "LayerChild:DisableDrawLayer('BACKGROUND')");
    assert_eq!(quads(&mut vm), 0);
    run(&mut vm, "LayerChild:EnableDrawLayer('BACKGROUND')");
    assert_eq!(quads(&mut vm), 1);
    run(&mut vm, "LayerFrame:EnableDrawLayer('BACKGROUND')");
    assert_eq!(quads(&mut vm), 3);
}

#[test]
fn zero_texture_files_clear_regions_and_button_artwork() {
    let mut vm = vm();
    run(
        &mut vm,
        "ZeroTextureFrame = CreateFrame('CheckButton', nil, UIParent)
ZeroTextureFrame:SetSize(30, 30)
ZeroTextureFrame:SetPoint('CENTER')
ZeroTextureRegion = ZeroTextureFrame:CreateTexture(nil, 'BACKGROUND')
ZeroTextureRegion:SetAllPoints()
ZeroTextureRegion:SetTexture(134400)",
    );
    assert!(!wowlua_engine::scene::build(&mut vm).items.is_empty());
    run(
        &mut vm,
        "ZeroTextureRegion:SetTexture(0)
ZeroTextureFrame:SetNormalTexture(0)
ZeroTextureFrame:SetPushedTexture(0)",
    );
    assert!(wowlua_engine::scene::build(&mut vm).items.is_empty());
    let r = run(
        &mut vm,
        "return tostring(ZeroTextureRegion:GetTexture()), tostring(ZeroTextureFrame:GetNormalTexture():GetTexture())",
    );
    assert_eq!(r, ["nil", "nil"]);
    run(&mut vm, "ZeroTextureRegion:SetTexture(134400)");
    assert!(!wowlua_engine::scene::build(&mut vm).items.is_empty());
}

#[test]
fn unknown_events_and_scripts_fail() {
    let mut vm = vm();
    let r = run(
        &mut vm,
        "local f = CreateFrame('Frame')
return select(2, pcall(f.RegisterEvent, f, 'NO_SUCH_EVENT')), select(2, pcall(f.SetScript, f, 'OnClick', print))",
    );
    assert_eq!(
        r,
        [
            "Frame:RegisterEvent(): Frame:RegisterEvent(): Attempt to register unknown event \"NO_SUCH_EVENT\"",
            "Frame:SetScript(): Cannot assign script handler for 'onclick' (script type not supported by this object)\nLua Taint: MyAddon"
        ]
    );
}

#[test]
fn show_and_hide_run_scripts_when_visibility_changes() {
    let mut vm = vm();
    let r = run(
        &mut vm,
        "local log = {}
local p = CreateFrame('Frame')
local c = CreateFrame('Frame', nil, p)
for name, f in pairs({ p = p, c = c }) do
    f:SetScript('OnShow', function() log[#log + 1] = name .. '+' end)
    f:SetScript('OnHide', function() log[#log + 1] = name .. '-' end)
end
p:Hide()        -- c-, p- (children first; c was visible)
c:Hide()        -- nothing: c not visible
p:Show()        -- p+
c:Show()        -- c+
p:Show()        -- nothing
return table.concat(log, ' '), tostring(c:IsVisible())",
    );
    assert_eq!(r, ["c- p- p+ c+", "true"]);
}

/// A C error's "Lua Taint:" line names the addon whose code made the
/// failing call, though a library's value tainted the execution after a C
/// call (the user's client: ElvUI's Minimap.lua, a string from
/// ElvUI_Libraries' `utf8sub`, "Lua Taint: ElvUI").
#[test]
fn a_c_errors_taint_line_names_the_calling_addon() {
    let mut vm = vm();
    let lib = vm
        .load_as(
            b"function LibCut(s) return string.sub(s, 1, 3) end",
            "@MyLib/lib.lua",
            "MyLib",
        )
        .expect("compiles");
    vm.call(lib, &[]).expect("runs");
    let r = run(
        &mut vm,
        "local fs = UIParent:CreateFontString(nil, 'OVERLAY')
local text = LibCut('abcdef')
fs:SetTextColor(1, 1, 1)
local tainter = select(2, issecurevariable('LibCut'))
return select(2, pcall(fs.SetText, fs, text)), tainter",
    );
    assert_eq!(
        r,
        [
            "FontString:SetText(): Font not set\nLua Taint: MyAddon",
            "MyLib"
        ]
    );
}

#[test]
fn layout_and_stored_setters() {
    let mut vm = vm();
    let r = run(
        &mut vm,
        "local f = CreateFrame('Frame', nil, UIParent)
f:SetSize(100, 50)
f:SetPoint('BOTTOMLEFT', 10, 20)
f:SetFrameStrata('HIGH')
local fs = f:CreateFontString(nil, 'OVERLAY')
local noFont = select(2, pcall(fs.SetText, fs, 'hi'))
fs:SetFont('Fonts\\FRIZQT__.TTF', 12, '')
fs:SetText('hi')
return f:GetLeft(), f:GetTop(), f:GetFrameStrata(), fs:GetText(), fs:GetObjectType(), select(2, pcall(f.GetBoundsRect, f)), f:GetFrameLevel(), noFont",
    );
    assert_eq!(
        r,
        [
            "10",
            "70",
            "HIGH",
            "hi",
            "FontString",
            "10",
            "1",
            "FontString:SetText(): Font not set\nLua Taint: MyAddon"
        ]
    );
}

#[test]
fn stored_methods_is_enable_and_no_ops() {
    let mut vm = vm();
    let r = run(
        &mut vm,
        "local f = CreateFrame('Frame')
f:SetMovable(true)
f:EnableMouse(true)
f:SetToplevel(true)
local fs = f:CreateFontString()
local ok = fs:SetFont('Fonts\\\\FRIZQT__.TTF', 14, '')
f:RegisterForDrag('LeftButton')
local b = CreateFrame('Button')
Seen = {}
b:SetScript('OnDisable', function() Seen[#Seen + 1] = 'off' end)
b:Disable()
b:Disable()
local t = f:CreateTexture()
t:SetVertexColor(1, 0, 0)
local r, g, bb, a = t:GetVertexColor()
return tostring(f:IsMovable()), tostring(f:IsMouseEnabled()), tostring(f:IsToplevel()), tostring(ok),
    select(2, fs:GetFont()), tostring(b:IsEnabled()), table.concat(Seen, ','), r .. g .. bb .. a",
    );
    // A 14 high font keeps 13.999999046326 on camelot (`/stdtest engine`,
    // printed to 14 digits): the single float 13.999999046325684.
    assert_eq!(
        r,
        [
            "true",
            "true",
            "true",
            "true",
            "13.999999046325684",
            "false",
            "off",
            "1001"
        ]
    );
}

#[test]
fn unit_events_report_their_units() {
    let mut vm = vm();
    let r = run(
        &mut vm,
        "local f = CreateFrame('Frame')
f:RegisterUnitEvent('UNIT_HEALTH', 'player', 'target')
f:RegisterEvent('PLAYER_LOGIN')
local a, u1, u2 = f:IsEventRegistered('UNIT_HEALTH')
local b, none = f:IsEventRegistered('PLAYER_LOGIN')
return tostring(a), u1, u2, tostring(b), tostring(none), tostring(f:IsEventRegistered('BAG_UPDATE')),
    tostring(GetFramesRegisteredForEvent('PLAYER_LOGIN') == f), tostring(C_EventUtils.IsEventValid('PLAYER_LOGIN')),
    tostring(C_EventUtils.IsEventValid('NOPE'))",
    );
    assert_eq!(
        r,
        [
            "true", "player", "target", "true", "nil", "false", "true", "true", "false"
        ]
    );
}

#[test]
fn templates_frame_types_and_fonts() {
    let mut vm = vm();
    wowlua_engine::controls::install(&mut vm, "E");
    let r = run(
        &mut vm,
        "E.defineTemplate('MyTemplate', function(f) f.fromTemplate = true end)
E.defineFrameType('MyType', 'Button', function(f) f.custom = true end)
local f = CreateFrame('Frame', nil, UIParent, 'MyTemplate, BackdropTemplate')
local unknown = select(2, pcall(CreateFrame, 'Frame', nil, nil, 'NoSuchTemplate'))
local c = CreateFrame('mytype')
CreateFont('MyFont')
local fs = f:CreateFontString()
fs:SetFontObject('MyFont')
local ok = pcall(fs.SetFontObject, fs, 'NoSuchFont')
return tostring(f.fromTemplate), tostring(c.custom), c:GetObjectType(), #E.warnings(), tostring(ok),
    tostring(MyFont:IsObjectType('Font')), unknown",
    );
    // BackdropTemplate is Blizzard's (not loaded: a warning); a name nobody
    // defines fails as in the client.
    assert_eq!(
        r,
        [
            "true",
            "true",
            "Button",
            "1",
            "false",
            "true",
            "CreateFrame(): Couldn't find inherited node 'NoSuchTemplate'\nLua Taint: MyAddon"
        ]
    );
}

#[test]
fn drags_and_resize_grips() {
    let mut vm = vm();
    wowlua_engine::controls::install(&mut vm, "E");
    let r = run(
        &mut vm,
        "local f = CreateFrame('Frame', nil, UIParent)
f:SetSize(100, 50)
f:SetPoint('CENTER')
f:SetResizeBounds(80, 40, 120, 60)
f:SetMovable(true)
f:SetScript('OnDragStart', function(self) self:StartMoving() end)
f:SetScript('OnDragStop', function(self) self:StopMovingOrSizing() end)
local s = f:GetEffectiveScale()
E.drag(f, 10 * s, 20 * s)
local left, bottom = f:GetLeft(), f:GetBottom()
local grip = CreateFrame('Button', nil, f)
grip:SetScript('OnMouseDown', function() f:StartSizing('BOTTOMRIGHT') end)
grip:SetScript('OnMouseUp', function() f:StopMovingOrSizing() end)
E.sizeBy(grip, 50 * s, 5 * s)
local p = f:GetPoint(1)
return left, bottom, f:GetWidth(), f:GetHeight(), p, f:GetTop(), #E.frames()",
    );
    // Centered 100 x 50 on 1920 x 1080: left 910, bottom 515; moved by (10, 20)
    // (the drags in screen units: times the effective scale).
    // The top stays at 585 as the frame grows to its maximum width.
    assert_eq!(r, ["920", "535", "120", "55", "TOPLEFT", "585", "4"]);
}

#[test]
fn animation_groups_play_and_finish_on_the_frame_clock() {
    let mut vm = vm();
    let r = run(
        &mut vm,
        "Seen = {}
local f = CreateFrame('Frame', nil, UIParent)
local g = f:CreateAnimationGroup()
local a = g:CreateAnimation('Alpha')
a:SetDuration(0.5)
local b = g:CreateAnimation('Translation')
b:SetDuration(0.25)
b:SetOrder(2)
g:SetScript('OnPlay', function() Seen[#Seen + 1] = 'play' end)
g:SetScript('OnFinished', function() Seen[#Seen + 1] = 'finished' end)
g:Play()
return tostring(g:IsPlaying()), g:GetDuration(), a:GetObjectType(), #{ g:GetAnimations() }, #{ f:GetAnimationGroups() }",
    );
    assert_eq!(r, ["true", "0.75", "Alpha", "2", "1"]);
    wowlua_engine::clock::advance_frames(&mut vm, 30, 1000.0 / 60.0);
    let mid = run(&mut vm, "return table.concat(Seen, ',')");
    wowlua_engine::clock::advance_frames(&mut vm, 30, 1000.0 / 60.0);
    let end = run(&mut vm, "return table.concat(Seen, ',')");
    assert_eq!(mid, ["play"]);
    assert_eq!(end, ["play,finished"]);
}

/// A hidden edit box's `SetText`: its `OnTextChanged` waits until the box
/// shows, then comes in that frame after `OnUpdate` (as `/stdtest engine
/// textchange` recorded on Forever).
#[test]
fn a_hidden_edit_box_changes_text_once_it_shows() {
    let mut vm = vm();
    wowlua_engine::controls::install(&mut vm, "mock");
    run(
        &mut vm,
        "local log = {}
_G.textLog = log
local parent = CreateFrame('Frame', nil, UIParent)
parent:Hide()
local box = CreateFrame('EditBox', nil, parent)
box:SetAutoFocus(false)
box:SetScript('OnTextChanged', function(self, userInput)
    log[#log + 1] = 'changed(' .. tostring(userInput) .. ', ' .. self:GetText() .. ', visible ' .. tostring(self:IsVisible()) .. ')'
end)
box:SetText('hidden')
log[#log + 1] = 'set while hidden'
local frames = 0
local ticker = CreateFrame('Frame')
ticker:SetScript('OnUpdate', function()
    frames = frames + 1
    log[#log + 1] = 'update' .. frames
    if frames == 3 then
        parent:Show()
        log[#log + 1] = 'parent shown'
    elseif frames == 6 then
        parent:Hide()
        box:SetText('')
        log[#log + 1] = 'hidden, set empty'
        parent:Show()
        log[#log + 1] = 'shown again in the same frame'
    elseif frames == 9 then
        ticker:SetScript('OnUpdate', nil)
    end
end)
mock.advanceFrames(10)",
    );
    let r = run(&mut vm, "return table.concat(textLog, ' | ')");
    assert_eq!(
        r,
        [
            "set while hidden | update1 | update2 | update3 | parent shown | changed(false, hidden, visible true) | update4 | update5 | update6 | hidden, set empty | shown again in the same frame | changed(false, , visible true) | update7 | update8 | update9"
        ]
    );
}

/// `CreateFrame`'s fifth argument is the frame's ID (`GetID`), set before
/// the templates run (Blizzard's action buttons read it in `OnLoad`).
#[test]
fn create_frame_sets_the_id() {
    let mut vm = vm();
    let r = run(
        &mut vm,
        "local f = CreateFrame('CheckButton', nil, UIParent, nil, 7)
local g = CreateFrame('Frame')
return f:GetID(), g:GetID()",
    );
    assert_eq!(r, ["7", "0"]);
}

/// `SetDrawLayer(layer)` without a sublevel sets sublevel 0 (documented),
/// which `GetDrawLayer` answers (Blizzard's menus raise a pooled texture a
/// sublevel above it); `SetTextToFit` sets the text.
#[test]
fn a_draw_layer_set_alone_has_sublevel_zero_and_text_fits() {
    let mut vm = vm();
    let r = run(
        &mut vm,
        "local f = CreateFrame('Frame', nil, UIParent)
local t = f:CreateTexture(nil, 'BACKGROUND', nil, 3)
t:SetDrawLayer('ARTWORK')
local layer, sublevel = t:GetDrawLayer()
local s = f:CreateFontString()
s:SetFont('Fonts\\\\FRIZQT__.TTF', 12, '')
s:SetTextToFit('Learning')
return layer, sublevel, s:GetText()",
    );
    assert_eq!(r, ["ARTWORK", "0", "Learning"]);
}

/// UIParent has the screen's scale (768 over 1080 on the default screen,
/// decision T237): its children lay out and measure in its units; a frame
/// ignoring its parents' scale at the same scale measures alike, and a
/// frame without a parent counts in units of the unscaled screen.
#[test]
fn uiparent_has_the_screens_scale() {
    let mut vm = vm();
    let r = run(
        &mut vm,
        "local root = CreateFrame('Frame', nil, UIParent)
root:SetIgnoreParentScale(true)
root:SetScale(UIParent:GetScale())
root:SetAllPoints(UIParent)
local a = root:CreateFontString()
a:SetFont('Fonts\\FRIZQT__.TTF', 14, '')
a:SetText('A title that is long')
local b = UIParent:CreateFontString()
b:SetFont('Fonts\\FRIZQT__.TTF', 14, '')
b:SetText('A title that is long')
local lone = CreateFrame('Frame')
lone:SetAllPoints(UIParent)
local child = CreateFrame('Frame', nil, UIParent)
return tostring(UIParent:GetScale()), tostring(child:GetEffectiveScale()), tostring(root:GetWidth() == UIParent:GetWidth()),
    tostring(a:GetStringWidth() == b:GetStringWidth()), tostring(lone:GetEffectiveScale()),
    tostring(math.abs(lone:GetWidth() - UIParent:GetWidth() * UIParent:GetScale()) < 1e-3)",
    );
    assert_eq!(
        r,
        [
            "0.71111112833023",
            "0.71111112833023",
            "true",
            "true",
            "1",
            "true"
        ]
    );
}

/// The game's settings change the screen while it runs (decision T482):
/// "Use UI Scale" and its slider (the `useUiScale` and `uiScale` CVars)
/// scale UIParent and tell `UI_SCALE_CHANGED`; turned off, the screen's
/// own scale is back.
#[test]
fn the_ui_scale_cvars_scale_uiparent() {
    let mut vm = vm();
    let r = run(
        &mut vm,
        "Seen = {}
local f = CreateFrame('Frame')
f:RegisterEvent('UI_SCALE_CHANGED')
f:RegisterEvent('DISPLAY_SIZE_CHANGED')
f:SetScript('OnEvent', function(_, event) Seen[#Seen + 1] = event end)
C_CVar.SetCVar('useUiScale', '1')
C_CVar.SetCVar('uiScale', '0.8')
local scaled, height = tostring(UIParent:GetScale()), tostring(GetScreenHeight())
C_CVar.SetCVar('useUiScale', '0')
return scaled, height, tostring(UIParent:GetScale()), table.concat(Seen, ' ')",
    );
    assert_eq!(
        r,
        [
            "0.80000001192093",
            "960",
            "0.71111112833023",
            "UI_SCALE_CHANGED UI_SCALE_CHANGED UI_SCALE_CHANGED"
        ]
    );
}

/// The Graphics settings' window size (`C_VideoOptions.SetGameWindowSize`)
/// resizes the screen and tells `DISPLAY_SIZE_CHANGED`, and
/// `UI_SCALE_CHANGED` when UIParent's scale follows; the current size is
/// the screen's; 0 by 0 is the default (decision T482).
#[test]
fn the_window_size_resizes_the_screen() {
    let mut vm = vm();
    let r = run(
        &mut vm,
        "Seen = {}
local f = CreateFrame('Frame')
f:RegisterEvent('UI_SCALE_CHANGED')
f:RegisterEvent('DISPLAY_SIZE_CHANGED')
f:SetScript('OnEvent', function(_, event) Seen[#Seen + 1] = event end)
C_VideoOptions.SetGameWindowSize(1280, 720)
local w, h = GetPhysicalScreenSize()
local size = C_VideoOptions.GetCurrentGameWindowSize(0, false)
local scale = tostring(UIParent:GetScale())
C_VideoOptions.SetGameWindowSize(0, 0)
local w2, h2 = GetPhysicalScreenSize()
return w .. 'x' .. h, size.x .. 'x' .. size.y, scale, w2 .. 'x' .. h2, table.concat(Seen, ' ')",
    );
    assert_eq!(
        r,
        [
            "1280x720",
            "1280x720",
            "1",
            "1920x1080",
            "DISPLAY_SIZE_CHANGED UI_SCALE_CHANGED DISPLAY_SIZE_CHANGED UI_SCALE_CHANGED"
        ]
    );
}

/// WorldFrame covers the screen unscaled: 768 units high at any UI scale,
/// its width following the screen's aspect ratio, and frames anchored to it
/// get the screen's rectangle (decision T492).
#[test]
fn world_frame_covers_the_screen() {
    let mut vm = vm();
    let r = run(
        &mut vm,
        "local f = CreateFrame('Frame', nil, UIParent)
f:SetAllPoints(WorldFrame)
local w, h = WorldFrame:GetSize()
return w, h, WorldFrame:GetEffectiveScale(), tostring(math.abs(f:GetWidth() - UIParent:GetWidth()) < 1e-3),
    tostring(math.abs(f:GetHeight() - UIParent:GetHeight()) < 1e-3)",
    );
    assert_eq!(r, ["1365.3333740234375", "768", "1", "true", "true"]);
    // Its width follows the screen's aspect ratio; its height stays 768.
    for (width, height, ui_scale, size) in [
        (2560.0, 1080.0, None, ["1820.4444580078125", "768"]),
        (1280.0, 1024.0, Some(0.8), ["960", "768"]),
    ] {
        let screen = wowlua_engine::Screen {
            width,
            height,
            ui_scale,
            ..Default::default()
        };
        wowlua_engine::set_screen(&mut vm, screen);
        assert_eq!(run(&mut vm, "return WorldFrame:GetSize()"), size);
    }
    // and the window's size as the game's settings change it.
    assert_eq!(
        run(
            &mut vm,
            "C_VideoOptions.SetGameWindowSize(1024, 768) return WorldFrame:GetSize()"
        ),
        ["1024", "768"]
    );
}

/// Every atlas of the client from its own tables (decision T238): one
/// never recorded has its size, file, coordinates and raw size (its 2x
/// texture's, half the pixels); `useAtlasSize` takes the size; an atlas the
/// client lacks is none.
#[test]
fn atlases_are_the_clients() {
    let mut vm = vm();
    let r = run(
        &mut vm,
        "function CreateVector2D(x, y) return { x = x, y = y } end -- (Blizzard's, without its UI)
local i = C_Texture.GetAtlasInfo('questlog-main-background')
local t = UIParent:CreateTexture()
t:SetAtlas('QuestLog-main-background', true)
local raw = i.rawSize
return i.width, i.height, i.file, i.leftTexCoord, raw and raw.x, raw and raw.y,
    t:GetWidth(), t:GetHeight(), t:GetAtlas(), select('#', C_Texture.GetAtlasInfo('x')),
    tostring(C_Texture.GetAtlasExists('QuestLog-main-background')), tostring(C_Texture.GetAtlasExists('x'))",
    );
    assert_eq!(
        r,
        [
            "307",
            "510",
            "5684770",
            "0.30126953125",
            "614",
            "1020",
            "307",
            "510",
            "QuestLog-main-background",
            "0",
            "true",
            "false"
        ]
    );
}

/// A payload table a test fires reaches the handlers as the client's
/// does, its fields untainted (decision T239): Blizzard's secure handler
/// reading it stays secure; a frame in the payload is the same frame.
#[test]
fn fired_payload_tables_are_the_clients() {
    let mut vm = vm();
    wowlua_engine::controls::install(&mut vm, "E");
    let secure = vm
        .load(
            b"Seen = {}
local f = CreateFrame('Frame')
f:RegisterEvent('UNIT_AURA')
f:SetScript('OnEvent', function(_, _, unit, info)
    Seen[#Seen + 1] = tostring(info.isFullUpdate) .. ' ' .. tostring(issecure())
end)",
            "=Blizzard",
        )
        .expect("compiles");
    vm.call(secure, &[]).expect("runs");
    let r = run(
        &mut vm,
        "E.fireUnitEvent('UNIT_AURA', 'player', { isFullUpdate = true })
return Seen[1]",
    );
    assert_eq!(r, ["true true"]);
}

/// Talents open at level 5, as the client's answer below it says.
#[test]
fn talents_open_at_level_five() {
    let mut vm = vm();
    wowlua_engine::controls::install(&mut vm, "E");
    let r = run(
        &mut vm,
        "E.setPlayer({ level = 1 })
local a, why = C_SpecializationInfo.CanPlayerUseTalentUI()
E.setPlayer({ level = 5 })
local b, none = C_SpecializationInfo.CanPlayerUseTalentSpecUI()
return tostring(a), why, tostring(b), none",
    );
    assert_eq!(
        r,
        [
            "false",
            "This feature becomes available at level 5.",
            "true",
            ""
        ]
    );
}

/// A focused edit box hidden, or a parent of it hidden, lets go of the
/// keyboard (`OnEditFocusLost`): keys reach the game again (decision T244).
#[test]
fn a_hidden_edit_box_loses_the_focus() {
    let mut vm = vm();
    let r = run(
        &mut vm,
        "local out = {}
local p = CreateFrame('Frame', nil, UIParent)
local e = CreateFrame('EditBox', nil, p)
e:SetScript('OnEditFocusLost', function() out[#out + 1] = 'lost' end)
e:SetFocus()
out[#out + 1] = tostring(e:HasFocus())
p:Hide()
out[#out + 1] = tostring(e:HasFocus()) .. ' ' .. tostring(GetCurrentKeyBoardFocus())
p:Show()
e:SetFocus()
e:Hide()
out[#out + 1] = tostring(e:HasFocus())
-- another frame hidden: the focus stays
e:Show()
e:SetFocus()
local other = CreateFrame('Frame', nil, UIParent)
other:Hide()
out[#out + 1] = tostring(e:HasFocus())
e:ClearFocus()
return unpack(out)",
    );
    assert_eq!(
        r,
        ["true", "lost", "false nil", "lost", "false", "true", "lost"]
    );
}

/// What playing animations show (spec debugger §18.9): an alpha
/// animation's value as it runs, a flip book's frame, and a group set to
/// its final alpha keeping it when it finishes (`GetAlpha` too).
#[test]
fn animations_show_their_alpha_and_flip_book_frames() {
    let mut vm = vm();
    run(
        &mut vm,
        "local f = CreateFrame('Frame', 'AnimFrame', UIParent)
f:SetSize(100, 100)
f:SetPoint('CENTER')
local glow = f:CreateTexture(nil, 'ARTWORK')
glow:SetAllPoints()
glow:SetColorTexture(1, 1, 1)
f.Glow = glow
local g = f:CreateAnimationGroup()
g:SetToFinalAlpha(true)
local fade = g:CreateAnimation('Alpha')
fade:SetChildKey('Glow')
fade:SetFromAlpha(1)
fade:SetToAlpha(0)
fade:SetDuration(1)
local flip = g:CreateAnimation('FlipBook')
flip:SetChildKey('Glow')
flip:SetFlipBookRows(2)
flip:SetFlipBookColumns(2)
flip:SetFlipBookFrames(4)
flip:SetDuration(1)
AnimGroup = g
g:Play()",
    );
    wowlua_engine::animations::tick(&mut vm, 0.6);
    let fx = wowlua_engine::animations::effects(&vm);
    let e = fx.values().next().copied().expect("the glow's effect");
    assert!((e.alpha.unwrap() - 0.4).abs() < 1e-9, "{e:?}");
    // 0.6 of 4 frames: the third (row 1, column 0).
    assert_eq!(e.flip, Some((2, 2, 2)));
    // The scene draws the third frame's quarter.
    let scene = wowlua_engine::scene::build(&mut vm);
    let q = scene
        .items
        .iter()
        .find_map(|i| match i {
            wowlua_engine::scene::Item::Quad(q) => Some(q.clone()),
            _ => None,
        })
        .expect("the glow");
    assert_eq!(&q.texcoord[..2], &[0.0, 0.5]);
    assert!((q.color[3] - 0.4).abs() < 1e-6);
    wowlua_engine::animations::tick(&mut vm, 0.6);
    let r = run(
        &mut vm,
        "return tostring(AnimGroup:IsPlaying()), AnimFrame.Glow:GetAlpha()",
    );
    assert_eq!(r, ["false", "0"]);
}

/// A named colour (`|cnNAME:text|r`, the chat's links): the text alone is
/// shown and measured, in the global colour object's colour.
#[test]
fn named_colors_color_their_text() {
    let mut vm = vm();
    let r = run(
        &mut vm,
        "LINK_BLUE = { r = 0, g = 0.5, b = 1 }
local f = CreateFrame('Frame', nil, UIParent)
f:SetSize(300, 40)
f:SetPoint('CENTER')
local t = f:CreateFontString(nil, 'OVERLAY')
t:SetFont('Fonts\\\\FRIZQT__.TTF', 12)
t:SetAllPoints()
t:SetText('see |cnLINK_BLUE:here|r now')
local plain = f:CreateFontString(nil, 'OVERLAY')
plain:SetFont('Fonts\\\\FRIZQT__.TTF', 12)
plain:SetText('see here now')
return t:GetStringWidth() == plain:GetStringWidth() and 'same' or (t:GetStringWidth() .. ' ' .. plain:GetStringWidth())",
    );
    assert_eq!(r, ["same"]);
    let scene = wowlua_engine::scene::build(&mut vm);
    let text = scene
        .items
        .iter()
        .find_map(|i| match i {
            wowlua_engine::scene::Item::Text(t) => Some(t.clone()),
            _ => None,
        })
        .expect("the text");
    let runs: Vec<(String, [f32; 4])> = text.lines[0]
        .runs
        .iter()
        .map(|r| (r.text.clone(), r.color))
        .collect();
    assert_eq!(
        runs[1],
        ("here".to_string(), [0.0, 0.5, 1.0, 1.0]),
        "{runs:?}"
    );
    assert_eq!(
        runs.iter().map(|r| r.0.as_str()).collect::<String>(),
        "see here now"
    );
}

/// `CopyFontObject` copies a font's attributes, inherited ones too, as
/// Blizzard's `FontableFrameMixin` uses it (the chat frames' font).
#[test]
fn copy_font_object_copies_inherited_attributes() {
    let mut vm = vm();
    let r = run(
        &mut vm,
        "local base = CreateFont('BaseFont')
base:SetFont('Fonts\\\\ARIALN.TTF', 14, '')
base:SetTextColor(1, 0.5, 0)
local derived = CreateFont('DerivedFont')
derived:SetFontObject(base)
local copy = CreateFont('CopyFont')
copy:CopyFontObject(derived)
local file, size = copy:GetFont()
local r, g = copy:GetTextColor()
local dr, dg = derived:GetTextColor()
copy:SetFont(file, 18, '')
local file2, size2 = copy:GetFont()
return file, size, r == dr and g == dg and 'same color' or 'differs', file2, size2",
    );
    // Heights as the client gives them (single floats).
    assert_eq!(r[0], "Fonts\\ARIALN.TTF");
    assert!(r[1].starts_with("13.99"), "{r:?}");
    assert_eq!(&r[2..4], ["same color", "Fonts\\ARIALN.TTF"]);
}

/// A message frame's messages are drawn inside it, newest at the bottom
/// (spec debugger §18.9); a portrait the client would render is drawn as
/// the unit's race portrait.
#[test]
fn message_frames_and_portraits_in_the_scene() {
    let mut vm = vm();
    run(
        &mut vm,
        "local m = CreateFrame('MessageFrame', nil, UIParent)
m:SetSize(300, 60)
m:SetPoint('CENTER')
m:SetFont('Fonts\\\\ARIALN.TTF', 14, '')
m:AddMessage('first', 1, 1, 1)
m:AddMessage('second', 1, 1, 1)
local p = UIParent:CreateTexture(nil, 'ARTWORK')
p:SetSize(64, 64)
p:SetPoint('TOPLEFT')
SetPortraitTexture(p, 'player')",
    );
    let scene = wowlua_engine::scene::build(&mut vm);
    let texts: Vec<(String, f32)> = scene
        .items
        .iter()
        .filter_map(|i| match i {
            wowlua_engine::scene::Item::Text(t) => Some((
                t.lines[0].runs.iter().map(|r| r.text.clone()).collect(),
                t.rect.y,
            )),
            _ => None,
        })
        .collect();
    assert_eq!(texts.len(), 2, "{texts:?}");
    assert_eq!(texts[0].0, "first");
    assert_eq!(texts[1].0, "second");
    assert!(texts[0].1 < texts[1].1, "newest lowest: {texts:?}");
    let portrait = scene.items.iter().any(|i| {
        matches!(i, wowlua_engine::scene::Item::Quad(q)
            if matches!(&q.image, wowlua_engine::scene::Image::Portrait { fallback, .. }
                if fallback == "Interface\\CharacterFrame\\TemporaryPortrait-Male-Human"))
    });
    assert!(portrait, "the race portrait");
}

fn models(vm: &mut Vm) -> Vec<wowlua_engine::scene::Model> {
    wowlua_engine::scene::build(vm)
        .items
        .into_iter()
        .filter_map(|i| match i {
            wowlua_engine::scene::Item::Model(m) => Some(m),
            _ => None,
        })
        .collect()
}

/// A model frame shows the last model set, as placed and turned; the
/// setters read back as set; a cleared model shows nothing.
#[test]
fn model_frames_show_their_models() {
    use wowlua_engine::scene::{ModelCamera, ModelSource};
    let mut vm = vm();
    let r = run(
        &mut vm,
        "local m = CreateFrame('PlayerModel', 'TestModel', UIParent)
m:SetSize(200, 300)
m:SetPoint('CENTER')
m:SetModel(123)
m:SetDisplayInfo(15)
m:SetFacing(0.5)
m:SetPortraitZoom(1)
m:SetAnimation(4)
return m:GetDisplayInfo()",
    );
    assert_eq!(r, ["15"]);
    let m = models(&mut vm);
    assert_eq!(m.len(), 1);
    let a = &m[0].actors[0];
    assert_eq!(a.source, ModelSource::Display(15));
    assert_eq!(a.yaw, 0.5);
    assert_eq!(a.animation, 4);
    assert!(matches!(
        m[0].camera,
        ModelCamera::Frame { portrait_zoom, .. } if portrait_zoom == 1.0
    ));
    assert_eq!((m[0].rect.w, m[0].rect.h), (200.0, 300.0));
    // A unit: how it looks.
    run(&mut vm, "TestModel:SetUnit('player')");
    let m = models(&mut vm);
    let ModelSource::Character(look) = &m[0].actors[0].source else {
        panic!("{:?}", m[0].actors[0].source);
    };
    assert!(look.race > 0, "{look:?}");
    // A file ID given as a string is the file.
    run(&mut vm, "TestModel:SetModel('124442')");
    let m = models(&mut vm);
    assert_eq!(m[0].actors[0].source, ModelSource::File(124442));
    run(&mut vm, "TestModel:ClearModel()");
    assert!(models(&mut vm).is_empty());
}

/// A model scene draws its shown actors from its camera.
#[test]
fn model_scenes_show_their_actors() {
    use wowlua_engine::scene::{ModelCamera, ModelSource};
    let mut vm = vm();
    run(
        &mut vm,
        "local s = CreateFrame('ModelScene', nil, UIParent)
s:SetSize(300, 300)
s:SetPoint('CENTER')
s:SetCameraPosition(4, 0, 1)
s:SetCameraOrientationByAxisVectors(-1, 0, 0, 0, 1, 0, 0, 0, 1)
s:SetCameraFieldOfView(0.8)
local a = s:CreateActor()
a:SetModelByCreatureDisplayID(1693)
a:SetYaw(3.14)
a:SetPosition(0, 0, 0.5)
local hidden = s:CreateActor()
hidden:SetModelByFileID(5)
hidden:Hide()",
    );
    let m = models(&mut vm);
    assert_eq!(m.len(), 1);
    assert_eq!(m[0].actors.len(), 1);
    assert_eq!(m[0].actors[0].source, ModelSource::Display(1693));
    assert_eq!(m[0].actors[0].position, [0.0, 0.0, 0.5]);
    let ModelCamera::Scene { position, fov, .. } = m[0].camera else {
        panic!("{:?}", m[0].camera);
    };
    assert_eq!((position, fov), ([4.0, 0.0, 1.0], 0.8));
}

/// A model frame's placed camera (`Model_ApplyUICamera`'s) draws in camera
/// space: its model-space position and target back by 0.8171 times the
/// frame's effective scale, the model at its own position and size, a
/// field of view of 50 degrees across the frame's diagonal; the model's
/// pitch and roll turn about the view's axes after its facing.
#[test]
fn placed_model_frame_cameras_draw_in_camera_space() {
    use wowlua_engine::scene::ModelCamera;
    let mut vm = vm();
    run(
        &mut vm,
        "local m = CreateFrame('PlayerModel', nil, UIParent)
m:SetSize(200, 250)
m:SetPoint('CENTER')
m:SetDisplayInfo(15)
m:SetPosition(3, 0.2, -1.5)
m:SetFacing(-0.25)
m:SetPitch(-0.1)
local c = m:TransformCameraSpaceToModelSpace({ x = 4, y = 0, z = 0 })
m:SetCameraPosition(c.x, c.y, c.z)
m:SetCameraTarget(0, 0, 0)",
    );
    let m = models(&mut vm);
    let a = &m[0].actors[0];
    assert_eq!(a.position, [3.0, 0.2, -1.5]);
    assert!(a.fixed_axes);
    let ModelCamera::Scene { position, fov, .. } = m[0].camera else {
        panic!("{:?}", m[0].camera);
    };
    assert!((position[0] - 4.0).abs() < 1e-4, "{position:?}");
    // 50 degrees across the diagonal of 200 by 250, vertically.
    let vertical = 2.0 * ((25f32.to_radians()).tan() / (1.0f32 + 0.64).sqrt()).atan();
    assert!((fov - vertical).abs() < 1e-5, "{fov}");
}

struct Scenes;

impl wowlua_engine::clientdata::ClientData for Scenes {
    fn model_scene(&self, id: u32) -> Option<wowlua_engine::clientdata::ModelScene> {
        (id == 290).then(|| wowlua_engine::clientdata::ModelScene {
            kind: 1,
            cameras: vec![7],
            actors: vec![8, 9],
            flags: 0,
        })
    }
    fn scene_camera(&self, id: u32) -> Option<wowlua_engine::clientdata::SceneCamera> {
        (id == 7).then(|| wowlua_engine::clientdata::SceneCamera {
            id,
            tag: "primary".into(),
            target: [0.0, 0.0, 1.0],
            zoom_distance: 5.0,
            ..Default::default()
        })
    }
    fn scene_actor(&self, id: u32) -> Option<wowlua_engine::clientdata::SceneActor> {
        (id == 8).then(|| wowlua_engine::clientdata::SceneActor {
            id,
            tag: "player".into(),
            position: [0.0, 0.5, 0.0],
            display: 3,
            ..Default::default()
        })
    }
    fn item_inventory_type(&self, id: u32) -> Option<u32> {
        (id == 16923).then_some(20)
    }
    fn actor_display(&self, id: u32) -> Option<wowlua_engine::clientdata::ActorDisplay> {
        (id == 3).then(|| wowlua_engine::clientdata::ActorDisplay {
            animation: 4,
            alpha: 1.0,
            scale: 1.0,
            anim_speed: 1.0,
            ..Default::default()
        })
    }
}

/// With the client's tables at hand, `C_ModelInfo` answers from them;
/// without, nothing.
#[test]
fn model_scene_data_comes_from_the_clients_tables() {
    let mut vm = vm();
    assert_eq!(
        run(&mut vm, "return C_ModelInfo.GetModelSceneInfoByID(290)"),
        Vec::<String>::new()
    );
    wowlua_engine::clientdata::set(&mut vm, std::sync::Arc::new(Scenes));
    let r = run(
        &mut vm,
        "local kind, cameras, actors = C_ModelInfo.GetModelSceneInfoByID(290)
local camera = C_ModelInfo.GetModelSceneCameraInfoByID(cameras[1])
local actor = C_ModelInfo.GetModelSceneActorInfoByID(actors[1])
local display = C_ModelInfo.GetModelSceneActorDisplayInfoByID(actor.modelActorDisplayID)
return kind, #actors, camera.cameraType, camera.zoomDistance, camera.target.z, actor.scriptTag, actor.position.y, display.animation, C_ModelInfo.GetModelSceneActorInfoByID(9) == nil",
    );
    assert_eq!(
        r,
        [
            "1",
            "2",
            "OrbitCamera",
            "5",
            "1",
            "player",
            "0.5",
            "4",
            "Bool(true)"
        ]
    );
    // An item the test has no data for: its inventory type the client's.
    let r = run(
        &mut vm,
        "return C_Item.IsDressableItemByID(16923), C_Item.IsDressableItemByID(16924)",
    );
    assert_eq!(r, ["Bool(true)", "Bool(false)"]);
}

/// A portrait is the unit's model as it looked when set; a model without
/// auto dress, or with a slot undressed, wears less.
#[test]
fn portraits_and_undressed_models() {
    use wowlua_engine::scene::{Image, Item, ModelSource};
    let mut vm = vm();
    run(
        &mut vm,
        "-- A shirt and a chest piece worn.
GetInventoryItemID = function(unit, slot)
    return ({ [4] = 38, [5] = 39 })[slot]
end
local f = CreateFrame('Frame', nil, UIParent)
f:SetSize(64, 64)
f:SetPoint('CENTER')
local t = f:CreateTexture()
t:SetAllPoints()
SetPortraitTexture(t, 'player')
local m = CreateFrame('DressUpModel', 'Naked', UIParent)
m:SetSize(100, 100)
m:SetPoint('TOPLEFT')
m:SetAutoDress(false)
m:SetUnit('player')
local d = CreateFrame('DressUpModel', 'Dressed', UIParent)
d:SetSize(100, 100)
d:SetPoint('TOPRIGHT')
d:SetUnit('player')
d:UndressSlot(4)",
    );
    let scene = wowlua_engine::scene::build(&mut vm);
    let portrait = scene
        .items
        .iter()
        .find_map(|i| match i {
            Item::Quad(q) => match &q.image {
                Image::Portrait { source, fallback } => Some((source.clone(), fallback.clone())),
                _ => None,
            },
            _ => None,
        })
        .expect("the portrait");
    let ModelSource::Character(look) = *portrait.0 else {
        panic!("{:?}", portrait.0);
    };
    assert!(look.race > 0 && !look.items.is_empty(), "{look:?}");
    assert!(portrait.1.contains("TemporaryPortrait"));
    let looks: Vec<Vec<(u32, u32)>> = models(&mut vm)
        .iter()
        .map(|m| match &m.actors[0].source {
            ModelSource::Character(l) => l.items.clone(),
            s => panic!("{s:?}"),
        })
        .collect();
    assert!(looks[0].is_empty(), "{looks:?}");
    assert!(
        !looks[1].is_empty() && !looks[1].iter().any(|(s, _)| *s == 4),
        "{looks:?}"
    );
    assert!(look.items.iter().any(|(s, _)| *s == 4), "{look:?}");
}

struct Cameras;

impl wowlua_engine::clientdata::ClientData for Cameras {
    fn model_scene(&self, _: u32) -> Option<wowlua_engine::clientdata::ModelScene> {
        None
    }
    fn scene_camera(&self, _: u32) -> Option<wowlua_engine::clientdata::SceneCamera> {
        None
    }
    fn scene_actor(&self, _: u32) -> Option<wowlua_engine::clientdata::SceneActor> {
        None
    }
    fn actor_display(&self, _: u32) -> Option<wowlua_engine::clientdata::ActorDisplay> {
        None
    }
    fn ui_camera(&self, id: u32) -> Option<wowlua_engine::clientdata::UiCamera> {
        (id == 238).then(|| wowlua_engine::clientdata::UiCamera {
            position: [0.0, -0.5, -0.25],
            angles: [1.5, -1.0, 0.0],
            animation: 0,
            frame: -1,
            variation: -1,
            flags: 1,
        })
    }
}

/// `GetUICameraInfo` answers the client's row as it is: the model's
/// position and angles, its animation and whether it is centered; without
/// the tables, the recordings.
#[test]
fn ui_cameras_are_the_clients_rows() {
    let mut vm = wowlua_vm::Vm::wow();
    wowlua_engine::install(&mut vm, "camelot").expect("the engine");
    let recorded = run(&mut vm, "return GetUICameraInfo(249)");
    assert_eq!(recorded[0], "0.75");
    wowlua_engine::clientdata::set(&mut vm, std::sync::Arc::new(Cameras));
    let r = run(&mut vm, "return GetUICameraInfo(238)");
    assert_eq!(
        r,
        [
            "0",
            "-0.5",
            "-0.25",
            "1.5",
            "-1",
            "0",
            "0",
            "-1",
            "-1",
            "Bool(true)"
        ]
    );
}

/// Every terrain tile's minimap texture is `100000 + row * 64 + column`.
struct Tiles;

impl wowlua_engine::clientdata::ClientData for Tiles {
    fn model_scene(&self, _: u32) -> Option<wowlua_engine::clientdata::ModelScene> {
        None
    }
    fn scene_camera(&self, _: u32) -> Option<wowlua_engine::clientdata::SceneCamera> {
        None
    }
    fn scene_actor(&self, _: u32) -> Option<wowlua_engine::clientdata::SceneActor> {
        None
    }
    fn actor_display(&self, _: u32) -> Option<wowlua_engine::clientdata::ActorDisplay> {
        None
    }
    fn minimap_tile(&self, map: u32, x: u32, y: u32) -> Option<u32> {
        (map == 0).then_some(100000 + y * 64 + x)
    }
}

/// A minimap's terrain tiles, each cut to the minimap: (tile, rectangle,
/// texture coordinates' left, right, top, bottom).
fn minimap_tiles(vm: &mut Vm) -> Vec<(u32, [f32; 4], [f32; 4])> {
    let scene = wowlua_engine::scene::build(vm);
    scene
        .items
        .iter()
        .filter_map(|i| match i {
            wowlua_engine::scene::Item::Quad(q) => match q.image {
                wowlua_engine::scene::Image::FileId(f) if f >= 100000 => Some((
                    f - 100000,
                    [q.rect.x, q.rect.y, q.rect.w, q.rect.h],
                    [q.texcoord[0], q.texcoord[4], q.texcoord[1], q.texcoord[3]],
                )),
                _ => None,
            },
            _ => None,
        })
        .collect()
}

/// A minimap shows the terrain tiles about the player at its zoom, north
/// up, cut to it and masked; it follows the player, and shows none
/// indoors.
#[test]
fn minimaps_show_the_terrain_about_the_player() {
    let mut vm = vm();
    wowlua_engine::controls::install(&mut vm, "mock");
    wowlua_engine::clientdata::set(&mut vm, std::sync::Arc::new(Tiles));
    run(
        &mut vm,
        "mock.moveTo({ mapID = 1429, zone = 'Elwynn Forest', x = 0.4805, y = 0.4365 })
M = CreateFrame('Minimap', nil, UIParent)
M:SetSize(200, 200)
M:SetPoint('TOPLEFT', 0, 0)
M:SetMaskTexture('Interface\\\\Masks\\\\Round')",
    );
    let (north, west) = {
        let r = run(&mut vm, "local y, x = UnitPosition('player') return y, x");
        (r[0].parse::<f64>().unwrap(), r[1].parse::<f64>().unwrap())
    };
    let tile = 1600.0 / 3.0;
    let (col, row) = (32.0 - west / tile, 32.0 - north / tile);
    // Zoomed in fully (133 yards across), one tile, about the player.
    run(&mut vm, "M:SetZoom(5)");
    let tiles = minimap_tiles(&mut vm);
    assert_eq!(tiles.len(), 1, "{tiles:?}");
    let (t, rect, tc) = tiles[0];
    assert_eq!(t, row.floor() as u32 * 64 + col.floor() as u32);
    assert_eq!(rect, [0.0, 0.0, 200.0, 200.0]);
    let half = (133.0 + 1.0 / 3.0) / 2.0 / tile;
    let (u, v) = (col.fract(), row.fract());
    for (got, want) in tc.iter().zip([u - half, u + half, v - half, v + half]) {
        assert!((*got as f64 - want).abs() < 1e-4, "{tc:?}");
    }
    let scene = wowlua_engine::scene::build(&mut vm);
    let masked = scene.items.iter().any(|i| {
        matches!(i, wowlua_engine::scene::Item::Quad(q)
            if matches!(q.image, wowlua_engine::scene::Image::FileId(f) if f >= 100000)
                && q.masks.len() == 1
                && q.masks[0].rect.w == 200.0)
    });
    assert!(masked, "the tiles take the minimap's mask");
    // Zoomed out, the tiles cover the minimap without overlapping.
    run(&mut vm, "M:SetZoom(0)");
    let tiles = minimap_tiles(&mut vm);
    assert!(tiles.len() > 1);
    let area: f32 = tiles.iter().map(|(_, r, _)| r[2] * r[3]).sum();
    assert!((area - 40000.0).abs() < 1.0, "{area}");
    // Moved a tile east: the next column.
    let east = west - tile;
    run(
        &mut vm,
        &format!(
            "mock.moveTo({{ world = {{ y = {north}, x = {east}, instanceID = 0 }} }}) M:SetZoom(5)"
        ),
    );
    let tiles = minimap_tiles(&mut vm);
    assert_eq!(tiles[0].0, row.floor() as u32 * 64 + col.floor() as u32 + 1);
    run(&mut vm, "mock.moveTo({ indoors = true })");
    assert!(minimap_tiles(&mut vm).is_empty());
}

/// One item of the client's tables.
struct Items;

impl wowlua_engine::clientdata::ClientData for Items {
    fn model_scene(&self, _: u32) -> Option<wowlua_engine::clientdata::ModelScene> {
        None
    }
    fn scene_camera(&self, _: u32) -> Option<wowlua_engine::clientdata::SceneCamera> {
        None
    }
    fn scene_actor(&self, _: u32) -> Option<wowlua_engine::clientdata::SceneActor> {
        None
    }
    fn actor_display(&self, _: u32) -> Option<wowlua_engine::clientdata::ActorDisplay> {
        None
    }
    fn item(&self, id: u32) -> Option<wowlua_engine::clientdata::ClientItem> {
        (id == 277714 || id == 19019).then(|| wowlua_engine::clientdata::ClientItem {
            name: "Spectral Bear Cub".into(),
            quality: 3,
            icon: 294471,
            class: 15,
            subclass: 2,
            inventory_type: 0,
            item_level: 1,
            required_level: 0,
            expansion: 0,
            stack: 1,
            sell_price: 250,
            bind: 1,
            description: "Teaches you how to summon this companion.".into(),
        })
    }
}

/// An item the test has no data for is the client's (name, quality,
/// icon, class, a link, description), cached; one the test defines stays the test's.
#[test]
fn unknown_items_come_from_the_clients_tables() {
    let mut vm = vm();
    wowlua_engine::controls::install(&mut vm, "mock");
    run(
        &mut vm,
        "mock.defineItem(19019, { name = 'Mine', quality = 5 })",
    );
    wowlua_engine::clientdata::set(&mut vm, std::sync::Arc::new(Items));
    let r = run(
        &mut vm,
        "local name, link, quality, _, _, kind, _, _, loc, icon, _, class, sub = C_Item.GetItemInfo(277714)
local stack, price, bind = select(8, C_Item.GetItemInfo(277714)), select(11, C_Item.GetItemInfo(277714)), select(14, C_Item.GetItemInfo(277714))
assert(stack == 1 and price == 250 and bind == 1, tostring(stack) .. ' ' .. tostring(price) .. ' ' .. tostring(bind))
assert(select(18, C_Item.GetItemInfo(277714)) == 'Teaches you how to summon this companion.')
return name, quality, icon, class, sub, loc, tostring(link:find('item:277714', 1, true) ~= nil),
    tostring(C_Item.DoesItemExistByID(277714)), tostring(C_Item.IsItemDataCachedByID(277714)),
    (C_Item.GetItemInfo(19019)), tostring(C_Item.DoesItemExistByID(1234567))",
    );
    assert_eq!(
        r,
        [
            "Spectral Bear Cub",
            "3",
            "294471",
            "15",
            "2",
            "INVTYPE_NON_EQUIP_IGNORE",
            "true",
            "true",
            "true",
            "Mine",
            "false"
        ]
    );
}

/// The player faces where `moveTo` turns them (`GetPlayerFacing`); with
/// `rotateMinimap` the minimap's tiles turn clockwise by it, whole, cut by
/// the minimap's rectangle, and the arrow points up; without it the tiles
/// stay north up and the arrow turns.
#[test]
fn the_minimap_turns_with_the_player() {
    let mut vm = vm();
    wowlua_engine::controls::install(&mut vm, "mock");
    wowlua_engine::clientdata::set(&mut vm, std::sync::Arc::new(Tiles));
    run(
        &mut vm,
        "mock.moveTo({ mapID = 1429, zone = 'Elwynn Forest', x = 0.4805, y = 0.4365, facing = math.pi / 2 })
M = CreateFrame('Minimap', nil, UIParent)
M:SetSize(200, 200)
M:SetPoint('TOPLEFT', 0, 0)",
    );
    assert_eq!(
        run(&mut vm, "return string.format('%.4f', GetPlayerFacing())"),
        ["1.5708"]
    );
    let quads = |vm: &mut Vm| -> Vec<(bool, f32, f32, Option<f32>)> {
        let scene = wowlua_engine::scene::build(vm);
        scene
            .items
            .iter()
            .filter_map(|i| match i {
                wowlua_engine::scene::Item::Quad(q) => match &q.image {
                    wowlua_engine::scene::Image::FileId(f) if *f >= 100000 => {
                        Some((true, q.rotation, q.rect.w, q.clip.map(|c| c.w)))
                    }
                    wowlua_engine::scene::Image::File(p) if p.contains("MinimapArrow") => {
                        Some((false, q.rotation, q.rect.w, None))
                    }
                    _ => None,
                },
                _ => None,
            })
            .collect()
    };
    let fixed = quads(&mut vm);
    assert!(fixed.iter().filter(|q| q.0).all(|q| q.1 == 0.0));
    let arrow = fixed.iter().find(|q| !q.0).expect("the arrow");
    assert!((arrow.1 - std::f32::consts::FRAC_PI_2).abs() < 1e-5);
    run(&mut vm, "C_CVar.SetCVar('rotateMinimap', '1')");
    let turned = quads(&mut vm);
    let tiles: Vec<_> = turned.iter().filter(|q| q.0).collect();
    assert!(!tiles.is_empty());
    // A whole tile (533 yards at 466 across 200 pixels), turned, cut to
    // the minimap.
    for t in &tiles {
        assert!((t.1 + std::f32::consts::FRAC_PI_2).abs() < 1e-5, "{t:?}");
        assert!((t.2 - 228.57).abs() < 0.1, "{t:?}");
        assert_eq!(t.3, Some(200.0));
    }
    assert_eq!(turned.iter().find(|q| !q.0).expect("the arrow").1, 0.0);
}

/// Group members with a place in the world are their class's blips on the
/// minimap, where they are; a ping (`PingLocation`) is answered
/// (`GetPingPosition`), drawn for its first seconds, and told to event
/// callbacks (`MINIMAP_PING` is one alone on camelot); Blizzard's alone:
/// an addon's callback for it is forbidden (`ADDON_ACTION_FORBIDDEN`,
/// false, never called).
#[test]
fn minimaps_show_the_group_and_pings() {
    let mut vm = vm();
    wowlua_engine::controls::install(&mut vm, "mock");
    wowlua_engine::clientdata::set(&mut vm, std::sync::Arc::new(Tiles));
    let secure = vm
        .load(
            b"CreateFrame('Frame'):RegisterEventCallback('MINIMAP_PING', function(_, unit, a, b) Heard = { unit, a, b } end)",
            "=secure",
        )
        .expect("compiles");
    vm.call(secure, &[]).expect("registers");
    let r = run(
        &mut vm,
        "mock.moveTo({ mapID = 1429, zone = 'Elwynn Forest', x = 0.4805, y = 0.4365 })
M = CreateFrame('Minimap', nil, UIParent)
M:SetSize(200, 200)
M:SetPoint('TOPLEFT', 0, 0)
local y, x = UnitPosition('player')
mock.spawnUnit({ name = 'Anna', class = 'PRIEST', classID = 5, reaction = 'friendly', player = true, world = { y = y + 60, x = x } }):joinGroup()
local refused = tostring((pcall(CreateFrame('Frame').RegisterEvent, CreateFrame('Frame'), 'MINIMAP_PING')))
local watch = CreateFrame('Frame')
watch:RegisterEvent('ADDON_ACTION_FORBIDDEN')
watch:SetScript('OnEvent', function(_, _, addon, action) Forbidden = addon .. ' ' .. action end)
Mine = 'not called'
local registered = CreateFrame('Frame'):RegisterEventCallback('MINIMAP_PING', function() Mine = 'called' end)
M:PingLocation(-30, 20)
local py = UnitPosition('party1')
local gx, gy = M:GetPingPosition()
return py - y, string.format('%.4f', gx), string.format('%.4f', gy), refused .. ' ' .. table.concat(Heard, ' '),
    tostring(registered), Mine, Forbidden",
    );
    // A callback event on camelot: `RegisterEvent` refuses it, Blizzard's
    // callback gets it, an addon's is forbidden.
    // In widths of the view (the minimap's 200 UI units across).
    assert_eq!(
        r,
        [
            "60",
            "-0.1500",
            "0.1000",
            "false player -30 20",
            "false",
            "not called",
            "MyAddon Frame:RegisterEventCallback()"
        ]
    );
    let marks = |vm: &mut Vm| -> Vec<(String, [f32; 4], [f32; 2])> {
        let scene = wowlua_engine::scene::build(vm);
        scene
            .items
            .iter()
            .filter_map(|i| match i {
                wowlua_engine::scene::Item::Quad(q) => match &q.image {
                    wowlua_engine::scene::Image::File(p)
                        if p.contains("PartyRaidBlips") || p.contains("Ping") =>
                    {
                        Some((
                            p.rsplit('\\').next().unwrap().to_string(),
                            [q.rect.x, q.rect.y, q.rect.w, q.rect.h],
                            [q.texcoord[0], q.texcoord[1]],
                        ))
                    }
                    _ => None,
                },
                _ => None,
            })
            .collect()
    };
    let now = marks(&mut vm);
    // The priest 60 yards north: 60 / (466.67 / 200) pixels above the
    // center (100, 100), the priest's cell (the fifth) of the sheet.
    let blip = now
        .iter()
        .find(|m| m.0 == "PartyRaidBlips")
        .expect("the blip");
    assert!(
        (blip.1[0] + blip.1[2] / 2.0 - 100.0).abs() < 0.01,
        "{blip:?}"
    );
    assert!((blip.1[1] + blip.1[3] / 2.0 - (100.0 - 60.0 * 200.0 / 466.6667)).abs() < 0.01);
    assert_eq!(blip.2, [0.5, 0.0]);
    let center = now
        .iter()
        .find(|m| m.0 == "UI-Minimap-Ping-Center")
        .expect("the ping");
    assert_eq!(
        (
            center.1[0] + center.1[2] / 2.0,
            center.1[1] + center.1[3] / 2.0
        ),
        (70.0, 80.0)
    );
    // The ping stays where it was in the world: the player walks 60 yards
    // north, and it is 60 yards further south of them.
    run(
        &mut vm,
        "local y, x = UnitPosition('player') mock.moveTo({ world = { y = y + 60, x = x, instanceID = 0 } })",
    );
    let moved = run(&mut vm, "local gx, gy = M:GetPingPosition() return gx, gy");
    let gy: f64 = moved[1].parse().unwrap();
    assert!((gy - (0.1 - 60.0 / 466.666_666)).abs() < 1e-6, "{moved:?}");
    run(&mut vm, "mock.advanceTime(6)");
    assert!(marks(&mut vm).iter().all(|m| !m.0.contains("Ping")));
    // The view's radius follows the zoom (probed: 233 1/3, 100, 66 2/3).
    run(&mut vm, "Minimap = M");
    let radius = |vm: &mut Vm, z: u32| {
        run(
            vm,
            &format!("M:SetZoom({z}) return string.format('%.3f', C_Minimap.GetViewRadius())"),
        )
    };
    assert_eq!(radius(&mut vm, 0), ["233.333"]);
    assert_eq!(radius(&mut vm, 4), ["100.000"]);
    assert_eq!(radius(&mut vm, 5), ["66.667"]);
}

/// The callback events the client keeps from `RegisterEvent` (probed in WoW
/// Forever): refused as unknown, not valid; the ones that are events too
/// register.
#[test]
fn callback_events_alone_are_refused() {
    let mut vm = vm();
    let r = run(
        &mut vm,
        "local out = {}
for _, e in ipairs({ 'MINIMAP_PING', 'COMBAT_LOG_EVENT', 'COMBAT_LOG_EVENT_UNFILTERED',
    'COMBAT_LOG_EVENT_INTERNAL_UNFILTERED', 'ENCOUNTER_STATE_CHANGED', 'TOOLTIP_SHOW_ITEM_COMPARISON',
    'COMBAT_LOG_APPLY_FILTER_SETTINGS', 'COMBAT_LOG_REFILTER_ENTRIES', 'CLASS_TALENTS_SWITCH_TO_LOADOUT_BY_NAME' }) do
    local f = CreateFrame('Frame')
    out[#out + 1] = tostring((pcall(f.RegisterEvent, f, e))) .. '/' .. tostring(C_EventUtils.IsEventValid(e))
end
return table.concat(out, ' ')",
    );
    assert_eq!(
        r,
        [
            "false/false true/true true/true false/false true/true false/false false/false false/false false/false"
        ]
    );
}

/// A fresh texture snaps to the pixel grid with a texel snapping bias of
/// 0.3, the client's float (probed in WoW Forever).
#[test]
fn a_fresh_texture_snaps_with_a_bias_of_three_tenths() {
    let mut vm = vm();
    let r = run(
        &mut vm,
        "local t = UIParent:CreateTexture()
return tostring(t:IsSnappingToPixelGrid()), string.format('%.14f', t:GetTexelSnappingBias())",
    );
    assert_eq!(r, ["true", "0.30000001192093"]);
}

/// The client's wardrobe cameras: an armor slot's by race and gender.
struct WardrobeCameras;

impl wowlua_engine::clientdata::ClientData for WardrobeCameras {
    fn model_scene(&self, _: u32) -> Option<wowlua_engine::clientdata::ModelScene> {
        None
    }
    fn scene_camera(&self, _: u32) -> Option<wowlua_engine::clientdata::SceneCamera> {
        None
    }
    fn scene_actor(&self, _: u32) -> Option<wowlua_engine::clientdata::SceneActor> {
        None
    }
    fn actor_display(&self, _: u32) -> Option<wowlua_engine::clientdata::ActorDisplay> {
        None
    }
    fn item(&self, id: u32) -> Option<wowlua_engine::clientdata::ClientItem> {
        (id == 44803).then(|| wowlua_engine::clientdata::ClientItem {
            name: "A helm".into(),
            class: 4,
            inventory_type: 1,
            ..Default::default()
        })
    }
    fn transmog_camera(&self, race: u32, gender: u32, slot: u32, variation: u32) -> Option<u32> {
        match (race, gender, slot, variation) {
            (5, 0, 1, 0) => Some(542),
            (96, 1, 1, 0) => Some(1957),
            _ => None,
        }
    }
}

/// An appearance's wardrobe camera is the client's for its item's slot on
/// the player's race and gender, over the recorded one (the recording's
/// Undead male's); without the tables, the recorded one.
#[test]
fn wardrobe_cameras_follow_the_players_race_and_gender() {
    let mut vm = vm();
    let recorded = run(
        &mut vm,
        "return C_TransmogCollection.GetAppearanceCameraID(11257)",
    );
    assert_eq!(recorded, ["542"]);
    wowlua_engine::controls::install(&mut vm, "mock");
    wowlua_engine::clientdata::set(&mut vm, std::sync::Arc::new(WardrobeCameras));
    run(
        &mut vm,
        "mock.setPlayer({ name = 'P', realm = 'R', class = 'SHAMAN', race = 'Skyborne', sex = 3, level = 21 })",
    );
    let r = run(
        &mut vm,
        "local s = C_TransmogCollection.GetAppearanceSources(11257, 1)[1]
return C_TransmogCollection.GetAppearanceCameraID(11257), C_TransmogCollection.GetAppearanceCameraIDBySource(s.sourceID)",
    );
    assert_eq!(r, ["1957", "1957"]);
}

/// Two points setting one edge: the one earlier in `Enum.FramePoint`'s
/// order places it, whatever order they were set in (the dropdowns' text).
#[test]
fn points_resolve_in_frame_point_order() {
    let mut vm = vm();
    let r = run(
        &mut vm,
        "local f = CreateFrame('Frame', nil, UIParent)
f:SetSize(120, 25)
f:SetPoint('TOPLEFT', 100, -100)
local arrow = f:CreateTexture()
arrow:SetSize(20, 20)
arrow:SetPoint('RIGHT', 0, -3)
local text = f:CreateFontString(nil, 'OVERLAY', 'GameFontHighlight')
text:SetHeight(10)
text:SetPoint('TOPRIGHT', arrow, 'LEFT')
text:SetPoint('TOPLEFT', 8, -8)
return f:GetTop() - text:GetTop(), text:GetLeft() - f:GetLeft(), text:GetRight() - f:GetLeft()",
    );
    assert_eq!(r, ["8", "8", "100"]);
}

/// `SetFormattedText` on a button gives its font string the text, as
/// `SetText` does (the macro window's second tab).
#[test]
fn a_buttons_formatted_text_reaches_its_font_string() {
    let mut vm = vm();
    let r = run(
        &mut vm,
        "local b = CreateFrame('Button', nil, UIParent)
b:SetFormattedText('%s Specific Macros', 'Anna')
return b:GetText(), b:GetFontString():GetText()",
    );
    assert_eq!(r, ["Anna Specific Macros", "Anna Specific Macros"]);
}

/// A button's `GetTextWidth` is its font string's text width (the money
/// frames size their coin buttons by it).
#[test]
fn a_buttons_text_width_is_its_font_strings() {
    let mut vm = vm();
    let r = run(
        &mut vm,
        "local b = CreateFrame('Button', nil, UIParent)
local fs = b:CreateFontString()
fs:SetFont('Fonts\\\\ARIALN.TTF', 14, 'OUTLINE')
b:SetFontString(fs)
b:SetText('25')
return tostring(b:GetTextWidth() > 0), tostring(b:GetTextWidth() == fs:GetStringWidth())",
    );
    assert_eq!(r, ["true", "true"]);
}

/// A tooltip lays its lines out and sizes itself to them, as the client
/// does: from its top left, one under another, a right text at its right
/// edge (the lines have no anchors of their own).
#[test]
fn tooltips_lay_out_their_lines() {
    let mut vm = vm();
    let r = run(
        &mut vm,
        "local t = CreateFrame('GameTooltip', 'TestTip', UIParent)
t:SetOwner(UIParent, 'ANCHOR_NONE')
t:SetPoint('CENTER')
t:AddLine('First line')
t:AddDoubleLine('Left', 'Right')
local l1, l2, r2 = TestTipTextLeft1, TestTipTextLeft2, TestTipTextRight2
return tostring(t:GetWidth() > l1:GetStringWidth()), string.format('%g', t:GetLeft() + 10 - l1:GetLeft()),
    string.format('%g', t:GetTop() - 10 - l1:GetTop()), tostring(l2:GetTop() < l1:GetBottom()),
    string.format('%g', t:GetRight() - 10 - r2:GetRight())",
    );
    assert_eq!(r, ["true", "0", "0", "true", "0"]);
}

/// `SetFrameLevel` moves the frame's descendants that have a level of
/// their own by as much (Blizzard's menus: entries 20 over the menu, then
/// the menu raised).
#[test]
fn frame_levels_move_with_their_parent() {
    let mut vm = vm();
    let r = run(
        &mut vm,
        "local menu = CreateFrame('Frame', nil, UIParent)
local entry = CreateFrame('Button', nil, menu)
entry:SetFrameLevel(menu:GetFrameLevel() + 20)
local plain = CreateFrame('Frame', nil, menu)
menu:SetFrameLevel(502)
return entry:GetFrameLevel(), plain:GetFrameLevel()",
    );
    assert_eq!(r, ["522", "503"]);
}

/// A unit position frame draws the player's pin (`AddUnit`) at the
/// player's place on its map, across the frame (the world map's arrow).
#[test]
fn unit_position_frames_draw_the_player() {
    let mut vm = vm();
    wowlua_engine::controls::install(&mut vm, "mock");
    run(
        &mut vm,
        "mock.moveTo({ mapID = 1429, zone = 'Elwynn Forest', x = 0.25, y = 0.75 })
local f = CreateFrame('UnitPositionFrame', 'PinFrame', UIParent)
f:SetSize(200, 100)
f:SetPoint('TOPLEFT')
f:SetUiMapID(1429)
f:AddUnit('player', 'Interface\\\\Buttons\\\\WHITE8X8', 10, 10, 1, 1, 1, 1, 7, true)",
    );
    let scene = wowlua_engine::scene::build(&mut vm);
    let id = scene
        .items
        .iter()
        .find_map(|i| match i {
            wowlua_engine::scene::Item::Quad(q)
                if q.image
                    == wowlua_engine::scene::Image::File("Interface\\Buttons\\WHITE8X8".into()) =>
            {
                Some(q.rect)
            }
            _ => None,
        })
        .expect("the player's pin");
    let unit = id.w / 10.0;
    let (cx, cy) = (id.x + id.w / 2.0, id.y + id.h / 2.0);
    assert!(
        (cx - 50.0 * unit).abs() < 0.5 && (cy - 75.0 * unit).abs() < 0.5,
        "{id:?}"
    );
}

/// `SetAtlas` with an atlas the client lacks leaves the texture as it was
/// (Blizzard's PvP rank ring); WoW Forever's PvP ranks are major faction
/// 2800, as probed.
#[test]
fn unknown_atlases_and_pvp_ranks() {
    let mut vm = vm();
    let r = run(
        &mut vm,
        "local t = UIParent:CreateTexture()
t:SetAtlas('pvpqueue-sidebar-honorbar-background')
t:SetAtlas('UI-Character-Info-Honor-Bar')
local p = C_MajorFactions.GetMajorFactionProgressionInfo(2800)
local r = C_MajorFactions.GetRenownRewardsForLevel(2800, 1)
return string.lower(t:GetAtlas()), p.maxLevel, p.renownLevel, p.renownLevelThreshold,
    C_MajorFactions.GetTotalReputationForRenownLevel(2800, 4), C_MajorFactions.GetTotalReputationForRenownLevel(2800, 14),
    r[1].name",
    );
    assert_eq!(
        r,
        [
            "pvpqueue-sidebar-honorbar-background",
            "14",
            "0",
            "750",
            "3900",
            "24750",
            "Rank 1 Rewards"
        ]
    );
}

/// The client's transmog sources: item 2575 (a shirt) is source 156906.
struct Sources;

impl wowlua_engine::clientdata::ClientData for Sources {
    fn model_scene(&self, _: u32) -> Option<wowlua_engine::clientdata::ModelScene> {
        None
    }
    fn scene_camera(&self, _: u32) -> Option<wowlua_engine::clientdata::SceneCamera> {
        None
    }
    fn scene_actor(&self, _: u32) -> Option<wowlua_engine::clientdata::SceneActor> {
        None
    }
    fn actor_display(&self, _: u32) -> Option<wowlua_engine::clientdata::ActorDisplay> {
        None
    }
    fn item_source(&self, item: u32) -> Option<(u32, u32)> {
        (item == 2575).then_some((156906, 4))
    }
    fn appearance_source(&self, source: u32) -> Option<(u32, u32)> {
        (source == 156906).then_some((2575, 4))
    }
}

/// An actor's transmog list holds what it wears and tried on, and its
/// scene is told of each item it puts on (`OnDressModel`), as in game.
#[test]
fn actors_list_and_tell_what_they_wear() {
    let mut vm = vm();
    wowlua_engine::clientdata::set(&mut vm, std::sync::Arc::new(Sources));
    let r = run(
        &mut vm,
        "local s = CreateFrame('ModelScene', nil, UIParent)
Told = {}
s:SetScript('OnDressModel', function(_, source, slot, removed)
    Told[#Told + 1] = source .. '@' .. slot .. (removed and '-' or '+')
end)
local a = s:CreateActor()
a:SetModelByUnit('player')
a:TryOn('item:2575')
local list = a:GetItemTransmogInfoList()
return list[4].appearanceID, table.concat(Told, ' '), C_TransmogCollection.GetSourceItemID(156906)",
    );
    assert_eq!(r, ["156906", "156906@4+", "2575"]);
}

/// The client's transmog tables: appearance 999001, which no recording
/// holds, with sources 999102 and 999101 of item 2575 (a shirt), one a
/// vendor's, one crafted.
struct Wardrobe;

impl wowlua_engine::clientdata::ClientData for Wardrobe {
    fn model_scene(&self, _: u32) -> Option<wowlua_engine::clientdata::ModelScene> {
        None
    }
    fn scene_camera(&self, _: u32) -> Option<wowlua_engine::clientdata::SceneCamera> {
        None
    }
    fn scene_actor(&self, _: u32) -> Option<wowlua_engine::clientdata::SceneActor> {
        None
    }
    fn actor_display(&self, _: u32) -> Option<wowlua_engine::clientdata::ActorDisplay> {
        None
    }
    fn transmog_source(&self, source: u32) -> Option<wowlua_engine::clientdata::TransmogSource> {
        let source_type = match source {
            999101 => 3,
            999102 => 8,
            _ => return None,
        };
        Some(wowlua_engine::clientdata::TransmogSource {
            source,
            visual: 999001,
            item: 2575,
            item_mod: 0,
            source_type,
            class: 4,
            subclass: 0,
            inventory_type: 4,
        })
    }
    fn appearance_sources(&self, visual: u32) -> Vec<u32> {
        if visual == 999001 {
            vec![999101, 999102]
        } else {
            Vec::new()
        }
    }
}

/// An appearance and source the recordings lack answer from the client's
/// tables (decision T462): in source order, the shirt category, the API's
/// slot and source types, not collected.
#[test]
fn transmog_sources_come_from_the_client_tables() {
    let mut vm = vm();
    wowlua_engine::clientdata::set(&mut vm, std::sync::Arc::new(Wardrobe));
    let r = run(
        &mut vm,
        "local list = C_TransmogCollection.GetAppearanceSources(999001)
local a, b = list[1], list[2]
local info = C_TransmogCollection.GetSourceInfo(999102)
return #list, a.sourceID, a.categoryID, a.invType, a.sourceType, b.sourceType,
    tostring(a.isCollected), info.itemID, info.visualID,
    #C_TransmogCollection.GetAllAppearanceSources(999001),
    C_TransmogCollection.GetCategoryForItem(999101)",
    );
    assert_eq!(
        r,
        [
            "2", "999101", "5", "5", "3", "6", "false", "2575", "999001", "2", "5"
        ]
    );
}

/// The client's flight paths: Durotar (1411) shows Orgrimmar and a
/// neutral node off the map.
struct Flights;

impl wowlua_engine::clientdata::ClientData for Flights {
    fn model_scene(&self, _: u32) -> Option<wowlua_engine::clientdata::ModelScene> {
        None
    }
    fn scene_camera(&self, _: u32) -> Option<wowlua_engine::clientdata::SceneCamera> {
        None
    }
    fn scene_actor(&self, _: u32) -> Option<wowlua_engine::clientdata::SceneActor> {
        None
    }
    fn actor_display(&self, _: u32) -> Option<wowlua_engine::clientdata::ActorDisplay> {
        None
    }
    fn taxi_nodes(&self, ui_map: u32) -> Option<Vec<wowlua_engine::clientdata::TaxiNode>> {
        (ui_map == 1411).then(|| {
            vec![
                wowlua_engine::clientdata::TaxiNode {
                    id: 23,
                    name: "Orgrimmar, Durotar".into(),
                    faction: 1,
                    x: 0.45,
                    y: 0.1,
                },
                wowlua_engine::clientdata::TaxiNode {
                    id: 80,
                    name: "Ratchet, The Barrens".into(),
                    faction: 0,
                    x: -0.2,
                    y: 0.9,
                },
            ]
        })
    }
}

/// A world map's flight paths are the client's tables' (decision T463):
/// in their order, the faction's icon, a position vector.
#[test]
fn flight_paths_come_from_the_client_tables() {
    let mut vm = vm();
    wowlua_engine::clientdata::set(&mut vm, std::sync::Arc::new(Flights));
    let r = run(
        &mut vm,
        "local list = C_TaxiMap.GetTaxiNodesForMap(1411)
local a, b = list[1], list[2]
return #list, a.nodeID, a.name, a.atlasName, b.atlasName, b.faction, b.position.x,
    tostring(a.isUndiscovered)",
    );
    assert_eq!(
        r,
        [
            "2",
            "23",
            "Orgrimmar, Durotar",
            "TaxiNode_Horde",
            "TaxiNode_Neutral",
            "0",
            "-0.2",
            "false"
        ]
    );
}

/// A client whose tables answer `C_Map.GetMapInfo(5000)`, a map no
/// recording has, and Durotar's name otherwise.
struct MapTables;

impl wowlua_engine::clientdata::ClientData for MapTables {
    fn model_scene(&self, _: u32) -> Option<wowlua_engine::clientdata::ModelScene> {
        None
    }
    fn scene_camera(&self, _: u32) -> Option<wowlua_engine::clientdata::SceneCamera> {
        None
    }
    fn scene_actor(&self, _: u32) -> Option<wowlua_engine::clientdata::SceneActor> {
        None
    }
    fn actor_display(&self, _: u32) -> Option<wowlua_engine::clientdata::ActorDisplay> {
        None
    }
    fn answer(&self, call: &str) -> Option<String> {
        match call {
            "C_Map.GetMapInfo(5000)" => Some(
                "1: {flags=number 0,mapID=number 5000,mapType=number 3,name=string New Land,parentMapID=number 947}".into(),
            ),
            // its own data the client's, completed the recorded character's
            "GetAchievementInfo(1042)" => Some(
                "15: number 1042, string Hugs given, number 10, boolean true, number 1, number 2, number 3, string Hugs given, number 1, number 136243, string , boolean false, boolean true, string Someone, boolean true".into(),
            ),
            "C_Map.GetMapInfo(1411)" => Some(
                "1: {flags=number 2,mapID=number 1411,mapType=number 3,name=string Durotar (client),parentMapID=number 1414}".into(),
            ),
            _ => None,
        }
    }
}

/// The client's tables answer a call before the recordings do (decision
/// T464): a map the recordings lack, and a recorded one.
#[test]
fn the_client_tables_answer_before_the_recordings() {
    let mut vm = vm();
    let before = run(&mut vm, "return C_Map.GetMapInfo(1411).name");
    assert_eq!(before, ["Durotar"]);
    wowlua_engine::clientdata::set(&mut vm, std::sync::Arc::new(MapTables));
    let r = run(
        &mut vm,
        "return C_Map.GetMapInfo(5000).name, C_Map.GetMapInfo(5000).parentMapID,
    C_Map.GetMapInfo(1411).name, C_Map.GetMapInfo(1412).name",
    );
    assert_eq!(r, ["New Land", "947", "Durotar (client)", "Mulgore"]);
    // The character's part of an answer stays the recorded one (decision
    // T465).
    let r = run(
        &mut vm,
        "local _, name, points, completed, month, _, _, _, _, _, _, _, mine, by = GetAchievementInfo(1042)
return name, points, tostring(completed), tostring(month), tostring(mine), tostring(by)",
    );
    assert_eq!(r, ["Hugs given", "10", "false", "nil", "false", "nil"]);
}

/// A client whose tables have one atlas of a new build.
struct NewAtlas;

impl wowlua_engine::clientdata::ClientData for NewAtlas {
    fn model_scene(&self, _: u32) -> Option<wowlua_engine::clientdata::ModelScene> {
        None
    }
    fn scene_camera(&self, _: u32) -> Option<wowlua_engine::clientdata::SceneCamera> {
        None
    }
    fn scene_actor(&self, _: u32) -> Option<wowlua_engine::clientdata::SceneActor> {
        None
    }
    fn actor_display(&self, _: u32) -> Option<wowlua_engine::clientdata::ActorDisplay> {
        None
    }
    fn atlas(&self, name: &str) -> Option<wowlua_engine::clientdata::ClientAtlas> {
        (name.eq_ignore_ascii_case("New-Build-Atlas")).then_some(
            wowlua_engine::clientdata::ClientAtlas {
                width: 32.0,
                height: 16.0,
                file: 123456,
                coords: [0.0, 0.5, 0.25, 0.75],
                tiles_horizontally: true,
                tiles_vertically: false,
                raw: (64.0, 32.0),
            },
        )
    }
}

/// An atlas is the client's tables' where at hand (decision T472): one
/// the engine's snapshot lacks exists, with its size, file and region.
#[test]
fn atlases_come_from_the_client_tables() {
    let mut vm = vm();
    let before = run(
        &mut vm,
        "return tostring(C_Texture.GetAtlasExists('New-Build-Atlas'))",
    );
    assert_eq!(before, ["false"]);
    wowlua_engine::clientdata::set(&mut vm, std::sync::Arc::new(NewAtlas));
    let r = run(
        &mut vm,
        "local i = C_Texture.GetAtlasInfo('new-build-atlas')
return tostring(C_Texture.GetAtlasExists('New-Build-Atlas')), i.width, i.height, i.file,
    i.rightTexCoord, tostring(i.tilesHorizontally)",
    );
    assert_eq!(r, ["true", "32", "16", "123456", "0.5", "true"]);
}

/// `BNGetInfo()` answers the account; given an argument, nothing (both
/// recorded).
#[test]
fn bn_get_info_answers_without_an_argument() {
    let mut vm = vm();
    let r = run(
        &mut vm,
        "local _, tag = BNGetInfo()
return tostring(tag ~= nil), select('#', BNGetInfo('player'))",
    );
    assert_eq!(r, ["true", "0"]);
}

/// A texture shows one image: `SetColorTexture` replaces its file, and
/// `SetTexture` its plain color.
#[test]
fn a_color_texture_replaces_the_file() {
    use wowlua_engine::scene::{Image, Item};
    let mut vm = vm();
    run(
        &mut vm,
        "local t = UIParent:CreateTexture()
t:SetSize(10, 10)
t:SetPoint('CENTER')
t:SetTexture('Interface\\\\Buttons\\\\WHITE8X8')
t:SetColorTexture(1, 1, 1, 0.04)",
    );
    let scene = wowlua_engine::scene::build(&mut vm);
    let q = scene
        .items
        .iter()
        .find_map(|i| match i {
            Item::Quad(q) => Some(q.clone()),
            _ => None,
        })
        .expect("the texture");
    assert!(
        matches!(q.image, Image::Color) && (q.color[3] - 0.04).abs() < 0.01,
        "{q:?}"
    );
}

/// The client's creatures: NPC 448 shows display 500.
struct Creatures;

impl wowlua_engine::clientdata::ClientData for Creatures {
    fn model_scene(&self, _: u32) -> Option<wowlua_engine::clientdata::ModelScene> {
        None
    }
    fn scene_camera(&self, _: u32) -> Option<wowlua_engine::clientdata::SceneCamera> {
        None
    }
    fn scene_actor(&self, _: u32) -> Option<wowlua_engine::clientdata::SceneActor> {
        None
    }
    fn actor_display(&self, _: u32) -> Option<wowlua_engine::clientdata::ActorDisplay> {
        None
    }
    fn creature_display(&self, creature: u32) -> Option<u32> {
        (creature == 448).then_some(500)
    }
    fn area_name(&self, area: u32) -> Option<String> {
        (area == 2437).then(|| "Ragefire Chasm".into())
    }
    fn game_file_id(&self, path: &str) -> Option<Option<u32>> {
        Some(
            path.eq_ignore_ascii_case("interface\\buttons\\white8x8")
                .then_some(130871),
        )
    }
    fn item(&self, id: u32) -> Option<wowlua_engine::clientdata::ClientItem> {
        // An item whose name the client's tables lack (sent by the server).
        (id == 872).then(|| wowlua_engine::clientdata::ClientItem {
            icon: 135419,
            class: 2,
            subclass: 1,
            inventory_type: 17,
            ..Default::default()
        })
    }
}

/// A texture tells its file by ID, as the client answered: a game file's
/// own, the ID set, nil for no file; `SetTexture` fails for `""` and 0.
#[test]
fn textures_tell_their_files_by_id() {
    let mut vm = vm();
    wowlua_engine::clientdata::set(&mut vm, std::sync::Arc::new(Creatures));
    let r = run(
        &mut vm,
        "local t = UIParent:CreateTexture()
local out = { tostring(select('#', t:GetTexture())), tostring(t:GetTexture()) }
local function try(file)
    local ok = t:SetTexture(file)
    out[#out + 1] = tostring(ok) .. ' ' .. tostring(t:GetTexture()) .. ' ' .. tostring(t:GetTextureFileID())
end
try('Interface\\\\Buttons\\\\WHITE8X8')
try('Interface/Buttons/NoSuchFile')
try('NoSuchFile')
try('')
try(0)
try(134400)
try(nil)
try('Interface\\\\AddOns\\\\Absent\\\\x.tga')
t:SetAtlas('128-RedButton-Exit')
out[#out + 1] = tostring(t:GetTexture())
return table.concat(out, ', ')",
    );
    assert_eq!(
        r,
        [
            "1, nil, true 130871 130871, true nil nil, true nil nil, false nil nil, false nil nil, \
true 134400 134400, true nil nil, true nil nil, 8063723"
        ]
    );
}

/// An area the recordings lack is named by the client's tables; an ID
/// naming no area stays nil.
#[test]
fn areas_are_named_by_the_clients_tables() {
    let mut vm = vm();
    wowlua_engine::clientdata::set(&mut vm, std::sync::Arc::new(Creatures));
    let r = run(
        &mut vm,
        "return C_Map.GetAreaInfo(2437), tostring(C_Map.GetAreaInfo(0))",
    );
    assert_eq!(r, ["Ragefire Chasm", "nil"]);
}

/// A model given a creature reports its display; a texture given a
/// creature display's portrait shows it; an item whose name the client
/// lacks still has its icon at once.
#[test]
fn creature_displays_portraits_and_unnamed_items() {
    use wowlua_engine::scene::{Image, Item, ModelSource};
    let mut vm = vm();
    wowlua_engine::clientdata::set(&mut vm, std::sync::Arc::new(Creatures));
    let r = run(
        &mut vm,
        "local m = CreateFrame('PlayerModel', nil, UIParent)
m:SetCreature(448)
local t = UIParent:CreateTexture()
t:SetSize(32, 32)
t:SetPoint('CENTER')
SetPortraitTextureFromCreatureDisplayID(t, m:GetDisplayInfo())
local _, _, _, loc, icon = C_Item.GetItemInfoInstant(872)
return m:GetDisplayInfo(), icon, loc, C_Item.GetItemIconByID(872), tostring(C_Item.DoesItemExistByID(872))",
    );
    assert_eq!(r, ["500", "135419", "INVTYPE_2HWEAPON", "135419", "true"]);
    let scene = wowlua_engine::scene::build(&mut vm);
    assert!(scene.items.iter().any(|i| matches!(i,
        Item::Quad(q) if matches!(&q.image, Image::Portrait { source, .. } if **source == ModelSource::Display(500)))));
}

/// Two spells of the client's tables: Fireball (in camelot's scan) and
/// one the scan lacks.
struct Spells;

impl wowlua_engine::clientdata::ClientData for Spells {
    fn model_scene(&self, _: u32) -> Option<wowlua_engine::clientdata::ModelScene> {
        None
    }
    fn scene_camera(&self, _: u32) -> Option<wowlua_engine::clientdata::SceneCamera> {
        None
    }
    fn scene_actor(&self, _: u32) -> Option<wowlua_engine::clientdata::SceneActor> {
        None
    }
    fn actor_display(&self, _: u32) -> Option<wowlua_engine::clientdata::ActorDisplay> {
        None
    }
    fn spell(&self, id: u32) -> Option<wowlua_engine::clientdata::ClientSpell> {
        let (name, cast_ms) = match id {
            133 => ("Fireball (tables)", 1500),
            9_900_001 => ("Unscanned Bolt", 2500),
            _ => return None,
        };
        Some(wowlua_engine::clientdata::ClientSpell {
            name: name.into(),
            subtext: "Rank 9".into(),
            icon: Some(135812),
            cast_ms,
            min_range: 0.0,
            max_range: 30.0,
            school_mask: 4,
            costs: vec![wowlua_engine::clientdata::SpellCost {
                power_type: 0,
                cost: 30,
                min_cost: 30,
                percent: 0.0,
            }],
            ..Default::default()
        })
    }
    fn spell_description(
        &self,
        id: u32,
        caster: &wowlua_engine::clientdata::SpellCaster,
    ) -> Option<String> {
        (id == 9_900_001)
            .then(|| format!("Burns for {} at level {}.", 10 + caster.level, caster.level))
    }
}

/// A spell no test defines comes from the client's tables (decision
/// T391): one camelot's scan lacks (name, rank, icon, cast time, range,
/// costs, its description for the player's level, 60 here), and one the
/// scan and the recordings have: the installation's current data wins.
#[test]
fn unknown_spells_come_from_the_clients_tables() {
    let mut vm = vm();
    wowlua_engine::clientdata::set(&mut vm, std::sync::Arc::new(Spells));
    let r = run(
        &mut vm,
        "local s = C_Spell.GetSpellInfo(9900001)
local f = C_Spell.GetSpellInfo(133)
local cost = C_Spell.GetSpellPowerCost(9900001)[1]
return s.name, s.castTime, s.maxRange, s.iconID, C_Spell.GetSpellSubtext(9900001),
    C_Spell.GetSpellDescription(9900001), f.name, f.castTime, f.maxRange,
    cost.name, cost.cost, tostring(C_Spell.DoesSpellExist(9900001))",
    );
    assert_eq!(
        r,
        [
            "Unscanned Bolt",
            "2500",
            "30",
            "135812",
            "Rank 9",
            "Burns for 70 at level 60.",
            "Fireball (tables)",
            "1500",
            "30",
            "MANA",
            "30",
            "true"
        ]
    );
}

/// A font whose glyphs are 10.2 pixels apart, their ink 8.3 wide.
struct Glyphs;

impl wowlua_engine::clientdata::ClientData for Glyphs {
    fn model_scene(&self, _: u32) -> Option<wowlua_engine::clientdata::ModelScene> {
        None
    }
    fn scene_camera(&self, _: u32) -> Option<wowlua_engine::clientdata::SceneCamera> {
        None
    }
    fn scene_actor(&self, _: u32) -> Option<wowlua_engine::clientdata::SceneActor> {
        None
    }
    fn actor_display(&self, _: u32) -> Option<wowlua_engine::clientdata::ActorDisplay> {
        None
    }
    fn glyph(&self, _font: &str, _px: u32, _c: char) -> Option<(f32, f32)> {
        Some((10.2, 8.3))
    }
}

/// With the installation (decision T392), text the recordings cannot
/// measure is measured with the client's font files: a font they lack and
/// characters outside printable ASCII (advances rounded up, the last
/// glyph its ink rounded up and one more); what they have stays as
/// recorded.
#[test]
fn unrecorded_glyphs_are_measured_with_the_clients_fonts() {
    let measure = |client: bool| {
        let mut vm = vm();
        if client {
            wowlua_engine::clientdata::set(&mut vm, std::sync::Arc::new(Glyphs));
        }
        run(
            &mut vm,
            "UIParent:SetScale(1)
local function w(font, text)
    local fs = UIParent:CreateFontString()
    fs:SetFont(font, 12, '')
    fs:SetShadowOffset(0, 0)
    fs:SetText(text)
    return fs:GetStringWidth()
end
local px = 768 / select(2, GetPhysicalScreenSize())
return w('Fonts\\\\ARHei.ttf', 'ab') / px, w('Fonts\\\\FRIZQT__.TTF', 'éé') / px,
    w('Fonts\\\\FRIZQT__.TTF', 'iiii') / px",
        )
        .into_iter()
        .map(|v| v.parse::<f64>().unwrap().round())
        .collect::<Vec<f64>>()
    };
    let recorded = measure(false);
    let with_fonts = measure(true);
    // 11 + 10: an advance rounded up, then the last glyph's ink rounded
    // up and one
    assert_eq!(with_fonts[0], 21.0, "{with_fonts:?}");
    assert_eq!(with_fonts[1], 21.0, "{with_fonts:?}");
    assert_eq!(with_fonts[2], recorded[2], "recorded glyphs stay");
    assert!(recorded[2] < 30.0, "{recorded:?}");
    assert_ne!(recorded[1], with_fonts[1], "{recorded:?}");
}

/// `Raise()` moves a frame's descendants that have levels of their own
/// with it, as `SetFrameLevel` does: a window raised as it opens keeps
/// its panes over its background (the spellbook's `PlayerSpellsFrame`
/// drew its pages under it).
#[test]
fn raise_keeps_panes_above_their_window() {
    let mut vm = vm();
    let r = run(
        &mut vm,
        "local other = CreateFrame('Frame', nil, UIParent)
other:SetFrameLevel(500)
other:Show()
local window = CreateFrame('Frame', nil, UIParent)
window:SetFrameLevel(1)
local pane = CreateFrame('Frame', nil, window)
pane:SetFrameLevel(100)
window:Raise()
return window:GetFrameLevel(), pane:GetFrameLevel()",
    );
    let (window, pane): (f64, f64) = (r[0].parse().unwrap(), r[1].parse().unwrap());
    assert!(window > 500.0, "{r:?}");
    assert_eq!(pane - window, 99.0, "{r:?}");
}

/// A spell's tooltip with the client's tables (decision T391): its cost
/// and range, its cast time and cooldown, its description; and a tooltip
/// line that wraps (decision T393) breaks at the tooltip's width, at
/// least 300, where `GetStringWidth` stays the text's on one line.
#[test]
fn spell_tooltips_show_the_clients_facts_and_wrap() {
    let mut vm = vm();
    wowlua_engine::clientdata::set(&mut vm, std::sync::Arc::new(Spells));
    let r = run(
        &mut vm,
        "local data = C_TooltipInfo.GetSpellByID(9900001)
local out = {}
for _, l in ipairs(data.lines) do table.insert(out, (l.leftText or '') .. '|' .. (l.rightText or '')) end
local t = CreateFrame('GameTooltip', 'WrapTip', UIParent)
t:SetOwner(UIParent, 'ANCHOR_NONE')
t:AddLine('Short')
t:AddLine(string.rep('word ', 60), 1, 1, 1, true)
return table.concat(out, ' ; '), t:GetWidth(), tostring(WrapTipTextLeft2:GetHeight() > 20)",
    );
    assert_eq!(
        r[0],
        "Unscanned Bolt|Rank 9 ; 30 Mana|30 yd range ; 2.5 sec cast| ; Burns for 70 at level 60.|"
    );
    let width: f64 = r[1].parse().unwrap();
    assert!((width - 320.0).abs() < 1.0, "{r:?}");
    assert_eq!(r[2], "true", "{r:?}");
}
