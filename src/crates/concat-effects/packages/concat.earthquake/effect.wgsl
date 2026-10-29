struct Params { amount: f32 }

fn h2(p: vec2<f32>) -> f32 {
    return hash(floor(p), 0.0);
}

fn effect(uv: vec2<f32>) -> vec4<f32> {
    let k = params.amount / 100.0;
    let beat = frame.time * 2.0;
    let env = abs(sin(beat * 3.14159265359));
    let jx = (h2(vec2<f32>(floor(beat), 1.0)) - 0.5) * 0.05 * k * env;
    let jy = (h2(vec2<f32>(floor(beat), 2.0)) - 0.5) * 0.05 * k * env;
    let c = sample(uv + vec2<f32>(jx, jy));
    let roll = 1.0 - 0.250 * k * env * 0.6;
    let flick = 1.0 + (h2(vec2<f32>(floor(beat), 3.0)) - 0.5) * 0.2 * k * env;
    return vec4<f32>(c.rgb * roll * flick, c.a);
}
