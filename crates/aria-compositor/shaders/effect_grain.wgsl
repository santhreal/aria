// Film grain; amount in params[0].x (0 … 1), seed in params[0].y.
//
// Zero-mean noise per pixel, scaled by coverage so a transparent region stays
// transparent and premultiplied colour stays inside its own alpha. The noise is
// a hash of (pixel, seed), so one seed renders one grain pattern every time.

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

/// Swing of the grain at amount = 1, as a fraction of full range.
const MAX_SWING: f32 = 0.25;

fn hash(v: vec2<f32>) -> f32 {
    return fract(sin(dot(v, vec2<f32>(127.1, 311.7))) * 43758.5453);
}

@fragment
fn fs(in: VsOut) -> @location(0) vec4<f32> {
    let amount = clamp(uniforms.params[0].x, 0.0, 1.0);
    let seed = uniforms.params[0].y;
    let src = textureSample(input_tex, input_sampler, in.uv);

    let dims = vec2<f32>(1.0 / uniforms.texel_size.x, 1.0 / uniforms.texel_size.y);
    let pixel = floor(in.uv * dims);
    let noise = (hash(pixel + vec2<f32>(seed, seed * 0.37)) - 0.5) * 2.0;

    let swing = noise * amount * MAX_SWING * src.a;
    let rgb = clamp(src.rgb + vec3<f32>(swing), vec3<f32>(0.0), vec3<f32>(src.a));
    return vec4<f32>(rgb, src.a);
}
