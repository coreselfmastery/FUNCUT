struct Params { amount: f32 }

fn thermal(c: vec3<f32>) -> vec3<f32> {
    let t = clamp(luma(c) * 1.25, 0.0, 1.0);
    let r = smoothstep(0.35, 0.75, t);
    let b = 1.0 - smoothstep(0.1, 0.45, t);
    let g = smoothstep(0.25, 0.55, t) * (1.0 - smoothstep(0.55, 0.9, t));
    return vec3<f32>(r, g * 0.85, b);
}

fn effect(uv: vec2<f32>) -> vec4<f32> {
    let c = sample(uv);
    let k = params.amount / 100.0;
    var out_c = thermal(c.rgb);
    out_c = mix(c.rgb, out_c, k);
    return vec4<f32>(out_c, c.a);
}
