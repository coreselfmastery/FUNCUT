struct Params { amount: f32 }

fn h2(p: vec2<f32>) -> f32 {
    return hash(floor(p), 0.0);
}

fn effect(uv: vec2<f32>) -> vec4<f32> {
    let k = params.amount / 100.0;
    let stepped = floor(frame.time * 8.0) / max(8.0, 1.0);
    let c = sample(uv + vec2<f32>(stepped * 0.01 * k, 0.0));
    let flick = 1.0 + (h2(vec2<f32>(floor(frame.time * 8.0), 4.0)) - 0.5) * 0.150 * 2.0 * k;
    return vec4<f32>(c.rgb * flick, c.a);
}
