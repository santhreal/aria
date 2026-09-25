// Radial channel separation; amount in params[0].x (0 … 1).
//
// Red is sampled slightly outward from the frame centre and blue slightly
// inward, which is how a real lens splits the spectrum. Alpha follows green so
// the layer's coverage is unchanged.

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

// Peak separation at the frame edge, in uv units, at amount = 1.
const MAX_SHIFT: f32 = 0.02;

@fragment
fn fs(in: VsOut) -> @location(0) vec4<f32> {
    let amount = clamp(uniforms.params[0].x, 0.0, 1.0);
    let from_centre = in.uv - vec2<f32>(0.5, 0.5);
    let shift = from_centre * (amount * MAX_SHIFT * 2.0);

    let red = textureSample(input_tex, input_sampler, in.uv + shift);
    let green = textureSample(input_tex, input_sampler, in.uv);
    let blue = textureSample(input_tex, input_sampler, in.uv - shift);
    return vec4<f32>(red.r, green.g, blue.b, green.a);
}
