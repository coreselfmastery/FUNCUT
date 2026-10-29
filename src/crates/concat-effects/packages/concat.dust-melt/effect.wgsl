struct Params { amount: f32, softness: f32 }

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

fn vnoise(p: vec2<f32>, seed: f32) -> f32 {
    let cell = floor(p);
    let f = smoothstep(vec2<f32>(0.0), vec2<f32>(1.0), fract(p));
    let a = hash(cell, seed);
    let b = hash(cell + vec2<f32>(1.0, 0.0), seed);
    let c = hash(cell + vec2<f32>(0.0, 1.0), seed);
    let d = hash(cell + vec2<f32>(1.0, 1.0), seed);
    return mix(mix(a, b, f.x), mix(c, d, f.x), f.y);
}

fn transition(uv: vec2<f32>, progress: f32) -> vec4<f32> {
    let p = ease_smooth(progress);
    let k = params.amount / 100.0;
    let n = vnoise(uv * vec2<f32>(12.0), progress);
    let s = 0.250 + params.softness / 100.0 * 0.15;
    let w = smoothstep(n - s, n + s, p) * k;
    let a = from_at(uv);
    let b = to_at(uv);
    let c = mix(a, b, w);
    let dim = 1.0 - 0.12 * k * (1.0 - abs(w * 2.0 - 1.0));
    return vec4<f32>(c.rgb * dim, c.a);
}
