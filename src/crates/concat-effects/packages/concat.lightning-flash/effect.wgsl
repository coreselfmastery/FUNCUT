struct Params { amount: f32 }

fn h2(p: vec2<f32>) -> f32 {
    return hash(floor(p), 0.0);
}

fn effect(uv: vec2<f32>) -> vec4<f32> {
    let c = sample(uv);
    let k = params.amount / 100.0;
    let t = frame.time / 2.40;
    let strike = floor(t);
    let ph = fract(t);
    let h = h2(vec2<f32>(strike, 9.0));
    var pulse = 0.0;
    if (ph < 0.05) {
        pulse = 1.0 - ph / 0.05;
    } else if (ph > 0.12 && ph < 0.16) {
        pulse = 1.0 - (ph - 0.12) / 0.04;
    }
    pulse *= step(0.35, h);
    return vec4<f32>(c.rgb + vec3<f32>(0.85, 0.9, 1.0) * pulse * 0.500 * k, c.a);
}
