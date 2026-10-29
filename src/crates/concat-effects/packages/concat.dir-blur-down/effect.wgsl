struct Params { amount: f32 }

fn effect(uv: vec2<f32>) -> vec4<f32> {
    let k = params.amount / 100.0;
    var sum = vec4<f32>(0.0);
    for (var i = -3; i <= 3; i = i + 1) {
        sum += sample(uv + vec2<f32>(0.0, 1.0) * f32(i) * 0.012 * k);
    }
    let c = sum / 7.0;
    let lean = 1.0 + (1.0 * uv.y - 0.5) * 0.100 * 2.0 * k;
    return vec4<f32>(c.rgb * lean, c.a);
}
