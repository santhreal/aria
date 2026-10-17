// Generator: SMPTE colour bars for checking a monitor and a signal path.
// Flatten (catalog order): level @ params[0].x, the brightness of the seven
// bars (0.75 for 75% bars, 1.0 for 100% bars).
//
// Top two thirds: grey, yellow, cyan, green, magenta, red, blue at the level.
// Then the reversed blue row, and in the bottom quarter -I, full white, +Q,
// black, and a pluge step of 4% above black.

struct EffectUniforms {
    texel_size: vec4<f32>,
    params: array<vec4<f32>, 4>,
}

// A generator draws from its position and parameters alone; the input is the
// transparent solid that carries it, bound because every effect pass binds it.
@group(0) @binding(0) var input_tex: texture_2d<f32>;
@group(0) @binding(1) var input_sampler: sampler;
@group(0) @binding(2) var<uniform> uniforms: EffectUniforms;

struct VsOut {
    @builtin(position) pos: vec4<f32>,
    @location(0) uv: vec2<f32>,
}

@vertex
fn vs(@builtin(vertex_index) vi: u32) -> VsOut {
    var positions = array<vec2<f32>, 6>(
        vec2<f32>(-1.0, -1.0), vec2<f32>( 1.0, -1.0), vec2<f32>(-1.0,  1.0),
        vec2<f32>(-1.0,  1.0), vec2<f32>( 1.0, -1.0), vec2<f32>( 1.0,  1.0),
    );
    var uvs = array<vec2<f32>, 6>(
        vec2<f32>(0.0, 1.0), vec2<f32>(1.0, 1.0), vec2<f32>(0.0, 0.0),
        vec2<f32>(0.0, 0.0), vec2<f32>(1.0, 1.0), vec2<f32>(1.0, 0.0),
    );
    var out: VsOut;
    out.pos = vec4<f32>(positions[vi], 0.0, 1.0);
    out.uv = uvs[vi];
    return out;
}

/// The layer's size in pixels.
fn dims() -> vec2<f32> {
    return vec2<f32>(1.0 / uniforms.texel_size.x, 1.0 / uniforms.texel_size.y);
}

/// A straight colour, clamped, as the premultiplied colour the chain stores.
fn premultiplied(c: vec4<f32>) -> vec4<f32> {
    let s = clamp(c, vec4<f32>(0.0), vec4<f32>(1.0));
    return vec4<f32>(s.rgb * s.a, s.a);
}

@fragment
fn fs(in: VsOut) -> @location(0) vec4<f32> {
    let level = clamp(uniforms.params[0].x, 0.0, 1.0);
    let x = clamp(in.uv.x, 0.0, 0.999999);
    let bar = i32(floor(x * 7.0));
    var rgb = vec3<f32>(0.0);
    if (in.uv.y < 2.0 / 3.0) {
        var top = array<vec3<f32>, 7>(
            vec3<f32>(1.0, 1.0, 1.0), vec3<f32>(1.0, 1.0, 0.0), vec3<f32>(0.0, 1.0, 1.0),
            vec3<f32>(0.0, 1.0, 0.0), vec3<f32>(1.0, 0.0, 1.0), vec3<f32>(1.0, 0.0, 0.0),
            vec3<f32>(0.0, 0.0, 1.0),
        );
        rgb = top[bar] * level;
    } else if (in.uv.y < 0.75) {
        var middle = array<vec3<f32>, 7>(
            vec3<f32>(0.0, 0.0, 1.0), vec3<f32>(0.0, 0.0, 0.0), vec3<f32>(1.0, 0.0, 1.0),
            vec3<f32>(0.0, 0.0, 0.0), vec3<f32>(0.0, 1.0, 1.0), vec3<f32>(0.0, 0.0, 0.0),
            vec3<f32>(1.0, 1.0, 1.0),
        );
        rgb = middle[bar] * level;
    } else if (x < 5.0 / 28.0) {
        rgb = vec3<f32>(0.0, 0.129, 0.298);
    } else if (x < 10.0 / 28.0) {
        rgb = vec3<f32>(1.0);
    } else if (x < 15.0 / 28.0) {
        rgb = vec3<f32>(0.196, 0.0, 0.416);
    } else if (x >= 16.0 / 21.0 && x < 17.0 / 21.0) {
        rgb = vec3<f32>(0.04);
    }
    return vec4<f32>(rgb, 1.0);
}
