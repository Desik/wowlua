//! `cargo run -p wowlua-render --example model -- <client folder> <display ID> <out.png> [portrait zoom] [facing] [animation]`:
//! a creature display (or `file:<id>`, `char:<race>:<sex>:<class>[:<items>]`)
//! drawn as a model frame shows it.

use wowlua_engine::scene::{Actor, Item, Light, Model, ModelCamera, ModelSource, Rect, Scene};

fn main() {
    let a: Vec<String> = std::env::args().collect();
    let install = std::env::var_os("WOWLUA_WOW_PATH")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| "/Applications/World of Warcraft".into());
    let casc = wowlua_render::client::open_client(&install, &[a[1].clone()]).expect("the client");
    let mut assets = wowlua_render::client::ClientAssets::new(Some(casc), &[]);
    let arg = |i: usize, d: f32| a.get(i).and_then(|v| v.parse().ok()).unwrap_or(d);
    // `file:<id>`, `char:<race>:<sex>:<class>[:<item>,<item>…]`, or a
    // display ID.
    let source = if let Some(f) = a[2].strip_prefix("file:") {
        ModelSource::File(f.parse().unwrap())
    } else if let Some(c) = a[2].strip_prefix("item:") {
        ModelSource::Item(c.parse().unwrap())
    } else if let Some(c) = a[2].strip_prefix("creature:") {
        ModelSource::Creature(c.parse().unwrap())
    } else if let Some(c) = a[2].strip_prefix("char:") {
        let p: Vec<&str> = c.split(':').collect();
        ModelSource::Character(wowlua_engine::scene::Look {
            race: p[0].parse().unwrap(),
            sex: p[1].parse().unwrap(),
            class: p[2].parse().unwrap(),
            items: Vec::new(),
            tried: p
                .get(3)
                .map(|i| i.split(',').map(|i| format!("item:{i}")).collect())
                .unwrap_or_default(),
        })
    } else {
        ModelSource::Display(a[2].parse().unwrap())
    };
    let scene = Scene {
        over_units: Vec::new(),
        width: 400,
        height: 500,
        items: vec![Item::Model(Model {
            widget: 0,
            rect: Rect {
                x: 0.0,
                y: 0.0,
                w: 400.0,
                h: 500.0,
            },
            clip: None,
            alpha: 1.0,
            actors: vec![Actor {
                source,
                position: [0.0; 3],
                yaw: arg(5, 0.0),
                pitch: 0.0,
                roll: 0.0,
                fixed_axes: false,
                scale: 1.0,
                alpha: 1.0,
                animation: arg(6, 0.0) as u32,
                variation: 0,
                placement: None,
                phase: 0,
            }],
            camera: ModelCamera::Frame {
                portrait_zoom: arg(4, 0.0),
                distance_scale: 1.0,
                camera: None,
            },
            light: Light {
                enabled: true,
                direction: [-1.0, 0.3, -0.6],
                ambient: [0.55; 3],
                diffuse: [0.75; 3],
            },
            time: 0.0,
        })],
        world: None,
    };
    let mut r = wowlua_render::Renderer::new_blocking().expect("a GPU");
    let t = std::time::Instant::now();
    let img = r
        .render_blocking(&scene, 1.0, None, &mut assets)
        .expect("drawn");
    println!("drawn in {:?}", t.elapsed());
    std::fs::write(&a[3], wowlua_render::encode_png(&img).unwrap()).unwrap();
}
