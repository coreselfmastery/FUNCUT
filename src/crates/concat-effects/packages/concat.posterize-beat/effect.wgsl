struct Params { amount: f32 }

fn effect(uv: vec2<f32>) -> vec4<f32> {
    let c = sample(uv);
    let k = params.amount / 100.0;
    let n = max(f32(4) - floor(frame.time * 1.20), 2.0);
    let step_l = 1.0 / n;
    var out_c = floor(c.rgb / step_l + 0.5) * step_l;
    out_c = mix(c.rgb, out_c, k);
    return vec4<f32>(out_c, c.a);
}
