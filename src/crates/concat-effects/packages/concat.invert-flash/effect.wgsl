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

fn transition(uv: vec2<f32>, progress: f32) -> vec4<f32> {
    let p = 0.5 + (progress - 0.5) * 0.6;
    let k = params.amount / 100.0;
    let p_e = ease_out_expo(p);
    let a = from_at(uv);
    let b = to_at(uv);
    var c = mix(a, b, p_e);
    let tint = vec3<f32>(1.0, 1.0, 1.0);
    let pulse = sin(3.14159265359 * p) * k;
    c = vec4<f32>(mix(c.rgb, vec3<f32>(1.0) - c.rgb, clamp(pulse, 0.0, 1.0)), c.a);
    return c;
}
