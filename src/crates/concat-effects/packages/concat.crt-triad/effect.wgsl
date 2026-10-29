struct Params { amount: f32 }

fn glsl_mod(a: f32, b: f32) -> f32 {
    return a - b * floor(a / b);
}

fn effect(uv: vec2<f32>) -> vec4<f32> {
    let c = sample(uv);
    let k = params.amount / 100.0;
    let stripes = max(frame.size.x / 3.0, 1.0);
    let which = glsl_mod(floor(uv.x * stripes), 3.0);
    var mask = vec3<f32>(0.55);
    if (which < 0.5) {
        mask = vec3<f32>(1.0, 0.45, 0.45);
    } else if (which < 1.5) {
        mask = vec3<f32>(0.45, 1.0, 0.45);
    } else {
        mask = vec3<f32>(0.45, 0.45, 1.0);
    }
    let m = mix(vec3<f32>(1.0), mask, k);
    return vec4<f32>(c.rgb * m * (1.0 - 0.280 * k * 0.25), c.a);
}
