//! The weather (decision T565): a test sets it, `C_Weather.
//! GetCurrentWeather` answers it, and the 3D world's view carries it.

use wowlua_vm::{Value, Vm};

fn vm() -> Vm {
    let mut vm = Vm::wow();
    wowlua_engine::install(&mut vm, "camelot").expect("installs");
    wowlua_engine::controls::install(&mut vm, "mock");
    vm
}

fn run(vm: &mut Vm, src: &str) -> Result<Vec<String>, String> {
    let f = vm.load(src.as_bytes(), "@Test.lua").expect("compiles");
    let text = |vm: &Vm, v: Value| match vm.hidden(v) {
        Value::Str(s) => String::from_utf8_lossy(vm.str_bytes(s)).into_owned(),
        Value::Nil => "nil".into(),
        Value::Num(n) => format!("{n}"),
        other => format!("{other:?}"),
    };
    match vm.call(f, &[]) {
        Ok(r) => Ok(r.into_iter().map(|v| text(vm, v)).collect()),
        Err(e) => Err(text(vm, e.value)),
    }
}

#[test]
fn a_test_sets_the_weather_the_client_answers() {
    let mut vm = vm();
    let now = "local w = C_Weather.GetCurrentWeather() return w.type, w.intensity";
    assert_eq!(run(&mut vm, now).unwrap(), ["0", "0"]);
    run(&mut vm, "mock.setWeather('snow', 0.5)").unwrap();
    assert_eq!(run(&mut vm, now).unwrap(), ["2", "0.5"]);
    run(&mut vm, "mock.setWeather('rain')").unwrap();
    assert_eq!(run(&mut vm, now).unwrap(), ["1", "1"]);
    run(&mut vm, "mock.setWeather('sandstorm', 7)").unwrap();
    assert_eq!(run(&mut vm, now).unwrap(), ["3", "1"]);
    run(&mut vm, "mock.setWeather('clear', 0.8)").unwrap();
    assert_eq!(run(&mut vm, now).unwrap(), ["0", "0"]);
    let err = run(&mut vm, "mock.setWeather('hail')").unwrap_err();
    assert!(err.contains("no weather `hail`"), "{err}");
    assert_eq!(
        run(
            &mut vm,
            "return Enum.WeatherType.Snow, Enum.WeatherType.Sandstorm"
        )
        .unwrap(),
        ["2", "3"]
    );
}
