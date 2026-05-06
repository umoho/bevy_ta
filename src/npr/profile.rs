use std::{
    collections::{BTreeMap, HashMap},
    fs,
    path::{Path, PathBuf},
};

use bevy::prelude::*;
use ron::value::Value as RonValue;
use serde::{Deserialize, Serialize};
use serde_json::{Map as JsonMap, Value as JsonValue, json};

use super::toon::{
    CharacterMaterialParams, FaceSdfParams, FaceSdfParamsData, ToonMaterial, ToonMaterialData,
    ToonParamsData,
};

pub const CHARACTER_SURFACE_SHADER_KEY: &str = "character_surface";
pub const CHARACTER_HAIR_SHADER_KEY: &str = "character_hair";
pub const CHARACTER_FACE_SDF_SHADER_KEY: &str = "character_face_sdf";
pub const PROFILE_VERSION: u32 = 1;

pub struct CharacterRenderProfilePlugin;

impl Plugin for CharacterRenderProfilePlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<ShaderProfileRegistry>();
        let mut registry = app.world_mut().resource_mut::<ShaderProfileRegistry>();
        registry.register(Box::new(CharacterMaterialProfileHandler {
            shader_key: CHARACTER_SURFACE_SHADER_KEY,
            use_face_sdf: false,
        }));
        registry.register(Box::new(CharacterMaterialProfileHandler {
            shader_key: CHARACTER_HAIR_SHADER_KEY,
            use_face_sdf: false,
        }));
        registry.register(Box::new(CharacterMaterialProfileHandler {
            shader_key: CHARACTER_FACE_SDF_SHADER_KEY,
            use_face_sdf: true,
        }));
    }
}

#[derive(Resource, Default)]
pub struct ShaderProfileRegistry {
    handlers: HashMap<String, Box<dyn ShaderProfileHandler>>,
}

impl ShaderProfileRegistry {
    /// 注册一个按 `shader_key` 分发的材质处理器。
    pub fn register(&mut self, handler: Box<dyn ShaderProfileHandler>) {
        self.handlers
            .insert(handler.shader_key().to_string(), handler);
    }

    /// 获取指定 `shader_key` 对应的材质处理器。
    pub fn get(&self, shader_key: &str) -> Option<&dyn ShaderProfileHandler> {
        self.handlers.get(shader_key).map(Box::as_ref)
    }

    /// 生成面向 MCP/BRP 的材质参数快照。
    pub fn snapshot_material_params(
        &self,
        shader_key: &str,
        material: &ToonMaterial,
    ) -> Result<JsonValue, String> {
        let mut params = snapshot_common_material_params(material);
        if let Some(handler) = self.get(shader_key) {
            params.extend(handler.snapshot_shader_params(material)?);
        }
        Ok(JsonValue::Object(params))
    }

    /// 按字段路径写入运行时材质参数。
    pub fn set_material_param(
        &self,
        shader_key: &str,
        material: &mut ToonMaterial,
        field: &str,
        value: &JsonValue,
    ) -> Result<(), String> {
        if set_common_material_field(material, field, value)? {
            return Ok(());
        }

        if let Some(handler) = self.get(shader_key)
            && handler.set_shader_field(material, field, value)?
        {
            return Ok(());
        }

        Err(format!(
            "不支持的材质字段 `{field}`，shader_key=`{shader_key}`。请先调用 list_material_params 查看可用参数。"
        ))
    }
}

pub trait ShaderProfileHandler: Send + Sync {
    /// 返回该处理器负责的 `shader_key`。
    fn shader_key(&self) -> &'static str;

    /// 从运行时材质捕获可持久化 profile 数据。
    fn capture_toon_material(&self, material: &ToonMaterial) -> Result<RonValue, String>;

    /// 将 profile 数据回写到运行时材质。
    fn apply_to_toon_material(
        &self,
        params: &RonValue,
        resources: &RenderPartResources,
        material: &mut ToonMaterial,
        images: &mut Assets<Image>,
        asset_server: &AssetServer,
    ) -> Result<(), String>;

    /// 生成 shader 专属的参数快照，供 MCP/BRP 列表接口返回。
    fn snapshot_shader_params(
        &self,
        _material: &ToonMaterial,
    ) -> Result<JsonMap<String, JsonValue>, String> {
        Ok(JsonMap::new())
    }

    /// 写入 shader 专属的参数字段；返回 `true` 表示已处理。
    fn set_shader_field(
        &self,
        _material: &mut ToonMaterial,
        _field: &str,
        _value: &JsonValue,
    ) -> Result<bool, String> {
        Ok(false)
    }
}

struct CharacterMaterialProfileHandler {
    shader_key: &'static str,
    use_face_sdf: bool,
}

impl ShaderProfileHandler for CharacterMaterialProfileHandler {
    fn shader_key(&self) -> &'static str {
        self.shader_key
    }

    fn capture_toon_material(&self, material: &ToonMaterial) -> Result<RonValue, String> {
        ron_value_from_serializable(&CharacterMaterialProfile::from_material(
            material,
            self.shader_key,
        ))
    }

    fn apply_to_toon_material(
        &self,
        params: &RonValue,
        resources: &RenderPartResources,
        material: &mut ToonMaterial,
        images: &mut Assets<Image>,
        asset_server: &AssetServer,
    ) -> Result<(), String> {
        let use_base_color_texture = material.params.use_base_color_texture;
        let profile = ron_value_into::<CharacterMaterialProfile>(params.clone())?;
        material.character_material =
            CharacterMaterialParams::from_profile(&profile, self.shader_key);
        material.face_sdf = if self.use_face_sdf {
            FaceSdfParams::from_profile(&profile, self.shader_key)
        } else {
            FaceSdfParams::default()
        };
        profile.toon.apply_to_material(material, images);
        material.apply_render_part_resources(resources, asset_server);
        material.params.use_base_color_texture = use_base_color_texture;
        Ok(())
    }

    fn snapshot_shader_params(
        &self,
        material: &ToonMaterial,
    ) -> Result<JsonMap<String, JsonValue>, String> {
        let mut params = JsonMap::new();
        if self.use_face_sdf {
            params.insert(
                "face_sdf".to_string(),
                json!(FaceSdfParamsData::from_runtime(
                    &material.face_sdf,
                    self.shader_key
                )),
            );
        }
        Ok(params)
    }

    fn set_shader_field(
        &self,
        material: &mut ToonMaterial,
        field: &str,
        value: &JsonValue,
    ) -> Result<bool, String> {
        if !self.use_face_sdf {
            return Ok(false);
        }

        let handled = match field {
            "face_sdf.enabled" => {
                material.face_sdf.enabled = value_bool_u32(value)?;
                true
            }
            "face_sdf.use_texture" => {
                material.face_sdf.texture_enabled = value_bool_u32(value)?;
                true
            }
            "face_sdf.uv_mirror_enabled" => {
                material.face_sdf.uv_mirror_enabled = value_bool_u32(value)?;
                true
            }
            "face_sdf.debug_mode" => {
                material.face_sdf.debug_mode = value_u32(value)?;
                true
            }
            "face_sdf.shadow_strength" => {
                material.face_sdf.shadow_strength = value_f32(value)?;
                true
            }
            "face_sdf.blend_weight" => {
                material.face_sdf.blend_weight = value_f32(value)?;
                true
            }
            "face_sdf.threshold_bias" => {
                material.face_sdf.threshold_bias = value_f32(value)?;
                true
            }
            "face_sdf.softness" => {
                material.face_sdf.softness = value_f32(value)?;
                true
            }
            "face_sdf.horizontal_scale" => {
                material.face_sdf.horizontal_scale = value_f32(value)?;
                true
            }
            "face_sdf.horizontal_bias" => {
                material.face_sdf.horizontal_bias = value_f32(value)?;
                true
            }
            "face_sdf.vertical_influence" => {
                material.face_sdf.vertical_influence = value_f32(value)?;
                true
            }
            "face_sdf.backlight_clamp" => {
                material.face_sdf.backlight_clamp = value_f32(value)?;
                true
            }
            "face_sdf.procedural_terminator_softness" => {
                material.face_sdf.procedural_terminator_softness = value_f32(value)?;
                true
            }
            "face_sdf.procedural_vertical_curve" => {
                material.face_sdf.procedural_vertical_curve = value_f32(value)?;
                true
            }
            _ => false,
        };

        Ok(handled)
    }
}

#[derive(Clone, Serialize, Deserialize)]
pub struct CharacterMaterialProfile {
    pub toon: ToonMaterialData,
    pub scene_interaction: SceneInteractionParams,
    pub shading: MaterialShadingParams,
    #[serde(default)]
    pub face_sdf: FaceSdfParamsData,
}

impl CharacterMaterialProfile {
    pub fn from_material(material: &ToonMaterial, shader_key: &str) -> Self {
        let mut profile = Self::default();
        profile.toon = ToonMaterialData::from_material(material);
        profile.scene_interaction =
            SceneInteractionParams::from_runtime(&material.character_material);
        profile.shading = MaterialShadingParams::from_runtime(&material.character_material);
        profile.face_sdf = FaceSdfParamsData::from_runtime(&material.face_sdf, shader_key);
        profile
    }
}

impl Default for CharacterMaterialProfile {
    fn default() -> Self {
        Self {
            toon: ToonMaterialData::default(),
            scene_interaction: SceneInteractionParams::default(),
            shading: MaterialShadingParams::default(),
            face_sdf: FaceSdfParamsData::default(),
        }
    }
}

#[derive(Clone, Serialize, Deserialize)]
pub struct SceneInteractionParams {
    pub direct_light_weight: f32,
    pub env_light_weight: f32,
    pub shadow_receive_weight: f32,
    pub ambient_floor: f32,
    pub light_color_influence: f32,
}

impl Default for SceneInteractionParams {
    fn default() -> Self {
        Self {
            direct_light_weight: 1.0,
            env_light_weight: 0.35,
            shadow_receive_weight: 0.65,
            ambient_floor: 0.12,
            light_color_influence: 0.35,
        }
    }
}

impl SceneInteractionParams {
    fn from_runtime(params: &CharacterMaterialParams) -> Self {
        Self {
            direct_light_weight: params.scene_primary.x,
            env_light_weight: params.scene_primary.y,
            shadow_receive_weight: params.scene_primary.z,
            ambient_floor: params.scene_primary.w,
            light_color_influence: params.shading_secondary.w,
        }
    }
}

#[derive(Clone, Serialize, Deserialize)]
pub struct MaterialShadingParams {
    pub specular_scale: f32,
    pub rim_scale: f32,
    pub shadow_offset: f32,
    pub shadow_softness_bias: f32,
    pub shadow_color_mix: f32,
    pub highlight_boost: f32,
}

impl Default for MaterialShadingParams {
    fn default() -> Self {
        Self {
            specular_scale: 1.0,
            rim_scale: 1.0,
            shadow_offset: 0.0,
            shadow_softness_bias: 0.0,
            shadow_color_mix: 0.0,
            highlight_boost: 0.0,
        }
    }
}

impl MaterialShadingParams {
    fn from_runtime(params: &CharacterMaterialParams) -> Self {
        Self {
            specular_scale: params.shading_primary.y,
            rim_scale: params.shading_primary.z,
            shadow_offset: params.shading_primary.w,
            shadow_softness_bias: params.shading_secondary.x,
            shadow_color_mix: params.shading_secondary.y,
            highlight_boost: params.shading_secondary.z,
        }
    }
}

impl CharacterMaterialParams {
    pub fn from_profile(profile: &CharacterMaterialProfile, _shader_key: &str) -> Self {
        Self {
            scene_primary: Vec4::new(
                profile.scene_interaction.direct_light_weight,
                profile.scene_interaction.env_light_weight,
                profile.scene_interaction.shadow_receive_weight,
                profile.scene_interaction.ambient_floor,
            ),
            shading_primary: Vec4::new(
                0.0,
                profile.shading.specular_scale,
                profile.shading.rim_scale,
                profile.shading.shadow_offset,
            ),
            shading_secondary: Vec4::new(
                profile.shading.shadow_softness_bias,
                profile.shading.shadow_color_mix,
                profile.shading.highlight_boost,
                profile.scene_interaction.light_color_influence,
            ),
        }
    }
}

impl FaceSdfParams {
    pub fn from_profile(profile: &CharacterMaterialProfile, shader_key: &str) -> Self {
        FaceSdfParamsData::clone_for_shader_key(&profile.face_sdf, shader_key).into_runtime()
    }
}

fn snapshot_common_material_params(material: &ToonMaterial) -> JsonMap<String, JsonValue> {
    let mut params = JsonMap::new();
    params.insert(
        "toon".to_string(),
        json!(ToonParamsData::from_runtime(&material.params)),
    );
    params.insert(
        "character_material".to_string(),
        json!({
            "scene_primary": material.character_material.scene_primary.to_array(),
            "shading_primary": material.character_material.shading_primary.to_array(),
            "shading_secondary": material.character_material.shading_secondary.to_array(),
        }),
    );
    params
}

fn set_common_material_field(
    material: &mut ToonMaterial,
    field: &str,
    value: &JsonValue,
) -> Result<bool, String> {
    let handled = match field {
        "toon.base_color" => {
            material.params.base_color = LinearRgba::from_f32_array(value_vec4(value)?);
            true
        }
        "toon.shade_threshold" => {
            material.params.shade_threshold = value_f32(value)?;
            true
        }
        "toon.shade_softness" => {
            material.params.shade_softness = value_f32(value)?;
            true
        }
        "toon.lit_boost" => {
            material.params.lit_boost = value_f32(value)?;
            true
        }
        "toon.shadow_strength" => {
            material.params.shadow_strength = value_f32(value)?;
            true
        }
        "toon.ambient_strength" => {
            material.params.ambient_strength = value_f32(value)?;
            true
        }
        "toon.specular_enabled" => {
            material.params.specular_enabled = value_bool_u32(value)?;
            true
        }
        "toon.specular_strength" => {
            material.params.specular_strength = value_f32(value)?;
            true
        }
        "toon.specular_threshold" => {
            material.params.specular_threshold = value_f32(value)?;
            true
        }
        "toon.specular_softness" => {
            material.params.specular_softness = value_f32(value)?;
            true
        }
        "toon.rim_enabled" => {
            material.params.rim_enabled = value_bool_u32(value)?;
            true
        }
        "toon.rim_strength" => {
            material.params.rim_strength = value_f32(value)?;
            true
        }
        "toon.rim_threshold" => {
            material.params.rim_threshold = value_f32(value)?;
            true
        }
        "toon.rim_softness" => {
            material.params.rim_softness = value_f32(value)?;
            true
        }
        "toon.outline_enabled" => {
            material.params.outline_enabled = value_bool_u32(value)?;
            true
        }
        "toon.outline_width" => {
            material.params.outline_width = value_f32(value)?;
            true
        }
        "toon.alpha_cutoff" => {
            material.params.alpha_cutoff = value_f32(value)?;
            true
        }
        "toon.specular_color" => {
            material.params.specular_color = LinearRgba::from_f32_array(value_vec4(value)?);
            true
        }
        "toon.rim_color" => {
            material.params.rim_color = LinearRgba::from_f32_array(value_vec4(value)?);
            true
        }
        "toon.outline_color" => {
            material.params.outline_color = LinearRgba::from_f32_array(value_vec4(value)?);
            true
        }
        "character_material.scene_primary" => {
            material.character_material.scene_primary = Vec4::from_array(value_vec4(value)?);
            true
        }
        "character_material.shading_primary" => {
            material.character_material.shading_primary = Vec4::from_array(value_vec4(value)?);
            true
        }
        "character_material.shading_secondary" => {
            material.character_material.shading_secondary = Vec4::from_array(value_vec4(value)?);
            true
        }
        _ => false,
    };

    Ok(handled)
}

fn value_f32(value: &JsonValue) -> Result<f32, String> {
    value
        .as_f64()
        .map(|value| value as f32)
        .filter(|value| value.is_finite())
        .ok_or_else(|| "期望有限浮点数".to_string())
}

fn value_u32(value: &JsonValue) -> Result<u32, String> {
    if let Some(value) = value
        .as_f64()
        .filter(|value| value.is_finite() && *value >= 0.0 && value.fract() == 0.0)
    {
        return u32::try_from(value as u64).map_err(|_| "期望非负整数".to_string());
    }

    value
        .as_u64()
        .and_then(|value| u32::try_from(value).ok())
        .ok_or_else(|| "期望非负整数".to_string())
}

fn value_bool_u32(value: &JsonValue) -> Result<u32, String> {
    if let Some(value) = value.as_bool() {
        return Ok(u32::from(value));
    }

    value
        .as_u64()
        .and_then(|value| u32::try_from(value).ok())
        .filter(|value| *value <= 1)
        .ok_or_else(|| "期望布尔值、0 或 1".to_string())
}

fn value_vec4(value: &JsonValue) -> Result<[f32; 4], String> {
    let values = value
        .as_array()
        .ok_or_else(|| "期望长度为 4 的数字数组".to_string())?;
    if values.len() != 4 {
        return Err("期望长度为 4 的数字数组".to_string());
    }

    let mut result = [0.0; 4];
    for (index, value) in values.iter().enumerate() {
        result[index] = value_f32(value)?;
    }
    Ok(result)
}

#[derive(Clone, Default, Serialize, Deserialize)]
pub struct CharacterRenderProfile {
    pub version: u32,
    pub model_binding: ModelBinding,
    pub shared: SharedRenderProfile,
    pub parts: Vec<RenderPartBinding>,
}

impl CharacterRenderProfile {
    pub fn load_from_path(path: &Path) -> Result<Self, String> {
        let content = fs::read_to_string(path)
            .map_err(|err| format!("failed to read {}: {err}", path.display()))?;
        ron::from_str(&content).map_err(|err| format!("failed to parse {}: {err}", path.display()))
    }

    pub fn load_for_scene_asset_path(scene_asset_path: &str) -> Result<Self, String> {
        let path = character_render_profile_path(scene_asset_path);
        Self::load_from_path(&path)
    }

    pub fn save_to_path(&self, path: &Path) -> Result<(), String> {
        let Some(parent) = path.parent() else {
            return Err(format!("invalid path: {}", path.display()));
        };
        fs::create_dir_all(parent)
            .map_err(|err| format!("failed to create {}: {err}", parent.display()))?;
        let ron = ron::ser::to_string_pretty(self, ron::ser::PrettyConfig::default())
            .map_err(|err| format!("failed to serialize render profile: {err}"))?;
        fs::write(path, ron).map_err(|err| format!("failed to write {}: {err}", path.display()))
    }

    pub fn upsert_part(&mut self, part: RenderPartBinding) {
        if let Some(existing) = self
            .parts
            .iter_mut()
            .find(|existing| existing.binding_key == part.binding_key)
        {
            *existing = part;
        } else {
            self.parts.push(part);
            self.parts
                .sort_by(|left, right| left.binding_key.cmp(&right.binding_key));
        }
    }

    pub fn find_part(&self, binding_key: &str) -> Option<&RenderPartBinding> {
        self.parts
            .iter()
            .find(|part| part.binding_key == binding_key)
    }
}

#[derive(Clone, Default, Serialize, Deserialize)]
pub struct ModelBinding {
    pub scene_asset_path: Option<String>,
}

#[derive(Clone, Default, Serialize, Deserialize)]
pub struct SharedRenderProfile {
    pub scalar_overrides: BTreeMap<String, f32>,
}

#[derive(Clone, Serialize, Deserialize)]
pub struct RenderPartBinding {
    pub binding_key: String,
    pub shader_key: String,
    pub resources: RenderPartResources,
    pub params: RonValue,
}

#[derive(Clone, Default, Serialize, Deserialize)]
pub struct RenderPartResources {
    pub base_color_texture: Option<String>,
    #[serde(default)]
    pub face_shadow_texture: Option<String>,
}

pub fn character_render_profile_path(scene_asset_path: &str) -> PathBuf {
    let source_path = scene_asset_path
        .split('#')
        .next()
        .unwrap_or(scene_asset_path);
    let source_path = Path::new(source_path);
    let file_stem = source_path
        .file_stem()
        .and_then(|stem| stem.to_str())
        .filter(|stem| !stem.is_empty())
        .unwrap_or("character");

    Path::new("assets").join(source_path.with_file_name(format!("{file_stem}.toon-model.ron")))
}

pub fn ron_value_from_serializable<T: Serialize>(value: &T) -> Result<RonValue, String> {
    let ron = ron::ser::to_string(value)
        .map_err(|err| format!("failed to encode params into ron value: {err}"))?;
    ron::from_str::<RonValue>(&ron)
        .map_err(|err| format!("failed to decode ron value from params: {err}"))
}

pub fn ron_value_into<T: for<'de> Deserialize<'de>>(value: RonValue) -> Result<T, String> {
    value
        .into_rust::<T>()
        .map_err(|err| format!("failed to decode params from ron value: {err}"))
}
