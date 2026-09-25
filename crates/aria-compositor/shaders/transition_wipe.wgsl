// Hard edge sweeping across the frame: progress in params.x, direction in params.y.
//
// 0: incoming grows from the left edge.
// 1: incoming grows from the right edge.
// 2: incoming grows from the bottom edge (the edge travels up).
// 3: incoming grows from the top edge (the edge travels down).

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
    let a = textureSample(outgoing_tex, samp, in.uv);
    let b = textureSample(incoming_tex, samp, in.uv);
    let t = clamp(u.params.x, 0.0, 1.0);
    let direction = i32(round(clamp(u.params.y, 0.0, 3.0)));

    // Distance from the edge the incoming frame enters by. uv.y = 0 is the top
    // row, matching the vertex table above.
    var from_edge = in.uv.x;
    switch direction {
        case 0: { from_edge = in.uv.x; }
        case 1: { from_edge = 1.0 - in.uv.x; }
        case 2: { from_edge = 1.0 - in.uv.y; }
        default: { from_edge = in.uv.y; }
    }
    return select(a, b, from_edge <= t);
}
