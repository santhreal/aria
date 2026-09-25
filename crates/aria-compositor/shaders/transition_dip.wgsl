// Dip through a solid colour: progress in params.x, colour in params.y (0 black, 1 white).
//
// The first half fades the outgoing frame down to the colour, the second half
// fades the incoming frame up out of it, so the cut passes through one fully
// opaque frame at the midpoint.

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
    let white = clamp(u.params.y, 0.0, 1.0);
    // Opaque, and premultiplied like everything else on this path.
    let dip = vec4<f32>(vec3<f32>(white), 1.0);

    if (t < 0.5) {
        let a = textureSample(outgoing_tex, samp, in.uv);
        return mix(a, dip, t * 2.0);
    }
    let b = textureSample(incoming_tex, samp, in.uv);
    return mix(dip, b, (t - 0.5) * 2.0);
}
