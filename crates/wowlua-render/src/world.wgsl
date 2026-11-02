// The 3D world's terrain (decision T522): each chunk's four texture
// layers blended by its blend map, lit by the zone's light (its ambient,
// and its direct light by how the ground faces the sun) and faded into the
// fog with distance; shining where its layers' alphas say (decision T579). Where its baked shadow (decision T563) falls, its
// light is less by the light's shadow; under the units, their blob
// shadows; where the sun's shadow maps shade it (T564), its direct light
// is gone.

struct Globals {
    view: mat4x4<f32>,
    proj: mat4x4<f32>,
    // The sun's direction (toward it).
    sun: vec4<f32>,
    // The fog's color, and where it starts (yards).
    fog: vec4<f32>,
    // The eye, and where the fog is whole (yards).
    eye: vec4<f32>,
    // The light's direct and ambient colors (decision T555).
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
    // The sun's shadow maps (decision T564): each map's matrix, where each
    // ends (yards ahead of the eye), a texel's size in yards in each; x
    // how many (0: none), y soft, z blended where they meet, w a texel
    // (0 to 1 across); the way the eye looks.
    shadow_mats: array<mat4x4<f32>, 4>,
    shadow_ends: vec4<f32>,
    shadow_texels: vec4<f32>,
    shadow: vec4<f32>,
    look: vec4<f32>,
    // The terrain's shine (decision T579): x on, y its exponent; z the
    // clock.
    terrain: vec4<f32>,
    // The sky's top and band colors (decision T583).
    sky_top: vec4<f32>,
    sky_band: vec4<f32>,
    // The light bounced from the sky and the ground (decision T585): w on.
    bounce_sky: vec4<f32>,
    bounce_ground: vec4<f32>,
};

@group(0) @binding(0) var<uniform> g: Globals;

// The ambient light a surface facing `n` takes (decision T585): with the
// bounce light on (`bounce_sky.w`), from the sky's color facing up to the
// ground's facing down; else the light's ambient.
fn ambient_at(n: vec3<f32>) -> vec3<f32> {
    if g.bounce_sky.w < 0.5 {
        return g.ambient.rgb;
    }
    return mix(g.bounce_ground.rgb, g.bounce_sky.rgb, clamp(n.z * 0.5 + 0.5, 0.0, 1.0));
}
@group(0) @binding(1) var blob: texture_2d<f32>;
@group(0) @binding(2) var blob_sampler: sampler;
@group(0) @binding(3) var shadow_map: texture_depth_2d_array;
@group(0) @binding(4) var shadow_compare: sampler_comparison;

// How much of the sun one shadow map lets reach a point facing `n` (0 to
// 1): looked up a texel and a half off its surface, against its own
// shadow; nine looks around it where soft.
fn sun_through(p: vec3<f32>, n: vec3<f32>, k: i32) -> f32 {
    let q = g.shadow_mats[k] * vec4<f32>(p + n * g.shadow_texels[k] * 1.5, 1.0);
    let uv = vec2<f32>(q.x * 0.5 + 0.5, 0.5 - q.y * 0.5);
    if any(uv < vec2<f32>(0.0)) || any(uv > vec2<f32>(1.0)) || q.z > 1.0 {
        return 1.0;
    }
    if g.shadow.y < 0.5 {
        return textureSampleCompareLevel(shadow_map, shadow_compare, uv, k, q.z);
    }
    var sum = 0.0;
    for (var y = -1; y <= 1; y++) {
        for (var x = -1; x <= 1; x++) {
            let at = uv + vec2<f32>(f32(x), f32(y)) * g.shadow.w;
            sum += textureSampleCompareLevel(shadow_map, shadow_compare, at, k, q.z);
        }
    }
    return sum / 9.0;
}

// How much of the sun reaches a point (decision T564): through the map
// its distance ahead of the eye falls in, blended into the next over the
// last tenth where they blend; fading out over the last map's last tenth.
fn sunlight(p: vec3<f32>, n: vec3<f32>) -> f32 {
    let count = i32(g.shadow.x);
    if count == 0 {
        return 1.0;
    }
    let ahead = dot(p - g.eye.xyz, g.look.xyz);
    let last = g.shadow_ends[count - 1];
    if ahead >= last {
        return 1.0;
    }
    var k = 0;
    for (var i = 0; i < count - 1; i++) {
        if ahead >= g.shadow_ends[i] {
            k = i + 1;
        }
    }
    var lit = sun_through(p, n, k);
    if g.shadow.z > 0.5 && k + 1 < count {
        let start = select(0.0, g.shadow_ends[max(k - 1, 0)], k > 0);
        let band = (g.shadow_ends[k] - start) * 0.1;
        let t = (ahead - (g.shadow_ends[k] - band)) / band;
        if t > 0.0 {
            lit = mix(lit, sun_through(p, n, k + 1), t);
        }
    }
    return mix(lit, 1.0, clamp((ahead - last * 0.9) / (last * 0.1), 0.0, 1.0));
}

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

@group(1) @binding(0) var layer0: texture_2d<f32>;
@group(1) @binding(1) var layer1: texture_2d<f32>;
@group(1) @binding(2) var layer2: texture_2d<f32>;
@group(1) @binding(3) var layer3: texture_2d<f32>;
@group(1) @binding(4) var blend: texture_2d<f32>;
@group(1) @binding(5) var tiled: sampler;
@group(1) @binding(6) var clamped: sampler;

struct VIn {
    @location(0) pos: vec3<f32>,
    @location(1) normal: vec3<f32>,
    // The chunk's own: 0 to 1 from its north-west corner, east and south.
    @location(2) uv: vec2<f32>,
};

struct VOut {
    @builtin(position) clip: vec4<f32>,
    @location(0) normal: vec3<f32>,
    @location(1) uv: vec2<f32>,
    @location(2) far: f32,
    @location(3) pos: vec3<f32>,
};

@vertex
fn vs(v: VIn) -> VOut {
    var o: VOut;
    o.clip = g.proj * g.view * vec4<f32>(v.pos, 1.0);
    o.normal = v.normal;
    o.uv = v.uv;
    o.far = distance(v.pos, g.eye.xyz);
    o.pos = v.pos;
    return o;
}

@fragment
fn fs(i: VOut) -> @location(0) vec4<f32> {
    // (each texture eight times across a chunk)
    let t = i.uv * 8.0;
    let a = textureSample(blend, clamped, i.uv);
    let s0 = textureSample(layer0, tiled, t);
    let s1 = textureSample(layer1, tiled, t);
    let s2 = textureSample(layer2, tiled, t);
    let s3 = textureSample(layer3, tiled, t);
    var c = mix(mix(mix(s0.rgb, s1.rgb, a.r), s2.rgb, a.g), s3.rgb, a.b);
    // (the layers' alphas: their shine, blended as their colors: T579)
    let shine = mix(mix(mix(s0.a, s1.a, a.r), s2.a, a.g), s3.a, a.b);
    let n = normalize(i.normal);
    let reach = sunlight(i.pos, n);
    let sun = max(dot(n, g.sun.xyz), 0.0) * reach;
    let shade = (1.0 - a.a * g.ambient.w) * blobs(i.pos);
    let lit = (ambient_at(n) + g.direct.rgb * sun) * shade;
    // The sun's shine off the ground where its layers' alphas say
    // (`specular`, T579): Blinn-Phong, in the direct light's color.
    let half_way = normalize(g.sun.xyz + normalize(g.eye.xyz - i.pos));
    let gloss = pow(max(dot(n, half_way), 0.0), g.terrain.y) * shine * g.terrain.x;
    let shone = c * lit + g.direct.rgb * gloss * reach * shade * step(0.0, dot(n, g.sun.xyz));
    let fog = clamp((i.far - g.fog.w) / max(g.eye.w - g.fog.w, 1.0), 0.0, 1.0);
    return vec4<f32>(mix(shone, g.fog.rgb, fog), 1.0);
}
