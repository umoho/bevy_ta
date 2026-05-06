#import bevy_pbr::{
    forward_io::VertexOutput,
    mesh_view_bindings as view_bindings,
    mesh_view_types,
    shadows,
}
#import "shaders/npr/toon/face_sdf.wgsl" as face_sdf_module

struct ToonParams {
    base_color: vec4<f32>,
    shade_threshold: f32,
    shade_softness: f32,
    lit_boost: f32,
    shadow_strength: f32,
    ambient_strength: f32,
    specular_enabled: u32,
    specular_strength: f32,
    specular_threshold: f32,
    specular_softness: f32,
    rim_enabled: u32,
    rim_strength: f32,
    rim_threshold: f32,
    rim_softness: f32,
    outline_enabled: u32,
    outline_width: f32,
    use_base_color_texture: u32,
    alpha_cutoff: f32,
    specular_color: vec4<f32>,
    rim_color: vec4<f32>,
    outline_color: vec4<f32>,
}

struct CharacterMaterialParams {
    scene_primary: vec4<f32>,
    shading_primary: vec4<f32>,
    shading_secondary: vec4<f32>,
}

struct FaceSdfParams {
    enabled: u32,
    texture_enabled: u32,
    uv_mirror_enabled: u32,
    debug_mode: u32,
    specular_preserve: f32,
    shadow_strength: f32,
    blend_weight: f32,
    threshold_bias: f32,
    softness: f32,
    horizontal_scale: f32,
    horizontal_bias: f32,
    vertical_influence: f32,
    backlight_clamp: f32,
    procedural_terminator_softness: f32,
    procedural_vertical_curve: f32,
    reserved0: f32,
    reserved1: f32,
    face_forward: vec4<f32>,
    face_right: vec4<f32>,
    face_up: vec4<f32>,
}

@group(#{MATERIAL_BIND_GROUP}) @binding(0) var<uniform> toon: ToonParams;
@group(#{MATERIAL_BIND_GROUP}) @binding(1) var base_color_texture: texture_2d<f32>;
@group(#{MATERIAL_BIND_GROUP}) @binding(2) var base_color_sampler: sampler;
@group(#{MATERIAL_BIND_GROUP}) @binding(3) var ramp_texture: texture_2d<f32>;
@group(#{MATERIAL_BIND_GROUP}) @binding(4) var ramp_sampler: sampler;
@group(#{MATERIAL_BIND_GROUP}) @binding(5) var<uniform> character_material: CharacterMaterialParams;
@group(#{MATERIAL_BIND_GROUP}) @binding(6) var<uniform> face_sdf: FaceSdfParams;
@group(#{MATERIAL_BIND_GROUP}) @binding(7) var face_shadow_texture: texture_2d<f32>;
@group(#{MATERIAL_BIND_GROUP}) @binding(8) var face_shadow_sampler: sampler;

fn main_light_direction() -> vec3<f32> {
    if view_bindings::lights.n_directional_lights > 0u {
        return normalize(view_bindings::lights.directional_lights[0].direction_to_light);
    }

    return normalize(vec3<f32>(-0.35, 0.8, 0.45));
}

fn main_light_color() -> vec3<f32> {
    if view_bindings::lights.n_directional_lights > 0u {
        let raw_color = view_bindings::lights.directional_lights[0].color.rgb;
        let peak = max(max(raw_color.r, raw_color.g), raw_color.b);
        if peak > 0.0 {
            return raw_color / peak;
        }
    }
    return vec3<f32>(1.0);
}

fn direction_world_to_view(direction: vec3<f32>) -> vec3<f32> {
    return normalize((view_bindings::view.view_from_world * vec4<f32>(direction, 0.0)).xyz);
}

fn safe_normalize2(value: vec2<f32>) -> vec2<f32> {
    let len = length(value);
    if len > 0.0001 {
        return value / len;
    }
    return vec2<f32>(0.0);
}

fn main_light_screen_rim_mask(normal: vec3<f32>, light_dir: vec3<f32>) -> f32 {
    let normal_view = direction_world_to_view(normal);
    let light_view = direction_world_to_view(light_dir);
    let light_screen_len = length(light_view.xy);

    if light_screen_len <= 0.0001 {
        return 1.0;
    }

    let alignment = dot(safe_normalize2(normal_view.xy), light_view.xy / light_screen_len);
    return smoothstep(-0.25, 0.75, alignment);
}

fn main_directional_shadow(in: VertexOutput, normal: vec3<f32>) -> f32 {
    if view_bindings::lights.n_directional_lights == 0u {
        return 1.0;
    }

    let main_light = view_bindings::lights.directional_lights[0u];
    if (main_light.flags & mesh_view_types::DIRECTIONAL_LIGHT_FLAGS_SHADOWS_ENABLED_BIT) == 0u {
        return 1.0;
    }

    let view_position = view_bindings::view.view_from_world * in.world_position;
    return shadows::fetch_directional_shadow(0u, in.world_position, normal, view_position.z);
}

fn sample_base_color(in: VertexOutput) -> vec4<f32> {
    var color = toon.base_color;

#ifdef VERTEX_UVS_A
    if toon.use_base_color_texture != 0u {
        color *= textureSample(base_color_texture, base_color_sampler, in.uv);
    }
#endif

#ifdef VERTEX_COLORS
    color *= in.color;
#endif

    return color;
}

fn sample_world_normal(in: VertexOutput, is_front: bool) -> vec3<f32> {
    var normal = normalize(in.world_normal);

    if !is_front {
        normal = -normal;
    }

    return normalize(normal);
}

fn toon_ramp_u(normal: vec3<f32>, light_dir: vec3<f32>) -> f32 {
    let shadow_offset = character_material.shading_primary.w;
    let shadow_softness_bias = character_material.shading_secondary.x;
    let shadow_receive_weight = character_material.scene_primary.z;
    let shade_softness = toon.shade_softness + shadow_softness_bias;
    let ndotl = saturate(dot(normal, light_dir) + shadow_offset);
    let edge0 = saturate(toon.shade_threshold - shade_softness);
    let edge1 = saturate(toon.shade_threshold + shade_softness);

    return mix(1.0, smoothstep(edge0, edge1, ndotl), shadow_receive_weight);
}

fn sample_ramp_color(ramp_u: f32) -> vec3<f32> {
    return textureSample(ramp_texture, ramp_sampler, vec2<f32>(ramp_u, 0.5)).rgb;
}

fn toon_ambient() -> vec3<f32> {
    let env_light_weight = character_material.scene_primary.y;
    let ambient_floor = character_material.scene_primary.w;

    return max(
        view_bindings::lights.ambient_color.rgb * toon.ambient_strength * env_light_weight,
        vec3<f32>(ambient_floor),
    );
}

fn toon_lit_base(
    base_rgb: vec3<f32>,
    ramp_u: f32,
    ramp_color: vec3<f32>,
    light_color: vec3<f32>,
) -> vec3<f32> {
    let direct_light_weight = character_material.scene_primary.x;
    let shadow_receive_weight = character_material.scene_primary.z;
    let shadow_color_mix = character_material.shading_secondary.y;
    let highlight_boost = character_material.shading_secondary.z;
    let light_color_influence = character_material.shading_secondary.w;

    let remapped_light_color = mix(vec3<f32>(1.0), light_color, light_color_influence);
    let shade = mix(
        vec3<f32>(1.0 - toon.shadow_strength * shadow_receive_weight),
        vec3<f32>(toon.lit_boost + highlight_boost),
        ramp_u,
    );
    let shadow_tint = mix(vec3<f32>(1.0), vec3<f32>(1.08, 0.96, 0.94), shadow_color_mix);
    let shaded_base = mix(base_rgb * shadow_tint, base_rgb, ramp_u);

    return shaded_base * ramp_color * shade * remapped_light_color * direct_light_weight
        + base_rgb * toon_ambient();
}

fn toon_specular(normal: vec3<f32>, light_dir: vec3<f32>, view_dir: vec3<f32>) -> vec3<f32> {
    let specular_scale = character_material.shading_primary.y;
    let specular_strength = toon.specular_strength * specular_scale;
    let face_specular_scale = select(1.0, face_sdf.specular_preserve, face_sdf.enabled != 0u);
    let half_vec = normalize(light_dir + view_dir);
    let specular_base = saturate(dot(normal, half_vec));
    let specular_start = saturate(toon.specular_threshold - toon.specular_softness);
    let specular_end = saturate(toon.specular_threshold + toon.specular_softness);
    let specular_enabled = select(0.0, 1.0, toon.specular_enabled != 0u);
    let specular = smoothstep(specular_start, specular_end, specular_base)
        * specular_strength
        * face_specular_scale
        * specular_enabled;

    return toon.specular_color.rgb * specular;
}

fn toon_rim(
    in: VertexOutput,
    normal: vec3<f32>,
    light_dir: vec3<f32>,
    view_dir: vec3<f32>,
) -> vec3<f32> {
    let rim_scale = character_material.shading_primary.z;
    let shadow_receive_weight = character_material.scene_primary.z;
    let rim_strength = toon.rim_strength * rim_scale;
    let rim_base = 1.0 - saturate(dot(normal, view_dir));
    let rim_start = saturate(toon.rim_threshold - toon.rim_softness);
    let rim_end = saturate(toon.rim_threshold + toon.rim_softness);
    let rim_enabled = select(0.0, 1.0, toon.rim_enabled != 0u);
    let rim_light_mask = main_light_screen_rim_mask(normal, light_dir);
    let rim_shadow = mix(1.0, main_directional_shadow(in, normal), shadow_receive_weight);
    let rim = smoothstep(rim_start, rim_end, rim_base)
        * rim_light_mask
        * rim_shadow
        * rim_strength
        * rim_enabled;

    return toon.rim_color.rgb * rim;
}

fn face_sdf_ramp_override(
    in: VertexOutput,
    base_ramp_u: f32,
    light_dir: vec3<f32>,
) -> vec4<f32> {
    let face_forward = normalize(face_sdf.face_forward.xyz);
    let face_right = normalize(face_sdf.face_right.xyz);
    let face_up = normalize(face_sdf.face_up.xyz);
    let face_light = vec3<f32>(
        dot(light_dir, face_right),
        dot(light_dir, face_up),
        dot(light_dir, face_forward),
    );
    let face_uv = face_sdf_module::face_sdf_mirrored_uv(in.uv, face_light.x, face_sdf.uv_mirror_enabled);

    var face_sample = face_sdf_module::face_sdf_procedural_sample(
        face_uv,
        face_sdf.procedural_terminator_softness,
        face_sdf.procedural_vertical_curve,
    );
    if face_sdf.texture_enabled != 0u {
        face_sample = textureSample(face_shadow_texture, face_shadow_sampler, face_uv).r;
    }

    let threshold = face_sdf_module::face_sdf_threshold(
        face_light.x,
        face_light.y,
        face_sdf.horizontal_scale,
        face_sdf.horizontal_bias,
        face_sdf.vertical_influence,
        face_sdf.backlight_clamp,
        face_sdf.threshold_bias,
    );
    let face_lit = face_sdf_module::face_sdf_lit_mask(face_sample, threshold, face_sdf.softness);
    let face_shadow_floor = 1.0 - face_sdf.shadow_strength;
    let face_ramp_u = mix(face_shadow_floor, 1.0, face_lit);
    let resolved_ramp_u = mix(base_ramp_u, face_ramp_u, face_sdf.blend_weight);

    if face_sdf.debug_mode == 1u {
        return vec4<f32>(vec3<f32>(face_sample), 1.0);
    }
    if face_sdf.debug_mode == 2u {
        return vec4<f32>(vec3<f32>(threshold), 1.0);
    }
    if face_sdf.debug_mode == 3u {
        return vec4<f32>(vec3<f32>(face_lit), 1.0);
    }

    return vec4<f32>(vec3<f32>(resolved_ramp_u), 0.0);
}

@fragment
fn fragment(in: VertexOutput, @builtin(front_facing) is_front: bool) -> @location(0) vec4<f32> {
    var base_color = sample_base_color(in);
    if base_color.a < toon.alpha_cutoff {
        discard;
    }

    let normal = sample_world_normal(in, is_front);

    let light_dir = main_light_direction();
    let light_color = main_light_color();
    let view_dir = normalize(view_bindings::view.world_position.xyz - in.world_position.xyz);

    var ramp_u = toon_ramp_u(normal, light_dir);
    if face_sdf.enabled != 0u {
        let face_override = face_sdf_ramp_override(in, ramp_u, light_dir);
        if face_override.a > 0.5 {
            return face_override;
        }
        ramp_u = face_override.r;
    }

    let ramp_color = sample_ramp_color(ramp_u);
    var final_rgb = toon_lit_base(base_color.rgb, ramp_u, ramp_color, light_color);
    final_rgb += toon_specular(normal, light_dir, view_dir);
    final_rgb += toon_rim(in, normal, light_dir, view_dir);

    return vec4<f32>(final_rgb, base_color.a);
}
