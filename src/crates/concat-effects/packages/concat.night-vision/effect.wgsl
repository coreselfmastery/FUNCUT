struct Params { amount: f32 }

fn night_vision(c: vec3<f32>) -> vec3<f32> {
    let l = luma(c);
    let boost = pow(clamp(l * 1.8 + 0.08, 0.0, 1.0), 0.8);
    return vec3<f32>(boost * 0.15, boost, boost * 0.25);
}

fn effect(uv: vec2<f32>) -> vec4<f32> {
    let c = sample(uv);
    let k = params.amount / 100.0;
    var out_c = night_vision(c.rgb);
    out_c = mix(c.rgb, out_c, k);
    return vec4<f32>(out_c, c.a);
}
