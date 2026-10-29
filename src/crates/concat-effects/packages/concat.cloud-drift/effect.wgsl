struct Params { amount: f32 }

fn h2(p: vec2<f32>) -> f32 {
    return hash(floor(p), 0.0);
}

fn effect(uv: vec2<f32>) -> vec4<f32> {
    let c = sample(uv);
    let k = params.amount / 100.0;
    let g = 5.0;
    let p = uv * g + vec2<f32>(frame.time * 0.015, frame.time * 0.0090);
    let cell = floor(p);
    let local = fract(p) - vec2<f32>(0.5);
    let h = h2(cell);
    let h2v = h2(cell + vec2<f32>(1.0, 0.0));
    let w = clamp(local.x + 0.5, 0.0, 1.0);
    let n = mix(h, h2v, w);
    let lift = (n - 0.5) * 2.0;
    var out_c = c.rgb * (1.0 - 0.120 * k) + vec3<f32>(1.0, 0.98, 0.94) * lift * k * 0.35;
    return vec4<f32>(out_c, c.a);
}
