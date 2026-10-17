// Generator: a ramp from one colour to another along a line.
// Flatten (catalog order): start_color(4) @ params[0], end_color(4) @ params[1],
// start.xy @ params[2].xy, end.xy @ params[2].zw. Points are fractions of the
// frame from its top-left corner; the ramp is measured in pixels, so it keeps
// its angle on a frame that is not square.

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
    let size = dims();
    let start = uniforms.params[2].xy * size;
    let axis = uniforms.params[2].zw * size - start;
    let length2 = dot(axis, axis);
    var t = 0.0;
    if (length2 > 1e-6) {
        t = clamp(dot(in.uv * size - start, axis) / length2, 0.0, 1.0);
    }
    // Mixed premultiplied, so a ramp into a transparent end keeps its hue.
    return mix(premultiplied(uniforms.params[0]), premultiplied(uniforms.params[1]), t);
}
