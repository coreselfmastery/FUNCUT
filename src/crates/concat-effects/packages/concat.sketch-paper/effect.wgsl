struct Params { amount: f32 }

fn effect(uv: vec2<f32>) -> vec4<f32> {
    let c = sample(uv);
    let k = params.amount / 100.0;
    let g = hash(floor(uv * frame.size), fract(frame.time * 7.31)) - 0.5;
    let l = luma(c.rgb);
    var out_c = mix(c.rgb, vec3<f32>(0.93, 0.91, 0.86), 0.85 * k);
    out_c += vec3<f32>(g * 0.04 * k);
    out_c = mix(c.rgb, out_c, k);
    let shade = mix(1.0, 0.55 + l * 0.45, k);
    return vec4<f32>(out_c * shade, c.a);
}
