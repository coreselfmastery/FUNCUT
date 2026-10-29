struct Params { softness: f32, amount: f32 }

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

fn glsl_mod(a: f32, b: f32) -> f32 {
    return a - b * floor(a / b);
}

fn transition(uv: vec2<f32>, progress: f32) -> vec4<f32> {
    let p = ease_smooth(progress);
    let k = params.amount / 100.0;
    let soft = params.softness / 100.0 * 0.25 + 0.03;
    let cell = floor(uv.x * 9.0);
    let dir = 1.0;
    let band_w = 1.0 / 9.0;
    let local = fract(uv.x * 9.0);
    let stagger = glsl_mod(cell, 3.0) / 3.0 * 0.5;
    let b = mix(-stagger - soft, 1.0 + stagger + soft, p);
    let edge = select(b, 1.0 - b + stagger * 2.0, dir < 0.0);
    let w = smoothstep(edge - soft, edge + soft, local);
    let a = from_at(uv);
    let pic = to_at(uv);
    let c = mix(a, pic, w * k);
    let dim = 1.0 - 0.08 * k * (1.0 - abs(w * 2.0 - 1.0));
    return vec4<f32>(c.rgb * dim, mix(a.a, pic.a, w * k));
}
