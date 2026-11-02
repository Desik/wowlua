// The 3D world's ambient occlusion (decision T584), as the game's SSAO
// settings say: each pixel of a half-sized target looks at the depth around
// it within `assaoRadius` yards and darkens by how much of its open
// hemisphere is closed (the Alchemy estimate: its strength
// `assaoShadowMult`, its power `assaoShadowPower`, at most
// `assaoShadowClamp`), fading out from `assaoFadeOutFrom` to
// `assaoFadeOutTo` yards; then laid over the world's colors, softened.
// `DEPTH_TYPE` is the depth's texture type (multisampled with MSAA).

struct Ao {
    // The projection's x and y scales, its depth's two terms.
    proj: vec4<f32>,
    // radius (yards), strength, power, clamp.
    how: vec4<f32>,
    // fade from, to (yards), samples, the depth's sample count.
    more: vec4<f32>,
    // The depth's width and height, the target's.
    size: vec4<f32>,
};

@group(0) @binding(0) var<uniform> a: Ao;
@group(0) @binding(1) var depth: DEPTH_TYPE;

struct VOut {
    @builtin(position) clip: vec4<f32>,
    @location(0) uv: vec2<f32>,
};

@vertex
fn vs(@builtin(vertex_index) i: u32) -> VOut {
    // one triangle over the target
    let p = vec2<f32>(f32((i << 1u) & 2u) * 2.0 - 1.0, f32(i & 2u) * 2.0 - 1.0);
    var o: VOut;
    o.clip = vec4<f32>(p, 0.0, 1.0);
    o.uv = vec2<f32>(p.x * 0.5 + 0.5, 0.5 - p.y * 0.5);
    return o;
}

// The view's point at a place of the depth (0 to 1 across), none (z 0)
// where nothing is drawn.
fn at(uv: vec2<f32>) -> vec3<f32> {
    return at_pixel(vec2<i32>(floor(uv * a.size.xy)));
}

// The view's point at a pixel of the depth.
fn at_pixel(pixel: vec2<i32>) -> vec3<f32> {
    let px = clamp(pixel, vec2<i32>(0), vec2<i32>(a.size.xy) - 1);
    let d = textureLoad(depth, px, 0);
    if d >= 1.0 {
        return vec3<f32>(0.0);
    }
    // (depth = (A z + B) / -z; the point at its pixel's middle, where its
    // depth is)
    let z = -a.proj.w / (d + a.proj.z);
    let c = (vec2<f32>(px) + 0.5) / a.size.xy;
    let ndc = vec2<f32>(c.x * 2.0 - 1.0, 1.0 - c.y * 2.0);
    return vec3<f32>(ndc.x * -z / a.proj.x, ndc.y * -z / a.proj.y, z);
}

@fragment
fn occlusion(i: VOut) -> @location(0) vec4<f32> {
    // (its depth's pixel: the target's twice over)
    let pixel = vec2<i32>(floor(i.clip.xy)) * 2;
    let p = at_pixel(pixel);
    if p.z == 0.0 {
        return vec4<f32>(1.0);
    }
    // Its surface's way from its neighbors, each way the nearer one's (no
    // edge between them); facing the eye where they tell none.
    let l = at_pixel(pixel - vec2<i32>(1, 0));
    let r = at_pixel(pixel + vec2<i32>(1, 0));
    let u = at_pixel(pixel - vec2<i32>(0, 1));
    let d = at_pixel(pixel + vec2<i32>(0, 1));
    let dx = select(r - p, p - l, abs(l.z - p.z) < abs(r.z - p.z));
    let dy = select(d - p, p - u, abs(u.z - p.z) < abs(d.z - p.z));
    let across = cross(dx, dy);
    var n = -normalize(p);
    if dot(across, across) > 1e-12 {
        n = normalize(across);
    }
    if dot(n, p) > 0.0 {
        n = -n;
    }
    let far = length(p);
    let fade = 1.0 - smoothstep(a.more.x, max(a.more.y, a.more.x + 1.0), far);
    if fade <= 0.0 {
        return vec4<f32>(1.0);
    }
    // The radius on the screen (0 to 1 across, by height).
    let reach = a.how.x * a.proj.y * 0.5 / -p.z;
    let count = i32(a.more.z);
    // (each pixel's pattern turned its own way)
    // (interleaved gradient noise: no pattern of its own at any place)
    let spin = fract(52.9829189 * fract(dot(i.clip.xy, vec2<f32>(0.06711056, 0.00583715)))) * 6.2831853;
    var closed = 0.0;
    for (var k = 0; k < count; k++) {
        let f = (f32(k) + 0.5) / f32(count);
        let angle = f32(k) * 2.3999632 + spin;
        let r = reach * sqrt(f);
        let q = at(i.uv + vec2<f32>(cos(angle) * r * a.size.y / a.size.x, sin(angle) * r));
        if q.z == 0.0 {
            continue;
        }
        let v = q - p;
        let near = dot(v, v);
        // (nothing past the radius counts)
        if near > a.how.x * a.how.x {
            continue;
        }
        // (surfaces within a few hundredths of a yard of it, as two
        // drawn on each other, do not count)
        closed += max(dot(v, n) - 0.01 * -p.z - 0.06, 0.0) / (near + 0.01);
    }
    var open = max(1.0 - 2.0 * a.how.y / f32(count) * closed, 0.0);
    open = pow(open, a.how.z);
    open = max(open, 1.0 - a.how.w);
    return vec4<f32>(vec4<f32>(mix(1.0, open, fade)));
}

@group(1) @binding(0) var ao_map: texture_2d<f32>;
@group(1) @binding(1) var ao_sampler: sampler;

// Laid over the world: the target softened over a few of its texels,
// multiplying the colors.
@fragment
fn apply(i: VOut) -> @location(0) vec4<f32> {
    let texel = 1.0 / a.size.zw;
    var sum = 0.0;
    for (var y = -1; y <= 1; y++) {
        for (var x = -1; x <= 1; x++) {
            sum += textureSample(ao_map, ao_sampler, i.uv + vec2<f32>(f32(x), f32(y)) * texel).r;
        }
    }
    let ao = sum / 9.0;
    return vec4<f32>(ao, ao, ao, 1.0);
}
