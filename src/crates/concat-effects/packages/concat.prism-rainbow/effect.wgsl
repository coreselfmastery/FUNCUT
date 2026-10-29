struct Params { amount: f32 }

fn effect(uv: vec2<f32>) -> vec4<f32> {
    let c = sample(uv);
    let k = params.amount / 100.0;
    let phase = uv.x + uv.y * 0.35 + frame.time * 0.06;
    let r = 0.5 + 0.5 * sin(phase * 6.2831);
    let g = 0.5 + 0.5 * sin(phase * 6.2831 + 2.0944);
    let b = 0.5 + 0.5 * sin(phase * 6.2831 + 4.1888);
    let wash = vec3<f32>(r, g, b) * smoothstep(0.0, 0.4, luma(c.rgb));
    return vec4<f32>(c.rgb + wash * k * 0.5, c.a);
}
