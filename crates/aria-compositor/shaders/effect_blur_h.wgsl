// Separable horizontal box blur; radius in pixels (params[0].x).

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
        vec2<f32>(-1.0, -1.0),
        vec2<f32>( 1.0, -1.0),
        vec2<f32>(-1.0,  1.0),
        vec2<f32>(-1.0,  1.0),
        vec2<f32>( 1.0, -1.0),
        vec2<f32>( 1.0,  1.0),
    );
    var uvs = array<vec2<f32>, 6>(
        vec2<f32>(0.0, 1.0),
        vec2<f32>(1.0, 1.0),
        vec2<f32>(0.0, 0.0),
        vec2<f32>(0.0, 0.0),
        vec2<f32>(1.0, 1.0),
        vec2<f32>(1.0, 0.0),
    );
    var out: VsOut;
    out.pos = vec4<f32>(positions[vi], 0.0, 1.0);
    out.uv = uvs[vi];
    return out;
}

fn sample_at(uv: vec2<f32>) -> vec4<f32> {
    return textureSample(input_tex, input_sampler, uv);
}

// Box radius, in texels, that one pass covers. Bilinear midpoint pairing reads
// two texels per sample, so 32 costs the 33 samples a per-texel loop spent on
// 16. Callers that need more support iterate the pass.
const MAX_RADIUS: i32 = 32;

@fragment
fn fs(in: VsOut) -> @location(0) vec4<f32> {
    let r = max(uniforms.params[0].x, 0.0);
    if (r <= 0.0) {
        return sample_at(in.uv);
    }
    let step = uniforms.texel_size.xy;
    let radius = min(i32(r), MAX_RADIUS);
    // The center texel has no partner.
    var acc = sample_at(in.uv);
    var wsum = 1.0;
    // Texels (2k-1, 2k) share the midpoint 2k-0.5: one linearly filtered read
    // averages the pair exactly, so it stands in for two unit weights.
    let pairs = radius / 2;
    for (var k = 1; k <= pairs; k++) {
        let offset = (f32(2 * k) - 0.5) * step.x;
        acc += (sample_at(in.uv + vec2<f32>(offset, 0.0))
              + sample_at(in.uv - vec2<f32>(offset, 0.0))) * 2.0;
        wsum += 4.0;
    }
    // An odd radius leaves the outermost texel unpaired on each side.
    if ((radius & 1) == 1) {
        let edge = f32(radius) * step.x;
        acc += sample_at(in.uv + vec2<f32>(edge, 0.0))
             + sample_at(in.uv - vec2<f32>(edge, 0.0));
        wsum += 2.0;
    }
    return acc / wsum;
}
