struct Params { amount: f32 }

fn effect(uv: vec2<f32>) -> vec4<f32> {
    let k = params.amount / 100.0;
    let g = max(frame.size.x / 24.0, 4.0);
    let p = floor(uv * frame.size / g) * g / frame.size;
    let c = sample(p);
    let local = fract(uv * frame.size / g);
    let line = max(1.0 - smoothstep(0.0, 0.09, min(local.x, 1.0 - local.x)),
                   1.0 - smoothstep(0.0, 0.09, min(local.y, 1.0 - local.y)));
    return vec4<f32>(c.rgb * (1.0 - line * 0.1 * k), c.a);
}
