struct Params { amount: f32 }

fn h2(p: vec2<f32>) -> f32 {
    return hash(floor(p), 0.0);
}

fn effect(uv: vec2<f32>) -> vec4<f32> {
    let c = sample(uv);
    let k = params.amount / 100.0;
    let g = 8.0;
    let p = vec2<f32>(uv.x * g + frame.time * 0.030, uv.y * g + frame.time * 0.140 * -1.0);
    let cell = floor(p);
    let local = fract(p) - vec2<f32>(0.5);
    let h = h2(cell);
    let d = length(local) / (0.18 + h * 0.600);
    var dot_c = 1.0 - smoothstep(0.4, 1.0, d);
    dot_c *= 0.6 + 0.4 * sin(frame.time * (1.0 + h * 4.0) + h * 6.2831) * 0.600 + (1.0 - 0.600) * 0.4;
    return vec4<f32>(c.rgb + vec3<f32>(1.0, 0.45, 0.12) * dot_c * k, c.a);
}
