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
    let lines = step(0.45, h) * smoothstep(0.03, 0.3, r);
    let p = uv * 16.0;
    let local = fract(p) - vec2<f32>(0.5);
    let d = length(local) * 2.0;
    let dots = 1.0 - smoothstep(0.6, 0.9, d);
    var out_c = c.rgb * (1.0 - 0.3 * k * dots);
    out_c += vec3<f32>(1.0, 1.0, 1.0) * lines * k * 0.7;
    return vec4<f32>(out_c, c.a);
}
