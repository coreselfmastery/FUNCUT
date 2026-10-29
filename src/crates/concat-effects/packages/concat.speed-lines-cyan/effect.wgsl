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
    let h = h2(vec2<f32>(sector, 7.0));
    let r = length(uv - vec2<f32>(0.5));
    let fade = smoothstep(0.05, 0.25, r) * (1.0 - smoothstep(0.55, 0.95, r));
    let surge = 0.75 + 0.25 * sin(frame.time * 6.0 + h * 6.2831);
    let streak = step(0.62, h) * fade * surge;
    return vec4<f32>(c.rgb + vec3<f32>(0.4, 1.0, 1.3) * streak * k * 0.9, c.a);
}
