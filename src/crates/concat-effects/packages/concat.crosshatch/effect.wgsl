struct Params { amount: f32 }

fn effect(uv: vec2<f32>) -> vec4<f32> {
    let c = sample(uv);
    let k = params.amount / 100.0;
    let d = (uv.x + uv.y) * 28.0;
    let line = 0.5 + 0.5 * sin(d * 3.14159265359);
    let l = luma(c.rgb);
    let density = (1.0 - smoothstep(0.1, 0.8, l)) * 0.8 + 0.1;
    let ink = line * density * k;
    var out_c = mix(c.rgb, c.rgb * 0.25, ink);
    return vec4<f32>(out_c, c.a);
}
