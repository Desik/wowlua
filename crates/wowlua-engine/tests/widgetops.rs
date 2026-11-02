//! Methods and functions that did nothing (or were not mocked) until
//! decision T289: what the widget's getters and the world answer after
//! them.

#![allow(clippy::disallowed_methods)] // (tests set up their files on the disk)

use wowlua_vm::{Value, Vm};

fn vm() -> Vm {
    let mut vm = Vm::wow();
    wowlua_engine::install(&mut vm, "camelot").expect("installs");
    wowlua_engine::controls::install(&mut vm, "mock");
    vm
}

fn run(vm: &mut Vm, src: &str) -> Vec<String> {
    let f = vm
        .load_as(src.as_bytes(), "@MyAddon/main.lua", "MyAddon")
        .expect("compiles");
    let text = |vm: &Vm, v: Value| match v {
        Value::Str(s) => String::from_utf8_lossy(vm.str_bytes(s)).into_owned(),
        Value::Num(n) => format!("{n}"),
        Value::Bool(b) => format!("{b}"),
        Value::Nil => "nil".into(),
        other => format!("{other:?}"),
    };
    match vm.call(f, &[]) {
        Ok(r) => r.into_iter().map(|v| text(vm, v)).collect(),
        Err(e) => panic!("{}", text(vm, e.value)),
    }
}

#[test]
fn raise_and_lower_move_a_frame_above_and_below_its_strata() {
    let mut vm = vm();
    let r = run(
        &mut vm,
        "local a = CreateFrame('Frame', nil, UIParent)
local b = CreateFrame('Frame', nil, UIParent)
b:SetFrameLevel(7)
local child = CreateFrame('Frame', nil, a)
local high = CreateFrame('Frame', nil, UIParent)
high:SetFrameStrata('HIGH')
high:SetFrameLevel(50)
a:Raise()
local raised, childRaised = a:GetFrameLevel(), child:GetFrameLevel()
a:Lower()
return raised, childRaised, a:GetFrameLevel(), b:GetFrameLevel()",
    );
    // Above b's 7 (not the HIGH strata's 50), its child with it; then
    // below every other frame of its strata (UIParent's 0 among them).
    assert_eq!(r, ["8", "9", "0", "7"]);
}

#[test]
fn an_edit_box_inserts_in_place_of_its_selection() {
    let mut vm = vm();
    let r = run(
        &mut vm,
        "local e = CreateFrame('EditBox', nil, UIParent)
e:SetText('hello world')
e:HighlightText(0, 5)
e:Insert('HELLO')
local first, cursor = e:GetText(), e:GetCursorPosition()
e:SetCursorPosition(0)
e:Insert('> ')
local second = e:GetText()
e:SetCursorPosition(1)
e:SetText('abc')
e:Insert('!')
local third = e:GetText()
e:HighlightText()
e:ClearHighlightText()
e:Insert('?')
return first, cursor, second, third, e:GetText()",
    );
    assert_eq!(r, ["HELLO world", "5", "> HELLO world", "abc!", "abc!?"]);
}

#[test]
fn typing_replaces_the_selected_text() {
    let mut vm = vm();
    let r = run(
        &mut vm,
        "local e = CreateFrame('EditBox', nil, UIParent)
local changes = {}
e:SetScript('OnTextChanged', function(self, user) changes[#changes + 1] = tostring(user) end)
e:SetText('old')
e:SetScript('OnEditFocusGained', function(self) self:HighlightText() end)
mock.type(e, 'new')
return e:GetText(), table.concat(changes, ' ')",
    );
    assert_eq!(r, ["new", "true true true"]);
}

#[test]
fn typing_into_a_box_starts_after_its_text() {
    let mut vm = vm();
    let r = run(
        &mut vm,
        "local e = CreateFrame('EditBox', nil, UIParent)
e:SetText('Sheep')
e:SetCursorPosition(0)
mock.type(e, '!')
local first = e:GetText()
e:SetCursorPosition(0)
mock.type(e, '>')
return first, e:GetText()",
    );
    // A click into the box puts the cursor after the text; with the focus
    // kept, typing goes where the cursor was put.
    assert_eq!(r, ["Sheep!", ">Sheep!"]);
}

#[test]
fn copy_font_object_copies_the_font_and_its_colors() {
    let mut vm = vm();
    let r = run(
        &mut vm,
        "local a = CreateFont('CopySource')
a:SetFont('Fonts\\\\FRIZQT__.TTF', 14, 'OUTLINE')
a:SetTextColor(1, 0, 0)
a:SetJustifyH('LEFT')
local b = CreateFont('CopyTarget')
b:CopyFontObject('CopySource')
a:SetTextColor(0, 1, 0)
local file, size, flags = b:GetFont()
local r, g = b:GetTextColor()
return file, size == select(2, a:GetFont()), flags, r, g, b:GetJustifyH()",
    );
    assert_eq!(
        r,
        ["Fonts\\FRIZQT__.TTF", "true", "OUTLINE", "1", "0", "LEFT"]
    );
}

#[test]
fn points_offsets_adjust_and_clear() {
    let mut vm = vm();
    let r = run(
        &mut vm,
        "local f = CreateFrame('Frame', nil, UIParent)
f:SetPoint('TOPLEFT', 10, 20)
f:AdjustPointsOffset(5, -5)
local _, _, _, x, y = f:GetPoint(1)
f:ClearPointsOffset()
local _, _, _, x2, y2 = f:GetPoint(1)
return x, y, x2, y2",
    );
    assert_eq!(r, ["15", "15", "0", "0"]);
}

#[test]
fn the_clears_undo_their_setters() {
    let mut vm = vm();
    let r = run(
        &mut vm,
        "local b = CreateFrame('Button', nil, UIParent)
b:SetNormalTexture('Interface\\\\Buttons\\\\WHITE8X8')
local tex = b:GetNormalTexture()
b:ClearNormalTexture()
local fs = b:CreateFontString(nil, 'OVERLAY')
fs:SetFont('Fonts\\\\FRIZQT__.TTF', 12, '')
fs:SetText('x')
fs:ClearText()
local holder = CreateFrame('Frame', nil, UIParent)
local kid = CreateFrame('Frame', nil, holder)
kid:SetParentKey('Kid')
kid:ClearParentKey()
local t = holder:CreateTexture()
t:SetTexCoord(0.1, 0.9, 0.1, 0.9)
t:ResetTexCoord()
local m = CreateFrame('PlayerModel', nil, UIParent)
m:SetModel(130737)
m:ClearModel()
return tostring(b:GetNormalTexture() == tex and tex:GetTexture()), tostring(tex:IsShown()), tostring(fs:GetText()),
    tostring(holder.Kid), tostring(kid:GetParentKey()), t:GetTexCoord(), m:GetModelFileID()",
    );
    assert_eq!(r, ["nil", "false", "nil", "nil", "nil", "0", "0"]);
    // (a fresh button's cleared texture: none, as the client answered)
    let r = run(
        &mut vm,
        "local b = CreateFrame('Button', nil, UIParent)
b:ClearNormalTexture()
return tostring(b:GetNormalTexture())",
    );
    assert_eq!(r, ["nil"]);
}

#[test]
fn stop_animating_stops_the_regions_groups() {
    let mut vm = vm();
    let r = run(
        &mut vm,
        "local f = CreateFrame('Frame', nil, UIParent)
local g = f:CreateAnimationGroup()
local a = g:CreateAnimation('Alpha')
a:SetDuration(10)
local stopped = 0
g:SetScript('OnStop', function() stopped = stopped + 1 end)
g:Play()
local playing = g:IsPlaying()
f:StopAnimating()
return tostring(playing), tostring(g:IsPlaying()), stopped",
    );
    assert_eq!(r, ["true", "false", "1"]);
}

#[test]
fn the_merchant_filter_is_kept() {
    let mut vm = vm();
    let r = run(
        &mut vm,
        "local before = GetMerchantFilter()
SetMerchantFilter(1)
local set = GetMerchantFilter()
ResetSetMerchantFilter()
return before, set, GetMerchantFilter()",
    );
    assert_eq!(r, ["2", "1", "2"]);
}

#[test]
fn the_guild_roster_keeps_its_selection() {
    let mut vm = vm();
    let r = run(
        &mut vm,
        "local outside = GetGuildRosterSelection()
mock.joinGuild({ members = { { name = 'Other' } } })
SetGuildRosterSelection(2)
local picked = GetGuildRosterSelection()
SetGuildRosterSelection(9)
return outside, picked, GetGuildRosterSelection()",
    );
    assert_eq!(r, ["0", "2", "0"]);
}

#[test]
fn an_early_landing_ends_the_flight_once() {
    let mut vm = vm();
    let r = run(
        &mut vm,
        "local master = mock.spawnUnit({ name = 'Gryth Skyfeather', level = 55, reaction = 'friendly' })
mock.defineGossip(master, { text = 'Where to?', options = {
    { name = 'I need a ride.', icon = 132057, gossipOptionID = 30000, opens = 'taxi' },
} })
mock.defineTaxi(master, { mapID = 1415, nodes = {
    { name = 'Stormwind, Elwynn', nodeID = 2, x = 0.4, y = 0.7, state = 0, cost = 0 },
    { name = 'Ironforge, Dun Morogh', nodeID = 6, x = 0.45, y = 0.5, state = 1, cost = 0,
      routes = 1, hops = { 1, 2 }, flightTime = 100 },
} })
master:setDistance(3)
mock.player:interact(master)
mock.advanceTime(1)
C_GossipInfo.SelectOption(30000)
mock.advanceTime(1)
local landings = 0
local f = CreateFrame('Frame')
f:RegisterEvent('PLAYER_CONTROL_GAINED')
f:SetScript('OnEvent', function() landings = landings + 1 end)
TaxiRequestEarlyLanding()
local grounded = landings
TakeTaxiNode(2)
mock.advanceTime(1)
local flying = UnitOnTaxi('player')
TaxiRequestEarlyLanding()
mock.advanceTime(1)
local landed = UnitOnTaxi('player')
mock.advanceTime(200)
return grounded, tostring(flying), tostring(landed), landings",
    );
    assert_eq!(r, ["0", "true", "false", "1"]);
}

#[test]
fn clears_of_attributes_scripts_and_animations() {
    let mut vm = vm();
    let r = run(
        &mut vm,
        "local f = CreateFrame('Frame', nil, UIParent)
f:SetAttribute('unit', 'player')
f:ClearAttributes()
f:SetScript('OnShow', function() end)
f:ClearScripts()
local g = f:CreateAnimationGroup()
g:CreateAnimation('Alpha'):SetDuration(2)
g:RemoveAnimations()
local b = CreateFrame('Button', nil, UIParent)
b:LockHighlight()
local locked = b:IsHighlightLocked()
local m = CreateFrame('ModelScene', nil, UIParent)
return tostring(f:GetAttribute('unit')), tostring(f:GetScript('OnShow')), select('#', g:GetAnimations()),
    g:GetDuration(), tostring(locked), m:GetDrawLayer()",
    );
    assert_eq!(r, ["nil", "nil", "0", "0", "true", "ARTWORK", "0"]);
}
