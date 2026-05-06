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

use crate::app::OrbitCamera;

mod debug_camera;
mod material;

const BRP_PORT_ENV: &str = "BRP_EXTRAS_PORT";
const SCREENSHOT_DIR_ENV: &str = "BEVY_TA_CAPTURE_DIR";
const DEFAULT_CAPTURE_DIR: &str = "assets/private/captures";
pub(crate) const METHOD_PREFIX: &str = "bevy_ta/";

pub struct McpDebugPlugin;

impl Plugin for McpDebugPlugin {
    fn build(&self, app: &mut App) {
        let port = effective_brp_port();
        app.add_plugins(BrpExtrasPlugin::with_port(port))
            .init_resource::<CaptureCounter>()
            .register_type::<McpCapturePrimaryWindow>()
            .register_type::<McpSetOrbitCamera>()
            .register_type::<McpSetMaterialParam>()
            .register_type::<McpSaveToonProfile>()
            .register_type::<debug_camera::McpDebugCamera>()
            .register_type::<debug_camera::McpCreateDebugCamera>()
            .register_type::<debug_camera::McpSetDebugCamera>()
            .register_type::<debug_camera::McpCaptureDebugCamera>()
            .register_type::<debug_camera::McpDeleteDebugCamera>()
            .add_systems(Startup, log_mcp_usage)
            .add_systems(
                Update,
                (
                    capture_screenshot_on_hotkey,
                    debug_camera::cleanup_deleted_debug_cameras,
                ),
            )
            .add_observer(handle_mcp_capture_primary_window)
            .add_observer(handle_mcp_set_orbit_camera)
            .add_observer(material::handle_mcp_set_material_param)
            .add_observer(material::handle_mcp_save_toon_profile)
            .add_observer(debug_camera::handle_create_debug_camera)
            .add_observer(debug_camera::handle_set_debug_camera)
            .add_observer(debug_camera::handle_capture_debug_camera)
            .add_observer(debug_camera::handle_delete_debug_camera);

        register_mcp_methods(app.world_mut());
    }
}

#[derive(Resource, Default)]
struct CaptureCounter(u32);

#[derive(Event, Reflect, Debug, Clone, Default)]
#[reflect(Event, Default)]
pub struct McpCapturePrimaryWindow {
    pub path: String,
}

#[derive(Event, Reflect, Debug, Clone, Default)]
#[reflect(Event, Default)]
pub struct McpSetOrbitCamera {
    pub entity: Option<u64>,
    pub name: Option<String>,
    pub target: Option<[f32; 3]>,
    pub distance: Option<f32>,
    pub yaw: Option<f32>,
    pub pitch: Option<f32>,
}

/// 通过字段路径修改运行时材质参数。
#[derive(Event, Reflect, Debug, Clone, Default)]
#[reflect(Event, Default)]
pub struct McpSetMaterialParam {
    pub entity: Option<u64>,
    pub node_name: Option<String>,
    pub shader_key: Option<String>,
    pub field: String,
    pub number: Option<f32>,
    pub boolean: Option<bool>,
    pub vec4: Option<[f32; 4]>,
    pub apply_all: bool,
}

/// 将当前运行时材质参数保存到 `.toon-model.ron`。
#[derive(Event, Reflect, Debug, Clone, Default)]
#[reflect(Event, Default)]
pub struct McpSaveToonProfile {
    pub entity: Option<u64>,
    pub node_name: Option<String>,
    pub path: Option<String>,
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
    ];

    let mut remote_methods = world.resource_mut::<RemoteMethods>();
    for (name, system_id) in methods {
        remote_methods.insert(
            format!("{METHOD_PREFIX}{name}"),
            RemoteMethodSystemId::Instant(system_id),
        );
    }
    material::register_mcp_methods(world);
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

pub(crate) fn parse_params<T: for<'de> Deserialize<'de>>(
    params: Option<Value>,
) -> Result<T, BrpError> {
    serde_json::from_value(params.unwrap_or(Value::Null)).map_err(|error| BrpError {
        code: INVALID_PARAMS,
        message: format!("Invalid params: {error}"),
        data: None,
    })
}

pub(crate) fn parse_entity_bits(bits: u64) -> Result<Entity, BrpError> {
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

pub(crate) fn invalid_params(message: impl Into<String>) -> BrpError {
    BrpError {
        code: INVALID_PARAMS,
        message: message.into(),
        data: None,
    }
}

pub(crate) fn internal_error(message: impl Into<String>) -> BrpError {
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

pub(crate) fn absolute_path(path: &str) -> Result<PathBuf, BrpError> {
    let path = Path::new(path);
    if path.is_absolute() {
        Ok(path.to_path_buf())
    } else {
        std::env::current_dir()
            .map(|cwd| cwd.join(path))
            .map_err(|error| internal_error(format!("Failed to get current directory: {error}")))
    }
}
