struct Params { amount: f32 }

fn effect(uv: vec2<f32>) -> vec4<f32> {
    let c = sample(uv);
    let k = params.amount / 100.0;
    let swap = step(0.5, fract(frame.time * 0.400 + 0.600));
    let swapped = mix(c.rgb, c.gbr, swap);
    return vec4<f32>(mix(c.rgb, swapped, k), c.a);
}
