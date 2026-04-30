use std::{
    collections::{BTreeMap, HashMap},
    fs,
    path::{Path, PathBuf},
};

use bevy::prelude::*;
use ron::value::Value as RonValue;
use serde::{Deserialize, Deserializer, Serialize, Serializer};

use super::toon::{CharacterSurfaceParams, ToonMaterial, ToonMaterialData};

pub const CHARACTER_SURFACE_SHADER_KEY: &str = "character_surface";
pub const PROFILE_VERSION: u32 = 1;

pub struct CharacterRenderProfilePlugin;

impl Plugin for CharacterRenderProfilePlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<ShaderProfileRegistry>();
        app.world_mut()
            .resource_mut::<ShaderProfileRegistry>()
            .register(Box::new(CharacterSurfaceProfileHandler));
    }
}

#[derive(Resource, Default)]
pub struct ShaderProfileRegistry {
    handlers: HashMap<String, Box<dyn ShaderProfileHandler>>,
}

impl ShaderProfileRegistry {
    pub fn register(&mut self, handler: Box<dyn ShaderProfileHandler>) {
        self.handlers
            .insert(handler.shader_key().to_string(), handler);
    }

    pub fn get(&self, shader_key: &str) -> Option<&dyn ShaderProfileHandler> {
        self.handlers.get(shader_key).map(Box::as_ref)
    }
}

pub trait ShaderProfileHandler: Send + Sync {
    fn shader_key(&self) -> &'static str;
    fn capture_toon_material(&self, material: &ToonMaterial) -> Result<RonValue, String>;
    fn apply_to_toon_material(
        &self,
        params: &RonValue,
        resources: &RenderPartResources,
        material: &mut ToonMaterial,
        images: &mut Assets<Image>,
        asset_server: &AssetServer,
    ) -> Result<(), String>;
}

struct CharacterSurfaceProfileHandler;

impl ShaderProfileHandler for CharacterSurfaceProfileHandler {
    fn shader_key(&self) -> &'static str {
        CHARACTER_SURFACE_SHADER_KEY
    }

    fn capture_toon_material(&self, material: &ToonMaterial) -> Result<RonValue, String> {
        ron_value_from_serializable(&CharacterSurfaceContainerProfile::from_material(material))
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
        let character_surface_container =
            ron_value_into::<CharacterSurfaceContainerProfile>(params.clone())?;
        material.character_surface = CharacterSurfaceParams::from_profile(
            &character_surface_container,
            resources.region_mask_texture.is_some(),
        );
        character_surface_container
            .toon
            .apply_to_material(material, images);
        material.apply_render_part_resources(resources, asset_server);
        material.params.use_base_color_texture = use_base_color_texture;
        Ok(())
    }
}

#[derive(Clone, Serialize, Deserialize)]
pub struct CharacterSurfaceContainerProfile {
    pub toon: ToonMaterialData,
    #[serde(
        serialize_with = "serialize_region_mask_mode",
        deserialize_with = "deserialize_region_mask_mode"
    )]
    pub region_mask_mode: CharacterSurfaceRegionMaskMode,
    pub scene_interaction: SceneInteractionParams,
    pub region_params: CharacterSurfaceRegionParamSet,
}

impl CharacterSurfaceContainerProfile {
    pub fn from_material(material: &ToonMaterial) -> Self {
        Self {
            toon: ToonMaterialData::from_material(material),
            ..Default::default()
        }
    }
}

impl Default for CharacterSurfaceContainerProfile {
    fn default() -> Self {
        Self {
            toon: ToonMaterialData::default(),
            region_mask_mode: CharacterSurfaceRegionMaskMode::ChannelsRgba,
            scene_interaction: SceneInteractionParams::default(),
            region_params: CharacterSurfaceRegionParamSet::default(),
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum CharacterSurfaceRegionMaskMode {
    None,
    ChannelsRgba,
}

#[derive(Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum CharacterSurfaceRegionKind {
    Fabric,
    HardSurface,
    Metal,
    Leather,
}

fn serialize_region_mask_mode<S>(
    value: &CharacterSurfaceRegionMaskMode,
    serializer: S,
) -> Result<S::Ok, S::Error>
where
    S: Serializer,
{
    serializer.serialize_str(match value {
        CharacterSurfaceRegionMaskMode::None => "none",
        CharacterSurfaceRegionMaskMode::ChannelsRgba => "channels_rgba",
    })
}

fn deserialize_region_mask_mode<'de, D>(
    deserializer: D,
) -> Result<CharacterSurfaceRegionMaskMode, D::Error>
where
    D: Deserializer<'de>,
{
    let value = String::deserialize(deserializer)?;
    match value.as_str() {
        "none" => Ok(CharacterSurfaceRegionMaskMode::None),
        "channels_rgba" => Ok(CharacterSurfaceRegionMaskMode::ChannelsRgba),
        other => Err(serde::de::Error::custom(format!(
            "unknown region mask mode: {other}"
        ))),
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

#[derive(Clone, Serialize, Deserialize)]
pub struct CharacterSurfaceRegionParamSet {
    pub fabric: CharacterSurfaceRegionParams,
    pub hard_surface: CharacterSurfaceRegionParams,
    pub metal: CharacterSurfaceRegionParams,
    pub leather: CharacterSurfaceRegionParams,
}

impl Default for CharacterSurfaceRegionParamSet {
    fn default() -> Self {
        Self {
            fabric: CharacterSurfaceRegionParams {
                specular_boost: 0.15,
                rim_boost: 0.12,
                shadow_bias: 0.0,
                detail_normal_weight: 0.35,
            },
            hard_surface: CharacterSurfaceRegionParams {
                specular_boost: 0.65,
                rim_boost: 0.4,
                shadow_bias: 0.05,
                detail_normal_weight: 0.2,
            },
            metal: CharacterSurfaceRegionParams {
                specular_boost: 1.0,
                rim_boost: 0.55,
                shadow_bias: 0.08,
                detail_normal_weight: 0.1,
            },
            leather: CharacterSurfaceRegionParams {
                specular_boost: 0.45,
                rim_boost: 0.25,
                shadow_bias: 0.03,
                detail_normal_weight: 0.25,
            },
        }
    }
}

#[derive(Clone, Serialize, Deserialize)]
pub struct CharacterSurfaceRegionParams {
    pub specular_boost: f32,
    pub rim_boost: f32,
    pub shadow_bias: f32,
    pub detail_normal_weight: f32,
}

impl CharacterSurfaceParams {
    pub fn from_profile(
        profile: &CharacterSurfaceContainerProfile,
        has_region_mask_texture: bool,
    ) -> Self {
        let mask_enabled = matches!(
            profile.region_mask_mode,
            CharacterSurfaceRegionMaskMode::ChannelsRgba
        ) && has_region_mask_texture;
        Self {
            fabric: profile.region_params.fabric.into_runtime(),
            hard_surface: profile.region_params.hard_surface.into_runtime(),
            metal: profile.region_params.metal.into_runtime(),
            leather: profile.region_params.leather.into_runtime(),
            scene_primary: Vec4::new(
                profile.scene_interaction.direct_light_weight,
                profile.scene_interaction.env_light_weight,
                profile.scene_interaction.shadow_receive_weight,
                profile.scene_interaction.ambient_floor,
            ),
            // y 通道用作 shader 里的“是否启用 region mask”开关。
            scene_secondary: Vec4::new(
                profile.scene_interaction.light_color_influence,
                f32::from(mask_enabled),
                0.0,
                0.0,
            ),
        }
    }
}

impl CharacterSurfaceRegionParams {
    pub fn into_runtime(&self) -> Vec4 {
        Vec4::new(
            self.specular_boost,
            self.rim_boost,
            self.shadow_bias,
            self.detail_normal_weight,
        )
    }
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
    pub normal_texture: Option<String>,
    pub region_mask_texture: Option<String>,
    pub lighting_control_texture: Option<String>,
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
