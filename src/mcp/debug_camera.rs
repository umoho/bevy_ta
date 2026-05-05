use std::path::{Path, PathBuf};

use bevy::{
    camera::RenderTarget,
    prelude::*,
    render::{
        render_resource::TextureFormat,
        view::screenshot::{Screenshot, save_to_disk},
    },
};

const DEFAULT_DEBUG_CAMERA_WIDTH: u32 = 1024;
const DEFAULT_DEBUG_CAMERA_HEIGHT: u32 = 1024;
const DEFAULT_DEBUG_CAMERA_ORDER: isize = -100;
const DEFAULT_CAPTURE_DIR: &str = "assets/private/captures";

#[derive(Component, Reflect, Debug, Clone)]
#[reflect(Component)]
pub struct McpDebugCamera {
    pub name: String,
    pub width: u32,
    pub height: u32,
    pub target: [f32; 3],
}

#[derive(Component, Clone)]
pub(super) struct DebugCameraImage(Handle<Image>);

#[derive(Component)]
pub(super) struct PendingDebugCameraDelete {
    frames_remaining: u8,
}

#[derive(Event, Reflect, Debug, Clone, Default)]
#[reflect(Event, Default)]
pub struct McpCreateDebugCamera {
    pub name: String,
    pub width: Option<u32>,
    pub height: Option<u32>,
    pub position: Option<[f32; 3]>,
    pub target: Option<[f32; 3]>,
    pub distance: Option<f32>,
    pub yaw: Option<f32>,
    pub pitch: Option<f32>,
    pub active: Option<bool>,
}

#[derive(Event, Reflect, Debug, Clone, Default)]
#[reflect(Event, Default)]
pub struct McpSetDebugCamera {
    pub entity: Option<u64>,
    pub name: Option<String>,
    pub width: Option<u32>,
    pub height: Option<u32>,
    pub position: Option<[f32; 3]>,
    pub target: Option<[f32; 3]>,
    pub distance: Option<f32>,
    pub yaw: Option<f32>,
    pub pitch: Option<f32>,
    pub active: Option<bool>,
}

#[derive(Event, Reflect, Debug, Clone, Default)]
#[reflect(Event, Default)]
pub struct McpCaptureDebugCamera {
    pub entity: Option<u64>,
    pub name: Option<String>,
    pub path: String,
}

#[derive(Event, Reflect, Debug, Clone, Default)]
#[reflect(Event, Default)]
pub struct McpDeleteDebugCamera {
    pub entity: Option<u64>,
    pub name: Option<String>,
    pub delete_all: bool,
}

pub(super) fn handle_create_debug_camera(
    event: On<McpCreateDebugCamera>,
    mut commands: Commands,
    existing: Query<&McpDebugCamera>,
    mut images: ResMut<Assets<Image>>,
) {
    let name = if event.name.trim().is_empty() {
        next_debug_camera_name(&existing)
    } else {
        event.name.trim().to_string()
    };
    if existing.iter().any(|camera| camera.name == name) {
        error!("MCP create debug camera failed: duplicate name `{name}`");
        return;
    }

    let width = dimension_or_default(event.width, DEFAULT_DEBUG_CAMERA_WIDTH);
    let height = dimension_or_default(event.height, DEFAULT_DEBUG_CAMERA_HEIGHT);
    let target = event.target.unwrap_or([0.0, 1.0, 0.0]);
    let transform = match debug_camera_transform(
        event.position,
        target,
        event.distance,
        event.yaw,
        event.pitch,
    ) {
        Ok(transform) => transform,
        Err(error) => {
            error!("MCP create debug camera failed: {error}");
            return;
        }
    };
    let image_handle = images.add(debug_camera_image(width, height));

    let entity = commands
        .spawn((
            Camera3d::default(),
            Camera {
                order: DEFAULT_DEBUG_CAMERA_ORDER,
                clear_color: Color::BLACK.into(),
                is_active: event.active.unwrap_or(true),
                ..default()
            },
            RenderTarget::Image(image_handle.clone().into()),
            transform,
            Name::new(format!("mcp_debug_camera:{name}")),
            McpDebugCamera {
                name: name.clone(),
                width,
                height,
                target,
            },
            DebugCameraImage(image_handle),
        ))
        .id();

    info!(
        "MCP created debug camera entity={} name={} size={}x{} target={:?}",
        entity.to_bits(),
        name,
        width,
        height,
        target
    );
}

pub(super) fn handle_set_debug_camera(
    event: On<McpSetDebugCamera>,
    mut cameras: Query<(
        Entity,
        &mut McpDebugCamera,
        &mut Transform,
        &mut Camera,
        &mut RenderTarget,
        &mut DebugCameraImage,
    )>,
    mut images: ResMut<Assets<Image>>,
) {
    let target_entity = match event.entity.map(parse_entity_bits).transpose() {
        Ok(entity) => entity,
        Err(error) => {
            error!("MCP set debug camera failed: {error}");
            return;
        }
    };

    for (entity, mut debug_camera, mut transform, mut camera, mut render_target, mut image) in
        &mut cameras
    {
        if !debug_camera_matches(entity, &debug_camera, target_entity, event.name.as_deref()) {
            continue;
        }

        let width = event
            .width
            .map(|width| dimension_or_default(Some(width), debug_camera.width))
            .unwrap_or(debug_camera.width);
        let height = event
            .height
            .map(|height| dimension_or_default(Some(height), debug_camera.height))
            .unwrap_or(debug_camera.height);
        if width != debug_camera.width || height != debug_camera.height {
            images.remove(image.0.id());
            image.0 = images.add(debug_camera_image(width, height));
            *render_target = RenderTarget::Image(image.0.clone().into());
            debug_camera.width = width;
            debug_camera.height = height;
        }

        if let Some(active) = event.active {
            camera.is_active = active;
        }

        let target = event.target.unwrap_or(debug_camera.target);
        match debug_camera_transform(
            event.position,
            target,
            event.distance,
            event.yaw,
            event.pitch,
        ) {
            Ok(next_transform) => {
                *transform = next_transform;
                debug_camera.target = target;
                info!(
                    "MCP set debug camera entity={} name={} size={}x{} active={} target={:?}",
                    entity.to_bits(),
                    debug_camera.name,
                    debug_camera.width,
                    debug_camera.height,
                    camera.is_active,
                    target
                );
            }
            Err(error) => error!("MCP set debug camera failed: {error}"),
        }
        return;
    }

    error!("MCP set debug camera failed: no matching debug camera found");
}

pub(super) fn handle_capture_debug_camera(
    event: On<McpCaptureDebugCamera>,
    mut commands: Commands,
    cameras: Query<(Entity, &McpDebugCamera, &DebugCameraImage)>,
) {
    let target_entity = match event.entity.map(parse_entity_bits).transpose() {
        Ok(entity) => entity,
        Err(error) => {
            error!("MCP capture debug camera failed: {error}");
            return;
        }
    };

    for (entity, debug_camera, image) in &cameras {
        if !debug_camera_matches(entity, debug_camera, target_entity, event.name.as_deref()) {
            continue;
        }

        let path = capture_path(&event.path, &debug_camera.name);
        spawn_image_screenshot(&mut commands, image.0.clone(), path.clone());
        info!(
            "MCP requested debug camera capture entity={} name={} path={}",
            entity.to_bits(),
            debug_camera.name,
            path.display()
        );
        return;
    }

    error!("MCP capture debug camera failed: no matching debug camera found");
}

pub(super) fn handle_delete_debug_camera(
    event: On<McpDeleteDebugCamera>,
    mut commands: Commands,
    mut cameras: Query<(
        Entity,
        &McpDebugCamera,
        &mut Camera,
        Option<&PendingDebugCameraDelete>,
    )>,
) {
    let target_entity = match event.entity.map(parse_entity_bits).transpose() {
        Ok(entity) => entity,
        Err(error) => {
            error!("MCP delete debug camera failed: {error}");
            return;
        }
    };

    let mut deleted = 0usize;
    for (entity, debug_camera, mut camera, pending_delete) in &mut cameras {
        let matches = event.delete_all
            || debug_camera_matches(entity, debug_camera, target_entity, event.name.as_deref());
        if !matches {
            continue;
        }

        camera.is_active = false;
        if pending_delete.is_none() {
            commands.entity(entity).insert(PendingDebugCameraDelete {
                frames_remaining: 3,
            });
        }
        deleted += 1;
        info!(
            "MCP scheduled debug camera delete entity={} name={}",
            entity.to_bits(),
            debug_camera.name
        );

        if !event.delete_all {
            break;
        }
    }

    if deleted == 0 {
        error!("MCP delete debug camera failed: no matching debug camera found");
    }
}

pub(super) fn cleanup_deleted_debug_cameras(
    mut commands: Commands,
    mut cameras: Query<(Entity, &DebugCameraImage, &mut PendingDebugCameraDelete)>,
    mut images: ResMut<Assets<Image>>,
) {
    for (entity, image, mut pending_delete) in &mut cameras {
        if pending_delete.frames_remaining > 0 {
            pending_delete.frames_remaining -= 1;
            continue;
        }

        images.remove(image.0.id());
        commands.entity(entity).despawn();
        info!("MCP removed debug camera entity={}", entity.to_bits());
    }
}

fn debug_camera_image(width: u32, height: u32) -> Image {
    Image::new_target_texture(
        width,
        height,
        TextureFormat::Rgba8Unorm,
        Some(TextureFormat::Rgba8UnormSrgb),
    )
}

fn debug_camera_transform(
    position: Option<[f32; 3]>,
    target: [f32; 3],
    distance: Option<f32>,
    yaw: Option<f32>,
    pitch: Option<f32>,
) -> Result<Transform, String> {
    let target = Vec3::from_array(target);
    ensure_finite_vec3("target", target)?;
    let position = if let Some(position) = position {
        let position = Vec3::from_array(position);
        ensure_finite_vec3("position", position)?;
        position
    } else {
        let distance = distance.unwrap_or(6.0);
        ensure_finite_positive("distance", distance)?;
        let yaw = yaw.unwrap_or(-0.4);
        let pitch = pitch.unwrap_or(-0.2);
        ensure_finite("yaw", yaw)?;
        ensure_finite("pitch", pitch)?;
        let rotation = Quat::from_euler(EulerRot::YXZ, yaw, pitch, 0.0);
        target + rotation * Vec3::new(0.0, 0.0, distance)
    };

    if position.distance_squared(target) < 0.0001 {
        return Err("position must not equal target".to_string());
    }

    Ok(Transform::from_translation(position).looking_at(target, Vec3::Y))
}

fn spawn_image_screenshot(commands: &mut Commands, image: Handle<Image>, path: PathBuf) {
    if let Some(parent) = path.parent()
        && let Err(error) = std::fs::create_dir_all(parent)
    {
        error!(
            "Failed to create debug camera capture dir {}: {error}",
            parent.display()
        );
    }

    commands
        .spawn(Screenshot::image(image))
        .observe(save_to_disk(path));
}

fn capture_path(path: &str, camera_name: &str) -> PathBuf {
    let path = path.trim();
    if path.is_empty() {
        return Path::new(DEFAULT_CAPTURE_DIR).join(format!("{camera_name}.png"));
    }

    let path = Path::new(path);
    if path.is_absolute() {
        path.to_path_buf()
    } else {
        std::env::current_dir()
            .map(|cwd| cwd.join(path))
            .unwrap_or_else(|_| path.to_path_buf())
    }
}

fn dimension_or_default(value: Option<u32>, fallback: u32) -> u32 {
    value.filter(|value| *value > 0).unwrap_or(fallback)
}

fn next_debug_camera_name(existing: &Query<&McpDebugCamera>) -> String {
    for index in 0..1024 {
        let name = format!("debug_camera_{index}");
        if existing.iter().all(|camera| camera.name != name) {
            return name;
        }
    }

    "debug_camera".to_string()
}

fn debug_camera_matches(
    entity: Entity,
    camera: &McpDebugCamera,
    target_entity: Option<Entity>,
    target_name: Option<&str>,
) -> bool {
    target_entity.is_none_or(|target| target == entity)
        && target_name.is_none_or(|target| camera.name == target)
}

fn parse_entity_bits(bits: u64) -> Result<Entity, String> {
    Entity::try_from_bits(bits).ok_or_else(|| format!("invalid entity bits: {bits}"))
}

fn ensure_finite(name: &str, value: f32) -> Result<(), String> {
    if value.is_finite() {
        Ok(())
    } else {
        Err(format!("`{name}` must be finite"))
    }
}

fn ensure_finite_positive(name: &str, value: f32) -> Result<(), String> {
    if value.is_finite() && value > 0.0 {
        Ok(())
    } else {
        Err(format!("`{name}` must be finite and positive"))
    }
}

fn ensure_finite_vec3(name: &str, value: Vec3) -> Result<(), String> {
    if value.is_finite() {
        Ok(())
    } else {
        Err(format!("`{name}` must contain finite values"))
    }
}
