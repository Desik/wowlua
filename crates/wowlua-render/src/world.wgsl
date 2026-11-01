// The 3D world's terrain (decision T522): each chunk's four texture
// layers blended by its blend map, lit by the zone's light (its ambient,
// and its direct light by how the ground faces the sun) and faded into the
// fog with distance.

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
    ambient: vec4<f32>,
};

@group(0) @binding(0) var<uniform> g: Globals;

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
};

@vertex
fn vs(v: VIn) -> VOut {
    var o: VOut;
    o.clip = g.proj * g.view * vec4<f32>(v.pos, 1.0);
    o.normal = v.normal;
    o.uv = v.uv;
    o.far = distance(v.pos, g.eye.xyz);
    return o;
}

@fragment
fn fs(i: VOut) -> @location(0) vec4<f32> {
    // (each texture eight times across a chunk)
    let t = i.uv * 8.0;
    let a = textureSample(blend, clamped, i.uv);
    var c = textureSample(layer0, tiled, t).rgb;
    c = mix(c, textureSample(layer1, tiled, t).rgb, a.r);
    c = mix(c, textureSample(layer2, tiled, t).rgb, a.g);
    c = mix(c, textureSample(layer3, tiled, t).rgb, a.b);
    let lit = g.ambient.rgb + g.direct.rgb * max(dot(normalize(i.normal), g.sun.xyz), 0.0);
    let fog = clamp((i.far - g.fog.w) / max(g.eye.w - g.fog.w, 1.0), 0.0, 1.0);
    return vec4<f32>(mix(c * lit, g.fog.rgb, fog), 1.0);
}
