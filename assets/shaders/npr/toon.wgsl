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

@group(#{MATERIAL_BIND_GROUP}) @binding(0) var<uniform> toon: ToonParams;
@group(#{MATERIAL_BIND_GROUP}) @binding(1) var base_color_texture: texture_2d<f32>;
@group(#{MATERIAL_BIND_GROUP}) @binding(2) var base_color_sampler: sampler;
@group(#{MATERIAL_BIND_GROUP}) @binding(3) var ramp_texture: texture_2d<f32>;
@group(#{MATERIAL_BIND_GROUP}) @binding(4) var ramp_sampler: sampler;

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

@fragment
fn fragment(in: VertexOutput, @builtin(front_facing) is_front: bool) -> @location(0) vec4<f32> {
    var base_color = sample_base_color(in);
    if base_color.a < toon.alpha_cutoff {
        discard;
    }

    var normal = normalize(in.world_normal);
    if !is_front {
        normal = -normal;
    }

    let light_dir = main_light_direction();
    let light_color = main_light_color();
    let view_dir = normalize(view_bindings::view.world_position.xyz - in.world_position.xyz);

    let ndotl = saturate(dot(normal, light_dir));
    let edge0 = saturate(toon.shade_threshold - toon.shade_softness);
    let edge1 = saturate(toon.shade_threshold + toon.shade_softness);
    let ramp_u = smoothstep(edge0, edge1, ndotl);
    let ramp_color = textureSample(ramp_texture, ramp_sampler, vec2<f32>(ramp_u, 0.5)).rgb;

    let ambient = view_bindings::lights.ambient_color.rgb * toon.ambient_strength;
    let shade = mix(vec3<f32>(1.0 - toon.shadow_strength), vec3<f32>(toon.lit_boost), ramp_u);
    var final_rgb = base_color.rgb * ramp_color * shade * light_color + base_color.rgb * ambient;

    let half_vec = normalize(light_dir + view_dir);
    let specular_base = saturate(dot(normal, half_vec));
    let specular_start = saturate(toon.specular_threshold - toon.specular_softness);
    let specular_end = saturate(toon.specular_threshold + toon.specular_softness);
    let specular_enabled = select(0.0, 1.0, toon.specular_enabled != 0u);
    let specular = smoothstep(specular_start, specular_end, specular_base)
        * toon.specular_strength
        * specular_enabled;
    final_rgb += toon.specular_color.rgb * specular;

    let rim_base = 1.0 - saturate(dot(normal, view_dir));
    let rim_start = saturate(toon.rim_threshold - toon.rim_softness);
    let rim_end = saturate(toon.rim_threshold + toon.rim_softness);
    let rim_enabled = select(0.0, 1.0, toon.rim_enabled != 0u);
    let rim = smoothstep(rim_start, rim_end, rim_base) * toon.rim_strength * rim_enabled;
    final_rgb += toon.rim_color.rgb * rim;

    return vec4<f32>(final_rgb, base_color.a);
}
