//! The 3D world from the WoW installation of this machine (decision T522);
//! skipped where there is none.

use wowlua_vm::{Value, Vm};

/// A camelot session with the client's data, at Northshire Abbey where a
/// new human starts.
fn session() -> Option<Vm> {
    session_and_casc().map(|(vm, _)| vm)
}

/// [`session`], and the client's storage it reads.
fn session_and_casc() -> Option<(Vm, std::sync::Arc<wowlua_casc::Casc>)> {
    let root = std::env::var_os("WOWLUA_WOW_PATH")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| "/Applications/World of Warcraft".into());
    let storage = wowlua_casc::FsStorage { root };
    let product = wowlua_casc::Casc::product_of_folder(&storage, "_forever_")?;
    let casc = std::sync::Arc::new(wowlua_casc::Casc::open(Box::new(storage), &product).ok()?);
    let for_data = casc.clone();
    let data = std::sync::Arc::new(wowlua_render::client::CascClientData::new(Box::new(
        move || Some(for_data.clone()),
    )));
    let mut vm = Vm::wow();
    wowlua_engine::install(&mut vm, "camelot").ok()?;
    wowlua_engine::controls::install(&mut vm, "mock");
    wowlua_engine::clientdata::set(&mut vm, data);
    Some((vm, casc))
}

fn run(vm: &mut Vm, src: &str) -> Vec<String> {
    let f = vm.load(src.as_bytes(), "@Test.lua").expect("compiles");
    let text = |vm: &Vm, v: Value| match vm.hidden(v) {
        Value::Str(s) => String::from_utf8_lossy(vm.str_bytes(s)).into_owned(),
        Value::Num(n) => format!("{}", (n * 100.0).round() / 100.0),
        other => format!("{other:?}"),
    };
    match vm.call(f, &[]) {
        Ok(r) => r.into_iter().map(|v| text(vm, v)).collect(),
        Err(e) => panic!("{}", text(vm, e.value)),
    }
}

/// With the world on, the player and the units stand on the ground; off,
/// where the test put them.
#[test]
fn the_player_and_units_stand_on_the_ground() {
    let Some(mut vm) = session() else {
        eprintln!("no camelot client: skipped");
        return;
    };
    let src = "mock.moveTo({ world = { y = -8949.95, x = -132.493, z = 0, instanceID = 0 } })
local wolf = mock.spawnUnit({ name = 'Wolf', reaction = 'hostile',
    world = { y = -8930, x = -132.493, z = 0 } })
mock.player:target(wolf)
local _, _, z = UnitPosition('player')
local _, _, wz = UnitPosition('target')
return z, wz";
    let off = run(&mut vm, src);
    assert_eq!(off, ["0", "0"]);
    wowlua_engine::set_world(&mut vm, true);
    let on = run(&mut vm, src);
    assert_eq!(on[0], "83.53");
    // (20 yards north, up the abbey's slope)
    let wolf: f64 = on[1].parse().expect("a height");
    assert!(wolf > 75.0 && wolf < 95.0 && wolf != 83.53, "{wolf}");
}

/// Northshire Abbey's middle (north, west) and how far a side reaches,
/// from the client's own placement.
fn abbey() -> Option<(f64, f64, f64)> {
    let root = std::env::var_os("WOWLUA_WOW_PATH")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| "/Applications/World of Warcraft".into());
    let storage = wowlua_casc::FsStorage { root };
    let product = wowlua_casc::Casc::product_of_folder(&storage, "_forever_")?;
    let casc = wowlua_casc::Casc::open(Box::new(storage), &product).ok()?;
    let mut lib = wowlua_casc::display::Library::default();
    let objects = lib.objects(&casc, 0, 32, 48)?;
    let from_start = |x: &wowlua_casc::obj::Building| (x.at[0] + 8949.95).hypot(x.at[1] + 132.493);
    let b = objects
        .buildings
        .iter()
        .filter(|b| b.bounds[1][0] - b.bounds[0][0] < 300.0)
        .min_by(|a, b| from_start(a).total_cmp(&from_start(b)))?;
    Some((
        (b.bounds[0][0] + b.bounds[1][0]) / 2.0,
        (b.bounds[0][1] + b.bounds[1][1]) / 2.0,
        (b.bounds[1][0] - b.bounds[0][0]).max(b.bounds[1][1] - b.bounds[0][1]),
    ))
}

/// With the world on, the abbey's walls stop the player running into it
/// and stand between them and a wolf on its other side: a spell at the
/// wolf is refused (decision T524).
#[test]
fn the_buildings_stop_the_player_and_hide_the_target() {
    let (Some(mut vm), Some((n, w, reach))) = (session(), abbey()) else {
        eprintln!("no camelot client: skipped");
        return;
    };
    wowlua_engine::set_world(&mut vm, true);
    let src = format!(
        "mock.moveTo({{ world = {{ y = {north}, x = {w}, z = 0, instanceID = 0 }}, facing = math.pi }})
mock.defineSpell(133, {{ name = 'Fireball', known = true, maxRange = 400 }})
Errors = {{}}
local f = CreateFrame('Frame')
f:RegisterEvent('UI_ERROR_MESSAGE')
f:SetScript('OnEvent', function(_, _, _, text) Errors[#Errors + 1] = text end)
local wolf = mock.spawnUnit({{ name = 'Wolf', reaction = 'hostile', world = {{ y = {south}, x = {w}, z = 0 }} }})
mock.player:target(wolf)
mock.player:useSpell(133)
MoveForwardStart()
mock.advanceFrames(600, 1000 / 60)
MoveForwardStop()
local y = UnitPosition('player')
return y, table.concat(Errors, ', ')",
        north = n + reach,
        south = n - reach,
    );
    let out = run(&mut vm, &src);
    let y: f64 = out[0].parse().expect("a place");
    // (ten seconds south, seventy yards: stopped at the wall, between where
    // it started and the abbey's middle)
    assert!(y < n + reach && y > n, "{y} of {n} + {reach}");
    assert_eq!(out[1], "Target not in line of sight");
}

/// The world is drawn at `RenderScale` of the screen's pixels each way and
/// stretched to it (decision T539): the whole screen covered, a half
/// scale's picture the full one's, coarser; the setting kept to 0.33 to 2.
#[test]
fn the_world_is_drawn_at_its_render_scale() {
    let Some((mut vm, casc)) = session_and_casc() else {
        eprintln!("no camelot client: skipped");
        return;
    };
    let Ok(mut r) = wowlua_render::Renderer::new_blocking() else {
        eprintln!("no GPU: skipped");
        return;
    };
    let mut assets = wowlua_render::client::ClientAssets::new(Some(casc), &[]);
    wowlua_engine::set_world(&mut vm, true);
    run(
        &mut vm,
        "mock.moveTo({ world = { y = -8949.95, x = -132.493, z = 0, instanceID = 0 }, facing = 0 })
mock.advanceFrames(2, 1000 / 60)",
    );
    let mut at = |vm: &mut Vm, scale: &str| {
        run(vm, &format!("C_CVar.SetCVar('RenderScale', '{scale}')"));
        let scene = wowlua_engine::scene::build(vm);
        let set = scene.world.as_ref().expect("the world").render_scale;
        let image = r
            .render_blocking(&scene, 1.0, None, &mut assets)
            .expect("drawn");
        (set, image)
    };
    // (the first draw reads the world)
    at(&mut vm, "1");
    let (_, full) = at(&mut vm, "1");
    let (set, half) = at(&mut vm, "0.5");
    assert_eq!(set, 0.5);
    assert_eq!((half.width, half.height), (full.width, full.height));
    let pixel = |i: &wowlua_casc::Rgba, x: u32, y: u32| {
        let at = ((y * i.width + x) * 4) as usize;
        [i.pixels[at], i.pixels[at + 1], i.pixels[at + 2]]
    };
    let (w, h) = (half.width - 1, half.height - 1);
    for (x, y) in [(0, 0), (w, 0), (0, h), (w, h), (w / 2, h / 2)] {
        assert_ne!(pixel(&half, x, y), [0, 0, 0], "{x}, {y} drawn");
    }
    let differ: Vec<u32> = full
        .pixels
        .iter()
        .zip(&half.pixels)
        .map(|(a, b)| u32::from(a.abs_diff(*b)))
        .collect();
    let mean = f64::from(differ.iter().sum::<u32>()) / differ.len() as f64;
    assert!(mean > 0.0 && mean < 12.0, "{mean}");
    assert_eq!(at(&mut vm, "0.1").0, 0.33);
    assert_eq!(at(&mut vm, "4").0, 2.0);
}

/// The world under the place's light at the realm's time of day (decision
/// T555): Northshire's abbey at noon and at midnight, its sky and ground
/// lit differently. `WOWLUA_SHOTS` names a folder to keep the pictures in.
#[test]
fn the_world_is_lit_by_the_time_of_day() {
    let Some((mut vm, casc)) = session_and_casc() else {
        eprintln!("no camelot client: skipped");
        return;
    };
    let Ok(mut r) = wowlua_render::Renderer::new_blocking() else {
        eprintln!("no GPU: skipped");
        return;
    };
    let mut assets = wowlua_render::client::ClientAssets::new(Some(casc), &[]);
    wowlua_engine::set_world(&mut vm, true);
    run(
        &mut vm,
        "mock.moveTo({ world = { y = -8949.95, x = -132.493, z = 0, instanceID = 0 }, facing = 0 })
mock.advanceFrames(2, 1000 / 60)",
    );
    let mut at = |vm: &mut Vm, hour: u32| {
        run(vm, &format!("mock.setTimeOfDay({hour}, 0)"));
        assert_eq!(
            run(vm, "return GetGameTime()"),
            [hour.to_string(), "0".into()]
        );
        let scene = wowlua_engine::scene::build(vm);
        r.render_blocking(&scene, 1.0, None, &mut assets)
            .expect("drawn")
    };
    // (the first draw reads the world)
    at(&mut vm, 12);
    let noon = at(&mut vm, 12);
    let night = at(&mut vm, 0);
    let dusk = at(&mut vm, 19);
    if let Some(dir) = std::env::var_os("WOWLUA_SHOTS") {
        let dir = std::path::PathBuf::from(dir);
        for (name, image) in [("noon", &noon), ("midnight", &night), ("dusk", &dusk)] {
            let png = wowlua_render::encode_png(image).expect("encoded");
            std::fs::write(dir.join(format!("northshire-{name}.png")), png).expect("written");
        }
    }
    let mean = |i: &wowlua_casc::Rgba| {
        i.pixels.iter().map(|&p| f64::from(p)).sum::<f64>() / i.pixels.len() as f64
    };
    let (day, dark) = (mean(&noon), mean(&night));
    eprintln!(
        "mean brightness: noon {day:.1}, midnight {dark:.1}, dusk {:.1}",
        mean(&dusk)
    );
    assert!((day - dark).abs() > 5.0, "{day} {dark}");
}

/// The light's sky models (decision T556): WoW Forever's Zephras Isle has
/// one in its global light (at the origin, 34 yards about it: a map's
/// global light whatever its radius), drawn behind the world.
#[test]
fn the_sky_shows_its_models() {
    let Some((mut vm, casc)) = session_and_casc() else {
        eprintln!("no camelot client: skipped");
        return;
    };
    let Ok(mut r) = wowlua_render::Renderer::new_blocking() else {
        eprintln!("no GPU: skipped");
        return;
    };
    let mut assets = wowlua_render::client::ClientAssets::new(Some(casc), &[]);
    let at = [3350.0, 750.0, 100.0];
    let sky = wowlua_render::Assets::light(&mut assets, 2991, at, 1440.0).expect("the light");
    eprintln!("Zephras Isle's sky models: {:?}", sky.skyboxes);
    assert!(!sky.skyboxes.is_empty());
    wowlua_engine::set_world(&mut vm, true);
    run(
        &mut vm,
        "mock.moveTo({ world = { y = 3350.0, x = 750.0, z = 0, instanceID = 2991 }, facing = 2 })
mock.advanceFrames(2, 1000 / 60)
MoveViewUpStart()
mock.advanceFrames(30, 1000 / 60)
MoveViewUpStop()",
    );
    let scene = wowlua_engine::scene::build(&mut vm);
    r.render_blocking(&scene, 1.0, None, &mut assets)
        .expect("drawn");
    let image = r
        .render_blocking(&scene, 1.0, None, &mut assets)
        .expect("drawn");
    if let Some(dir) = std::env::var_os("WOWLUA_SHOTS") {
        let png = wowlua_render::encode_png(&image).expect("encoded");
        std::fs::write(std::path::PathBuf::from(dir).join("zephras-sky.png"), png)
            .expect("written");
    }
}

/// The terrain's liquids (decision T557): the river of Northshire's tile
/// (its largest liquid), seen from 15 yards north of the middle of its
/// squares; the riverbed and the player standing in it show through.
#[test]
fn the_water_is_drawn() {
    let Some((mut vm, casc)) = session_and_casc() else {
        eprintln!("no camelot client: skipped");
        return;
    };
    let Ok(mut r) = wowlua_render::Renderer::new_blocking() else {
        eprintln!("no GPU: skipped");
        return;
    };
    // (the largest liquid of Northshire's tile, the middle of its squares)
    let mut lib = wowlua_casc::display::Library::default();
    let tile = lib
        .terrain_tile(&casc, 0, 32, 48)
        .expect("Northshire's tile");
    let l = tile
        .liquids
        .iter()
        .max_by_key(|l| l.exists.iter().filter(|e| **e).count())
        .expect("its water");
    let (north, west) = wowlua_casc::adt::corner(32, 48);
    let unit = wowlua_casc::adt::UNIT;
    let squares: Vec<(f64, f64)> = (0..usize::from(l.height))
        .flat_map(|j| (0..usize::from(l.width)).map(move |i| (i, j)))
        .filter(|&(i, j)| l.exists[j * usize::from(l.width) + i])
        .map(|(i, j)| (i as f64 + 0.5, j as f64 + 0.5))
        .collect();
    let count = squares.len() as f64;
    let (ci, cj) = squares
        .iter()
        .fold((0.0, 0.0), |a, s| (a.0 + s.0 / count, a.1 + s.1 / count));
    let n = north - f64::from(l.chunk.1) * wowlua_casc::adt::CHUNK - (f64::from(l.y) + cj) * unit;
    let w = west - f64::from(l.chunk.0) * wowlua_casc::adt::CHUNK - (f64::from(l.x) + ci) * unit;
    let mut assets = wowlua_render::client::ClientAssets::new(Some(casc), &[]);
    wowlua_engine::set_world(&mut vm, true);
    run(
        &mut vm,
        &format!(
            "mock.moveTo({{ world = {{ y = {}, x = {w}, z = 0, instanceID = 0 }}, facing = math.pi }})
mock.advanceFrames(2, 1000 / 60)
CameraZoomOut(20)",
            n + 15.0
        ),
    );
    // (WoW Forever's PBR river: drawn with its own colors and water's frames)
    let kinds = wowlua_render::Assets::liquids(&mut assets).expect("the liquids");
    let kind = kinds.resolve(l).expect("its type").kind;
    assert_eq!(kind.look, wowlua_casc::liquid::Look::Pbr, "{}", kind.name);
    assert!(kind.colors.len() >= 2);
    assert!(kinds.kind(1).is_some_and(|k| !k.frames.is_empty()));
    let scene = wowlua_engine::scene::build(&mut vm);
    r.render_blocking(&scene, 1.0, None, &mut assets)
        .expect("drawn");
    let image = r
        .render_blocking(&scene, 1.0, None, &mut assets)
        .expect("drawn");
    if let Some(dir) = std::env::var_os("WOWLUA_SHOTS") {
        let png = wowlua_render::encode_png(&image).expect("encoded");
        std::fs::write(
            std::path::PathBuf::from(dir).join("northshire-water.png"),
            png,
        )
        .expect("written");
    }
}

/// The props that move (decision T558): Northshire's bird and fountain
/// move as they stand, its trees do not (in the data); at the abbey the
/// world poses the moving ones near it one by one.
#[test]
fn the_props_move() {
    let Some((mut vm, casc)) = session_and_casc() else {
        eprintln!("no camelot client: skipped");
        return;
    };
    let moves = |file: u32| {
        casc.file_by_id(file)
            .and_then(|b| wowlua_casc::m2::Model::parse(&b))
            .map(|m| (m.name.clone(), m.moves()))
    };
    assert_eq!(moves(190719), Some(("Bird01".into(), true)));
    assert_eq!(moves(199949), Some(("FountainParticles".into(), true)));
    assert_eq!(moves(189932), Some(("ElwynnTreeMid01".into(), false)));
    let Ok(mut r) = wowlua_render::Renderer::new_blocking() else {
        eprintln!("no GPU: skipped");
        return;
    };
    let mut assets = wowlua_render::client::ClientAssets::new(Some(casc), &[]);
    wowlua_engine::set_world(&mut vm, true);
    run(
        &mut vm,
        "mock.moveTo({ world = { y = -8949.95, x = -132.493, z = 0, instanceID = 0 }, facing = 0 })
mock.advanceFrames(2, 1000 / 60)",
    );
    let scene = wowlua_engine::scene::build(&mut vm);
    let start = std::time::Instant::now();
    r.render_blocking(&scene, 1.0, None, &mut assets)
        .expect("drawn");
    r.render_blocking(&scene, 1.0, None, &mut assets)
        .expect("drawn");
    eprintln!(
        "{} props posed; two frames took {:?}",
        r.moving_props(),
        start.elapsed()
    );
    assert!(r.moving_props() > 0);
}

/// The world's units posed on the GPU look as posed on the CPU (decision
/// T540): Northshire's start among six NPCs, the same picture.
#[test]
fn the_gpu_poses_the_units_as_the_cpu_does() {
    let Some((mut vm, casc)) = session_and_casc() else {
        eprintln!("no camelot client: skipped");
        return;
    };
    wowlua_engine::set_world(&mut vm, true);
    run(
        &mut vm,
        "mock.moveTo({ world = { y = -8949.95, x = -132.493, z = 0, instanceID = 0 }, facing = 0 })
local displays = { 1261, 1276, 1277, 1288, 1440, 163 }
for i = 1, 6 do
    mock.spawnUnit({ name = 'Npc' .. i, reaction = 'friendly', displayID = displays[i],
        world = { y = -8949.95 + 5 + i, x = -132.493 - 5 + i * 1.6, z = 0 } })
end
mock.advanceFrames(2, 1000 / 60)",
    );
    let scene = wowlua_engine::scene::build(&mut vm);
    let draw = |cpu: bool| {
        let mut r = wowlua_render::Renderer::new_blocking().ok()?;
        if cpu {
            r.skin_on_the_cpu();
        }
        let mut assets = wowlua_render::client::ClientAssets::new(Some(casc.clone()), &[]);
        // (the first draw reads the world)
        r.render_blocking(&scene, 1.0, None, &mut assets).ok()?;
        r.render_blocking(&scene, 1.0, None, &mut assets).ok()
    };
    let (Some(gpu), Some(cpu)) = (draw(false), draw(true)) else {
        eprintln!("no GPU: skipped");
        return;
    };
    assert_eq!((gpu.width, gpu.height), (cpu.width, cpu.height));
    let differ: Vec<u8> = gpu
        .pixels
        .iter()
        .zip(&cpu.pixels)
        .map(|(a, b)| a.abs_diff(*b))
        .collect();
    let mean = differ.iter().map(|d| f64::from(*d)).sum::<f64>() / differ.len() as f64;
    let far = differ.iter().filter(|d| **d > 24).count();
    assert!(mean < 0.5, "{mean}");
    assert!(far * 1000 < differ.len(), "{far} of {}", differ.len());
}
