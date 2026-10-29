struct Params { amount: f32 }

fn h2(p: vec2<f32>) -> f32 {
    return hash(floor(p), 0.0);
}

fn effect(uv: vec2<f32>) -> vec4<f32> {
    let c = sample(uv);
    let k = params.amount / 100.0;
    let g = 7.0;
    let drift = vec2<f32>(frame.time * 0.02, -frame.time * 0.013);
    let p = uv * g + drift;
    let cell = floor(p);
    let local = fract(p) - vec2<f32>(0.5);
    let h = h2(cell);
    let d = length(local) / (0.28 + h * 0.22);
    let disc = 1.0 - smoothstep(0.55, 1.0, d);
    let twinkle = 0.7 + 0.3 * sin(frame.time * (1.0 + h * 3.0) + h * 6.2831);
    return vec4<f32>(c.rgb + vec3<f32>(1.0, 0.95, 0.8) * disc * twinkle * k * 0.8, c.a);
}
