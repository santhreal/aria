// Row-band tearing with channel slip; amount in params[0].x, seed in params[0].y.
//
// The frame is cut into horizontal bands. A band whose hash falls inside
// `amount` slides sideways, and the red and blue channels slip against green by
// a fraction of that slide. Everything is a function of (band, seed), so a given
// seed renders the same frame every time — a scrub back and forth does not
// reshuffle the tear.

struct EffectUniforms {
    texel_size: vec4<f32>,
    params: array<vec4<f32>, 4>,
}

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

/// Bands the frame is cut into at amount = 1.
const BANDS: f32 = 24.0;
/// Widest sideways slide, in uv units.
const MAX_SLIDE: f32 = 0.08;

fn hash(v: vec2<f32>) -> f32 {
    return fract(sin(dot(v, vec2<f32>(12.9898, 78.233))) * 43758.5453);
}

@fragment
fn fs(in: VsOut) -> @location(0) vec4<f32> {
    let amount = clamp(uniforms.params[0].x, 0.0, 1.0);
    let seed = uniforms.params[0].y;
    let band = floor(in.uv.y * BANDS);
    let pick = hash(vec2<f32>(band, seed));

    // Only the noisiest bands tear, so amount controls how much of the frame is
    // disturbed as well as how far it moves.
    var slide = 0.0;
    if (pick < amount) {
        slide = (hash(vec2<f32>(band + 1.0, seed * 1.7 + 3.0)) - 0.5) * 2.0
            * MAX_SLIDE * amount;
    }

    let base = in.uv + vec2<f32>(slide, 0.0);
    let slip = vec2<f32>(slide * 0.35, 0.0);
    let red = textureSample(input_tex, input_sampler, base + slip);
    let green = textureSample(input_tex, input_sampler, base);
    let blue = textureSample(input_tex, input_sampler, base - slip);
    return vec4<f32>(red.r, green.g, blue.b, green.a);
}
