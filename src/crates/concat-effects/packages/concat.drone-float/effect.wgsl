struct Params { amount: f32 }

fn effect(uv: vec2<f32>) -> vec4<f32> {
    let k = params.amount / 100.0;
    let off = vec2<f32>(sin(frame.time * 0.110 + 1.200) * 0.0025, cos(frame.time * 0.085 + 1.200) * 0.0015) * k;
    let c = sample(uv + off);
    let breathe_l = 1.0 + sin(frame.time * 0.110 * 2.0 + 1.200) * 0.035 * k;
    return vec4<f32>(c.rgb * breathe_l, c.a);
}
