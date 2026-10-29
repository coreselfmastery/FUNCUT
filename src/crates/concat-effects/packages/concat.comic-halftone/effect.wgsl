struct Params { amount: f32 }

fn effect(uv: vec2<f32>) -> vec4<f32> {
    let c = sample(uv);
    let k = params.amount / 100.0;
    let g = 10.0;
    let p = uv * g;
    let cell = floor(p);
    let local = fract(p) - vec2<f32>(0.5);
    let rot = vec2<f32>(local.x + local.y * 0.5, local.y - local.x * 0.5);
    let d = length(rot) * 2.0;
    let l = luma(c.rgb);
    var dotted = 1.0 - smoothstep(l * 0.9 - 0.08, l * 0.9 + 0.08, d);
    dotted = mix(1.0, dotted, k);
    var out_c = mix(c.rgb, vec3<f32>(0.02, 0.02, 0.05), dotted * k);
    out_c = mix(out_c, vec3<f32>(1.0, 0.99, 0.95), (1.0 - dotted) * k * l);
    return vec4<f32>(out_c, c.a);
}
