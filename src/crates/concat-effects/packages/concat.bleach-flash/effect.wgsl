struct Params { amount: f32 }

fn effect(uv: vec2<f32>) -> vec4<f32> {
    let c = sample(uv);
    let k = params.amount / 100.0;
    var out_c = saturation(c.rgb, 1.0 - 0.82 * k);
    out_c = contrast(out_c, 1.0 + 0.45 * k);
    let l = luma(out_c);
    out_c = mix(out_c, vec3<f32>(l) * 1.05 + vec3<f32>(0.02), 0.35 * k);
    return vec4<f32>(out_c, c.a);
}
