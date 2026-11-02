//! A point of the world on the screen as the world pass draws it (decision
//! T546): what a view's overlays are drawn at.

use wowlua_engine::scene::WorldView;
use wowlua_render::project;

/// A camera at the origin looking north (north, west, up yards), seeing
/// 90 degrees across.
fn north() -> WorldView {
    WorldView {
        map: 0,
        eye: [0.0, 0.0, 0.0],
        look: [1.0, 0.0, 0.0],
        up: [0.0, 0.0, 1.0],
        fov: 90f64.to_radians(),
        far: 1000.0,
        detail: 500.0,
        objects: 500.0,
        cull: 18.0,
        render_scale: 1.0,
        units: Vec::new(),
        time: 0.0,
        day_time: 1440.0,
        clutter_dist: 0.0,
        clutter_density: 0.0,
        shadows: Default::default(),
        weather: (0, 0.0),
        weather_density: 2.0,
        msaa: 1,
        post_aa: 0,
        filtering: 5,
        specular: true,
        terrain_mip: 0,
        horizon: 1000.0,
        spells: Vec::new(),
    }
}

#[test]
fn a_point_of_the_world_shows_where_the_camera_sees_it() {
    let w = north();
    let near = |a: [f32; 2], b: [f32; 2]| (a[0] - b[0]).abs() < 0.5 && (a[1] - b[1]).abs() < 0.5;
    // straight ahead: the middle
    let ahead = project(&w, 1000, 500, [10.0, 0.0, 0.0]).expect("ahead");
    assert!(near(ahead, [500.0, 250.0]), "{ahead:?}");
    // 45 degrees to the west, looking north: the left edge
    let west = project(&w, 1000, 500, [10.0, 10.0, 0.0]).expect("west");
    assert!(near(west, [0.0, 250.0]), "{west:?}");
    // above the middle: higher on the screen
    let up = project(&w, 1000, 500, [10.0, 0.0, 2.0]).expect("up");
    assert!(up[1] < 250.0 && (up[0] - 500.0).abs() < 0.5, "{up:?}");
    // behind the eye: nowhere
    assert!(project(&w, 1000, 500, [-10.0, 0.0, 0.0]).is_none());
}
