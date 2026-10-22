// rgba_fx.wgsl — premultiplied RGBA bitmap with optional mask/chroma effects.

struct Placement {
    linear: vec4<f32>,
    trans_opacity: vec4<f32>,
    uv_rect: vec4<f32>,
    // How far the drawn quad reaches past its left, top, right and bottom
    // edges, as fractions of the quad; zero draws the quad to its edges.
    bleed: vec4<f32>,
    // Packed color-grade lanes; read with the generated `grade_*` accessors.
    grade: array<vec4<f32>, GRADE_LANES>,
}

struct Effects {
    // mask_kind, mask_feather, mask_invert (0/1), mask_enabled (0/1)
    mask: vec4<f32>,
    // chroma_rgb (normalized), chroma_enabled (0/1)
    chroma: vec4<f32>,
    // chroma_strength, chroma_shadow, pad, pad
    chroma_params: vec4<f32>,
    // quad half-extents px (x, y), mask rotation_rad (z), mask roundness (w)
    half: vec4<f32>,
    // mask center xy (fraction of layer size), mask size xy (fraction)
    mask_geo: vec4<f32>,
    // coverage remap offset (x) and scale (y), mask expansion px (z), pad
    mask_mix: vec4<f32>,
}

@group(0) @binding(0) var tex: texture_2d<f32>;
@group(0) @binding(1) var samp: sampler;
@group(0) @binding(2) var<uniform> p: Placement;
@group(0) @binding(3) var<uniform> fx: Effects;

struct VertexOutput {
    @builtin(position) position: vec4<f32>,
    @location(0) uv: vec2<f32>,
    @location(1) local: vec2<f32>,
    @location(2) quad_uv: vec2<f32>,
}

fn quad_corner(vertex_index: u32) -> vec2<f32> {
    var corners = array<vec2<f32>, 6>(
        vec2(-0.5, -0.5), vec2(0.5, -0.5), vec2(-0.5, 0.5),
        vec2(-0.5, 0.5), vec2(0.5, -0.5), vec2(0.5, 0.5),
    );
    return corners[vertex_index];
}

// `c` moved past the quad edges it lies on by `bleed`, in quad units. The
// mask and the vignette read the moved corner in the quad's own coordinates.
fn bled_corner(c: vec2<f32>, bleed: vec4<f32>) -> vec2<f32> {
    return c + select(bleed.zw, -bleed.xy, c < vec2(0.0, 0.0));
}

@vertex
fn vs(@builtin(vertex_index) vertex_index: u32) -> VertexOutput {
    let c = bled_corner(quad_corner(vertex_index), p.bleed);
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
    out.local = c * 2.0 * fx.half.xy;
    out.quad_uv = c + vec2(0.5, 0.5);
    return out;
}

@fragment
fn fs(in: VertexOutput) -> @location(0) vec4<f32> {
    var premul = textureSample(tex, samp, in.uv);

    if fx.chroma.w > 0.5 {
        let a = premul.a;
        var straight = vec3(0.0);
        if a > 1e-4 {
            straight = premul.rgb / a;
        }
        let chroma_mul = chroma_alpha(straight, fx.chroma.rgb, fx.chroma_params.x, fx.chroma_params.y);
        premul = premul * chroma_mul;
    }

    // Grade the straight-alpha color after chroma keying (the key targets the
    // source footage), before mask/opacity shape the alpha.
    if premul.a > 0.0 && p.grade[0].w > 0.5 {
        var graded = apply_color_grade(premul.rgb / premul.a, p.grade);
        graded = apply_grade_sharpness(graded, in.uv, tex, samp, grade_sharpness(p.grade));
        graded = apply_grade_vignette(graded, in.quad_uv, grade_vignette(p.grade));
        premul = vec4(graded * premul.a, premul.a);
    }

    if fx.mask.w > 0.5 {
        let malpha = mask_alpha(
            in.local,
            fx.half.xy,
            u32(fx.mask.x + 0.5),
            fx.mask.y,
            fx.mask.z,
            fx.mask_geo.xy,
            fx.mask_geo.zw,
            fx.half.z,
            fx.half.w,
            fx.mask_mix.z,
        );
        premul = premul * clamp(fx.mask_mix.x + fx.mask_mix.y * malpha, 0.0, 1.0);
    }

    return premul * p.trans_opacity.z;
}
