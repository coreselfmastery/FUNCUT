struct Params { amount: f32 }

fn effect(uv: vec2<f32>) -> vec4<f32> {
    let k = params.amount / 100.0;
    var sum = sample(uv);
    for (var i = 1; i <= 4; i = i + 1) {
        let t = f32(i) / 4.0;
        sum += sample(uv + vec2<f32>(t * 0.05 * k, 0.0)) * (1.0 - t * 0.5);
        sum += sample(uv - vec2<f32>(t * 0.05 * k, 0.0)) * (1.0 - t * 0.5);
    }
    let c = sum / 8.0;
    return vec4<f32>(c.rgb * (1.0 + 0.450 * k), c.a);
}
