struct Params { amount: f32 }

fn effect(uv: vec2<f32>) -> vec4<f32> {
    let c = sample(uv);
    let k = params.amount / 100.0;
    let w = clamp(luma(c.rgb) * 0.75 + uv.x * 0.35, 0.0, 1.0);
    var out_c = mix(vec3<f32>(0.05, 0.1, 0.3), vec3<f32>(1.0, 0.4, 0.2), w);
    out_c = mix(c.rgb, out_c, k);
    return vec4<f32>(out_c, c.a);
}
