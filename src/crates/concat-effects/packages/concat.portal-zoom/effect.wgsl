struct Params { amount: f32 }

fn ease_smooth(x: f32) -> f32 { return smoothstep(0.0, 1.0, x); }
fn ease_out_expo(x: f32) -> f32 {
    if (x >= 1.0) { return 1.0; }
    return 1.0 - exp2(-9.0 * x);
}
fn ease_in_expo(x: f32) -> f32 {
    if (x <= 0.0) { return 0.0; }
    if (x >= 1.0) { return 1.0; }
    return exp2(10.0 * x - 10.0);
}
fn ease_out_back(x: f32) -> f32 {
    let c1 = 1.70158;
    let c3 = c1 + 1.0;
    let t = x - 1.0;
    return 1.0 + c3 * t * t * t + c1 * t * t;
}
fn ease_out_elastic(x: f32) -> f32 {
    if (x <= 0.0) { return 0.0; }
    if (x >= 1.0) { return 1.0; }
    let c4 = 6.28318530718 / 3.0;
    return exp2(-10.0 * x) * sin((x * 10.0 - 0.75) * c4) + 1.0;
}
fn ease_out_bounce(x: f32) -> f32 {
    let n1 = 7.5625;
    let d1 = 2.75;
    if (x < 1.0 / d1) { return n1 * x * x; }
    if (x < 2.0 / d1) {
        let t = x - 1.5 / d1;
        return n1 * t * t + 0.75;
    }
    if (x < 2.5 / d1) {
        let t = x - 2.25 / d1;
        return n1 * t * t + 0.9375;
    }
    let t = x - 2.625 / d1;
    return n1 * t * t + 0.984375;
}
fn ease_in_out_back(x: f32) -> f32 {
    let c2 = 1.70158 * 1.525;
    let t = x * 2.0;
    if (t < 1.0) {
        return (t * t * ((c2 + 1.0) * t - c2)) / 2.0;
    }
    let u = t - 2.0;
    return (u * u * ((c2 + 1.0) * u + c2)) / 2.0 + 1.0;
}
fn ease_out_quint(x: f32) -> f32 {
    let t = 1.0 - x;
    return 1.0 - t * t * t * t * t;
}

fn zoom_uv(uv: vec2<f32>, amount: f32) -> vec2<f32> {
    return (uv - vec2<f32>(0.5)) / max(amount, 0.0001) + vec2<f32>(0.5);
}
fn rot_uv(uv: vec2<f32>, radians: f32) -> vec2<f32> {
    let c = cos(radians);
    let s = sin(radians);
    let p = uv - vec2<f32>(0.5);
    return vec2<f32>(c * p.x - s * p.y, s * p.x + c * p.y) + vec2<f32>(0.5);
}

fn transition(uv: vec2<f32>, progress: f32) -> vec4<f32> {
    let k = params.amount / 100.0;
    let p = ease_out_expo(progress);
    let out_scale = mix(1.0, 1.55, p);
    let in_scale = mix(0.6, 1.0, ease_smooth(progress));
    var a = from_at(zoom_uv(uv, out_scale));
    var b = to_at(zoom_uv(uv, in_scale));
    var c = mix(a, b, p);
    let pulse = sin(3.14159265359 * progress) * 0.14 * k;
    c = vec4<f32>(c.rgb + vec3<f32>(1.0, 0.62, 0.24) * pulse, mix(a.a, b.a, p));
    return c;
}
