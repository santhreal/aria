// Mask stack: the stack so far @0, one mask's coverage @1.
//
// params.x = op id (`MaskOp::id`), params.y = opacity, params.z = softness,
// params.w = invert (0 or 1). texel_size.z = the coverage the stack starts
// from, texel_size.w = 1 when @0 holds the stack so far; the stack's first
// mask reads the start instead.
//
// Both inputs are premultiplied white, so alpha is the coverage. The output
// is the combined coverage in every channel, the same premultiplied white the
// matte pass reads a stencil from. Softness blurs the mask's coverage over a
// radius before inversion, as in `matte.wgsl`.

struct StackUniforms {
    texel_size: vec4<f32>,
    params: vec4<f32>,
}

@group(0) @binding(0) var stack_tex: texture_2d<f32>;
@group(0) @binding(1) var mask_tex: texture_2d<f32>;
@group(0) @binding(2) var samp: sampler;
@group(0) @binding(3) var<uniform> u: StackUniforms;

/// Widest softness reach, in texels (`matte.wgsl`).
const MAX_SOFTNESS_TEXELS: f32 = 12.0;
/// Taps per axis of the coverage blur.
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

/// `op(a, m)`; the GPU twin of `MaskOp::apply`.
fn combine(op: u32, a: f32, m: f32) -> f32 {
    switch op {
        case 1u: {
            return max(a - m, 0.0);
        }
        case 2u: {
            return a * m;
        }
        case 3u: {
            return max(a, m);
        }
        case 4u: {
            return min(a, m);
        }
        case 5u: {
            return abs(a - m);
        }
        default: {
            return min(a + m, 1.0);
        }
    }
}

@fragment
fn fs(in: VsOut) -> @location(0) vec4<f32> {
    let held = textureSample(stack_tex, samp, in.uv).a;
    let a = select(u.texel_size.z, held, u.texel_size.w > 0.5);

    let radius = clamp(u.params.z, 0.0, 1.0) * MAX_SOFTNESS_TEXELS;
    let step = radius * 0.5 * u.texel_size.xy;
    var m = 0.0;
    for (var y = -TAPS / 2; y <= TAPS / 2; y = y + 1) {
        for (var x = -TAPS / 2; x <= TAPS / 2; x = x + 1) {
            let offset = vec2<f32>(f32(x), f32(y)) * step;
            m = m + textureSample(mask_tex, samp, in.uv + offset).a;
        }
    }
    m = clamp(m / f32(TAPS * TAPS), 0.0, 1.0);
    if (u.params.w > 0.5) {
        m = 1.0 - m;
    }

    let opacity = clamp(u.params.y, 0.0, 1.0);
    let out = clamp(mix(a, combine(u32(u.params.x + 0.5), a, m), opacity), 0.0, 1.0);
    return vec4<f32>(out, out, out, out);
}
