#import bevy_pbr::{
    forward_io::VertexOutput,
    mesh_view_bindings as view_bindings,
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

fn main_light_direction() -> vec3<f32> {
    if view_bindings::lights.n_directional_lights > 0u {
        return normalize(view_bindings::lights.directional_lights[0].direction_to_light);
    }

    // 没有主光时给一个稳定方向，方便空场景和材质预览也能看出明暗分区。
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
    let specular_scale = character_material.shading_primary.y;
    let rim_scale = character_material.shading_primary.z;
    let shadow_offset = character_material.shading_primary.w;
    let shadow_softness_bias = character_material.shading_secondary.x;
    let shadow_color_mix = character_material.shading_secondary.y;
    let highlight_boost = character_material.shading_secondary.z;
    let light_color_influence = character_material.shading_secondary.w;

    var direct_light_weight = character_material.scene_primary.x;
    var env_light_weight = character_material.scene_primary.y;
    var shadow_receive_weight = character_material.scene_primary.z;
    var ambient_floor = character_material.scene_primary.w;
    var shade_threshold = toon.shade_threshold;
    var shade_softness = toon.shade_softness + shadow_softness_bias;
    var specular_strength = toon.specular_strength * specular_scale;
    var rim_strength = toon.rim_strength * rim_scale;
    var ndotl = saturate(dot(normal, light_dir) + shadow_offset);

    let edge0 = saturate(shade_threshold - shade_softness);
    let edge1 = saturate(shade_threshold + shade_softness);
    let ramp_u = mix(1.0, smoothstep(edge0, edge1, ndotl), shadow_receive_weight);
    let ramp_color = textureSample(ramp_texture, ramp_sampler, vec2<f32>(ramp_u, 0.5)).rgb;

    let remapped_light_color = mix(vec3<f32>(1.0), light_color, light_color_influence);
    let ambient = max(
        view_bindings::lights.ambient_color.rgb * toon.ambient_strength * env_light_weight,
        vec3<f32>(ambient_floor),
    );
    let shade = mix(
        vec3<f32>(1.0 - toon.shadow_strength * shadow_receive_weight),
        vec3<f32>(toon.lit_boost + highlight_boost),
        ramp_u,
    );
    let shadow_tint = mix(vec3<f32>(1.0), vec3<f32>(1.08, 0.96, 0.94), shadow_color_mix);
    let shaded_base = mix(base_color.rgb * shadow_tint, base_color.rgb, ramp_u);

    var final_rgb = shaded_base * ramp_color * shade * remapped_light_color * direct_light_weight
        + base_color.rgb * ambient;

    let half_vec = normalize(light_dir + view_dir);
    let specular_base = saturate(dot(normal, half_vec));
    let specular_start = saturate(toon.specular_threshold - toon.specular_softness);
    let specular_end = saturate(toon.specular_threshold + toon.specular_softness);
    let specular_enabled = select(0.0, 1.0, toon.specular_enabled != 0u);
    let specular = smoothstep(specular_start, specular_end, specular_base)
        * specular_strength
        * specular_enabled;
    final_rgb += toon.specular_color.rgb * specular;

    let rim_base = 1.0 - saturate(dot(normal, view_dir));
    let rim_start = saturate(toon.rim_threshold - toon.rim_softness);
    let rim_end = saturate(toon.rim_threshold + toon.rim_softness);
    let rim_enabled = select(0.0, 1.0, toon.rim_enabled != 0u);
    let rim = smoothstep(rim_start, rim_end, rim_base)
        * rim_strength
        * rim_enabled;
    final_rgb += toon.rim_color.rgb * rim;

    return vec4<f32>(final_rgb, base_color.a);
}
