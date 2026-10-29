struct Params { amount: f32 }

fn effect(uv: vec2<f32>) -> vec4<f32> {
    let c = sample(uv);
    let k = params.amount / 100.0;
    let t = frame.time * 1.60 + 0.800;
    let r = c.r * (1.0 + sin(t) * 0.300 * k);
    let g = c.g * (1.0 + sin(t + 2.0944) * 0.050 * k);
    let b = c.b * (1.0 + sin(t + 4.1888) * -0.250 * k);
    return vec4<f32>(r, g, b, c.a);
}
