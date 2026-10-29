struct Params { amount: f32 }

fn xray(c: vec3<f32>) -> vec3<f32> {
    let l = luma(c);
    let bone = pow(1.0 - clamp(l, 0.0, 1.0), 1.4);
    return vec3<f32>(bone * 0.75, bone * 0.9, bone);
}

fn effect(uv: vec2<f32>) -> vec4<f32> {
    let c = sample(uv);
    let k = params.amount / 100.0;
    var out_c = xray(c.rgb);
    out_c = mix(c.rgb, out_c, k);
    return vec4<f32>(out_c, c.a);
}
