struct Params { amount: f32 }

fn effect(uv: vec2<f32>) -> vec4<f32> {
    let c = sample(uv);
    let k = params.amount / 100.0;
    let w = 0.0035 + 0.0035 * k;
    let line = 1.0 - smoothstep(0.0, w, abs(uv.x - 0.5));
    let glow = 1.0 - smoothstep(0.0, w * 6.0, abs(uv.x - 0.5));
    return vec4<f32>(c.rgb + vec3<f32>(1.0, 1.0, 1.0) * (line * 0.9 + glow * 0.15) * k, c.a);
}
