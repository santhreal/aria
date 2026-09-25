// Unsharp mask over the four nearest neighbours; amount in params[0].x.
//
// Colour arrives premultiplied, so the result is clamped to the centre alpha:
// a sharpened edge must not push a channel above its own coverage.

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

@fragment
fn fs(in: VsOut) -> @location(0) vec4<f32> {
    let amount = max(uniforms.params[0].x, 0.0);
    let step = uniforms.texel_size.xy;
    let centre = textureSample(input_tex, input_sampler, in.uv);
    let l = textureSample(input_tex, input_sampler, in.uv - vec2<f32>(step.x, 0.0));
    let r = textureSample(input_tex, input_sampler, in.uv + vec2<f32>(step.x, 0.0));
    let u = textureSample(input_tex, input_sampler, in.uv - vec2<f32>(0.0, step.y));
    let d = textureSample(input_tex, input_sampler, in.uv + vec2<f32>(0.0, step.y));

    let laplacian = centre.rgb * 4.0 - (l.rgb + r.rgb + u.rgb + d.rgb);
    let rgb = clamp(centre.rgb + laplacian * amount, vec3<f32>(0.0), vec3<f32>(centre.a));
    return vec4<f32>(rgb, centre.a);
}
