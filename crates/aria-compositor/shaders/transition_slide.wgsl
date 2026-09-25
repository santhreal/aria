// Push: the incoming frame slides in from the right and shoves the outgoing
// frame out to the left. Progress in params.x.
//
// Both frames move together, so the seam between them travels at one speed and
// neither frame is scaled.

struct TransitionUniforms {
    texel_size: vec4<f32>,
    params: vec4<f32>,
}

@group(0) @binding(0) var outgoing_tex: texture_2d<f32>;
@group(0) @binding(1) var incoming_tex: texture_2d<f32>;
@group(0) @binding(2) var samp: sampler;
@group(0) @binding(3) var<uniform> u: TransitionUniforms;

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
    let t = clamp(u.params.x, 0.0, 1.0);
    let seam = 1.0 - t;
    if (in.uv.x < seam) {
        // Outgoing frame, shifted left by t.
        let uv = vec2<f32>(in.uv.x + t, in.uv.y);
        return textureSample(outgoing_tex, samp, uv);
    }
    // Incoming frame, entering from the right edge.
    let uv = vec2<f32>(in.uv.x - seam, in.uv.y);
    return textureSample(incoming_tex, samp, uv);
}
