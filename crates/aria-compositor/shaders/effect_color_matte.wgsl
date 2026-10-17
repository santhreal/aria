// Generator: one colour over the whole layer.
// Flatten (catalog order): color(4) @ params[0].

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
    return premultiplied(uniforms.params[0]);
}
