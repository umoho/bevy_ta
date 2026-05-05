use std::{
    env,
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};

use bevy::{
    prelude::*,
    render::view::screenshot::{Screenshot, save_to_disk},
};
use bevy_brp_extras::{BrpExtrasPlugin, DEFAULT_REMOTE_PORT};
use bevy_remote::{
    BrpError, BrpResult, RemoteMethodSystemId, RemoteMethods,
    error_codes::{INTERNAL_ERROR, INVALID_PARAMS},
};
use serde::Deserialize;
use serde_json::{Value, json};

use crate::{
    app::OrbitCamera,
    npr::toon::{ToonMaterial, ToonMaterialBindingSource, ToonParamsData},
};

const BRP_PORT_ENV: &str = "BRP_EXTRAS_PORT";
const SCREENSHOT_DIR_ENV: &str = "BEVY_TA_CAPTURE_DIR";
const DEFAULT_CAPTURE_DIR: &str = "assets/private/captures";
const METHOD_PREFIX: &str = "bevy_ta/";

pub struct McpDebugPlugin;

impl Plugin for McpDebugPlugin {
    fn build(&self, app: &mut App) {
        let port = effective_brp_port();
        app.add_plugins(BrpExtrasPlugin::with_port(port))
            .init_resource::<CaptureCounter>()
            .register_type::<McpCapturePrimaryWindow>()
            .register_type::<McpSetOrbitCamera>()
            .register_type::<McpSetToonParam>()
            .add_systems(Startup, log_mcp_usage)
            .add_systems(Update, capture_screenshot_on_hotkey)
            .add_observer(handle_mcp_capture_primary_window)
            .add_observer(handle_mcp_set_orbit_camera)
            .add_observer(handle_mcp_set_toon_param);

        register_mcp_methods(app.world_mut());
    }
}

#[derive(Resource, Default)]
struct CaptureCounter(u32);

#[derive(Event, Reflect, Debug, Clone)]
#[reflect(Event)]
pub struct McpCapturePrimaryWindow {
    pub path: String,
}

#[derive(Event, Reflect, Debug, Clone)]
#[reflect(Event)]
pub struct McpSetOrbitCamera {
    pub entity: Option<u64>,
    pub name: Option<String>,
    pub target: Option<[f32; 3]>,
    pub distance: Option<f32>,
    pub yaw: Option<f32>,
    pub pitch: Option<f32>,
}

#[derive(Event, Reflect, Debug, Clone)]
#[reflect(Event)]
pub struct McpSetToonParam {
    pub entity: Option<u64>,
    pub node_name: Option<String>,
    pub field: String,
    pub number: Option<f32>,
    pub boolean: Option<bool>,
    pub vec4: Option<[f32; 4]>,
    pub apply_all: bool,
}

fn register_mcp_methods(world: &mut World) {
    let methods = [
        (
            "capture_primary_window",
            world.register_system(capture_primary_window_handler),
        ),
        ("list_cameras", world.register_system(list_cameras_handler)),
        (
            "set_orbit_camera",
            world.register_system(set_orbit_camera_handler),
        ),
        (
            "list_toon_materials",
            world.register_system(list_toon_materials_handler),
        ),
        (
            "set_toon_param",
            world.register_system(set_toon_param_handler),
        ),
    ];

    let mut remote_methods = world.resource_mut::<RemoteMethods>();
    for (name, system_id) in methods {
        remote_methods.insert(
            format!("{METHOD_PREFIX}{name}"),
            RemoteMethodSystemId::Instant(system_id),
        );
    }
}

fn log_mcp_usage() {
    let port = effective_brp_port();
    let capture_dir = capture_directory();
    info!("MCP/BRP 调试接口已启用，端口 http://127.0.0.1:{port}");
    info!(
        "主窗口截图: curl -s http://127.0.0.1:{port} -H 'Content-Type: application/json' -d '{{\"jsonrpc\":\"2.0\",\"method\":\"bevy_ta/capture_primary_window\",\"params\":{{\"path\":\"{}/capture.png\"}},\"id\":1}}'",
        capture_dir.display()
    );
    info!("也可以按 F12 直接导出当前窗口截图");
}

fn capture_screenshot_on_hotkey(
    mut commands: Commands,
    keyboard: Res<ButtonInput<KeyCode>>,
    mut counter: ResMut<CaptureCounter>,
) {
    if !keyboard.just_pressed(KeyCode::F12) {
        return;
    }

    let path = next_capture_path(counter.0);
    counter.0 += 1;
    spawn_primary_window_screenshot(&mut commands, path);
}

#[derive(Deserialize)]
struct CapturePrimaryWindowParams {
    path: String,
}

fn capture_primary_window_handler(
    In(params): In<Option<Value>>,
    mut commands: Commands,
) -> BrpResult {
    let params: CapturePrimaryWindowParams = parse_params(params)?;
    let path = absolute_path(&params.path)?;
    spawn_primary_window_screenshot(&mut commands, path.clone());

    Ok(json!({
        "success": true,
        "path": path,
        "note": "Screenshot capture was queued for the next rendered frame."
    }))
}

fn handle_mcp_capture_primary_window(event: On<McpCapturePrimaryWindow>, mut commands: Commands) {
    match absolute_path(&event.path) {
        Ok(path) => spawn_primary_window_screenshot(&mut commands, path),
        Err(error) => error!("MCP screenshot event failed: {}", error.message),
    }
}

fn spawn_primary_window_screenshot(commands: &mut Commands, path: PathBuf) {
    if let Some(parent) = path.parent()
        && let Err(error) = std::fs::create_dir_all(parent)
    {
        error!("无法创建截图目录 {}: {error}", parent.display());
    }

    commands
        .spawn(Screenshot::primary_window())
        .observe(save_to_disk(path.clone()));
    info!("已请求导出主窗口截图 {}", path.display());
}

fn list_cameras_handler(In(_params): In<Option<Value>>, world: &mut World) -> BrpResult {
    let mut query = world.query::<(
        Entity,
        Option<&Name>,
        &Transform,
        Option<&OrbitCamera>,
        &Camera,
    )>();
    let cameras = query
        .iter(world)
        .map(|(entity, name, transform, orbit, camera)| {
            let orbit = orbit.map(|orbit| {
                json!({
                    "target": vec3_to_array(orbit.target),
                    "distance": orbit.distance,
                    "yaw": orbit.yaw,
                    "pitch": orbit.pitch,
                })
            });

            json!({
                "entity": entity.to_bits(),
                "name": name.map(Name::as_str),
                "is_active": camera.is_active,
                "translation": vec3_to_array(transform.translation),
                "orbit": orbit,
            })
        })
        .collect::<Vec<_>>();

    Ok(json!({ "cameras": cameras }))
}

#[derive(Deserialize)]
struct SetOrbitCameraParams {
    entity: Option<u64>,
    name: Option<String>,
    target: Option<[f32; 3]>,
    distance: Option<f32>,
    yaw: Option<f32>,
    pitch: Option<f32>,
}

fn set_orbit_camera_handler(In(params): In<Option<Value>>, world: &mut World) -> BrpResult {
    let params: SetOrbitCameraParams = parse_params(params)?;
    let target_entity = params.entity.map(parse_entity_bits).transpose()?;
    let mut query = world.query::<(Entity, Option<&Name>, &mut Transform, &mut OrbitCamera)>();

    for (entity, name, mut transform, mut orbit) in query.iter_mut(world) {
        if !camera_matches(entity, name, target_entity, params.name.as_deref()) {
            continue;
        }

        apply_orbit_camera_update(
            &mut orbit,
            &mut transform,
            params.target,
            params.distance,
            params.yaw,
            params.pitch,
        )?;

        return Ok(json!({
            "success": true,
            "camera": {
                "entity": entity.to_bits(),
                "name": name.map(Name::as_str),
                "target": vec3_to_array(orbit.target),
                "distance": orbit.distance,
                "yaw": orbit.yaw,
                "pitch": orbit.pitch,
                "translation": vec3_to_array(transform.translation),
            }
        }));
    }

    Err(invalid_params(
        "No matching orbit camera found. Pass `entity`, `name`, or omit both to use the first orbit camera.",
    ))
}

fn handle_mcp_set_orbit_camera(
    event: On<McpSetOrbitCamera>,
    mut cameras: Query<(Entity, Option<&Name>, &mut Transform, &mut OrbitCamera)>,
) {
    let target_entity = match event.entity.map(parse_entity_bits).transpose() {
        Ok(entity) => entity,
        Err(error) => {
            error!("MCP set orbit camera event failed: {}", error.message);
            return;
        }
    };

    for (entity, name, mut transform, mut orbit) in &mut cameras {
        if !camera_matches(entity, name, target_entity, event.name.as_deref()) {
            continue;
        }

        match apply_orbit_camera_update(
            &mut orbit,
            &mut transform,
            event.target,
            event.distance,
            event.yaw,
            event.pitch,
        ) {
            Ok(()) => info!(
                "MCP set orbit camera entity={} target={:?} distance={} yaw={} pitch={}",
                entity.to_bits(),
                orbit.target,
                orbit.distance,
                orbit.yaw,
                orbit.pitch
            ),
            Err(error) => error!("MCP set orbit camera event failed: {}", error.message),
        }
        return;
    }

    error!("MCP set orbit camera event failed: no matching orbit camera found");
}

fn apply_orbit_camera_update(
    orbit: &mut OrbitCamera,
    transform: &mut Transform,
    target: Option<[f32; 3]>,
    distance: Option<f32>,
    yaw: Option<f32>,
    pitch: Option<f32>,
) -> BrpResult<()> {
    if let Some(target) = target {
        orbit.target = Vec3::from_array(target);
    }
    if let Some(distance) = distance {
        ensure_finite_positive("distance", distance)?;
        orbit.distance = distance;
    }
    if let Some(yaw) = yaw {
        ensure_finite("yaw", yaw)?;
        orbit.yaw = yaw;
    }
    if let Some(pitch) = pitch {
        ensure_finite("pitch", pitch)?;
        orbit.pitch = pitch;
    }
    orbit.orbit_velocity = Vec2::ZERO;
    orbit.zoom_velocity = 0.0;
    orbit.apply_to_transform(transform);

    Ok(())
}

fn list_toon_materials_handler(In(_params): In<Option<Value>>, world: &mut World) -> BrpResult {
    let mut query = world.query::<(
        Entity,
        Option<&Name>,
        &MeshMaterial3d<ToonMaterial>,
        Option<&ToonMaterialBindingSource>,
    )>();

    let material_refs = query
        .iter(world)
        .map(|(entity, name, handle, source)| {
            (
                entity,
                name.map(|name| name.as_str().to_string()),
                handle.id(),
                source.map(|source| source.node_name.clone()),
                source.map(|source| source.shader_key.clone()),
            )
        })
        .collect::<Vec<_>>();

    let materials = world.resource::<Assets<ToonMaterial>>();
    let entries = material_refs
        .into_iter()
        .filter_map(|(entity, name, material_id, node_name, shader_key)| {
            let material = materials.get(material_id)?;
            Some(json!({
                "entity": entity.to_bits(),
                "name": name,
                "material_id": format!("{:?}", material_id),
                "node_name": node_name,
                "shader_key": shader_key,
                "params": ToonParamsData::from_runtime(&material.params),
                "character_material": {
                    "scene_primary": material.character_material.scene_primary.to_array(),
                    "shading_primary": material.character_material.shading_primary.to_array(),
                    "shading_secondary": material.character_material.shading_secondary.to_array(),
                },
            }))
        })
        .collect::<Vec<_>>();

    Ok(json!({ "materials": entries }))
}

#[derive(Deserialize)]
struct SetToonParamParams {
    entity: Option<u64>,
    node_name: Option<String>,
    field: String,
    value: Value,
    #[serde(default)]
    apply_all: bool,
}

fn set_toon_param_handler(In(params): In<Option<Value>>, world: &mut World) -> BrpResult {
    let params: SetToonParamParams = parse_params(params)?;
    let target_entity = params.entity.map(parse_entity_bits).transpose()?;

    let target_ids = {
        let mut query = world.query::<(
            Entity,
            &MeshMaterial3d<ToonMaterial>,
            Option<&ToonMaterialBindingSource>,
        )>();

        query
            .iter(world)
            .filter_map(|(entity, handle, source)| {
                let matches_entity = target_entity.is_none_or(|target| target == entity);
                let matches_node = params
                    .node_name
                    .as_deref()
                    .is_none_or(|target| source.is_some_and(|source| source.node_name == target));
                let should_apply = params.apply_all
                    || params.entity.is_some()
                    || params.node_name.is_some()
                    || (target_entity.is_none() && params.node_name.is_none());

                if should_apply && matches_entity && matches_node {
                    Some(handle.id())
                } else {
                    None
                }
            })
            .collect::<Vec<_>>()
    };

    if target_ids.is_empty() {
        return Err(invalid_params(
            "No matching ToonMaterial found. Pass `entity`, `node_name`, or `apply_all: true`.",
        ));
    }

    let mut changed = 0usize;
    let mut materials = world.resource_mut::<Assets<ToonMaterial>>();
    for id in target_ids {
        if let Some(material) = materials.get_mut(id) {
            set_toon_field(material, &params.field, &params.value).map_err(invalid_params)?;
            changed += 1;
        }
    }

    Ok(json!({
        "success": true,
        "field": params.field,
        "changed_count": changed,
    }))
}

fn handle_mcp_set_toon_param(
    event: On<McpSetToonParam>,
    query: Query<(
        Entity,
        &MeshMaterial3d<ToonMaterial>,
        Option<&ToonMaterialBindingSource>,
    )>,
    mut materials: ResMut<Assets<ToonMaterial>>,
) {
    let target_entity = match event.entity.map(parse_entity_bits).transpose() {
        Ok(entity) => entity,
        Err(error) => {
            error!("MCP set toon param event failed: {}", error.message);
            return;
        }
    };

    let Some(value) = toon_event_value(&event) else {
        error!("MCP set toon param event failed: pass exactly one of number, boolean, or vec4");
        return;
    };

    let target_ids = query
        .iter()
        .filter_map(|(entity, handle, source)| {
            let matches_entity = target_entity.is_none_or(|target| target == entity);
            let matches_node = event
                .node_name
                .as_deref()
                .is_none_or(|target| source.is_some_and(|source| source.node_name == target));
            let should_apply = event.apply_all
                || event.entity.is_some()
                || event.node_name.is_some()
                || (target_entity.is_none() && event.node_name.is_none());

            if should_apply && matches_entity && matches_node {
                Some(handle.id())
            } else {
                None
            }
        })
        .collect::<Vec<_>>();

    if target_ids.is_empty() {
        error!("MCP set toon param event failed: no matching ToonMaterial found");
        return;
    }

    let mut changed = 0usize;
    for id in target_ids {
        if let Some(material) = materials.get_mut(id) {
            match set_toon_field(material, &event.field, &value) {
                Ok(()) => changed += 1,
                Err(error) => {
                    error!("MCP set toon param event failed: {error}");
                    return;
                }
            }
        }
    }

    info!(
        "MCP set toon param field={} changed_count={}",
        event.field, changed
    );
}

fn toon_event_value(event: &McpSetToonParam) -> Option<Value> {
    let provided_count = usize::from(event.number.is_some())
        + usize::from(event.boolean.is_some())
        + usize::from(event.vec4.is_some());
    if provided_count != 1 {
        return None;
    }

    if let Some(value) = event.number {
        Some(json!(value))
    } else if let Some(value) = event.boolean {
        Some(json!(value))
    } else {
        event.vec4.map(|value| json!(value))
    }
}

fn set_toon_field(material: &mut ToonMaterial, field: &str, value: &Value) -> Result<(), String> {
    match field {
        "base_color" => {
            material.params.base_color = LinearRgba::from_f32_array(value_color(value)?)
        }
        "shade_threshold" => material.params.shade_threshold = value_f32(value)?,
        "shade_softness" => material.params.shade_softness = value_f32(value)?,
        "lit_boost" => material.params.lit_boost = value_f32(value)?,
        "shadow_strength" => material.params.shadow_strength = value_f32(value)?,
        "ambient_strength" => material.params.ambient_strength = value_f32(value)?,
        "specular_enabled" => material.params.specular_enabled = value_bool_u32(value)?,
        "specular_strength" => material.params.specular_strength = value_f32(value)?,
        "specular_threshold" => material.params.specular_threshold = value_f32(value)?,
        "specular_softness" => material.params.specular_softness = value_f32(value)?,
        "rim_enabled" => material.params.rim_enabled = value_bool_u32(value)?,
        "rim_strength" => material.params.rim_strength = value_f32(value)?,
        "rim_threshold" => material.params.rim_threshold = value_f32(value)?,
        "rim_softness" => material.params.rim_softness = value_f32(value)?,
        "outline_enabled" => material.params.outline_enabled = value_bool_u32(value)?,
        "outline_width" => material.params.outline_width = value_f32(value)?,
        "alpha_cutoff" => material.params.alpha_cutoff = value_f32(value)?,
        "specular_color" => {
            material.params.specular_color = LinearRgba::from_f32_array(value_color(value)?);
        }
        "rim_color" => material.params.rim_color = LinearRgba::from_f32_array(value_color(value)?),
        "outline_color" => {
            material.params.outline_color = LinearRgba::from_f32_array(value_color(value)?);
        }
        "character_material.scene_primary" => {
            material.character_material.scene_primary = Vec4::from_array(value_vec4(value)?);
        }
        "character_material.shading_primary" => {
            material.character_material.shading_primary = Vec4::from_array(value_vec4(value)?);
        }
        "character_material.shading_secondary" => {
            material.character_material.shading_secondary = Vec4::from_array(value_vec4(value)?);
        }
        _ => {
            return Err(format!(
                "Unsupported ToonMaterial field `{field}`. Use list_toon_materials to inspect supported params."
            ));
        }
    }

    Ok(())
}

fn parse_params<T: for<'de> Deserialize<'de>>(params: Option<Value>) -> Result<T, BrpError> {
    serde_json::from_value(params.unwrap_or(Value::Null)).map_err(|error| BrpError {
        code: INVALID_PARAMS,
        message: format!("Invalid params: {error}"),
        data: None,
    })
}

fn parse_entity_bits(bits: u64) -> Result<Entity, BrpError> {
    Entity::try_from_bits(bits)
        .ok_or_else(|| invalid_params(format!("Invalid entity bits: {bits}")))
}

fn camera_matches(
    entity: Entity,
    name: Option<&Name>,
    target_entity: Option<Entity>,
    target_name: Option<&str>,
) -> bool {
    target_entity.is_none_or(|target| target == entity)
        && target_name.is_none_or(|target| name.is_some_and(|name| name.as_str() == target))
}

fn ensure_finite(name: &str, value: f32) -> BrpResult<()> {
    if value.is_finite() {
        Ok(())
    } else {
        Err(invalid_params(format!("`{name}` must be finite")))
    }
}

fn ensure_finite_positive(name: &str, value: f32) -> BrpResult<()> {
    if value.is_finite() && value > 0.0 {
        Ok(())
    } else {
        Err(invalid_params(format!(
            "`{name}` must be finite and positive"
        )))
    }
}

fn value_f32(value: &Value) -> Result<f32, String> {
    value
        .as_f64()
        .map(|value| value as f32)
        .filter(|value| value.is_finite())
        .ok_or_else(|| "expected a finite number".to_string())
}

fn value_bool_u32(value: &Value) -> Result<u32, String> {
    if let Some(value) = value.as_bool() {
        return Ok(u32::from(value));
    }

    value
        .as_u64()
        .and_then(|value| u32::try_from(value).ok())
        .filter(|value| *value <= 1)
        .ok_or_else(|| "expected boolean, 0, or 1".to_string())
}

fn value_color(value: &Value) -> Result<[f32; 4], String> {
    value_vec4(value)
}

fn value_vec4(value: &Value) -> Result<[f32; 4], String> {
    let values = value
        .as_array()
        .ok_or_else(|| "expected an array of four numbers".to_string())?;
    if values.len() != 4 {
        return Err("expected an array of four numbers".to_string());
    }

    let mut result = [0.0; 4];
    for (index, value) in values.iter().enumerate() {
        result[index] = value_f32(value)?;
    }
    Ok(result)
}

fn invalid_params(message: impl Into<String>) -> BrpError {
    BrpError {
        code: INVALID_PARAMS,
        message: message.into(),
        data: None,
    }
}

fn internal_error(message: impl Into<String>) -> BrpError {
    BrpError {
        code: INTERNAL_ERROR,
        message: message.into(),
        data: None,
    }
}

fn vec3_to_array(value: Vec3) -> [f32; 3] {
    value.to_array()
}

fn effective_brp_port() -> u16 {
    env::var(BRP_PORT_ENV)
        .ok()
        .and_then(|text| text.parse::<u16>().ok())
        .filter(|port| *port > 0)
        .unwrap_or(DEFAULT_REMOTE_PORT)
}

fn capture_directory() -> PathBuf {
    env::var(SCREENSHOT_DIR_ENV)
        .ok()
        .map(PathBuf::from)
        .filter(|path| !path.as_os_str().is_empty())
        .unwrap_or_else(|| PathBuf::from(DEFAULT_CAPTURE_DIR))
}

fn next_capture_path(counter: u32) -> PathBuf {
    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs())
        .unwrap_or(0);
    let file_name = format!("capture-{timestamp}-{counter}.png");
    capture_directory().join(Path::new(&file_name))
}

fn absolute_path(path: &str) -> Result<PathBuf, BrpError> {
    let path = Path::new(path);
    if path.is_absolute() {
        Ok(path.to_path_buf())
    } else {
        std::env::current_dir()
            .map(|cwd| cwd.join(path))
            .map_err(|error| internal_error(format!("Failed to get current directory: {error}")))
    }
}
