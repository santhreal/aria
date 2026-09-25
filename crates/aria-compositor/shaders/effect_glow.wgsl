// Bloom around the bright parts; threshold in params[0].x, intensity in params[0].y.
//
// Two rings of taps collect whatever is brighter than the threshold and add it
// back, so a highlight spills into its surroundings. One pass, so the spill is a
// ring blur rather than a separable Gaussian — a bright edge grows a halo
// without a second offscreen target.

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

/// Taps per ring.
const TAPS: i32 = 8;
/// Inner ring radius in texels; the outer ring is twice this.
const RING: f32 = 4.0;

fn bright(uv: vec2<f32>, threshold: f32) -> vec3<f32> {
    let c = textureSample(input_tex, input_sampler, uv);
    let luma = dot(c.rgb, vec3<f32>(0.2126, 0.7152, 0.0722));
    // Above the threshold only, and rescaled so a threshold of 0 keeps the whole
    // image rather than doubling it.
    let over = max(luma - threshold, 0.0) / max(1.0 - threshold, 0.001);
    return c.rgb * min(over, 1.0);
}

@fragment
fn fs(in: VsOut) -> @location(0) vec4<f32> {
    let threshold = clamp(uniforms.params[0].x, 0.0, 1.0);
    let intensity = max(uniforms.params[0].y, 0.0);
    let src = textureSample(input_tex, input_sampler, in.uv);
    let step = uniforms.texel_size.xy;

    var spill = vec3<f32>(0.0);
    for (var i = 0; i < TAPS; i = i + 1) {
        let angle = 6.2831853 * f32(i) / f32(TAPS);
        let dir = vec2<f32>(cos(angle), sin(angle));
        spill = spill + bright(in.uv + dir * step * RING, threshold);
        spill = spill + bright(in.uv + dir * step * RING * 2.0, threshold);
    }
    spill = spill / f32(TAPS * 2);

    // Glow adds light, so alpha is untouched and colour is capped at coverage:
    // a transparent pixel cannot glow.
    let rgb = clamp(src.rgb + spill * intensity, vec3<f32>(0.0), vec3<f32>(src.a));
    return vec4<f32>(rgb, src.a);
}
