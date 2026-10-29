struct Params { amount: f32 }

fn h2(p: vec2<f32>) -> f32 {
    return hash(floor(p), 0.0);
}

fn effect(uv: vec2<f32>) -> vec4<f32> {
    let c = sample(uv);
    let k = params.amount / 100.0;
    let band_y = fract(0.7 - frame.time * 0.12);
    let band = 1.0 - smoothstep(0.0, 0.05, abs(uv.y - band_y));
    let row = floor(uv.y * 60.0);
    let noise = h2(vec2<f32>(row, floor(frame.time * 15.0)));
    let dirty = 1.0 - 0.320 * k * noise * band;
    let dim = 1.0 - 0.12 * k * (0.5 + 0.5 * sin(uv.y * 900.0));
    return vec4<f32>(c.rgb * dirty * dim + vec3<f32>(band * k * 0.18), c.a);
}
