struct Params { amount: f32 }

fn effect(uv: vec2<f32>) -> vec4<f32> {
    let c = sample(uv);
    let k = params.amount / 100.0;
    let g = vec2<f32>(24.0, 18.0);
    let p = uv * g;
    let local = fract(p) - vec2<f32>(0.5);
    let d = length(local) * 2.2;
    let dot = 1.0 - smoothstep(0.4, 0.9, d);
    return vec4<f32>(mix(c.rgb, c.rgb * 0.6 + vec3<f32>(0.1, 0.12, 0.16), dot * k), c.a);
}
