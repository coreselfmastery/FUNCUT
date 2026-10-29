struct Params { amount: f32 }

fn effect(uv: vec2<f32>) -> vec4<f32> {
    let c = sample(uv);
    let k = params.amount / 100.0;
    let b = 0.045 * (0.4 + 0.6 * k);
    let edge = min(min(uv.x, 1.0 - uv.x), min(uv.y, 1.0 - uv.y));
    if (edge < b) {
        return vec4<f32>(mix(c.rgb, vec3<f32>(1.0, 0.98, 0.94), 0.85), c.a);
    }
    let shadow = 1.0 - smoothstep(b, b + 0.05, edge);
    return vec4<f32>(c.rgb * (1.0 - shadow * 0.12 * k), c.a);
}
