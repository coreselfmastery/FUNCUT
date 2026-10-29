struct Params { amount: f32 }

fn effect(uv: vec2<f32>) -> vec4<f32> {
    let c = sample(uv);
    let k = params.amount / 100.0;
    let lines = max(frame.size.y / 3.0, 6.0);
    let s = 0.5 + 0.5 * sin(uv.y * lines * 3.14159265359 * 2.0);
    let dim = 1.0 - 0.350 * k * s;
    return vec4<f32>(c.rgb * dim, c.a);
}
