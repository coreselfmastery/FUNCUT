struct Params { amount: f32 }

fn effect(uv: vec2<f32>) -> vec4<f32> {
    let k = params.amount / 100.0;
    let off = vec2<f32>(sin(frame.time * 0.070 + 1.500) * 0.0018, cos(frame.time * 0.054 + 1.500) * 0.0010) * k;
    let c = sample(uv + off);
    let breathe_l = 1.0 + sin(frame.time * 0.070 * 2.0 + 1.500) * 0.025 * k;
    return vec4<f32>(c.rgb * breathe_l, c.a);
}
