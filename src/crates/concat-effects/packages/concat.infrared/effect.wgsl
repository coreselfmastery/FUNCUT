struct Params { amount: f32 }

fn infrared(c: vec3<f32>) -> vec3<f32> {
    let hot = clamp(luma(c) * 1.4, 0.0, 1.0);
    let vein = clamp((c.g - c.r) * 2.0 + 0.3, 0.0, 1.0);
    return vec3<f32>(hot, vein * 0.35, 1.0 - hot * 0.85);
}

fn effect(uv: vec2<f32>) -> vec4<f32> {
    let c = sample(uv);
    let k = params.amount / 100.0;
    var out_c = infrared(c.rgb);
    out_c = mix(c.rgb, out_c, k);
    return vec4<f32>(out_c, c.a);
}
