//! The Graphics settings (decision T561): a setting's level sets the CVars
//! it stands for, and the overall quality sets each setting's level, as
//! the client does.

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
        Value::Nil => "nil".into(),
        other => format!("{other:?}"),
    };
    match vm.call(f, &[]) {
        Ok(r) => r.into_iter().map(|v| text(vm, v)).collect(),
        Err(e) => panic!("{}", text(vm, e.value)),
    }
}

#[test]
fn a_shadow_quality_sets_its_shadows() {
    let mut vm = vm();
    let shadows = "return C_CVar.GetCVar('shadowMode'), C_CVar.GetCVar('shadowNumCascades'), C_CVar.GetCVar('shadowSoft')";
    run(&mut vm, "C_CVar.SetCVar('graphicsShadowQuality', 2)");
    assert_eq!(run(&mut vm, shadows), ["2", "3", "0"]);
    run(&mut vm, "C_CVar.SetCVar('graphicsShadowQuality', 0)");
    assert_eq!(run(&mut vm, shadows), ["0", "1", "0"]);
}

#[test]
fn the_overall_quality_sets_each_setting() {
    let mut vm = vm();
    // (as the client starts: the default quality 6, shadows at Ultra)
    assert_eq!(
        run(
            &mut vm,
            "return C_CVar.GetCVar('shadowMode'), C_CVar.GetCVar('shadowSoft')"
        ),
        ["3", "1"]
    );
    // (camelot's recorded levels: quality 8, shadows at Ultra High)
    run(&mut vm, "C_CVar.SetCVar('graphicsQuality', 8)");
    assert_eq!(
        run(
            &mut vm,
            "return C_CVar.GetCVar('graphicsShadowQuality'), C_CVar.GetCVar('shadowMode'), C_CVar.GetCVar('shadowSoft')"
        ),
        ["5", "4", "1"]
    );
    run(&mut vm, "C_CVar.SetCVar('graphicsQuality', 0)");
    assert_eq!(
        run(
            &mut vm,
            "return C_CVar.GetCVar('graphicsShadowQuality'), C_CVar.GetCVar('shadowMode')"
        ),
        ["0", "0"]
    );
}
