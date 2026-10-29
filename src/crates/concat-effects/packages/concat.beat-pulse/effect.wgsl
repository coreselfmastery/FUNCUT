struct Params { amount: f32 }

fn effect(uv: vec2<f32>) -> vec4<f32> {
    let c = sample(uv);
    let k = params.amount / 100.0;
    let beat = frame.time * 2.00000;
    let pulse = 1.0 + pow(1.0 - fract(beat), 2.0) * 0.220 * k;
    var out_c = c.rgb * pulse;
    return vec4<f32>(out_c, c.a);
}
