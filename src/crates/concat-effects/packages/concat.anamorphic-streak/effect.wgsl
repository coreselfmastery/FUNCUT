struct Params { amount: f32 }

fn effect(uv: vec2<f32>) -> vec4<f32> {
    let c = sample(uv);
    let k = params.amount / 100.0;
    let l = luma(c.rgb);
    let hot = smoothstep(0.45, 0.95, l);
    let streak = pow(1.0 - abs(uv.y - 0.5) * 2.0, 3.0);
    return vec4<f32>(c.rgb + vec3<f32>(0.5, 0.75, 1.4) * hot * streak * k, c.a);
}
