struct Params { amount: f32 }

fn h2(p: vec2<f32>) -> f32 {
    return hash(floor(p), 0.0);
}

fn effect(uv: vec2<f32>) -> vec4<f32> {
    let c = sample(uv);
    let k = params.amount / 100.0;
    let g = 14.0;
    let p = vec2<f32>(uv.x * g, uv.y * g + frame.time * 1.30 * 8.0);
    let cell = floor(p);
    let local = fract(p);
    let h = h2(cell);
    let on = step(0.6, h);
    let streak = (1.0 - smoothstep(0.0, 0.1, abs(local.x - 0.5))) * smoothstep(0.0, 0.3, local.y) * (1.0 - smoothstep(0.6, 1.0, local.y));
    return vec4<f32>(c.rgb + vec3<f32>(0.4, 0.9, 1.2) * streak * on * k * 0.55, c.a);
}
