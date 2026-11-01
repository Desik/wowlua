//! `cargo run -p wowlua-casc --example lightdump -- <product> <map> <north> <west>`:
//! the lights at a place and their time keys (direct color, fog end).

use wowlua_casc::tables::Table;

fn main() {
    let a: Vec<String> = std::env::args().collect();
    let root = std::env::var_os("WOWLUA_WOW_PATH")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| "/Applications/World of Warcraft".into());
    let casc = wowlua_casc::Casc::open(Box::new(wowlua_casc::FsStorage { root }), &a[1])
        .expect("the product");
    if a[2] == "skyboxes" {
        let sky = Table::open(&casc, "LightSkybox").expect("LightSkybox");
        let params = Table::open(&casc, "LightParams").expect("LightParams");
        let light = Table::open(&casc, "Light").expect("Light");
        for s in sky.rows() {
            let users: Vec<u32> = params
                .rows()
                .filter(|p| p.int("LightSkyboxID") == s.id())
                .map(|p| p.id())
                .collect();
            let places: Vec<String> = light
                .rows()
                .filter(|l| l.ints("LightParamsID").iter().any(|p| users.contains(p)))
                .take(3)
                .map(|l| format!("map {} {:?}", l.int("ContinentID"), l.floats("GameCoords")))
                .collect();
            println!(
                "skybox {} {:?} flags {:#x} file {} params {} lights {:?}",
                s.id(),
                s.string("Name"),
                s.int("Flags"),
                s.int("SkyboxFileDataID"),
                users.len(),
                places
            );
        }
        return;
    }
    let map: u32 = a[2].parse().unwrap();
    let (n, w): (f32, f32) = (a[3].parse().unwrap(), a[4].parse().unwrap());
    let light = Table::open(&casc, "Light").expect("Light");
    let data = Table::open(&casc, "LightData").expect("LightData");
    for r in light.rows() {
        if r.int("ContinentID") != map {
            continue;
        }
        let c = r.floats("GameCoords");
        let d = ((c[0] - n).powi(2) + (c[1] - w).powi(2)).sqrt();
        let global = c.iter().all(|v| *v == 0.0);
        if !global && d > r.float("GameFalloffEnd") {
            continue;
        }
        let params = r.ints("LightParamsID");
        println!(
            "light {} at {:?} falloff {}..{} dist {d:.0} params {:?}",
            r.id(),
            c,
            r.float("GameFalloffStart"),
            r.float("GameFalloffEnd"),
            params
        );
        if let Some(lp) = Table::open(&casc, "LightParams") {
            let boxes: Vec<u32> = params
                .iter()
                .map(|p| lp.row(*p).map_or(0, |r| r.int("LightSkyboxID")))
                .collect();
            println!("  skyboxes by slot {boxes:?}");
        }
        let mut keys: Vec<_> = data.rows_where("LightParamID", params[0]);
        keys.sort_by_key(|k| k.int("Time"));
        for k in keys {
            println!(
                "  time {:>4} direct {:06X} ambient {:06X} skytop {:06X} fog {:06X} fogend {} scaler {}",
                k.int("Time"),
                k.int("DirectColor"),
                k.int("AmbientColor"),
                k.int("SkyTopColor"),
                k.int("SkyFogColor"),
                k.float("FogEnd"),
                k.float("FogScaler")
            );
            if std::env::var_os("FOG").is_some() {
                println!(
                    "    main fog {}..{} density {} coeffs {:?} height coeffs {:?} density coeffs {:?} endfogdist {} startoffset {}",
                    k.float("MainFogStartDist"),
                    k.float("MainFogEndDist"),
                    k.float("FogDensity"),
                    k.floats("MainFogCoefficients"),
                    k.floats("FogHeightCoefficients"),
                    k.floats("HeightDensityFogCoeff"),
                    k.float("EndFogColorDistance"),
                    k.float("FogStartOffset")
                );
            }
            if std::env::var_os("RAW").is_some() {
                let names = [
                    "LightParamID",
                    "Time",
                    "DirectColor",
                    "AmbientColor",
                    "SkyTopColor",
                    "SkyMiddleColor",
                    "SkyBand1Color",
                    "SkyBand2Color",
                    "SkySmogColor",
                    "SkyFogColor",
                    "SunColor",
                    "CloudSunColor",
                ];
                let raw: Vec<String> = names
                    .iter()
                    .map(|n| format!("{n}={:X}", k.int(n)))
                    .collect();
                println!("    {}", raw.join(" "));
            }
        }
    }
}
