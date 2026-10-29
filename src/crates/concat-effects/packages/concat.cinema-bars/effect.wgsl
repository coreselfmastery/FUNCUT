struct Params { amount: f32 }

fn effect(uv: vec2<f32>) -> vec4<f32> {
    let c = sample(uv);
    let k = params.amount / 100.0;
    let bar = 0.2907949790794979;
    if (uv.y < bar || uv.y > 1.0 - bar) {
        return vec4<f32>(vec3<f32>(0.0), c.a);
    }
    let lift = 1.0 - 0.050 * k;
    return vec4<f32>(c.rgb * lift, c.a);
}
