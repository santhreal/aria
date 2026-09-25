// Radial streak toward the frame centre; amount in params[0].x (0 … 1).
//
// Samples march from the pixel toward the centre and are averaged, so the frame
// smears along the line it would travel under a zoom. The centre stays sharp
// because its march length is zero.

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

const SAMPLES: i32 = 12;
/// Fraction of the distance to the centre covered at amount = 1.
const MAX_REACH: f32 = 0.3;

@fragment
fn fs(in: VsOut) -> @location(0) vec4<f32> {
    let amount = clamp(uniforms.params[0].x, 0.0, 1.0);
    let centre = vec2<f32>(0.5, 0.5);
    let toward = (centre - in.uv) * (amount * MAX_REACH);

    var sum = vec4<f32>(0.0);
    for (var i = 0; i < SAMPLES; i = i + 1) {
        let t = f32(i) / f32(SAMPLES - 1);
        sum = sum + textureSample(input_tex, input_sampler, in.uv + toward * t);
    }
    return sum / f32(SAMPLES);
}
