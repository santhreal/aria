@fragment
fn fs(in: VsOut) -> @location(0) vec4<f32> {
    let px = textureSample(input_tex, aria_sampler, in.uv);
    return vec4<f32>(px.rgb * param_gain(), px.a);
}
