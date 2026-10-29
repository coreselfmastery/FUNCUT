struct Params { amount: f32 }

fn effect(uv: vec2<f32>) -> vec4<f32> {
    let k = params.amount / 100.0;
    let d = normalize(uv - vec2<f32>(0.5) + vec2<f32>(0.0001));
    var sum = vec4<f32>(0.0);
    for (var i = 0; i < 6; i = i + 1) {
        let t = f32(i) * 0.012 * k;
        sum += sample(uv - d * t);
    }
    let c = sum / 6.0;
    let r = length(uv - vec2<f32>(0.5));
    let lit = 1.0 + 0.350 * k * (1.0 - smoothstep(0.0, 0.5, r)) + -0.050 * k * smoothstep(0.4, 0.9, r);
    return vec4<f32>(c.rgb * lit, c.a);
}
