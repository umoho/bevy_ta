#import bevy_pbr::{
    forward_io::VertexOutput,
    mesh_view_bindings as view_bindings,
    mesh_view_types,
    shadows,
}

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

@group(#{MATERIAL_BIND_GROUP}) @binding(0) var<uniform> toon: ToonParams;
@group(#{MATERIAL_BIND_GROUP}) @binding(1) var base_color_texture: texture_2d<f32>;
@group(#{MATERIAL_BIND_GROUP}) @binding(2) var base_color_sampler: sampler;
@group(#{MATERIAL_BIND_GROUP}) @binding(3) var ramp_texture: texture_2d<f32>;
@group(#{MATERIAL_BIND_GROUP}) @binding(4) var ramp_sampler: sampler;
@group(#{MATERIAL_BIND_GROUP}) @binding(5) var<uniform> character_material: CharacterMaterialParams;

/// 返回角色卡通着色使用的主方向光方向；没有方向光时使用稳定的预览方向。
fn main_light_direction() -> vec3<f32> {
    if view_bindings::lights.n_directional_lights > 0u {
        return normalize(view_bindings::lights.directional_lights[0].direction_to_light);
    }

    // 没有主光时给一个稳定方向，方便空场景和材质预览也能看出明暗分区。
    return normalize(vec3<f32>(-0.35, 0.8, 0.45));
}

/// 返回归一化后的主方向光颜色，避免光强数值直接改变材质色相。
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

/// 将世界空间方向转换到观察空间，供屏幕方向相关的效果使用。
fn direction_world_to_view(direction: vec3<f32>) -> vec3<f32> {
    return normalize((view_bindings::view.view_from_world * vec4<f32>(direction, 0.0)).xyz);
}

/// 安全归一化二维向量，避免零长度方向在边缘光 mask 中产生异常。
fn safe_normalize2(value: vec2<f32>) -> vec2<f32> {
    let len = length(value);
    if len > 0.0001 {
        return value / len;
    }
    return vec2<f32>(0.0);
}

/// 根据主光在屏幕上的方向，压制背离主光一侧的普通 Fresnel 边缘光。
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

/// 采样主方向光的 shadow factor；没有方向光或未开阴影时返回全亮。
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

/// 采样材质基础色，并叠加基础色贴图和顶点色。
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

/// 获取世界空间法线，并在双面片元背面时翻转法线。
fn sample_world_normal(in: VertexOutput, is_front: bool) -> vec3<f32> {
    var normal = normalize(in.world_normal);

    if !is_front {
        normal = -normal;
    }

    return normalize(normal);
}

/// 计算 ramp 采样横坐标，包含主光 NdotL、阴影偏移、柔和度和阴影接收权重。
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

/// 从 ramp 纹理读取当前光照阶调颜色。
fn sample_ramp_color(ramp_u: f32) -> vec3<f32> {
    return textureSample(ramp_texture, ramp_sampler, vec2<f32>(ramp_u, 0.5)).rgb;
}

/// 计算环境光，并用材质地板值保证暗部不会完全压黑。
fn toon_ambient() -> vec3<f32> {
    let env_light_weight = character_material.scene_primary.y;
    let ambient_floor = character_material.scene_primary.w;

    return max(
        view_bindings::lights.ambient_color.rgb * toon.ambient_strength * env_light_weight,
        vec3<f32>(ambient_floor),
    );
}

/// 计算 toon 基础受光颜色，包括 ramp、亮暗强度、主光颜色影响和阴影染色。
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

/// 计算硬阈值高光，并返回可直接叠加到最终颜色的高光颜色。
fn toon_specular(normal: vec3<f32>, light_dir: vec3<f32>, view_dir: vec3<f32>) -> vec3<f32> {
    let specular_scale = character_material.shading_primary.y;
    let specular_strength = toon.specular_strength * specular_scale;
    let half_vec = normalize(light_dir + view_dir);
    let specular_base = saturate(dot(normal, half_vec));
    let specular_start = saturate(toon.specular_threshold - toon.specular_softness);
    let specular_end = saturate(toon.specular_threshold + toon.specular_softness);
    let specular_enabled = select(0.0, 1.0, toon.specular_enabled != 0u);
    let specular = smoothstep(specular_start, specular_end, specular_base)
        * specular_strength
        * specular_enabled;

    return toon.specular_color.rgb * specular;
}

/// 计算受主光屏幕方向和主光阴影共同控制的 Fresnel 边缘光。
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

/// 片元入口：组合基础 toon 受光、高光和边缘光。
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

    let ramp_u = toon_ramp_u(normal, light_dir);
    let ramp_color = sample_ramp_color(ramp_u);
    var final_rgb = toon_lit_base(base_color.rgb, ramp_u, ramp_color, light_color);
    final_rgb += toon_specular(normal, light_dir, view_dir);
    final_rgb += toon_rim(in, normal, light_dir, view_dir);

    return vec4<f32>(final_rgb, base_color.a);
}
