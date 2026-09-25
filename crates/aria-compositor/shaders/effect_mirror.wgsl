// Mirror one half of the frame onto the other; mode in params[0].x.
//
// 0: left half mirrored onto the right.
// 1: right half mirrored onto the left.
// 2: top half mirrored onto the bottom.
// 3: bottom half mirrored onto the top.

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
    let mode = i32(round(clamp(uniforms.params[0].x, 0.0, 3.0)));
    var uv = in.uv;
    // uv.y = 0 is the top row, matching the vertex table above.
    switch mode {
        case 0: { uv.x = min(uv.x, 1.0 - uv.x); }
        case 1: { uv.x = max(uv.x, 1.0 - uv.x); }
        case 2: { uv.y = min(uv.y, 1.0 - uv.y); }
        default: { uv.y = max(uv.y, 1.0 - uv.y); }
    }
    return textureSample(input_tex, input_sampler, uv);
}
