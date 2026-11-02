// The 3D world's weather (decision T565): rain, snow or sand falling in a
// box about the eye. Each of the instances is a quad of the weather's
// texture at a steady place of its own in the box, moved by the clock and
// wrapped in the box, so it stays where it is in the world as the eye
// moves; faded out toward the box's sides. Rain stands upright and falls
// fast, snow faces the eye and drifts down slowly, sand blows across.

struct Globals {
    view_proj: mat4x4<f32>,
    // The eye, and the clock (seconds).
    eye: vec4<f32>,
    // The camera's right and up (billboards).
    right: vec4<f32>,
    up: vec4<f32>,
    // Its color, lit, and how opaque.
    color: vec4<f32>,
    // x: the kind (1 rain, 2 snow, 3 sand); y: the box's side (yards);
    // z: how fast it falls (yards a second); w: a quad's size (yards).
    how: vec4<f32>,
};

@group(0) @binding(0) var<uniform> g: Globals;
@group(0) @binding(1) var tex: texture_2d<f32>;
@group(0) @binding(2) var samp: sampler;

struct Out {
    @builtin(position) clip: vec4<f32>,
    @location(0) uv: vec2<f32>,
    @location(1) fade: f32,
};

// A steady random point in the unit cube for an instance.
fn hash3(i: u32) -> vec3<f32> {
    var x = i * 747796405u + 2891336453u;
    var v = vec3<u32>(x, x * 1664525u + 1013904223u, x * 22695477u + 1u);
    v = (v ^ (v >> vec3<u32>(16u))) * 2246822519u;
    v = v ^ (v >> vec3<u32>(13u));
    return vec3<f32>(v & vec3<u32>(0xFFFFFFu)) / 16777216.0;
}

@vertex
fn vs(@builtin(vertex_index) vi: u32, @builtin(instance_index) ii: u32) -> Out {
    let kind = u32(g.how.x);
    let side = g.how.y;
    let t = g.eye.w;
    let seed = hash3(ii);
    // (its motion: down for rain and snow, snow swaying; across for sand)
    var motion = vec3<f32>(0.0, 0.0, -g.how.z * t);
    if kind == 2u {
        let phase = seed.x * 6.2832;
        motion += vec3<f32>(sin(t * 0.7 + phase), cos(t * 0.5 + phase), 0.0) * 0.6;
    } else if kind == 3u {
        motion = vec3<f32>(g.how.z * t * 0.4, g.how.z * t, -g.how.z * t * 0.05);
    }
    // (wrapped in the box about the eye, in place in the world)
    let corner = g.eye.xyz - vec3<f32>(side * 0.5);
    let p = corner + fract((seed * side + motion - corner) / side) * side;
    var corners = array<vec2<f32>, 6>(
        vec2<f32>(-0.5, -0.5), vec2<f32>(0.5, -0.5), vec2<f32>(0.5, 0.5),
        vec2<f32>(-0.5, -0.5), vec2<f32>(0.5, 0.5), vec2<f32>(-0.5, 0.5),
    );
    let c = corners[vi];
    var right = g.right.xyz;
    var up = g.up.xyz;
    var size = vec2<f32>(g.how.w, g.how.w);
    if kind == 1u {
        // (a streak: upright, turned to the eye, eight times as long)
        up = vec3<f32>(0.0, 0.0, 1.0);
        let to_eye = g.eye.xyz - p;
        right = normalize(cross(up, to_eye + vec3<f32>(1e-4, 0.0, 0.0)));
        size = vec2<f32>(g.how.w, g.how.w * 8.0);
    }
    let world = p + right * c.x * size.x + up * c.y * size.y;
    var o: Out;
    o.clip = g.view_proj * vec4<f32>(world, 1.0);
    o.uv = vec2<f32>(c.x + 0.5, 0.5 - c.y);
    let d = distance(p, g.eye.xyz) / (side * 0.5);
    o.fade = 1.0 - smoothstep(0.6, 1.0, d);
    return o;
}

@fragment
fn fs(i: Out) -> @location(0) vec4<f32> {
    let t = textureSample(tex, samp, i.uv);
    let a = t.a * g.color.a * i.fade;
    if a < 0.01 {
        discard;
    }
    // (its alpha alone, in the weather's lit color: WoW Forever's rain
    // texture is black, its alpha the streak; unverified)
    return vec4<f32>(g.color.rgb, a);
}
