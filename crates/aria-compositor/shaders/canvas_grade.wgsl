// Full-canvas color grade pass. The input is the already-composited canvas;
// alpha passes through unchanged.
//
// The uniform is the same packed lane block the layer shaders take, read back
// through the generated `grade_*` accessors.
//
// Prefixed with the generated preamble and grade.wgsl at pipeline compile time.

struct GradeUniforms {
    // Packed color-grade lanes; read with the generated `grade_*` accessors.
    grade: array<vec4<f32>, GRADE_LANES>,
}

@group(0) @binding(0) var input_tex: texture_2d<f32>;
@group(0) @binding(1) var input_sampler: sampler;
@group(0) @binding(2) var<uniform> uniforms: GradeUniforms;

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

@fragment
fn fs(in: VsOut) -> @location(0) vec4<f32> {
    let color = textureSample(input_tex, input_sampler, in.uv);
    var rgb = apply_color_grade(color.rgb, uniforms.grade);
    rgb = apply_grade_sharpness(rgb, in.uv, input_tex, input_sampler, grade_sharpness(uniforms.grade));
    rgb = apply_grade_vignette(rgb, in.uv, grade_vignette(uniforms.grade));
    return vec4<f32>(rgb, color.a);
}
