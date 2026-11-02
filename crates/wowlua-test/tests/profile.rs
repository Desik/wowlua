//! World profile parts are applied through the engine's controls.

use wowlua_vm::{Value, Vm};

#[test]
fn renown_is_applied_by_faction_and_optional() {
    let mut vm = Vm::wow();
    wowlua_engine::install(&mut vm, "camelot").expect("installs");
    let controls = wowlua_engine::controls::table(&mut vm);
    for profile in [
        r#"{"renown":{"2802":16,"2507":0}}"#,
        "{}",
        r#"{"renown":null}"#,
    ] {
        wowlua_test::profile::apply_text(&mut vm, controls, profile, "renown test")
            .expect("the profile applies");
        let f = vm
            .load(
                b"return C_MajorFactions.GetCurrentRenownLevel(2802), C_MajorFactions.GetCurrentRenownLevel(2507)",
                "=renown test",
            )
            .expect("compiles");
        let result = vm.call(f, &[]).expect("renown is readable");
        assert!(matches!(
            result.as_slice(),
            [Value::Num(16.0), Value::Num(0.0)]
        ));
    }
}
