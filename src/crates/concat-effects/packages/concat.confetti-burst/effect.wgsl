struct Params { amount: f32 }

fn h2(p: vec2<f32>) -> f32 {
    return hash(floor(p), 0.0);
}

fn effect(uv: vec2<f32>) -> vec4<f32> {
    let c = sample(uv);
    let k = params.amount / 100.0;
    let g = 8.0;
    let p = vec2<f32>(uv.x * g + frame.time * 0.040, uv.y * g + frame.time * 0.200 * 1.0);
    let cell = floor(p);
    let local = fract(p) - vec2<f32>(0.5);
    let h = h2(cell);

    var tint = vec3<f32>(1.0, 0.25, 0.3);
    if (h > 0.66) {
        tint = vec3<f32>(0.3, 0.6, 1.0);
    } else if (h > 0.33) {
        tint = vec3<f32>(1.0, 0.85, 0.3);
    }
    let d = length(local) / (0.22 + h * 0.600);
    var dot_c = 1.0 - smoothstep(0.35, 0.95, d);
    dot_c *= 0.7 + 0.3 * sin(frame.time * (2.0 + h * 3.0) + h * 6.2831);
    return vec4<f32>(c.rgb + tint * dot_c * k, c.a);
}
