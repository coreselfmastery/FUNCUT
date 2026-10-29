struct Params { amount: f32 }

fn effect(uv: vec2<f32>) -> vec4<f32> {
    let k = params.amount / 100.0;
    let off = vec2<f32>(sin(frame.time * 0.350 + 0.900) * 0.0040, cos(frame.time * 0.269 + 0.900) * 0.0030) * k;
    let c = sample(uv + off);
    let breathe_l = 1.0 + sin(frame.time * 0.350 * 2.0 + 0.900) * 0.050 * k;
    return vec4<f32>(c.rgb * breathe_l, c.a);
}
