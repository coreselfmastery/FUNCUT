struct Params { size: f32, softness: f32, edge: f32 }

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

fn shape_sdf(p: vec2<f32>) -> f32 {{
    let bar_x = max(abs(p.x) - 0.22, abs(p.y) - 1.0);
    let bar_y = max(abs(p.x) - 1.0, abs(p.y) - 0.22);
    return min(bar_x, bar_y);
}}
fn transition(uv: vec2<f32>, progress: f32) -> vec4<f32> {
    let p = ease_out_elastic(progress);
    let r = max(p * (params.size / 100.0) * 1.5, 0.0001);
    let soft = params.softness / 100.0 * 0.2 + 0.02;
    let d = shape_sdf((uv - vec2<f32>(0.5)) / r) * r;
    let w = 1.0 - smoothstep(-soft, soft, d);
    var c = mix(from_at(uv), to_at(uv), w);

    let line = (1.0 - smoothstep(0.0, soft * 2.0, abs(d))) * (params.edge / 100.0);
    c = vec4<f32>(c.rgb + vec3<f32>(1.0, 1.0, 1.0) * line, c.a);
    return c;
}
