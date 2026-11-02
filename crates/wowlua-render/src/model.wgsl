// A model's batches: up to three textures combined as the client's M2
// shaders do (`combiners.rs` numbers them), lit by the frame's light, into
// a transparent target (colors premultiplied by alpha).

struct Globals {
    view_proj: mat4x4<f32>,
    view: mat4x4<f32>,
    // The direction the light shines in.
    light_dir: vec4<f32>,
    ambient: vec4<f32>,
    diffuse: vec4<f32>,
    // The sun's shadow maps the world's units take (decision T564): as
    // the world's (`world.wgsl`), and the eye.
    shadow_mats: array<mat4x4<f32>, 4>,
    shadow_ends: vec4<f32>,
    shadow_texels: vec4<f32>,
    shadow: vec4<f32>,
    look: vec4<f32>,
    eye: vec4<f32>,
    // The world's light bounced from the sky and the ground (decision
    // T585): w on.
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
@group(0) @binding(1) var shadow_map: texture_depth_2d_array;
@group(0) @binding(2) var shadow_compare: sampler_comparison;

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

@group(1) @binding(0) var tex1: texture_2d<f32>;
@group(1) @binding(1) var tex2: texture_2d<f32>;
@group(1) @binding(2) var tex3: texture_2d<f32>;
@group(1) @binding(3) var samp: sampler;

// A batch the GPU poses (decision T540): where its part stands, its two
// texture transforms, its color, and its flags, shaders, first bone and
// bone count.
struct Record {
    world: mat4x4<f32>,
    uv1: mat4x4<f32>,
    uv2: mat4x4<f32>,
    color: vec4<f32>,
    info: vec4<u32>,
};

@group(2) @binding(0) var<storage, read> records: array<Record>;
@group(2) @binding(1) var<storage, read> bones: array<mat4x4<f32>>;

struct In {
    @location(0) pos: vec3<f32>,
    @location(1) normal: vec3<f32>,
    // The first texture's coordinates (transformed), the second's, and the
    // second set as it is.
    @location(2) t1: vec2<f32>,
    @location(3) t2: vec2<f32>,
    @location(4) raw2: vec2<f32>,
    @location(5) color: vec4<f32>,
    // 1: unlit, 2: alpha key, 4: opaque; the blend mode at bit 8.
    @location(6) flags: u32,
    // The pixel shader, and the vertex shader at bit 8.
    @location(7) shaders: u32,
};

struct Out {
    @builtin(position) pos: vec4<f32>,
    @location(0) normal: vec3<f32>,
    @location(1) uv1: vec2<f32>,
    @location(2) uv2: vec2<f32>,
    @location(3) uv3: vec2<f32>,
    @location(4) color: vec4<f32>,
    @location(5) @interpolate(flat) flags: u32,
    @location(6) @interpolate(flat) ps: u32,
    @location(7) fade: f32,
    // Where it is (the world's: its sun's shadows, T564).
    @location(8) world: vec3<f32>,
};

// A model file's vertex as it is: posed by its bones (up to four, their
// weights) on the GPU.
struct Skinned {
    @location(0) pos: vec3<f32>,
    @location(1) normal: vec3<f32>,
    @location(2) uv: vec2<f32>,
    @location(3) uv2: vec2<f32>,
    @location(4) bones: vec4<u32>,
    @location(5) weights: vec4<f32>,
};

@vertex
fn vs(v: In) -> Out {
    return shade(v);
}

// The vertex posed by its bones and placed as its batch's record says (the
// record is the draw's instance), then as `vs` (decision T540).
@vertex
fn vs_skinned(v: Skinned, @builtin(instance_index) i: u32) -> Out {
    let r = records[i];
    var p = vec3<f32>(0.0);
    var n = vec3<f32>(0.0);
    var total = 0.0;
    for (var k = 0u; k < 4u; k++) {
        let w = v.weights[k];
        if (w > 0.0 && v.bones[k] < r.info.w) {
            let m = bones[r.info.z + v.bones[k]];
            p += (m * vec4<f32>(v.pos, 1.0)).xyz * w;
            n += (m * vec4<f32>(v.normal, 0.0)).xyz * w;
            total += w;
        }
    }
    if (total == 0.0) {
        p = v.pos;
        n = v.normal;
    }
    var o: In;
    o.pos = (r.world * vec4<f32>(p, 1.0)).xyz;
    o.normal = normalize((r.world * vec4<f32>(n, 0.0)).xyz);
    o.t1 = (r.uv1 * vec4<f32>(v.uv, 0.0, 1.0)).xy;
    o.t2 = (r.uv2 * vec4<f32>(v.uv2, 0.0, 1.0)).xy;
    o.raw2 = v.uv2;
    o.color = r.color;
    o.flags = r.info.x;
    o.shaders = r.info.y;
    return shade(o);
}

fn shade(v: In) -> Out {
    var o: Out;
    o.pos = g.view_proj * vec4<f32>(v.pos, 1.0);
    o.world = v.pos;
    o.normal = v.normal;
    o.color = v.color;
    o.flags = v.flags;
    o.ps = v.shaders & 255u;
    // A sphere map of the view's reflection.
    let view_pos = (g.view * vec4<f32>(v.pos, 1.0)).xyz;
    let n = normalize((g.view * vec4<f32>(v.normal, 0.0)).xyz);
    let r = reflect(normalize(view_pos), n);
    let m = 2.0 * sqrt(r.x * r.x + r.y * r.y + (r.z + 1.0) * (r.z + 1.0));
    let env = vec2<f32>(r.x / m + 0.5, -r.y / m + 0.5);
    let edge = clamp(abs(dot(n, normalize(-view_pos))), 0.0, 1.0);
    o.uv1 = v.t1;
    o.uv2 = vec2<f32>(0.0);
    o.uv3 = vec2<f32>(0.0);
    o.fade = 1.0;
    switch (v.shaders >> 8u) {
        case 1u: { o.uv1 = env; }
        case 2u: { o.uv2 = v.t2; }
        case 3u: { o.uv2 = env; }
        case 4u: { o.uv1 = env; o.uv2 = v.t1; }
        case 5u: { o.uv1 = env; o.uv2 = env; }
        case 6u: { o.uv2 = env; o.uv3 = v.t1; }
        case 7u: { o.uv2 = v.t1; }
        case 8u: { o.uv2 = v.t1; o.uv3 = v.t1; }
        case 9u: { o.fade = edge; }
        case 10u: { o.uv1 = v.t2; }
        case 11u: { o.uv2 = env; o.uv3 = v.t2; }
        case 12u: { o.fade = edge; o.uv2 = v.t2; }
        case 13u: { o.fade = edge; o.uv1 = env; }
        case 14u: { o.uv2 = v.t2; o.uv3 = v.t1; }
        case 15u: { o.uv2 = v.t2; o.uv3 = v.raw2; }
        case 16u: { o.uv1 = v.t2; o.uv3 = v.raw2; }
        default: {}
    }
    return o;
}

@fragment
fn fs(i: Out) -> @location(0) vec4<f32> {
    var uv2 = i.uv2;
    var uv3 = i.uv3;
    if (i.ps == 26u || i.ps == 27u || i.ps == 28u) {
        uv2 = i.uv1;
        uv3 = i.uv1;
    }
    let a = textureSample(tex1, samp, i.uv1);
    let b = textureSample(tex2, samp, uv2);
    let c = textureSample(tex3, samp, uv3);
    let tint = i.color.rgb;
    var diffuse = tint * a.rgb;
    var specular = vec3<f32>(0.0);
    var alpha = 1.0;
    var can_discard = false;
    switch (i.ps) {
        case 1u, 33u: { alpha = a.a; can_discard = true; }
        case 2u: { diffuse = tint * a.rgb * b.rgb; alpha = b.a; can_discard = true; }
        case 3u: { diffuse = tint * a.rgb * b.rgb * 2.0; alpha = b.a * 2.0; can_discard = true; }
        case 4u: { diffuse = tint * a.rgb * b.rgb * 2.0; }
        case 5u: { diffuse = tint * a.rgb * b.rgb; }
        case 6u, 36u: { diffuse = tint * a.rgb * b.rgb; alpha = a.a * b.a; can_discard = true; }
        case 7u: { diffuse = tint * a.rgb * b.rgb * 2.0; alpha = a.a * b.a * 2.0; can_discard = true; }
        case 8u: { alpha = a.a + b.a; can_discard = true; specular = b.rgb; }
        case 9u: { diffuse = tint * a.rgb * b.rgb * 2.0; alpha = a.a; can_discard = true; }
        case 10u: { alpha = a.a; can_discard = true; specular = b.rgb; }
        case 11u: { diffuse = tint * a.rgb * b.rgb; alpha = a.a; can_discard = true; }
        case 12u: { diffuse = tint * mix(a.rgb * b.rgb * 2.0, a.rgb, vec3<f32>(a.a)); }
        case 13u, 20u: { specular = b.rgb * b.a; }
        case 14u: { specular = b.rgb * b.a * (1.0 - a.a); }
        case 15u: {
            diffuse = tint * mix(a.rgb * b.rgb * 2.0, a.rgb, vec3<f32>(a.a));
            specular = c.rgb * c.a;
        }
        case 16u, 23u: { alpha = a.a; can_discard = true; specular = b.rgb * b.a; }
        case 17u: {
            alpha = a.a + b.a * (0.3 * b.r + 0.59 * b.g + 0.11 * b.b);
            can_discard = true;
            specular = b.rgb * b.a * (1.0 - a.a);
        }
        case 18u: { diffuse = tint * mix(mix(a.rgb, b.rgb, vec3<f32>(b.a)), a.rgb, vec3<f32>(a.a)); }
        case 19u: { diffuse = tint * mix(a.rgb * b.rgb * 2.0, c.rgb, vec3<f32>(c.a)); }
        case 21u: { alpha = a.a + b.a; can_discard = true; specular = b.rgb * (1.0 - a.a); }
        case 22u: { diffuse = tint * mix(a.rgb * b.rgb, a.rgb, vec3<f32>(a.a)); }
        case 24u: { diffuse = tint * mix(a.rgb, b.rgb, vec3<f32>(b.a)); specular = a.rgb * a.a; }
        case 25u: {
            let glow = clamp(c.a, 0.0, 1.0);
            diffuse = tint * mix(a.rgb * b.rgb * 2.0, a.rgb, vec3<f32>(a.a)) * (1.0 - glow);
            specular = c.rgb * glow;
        }
        case 26u: { diffuse = tint * a.rgb; alpha = a.a; can_discard = true; }
        case 27u: {
            diffuse = tint * mix(mix(a.rgb * b.rgb * 2.0, c.rgb, vec3<f32>(c.a)), a.rgb, vec3<f32>(a.a));
        }
        case 28u: { diffuse = tint * a.rgb; alpha = a.a; can_discard = true; }
        case 29u: { diffuse = tint * mix(a.rgb, b.rgb, vec3<f32>(b.a)); }
        case 30u, 32u: {
            diffuse = tint * mix(a.rgb * mix(vec3<f32>(1.0), b.rgb, vec3<f32>(b.a)), c.rgb, vec3<f32>(c.a));
            if (i.ps == 30u) { alpha = a.a; can_discard = true; }
        }
        case 31u: {
            diffuse = tint * a.rgb * mix(vec3<f32>(1.0), b.rgb, vec3<f32>(b.a));
            alpha = a.a;
            can_discard = true;
        }
        case 34u: { diffuse = vec3<f32>(0.0); alpha = a.a; can_discard = true; }
        case 35u: { let k = a * b * c; diffuse = tint * k.rgb; alpha = k.a; can_discard = true; }
        default: {}
    }
    let blend = (i.flags >> 8u) & 255u;
    let mesh = i.color.a * i.fade;
    var opacity = alpha * mesh;
    if (blend == 0u) {
        opacity = mesh;
    } else if (blend == 1u) {
        opacity = mesh;
        // An opaque combiner keys by its texture's alpha.
        let key = select(alpha, a.a, i.ps == 0u);
        if ((can_discard || i.ps == 0u) && key < 0.502) {
            discard;
        }
    } else if ((blend == 5u || blend == 6u) && can_discard && alpha < 0.502) {
        discard;
    }
    var lit = diffuse;
    if ((i.flags & 1u) == 0u) {
        let n = normalize(i.normal);
        let d = max(dot(n, -normalize(g.light_dir.xyz)), 0.0) * sunlight(i.world, n);
        lit = diffuse * min(ambient_at(n) + g.diffuse.rgb * d, vec3<f32>(1.0));
    }
    lit = lit + specular;
    if ((i.flags & 6u) != 0u) {
        opacity = 1.0;
    }
    return vec4<f32>(lit, opacity);
}

// Its depth alone, as the sun sees it (its shadow maps, decision T564):
// what an alpha keyed batch keys away casts none.
@fragment
fn fs_shadow(i: Out) {
    let a = textureSample(tex1, samp, i.uv1).a;
    if ((i.flags >> 8u) & 255u) == 1u && a < 0.502 {
        discard;
    }
}
