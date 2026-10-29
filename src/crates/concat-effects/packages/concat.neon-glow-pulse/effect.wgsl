struct Params { amount: f32 }

fn effect(uv: vec2<f32>) -> vec4<f32> {
    let c = sample(uv);
    let k = params.amount / 100.0;
    let l = luma(c.rgb);
    let rim = smoothstep(0.3, 0.9, l) * (0.6 + 0.4 * sin(frame.time * 2.0));
    return vec4<f32>(c.rgb + vec3<f32>(0.3, 1.0, 1.2) * rim * k, c.a);
}
