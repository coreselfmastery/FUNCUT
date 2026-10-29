struct Params { amount: f32 }

fn effect(uv: vec2<f32>) -> vec4<f32> {
    let k = params.amount / 100.0;
    let off = vec2<f32>(sin(frame.time * 0.300 + 0.700) * 0.0060, cos(frame.time * 0.231 + 0.700) * 0.0040) * k;
    let c = sample(uv + off);
    let breathe_l = 1.0 + sin(frame.time * 0.300 * 2.0 + 0.700) * 0.050 * k;
    return vec4<f32>(c.rgb * breathe_l, c.a);
}
