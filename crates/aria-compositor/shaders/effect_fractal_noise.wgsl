// Generator: fractal value noise between two colours.
// Flatten (catalog order): dark_color(4) @ params[0], light_color(4) @ params[1],
// scale @ params[2].x, evolution @ params[2].y, complexity @ params[2].z,
// contrast @ params[2].w, offset.xy @ params[3].xy.
//
// Scale is the size of the coarsest feature in pixels of a 1080-pixel-high
// frame, and the offset is in frame heights, so a proxy or a reduced preview
// draws the same noise. Evolution moves through a third noise axis: keyframe it
// to animate the pattern, one unit being one full change. Complexity is the
// number of octaves; a fractional value fades the last one in, so a keyframed
// complexity changes the detail smoothly.

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

const MAX_OCTAVES: i32 = 8;

fn hash3(p: vec3<f32>) -> f32 {
    var q = fract(p * vec3<f32>(0.1031, 0.1030, 0.0973));
    q = q + dot(q, q.yxz + 33.33);
    return fract((q.x + q.y) * q.z);
}

fn value_noise(p: vec3<f32>) -> f32 {
    let i = floor(p);
    let f = fract(p);
    let u = f * f * (3.0 - 2.0 * f);
    let x00 = mix(hash3(i), hash3(i + vec3<f32>(1.0, 0.0, 0.0)), u.x);
    let x10 = mix(hash3(i + vec3<f32>(0.0, 1.0, 0.0)), hash3(i + vec3<f32>(1.0, 1.0, 0.0)), u.x);
    let x01 = mix(hash3(i + vec3<f32>(0.0, 0.0, 1.0)), hash3(i + vec3<f32>(1.0, 0.0, 1.0)), u.x);
    let x11 = mix(hash3(i + vec3<f32>(0.0, 1.0, 1.0)), hash3(i + vec3<f32>(1.0, 1.0, 1.0)), u.x);
    return mix(mix(x00, x10, u.y), mix(x01, x11, u.y), u.z);
}

@fragment
fn fs(in: VsOut) -> @location(0) vec4<f32> {
    let size = dims();
    let scale = max(uniforms.params[2].x, 1e-3);
    let evolution = uniforms.params[2].y;
    let complexity = clamp(uniforms.params[2].z, 1.0, f32(MAX_OCTAVES));
    let contrast = max(uniforms.params[2].w, 0.0);
    let offset = uniforms.params[3].xy;

    // Position in frame heights, then in features of the coarsest octave.
    let at = (in.uv * vec2<f32>(size.x / size.y, 1.0) + offset) * 1080.0 / scale;
    var sum = 0.0;
    var norm = 0.0;
    var amplitude = 1.0;
    var frequency = 1.0;
    for (var octave = 0; octave < MAX_OCTAVES; octave = octave + 1) {
        let weight = clamp(complexity - f32(octave), 0.0, 1.0) * amplitude;
        // Each octave starts elsewhere in the noise, so the octaves do not
        // line up at the origin.
        let shift = vec3<f32>(f32(octave) * 17.31, f32(octave) * 9.73, f32(octave) * 5.17);
        sum = sum + weight * value_noise(vec3<f32>(at, evolution) * frequency + shift);
        norm = norm + weight;
        amplitude = amplitude * 0.5;
        frequency = frequency * 2.0;
    }
    let value = clamp((sum / max(norm, 1e-6) - 0.5) * contrast + 0.5, 0.0, 1.0);
    return mix(premultiplied(uniforms.params[0]), premultiplied(uniforms.params[1]), value);
}
