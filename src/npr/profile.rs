use std::{
    collections::{BTreeMap, HashMap},
    fs,
    path::{Path, PathBuf},
};

use bevy::prelude::*;
use ron::value::Value as RonValue;
use serde::{Deserialize, Serialize};

use super::toon::{CharacterMaterialParams, ToonMaterial, ToonMaterialData};

pub const CHARACTER_SURFACE_SHADER_KEY: &str = "character_surface";
pub const PROFILE_VERSION: u32 = 1;

pub struct CharacterRenderProfilePlugin;

impl Plugin for CharacterRenderProfilePlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<ShaderProfileRegistry>();
        app.world_mut()
            .resource_mut::<ShaderProfileRegistry>()
            .register(Box::new(CharacterMaterialProfileHandler(
                CHARACTER_SURFACE_SHADER_KEY,
            )));
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

struct CharacterMaterialProfileHandler(&'static str);

impl ShaderProfileHandler for CharacterMaterialProfileHandler {
    fn shader_key(&self) -> &'static str {
        self.0
    }

    fn capture_toon_material(&self, material: &ToonMaterial) -> Result<RonValue, String> {
        ron_value_from_serializable(&CharacterMaterialProfile::from_material(
            material,
            CHARACTER_SURFACE_SHADER_KEY,
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
            CharacterMaterialParams::from_profile(&profile, CHARACTER_SURFACE_SHADER_KEY);
        profile.toon.apply_to_material(material, images);
        material.apply_render_part_resources(resources, asset_server);
        material.params.use_base_color_texture = use_base_color_texture;
        Ok(())
    }
}

#[derive(Clone, Serialize, Deserialize)]
pub struct CharacterMaterialProfile {
    pub toon: ToonMaterialData,
    pub scene_interaction: SceneInteractionParams,
    pub shading: MaterialShadingParams,
}

impl CharacterMaterialProfile {
    pub fn from_material(material: &ToonMaterial, _shader_key: &str) -> Self {
        let mut profile = Self::default();
        profile.toon = ToonMaterialData::from_material(material);
        profile
    }
}

impl Default for CharacterMaterialProfile {
    fn default() -> Self {
        Self {
            toon: ToonMaterialData::default(),
            scene_interaction: SceneInteractionParams::default(),
            shading: MaterialShadingParams::default(),
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
