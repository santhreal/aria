// Track matte: content @0, stencil @1. params.x = channel (0 alpha, 1 luma),
// params.y = invert (0 or 1), params.z = softness.
//
// Both inputs are premultiplied, so coverage scales the whole RGBA. Luma is
// read from the stencil's unpremultiplied colour: a premultiplied black pixel
// and a transparent one are the same bytes, and a luma matte has to tell a
// black plate from a hole.
//
// Softness is a spatial blur of the stencil's coverage, not a curve on it. A
// hard-edged stencil has no gradient to reshape, so any per-pixel function
// would only lower the matte's contrast — it would make an opaque region
// translucent instead of making its edge soft. Blurring coverage over a radius
// is what turns an edge into a ramp, and it is exactly identity at radius zero.

struct MatteUniforms {
    texel_size: vec4<f32>,
    params: vec4<f32>,
}

@group(0) @binding(0) var content_tex: texture_2d<f32>;
@group(0) @binding(1) var stencil_tex: texture_2d<f32>;
@group(0) @binding(2) var samp: sampler;
@group(0) @binding(3) var<uniform> u: MatteUniforms;

/// Widest softness reach, in texels. A matte edge is a local feature; past a
/// dozen texels the cut stops reading as an edge at all.
const MAX_SOFTNESS_TEXELS: f32 = 12.0;
/// Taps per axis of the coverage blur. 5x5 keeps one pass and gives a linear
/// ramp across a hard edge.
const TAPS: i32 = 5;

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

/// The stencil's coverage at one point, before softness and inversion.
fn coverage_at(uv: vec2<f32>) -> f32 {
    let stencil = textureSample(stencil_tex, samp, uv);
    if (u.params.x > 0.5) {
        // Unpremultiply before reading luminance; a fully transparent pixel
        // carries no colour and cuts.
        let rgb = select(stencil.rgb / max(stencil.a, 1.0e-4), vec3<f32>(0.0), stencil.a <= 0.0);
        return clamp(dot(rgb, vec3<f32>(0.2126, 0.7152, 0.0722)), 0.0, 1.0) * stencil.a;
    }
    return stencil.a;
}

@fragment
fn fs(in: VsOut) -> @location(0) vec4<f32> {
    let content = textureSample(content_tex, samp, in.uv);

    let radius = clamp(u.params.z, 0.0, 1.0) * MAX_SOFTNESS_TEXELS;
    let step = radius * 0.5 * u.texel_size.xy;
    var coverage = 0.0;
    for (var y = -TAPS / 2; y <= TAPS / 2; y = y + 1) {
        for (var x = -TAPS / 2; x <= TAPS / 2; x = x + 1) {
            let offset = vec2<f32>(f32(x), f32(y)) * step;
            coverage = coverage + coverage_at(in.uv + offset);
        }
    }
    coverage = coverage / f32(TAPS * TAPS);

    if (u.params.y > 0.5) {
        coverage = 1.0 - coverage;
    }
    return content * clamp(coverage, 0.0, 1.0);
}
