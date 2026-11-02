//! The 3D world's frame time at Northshire's abbey, feature by feature
//! (decisions T555 to T560): `cargo test --release -p wowlua-render --lib
//! bench -- --ignored --nocapture`, with WoW Forever installed. Each frame
//! drawn to a texture off screen, not read back: the CPU's time to record
//! it, and the whole time until the GPU has finished it.

use std::sync::Arc;
use std::time::{Duration, Instant};

use wowlua_engine::scene::ModelSource;
use wowlua_vm::{Value, Vm};

use crate::client::ClientAssets;
use crate::{Assets, AtlasRegion, Image, Renderer, Rgba};

/// The client's assets with the newer world features turned off at will.
struct Switched {
    inner: ClientAssets,
    light: bool,
    liquids: bool,
    clutter: bool,
}

impl Assets for Switched {
    fn image(&mut self, image: &Image) -> Option<Rgba> {
        self.inner.image(image)
    }
    fn font(&mut self, path: &str) -> Option<Vec<u8>> {
        self.inner.font(path)
    }
    fn atlas(&mut self, name: &str) -> Option<AtlasRegion> {
        self.inner.atlas(name)
    }
    fn file(&mut self, id: u32) -> Option<Vec<u8>> {
        self.inner.file(id)
    }
    fn model(&mut self, source: &ModelSource) -> Option<wowlua_casc::display::Display> {
        self.inner.model(source)
    }
    fn terrain(&mut self, map: u32, x: i32, y: i32) -> Option<Arc<wowlua_casc::adt::Tile>> {
        self.inner.terrain(map, x, y)
    }
    fn objects(&mut self, map: u32, x: i32, y: i32) -> Option<Arc<wowlua_casc::obj::Objects>> {
        self.inner.objects(map, x, y)
    }
    fn building(&mut self, file: u32) -> Option<Arc<wowlua_casc::display::Building>> {
        self.inner.building(file)
    }
    fn minimap_tile(&mut self, map: u32, x: i32, y: i32) -> Option<u32> {
        self.inner.minimap_tile(map, x, y)
    }
    fn light(&mut self, map: u32, at: [f64; 3], day_time: f64) -> Option<wowlua_casc::light::Sky> {
        if self.light {
            self.inner.light(map, at, day_time)
        } else {
            None
        }
    }
    fn liquids(&mut self) -> Option<Arc<wowlua_casc::liquid::Liquids>> {
        if self.liquids {
            self.inner.liquids()
        } else {
            None
        }
    }
    fn clutter(&mut self) -> Option<Arc<wowlua_casc::clutter::Clutter>> {
        if self.clutter {
            self.inner.clutter()
        } else {
            None
        }
    }
}

fn run(vm: &mut Vm, src: &str) {
    let f = vm.load(src.as_bytes(), "@Bench.lua").expect("compiles");
    if let Err(e) = vm.call(f, &[]) {
        let msg = match vm.hidden(e.value) {
            Value::Str(s) => String::from_utf8_lossy(vm.str_bytes(s)).into_owned(),
            other => format!("{other:?}"),
        };
        panic!("{msg}");
    }
}

#[test]
#[ignore = "a measurement: needs WoW Forever installed, best in release"]
fn bench() {
    let root = std::env::var_os("WOWLUA_WOW_PATH")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| "/Applications/World of Warcraft".into());
    let storage = wowlua_casc::FsStorage { root };
    let Some(product) = wowlua_casc::Casc::product_of_folder(&storage, "_forever_") else {
        eprintln!("no WoW Forever: skipped");
        return;
    };
    let casc = Arc::new(wowlua_casc::Casc::open(Box::new(storage), &product).expect("the client"));
    let for_data = casc.clone();
    let data = Arc::new(crate::client::CascClientData::new(Box::new(move || {
        Some(for_data.clone())
    })));
    let mut vm = Vm::wow();
    wowlua_engine::install(&mut vm, "camelot").expect("the engine");
    wowlua_engine::controls::install(&mut vm, "mock");
    wowlua_engine::clientdata::set(&mut vm, data);
    wowlua_engine::set_world(&mut vm, true);
    run(
        &mut vm,
        "mock.moveTo({ world = { y = -8949.95, x = -132.493, z = 0, instanceID = 0 }, facing = 0 })
mock.advanceFrames(2, 1000 / 60)",
    );
    let mut r = Renderer::new_blocking().expect("a GPU");
    let (width, height) = (1920, 1080);
    let target = r.device.create_texture(&wgpu::TextureDescriptor {
        label: Some("bench"),
        size: wgpu::Extent3d {
            width,
            height,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: crate::FORMAT,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
        view_formats: &[],
    });
    let view = target.create_view(&wgpu::TextureViewDescriptor::default());
    let configs = [
        ("none of the new", false, false, true, false),
        ("+ light and sky", true, false, true, false),
        ("+ water", true, true, true, false),
        ("+ moving props, particles", true, true, false, false),
        ("+ clutter (all)", true, true, false, true),
    ];
    let median = |mut v: Vec<Duration>| {
        v.sort();
        v[v.len() / 2]
    };
    for (name, light, liquids, still, clutter) in configs {
        let mut assets = Switched {
            inner: ClientAssets::new(Some(casc.clone()), &[]),
            light,
            liquids,
            clutter,
        };
        r.world.still_props = still;
        // (warm up: the world read, the textures uploaded)
        for _ in 0..3 {
            let scene = wowlua_engine::scene::build(&mut vm);
            let e = r.draw(&scene, &view, width, height, 1.0, &mut assets);
            r.queue.submit([e.finish()]);
            let _ = r.device.poll(wgpu::PollType::wait_indefinitely());
        }
        let (mut cpu, mut total) = (Vec::new(), Vec::new());
        for _ in 0..60 {
            run(&mut vm, "mock.advanceFrames(1, 1000 / 60)");
            let scene = wowlua_engine::scene::build(&mut vm);
            let start = Instant::now();
            let e = r.draw(&scene, &view, width, height, 1.0, &mut assets);
            r.queue.submit([e.finish()]);
            cpu.push(start.elapsed());
            let _ = r.device.poll(wgpu::PollType::wait_indefinitely());
            total.push(start.elapsed());
        }
        eprintln!(
            "{name:28} cpu {:>6.2} ms  total {:>6.2} ms  ({} props posed, {} particles, {} clutter)",
            median(cpu).as_secs_f64() * 1000.0,
            median(total).as_secs_f64() * 1000.0,
            r.moving_props(),
            r.world_particles(),
            r.world_clutter(),
        );
    }
}
