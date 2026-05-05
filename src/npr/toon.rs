use bevy::{
    asset::RenderAssetUsages,
    image::{ImageAddressMode, ImageFilterMode, ImageSampler, ImageSamplerDescriptor},
    mesh::MeshVertexBufferLayoutRef,
    pbr::{Material, MaterialPipeline, MaterialPipelineKey, OpaqueRendererMethod},
    prelude::*,
    reflect::TypePath,
    render::render_resource::{
        AsBindGroup, Extent3d, Face, RenderPipelineDescriptor, ShaderType,
        SpecializedMeshPipelineError, TextureDimension, TextureFormat,
    },
    scene::SceneInstanceReady,
    shader::ShaderRef,
};
use serde::{Deserialize, Deserializer, Serialize, Serializer};

use super::profile::{
    CHARACTER_FACE_SDF_SHADER_KEY, CHARACTER_SURFACE_SHADER_KEY, CharacterRenderProfile,
    RenderPartResources, ShaderProfileRegistry,
};

const TOON_SHADER_PATH: &str = "shaders/npr/toon.wgsl";

type StandardMeshMaterial = MeshMaterial3d<StandardMaterial>;

pub struct ToonMaterialPlugin;

impl Plugin for ToonMaterialPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<FaceAnchorAliases>()
            .add_systems(Update, sync_face_sdf_anchor_data)
            .add_plugins(MaterialPlugin::<ToonMaterial>::default())
            .add_observer(convert_scene_materials_to_toon);
    }
}

#[derive(Resource)]
struct FaceAnchorAliases {
    names: Vec<&'static str>,
}

impl Default for FaceAnchorAliases {
    fn default() -> Self {
        Self {
            names: vec!["頭", "头", "Head", "head"],
        }
    }
}

#[derive(Component, Debug, Default, Clone, Copy)]
pub struct ToonMaterialTarget;

#[derive(Component, Debug, Clone)]
pub struct ToonModelBindingAssetPath(pub String);

#[derive(Component, Debug, Clone)]
pub struct ToonMaterialBindingSource {
    pub scene_asset_path: Option<String>,
    pub node_name: String,
    pub shader_key: String,
}

#[derive(Asset, TypePath, AsBindGroup, Debug, Clone)]
#[bind_group_data(ToonMaterialKey)]
pub struct ToonMaterial {
    #[uniform(0)]
    pub params: ToonParams,
    #[texture(1)]
    #[sampler(2)]
    pub base_color_texture: Option<Handle<Image>>,
    #[texture(3)]
    #[sampler(4)]
    pub ramp_texture: Handle<Image>,
    #[uniform(5)]
    pub character_material: CharacterMaterialParams,
    #[uniform(6)]
    pub face_sdf: FaceSdfParams,
    #[texture(7)]
    #[sampler(8)]
    pub face_shadow_texture: Option<Handle<Image>>,
    pub ramp_data: RampData,
    pub alpha_mode: AlphaMode,
    pub cull_mode: Option<Face>,
}

impl ToonMaterial {
    pub fn new(images: &mut Assets<Image>, base_color: LinearRgba) -> Self {
        let ramp_data = RampData::default();
        Self {
            params: ToonParams {
                base_color,
                ..Default::default()
            },
            base_color_texture: None,
            ramp_texture: create_ramp_texture(images, &ramp_data),
            character_material: CharacterMaterialParams::default(),
            face_sdf: FaceSdfParams::default(),
            face_shadow_texture: None,
            ramp_data,
            alpha_mode: AlphaMode::Opaque,
            cull_mode: Some(Face::Back),
        }
    }

    pub fn from_standard_material(material: &StandardMaterial, images: &mut Assets<Image>) -> Self {
        let mut toon = Self::new(images, material.base_color.into());
        toon.base_color_texture = material.base_color_texture.clone();
        toon.params.use_base_color_texture = u32::from(toon.base_color_texture.is_some());
        toon.alpha_mode = material.alpha_mode;
        toon.cull_mode = material.cull_mode;
        toon
    }

    pub fn set_base_color_texture_path(&mut self, asset_server: &AssetServer, path: Option<&str>) {
        self.base_color_texture = path
            .map(str::trim)
            .filter(|path| !path.is_empty())
            .map(|path| asset_server.load(path.to_string()));
        self.params.use_base_color_texture = u32::from(self.base_color_texture.is_some());
    }

    pub fn apply_render_part_resources(
        &mut self,
        resources: &RenderPartResources,
        asset_server: &AssetServer,
    ) {
        if resources.base_color_texture.is_some() {
            self.set_base_color_texture_path(asset_server, resources.base_color_texture.as_deref());
        }
        if resources.face_shadow_texture.is_some() {
            self.face_shadow_texture = resources
                .face_shadow_texture
                .as_deref()
                .map(str::trim)
                .filter(|path| !path.is_empty())
                .map(|path| asset_server.load(path.to_string()));
            self.face_sdf.texture_enabled = u32::from(self.face_shadow_texture.is_some());
        }
    }
}

impl Material for ToonMaterial {
    fn fragment_shader() -> ShaderRef {
        TOON_SHADER_PATH.into()
    }

    fn alpha_mode(&self) -> AlphaMode {
        self.alpha_mode
    }

    fn opaque_render_method(&self) -> OpaqueRendererMethod {
        OpaqueRendererMethod::Forward
    }

    fn specialize(
        _pipeline: &MaterialPipeline,
        descriptor: &mut RenderPipelineDescriptor,
        _layout: &MeshVertexBufferLayoutRef,
        key: MaterialPipelineKey<Self>,
    ) -> Result<(), SpecializedMeshPipelineError> {
        descriptor.primitive.cull_mode = key.bind_group_data.cull_mode;
        Ok(())
    }
}

#[repr(C)]
#[derive(Eq, PartialEq, Hash, Copy, Clone)]
pub struct ToonMaterialKey {
    cull_mode: Option<Face>,
}

impl From<&ToonMaterial> for ToonMaterialKey {
    fn from(material: &ToonMaterial) -> Self {
        Self {
            cull_mode: material.cull_mode,
        }
    }
}

#[derive(Reflect, ShaderType, Debug, Clone)]
pub struct ToonParams {
    pub base_color: LinearRgba,
    pub shade_threshold: f32,
    pub shade_softness: f32,
    pub lit_boost: f32,
    pub shadow_strength: f32,
    pub ambient_strength: f32,
    pub specular_enabled: u32,
    pub specular_strength: f32,
    pub specular_threshold: f32,
    pub specular_softness: f32,
    pub rim_enabled: u32,
    pub rim_strength: f32,
    pub rim_threshold: f32,
    pub rim_softness: f32,
    pub outline_enabled: u32,
    pub outline_width: f32,
    pub use_base_color_texture: u32,
    pub alpha_cutoff: f32,
    pub specular_color: LinearRgba,
    pub rim_color: LinearRgba,
    pub outline_color: LinearRgba,
}

impl Default for ToonParams {
    fn default() -> Self {
        Self {
            base_color: LinearRgba::WHITE,
            shade_threshold: 0.52,
            shade_softness: 0.06,
            lit_boost: 1.0,
            shadow_strength: 0.55,
            ambient_strength: 0.18,
            specular_enabled: 1,
            specular_strength: 0.25,
            specular_threshold: 0.86,
            specular_softness: 0.04,
            rim_enabled: 1,
            rim_strength: 0.18,
            rim_threshold: 0.68,
            rim_softness: 0.08,
            outline_enabled: 1,
            outline_width: 0.018,
            use_base_color_texture: 0,
            alpha_cutoff: 0.05,
            specular_color: LinearRgba::WHITE,
            rim_color: LinearRgba::WHITE,
            outline_color: LinearRgba::BLACK,
        }
    }
}

#[derive(Reflect, ShaderType, Debug, Clone)]
pub struct CharacterMaterialParams {
    pub scene_primary: Vec4,
    pub shading_primary: Vec4,
    pub shading_secondary: Vec4,
}

impl Default for CharacterMaterialParams {
    fn default() -> Self {
        Self {
            scene_primary: Vec4::new(1.0, 0.35, 0.65, 0.12),
            shading_primary: Vec4::new(0.0, 1.0, 1.0, 0.0),
            shading_secondary: Vec4::new(0.0, 0.0, 0.0, 0.35),
        }
    }
}

#[derive(Reflect, ShaderType, Debug, Clone)]
pub struct FaceSdfParams {
    pub enabled: u32,
    pub texture_enabled: u32,
    pub uv_mirror_enabled: u32,
    pub debug_mode: u32,
    pub shadow_strength: f32,
    pub blend_weight: f32,
    pub threshold_bias: f32,
    pub softness: f32,
    pub horizontal_scale: f32,
    pub horizontal_bias: f32,
    pub vertical_influence: f32,
    pub backlight_clamp: f32,
    pub procedural_terminator_softness: f32,
    pub procedural_vertical_curve: f32,
    pub reserved0: f32,
    pub reserved1: f32,
    pub face_forward: Vec4,
    pub face_right: Vec4,
    pub face_up: Vec4,
}

impl Default for FaceSdfParams {
    fn default() -> Self {
        Self {
            enabled: 0,
            texture_enabled: 0,
            uv_mirror_enabled: 1,
            debug_mode: 0,
            shadow_strength: 0.85,
            blend_weight: 1.0,
            threshold_bias: 0.0,
            softness: 0.03,
            horizontal_scale: 1.0,
            horizontal_bias: 0.0,
            vertical_influence: 0.15,
            backlight_clamp: 0.2,
            procedural_terminator_softness: 0.18,
            procedural_vertical_curve: 0.25,
            reserved0: 0.0,
            reserved1: 0.0,
            face_forward: Vec4::new(0.0, 0.0, -1.0, 0.0),
            face_right: Vec4::new(1.0, 0.0, 0.0, 0.0),
            face_up: Vec4::new(0.0, 1.0, 0.0, 0.0),
        }
    }
}

#[derive(Clone, Serialize, Deserialize)]
pub struct FaceSdfParamsData {
    pub enabled: bool,
    pub use_texture: bool,
    pub uv_mirror_enabled: bool,
    pub debug_mode: u32,
    pub shadow_strength: f32,
    pub blend_weight: f32,
    pub threshold_bias: f32,
    pub softness: f32,
    pub horizontal_scale: f32,
    pub horizontal_bias: f32,
    pub vertical_influence: f32,
    pub backlight_clamp: f32,
    pub procedural_terminator_softness: f32,
    pub procedural_vertical_curve: f32,
}

impl FaceSdfParamsData {
    pub fn from_runtime(params: &FaceSdfParams, shader_key: &str) -> Self {
        let mut data = Self::default();
        if shader_key == CHARACTER_FACE_SDF_SHADER_KEY {
            data.enabled = params.enabled != 0;
            data.use_texture = params.texture_enabled != 0;
            data.uv_mirror_enabled = params.uv_mirror_enabled != 0;
            data.debug_mode = params.debug_mode;
            data.shadow_strength = params.shadow_strength;
            data.blend_weight = params.blend_weight;
            data.threshold_bias = params.threshold_bias;
            data.softness = params.softness;
            data.horizontal_scale = params.horizontal_scale;
            data.horizontal_bias = params.horizontal_bias;
            data.vertical_influence = params.vertical_influence;
            data.backlight_clamp = params.backlight_clamp;
            data.procedural_terminator_softness = params.procedural_terminator_softness;
            data.procedural_vertical_curve = params.procedural_vertical_curve;
        }
        data
    }

    pub fn clone_for_shader_key(data: &Self, shader_key: &str) -> Self {
        if shader_key == CHARACTER_FACE_SDF_SHADER_KEY {
            data.clone()
        } else {
            Self::default()
        }
    }

    pub fn into_runtime(self) -> FaceSdfParams {
        let mut params = FaceSdfParams::default();
        params.enabled = u32::from(self.enabled);
        params.texture_enabled = u32::from(self.use_texture);
        params.uv_mirror_enabled = u32::from(self.uv_mirror_enabled);
        params.debug_mode = self.debug_mode;
        params.shadow_strength = self.shadow_strength;
        params.blend_weight = self.blend_weight;
        params.threshold_bias = self.threshold_bias;
        params.softness = self.softness;
        params.horizontal_scale = self.horizontal_scale;
        params.horizontal_bias = self.horizontal_bias;
        params.vertical_influence = self.vertical_influence;
        params.backlight_clamp = self.backlight_clamp;
        params.procedural_terminator_softness = self.procedural_terminator_softness;
        params.procedural_vertical_curve = self.procedural_vertical_curve;
        params
    }
}

impl Default for FaceSdfParamsData {
    fn default() -> Self {
        Self {
            enabled: false,
            use_texture: false,
            uv_mirror_enabled: true,
            debug_mode: 0,
            shadow_strength: 0.85,
            blend_weight: 1.0,
            threshold_bias: 0.0,
            softness: 0.03,
            horizontal_scale: 1.0,
            horizontal_bias: 0.0,
            vertical_influence: 0.15,
            backlight_clamp: 0.2,
            procedural_terminator_softness: 0.18,
            procedural_vertical_curve: 0.25,
        }
    }
}

#[derive(Clone, Serialize, Deserialize)]
pub struct ToonMaterialData {
    pub params: ToonParamsData,
    pub ramp: RampDataFile,
}

impl ToonMaterialData {
    pub fn from_material(material: &ToonMaterial) -> Self {
        Self {
            params: ToonParamsData::from_runtime(&material.params),
            ramp: RampDataFile::from_runtime(&material.ramp_data),
        }
    }

    pub fn apply_to_material(self, material: &mut ToonMaterial, images: &mut Assets<Image>) {
        material.params = self.params.into_runtime();
        material.ramp_data = self.ramp.into_runtime();
        rebuild_ramp_texture(images, &material.ramp_texture, &material.ramp_data);
    }
}

impl Default for ToonMaterialData {
    fn default() -> Self {
        Self {
            params: ToonParamsData::default(),
            ramp: RampDataFile::default(),
        }
    }
}

#[derive(Clone, Serialize, Deserialize)]
pub struct ToonParamsData {
    pub base_color: [f32; 4],
    pub shade_threshold: f32,
    pub shade_softness: f32,
    pub lit_boost: f32,
    pub shadow_strength: f32,
    pub ambient_strength: f32,
    pub specular_enabled: bool,
    pub specular_strength: f32,
    pub specular_threshold: f32,
    pub specular_softness: f32,
    pub rim_enabled: bool,
    pub rim_strength: f32,
    pub rim_threshold: f32,
    pub rim_softness: f32,
    pub outline_enabled: bool,
    pub outline_width: f32,
    pub alpha_cutoff: f32,
    pub specular_color: [f32; 4],
    pub rim_color: [f32; 4],
    pub outline_color: [f32; 4],
}

impl ToonParamsData {
    pub fn from_runtime(params: &ToonParams) -> Self {
        Self {
            base_color: params.base_color.to_f32_array(),
            shade_threshold: params.shade_threshold,
            shade_softness: params.shade_softness,
            lit_boost: params.lit_boost,
            shadow_strength: params.shadow_strength,
            ambient_strength: params.ambient_strength,
            specular_enabled: params.specular_enabled != 0,
            specular_strength: params.specular_strength,
            specular_threshold: params.specular_threshold,
            specular_softness: params.specular_softness,
            rim_enabled: params.rim_enabled != 0,
            rim_strength: params.rim_strength,
            rim_threshold: params.rim_threshold,
            rim_softness: params.rim_softness,
            outline_enabled: params.outline_enabled != 0,
            outline_width: params.outline_width,
            alpha_cutoff: params.alpha_cutoff,
            specular_color: params.specular_color.to_f32_array(),
            rim_color: params.rim_color.to_f32_array(),
            outline_color: params.outline_color.to_f32_array(),
        }
    }

    pub fn into_runtime(self) -> ToonParams {
        ToonParams {
            base_color: LinearRgba::from_f32_array(self.base_color),
            shade_threshold: self.shade_threshold,
            shade_softness: self.shade_softness,
            lit_boost: self.lit_boost,
            shadow_strength: self.shadow_strength,
            ambient_strength: self.ambient_strength,
            specular_enabled: u32::from(self.specular_enabled),
            specular_strength: self.specular_strength,
            specular_threshold: self.specular_threshold,
            specular_softness: self.specular_softness,
            rim_enabled: u32::from(self.rim_enabled),
            rim_strength: self.rim_strength,
            rim_threshold: self.rim_threshold,
            rim_softness: self.rim_softness,
            outline_enabled: u32::from(self.outline_enabled),
            outline_width: self.outline_width,
            use_base_color_texture: 0,
            alpha_cutoff: self.alpha_cutoff,
            specular_color: LinearRgba::from_f32_array(self.specular_color),
            rim_color: LinearRgba::from_f32_array(self.rim_color),
            outline_color: LinearRgba::from_f32_array(self.outline_color),
        }
    }
}

impl Default for ToonParamsData {
    fn default() -> Self {
        Self::from_runtime(&ToonParams::default())
    }
}

#[derive(Reflect, Debug, Clone)]
pub struct RampData {
    pub stops: Vec<RampStop>,
    pub interpolation: RampInterpolation,
    pub resolution: u32,
}

impl Default for RampData {
    fn default() -> Self {
        default_ramp_data()
    }
}

#[derive(Reflect, Debug, Clone)]
pub struct RampStop {
    pub position: f32,
    pub color: LinearRgba,
}

#[derive(Reflect, Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum RampInterpolation {
    Constant,
    Linear,
}

#[derive(Clone, Serialize, Deserialize)]
pub struct RampStopFile {
    pub position: f32,
    pub color: [f32; 4],
}

#[derive(Clone, Serialize, Deserialize)]
pub struct RampDataFile {
    pub stops: Vec<RampStopFile>,
    #[serde(
        serialize_with = "serialize_ramp_interpolation",
        deserialize_with = "deserialize_ramp_interpolation"
    )]
    pub interpolation: RampInterpolation,
    pub resolution: u32,
}

impl RampDataFile {
    pub fn from_runtime(ramp_data: &RampData) -> Self {
        Self {
            stops: ramp_data
                .stops
                .iter()
                .map(|stop| RampStopFile {
                    position: stop.position,
                    color: stop.color.to_f32_array(),
                })
                .collect(),
            interpolation: ramp_data.interpolation,
            resolution: ramp_data.resolution,
        }
    }

    pub fn into_runtime(self) -> RampData {
        RampData {
            stops: self
                .stops
                .into_iter()
                .map(|stop| RampStop {
                    position: stop.position,
                    color: LinearRgba::from_f32_array(stop.color),
                })
                .collect(),
            interpolation: self.interpolation,
            resolution: self.resolution,
        }
    }
}

impl Default for RampDataFile {
    fn default() -> Self {
        Self::from_runtime(&RampData::default())
    }
}

fn serialize_ramp_interpolation<S>(
    value: &RampInterpolation,
    serializer: S,
) -> Result<S::Ok, S::Error>
where
    S: Serializer,
{
    serializer.serialize_str(match value {
        RampInterpolation::Constant => "constant",
        RampInterpolation::Linear => "linear",
    })
}

fn deserialize_ramp_interpolation<'de, D>(deserializer: D) -> Result<RampInterpolation, D::Error>
where
    D: Deserializer<'de>,
{
    let value = String::deserialize(deserializer)?;
    match value.as_str() {
        "constant" => Ok(RampInterpolation::Constant),
        "linear" => Ok(RampInterpolation::Linear),
        other => Err(serde::de::Error::custom(format!(
            "unknown ramp interpolation: {other}"
        ))),
    }
}

pub fn default_ramp_data() -> RampData {
    RampData {
        stops: vec![
            RampStop {
                position: 0.0,
                color: LinearRgba::rgb(0.42, 0.42, 0.42),
            },
            RampStop {
                position: 0.48,
                color: LinearRgba::rgb(0.56, 0.56, 0.56),
            },
            RampStop {
                position: 0.52,
                color: LinearRgba::WHITE,
            },
            RampStop {
                position: 1.0,
                color: LinearRgba::WHITE,
            },
        ],
        interpolation: RampInterpolation::Constant,
        resolution: 16,
    }
}

pub fn create_ramp_texture(images: &mut Assets<Image>, ramp_data: &RampData) -> Handle<Image> {
    images.add(bake_ramp_image(ramp_data))
}

pub fn rebuild_ramp_texture(
    images: &mut Assets<Image>,
    ramp_texture: &Handle<Image>,
    ramp_data: &RampData,
) {
    let Some(image) = images.get_mut(ramp_texture.id()) else {
        return;
    };
    *image = bake_ramp_image(ramp_data);
}

fn bake_ramp_image(ramp_data: &RampData) -> Image {
    let resolution = ramp_data.resolution.max(2);
    let mut data = Vec::with_capacity(resolution as usize * 4);

    for index in 0..resolution {
        let position = index as f32 / (resolution - 1) as f32;
        let [r, g, b, a] = sample_ramp_color(ramp_data, position).to_f32_array();
        data.extend([
            (r.clamp(0.0, 1.0) * 255.0).round() as u8,
            (g.clamp(0.0, 1.0) * 255.0).round() as u8,
            (b.clamp(0.0, 1.0) * 255.0).round() as u8,
            (a.clamp(0.0, 1.0) * 255.0).round() as u8,
        ]);
    }

    let mut image = Image::new(
        Extent3d {
            width: resolution,
            height: 1,
            depth_or_array_layers: 1,
        },
        TextureDimension::D2,
        data,
        TextureFormat::Rgba8Unorm,
        RenderAssetUsages::default(),
    );
    let filter_mode = match ramp_data.interpolation {
        RampInterpolation::Constant => ImageFilterMode::Nearest,
        RampInterpolation::Linear => ImageFilterMode::Linear,
    };
    image.sampler = ImageSampler::Descriptor(ImageSamplerDescriptor {
        mag_filter: filter_mode,
        min_filter: filter_mode,
        address_mode_u: ImageAddressMode::ClampToEdge,
        address_mode_v: ImageAddressMode::ClampToEdge,
        ..Default::default()
    });
    image
}

fn sample_ramp_color(ramp_data: &RampData, position: f32) -> LinearRgba {
    let Some(first_stop) = ramp_data.stops.first() else {
        return LinearRgba::WHITE;
    };
    let Some(last_stop) = ramp_data.stops.last() else {
        return LinearRgba::WHITE;
    };
    if position <= first_stop.position {
        return first_stop.color;
    }
    if position >= last_stop.position {
        return last_stop.color;
    }

    for window in ramp_data.stops.windows(2) {
        let [left, right] = window else {
            continue;
        };
        if position < right.position {
            return match ramp_data.interpolation {
                RampInterpolation::Constant => left.color,
                RampInterpolation::Linear => {
                    let span = (right.position - left.position).max(f32::EPSILON);
                    let t = (position - left.position) / span;
                    LinearRgba::from_f32_array([
                        left.color.red + (right.color.red - left.color.red) * t,
                        left.color.green + (right.color.green - left.color.green) * t,
                        left.color.blue + (right.color.blue - left.color.blue) * t,
                        left.color.alpha + (right.color.alpha - left.color.alpha) * t,
                    ])
                }
            };
        }
    }

    last_stop.color
}

fn sync_face_sdf_anchor_data(
    aliases: Res<FaceAnchorAliases>,
    node_transforms: Query<(&Name, &GlobalTransform)>,
    face_material_meshes: Query<(&MeshMaterial3d<ToonMaterial>, &ToonMaterialBindingSource)>,
    mut materials: ResMut<Assets<ToonMaterial>>,
) {
    let Some((_, anchor_transform)) = node_transforms.iter().find(|(name, _)| {
        aliases
            .names
            .iter()
            .any(|alias| name.as_str().eq_ignore_ascii_case(alias) || name.as_str() == *alias)
    }) else {
        return;
    };

    let rotation = anchor_transform.rotation();
    let face_forward = (rotation * -Vec3::Z).normalize_or_zero();
    let face_right = (rotation * Vec3::X).normalize_or_zero();
    let face_up = (rotation * Vec3::Y).normalize_or_zero();
    if face_forward == Vec3::ZERO || face_right == Vec3::ZERO || face_up == Vec3::ZERO {
        return;
    }

    for (material_handle, binding_source) in &face_material_meshes {
        if binding_source.shader_key != CHARACTER_FACE_SDF_SHADER_KEY {
            continue;
        }
        let Some(material) = materials.get_mut(material_handle.id()) else {
            continue;
        };
        material.face_sdf.face_forward = face_forward.extend(0.0);
        material.face_sdf.face_right = face_right.extend(0.0);
        material.face_sdf.face_up = face_up.extend(0.0);
    }
}

fn convert_scene_materials_to_toon(
    scene_ready: On<SceneInstanceReady>,
    mut commands: Commands,
    asset_server: Res<AssetServer>,
    toon_targets: Query<Option<&ToonModelBindingAssetPath>, With<ToonMaterialTarget>>,
    scene_nodes: Query<&Children>,
    node_names: Query<Option<&Name>>,
    mesh_materials: Query<&StandardMeshMaterial>,
    standard_materials: Res<Assets<StandardMaterial>>,
    mut toon_materials: ResMut<Assets<ToonMaterial>>,
    mut images: ResMut<Assets<Image>>,
    profile_registry: Res<ShaderProfileRegistry>,
) {
    let Ok(binding_asset_path) = toon_targets.get(scene_ready.entity) else {
        return;
    };
    let binding_scene_asset_path = binding_asset_path.map(|path| path.0.clone());
    let model_bindings = binding_scene_asset_path
        .as_deref()
        .and_then(|scene_asset_path| {
            CharacterRenderProfile::load_for_scene_asset_path(scene_asset_path).ok()
        });

    // GLTF 会先生成 StandardMaterial；这里仅提取基础颜色和贴图，再换成独立的 toon 材质。
    for descendant in scene_nodes.iter_descendants(scene_ready.entity) {
        let Ok(mesh_material) = mesh_materials.get(descendant) else {
            continue;
        };
        let Some(source_material) = standard_materials.get(mesh_material) else {
            continue;
        };
        let node_name = node_names
            .get(descendant)
            .ok()
            .and_then(|name| name.map(ToString::to_string))
            .unwrap_or_else(|| format!("节点 {}", descendant.index()));
        let mut toon_material = ToonMaterial::from_standard_material(source_material, &mut images);
        let shader_key = model_bindings
            .as_ref()
            .and_then(|profile| profile.find_part(&node_name))
            .map(|part| part.shader_key.clone())
            .unwrap_or_else(|| CHARACTER_SURFACE_SHADER_KEY.to_string());

        if let Some(part) = model_bindings
            .as_ref()
            .and_then(|profile| profile.find_part(&node_name))
        {
            if let Some(handler) = profile_registry.get(&part.shader_key) {
                if let Err(err) = handler.apply_to_toon_material(
                    &part.params,
                    &part.resources,
                    &mut toon_material,
                    &mut images,
                    &asset_server,
                ) {
                    warn!(
                        "failed to apply toon profile for node `{}` shader `{}`: {}",
                        node_name, part.shader_key, err
                    );
                }
            }
        }

        commands
            .entity(descendant)
            .remove::<StandardMeshMaterial>()
            .insert(MeshMaterial3d(toon_materials.add(toon_material)))
            .insert(ToonMaterialBindingSource {
                scene_asset_path: binding_scene_asset_path.clone(),
                node_name,
                shader_key,
            });
    }
}
