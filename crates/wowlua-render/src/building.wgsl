// The 3D world's buildings (decision T523): a WMO group's batches, each
// with its material's texture, placed by the building's matrix; outdoors
// lit by the zone's light, indoors by the group's own colors; the units'
// blob shadows on its floors (decision T563); faded into the fog.

struct Globals {
    view: mat4x4<f32>,
    proj: mat4x4<f32>,
    sun: vec4<f32>,
    fog: vec4<f32>,
    eye: vec4<f32>,
    direct: vec4<f32>,
    // (w: how much of the light a shadow takes, decision T563)
    ambient: vec4<f32>,
    river: vec4<f32>,
    river_far: vec4<f32>,
    ocean: vec4<f32>,
    ocean_far: vec4<f32>,
    // The units' blob shadows (T563): where each stands and its radius;
    // `sun.w` how many.
    blobs: array<vec4<f32>, 16>,
};

@group(0) @binding(0) var<uniform> g: Globals;
@group(0) @binding(1) var blob: texture_2d<f32>;
@group(0) @binding(2) var blob_sampler: sampler;

// The units' blob shadows over a point (decision T563): the game's
// texture's grays multiplied in under each unit's feet, on what lies from
// two yards below them to one above.
fn blobs(p: vec3<f32>) -> f32 {
    var shade = 1.0;
    let n = u32(g.sun.w);
    for (var k = 0u; k < n; k++) {
        let b = g.blobs[k];
        let dz = p.z - b.z;
        let uv = (p.xy - b.xy) / (2.0 * b.w) + 0.5;
        if dz < -2.0 || dz > 1.0 || any(uv < vec2<f32>(0.0)) || any(uv > vec2<f32>(1.0)) {
            continue;
        }
        let t = textureSampleLevel(blob, blob_sampler, uv, 0.0);
        shade *= mix(1.0, t.r, t.a);
    }
    return shade;
}

@group(1) @binding(0) var diffuse: texture_2d<f32>;
@group(1) @binding(1) var tiled: sampler;
// x: alpha keyed (1) or not; y: indoors (1) or not; z: unlit (1) or not.
@group(1) @binding(2) var<uniform> how: vec4<f32>;

struct VIn {
    @location(0) pos: vec3<f32>,
    @location(1) normal: vec3<f32>,
    @location(2) uv: vec2<f32>,
    @location(3) color: vec4<f32>,
    // The placement's matrix, by columns.
    @location(4) m0: vec4<f32>,
    @location(5) m1: vec4<f32>,
    @location(6) m2: vec4<f32>,
    @location(7) m3: vec4<f32>,
};

struct VOut {
    @builtin(position) clip: vec4<f32>,
    @location(0) normal: vec3<f32>,
    @location(1) uv: vec2<f32>,
    @location(2) color: vec4<f32>,
    @location(3) far: f32,
    @location(4) world: vec3<f32>,
};

@vertex
fn vs(v: VIn) -> VOut {
    let m = mat4x4<f32>(v.m0, v.m1, v.m2, v.m3);
    let world = m * vec4<f32>(v.pos, 1.0);
    var o: VOut;
    o.clip = g.proj * g.view * world;
    o.normal = (m * vec4<f32>(v.normal, 0.0)).xyz;
    o.uv = v.uv;
    o.color = v.color;
    o.far = distance(world.xyz, g.eye.xyz);
    o.world = world.xyz;
    return o;
}

@fragment
fn fs(i: VOut) -> @location(0) vec4<f32> {
    let t = textureSample(diffuse, tiled, i.uv);
    if how.x > 0.5 && t.a < 0.5 {
        discard;
    }
    var light = g.ambient.rgb + g.direct.rgb * max(dot(normalize(i.normal), g.sun.xyz), 0.0);
    if how.y > 0.5 {
        light = i.color.rgb * 2.0;
    }
    if how.z > 0.5 {
        light = vec3<f32>(1.0);
    }
    light *= blobs(i.world);
    let fog = clamp((i.far - g.fog.w) / max(g.eye.w - g.fog.w, 1.0), 0.0, 1.0);
    return vec4<f32>(mix(t.rgb * light, g.fog.rgb, fog), t.a);
}
