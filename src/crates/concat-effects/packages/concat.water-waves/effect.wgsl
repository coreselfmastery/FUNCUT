struct Params { amount: f32 }

fn effect(uv: vec2<f32>) -> vec4<f32> {
    let k = params.amount / 100.0;
    let w = sin(uv.y * 9.00 + frame.time * 1.20) * 0.0120 * k;
    let c = sample(uv + vec2<f32>(w, 0.0));
    let lift = 1.0 + cos(uv.y * 9.00 + frame.time * 1.20) * 0.100 * k;
    return vec4<f32>(c.rgb * lift, c.a);
}
