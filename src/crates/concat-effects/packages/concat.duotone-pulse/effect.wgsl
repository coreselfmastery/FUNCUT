struct Params { amount: f32 }

fn effect(uv: vec2<f32>) -> vec4<f32> {
    let c = sample(uv);
    let k = params.amount / 100.0;
    let l = luma(c.rgb);
    let w = smoothstep(0.5 - 0.250, 0.5 + 0.250, l);
    var out_c = mix(vec3<f32>(0.05, 0.02, 0.12), vec3<f32>(1.0, 0.85, 0.6), w);
    out_c = mix(c.rgb, out_c, k);
    return vec4<f32>(out_c, c.a);
}
