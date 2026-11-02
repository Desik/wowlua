// The world drawn at RenderScale, stretched to the screen (decision T539):
// one triangle over the whole screen; its edges smoothed on the way as the
// game's image-based anti-aliasing says (`ffxAntiAliasingMode`, decision
// T578): FXAA, Low with a short search, High (and in place of CMAA and
// CMAA2) with a longer one.

@group(0) @binding(0) var scaled: texture_2d<f32>;
@group(0) @binding(1) var stretched: sampler;
// x, y: a texel of the scaled picture; z: the kind (0 none, 1 Low, 2 and
// up High).
@group(0) @binding(2) var<uniform> how: vec4<f32>;

struct Out {
    @builtin(position) position: vec4<f32>,
    @location(0) uv: vec2<f32>,
};

@vertex
fn vs(@builtin(vertex_index) i: u32) -> Out {
    let uv = vec2<f32>(f32((i << 1u) & 2u), f32(i & 2u));
    var out: Out;
    out.position = vec4<f32>(uv.x * 2.0 - 1.0, 1.0 - uv.y * 2.0, 0.0, 1.0);
    out.uv = uv;
    return out;
}

fn at(uv: vec2<f32>) -> vec3<f32> {
    return textureSampleLevel(scaled, stretched, uv, 0.0).rgb;
}

fn luma(c: vec3<f32>) -> f32 {
    return dot(c, vec3<f32>(0.299, 0.587, 0.114));
}

// FXAA: the edge's direction from the four diagonal neighbours' brightness,
// a blur along it, kept only where it stays within the neighbours' range.
fn fxaa(uv: vec2<f32>, texel: vec2<f32>, span: f32) -> vec3<f32> {
    let nw = luma(at(uv + vec2<f32>(-1.0, -1.0) * texel));
    let ne = luma(at(uv + vec2<f32>(1.0, -1.0) * texel));
    let sw = luma(at(uv + vec2<f32>(-1.0, 1.0) * texel));
    let se = luma(at(uv + vec2<f32>(1.0, 1.0) * texel));
    let m = at(uv);
    let lm = luma(m);
    let lo = min(lm, min(min(nw, ne), min(sw, se)));
    let hi = max(lm, max(max(nw, ne), max(sw, se)));
    // (not an edge: too little contrast, a texture's own detail kept)
    if hi - lo < max(1.0 / 24.0, hi * select(0.125, 0.166, span < 6.0)) {
        return m;
    }
    var dir = vec2<f32>(-((nw + ne) - (sw + se)), (nw + sw) - (ne + se));
    let reduce = max((nw + ne + sw + se) * 0.25 / 8.0, 1.0 / 128.0);
    let scale = 1.0 / (min(abs(dir.x), abs(dir.y)) + reduce);
    dir = clamp(dir * scale, vec2<f32>(-span), vec2<f32>(span)) * texel;
    let a = 0.5 * (at(uv + dir * (1.0 / 3.0 - 0.5)) + at(uv + dir * (2.0 / 3.0 - 0.5)));
    let b = a * 0.5 + 0.25 * (at(uv + dir * -0.5) + at(uv + dir * 0.5));
    let lb = luma(b);
    if lb < lo || lb > hi {
        return a;
    }
    return b;
}

@fragment
fn fs(in: Out) -> @location(0) vec4<f32> {
    if how.z < 0.5 {
        return textureSample(scaled, stretched, in.uv);
    }
    let span = select(8.0, 4.0, how.z < 1.5);
    return vec4<f32>(fxaa(in.uv, how.xy, span), 1.0);
}
