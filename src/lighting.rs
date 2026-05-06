use std::f32::consts::{FRAC_PI_2, PI};

use bevy::prelude::*;
#[cfg(feature = "dev_ui")]
use bevy_egui::{EguiContexts, egui};
#[cfg(feature = "dev_ui")]
use egui::Ui;

use crate::debug_gizmos::DebugGizmoSettings;

const DEFAULT_LIGHT_CENTER: Vec3 = Vec3::new(0.0, 1.1, 0.0);
const DEFAULT_LIGHT_RADIUS: f32 = 2.5;
const DEFAULT_LIGHT_YAW: f32 = -0.35;
const DEFAULT_LIGHT_PITCH: f32 = -0.80;
const MIN_LIGHT_RADIUS: f32 = 0.5;
const MAX_LIGHT_RADIUS: f32 = 20.0;
const MIN_LIGHT_PITCH: f32 = -FRAC_PI_2 + 0.05;
const MAX_LIGHT_PITCH: f32 = FRAC_PI_2 - 0.05;

#[derive(Resource, Debug, Clone)]
pub struct OrbitingLightSettings {
    pub center: Vec3,
    pub radius: f32,
    pub yaw: f32,
    pub pitch: f32,
    pub illuminance: f32,
    pub shadows_enabled: bool,
}

impl Default for OrbitingLightSettings {
    fn default() -> Self {
        Self {
            center: DEFAULT_LIGHT_CENTER,
            radius: DEFAULT_LIGHT_RADIUS,
            yaw: DEFAULT_LIGHT_YAW,
            pitch: DEFAULT_LIGHT_PITCH,
            illuminance: 18_000.0,
            shadows_enabled: true,
        }
    }
}

#[derive(Resource, Debug, Clone, Default)]
pub struct OrbitingLightGizmoState {
    pub active_circle: Option<OrbitingLightRotationCircle>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OrbitingLightRotationCircle {
    Yaw,
    Pitch,
}

#[derive(Component)]
struct OrbitingMainLight;

pub struct LightingPlugin;

impl Plugin for LightingPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<OrbitingLightSettings>()
            .init_resource::<OrbitingLightGizmoState>()
            .add_systems(Startup, spawn_orbiting_main_light)
            .add_systems(
                Update,
                (sync_orbiting_main_light, draw_orbiting_light_gizmos),
            );
    }
}

#[cfg(feature = "dev_ui")]
pub struct LightingEditorPlugin;

#[cfg(feature = "dev_ui")]
impl Plugin for LightingEditorPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(bevy_egui::EguiPrimaryContextPass, show_light_control_panel);
    }
}

fn spawn_orbiting_main_light(mut commands: Commands, settings: Res<OrbitingLightSettings>) {
    let position = orbit_light_position(&settings);
    commands.spawn((
        DirectionalLight {
            illuminance: settings.illuminance,
            shadows_enabled: settings.shadows_enabled,
            ..Default::default()
        },
        Transform::from_translation(position).looking_at(settings.center, Vec3::Y),
        OrbitingMainLight,
        Name::new("orbiting_main_light"),
    ));
}

fn sync_orbiting_main_light(
    settings: Res<OrbitingLightSettings>,
    mut lights: Query<(&mut Transform, &mut DirectionalLight), With<OrbitingMainLight>>,
) {
    let position = orbit_light_position(&settings);
    for (mut transform, mut light) in &mut lights {
        transform.translation = position;
        transform.look_at(settings.center, Vec3::Y);
        light.illuminance = settings.illuminance;
        light.shadows_enabled = settings.shadows_enabled;
    }
}

fn draw_orbiting_light_gizmos(
    settings: Res<OrbitingLightSettings>,
    gizmo_state: Res<OrbitingLightGizmoState>,
    gizmos_enabled: Res<DebugGizmoSettings>,
    mut gizmos: Gizmos,
) {
    if !gizmos_enabled.enabled || !gizmos_enabled.show_orbiting_light {
        return;
    }

    let center = settings.center;
    let position = orbit_light_position(&settings);
    let offset = position - center;

    let orbit_color = Color::srgba(0.95, 0.82, 0.22, 0.75);
    let center_color = Color::srgba(0.20, 0.85, 1.00, 0.95);
    let light_color = Color::srgba(1.00, 0.95, 0.80, 1.00);
    let ground_line_color = Color::srgba(1.00, 0.95, 0.80, 0.28);
    let axis_color = Color::srgba(0.55, 0.65, 0.75, 0.55);

    gizmos.line(center, position, light_color);
    let ground_point = Vec3::new(position.x, 0.0, position.z);
    gizmos.line(position, ground_point, ground_line_color);

    if let Some(circle) = gizmo_state.active_circle {
        match circle {
            OrbitingLightRotationCircle::Yaw => {
                let horizontal_offset = Vec3::new(offset.x, 0.0, offset.z);
                let horizontal_radius = horizontal_offset.length();
                if horizontal_radius > 0.0001 {
                    let circle_center = center + Vec3::Y * offset.y;
                    gizmos
                        .circle(
                            Isometry3d::new(
                                circle_center,
                                Quat::from_rotation_arc(Vec3::Z, Vec3::Y),
                            ),
                            horizontal_radius,
                            orbit_color,
                        )
                        .resolution(64);
                }
            }
            OrbitingLightRotationCircle::Pitch => {
                let horizontal_offset = Vec3::new(offset.x, 0.0, offset.z);
                if horizontal_offset.length_squared() > 0.0001 {
                    let horizontal_direction = horizontal_offset.normalize();
                    let normal = horizontal_direction.cross(Vec3::Y).normalize_or_zero();
                    if normal.length_squared() != 0.0 {
                        gizmos
                            .circle(
                                Isometry3d::new(center, Quat::from_rotation_arc(Vec3::Z, normal)),
                                settings.radius,
                                orbit_color,
                            )
                            .resolution(64);
                    }
                }
            }
        }
    }

    gizmos.sphere(Isometry3d::from_translation(center), 0.06, center_color);
    gizmos.sphere(Isometry3d::from_translation(position), 0.08, light_color);

    let axis_length = settings.radius.max(1.0) * 0.45;
    gizmos.line(
        center - Vec3::X * axis_length,
        center + Vec3::X * axis_length,
        axis_color,
    );
    gizmos.line(
        center - Vec3::Y * axis_length,
        center + Vec3::Y * axis_length,
        axis_color,
    );
    gizmos.line(
        center - Vec3::Z * axis_length,
        center + Vec3::Z * axis_length,
        axis_color,
    );
}

#[cfg(feature = "dev_ui")]
fn show_light_control_panel(
    mut contexts: EguiContexts,
    mut settings: ResMut<OrbitingLightSettings>,
    mut gizmo_state: ResMut<OrbitingLightGizmoState>,
) {
    let Ok(ctx) = contexts.ctx_mut() else {
        return;
    };

    egui::Window::new("光源控制")
        .resizable(true)
        .default_size([320.0, 280.0])
        .show(ctx, |ui| {
            ui.horizontal(|ui| {
                ui.label(egui::RichText::new("主方向光").strong());
                ui.separator();
                ui.checkbox(&mut settings.shadows_enabled, "阴影");
                if ui.small_button("重置").clicked() {
                    *settings = OrbitingLightSettings::default();
                }
            });

            let yaw_response =
                ui.add(egui::Slider::new(&mut settings.yaw, -PI..=PI).text("水平角"));
            let pitch_response = ui.add(
                egui::Slider::new(&mut settings.pitch, MIN_LIGHT_PITCH..=MAX_LIGHT_PITCH)
                    .text("俯仰角"),
            );

            ui.separator();
            ui.label(egui::RichText::new("轨道中心").strong());
            vec3_editor(ui, &mut settings.center);
            ui.add(
                egui::Slider::new(&mut settings.radius, MIN_LIGHT_RADIUS..=MAX_LIGHT_RADIUS)
                    .text("轨道半径"),
            );
            gizmo_state.active_circle = if yaw_response.dragged() {
                Some(OrbitingLightRotationCircle::Yaw)
            } else if pitch_response.dragged() {
                Some(OrbitingLightRotationCircle::Pitch)
            } else {
                None
            };
            ui.add(
                egui::Slider::new(&mut settings.illuminance, 0.0..=40_000.0)
                    .logarithmic(true)
                    .text("照度"),
            );

            ui.separator();
            let position = orbit_light_position(&settings);
            ui.label(format!(
                "当前灯位: ({:.2}, {:.2}, {:.2})",
                position.x, position.y, position.z
            ));
        });
}

fn vec3_editor(ui: &mut Ui, value: &mut Vec3) {
    ui.horizontal(|ui| {
        ui.label("X");
        ui.add(egui::DragValue::new(&mut value.x).speed(0.05));
        ui.label("Y");
        ui.add(egui::DragValue::new(&mut value.y).speed(0.05));
        ui.label("Z");
        ui.add(egui::DragValue::new(&mut value.z).speed(0.05));
    });
}

fn orbit_light_position(settings: &OrbitingLightSettings) -> Vec3 {
    let rotation = Quat::from_euler(EulerRot::YXZ, settings.yaw, settings.pitch, 0.0);
    settings.center
        + rotation
            * Vec3::new(
                0.0,
                0.0,
                settings.radius.clamp(MIN_LIGHT_RADIUS, MAX_LIGHT_RADIUS),
            )
}
