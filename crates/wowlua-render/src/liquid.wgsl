// The 3D world's liquids (decision T557): a terrain liquid's surface, its
// type's animated texture; water lit by the light and colored by its
// river's or sea's colors from near (shallow) to far (deep), seen through
// by its alpha; magma and slime glowing, opaque; faded into the fog.

struct Globals {
    view: mat4x4<f32>,
    proj: mat4x4<f32>,
    sun: vec4<f32>,
    fog: vec4<f32>,
    eye: vec4<f32>,
    direct: vec4<f32>,
    ambient: vec4<f32>,
    // Rivers' and lakes' colors near and far, the sea's (alpha: shallow,
    // deep).
    river: vec4<f32>,
    river_far: vec4<f32>,
    ocean: vec4<f32>,
    ocean_far: vec4<f32>,
};

@group(0) @binding(0) var<uniform> g: Globals;

@group(1) @binding(0) var frame: texture_2d<f32>;
@group(1) @binding(1) var tiled: sampler;
struct How {
    // x: glowing (1) or not; y: the sea (1) or not; z: its own colors (1).
    flags: vec4<f32>,
    near: vec4<f32>,
    far: vec4<f32>,
};

@group(2) @binding(0) var<uniform> how: How;

struct VIn {
    @location(0) pos: vec3<f32>,
    @location(1) uv: vec2<f32>,
    // 0 shallow to 1 deep.
    @location(2) depth: f32,
};

struct VOut {
    @builtin(position) clip: vec4<f32>,
    @location(0) uv: vec2<f32>,
    @location(1) depth: f32,
    @location(2) far: f32,
};

@vertex
fn vs(v: VIn) -> VOut {
    var o: VOut;
    o.clip = g.proj * g.view * vec4<f32>(v.pos, 1.0);
    o.uv = v.uv;
    o.depth = v.depth;
    o.far = distance(v.pos, g.eye.xyz);
    return o;
}

@fragment
fn fs(i: VOut) -> @location(0) vec4<f32> {
    let t = textureSample(frame, tiled, i.uv);
    let fog = clamp((i.far - g.fog.w) / max(g.eye.w - g.fog.w, 1.0), 0.0, 1.0);
    if how.flags.x > 0.5 {
        return vec4<f32>(mix(t.rgb, g.fog.rgb, fog), 1.0);
    }
    var near = g.river;
    var deep = g.river_far;
    if how.flags.y > 0.5 {
        near = g.ocean;
        deep = g.ocean_far;
    }
    if how.flags.z > 0.5 {
        near = vec4<f32>(how.near.rgb, near.a);
        deep = vec4<f32>(how.far.rgb, deep.a);
    }
    let d = clamp(i.depth, 0.0, 1.0);
    let color = mix(near.rgb, deep.rgb, d);
    let light = g.ambient.rgb + g.direct.rgb * max(g.sun.z, 0.0);
    let rgb = mix(color, t.rgb * light, 0.35);
    var alpha = mix(near.a, deep.a, d);
    if alpha <= 0.0 {
        alpha = mix(0.5, 0.9, d);
    }
    return vec4<f32>(mix(rgb, g.fog.rgb, fog), mix(alpha, 1.0, fog));
}
