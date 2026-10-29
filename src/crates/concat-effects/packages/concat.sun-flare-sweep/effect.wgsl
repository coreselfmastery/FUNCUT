struct Params { amount: f32 }

fn effect(uv: vec2<f32>) -> vec4<f32> {
    let c = sample(uv);
    let k = params.amount / 100.0;
    let d = distance(uv, vec2<f32>(0.12, 0.88));
    let sway = sin(frame.time * 0.30) * 0.08;
    let leak = 1.0 - smoothstep(0.1, 0.75, d + sway);
    return vec4<f32>(c.rgb + vec3<f32>(1.0, 0.72, 0.35) * leak * k, c.a);
}
