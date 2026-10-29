struct Params { amount: f32 }

fn effect(uv: vec2<f32>) -> vec4<f32> {
    let k = params.amount / 100.0;
    let off = vec2<f32>(sin(frame.time * 0.160 + 0.500) * 0.0000, cos(frame.time * 0.123 + 0.500) * 0.0000) * k;
    let c = sample(uv + off);
    let breathe_l = 1.0 + sin(frame.time * 0.160 * 2.0 + 0.500) * 0.070 * k;
    return vec4<f32>(c.rgb * breathe_l, c.a);
}
