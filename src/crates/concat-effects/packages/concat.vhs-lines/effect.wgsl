struct Params { amount: f32 }

fn h2(p: vec2<f32>) -> f32 {
    return hash(floor(p), 0.0);
}

fn effect(uv: vec2<f32>) -> vec4<f32> {
    let c = sample(uv);
    let k = params.amount / 100.0;
    let line = 0.5 + 0.5 * sin(uv.y * max(frame.size.y / 3.0, 6.0) * 3.14159265359 * 2.0);
    let band_y = 0.5 + 0.35 * sin(frame.time * 0.8);
    let band = 1.0 - smoothstep(0.0, 0.06, abs(uv.y - band_y));
    let noise = h2(vec2<f32>(floor(uv.y * 48.0), floor(frame.time * 12.0)));
    let skew = (noise - 0.5) * 0.004 * k;
    let shifted = sample(uv + vec2<f32>(skew, 0.0));
    let dim = 1.0 - 0.300 * k * line + 0.25 * k * band;
    let r = mix(c.r, shifted.r, 0.120);
    let b = mix(c.b, shifted.b, 0.120);
    return vec4<f32>(vec3<f32>(r, c.g, b) * dim, c.a);
}
