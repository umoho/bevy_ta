use std::{env, f32::consts::FRAC_PI_2};

use bevy::pbr::wireframe::WireframePlugin;
use bevy::{
    input::mouse::{AccumulatedMouseMotion, AccumulatedMouseScroll},
    prelude::*,
};
#[cfg(feature = "dev_ui")]
use bevy_egui::input::EguiWantsInput;

mod initial_orbit;

use crate::debug_gizmos::DebugGizmoPlugin;
use crate::lighting::LightingPlugin;
use crate::npr::{
    NprPlugin,
    toon::{ToonMaterial, ToonMaterialTarget, ToonModelBindingAssetPath},
};
#[cfg(feature = "dev_ui")]
use crate::ui::DevWindowState;

const PRIVATE_SCENE_ENV: &str = "BEVY_TA_CHARACTER_SCENE";
const PRIVATE_SCENE_SCALE_ENV: &str = "BEVY_TA_CHARACTER_SCALE";
const DEFAULT_PRIVATE_SCENE_SCALE: f32 = 5.0;
const BASE_MIN_DISTANCE: f32 = 2.0;
const BASE_MAX_DISTANCE: f32 = 150.0;

pub fn run() {
    let mut app = App::new();
    app.init_resource::<OrbitCameraSettings>()
        .add_plugins(initial_orbit::InitialOrbitFramingPlugin)
        .add_plugins(DebugGizmoPlugin)
        .add_plugins(LightingPlugin)
        .add_plugins(DefaultPlugins.set(WindowPlugin {
            primary_window: Some(Window {
                title: "Bevy TA NPR".into(),
                resizable: true,
                ..Default::default()
            }),
            ..Default::default()
        }))
        .add_plugins(WireframePlugin::default())
        .add_plugins(NprPlugin)
        .add_systems(Startup, setup)
        .add_systems(Update, (orbit_camera, toggle_outline));

    #[cfg(feature = "dev_ui")]
    app.init_resource::<DevWindowState>();
    #[cfg(feature = "dev_ui")]
    app.add_plugins((
        crate::ui::MaterialEditorPlugin,
        crate::lighting::LightingEditorPlugin,
    ));
    #[cfg(feature = "brp_tools")]
    app.add_plugins(crate::mcp::McpDebugPlugin);

    app.run();
}

fn setup(
    mut commands: Commands,
    asset_server: Res<AssetServer>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut toon_materials: ResMut<Assets<ToonMaterial>>,
    mut standard_materials: ResMut<Assets<StandardMaterial>>,
    mut images: ResMut<Assets<Image>>,
) {
    commands.insert_resource(GlobalAmbientLight {
        color: Color::WHITE,
        brightness: 1.0,
        affects_lightmapped_meshes: true,
    });

    commands.spawn((
        Mesh3d(meshes.add(Circle::new(3.5))),
        MeshMaterial3d(standard_materials.add(StandardMaterial {
            base_color: Color::srgb(0.78, 0.78, 0.74),
            perceptual_roughness: 0.85,
            ..Default::default()
        })),
        Transform::from_rotation(Quat::from_rotation_x(-FRAC_PI_2)),
    ));

    if let Ok(scene_path) = env::var(PRIVATE_SCENE_ENV) {
        let scene_scale = private_scene_scale();
        commands.insert_resource(OrbitSceneScale(scene_scale));
        commands.spawn((
            SceneRoot(asset_server.load::<Scene>(scene_path.clone())),
            Transform::from_scale(Vec3::splat(scene_scale)),
            ToonMaterialTarget,
            ToonModelBindingAssetPath(scene_path),
        ));
    } else {
        commands.insert_resource(OrbitSceneScale(1.0));
        spawn_placeholder_character(&mut commands, &mut meshes, &mut toon_materials, &mut images);
    }

    commands.spawn((
        Camera3d::default(),
        Transform::from_xyz(-2.8, 2.5, 6.0).looking_at(Vec3::new(0.0, 1.1, 0.0), Vec3::Y),
        OrbitCamera {
            target: Vec3::new(0.0, 1.1, 0.0),
            distance: 6.9,
            yaw: -0.42,
            pitch: -0.2,
            orbit_velocity: Vec2::ZERO,
            zoom_velocity: 0.0,
            pan_velocity: Vec2::ZERO,
        },
        Name::new("camera_orbit"),
    ));
}

fn private_scene_scale() -> f32 {
    env::var(PRIVATE_SCENE_SCALE_ENV)
        .ok()
        .and_then(|value| value.parse::<f32>().ok())
        .filter(|scale| scale.is_finite() && *scale > 0.0)
        .unwrap_or(DEFAULT_PRIVATE_SCENE_SCALE)
}

fn spawn_placeholder_character(
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
    toon_materials: &mut Assets<ToonMaterial>,
    images: &mut Assets<Image>,
) {
    let primary = toon_materials.add({
        let mut material = ToonMaterial::new(images, LinearRgba::rgb(0.86, 0.52, 0.42));
        material.params.outline_width = 0.025;
        material.params.rim_strength = 0.12;
        material
    });
    let secondary = toon_materials.add({
        let mut material = ToonMaterial::new(images, LinearRgba::rgb(0.22, 0.42, 0.72));
        material.params.shade_threshold = 0.48;
        material.params.outline_width = 0.02;
        material
    });

    // 默认场景只用几何体占位，避免把任何私有角色或版权资产名写进源码。
    commands
        .spawn((
            Transform::from_xyz(0.0, 0.95, 0.0),
            Visibility::Inherited,
            Name::new("toon_placeholder"),
            ToonMaterialTarget,
        ))
        .with_children(|parent| {
            parent.spawn((
                Mesh3d(meshes.add(Cuboid::new(0.95, 1.25, 0.55))),
                MeshMaterial3d(secondary.clone()),
                Transform::from_xyz(0.0, 0.25, 0.0),
            ));
            parent.spawn((
                Mesh3d(meshes.add(Sphere::new(0.48))),
                MeshMaterial3d(primary.clone()),
                Transform::from_xyz(0.0, 1.12, 0.0),
            ));
            parent.spawn((
                Mesh3d(meshes.add(Cuboid::new(0.28, 0.85, 0.28))),
                MeshMaterial3d(primary.clone()),
                Transform::from_xyz(-0.68, 0.18, 0.0).with_rotation(Quat::from_rotation_z(0.2)),
            ));
            parent.spawn((
                Mesh3d(meshes.add(Cuboid::new(0.28, 0.85, 0.28))),
                MeshMaterial3d(primary.clone()),
                Transform::from_xyz(0.68, 0.18, 0.0).with_rotation(Quat::from_rotation_z(-0.2)),
            ));
        });
}

#[derive(Component)]
pub(crate) struct OrbitCamera {
    pub(crate) target: Vec3,
    pub(crate) distance: f32,
    pub(crate) yaw: f32,
    pub(crate) pitch: f32,
    pub(crate) orbit_velocity: Vec2,
    pub(crate) zoom_velocity: f32,
    pub(crate) pan_velocity: Vec2,
}

impl OrbitCamera {
    pub(crate) fn apply_to_transform(&self, transform: &mut Transform) {
        let rotation = Quat::from_euler(EulerRot::YXZ, self.yaw, self.pitch, 0.0);
        transform.translation = self.target + rotation * Vec3::new(0.0, 0.0, self.distance);
        transform.look_at(self.target, Vec3::Y);
    }
}

#[derive(Resource)]
struct OrbitCameraSettings {
    orbit_sensitivity: f32,
    zoom_sensitivity: f32,
    pan_sensitivity: f32,
    damping: f32,
    pan_damping: f32,
}

#[derive(Resource, Clone, Copy)]
struct OrbitSceneScale(f32);

impl Default for OrbitCameraSettings {
    fn default() -> Self {
        Self {
            orbit_sensitivity: 0.005,
            zoom_sensitivity: 0.025,
            pan_sensitivity: 0.0010,
            damping: 14.0,
            pan_damping: 20.0,
        }
    }
}

fn orbit_camera(
    mouse_buttons: Res<ButtonInput<MouseButton>>,
    keyboard: Res<ButtonInput<KeyCode>>,
    mouse_motion: Res<AccumulatedMouseMotion>,
    mouse_scroll: Res<AccumulatedMouseScroll>,
    #[cfg(feature = "dev_ui")] egui_wants_input: Option<Res<EguiWantsInput>>,
    time: Res<Time>,
    settings: Res<OrbitCameraSettings>,
    scene_scale: Res<OrbitSceneScale>,
    mut cameras: Query<(&mut Transform, &mut OrbitCamera), With<Camera>>,
) {
    const PITCH_LIMIT: f32 = FRAC_PI_2 - 0.05;
    let min_distance = BASE_MIN_DISTANCE * scene_scale.0.max(0.01);
    let max_distance = BASE_MAX_DISTANCE * scene_scale.0.max(0.01);

    #[cfg(feature = "dev_ui")]
    if egui_wants_input
        .as_ref()
        .is_some_and(|egui_wants_input| egui_wants_input.wants_any_pointer_input())
    {
        return;
    }

    for (mut transform, mut orbit) in &mut cameras {
        if mouse_buttons.pressed(MouseButton::Left) {
            let orbit_delta = Vec2::new(-mouse_motion.delta.x, -mouse_motion.delta.y)
                * settings.orbit_sensitivity;
            orbit.yaw += orbit_delta.x;
            orbit.pitch += orbit_delta.y;
            orbit.orbit_velocity = orbit_delta * 60.0;
        }

        if mouse_scroll.delta != Vec2::ZERO {
            let is_pan = keyboard.any_pressed([KeyCode::Space]);
            let is_zoom = keyboard.any_pressed([
                KeyCode::ControlLeft,
                KeyCode::ControlRight,
                KeyCode::SuperLeft,
                KeyCode::SuperRight,
            ]);
            if is_pan {
                let distance = orbit.distance;
                orbit.pan_velocity += Vec2::new(-mouse_scroll.delta.x, mouse_scroll.delta.y)
                    * settings.pan_sensitivity
                    * distance
                    * 30.0;
            } else if is_zoom {
                orbit.zoom_velocity += mouse_scroll.delta.y * settings.zoom_sensitivity * 30.0;
            } else {
                orbit.orbit_velocity += Vec2::new(-mouse_scroll.delta.x, -mouse_scroll.delta.y)
                    * settings.orbit_sensitivity
                    * 20.0;
            }
        }

        let dt = time.delta_secs();
        orbit.yaw += orbit.orbit_velocity.x * dt;
        orbit.pitch = (orbit.pitch + orbit.orbit_velocity.y * dt).clamp(-PITCH_LIMIT, PITCH_LIMIT);
        orbit.distance =
            (orbit.distance + orbit.zoom_velocity * dt).clamp(min_distance, max_distance);

        if orbit.pan_velocity != Vec2::ZERO {
            let pan_velocity = orbit.pan_velocity;
            let rotation = Quat::from_euler(EulerRot::YXZ, orbit.yaw, orbit.pitch, 0.0);
            let right = rotation * Vec3::X;
            let up = rotation * Vec3::Y;
            orbit.target += (right * pan_velocity.x + up * pan_velocity.y) * dt;
        }

        let drag = (-settings.damping * dt).exp();
        orbit.orbit_velocity *= drag;
        orbit.zoom_velocity *= drag;
        let pan_drag = (-settings.pan_damping * dt).exp();
        orbit.pan_velocity *= pan_drag;

        orbit.apply_to_transform(&mut transform);
    }
}

fn toggle_outline(
    keyboard: Res<ButtonInput<KeyCode>>,
    mut toon_materials: ResMut<Assets<ToonMaterial>>,
) {
    if !keyboard.just_pressed(KeyCode::KeyO) {
        return;
    }

    let enable_outline = toon_materials
        .iter()
        .next()
        .map(|(_, material)| material.params.outline_enabled == 0)
        .unwrap_or(true);

    for (_, material) in toon_materials.iter_mut() {
        material.params.outline_enabled = u32::from(enable_outline);
    }
}
