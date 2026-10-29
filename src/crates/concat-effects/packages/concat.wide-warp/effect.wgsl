struct Params { amount: f32 }

fn screen(base: vec3<f32>, over: vec3<f32>) -> vec3<f32> {
    let b = min(base, vec3<f32>(1.0));
    return vec3<f32>(1.0) - (vec3<f32>(1.0) - b) * (vec3<f32>(1.0) - over) + (base - b);
}
fn vignette(rgb: vec3<f32>, uv: vec2<f32>, amount: f32) -> vec3<f32> {
    let d = distance(uv, vec2<f32>(0.5)) * 1.4142;
    return rgb * (1.0 - smoothstep(0.35, 1.1, d) * amount);
}
fn mono(rgb: vec3<f32>, weights: vec3<f32>) -> vec3<f32> {
    let w = weights / max(weights.r + weights.g + weights.b, 0.001);
    return vec3<f32>(dot(rgb, w));
}
fn matte(rgb: vec3<f32>, black: f32, white: f32) -> vec3<f32> {
    return rgb * (white - black) + vec3<f32>(black);
}
fn hue_rotate(rgb: vec3<f32>, degrees: f32) -> vec3<f32> {
    let a = radians(degrees);
    let y = dot(rgb, vec3<f32>(0.299, 0.587, 0.114));
    let i = dot(rgb, vec3<f32>(0.596, -0.274, -0.322));
    let q = dot(rgb, vec3<f32>(0.211, -0.523, 0.312));
    let i2 = i * cos(a) - q * sin(a);
    let q2 = i * sin(a) + q * cos(a);
    return vec3<f32>(
        y + 0.956 * i2 + 0.621 * q2,
        y - 0.272 * i2 - 0.647 * q2,
        y - 1.106 * i2 + 1.703 * q2,
    );
}

fn effect(uv: vec2<f32>) -> vec4<f32> {
    let k = params.amount / 100.0;
    let d = uv - vec2<f32>(0.5);
    let r2 = dot(d, d);
    let f = 1.0 + r2 * k * 0.7 * 1.0;
    let c = sample(vec2<f32>(0.5) + d / f);
    var out_c = vignette(c.rgb, uv, 0.320 * k);
    return vec4<f32>(out_c, c.a);
}
