// The 3D world's sky (decision T555): the light's six colors from the top
// down, by how high the eye looks (90°, 30°, 15°, 5°, 0°, -30°), behind
// everything.

struct Sky {
    // Top, middle, band 1, band 2, smog, fog.
    colors: array<vec4<f32>, 6>,
    // The camera: forward; right with tan(fov x / 2); up with tan(fov y / 2).
    forward: vec4<f32>,
    right: vec4<f32>,
    up: vec4<f32>,
};

@group(0) @binding(0) var<uniform> s: Sky;

struct VOut {
    @builtin(position) clip: vec4<f32>,
    @location(0) ndc: vec2<f32>,
};

@vertex
fn vs(@builtin(vertex_index) i: u32) -> VOut {
    // one triangle over the screen
    let p = vec2<f32>(f32((i << 1u) & 2u) * 2.0 - 1.0, f32(i & 2u) * 2.0 - 1.0);
    var o: VOut;
    o.clip = vec4<f32>(p, 1.0, 1.0);
    o.ndc = p;
    return o;
}

@fragment
fn fs(i: VOut) -> @location(0) vec4<f32> {
    let ray = normalize(
        s.forward.xyz + s.right.xyz * (i.ndc.x * s.right.w) + s.up.xyz * (i.ndc.y * s.up.w),
    );
    let deg = degrees(asin(clamp(ray.z, -1.0, 1.0)));
    let stops = array<f32, 6>(90.0, 30.0, 15.0, 5.0, 0.0, -30.0);
    var c = s.colors[5].rgb;
    for (var k = 0; k < 5; k = k + 1) {
        if (deg <= stops[k] && deg >= stops[k + 1]) {
            let f = (stops[k] - deg) / (stops[k] - stops[k + 1]);
            c = mix(s.colors[k].rgb, s.colors[k + 1].rgb, f);
        }
    }
    return vec4<f32>(c, 1.0);
}
