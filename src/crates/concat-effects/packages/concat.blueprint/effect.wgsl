struct Params { amount: f32 }

fn effect(uv: vec2<f32>) -> vec4<f32> {
    let c = sample(uv);
    let k = params.amount / 100.0;
    let g = 18.0;
    let local = fract(uv * g);
    let line = max(1.0 - smoothstep(0.0, 0.06, min(local.x, 1.0 - local.x)),
                   1.0 - smoothstep(0.0, 0.06, min(local.y, 1.0 - local.y)));
    let l = luma(c.rgb);
    var out_c = mix(vec3<f32>(0.16, 0.32, 0.62), vec3<f32>(0.95, 0.97, 1.0), line * 0.8);
    out_c = mix(c.rgb, out_c, k);
    out_c += vec3<f32>(l * 0.12 * k);
    return vec4<f32>(out_c, c.a);
}
