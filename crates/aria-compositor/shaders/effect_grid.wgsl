// Generator: lines of one colour on a regular grid, transparent between them.
// Flatten (catalog order): color(4) @ params[0], spacing @ params[1].x,
// width @ params[1].y, anchor.xy @ params[1].zw. Spacing and width are in
// pixels of a 1080-pixel-high frame; the anchor is a point two lines cross at,
// as a fraction of the frame.

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
    let scale = size.y / 1080.0;
    let spacing = max(uniforms.params[1].x * scale, 1e-3);
    let width = max(uniforms.params[1].y, 0.0) * scale;
    let p = (in.uv - uniforms.params[1].zw) * size;
    // Distance from this pixel's centre to the nearest line on either axis.
    let off = abs(p - spacing * round(p / spacing));
    let distance = min(off.x, off.y);
    // A pixel of falloff keeps a thin line from stepping; a line narrower than a
    // pixel fades with its width rather than vanishing at once.
    let coverage = clamp(width * 0.5 + 0.5 - distance, 0.0, 1.0) * min(width, 1.0);
    let c = clamp(uniforms.params[0], vec4<f32>(0.0), vec4<f32>(1.0));
    let a = c.a * coverage;
    return vec4<f32>(c.rgb * a, a);
}
