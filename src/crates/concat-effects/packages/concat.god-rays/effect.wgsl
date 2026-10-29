struct Params { amount: f32 }

fn h2(p: vec2<f32>) -> f32 {
    return hash(floor(p), 0.0);
}

fn sector_of(uv: vec2<f32>) -> f32 {
    let c = uv - vec2<f32>(0.5);
    let ang = atan2(c.y, c.x);
    return floor(fract(ang / 3.14159265359 * 0.5 + 0.5) * 20.0 + 0.25);
}

fn effect(uv: vec2<f32>) -> vec4<f32> {
    let c = sample(uv);
    let k = params.amount / 100.0;
    let sector = sector_of(uv);
    let lit = h2(vec2<f32>(sector, 1.0)) * 0.6 + 0.4 * h2(vec2<f32>(sector, 2.0));
    let r = length(uv - vec2<f32>(0.5));
    let fall = 1.0 - smoothstep(0.1, 0.65, r);
    let flick = 0.8 + 0.2 * sin(frame.time * 1.7 + sector);
    let shaft = lit * fall * flick * k;
    return vec4<f32>(c.rgb + vec3<f32>(1.0, 0.92, 0.7) * shaft, c.a);
}
