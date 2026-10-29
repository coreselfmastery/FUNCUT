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
    let p = ease_smooth(progress);
    let k = params.amount / 100.0;
    let env = sin(3.14159265359 * progress);
    let row = floor(uv.y * 6.0);
    let h = hash(vec2<f32>(row, 0.0), progress);
    let h2 = hash(vec2<f32>(row, 1.0), progress);
    let off = (h - 0.5) * 0.35 * k * env;
    let split_off = (h2 - 0.5) * 0.12 * k * env;
    let a = from_at(uv + vec2<f32>(off, 0.0));
    let b = to_at(uv + vec2<f32>(-off, 0.0));
    let r_ch = from_at(uv + vec2<f32>(off + split_off, 0.0)).r;
    let g_ch = to_at(uv + vec2<f32>(-off - split_off, 0.0)).g;
    var c = mix(a, b, p);
    c = vec4<f32>(mix(c.r, r_ch, 0.300), mix(c.g, g_ch, 0.300), c.b, mix(a.a, b.a, p));
    let scan = 1.0 - 0.500 * k * (h2 * 0.6 + 0.4) * env;
    return vec4<f32>(c.rgb * scan, c.a);
}
