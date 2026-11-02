// WoW Forever's PBR water (decision T583): its waves from two of its
// churning foam masks scrolled across each other, the sky reflected as it
// grazes (Fresnel), the sun's glint, its body from its type's near color to
// its far one as it deepens over the ground, its shore's foam where it is
// shallow and its river's along its flow; faded into the fog. Unverified:
// the game's own shader is not in its data.

struct Globals {
    view: mat4x4<f32>,
    proj: mat4x4<f32>,
    sun: vec4<f32>,
    fog: vec4<f32>,
    eye: vec4<f32>,
    direct: vec4<f32>,
    ambient: vec4<f32>,
    river: vec4<f32>,
    river_far: vec4<f32>,
    ocean: vec4<f32>,
    ocean_far: vec4<f32>,
    blobs: array<vec4<f32>, 16>,
    shadow_mats: array<mat4x4<f32>, 4>,
    shadow_ends: vec4<f32>,
    shadow_texels: vec4<f32>,
    shadow: vec4<f32>,
    look: vec4<f32>,
    // z: the clock (seconds).
    terrain: vec4<f32>,
    // The sky's top and band colors.
    sky_top: vec4<f32>,
    sky_band: vec4<f32>,
};

@group(0) @binding(0) var<uniform> g: Globals;

// Its masks (their alpha): two churning, the shore's two, the river's.
@group(1) @binding(0) var churn_a: texture_2d<f32>;
@group(1) @binding(1) var churn_b: texture_2d<f32>;
@group(1) @binding(2) var shore_a: texture_2d<f32>;
@group(1) @binding(3) var shore_b: texture_2d<f32>;
@group(1) @binding(4) var streaks: texture_2d<f32>;
@group(1) @binding(5) var tiled: sampler;

struct How {
    flags: vec4<f32>,
    near: vec4<f32>,
    far: vec4<f32>,
};

@group(2) @binding(0) var<uniform> how: How;

struct VIn {
    @location(0) pos: vec3<f32>,
    @location(1) uv: vec2<f32>,
    @location(2) depth: f32,
    // Yards of water over the ground (large where unknown).
    @location(3) over: f32,
    // Its flow: its way (north, west) by its speed.
    @location(4) flow: vec2<f32>,
};

struct VOut {
    @builtin(position) clip: vec4<f32>,
    @location(0) uv: vec2<f32>,
    @location(1) over: f32,
    @location(2) flow: vec2<f32>,
    @location(3) world: vec3<f32>,
};

@vertex
fn vs(v: VIn) -> VOut {
    var o: VOut;
    o.clip = g.proj * g.view * vec4<f32>(v.pos, 1.0);
    o.uv = v.uv;
    o.over = v.over;
    o.flow = v.flow;
    o.world = v.pos;
    return o;
}

// The waves' height at a point (0 to 1): the two churning masks, each its
// own size, drifting their own ways and with the flow.
fn waves(uv: vec2<f32>, drift: vec2<f32>, t: f32) -> f32 {
    let a = textureSample(churn_a, tiled, uv * 1.3 + vec2<f32>(0.021, 0.012) * t + drift * 1.3).a;
    let b = textureSample(churn_b, tiled, uv * 2.1 + vec2<f32>(-0.016, 0.024) * t + drift * 2.1).a;
    return a * 0.55 + b * 0.45;
}

@fragment
fn fs(i: VOut) -> @location(0) vec4<f32> {
    let t = g.terrain.z;
    // (uv: east and south, half a square's yards apart; the flow north
    // and west)
    let drift = vec2<f32>(-i.flow.y, -i.flow.x) * t * 0.12;
    let e = 0.004;
    let h0 = waves(i.uv, drift, t);
    let hu = waves(i.uv + vec2<f32>(e, 0.0), drift, t);
    let hv = waves(i.uv + vec2<f32>(0.0, e), drift, t);
    // (the slope toward north is less south, toward west less east)
    let strength = 0.025 / e;
    let n = normalize(vec3<f32>((hv - h0) * strength, (hu - h0) * strength, 1.0));
    let to_eye = normalize(g.eye.xyz - i.world);
    let facing = max(dot(n, to_eye), 0.0);
    let fresnel = 0.02 + 0.98 * pow(1.0 - facing, 5.0);
    let r = reflect(-to_eye, n);
    let sky = mix(g.sky_band.rgb, g.sky_top.rgb, clamp(r.z * 2.0, 0.0, 1.0));
    let light = g.ambient.rgb + g.direct.rgb * max(g.sun.z, 0.0);
    let deep = clamp(i.over / 5.0, 0.0, 1.0);
    // (the darker of its two colors the deep one: which is which
    // unverified)
    let luma = vec3<f32>(0.3, 0.59, 0.11);
    var shallow_color = how.near.rgb;
    var deep_color = how.far.rgb;
    if dot(shallow_color, luma) < dot(deep_color, luma) {
        shallow_color = how.far.rgb;
        deep_color = how.near.rgb;
    }
    let body = mix(shallow_color, deep_color, deep) * light;
    var rgb = mix(body, sky, fresnel);
    // The sun's glint on the waves.
    let half_way = normalize(g.sun.xyz + to_eye);
    let glint = pow(max(dot(n, half_way), 0.0), 300.0) * step(0.0, g.sun.z);
    rgb += g.direct.rgb * glint * 3.0;
    // Foam: the shore's where it is shallow (fewer of its cells as it
    // deepens), the river's along its flow, a little of the churn's.
    let shore = max(
        textureSample(shore_a, tiled, i.uv * 0.7 + vec2<f32>(0.004, -0.003) * t).a,
        textureSample(shore_b, tiled, i.uv * 1.1 + vec2<f32>(-0.003, 0.002) * t).a,
    );
    let edge = clamp(i.over / 0.8, 0.0, 1.0);
    let shore_foam = smoothstep(edge, edge + 0.2, shore) * (1.0 - edge) * 0.85;
    let speed = length(i.flow);
    let river_foam = textureSample(streaks, tiled, i.uv * 0.6 + drift * 2.0).a
        * clamp(speed, 0.0, 1.0) * 0.6;
    let churn_foam = smoothstep(0.7, 1.0, h0) * 0.25;
    let foam = clamp(max(max(shore_foam, river_foam), churn_foam), 0.0, 1.0);
    rgb = mix(rgb, vec3<f32>(0.92, 0.95, 0.97) * light, foam);
    var alpha = mix(0.45, 0.9, deep);
    alpha = max(max(alpha, fresnel), foam);
    let far = distance(i.world, g.eye.xyz);
    let fog = clamp((far - g.fog.w) / max(g.eye.w - g.fog.w, 1.0), 0.0, 1.0);
    return vec4<f32>(mix(rgb, g.fog.rgb, fog), mix(alpha, 1.0, fog));
}
