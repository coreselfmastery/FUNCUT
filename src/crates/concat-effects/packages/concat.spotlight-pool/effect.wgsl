struct Params { amount: f32 }

fn effect(uv: vec2<f32>) -> vec4<f32> {
    let c = sample(uv);
    let k = params.amount / 100.0;
    let d = distance(uv, vec2<f32>(0.5, 0.45));
    let pool = 1.0 - smoothstep(0.15, 0.62, d);
    let lit = mix(1.0, pool * 1.35 + 0.05, k);
    return vec4<f32>(c.rgb * lit, c.a);
}
