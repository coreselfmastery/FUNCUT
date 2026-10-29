struct Params { amount: f32 }

fn effect(uv: vec2<f32>) -> vec4<f32> {
    let c = sample(uv);
    let k = params.amount / 100.0;
    let l = luma(c.rgb);
    let hot = smoothstep(0.5, 1.0, l);
    let d = uv - vec2<f32>(0.5);
    let spike = pow(max(1.0 - abs(d.x) * 2.2, 0.0), 2.0) + pow(max(1.0 - abs(d.y) * 2.2, 0.0), 2.0);
    return vec4<f32>(c.rgb + vec3<f32>(1.0, 1.0, 1.0) * hot * spike * k, c.a);
}
