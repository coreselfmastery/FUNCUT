struct Params { amount: f32 }

fn h2(p: vec2<f32>) -> f32 {
    return hash(floor(p), 0.0);
}

fn effect(uv: vec2<f32>) -> vec4<f32> {
    let c = sample(uv);
    let k = params.amount / 100.0;
    let col = floor(uv.x * 90.0);
    let h = h2(vec2<f32>(col, floor(frame.time * 8.0)));
    let on = step(0.93, h);
    let x = fract(uv.x * 90.0);
    var line = 1.0 - smoothstep(0.0, 0.25, min(x, 1.0 - x));
    line *= on;
    return vec4<f32>(c.rgb + vec3<f32>(1.0, 0.97, 0.9) * line * k * 0.35, c.a);
}
