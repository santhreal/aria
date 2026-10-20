// rgba.wgsl — a straight-alpha RGBA bitmap on a placed quad (text, shapes,
// stickers, stills).
//
// The texture is uploaded *premultiplied* (rgb already scaled by alpha) so that
// bilinear min/magnification doesn't bleed background color through transparent
// texels — the classic halo around anti-aliased glyph edges. The fragment
// scales all four channels by the layer opacity (premultiplied stays
// premultiplied), and the pipeline blends with premultiplied src-over
// (src factor One, dst factor OneMinusSrcAlpha).

struct Placement {
    // Columns of the 2x2 unit-quad → clip-space linear part (m00, m10, m01, m11).
    linear: vec4<f32>,
    // Clip-space translation (x, y), layer opacity (z), pad (w).
    trans_opacity: vec4<f32>,
    // Sampled UV rect (u0, v0, u1, v1) across the quad.
    uv_rect: vec4<f32>,
    // Packed color-grade lanes; read with the generated `grade_*` accessors.
    grade: array<vec4<f32>, GRADE_LANES>,
}

@group(0) @binding(0) var tex: texture_2d<f32>;
@group(0) @binding(1) var samp: sampler;
@group(0) @binding(2) var<uniform> p: Placement;

struct VertexOutput {
    @builtin(position) position: vec4<f32>,
    @location(0) uv: vec2<f32>,
    @location(1) quad_uv: vec2<f32>,
}

fn quad_corner(vertex_index: u32) -> vec2<f32> {
    var corners = array<vec2<f32>, 6>(
        vec2(-0.5, -0.5), vec2(0.5, -0.5), vec2(-0.5, 0.5),
        vec2(-0.5, 0.5), vec2(0.5, -0.5), vec2(0.5, 0.5),
    );
    return corners[vertex_index];
}

@vertex
fn vs(@builtin(vertex_index) vertex_index: u32) -> VertexOutput {
    let c = quad_corner(vertex_index);
    let m = p.linear;
    let t = p.trans_opacity;
    var out: VertexOutput;
    out.position = vec4(
        m.x * c.x + m.z * c.y + t.x,
        m.y * c.x + m.w * c.y + t.y,
        0.0,
        1.0,
    );
    out.uv = mix(p.uv_rect.xy, p.uv_rect.zw, c + vec2(0.5, 0.5));
    out.quad_uv = c + vec2(0.5, 0.5);
    return out;
}

@fragment
fn fs(in: VertexOutput) -> @location(0) vec4<f32> {
    // Premultiplied sample; grade in straight-alpha space, then re-premultiply.
    let premul = textureSample(tex, samp, in.uv);
    let layer_a = premul.a * p.trans_opacity.z;
    if layer_a <= 0.0 {
        return vec4(0.0, 0.0, 0.0, 0.0);
    }
    var rgb = premul.rgb;
    if premul.a > 0.0 {
        rgb = rgb / premul.a;
    }
    if p.grade[0].w > 0.5 {
        rgb = apply_color_grade(rgb, p.grade);
        rgb = apply_grade_sharpness(rgb, in.uv, tex, samp, grade_sharpness(p.grade));
        rgb = apply_grade_vignette(rgb, in.quad_uv, grade_vignette(p.grade));
    }
    return vec4(rgb * layer_a, layer_a);
}
