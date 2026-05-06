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

#[derive(Component)]
struct OrbitingMainLight;

pub struct LightingPlugin;

impl Plugin for LightingPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<OrbitingLightSettings>()
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
    gizmos_enabled: Res<DebugGizmoSettings>,
    mut gizmos: Gizmos,
) {
    if !gizmos_enabled.enabled || !gizmos_enabled.show_orbiting_light {
        return;
    }

    let center = settings.center;
    let position = orbit_light_position(&settings);

    let orbit_color = Color::srgba(0.95, 0.82, 0.22, 0.75);
    let center_color = Color::srgba(0.20, 0.85, 1.00, 0.95);
    let light_color = Color::srgba(1.00, 0.95, 0.80, 1.00);
    let axis_color = Color::srgba(0.55, 0.65, 0.75, 0.55);

    gizmos
        .sphere(
            Isometry3d::from_translation(center),
            settings.radius,
            orbit_color,
        )
        .resolution(48);

    gizmos.line(center, position, light_color);
    gizmos.sphere(Isometry3d::from_translation(center), 0.07, center_color);
    gizmos.sphere(Isometry3d::from_translation(position), 0.12, light_color);

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
                if ui.small_button("重置").clicked() {
                    *settings = OrbitingLightSettings::default();
                }
            });

            ui.add(
                egui::Slider::new(&mut settings.radius, MIN_LIGHT_RADIUS..=MAX_LIGHT_RADIUS)
                    .text("球半径"),
            );
            ui.add(egui::Slider::new(&mut settings.yaw, -PI..=PI).text("水平角"));
            ui.add(
                egui::Slider::new(&mut settings.pitch, MIN_LIGHT_PITCH..=MAX_LIGHT_PITCH)
                    .text("俯仰角"),
            );
            ui.add(
                egui::Slider::new(&mut settings.illuminance, 0.0..=40_000.0)
                    .logarithmic(true)
                    .text("照度"),
            );

            ui.separator();
            ui.label(egui::RichText::new("球心位置").strong());
            vec3_editor(ui, &mut settings.center);
            ui.horizontal(|ui| {
                ui.checkbox(&mut settings.shadows_enabled, "阴影");
            });

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
