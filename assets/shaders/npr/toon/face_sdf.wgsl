fn face_sdf_mirrored_uv(uv: vec2<f32>, horizontal_light: f32, mirror_enabled: u32) -> vec2<f32> {
    var sample_uv = uv;
    if mirror_enabled != 0u && horizontal_light < 0.0 {
        sample_uv.x = 1.0 - sample_uv.x;
    }
    return sample_uv;
}

fn face_sdf_threshold(
    horizontal_light: f32,
    vertical_light: f32,
    horizontal_scale: f32,
    horizontal_bias: f32,
    vertical_influence: f32,
    backlight_clamp: f32,
    threshold_bias: f32,
) -> f32 {
    let side_amount = saturate(abs(horizontal_light) * horizontal_scale + horizontal_bias);
    let vertical_term = clamp(-vertical_light * vertical_influence, -0.3, 0.3);
    let backlight_term = saturate(-horizontal_light) * backlight_clamp;
    return clamp(0.18 + side_amount * 0.64 + vertical_term + backlight_term + threshold_bias, 0.0, 1.0);
}

fn face_sdf_procedural_sample(
    uv: vec2<f32>,
    terminator_softness: f32,
    vertical_curve: f32,
) -> f32 {
    let eye_band = exp(-pow((uv.y - 0.58) / 0.16, 2.0));
    let cheek_band = exp(-pow((uv.y - 0.40) / 0.20, 2.0));
    let chin_band = exp(-pow((uv.y - 0.18) / 0.16, 2.0));
    let brow_band = exp(-pow((uv.y - 0.82) / 0.14, 2.0));
    let nose_band = exp(-pow((uv.y - 0.50) / 0.18, 2.0));
    let center_pin = exp(-pow((uv.x - 0.50) / 0.12, 2.0));

    let cheek_push = cheek_band * 0.14;
    let chin_pull = chin_band * 0.08;
    let brow_pull = brow_band * 0.04;
    let nose_pull = nose_band * center_pin * 0.06;
    let horizontal_bias = mix(0.0, cheek_push - chin_pull - brow_pull - nose_pull, vertical_curve);

    let curved_u = uv.x + horizontal_bias;
    let horizontal = smoothstep(
        0.10 - terminator_softness,
        0.90 + terminator_softness,
        curved_u,
    );
    let forehead_to_chin = 1.0 - abs(uv.y * 2.0 - 1.0);
    let vertical_weight = mix(1.0, saturate(0.62 + forehead_to_chin * 0.32 + eye_band * 0.08), vertical_curve);
    return saturate(horizontal * vertical_weight);
}

fn face_sdf_lit_mask(sample_value: f32, threshold: f32, softness: f32) -> f32 {
    return smoothstep(threshold - softness, threshold + softness, sample_value);
}
