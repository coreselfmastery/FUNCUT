struct Params { amount: f32 }

fn h2(p: vec2<f32>) -> f32 {
    return hash(floor(p), 0.0);
}

fn effect(uv: vec2<f32>) -> vec4<f32> {
    let c = sample(uv);
    let k = params.amount / 100.0;
    let flick = 0.5 + 0.5 * sin(frame.time * 9.0 + h2(floor(uv * 3.0)) * 6.2831);
    let gate = 1.0 - smoothstep(0.3, 0.75, distance(uv, vec2<f32>(0.5)));
    let lit = 1.0 + flick * 0.250 * k * (0.4 + gate);
    return vec4<f32>(c.rgb * lit, c.a);
}
