struct Params { amount: f32 }

fn h2(p: vec2<f32>) -> f32 {
    return hash(floor(p), 0.0);
}

fn effect(uv: vec2<f32>) -> vec4<f32> {
    let k = params.amount / 100.0;
    let t = floor(frame.time * 6.0);
    let cell = floor(uv * vec2<f32>(9.0, 6.0));
    let h = h2(cell + vec2<f32>(t, 0.0));
    let off = (h - 0.5) * 0.25 * k * step(0.55, h2(cell + vec2<f32>(t, 1.0)));
    let moved = uv + vec2<f32>(off, 0.0);
    let r = sample(moved + vec2<f32>((h - 0.5) * 0.02 * 0.70 * k, 0.0)).r;
    let g = sample(moved).g;
    let b = sample(moved - vec2<f32>((h - 0.5) * 0.02 * 0.70 * k, 0.0)).b;
    let dim = 1.0 - 0.300 * k * h;
    return vec4<f32>(vec3<f32>(r, g, b) * dim, sample(moved).a);
}
