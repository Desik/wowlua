//! The addon running in the page (spec M14 S3): built into memory and run,
//! alone and after Blizzard's UI the page fetched (here from
//! `target/types-src`, as `cargo xtask types fetch` leaves it), with input
//! and output. One test: the files' host is the program's.

#![allow(clippy::disallowed_methods)] // (the UI code comes from the disk)

use serde_json::{Value as Json, json};
use std::path::Path;
use wowlua_web::Playground;
use wowlua_web::run::Options;

const MAIN: &str = r#"local frame = CreateFrame("Frame")
frame:RegisterEvent("PLAYER_LOGIN")
frame:SetScript("OnEvent", function()
    print("logged in")
end)

local button = CreateFrame("Button", "PlaygroundButton", UIParent)
button:SetSize(100, 40)
button:SetPoint("CENTER")
button:SetScript("OnClick", function()
    print("clicked")
end)

local elapsed = 0
local ticker = CreateFrame("Frame")
ticker:SetScript("OnUpdate", function(_, dt)
    elapsed = elapsed + dt
    if elapsed >= 0.5 then
        elapsed = -1000
        print("half a second")
    end
end)
"#;

fn texts(output: &[Json]) -> Vec<String> {
    output
        .iter()
        .map(|o| o["text"].as_str().unwrap_or_default().to_string())
        .collect()
}

fn run_and_play(p: &mut Playground, blizzard_ui: bool) {
    let started = p.run(Options {
        flavor: "camelot".into(),
        blizzard_ui,
        profile: None,
        debug: None,
        installation: None,
    });
    assert_eq!(started["ok"], true, "{started}");
    let game = p.game().expect("running");
    let mut seen = texts(&game.take_output());
    assert!(seen.contains(&"logged in".to_string()), "{seen:?}");
    // half a second of frames, in steps a page's animation frames take
    let mut now = 0.0;
    while now <= 600.0 {
        seen.extend(texts(&game.advance(now).unwrap()));
        now += 16.7;
    }
    assert!(seen.contains(&"half a second".to_string()), "{seen:?}");
    // a click in the middle of the screen
    let (w, h) = game.screen();
    game.input(&json!({ "action": "click", "x": w as f64 / 2.0, "y": h as f64 / 2.0 }))
        .unwrap();
    let after = texts(&game.take_output());
    assert!(after.contains(&"clicked".to_string()), "{after:?}");
}

#[test]
fn the_addon_runs_alone_and_after_blizzard_s_ui() {
    let mut p = Playground::new();
    p.write_file("src/main.wowlua", MAIN);
    run_and_play(&mut p, false);

    // a load error is the run's
    p.write_file("src/main.wowlua", "error(\"broken\")\n");
    let started = p.run(Options {
        flavor: "camelot".into(),
        blizzard_ui: false,
        profile: None,
        debug: None,
        installation: None,
    });
    let said = started.to_string();
    assert!(said.contains("broken"), "{said}");

    // Blizzard's UI, as the page fetches it
    let ui = wowlua_web::run::blizzard_ui("camelot").expect("camelot's list");
    let source = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/types-src");
    // (the page fetches wow-ui-source's copy: where `fetch` put an installed
    // client's in its place (T553), the copy a test run fetched)
    let code = if source.join("camelot/.wowlua-installed").exists() {
        let (commit, _) = wowlua_test::ui_commits("camelot").expect("the commit");
        wowlua_driver::cache_root()
            .map(|c| c.join(format!("blizzard-ui/camelot-{}", &commit[..12])))
            .filter(|d| d.join(".wowlua-complete").exists())
    } else {
        Some(source.join("camelot"))
    };
    let Some(code) = code.filter(|d| d.join("Interface").exists()) else {
        eprintln!("skipped Blizzard's UI: no UI code (run `cargo xtask types fetch`)");
        return;
    };
    let started = p.run(Options {
        flavor: "camelot".into(),
        blizzard_ui: true,
        profile: None,
        debug: None,
        installation: None,
    });
    assert!(
        started["error"]
            .as_str()
            .unwrap()
            .contains("loadBlizzardUI"),
        "{started}"
    );
    let root = ui["root"].as_str().unwrap();
    for path in ui["startup"].as_array().unwrap() {
        let path = path.as_str().unwrap();
        if let Ok(bytes) = std::fs::read(code.join(path)) {
            p.write_host_file(&format!("{root}/{path}"), &bytes);
        }
    }
    let strings = &ui["strings"];
    let name = Path::new(strings["path"].as_str().unwrap())
        .file_name()
        .unwrap();
    let bytes = std::fs::read(source.join("strings").join(name)).unwrap();
    p.write_host_file(strings["path"].as_str().unwrap(), &bytes);
    p.write_file("src/main.wowlua", MAIN);
    run_and_play(&mut p, true);
    let game = p.game().unwrap();
    // Blizzard's frames are there
    let tree = wowlua_debug::ui::frame_tree(game.vm(), Some("GameTooltip"), true);
    assert!(tree.is_ok(), "{tree:?}");
}
