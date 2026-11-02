//! Blizzard's UI code loaded as secure code: a small UI tree, and the
//! flavor's own when it is on disk (`cargo xtask types` fetches it).

#![allow(clippy::disallowed_methods)] // (tests set up their files on the disk)

use std::path::{Path, PathBuf};

use wowlua_vm::{Value, Vm};

fn vm(flavor: &str) -> Vm {
    let mut vm = Vm::wow();
    wowlua_engine::install(&mut vm, flavor).expect("installs");
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

/// Writes `files` under a fresh directory; returns its `Interface` folder.
fn tree(name: &str, files: &[(&str, &str)]) -> PathBuf {
    let root = std::env::temp_dir().join(format!("wowlua-ui-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    for (path, text) in files {
        let p = root.join("Interface").join(path);
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(p, text).unwrap();
    }
    root.join("Interface")
}

#[test]
fn templates_mixins_scripts_and_load_order() {
    let interface = tree(
        "basic",
        &[
            (
                "AddOns/Blizzard_Base/Blizzard_Base.toc",
                "## Title: Base\n## LoadFirst: 1\nMixin.lua\nTemplates.xml\n[Family]\\Family.lua\n",
            ),
            (
                "AddOns/Blizzard_Base/Mixin.lua",
                "local name = ...\nLoadLog = { name }\nMyMixin = {}\nfunction MyMixin:OnLoad() LoadLog[#LoadLog + 1] = 'mixin ' .. (self:GetName() or '?') end\nfunction MyMixin:Hello() return 'hello' end\n",
            ),
            (
                "AddOns/Blizzard_Base/Mainline/Family.lua",
                "FamilyLoaded = true\n",
            ),
            (
                "AddOns/Blizzard_Base/Templates.xml",
                r#"<Ui xmlns="http://www.blizzard.com/wow/ui/">
  <Frame name="MyTemplate" virtual="true" mixin="MyMixin">
    <Size x="100" y="40"/>
    <KeyValues><KeyValue key="kind" value="templated"/><KeyValue key="count" value="3" type="number"/></KeyValues>
    <Layers><Layer level="ARTWORK"><FontString parentKey="Label" inherits="MyFont" text="MY_TEXT"/></Layer></Layers>
    <Scripts><OnLoad method="OnLoad"/></Scripts>
  </Frame>
  <Font name="MyFont" font="Fonts\FRIZQT__.TTF" height="14" outline="NORMAL" virtual="true"/>
  <Frame name="MyEventFrame" intrinsic="true">
    <Scripts><OnLoad>LoadLog[#LoadLog + 1] = 'intrinsic'</OnLoad></Scripts>
  </Frame>
</Ui>"#,
            ),
            (
                "AddOns/Blizzard_Later/Blizzard_Later.toc",
                "## Dep: Blizzard_Base\nLater.lua\nLater.xml\n",
            ),
            (
                "AddOns/Blizzard_Later/Later.lua",
                "MY_TEXT = 'Shown text'\nLoadLog[#LoadLog + 1] = 'later'\n",
            ),
            (
                "AddOns/Blizzard_Later/Later.xml",
                r#"<Ui>
  <Frame name="MyFrame" parent="UIParent" inherits="MyTemplate" hidden="true">
    <Anchors><Anchor point="TOPLEFT" x="10" y="-20"/></Anchors>
    <Frames>
      <Button name="$parentButton" parentKey="Button">
        <Scripts>
          <OnLoad>LoadLog[#LoadLog + 1] = 'button ' .. self:GetName()</OnLoad>
          <OnClick>Clicked = button</OnClick>
        </Scripts>
      </Button>
    </Frames>
    <Scripts><OnLoad inherit="append">LoadLog[#LoadLog + 1] = 'own'</OnLoad></Scripts>
  </Frame>
</Ui>"#,
            ),
            (
                "AddOns/Blizzard_Lazy/Blizzard_Lazy.toc",
                "## LoadOnDemand: 1\nLazy.lua\n",
            ),
            ("AddOns/Blizzard_Lazy/Lazy.lua", "LazyLoaded = true\n"),
            (
                "AddOns/Blizzard_Glue/Blizzard_Glue.toc",
                "## AllowLoad: Glue\nGlue.lua\n",
            ),
            ("AddOns/Blizzard_Glue/Glue.lua", "GlueLoaded = true\n"),
        ],
    );
    let mut vm = vm("camelot");
    let report = wowlua_engine::ui::load(&mut vm, &interface, None);
    assert!(report.errors.is_empty(), "{:?}", report.errors);
    assert_eq!(report.addons, ["Blizzard_Base", "Blizzard_Later"]);
    let r = run(
        &mut vm,
        "local f = MyFrame
f.Button:Click()
local made = CreateFrame('Frame', 'Made', UIParent, 'MyTemplate')
local intrinsic = CreateFrame('MyEventFrame')
return table.concat(LoadLog, ','), tostring(FamilyLoaded), tostring(LazyLoaded), tostring(GlueLoaded),
    f:GetWidth(), f.kind, f.count, f.Label:GetText(), select(2, f.Label:GetFont()), tostring(f:IsShown()),
    select(4, f:GetPoint(1)), f:Hello(), tostring(Clicked), made:GetHeight(), made.Label:GetText(),
    intrinsic:GetObjectType(), tostring(issecurevariable('MyFrame'))",
    );
    assert_eq!(
        r,
        [
            "Blizzard_Base,later,button MyFrameButton,own,mixin MyFrame,mixin Made,intrinsic",
            "true",
            "nil",
            "nil",
            "100",
            "templated",
            "3",
            "Shown text",
            "13.999999046325684", // 14 as camelot keeps it
            "false",
            "10",
            "hello",
            "LeftButton",
            "40",
            "Shown text",
            "Frame",
            "true",
        ]
    );
}

/// A secure addon's API for addons, as Blizzard_AuraContainer builds it: a
/// template under `allowUntaintedCreation` is built untainted for an
/// addon (its scripts and `OnLoad` Blizzard's), and its inbound mixin's
/// `secureDelegates` run untainted for a tainted caller, unless it passes
/// a secret; `CreateSecureDelegate` elevates even then.
#[test]
fn untainted_creation_and_secure_delegates() {
    let interface = tree(
        "delegates",
        &[
            (
                "AddOns/Blizzard_Box/Blizzard_Box.toc",
                "## UseSecureEnvironment: 1\nBox.lua\nBox.xml [LoadIntoEnvironment global]\nExport.lua [LoadIntoEnvironment global]\n",
            ),
            (
                "AddOns/Blizzard_Box/Box.lua",
                "local _, addonTable = ...
local Shared = {}
function Shared:SetValue(v) self.value = v self.setSecure = issecure() end
function Shared:IsOwn() return GetForbiddenObjectTable(self) == self end
BoxInbound = { SetValue = Shared.SetValue, IsOwn = Shared.IsOwn }
BoxPrivate = { SetValue = Shared.SetValue, IsOwn = Shared.IsOwn }
function BoxPrivate:OnLoad() self.loadSecure = issecure() end
function BoxPrivate:OnUpdate() self.updateSecure = issecure() end
addonTable.Delegate = CreateSecureDelegate(function(x) return issecure(), x ~= nil end)
",
            ),
            (
                "AddOns/Blizzard_Box/Export.lua",
                "local _, addonTable = ...\nBoxDelegate = addonTable.Delegate\n",
            ),
            (
                "AddOns/Blizzard_Box/Box.xml",
                r#"<Ui>
  <ScopedModifier allowUntaintedCreation="true">
    <Frame name="BoxTemplate" virtual="true">
      <Mixins>
        <Mixin key="BoxInbound" source="secure" secureDelegates="true"/>
        <Mixin key="BoxPrivate" source="secure"/>
      </Mixins>
      <Scripts><OnLoad method="OnLoad"/><OnUpdate method="OnUpdate"/></Scripts>
    </Frame>
  </ScopedModifier>
  <Frame name="PlainBoxTemplate" virtual="true">
    <Mixins><Mixin key="BoxPrivate" source="secure"/></Mixins>
    <Scripts><OnLoad method="OnLoad"/></Scripts>
  </Frame>
</Ui>"#,
            ),
        ],
    );
    let mut vm = vm("camelot");
    let report = wowlua_engine::ui::load(&mut vm, &interface, None);
    assert!(report.errors.is_empty(), "{:?}", report.errors);
    let r = run(
        &mut vm,
        "Box = CreateFrame('Frame', nil, UIParent, 'BoxTemplate')
local plain = CreateFrame('Frame', nil, UIParent, 'PlainBoxTemplate')
Box:SetValue({})
local set, value = tostring(Box.setSecure), tostring(issecurevariable(Box, 'value'))
local after = tostring(issecure())
Box:SetValue(secretwrap(1))
local elevated, given = BoxDelegate(secretwrap(1))
return tostring(Box.loadSecure), tostring(plain.loadSecure), set, value, after,
    tostring(Box.setSecure), tostring(elevated), tostring(given), tostring(Box:IsOwn()),
    tostring(GetForbiddenObjectTable)",
    );
    assert_eq!(
        r,
        [
            "true", "false", "true", "true", "false", "false", "true", "true", "true", "nil"
        ]
    );
    wowlua_engine::clock::advance_frames(&mut vm, 1, 16.0);
    assert_eq!(run(&mut vm, "return tostring(Box.updateSecure)"), ["true"]);
}

/// `setAllPoints` with the element's own `<Anchors>`: the anchors and size
/// place it (Blizzard's player frame rest icon); without anchors it fills
/// its parent.
#[test]
fn own_anchors_take_the_place_of_set_all_points() {
    let interface = tree(
        "allpoints",
        &[
            ("AddOns/Blizzard_Rest/Blizzard_Rest.toc", "Rest.xml\n"),
            (
                "AddOns/Blizzard_Rest/Rest.xml",
                r#"<Ui>
  <Frame name="RestHolder" parent="UIParent">
    <Size x="200" y="100"/>
    <Anchors><Anchor point="TOPLEFT" x="10" y="-10"/></Anchors>
    <Frames>
      <Frame parentKey="Loop" setAllPoints="true">
        <Size x="20" y="20"/>
        <Anchors><Anchor point="TOPLEFT" x="64" y="-6"/></Anchors>
      </Frame>
      <Frame parentKey="Fill" setAllPoints="true"/>
    </Frames>
  </Frame>
</Ui>"#,
            ),
        ],
    );
    let mut vm = vm("camelot");
    let report = wowlua_engine::ui::load(&mut vm, &interface, None);
    assert!(report.errors.is_empty(), "{:?}", report.errors);
    let r = run(
        &mut vm,
        "local l, f = RestHolder.Loop, RestHolder.Fill
local p, _, rp, x, y = l:GetPoint(1)
return l:GetNumPoints(), p, rp, x, y, l:GetWidth(), l:GetHeight(), f:GetNumPoints(), f:GetWidth()",
    );
    assert_eq!(
        r,
        [
            "1", "TOPLEFT", "TOPLEFT", "64", "-6", "20", "20", "2", "200"
        ]
    );
}

/// A texture the XML does not anchor fills its parent, sized or not; a
/// slider's thumb does not, the slider anchors it `LEFT` to its `LEFT`
/// along its length by its value; a texture's `rotation` is in degrees.
#[test]
fn unanchored_textures_thumbs_and_rotation() {
    let interface = tree(
        "unanchored",
        &[
            ("AddOns/Blizzard_Box/Blizzard_Box.toc", "Box.xml\n"),
            (
                "AddOns/Blizzard_Box/Box.xml",
                r#"<Ui>
  <Frame name="BoxHolder" parent="UIParent">
    <Size x="200" y="100"/>
    <Anchors><Anchor point="TOPLEFT" x="10" y="-10"/></Anchors>
    <Layers><Layer level="ARTWORK">
      <Texture parentKey="Plain"/>
      <FontString parentKey="Label"/>
      <Texture parentKey="Sized"><Size x="40" y="20"/></Texture>
      <Texture parentKey="Turned" rotation="90"><Size x="10" y="50"/>
        <Anchors><Anchor point="CENTER"/></Anchors>
      </Texture>
    </Layer></Layers>
    <Frames>
      <Slider parentKey="Bar" orientation="HORIZONTAL" minValue="0" maxValue="10" defaultValue="5">
        <Size x="110" y="20"/>
        <Anchors><Anchor point="BOTTOMLEFT"/></Anchors>
        <ThumbTexture><Size x="10" y="10"/></ThumbTexture>
      </Slider>
    </Frames>
  </Frame>
</Ui>"#,
            ),
        ],
    );
    let mut vm = vm("camelot");
    let report = wowlua_engine::ui::load(&mut vm, &interface, None);
    assert!(report.errors.is_empty(), "{:?}", report.errors);
    let r = run(
        &mut vm,
        "local h = BoxHolder
local thumb = h.Bar:GetThumbTexture()
local hl, hb = h:GetLeft(), h:GetBottom()
h.Bar:SetValue(5)
local mid = thumb:GetLeft() - hl
local p, rel, rp, x, y = thumb:GetPoint(1)
local anchor = string.format('%s %s %s %g %g', p, tostring(rel == h.Bar), rp, x, y)
h.Bar:SetValue(10)
assert(h.Label:GetNumPoints() == 2, 'an unanchored font string fills its parent')
return h.Plain:GetNumPoints(), h.Sized:GetNumPoints(), h.Sized:GetWidth(), thumb:GetNumPoints(),
    mid, thumb:GetLeft() - hl, thumb:GetBottom() - hb, string.format('%.4f', h.Turned:GetRotation()), anchor",
    );
    assert_eq!(
        r,
        [
            "2",
            "2",
            "200",
            "1",
            "50",
            "100",
            "5",
            "1.5708",
            "LEFT true LEFT 50 0"
        ]
    );
}

/// An intrinsic type declared with `allowUntaintedCreation` (Blizzard's
/// `AuraContainer`), made by an addon while the addon loads: its `method`
/// scripts are Blizzard's, not the loading addon's code, so its `OnLoad`
/// runs untainted, as when the addon makes one later (the client: SuuUI
/// makes its aura containers while it loads, without an error).
#[test]
fn untainted_intrinsics_made_while_an_addon_loads() {
    let interface = tree(
        "intrinsic-untainted",
        &[
            (
                "AddOns/Blizzard_Box/Blizzard_Box.toc",
                "## UseSecureEnvironment: 1\nBox.lua\nBox.xml [LoadIntoEnvironment global]\n",
            ),
            (
                "AddOns/Blizzard_Box/Box.lua",
                "BoxPrivate = {}
function BoxPrivate:OnLoad_Intrinsic() self.loadSecure = issecure() end
",
            ),
            (
                "AddOns/Blizzard_Box/Box.xml",
                r#"<Ui>
  <ScopedModifier allowUntaintedCreation="true">
    <Frame name="BoxFrame" intrinsic="true">
      <Mixins><Mixin key="BoxPrivate" source="secure"/></Mixins>
      <Scripts><OnLoad method="OnLoad_Intrinsic"/></Scripts>
    </Frame>
  </ScopedModifier>
</Ui>"#,
            ),
        ],
    );
    let mut vm = vm("camelot");
    let report = wowlua_engine::ui::load(&mut vm, &interface, None);
    assert!(report.errors.is_empty(), "{:?}", report.errors);
    let addon = interface.parent().unwrap().join("MyAddon");
    std::fs::create_dir_all(&addon).unwrap();
    std::fs::write(
        addon.join("main.lua"),
        "AtLoad = CreateFrame('BoxFrame', nil, UIParent)\n",
    )
    .unwrap();
    let table = Value::Table(vm.new_table());
    wowlua_engine::ui::load_user_file(&mut vm, &addon, "main.lua", "MyAddon", table);
    assert_eq!(
        run(
            &mut vm,
            "local later = CreateFrame('BoxFrame', nil, UIParent)
return tostring(AtLoad.loadSecure), tostring(later.loadSecure), tostring(issecure())"
        ),
        ["true", "true", "false"]
    );
}

/// Blizzard's secure code making a frame from an addon's template with an
/// inline `OnLoad` stays secure after: the script compiles as the addon's
/// code, its function carrying the addon's taint (the `OnLoad` runs
/// tainted), without tainting the
/// execution that applies the template (the client: a secure group header
/// runs its `initialConfigFunction` for an addon's unit button template).
#[test]
fn an_addon_template_script_does_not_taint_its_secure_maker() {
    let interface = tree(
        "addon-template-secure-maker",
        &[
            ("AddOns/Blizzard_Box/Blizzard_Box.toc", "Box.lua\n"),
            (
                "AddOns/Blizzard_Box/Box.lua",
                "function BoxMake(template)
    local f = CreateFrame('Button', nil, UIParent, template)
    return issecure(), f.loadSecure
end
",
            ),
        ],
    );
    let mut vm = vm("camelot");
    let report = wowlua_engine::ui::load(&mut vm, &interface, None);
    assert!(report.errors.is_empty(), "{:?}", report.errors);
    let addon = interface.parent().unwrap().join("MyAddon");
    std::fs::create_dir_all(&addon).unwrap();
    std::fs::write(
        addon.join("templates.xml"),
        r#"<Ui>
  <Button name="MyLoadTemplate" virtual="true">
    <Scripts><OnLoad>self.loadSecure = issecure()</OnLoad></Scripts>
  </Button>
</Ui>"#,
    )
    .unwrap();
    let table = Value::Table(vm.new_table());
    wowlua_engine::ui::load_user_file(&mut vm, &addon, "templates.xml", "MyAddon", table);
    let make = vm.get_global("BoxMake");
    let template = vm.str("MyLoadTemplate");
    let r = vm.call_secure(make, &[template]).expect("runs");
    assert!(
        matches!(r[..], [Value::Bool(true), Value::Bool(false)]),
        "the maker secure after, the addon's OnLoad tainted: {r:?}"
    );
}

/// A load-on-demand Blizzard addon an addon loads while it loads is
/// Blizzard's code, as when the addon loads one later: its functions stay
/// secure and its files keep their names (spec §17.7: Blizzard's Lua files
/// run as secure code with their addon's name).
/// `import` finds a module next to the importing file (`.Name`) or in
/// another addon's folder (`AddOn.Name`, decision T566).
#[test]
fn modules_are_imported_by_relative_and_addon_names() {
    let interface = tree(
        "imports",
        &[
            (
                "AddOns/Blizzard_Helpers/Blizzard_Helpers.toc",
                "Helper.lua\nSub/Inner.lua\nUser.lua\n",
            ),
            (
                "AddOns/Blizzard_Helpers/Helper.lua",
                "return { name = 'helper' }\n",
            ),
            (
                "AddOns/Blizzard_Helpers/Sub/Inner.lua",
                "return { name = 'inner' }\n",
            ),
            (
                "AddOns/Blizzard_Helpers/User.lua",
                "RelativeName = import('.Helper').name .. import('.Sub.Inner').name\n",
            ),
            (
                "AddOns/Blizzard_Other/Blizzard_Other.toc",
                "## Dep: Blizzard_Helpers\nOther.lua\n",
            ),
            (
                "AddOns/Blizzard_Other/Other.lua",
                "AbsoluteName = import('Blizzard_Helpers.Helper').name\n\
                 .. import('Blizzard_Helpers.Sub.Inner').name\n\
                 Missing = select(2, pcall(import, 'Blizzard_Helpers.Nothing'))\n",
            ),
        ],
    );
    let mut vm = vm("camelot");
    let report = wowlua_engine::ui::load(&mut vm, &interface, None);
    assert!(report.errors.is_empty(), "{:?}", report.errors);
    let r = run(&mut vm, "return RelativeName, AbsoluteName, Missing");
    assert_eq!(r[..2], ["helperinner", "helperinner"]);
    assert!(
        r[2].ends_with("import: no module 'Blizzard_Helpers.Nothing'"),
        "{}",
        r[2]
    );
}

#[test]
fn blizzard_addons_loaded_while_an_addon_loads() {
    let interface = tree(
        "lod-while-loading",
        &[
            (
                "AddOns/Blizzard_Early/Blizzard_Early.toc",
                "## LoadOnDemand: 1\nEarly.lua\n",
            ),
            (
                "AddOns/Blizzard_Early/Early.lua",
                "function EarlyShow() end\nEarlySource = debugstack(1, 1, 0)\n",
            ),
            (
                "AddOns/Blizzard_Late/Blizzard_Late.toc",
                "## LoadOnDemand: 1\nLate.lua\n",
            ),
            ("AddOns/Blizzard_Late/Late.lua", "function LateShow() end\n"),
        ],
    );
    let mut vm = vm("camelot");
    let report = wowlua_engine::ui::load(&mut vm, &interface, None);
    assert!(report.errors.is_empty(), "{:?}", report.errors);
    let addon = interface.parent().unwrap().join("MyAddon");
    std::fs::create_dir_all(&addon).unwrap();
    std::fs::write(
        addon.join("main.lua"),
        "C_AddOns.LoadAddOn('Blizzard_Early')\nMyAddonGlobal = 1\n",
    )
    .unwrap();
    let table = Value::Table(vm.new_table());
    // Named as the client names the addon's files, as the test runner does.
    wowlua_engine::engine_mut(&mut vm).world.ui.chunk_prefix =
        Some("Interface/AddOns/MyAddon/".into());
    wowlua_engine::ui::load_user_file(&mut vm, &addon, "main.lua", "MyAddon", table);
    wowlua_engine::engine_mut(&mut vm).world.ui.chunk_prefix = None;
    assert_eq!(
        run(
            &mut vm,
            "C_AddOns.LoadAddOn('Blizzard_Late')
return tostring(issecurevariable('EarlyShow')), tostring(issecurevariable('LateShow')),
    tostring(issecurevariable('MyAddonGlobal')), EarlySource"
        ),
        [
            "true",
            "true",
            "false",
            "[Interface/AddOns/Blizzard_Early/Early.lua]:2: in main chunk\n..."
        ]
    );
}

/// A frame whose untrusted layout scripts are forbidden (ElvUI's aura
/// containers inherit `DisableUntrustedLayoutScriptsTemplate`): an addon's
/// `SetSize` hook does not see the secrets Blizzard's layout sizes it with
/// (the user's client: no error from ElvUI's hook), but sees the addon's
/// own sizes; its tainted `OnSizeChanged` does not run, a secure one does.
#[test]
fn forbidden_untrusted_layout_scripts() {
    let interface = tree(
        "aspects",
        &[
            (
                "AddOns/Blizzard_Aspects/Blizzard_Aspects.toc",
                "Layout.lua\nTemplates.xml\n",
            ),
            (
                "AddOns/Blizzard_Aspects/Layout.lua",
                "function LayoutIt(f) f:SetSize(secretwrap(30, 40)) end\nfunction SecureSized(f) f.secureSized = true end\n",
            ),
            (
                "AddOns/Blizzard_Aspects/Templates.xml",
                r#"<Ui>
  <Frame name="DisableUntrustedLayoutScriptsTemplate" virtual="true">
    <ForbiddenAspects><ForbiddenAspect aspect="UntrustedLayoutScriptExecution"/></ForbiddenAspects>
  </Frame>
</Ui>"#,
            ),
        ],
    );
    let mut vm = vm("camelot");
    let report = wowlua_engine::ui::load(&mut vm, &interface, None);
    assert!(report.errors.is_empty(), "{:?}", report.errors);
    run(
        &mut vm,
        "Kept = CreateFrame('Frame', nil, UIParent, 'DisableUntrustedLayoutScriptsTemplate')
Plain = CreateFrame('Frame', nil, UIParent)
local mover = CreateFrame('Frame', nil, UIParent)
Seen, Errors = {}, {}
seterrorhandler(function(e) Errors[#Errors + 1] = e end)
for _, f in ipairs({ Kept, Plain }) do
    f:SetPoint('CENTER')
    hooksecurefunc(f, 'SetSize', function(self, w, h)
        Seen[#Seen + 1] = self == Kept and 'kept' or 'plain'
        mover:SetSize(w, h)
    end)
    f:SetScript('OnSizeChanged', function(self) Seen[#Seen + 1] = 'size' end)
    f:HookScript('OnSizeChanged', SecureSized)
end",
    );
    for name in ["Kept", "Plain"] {
        let layout = vm.get_global("LayoutIt");
        let frame = vm.get_global(name);
        vm.call_from_engine(layout, &[frame]);
    }
    wowlua_engine::clock::advance_frames(&mut vm, 1, 16.0);
    let r = run(
        &mut vm,
        "Kept:SetSize(1, 2)
return table.concat(Seen, ','), #Errors, tostring(Kept.secureSized), tostring(Plain.secureSized)",
    );
    assert_eq!(r, ["plain,size,kept", "1", "true", "true"]);
}

/// The flavor's own UI code, if `cargo xtask types` fetched it: loads, and
/// says how many errors remain.
#[test]
fn the_flavor_ui_code_loads() {
    let interface =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/types-src/camelot/Interface");
    if !interface.exists() {
        eprintln!("skipped: {} is missing", interface.display());
        return;
    }
    // The flavor's global strings, as `cargo xtask types` names them.
    let xtask = std::fs::read_to_string(
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../xtask/src/types.rs"),
    )
    .unwrap_or_default();
    let commit = xtask
        .split("\"camelot\",")
        .nth(1)
        .and_then(|rest| rest.split('"').nth(3))
        .unwrap_or("");
    let strings = interface.join(format!("../../strings/{commit}.lua"));
    let mut vm = vm("camelot");
    let start = std::time::Instant::now();
    let report = wowlua_engine::ui::load(
        &mut vm,
        &interface,
        strings.exists().then_some(strings.as_path()),
    );
    eprintln!(
        "{} addons, {} errors in {:?}",
        report.addons.len(),
        report.errors.len(),
        start.elapsed()
    );
    let limit = if std::env::var("WOWLUA_UI_ERRORS").is_ok() {
        usize::MAX
    } else {
        40
    };
    for e in report.errors.iter().take(limit) {
        eprintln!("  {e}");
    }
    // The startup set WoW Forever had (`/stdtest engine`).
    let fixture = std::fs::read_to_string(
        Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/client/forever-camelot-engine.lua"),
    )
    .expect("the fixture");
    let line = fixture
        .lines()
        .find(|l| l.contains("[\"probe.blizzardAtStart\"]"))
        .expect("the startup set");
    let list = line
        .split(" = \"")
        .nth(1)
        .expect("a value")
        .trim_end_matches("\",");
    let mut client: Vec<&str> = list.split(' ').collect();
    client.sort_unstable();
    let mut ours: Vec<&str> = report.addons.iter().map(String::as_str).collect();
    ours.sort_unstable();
    assert_eq!(ours, client, "the startup set");
    if let Ok(path) = std::env::var("WOWLUA_UI_ADDONS") {
        std::fs::write(path, report.addons.join("\n")).expect("writes");
    }
    assert!(!report.addons.is_empty());
}

/// A button's font object given by its name is that font: its text is
/// drawn at the font's size (it was a pixel high, without a font).
#[test]
fn a_buttons_font_object_by_name_draws_its_text() {
    let interface = tree(
        "button-font-name",
        &[
            ("AddOns/Blizzard_Box/Blizzard_Box.toc", "Box.xml\n"),
            (
                "AddOns/Blizzard_Box/Box.xml",
                r#"<Ui>
  <FontFamily name="FamFont" virtual="true">
    <Member alphabet="roman"><Font font="Fonts\FRIZQT__.ttf" height="15"/></Member>
  </FontFamily>
  <Button name="RowTemplate" virtual="true">
    <ButtonText parentKey="ButtonText" nonspacewrap="true" justifyH="LEFT"><Anchors><Anchor point="LEFT" x="8"/><Anchor point="RIGHT" x="-30"/></Anchors></ButtonText>
    <NormalTexture file="Interface\Buttons\WHITE8X8"/>
    <NormalFont style="FamFont"/>
  </Button>
</Ui>"#,
            ),
        ],
    );
    let mut vm = vm("camelot");
    let report = wowlua_engine::ui::load(&mut vm, &interface, None);
    assert!(report.errors.is_empty(), "{:?}", report.errors);
    let r = run(
        &mut vm,
        "local c = CreateFrame('Button', 'Plain', UIParent)
c:SetSize(260, 25)
c:SetPoint('CENTER')
c:SetNormalFontObject('FamFont')
c:SetHighlightFontObject('FamFont')
c:SetText('Plain Button')
local row = CreateFrame('Button', nil, UIParent, 'RowTemplate')
row:SetSize(200, 25)
row:SetPoint('TOP', c, 'BOTTOM')
row.ButtonText:SetText('Row')
return tostring(c:GetNormalFontObject() == FamFont), tostring(c:GetHighlightFontObject() == FamFont)",
    );
    assert_eq!(r, ["true", "true"]);
    let scene = wowlua_engine::scene::build(&mut vm);
    let px: Vec<f32> = scene
        .items
        .iter()
        .filter_map(|i| match i {
            wowlua_engine::scene::Item::Text(t) => Some(t.px),
            _ => None,
        })
        .collect();
    assert_eq!(px, [15.0, 15.0]);
    // The template's `ButtonText`, in no `<Layer>`, is drawn on `OVERLAY`:
    // over the button's normal texture (ARTWORK).
    let order: Vec<&str> = scene
        .items
        .iter()
        .filter_map(|i| match i {
            wowlua_engine::scene::Item::Text(_) => Some("text"),
            wowlua_engine::scene::Item::Quad(_) => Some("quad"),
            _ => None,
        })
        .collect();
    assert!(order.contains(&"quad"), "{order:?}");
    assert_eq!(order.last(), Some(&"text"), "{order:?}");
}

/// A texture's `<Color>` is its plain color without a file and its tint
/// with one (Blizzard's flat panel backgrounds); `clipChildren` clips.
#[test]
fn xml_texture_colors_and_clipping() {
    let interface = tree(
        "texture-color",
        &[
            ("AddOns/Blizzard_Box/Blizzard_Box.toc", "Box.xml\n"),
            (
                "AddOns/Blizzard_Box/Box.xml",
                r#"<Ui>
  <Frame name="ColorBox" parent="UIParent" clipChildren="true">
    <Size x="100" y="100"/>
    <Anchors><Anchor point="CENTER"/></Anchors>
    <Layers><Layer level="BACKGROUND">
      <Texture parentKey="Plain" setAllPoints="true"><Color r="0.5" g="0.25" b="0" a="0.8"/></Texture>
      <Texture parentKey="Tinted" file="Interface\Buttons\WHITE8X8" setAllPoints="true"><Color r="0" g="1" b="0"/></Texture>
    </Layer></Layers>
  </Frame>
</Ui>"#,
            ),
        ],
    );
    let mut vm = vm("camelot");
    let report = wowlua_engine::ui::load(&mut vm, &interface, None);
    assert!(report.errors.is_empty(), "{:?}", report.errors);
    let r = run(
        &mut vm,
        "local p = ColorBox.Plain
local r, g, b = ColorBox.Tinted:GetVertexColor()
return tostring(ColorBox:DoesClipChildren()), tostring(p:GetTexture() ~= nil or p:GetTextureFileID() ~= nil or true), string.format('%.2f %.2f %.2f', r, g, b)",
    );
    assert_eq!(r[0], "true");
    assert_eq!(r[2], "0.00 1.00 0.00");
    let scene = wowlua_engine::scene::build(&mut vm);
    let colors: Vec<[f32; 4]> = scene
        .items
        .iter()
        .filter_map(|i| match i {
            wowlua_engine::scene::Item::Quad(q)
                if matches!(q.image, wowlua_engine::scene::Image::Color) =>
            {
                Some(q.color)
            }
            _ => None,
        })
        .collect();
    assert_eq!(colors.len(), 1, "{colors:?}");
    assert!(
        (colors[0][0] - 0.5).abs() < 0.01 && (colors[0][3] - 0.8).abs() < 0.01,
        "{colors:?}"
    );
}

/// Within a layer and sublevel a frame's font strings are drawn after its
/// textures, whatever order they were made in (the Settings title over
/// its border's top edge).
#[test]
fn text_draws_over_textures_of_its_layer() {
    let mut vm = vm("camelot");
    run(
        &mut vm,
        "local f = CreateFrame('Frame', nil, UIParent)
f:SetSize(100, 20)
f:SetPoint('CENTER')
local t = f:CreateFontString(nil, 'OVERLAY')
t:SetFont('Fonts\\\\FRIZQT__.TTF', 12, '')
t:SetText('Title')
t:SetPoint('CENTER')
local edge = f:CreateTexture(nil, 'OVERLAY')
edge:SetAllPoints()
edge:SetColorTexture(1, 1, 1)",
    );
    let scene = wowlua_engine::scene::build(&mut vm);
    let order: Vec<&str> = scene
        .items
        .iter()
        .filter_map(|i| match i {
            wowlua_engine::scene::Item::Text(_) => Some("text"),
            wowlua_engine::scene::Item::Quad(_) => Some("quad"),
            _ => None,
        })
        .collect();
    let text = order.iter().position(|k| *k == "text").expect("the text");
    assert!(order[..text].contains(&"quad"), "{order:?}");
}

/// A frame on its parent's level (`useParentLevel`) is drawn with its
/// parent, their regions sorted together by layer and sublevel (the gossip
/// window's parchment over its inset's marble); an XML font keeps its
/// justification.
#[test]
fn parent_level_frames_interleave_and_fonts_keep_justification() {
    let interface = tree(
        "parent-level",
        &[
            ("AddOns/Blizzard_Box/Blizzard_Box.toc", "Box.xml\n"),
            (
                "AddOns/Blizzard_Box/Box.xml",
                r#"<Ui>
  <Font name="LeftFont" font="Fonts\FRIZQT__.TTF" height="12" justifyH="LEFT" virtual="true"/>
  <Frame name="Holder" parent="UIParent">
    <Size x="100" y="100"/>
    <Anchors><Anchor point="CENTER"/></Anchors>
    <Layers><Layer level="BACKGROUND">
      <Texture parentKey="Parchment" setAllPoints="true"><Color r="1" g="0" b="0"/></Texture>
    </Layer></Layers>
    <Frames>
      <Frame parentKey="Inset" useParentLevel="true" setAllPoints="true">
        <Layers><Layer level="BACKGROUND" textureSubLevel="-5">
          <Texture parentKey="Marble" setAllPoints="true"><Color r="0" g="0" b="1"/></Texture>
        </Layer></Layers>
      </Frame>
    </Frames>
  </Frame>
</Ui>"#,
            ),
        ],
    );
    let mut vm = vm("camelot");
    let report = wowlua_engine::ui::load(&mut vm, &interface, None);
    assert!(report.errors.is_empty(), "{:?}", report.errors);
    let r = run(
        &mut vm,
        "local t = UIParent:CreateFontString(nil, 'OVERLAY', 'LeftFont')
return t:GetJustifyH()",
    );
    assert_eq!(r, ["LEFT"]);
    let scene = wowlua_engine::scene::build(&mut vm);
    let colors: Vec<[f32; 4]> = scene
        .items
        .iter()
        .filter_map(|i| match i {
            wowlua_engine::scene::Item::Quad(q)
                if matches!(q.image, wowlua_engine::scene::Image::Color) =>
            {
                Some(q.color)
            }
            _ => None,
        })
        .collect();
    // The inset's blue (sublevel -5) first, the holder's red over it.
    assert_eq!(colors.len(), 2, "{colors:?}");
    assert_eq!((colors[0][2], colors[1][0]), (1.0, 1.0), "{colors:?}");
}

/// A texture inline in text (`|Tfile:h:w|t`) takes its width in the line
/// and is drawn where it stands.
#[test]
fn inline_textures_are_drawn_in_text() {
    let mut vm = vm("camelot");
    let r = run(
        &mut vm,
        "local t = UIParent:CreateFontString(nil, 'OVERLAY')
t:SetFont('Fonts\\\\FRIZQT__.TTF', 12, '')
t:SetPoint('CENTER')
t:SetText('a b')
local plain = t:GetStringWidth()
t:SetText('a |TInterface\\\\GossipFrame\\\\AvailableQuestIcon:16:16|t b')
return tostring(t:GetStringWidth() > plain + 10)",
    );
    assert_eq!(r, ["true"]);
    let scene = wowlua_engine::scene::build(&mut vm);
    assert!(scene.items.iter().any(|i| matches!(i,
        wowlua_engine::scene::Item::Quad(q) if q.image == wowlua_engine::scene::Image::File("Interface\\GossipFrame\\AvailableQuestIcon".into()))));
}

/// A texture's XML `<Gradient>` and `SetGradient` shade its color.
#[test]
fn texture_gradients() {
    let interface = tree(
        "gradient",
        &[
            ("AddOns/Blizzard_Box/Blizzard_Box.toc", "Box.xml\n"),
            (
                "AddOns/Blizzard_Box/Box.xml",
                r#"<Ui>
  <Frame name="GradBox" parent="UIParent">
    <Size x="100" y="100"/>
    <Anchors><Anchor point="CENTER"/></Anchors>
    <Layers><Layer level="BACKGROUND">
      <Texture parentKey="BG" setAllPoints="true">
        <Color r="1" g="1" b="1"/>
        <Gradient orientation="VERTICAL"><MinColor r="0.25" g="0.5" b="0"/><MaxColor r="0" g="0" b="0"/></Gradient>
      </Texture>
    </Layer></Layers>
  </Frame>
</Ui>"#,
            ),
        ],
    );
    let mut vm = vm("camelot");
    let report = wowlua_engine::ui::load(&mut vm, &interface, None);
    assert!(report.errors.is_empty(), "{:?}", report.errors);
    let scene = wowlua_engine::scene::build(&mut vm);
    let g = scene
        .items
        .iter()
        .find_map(|i| match i {
            wowlua_engine::scene::Item::Quad(q) => q.gradient,
            _ => None,
        })
        .expect("a gradient");
    assert!(
        !g.0 && (g.1[0] - 0.25).abs() < 0.01 && g.2[0] == 0.0,
        "{g:?}"
    );
}

/// An atlas inline in text (`|A:name:h:w|a`) is drawn where it stands.
#[test]
fn inline_atlases_are_drawn_in_text() {
    let mut vm = vm("camelot");
    run(
        &mut vm,
        "local t = UIParent:CreateFontString(nil, 'OVERLAY')
t:SetFont('Fonts\\\\FRIZQT__.TTF', 12, '')
t:SetPoint('CENTER')
t:SetSize(60, 10)
t:SetText('|A:friends-status-online:19:19:0:0|a')",
    );
    let scene = wowlua_engine::scene::build(&mut vm);
    let quad = scene.items.iter().find_map(|i| match i {
        wowlua_engine::scene::Item::Quad(q) => Some(q.rect),
        _ => None,
    });
    let q = quad.expect("the atlas drawn");
    assert!(q.w > 10.0 && q.h > 10.0, "{q:?}");
}

/// Text in a scaled frame is drawn at the frame's scale: fitted text fits
/// (the Settings list's "NEW" badges, `scale=".8"`).
#[test]
fn text_in_scaled_frames_fits() {
    let mut vm = vm("camelot");
    run(
        &mut vm,
        "local f = CreateFrame('Frame', nil, UIParent)
f:SetScale(0.8)
f:SetSize(1, 1)
f:SetPoint('CENTER')
local t = f:CreateFontString(nil, 'OVERLAY')
t:SetFont('Fonts\\\\FRIZQT__.TTF', 12, '')
t:SetPoint('CENTER')
t:SetTextToFit('NEW')",
    );
    let scene = wowlua_engine::scene::build(&mut vm);
    let lines: Vec<String> = scene
        .items
        .iter()
        .filter_map(|i| match i {
            wowlua_engine::scene::Item::Text(t) => Some(
                t.lines
                    .iter()
                    .flat_map(|l| l.runs.iter().map(|r| r.text.clone()))
                    .collect(),
            ),
            _ => None,
        })
        .collect();
    assert_eq!(lines, ["NEW"]);
}
