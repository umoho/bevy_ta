use std::collections::HashSet;

use bevy::camera::Projection;
use bevy::pbr::wireframe::{Wireframe, WireframeColor};
use bevy::{camera::primitives::Aabb, prelude::*, transform::TransformSystems};
#[cfg(feature = "dev_ui")]
use bevy_egui::{EguiContexts, EguiPrimaryContextPass, egui, input::EguiWantsInput};

#[cfg(feature = "brp_tools")]
use crate::mcp::McpDebugCamera;
use crate::npr::toon::ToonMaterialTarget;

const DEBUG_GIZMO_TOGGLE_KEY: KeyCode = KeyCode::KeyG;
const CHARACTER_AABB_COLOR: Color = Color::srgba(0.35, 0.85, 1.0, 0.95);
const SELECTED_PRIMITIVE_COLOR: Color = Color::srgba(1.0, 0.78, 0.22, 0.95);
const DEBUG_CAMERA_COLOR: Color = Color::srgba(0.95, 0.45, 0.35, 0.95);
const DEBUG_CAMERA_ICON_SCALE: f32 = 0.14;

#[derive(Resource, Debug, Clone)]
pub struct DebugGizmoSettings {
    pub enabled: bool,
    pub show_character_bounds: bool,
    pub show_character_bounds_axes: bool,
    pub show_selected_primitives: bool,
    pub show_debug_cameras: bool,
    pub show_orbiting_light: bool,
}

impl Default for DebugGizmoSettings {
    fn default() -> Self {
        Self {
            enabled: false,
            show_character_bounds: true,
            show_character_bounds_axes: false,
            show_selected_primitives: true,
            show_debug_cameras: true,
            show_orbiting_light: true,
        }
    }
}

#[derive(Resource, Debug, Clone, Default)]
pub struct DebugSceneSelection {
    pub selected_material_entity: Option<Entity>,
    pub selected_material_primitives: Vec<Entity>,
}

impl DebugSceneSelection {
    pub fn clear_selected_material(&mut self) {
        self.selected_material_entity = None;
        self.selected_material_primitives.clear();
    }

    pub fn set_selected_material(
        &mut self,
        selected_material_entity: Entity,
        selected_material_primitives: impl IntoIterator<Item = Entity>,
    ) {
        self.selected_material_entity = Some(selected_material_entity);
        self.selected_material_primitives = selected_material_primitives.into_iter().collect();
    }
}

pub struct DebugGizmoPlugin;

impl Plugin for DebugGizmoPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<DebugGizmoSettings>()
            .init_resource::<DebugSceneSelection>()
            .add_systems(Update, toggle_debug_gizmos_on_hotkey)
            .add_systems(
                PostUpdate,
                sync_selected_primitive_wireframes.after(TransformSystems::Propagate),
            )
            .add_systems(
                PostUpdate,
                draw_character_bounds.after(TransformSystems::Propagate),
            )
            .add_systems(
                PostUpdate,
                draw_debug_camera_gizmos.after(TransformSystems::Propagate),
            );

        #[cfg(feature = "dev_ui")]
        app.add_systems(EguiPrimaryContextPass, show_debug_gizmo_window);
    }
}

fn toggle_debug_gizmos_on_hotkey(
    keyboard: Res<ButtonInput<KeyCode>>,
    #[cfg(feature = "dev_ui")] egui_wants_input: Option<Res<EguiWantsInput>>,
    mut settings: ResMut<DebugGizmoSettings>,
) {
    #[cfg(feature = "dev_ui")]
    if egui_wants_input
        .as_ref()
        .is_some_and(|egui_wants_input| egui_wants_input.wants_any_keyboard_input())
    {
        return;
    }

    if !keyboard.just_pressed(DEBUG_GIZMO_TOGGLE_KEY) {
        return;
    }

    settings.enabled = !settings.enabled;
    info!(
        "Debug gizmos {}",
        if settings.enabled {
            "enabled"
        } else {
            "disabled"
        }
    );
}

#[derive(Component)]
struct SelectedPrimitiveWireframe;

fn sync_selected_primitive_wireframes(
    settings: Res<DebugGizmoSettings>,
    selection: Res<DebugSceneSelection>,
    highlighted: Query<
        (Entity, Option<&Wireframe>, Option<&WireframeColor>),
        With<SelectedPrimitiveWireframe>,
    >,
    mut commands: Commands,
) {
    let should_show = settings.enabled && settings.show_selected_primitives;
    let selected_primitives: HashSet<Entity> = if should_show {
        selection
            .selected_material_primitives
            .iter()
            .copied()
            .collect()
    } else {
        HashSet::new()
    };
    let highlighted_entities = highlighted
        .iter()
        .map(|(entity, _, _)| entity)
        .collect::<HashSet<_>>();

    for (entity, maybe_wireframe, maybe_color) in highlighted.iter() {
        if !selected_primitives.contains(&entity) {
            commands
                .entity(entity)
                .remove::<SelectedPrimitiveWireframe>();
            commands.entity(entity).remove::<Wireframe>();
            commands.entity(entity).remove::<WireframeColor>();
            continue;
        }

        if maybe_wireframe.is_none() || maybe_color.is_none() {
            commands.entity(entity).insert((
                SelectedPrimitiveWireframe,
                Wireframe,
                WireframeColor {
                    color: SELECTED_PRIMITIVE_COLOR,
                },
            ));
        }
    }

    if !should_show {
        return;
    }

    for entity in selected_primitives.difference(&highlighted_entities) {
        commands.entity(*entity).insert((
            SelectedPrimitiveWireframe,
            Wireframe,
            WireframeColor {
                color: SELECTED_PRIMITIVE_COLOR,
            },
        ));
    }
}

fn draw_character_bounds(
    settings: Res<DebugGizmoSettings>,
    roots: Query<Entity, With<ToonMaterialTarget>>,
    children: Query<&Children>,
    mesh_boxes: Query<(&Aabb, &GlobalTransform), With<Mesh3d>>,
    mut gizmos: Gizmos,
) {
    if !settings.enabled || !settings.show_character_bounds {
        return;
    }

    for root in &roots {
        let mut points = Vec::new();
        for descendant in children
            .iter_descendants(root)
            .filter_map(|entity| mesh_boxes.get(entity).ok())
        {
            collect_world_aabb_points(&mut points, descendant.0, descendant.1);
        }

        let Some(world_aabb) = Aabb::enclosing(points.iter().copied()) else {
            continue;
        };

        gizmos.aabb_3d(world_aabb, Transform::IDENTITY, CHARACTER_AABB_COLOR);
        if settings.show_character_bounds_axes {
            gizmos.axes(
                Transform::from_translation(world_aabb.center.into()),
                world_aabb.half_extents.length().max(0.35),
            );
        }
    }
}

#[cfg(feature = "brp_tools")]
fn draw_debug_camera_gizmos(
    settings: Res<DebugGizmoSettings>,
    cameras: Query<(&McpDebugCamera, &GlobalTransform, &Projection)>,
    mut gizmos: Gizmos,
) {
    if !settings.enabled || !settings.show_debug_cameras {
        return;
    }

    for (camera, transform, projection) in &cameras {
        let camera_position = transform.translation();
        let target = Vec3::from_array(camera.target);

        draw_debug_camera_frustum(&mut gizmos, camera_position, target, projection);
    }
}

#[cfg(not(feature = "brp_tools"))]
fn draw_debug_camera_gizmos(settings: Res<DebugGizmoSettings>, mut gizmos: Gizmos) {
    let _ = (settings, &mut gizmos);
}

#[cfg(feature = "brp_tools")]
fn draw_debug_camera_frustum(
    gizmos: &mut Gizmos,
    camera_position: Vec3,
    target: Vec3,
    projection: &Projection,
) {
    let Projection::Perspective(perspective) = projection else {
        gizmos.line(camera_position, target, DEBUG_CAMERA_COLOR);
        return;
    };

    let forward = (target - camera_position).normalize_or_zero();
    if forward.length_squared() == 0.0 {
        return;
    }

    let up_hint = if forward.dot(Vec3::Y).abs() > 0.98 {
        Vec3::X
    } else {
        Vec3::Y
    };
    let right = forward.cross(up_hint).normalize_or_zero();
    if right.length_squared() == 0.0 {
        return;
    }
    let up = right.cross(forward).normalize_or_zero();
    if up.length_squared() == 0.0 {
        return;
    }

    let depth = camera_position.distance(target).max(0.35) * DEBUG_CAMERA_ICON_SCALE;
    let half_height = (perspective.fov * 0.5).tan() * depth;
    let half_width = half_height * perspective.aspect_ratio.max(0.01);
    let frustum_center = camera_position + forward * depth;

    let top_left = frustum_center + up * half_height - right * half_width;
    let top_right = frustum_center + up * half_height + right * half_width;
    let bottom_left = frustum_center - up * half_height - right * half_width;
    let bottom_right = frustum_center - up * half_height + right * half_width;

    gizmos.line(camera_position, top_left, DEBUG_CAMERA_COLOR);
    gizmos.line(camera_position, top_right, DEBUG_CAMERA_COLOR);
    gizmos.line(camera_position, bottom_left, DEBUG_CAMERA_COLOR);
    gizmos.line(camera_position, bottom_right, DEBUG_CAMERA_COLOR);

    gizmos.line(top_left, top_right, DEBUG_CAMERA_COLOR);
    gizmos.line(top_right, bottom_right, DEBUG_CAMERA_COLOR);
    gizmos.line(bottom_right, bottom_left, DEBUG_CAMERA_COLOR);
    gizmos.line(bottom_left, top_left, DEBUG_CAMERA_COLOR);

    draw_debug_camera_up_triangle(
        gizmos,
        frustum_center,
        up,
        right,
        depth,
        half_height,
        half_width,
    );
}

#[cfg(feature = "brp_tools")]
fn draw_debug_camera_up_triangle(
    gizmos: &mut Gizmos,
    frustum_center: Vec3,
    up: Vec3,
    right: Vec3,
    depth: f32,
    half_height: f32,
    half_width: f32,
) {
    let marker_height = (depth * 0.225).max(0.0825);
    let marker_width = (half_width * 0.83).min(depth * 0.39).max(depth * 0.165);
    let gap = (marker_height * 0.52).max(depth * 0.045);
    let triangle_center = frustum_center + up * (half_height + gap + marker_height * 0.10);
    let top = triangle_center + up * marker_height * 0.58;
    let left = triangle_center - up * marker_height * 0.42 - right * marker_width * 0.50;
    let right_point = triangle_center - up * marker_height * 0.42 + right * marker_width * 0.50;
    let triangle = Triangle3d::new(top, left, right_point);

    gizmos.primitive_3d(&triangle, Isometry3d::IDENTITY, DEBUG_CAMERA_COLOR);
}

#[cfg(feature = "dev_ui")]
fn show_debug_gizmo_window(mut contexts: EguiContexts, mut settings: ResMut<DebugGizmoSettings>) {
    let Ok(ctx) = contexts.ctx_mut() else {
        return;
    };

    egui::Window::new("调试 gizmos")
        .resizable(true)
        .default_size([280.0, 250.0])
        .show(ctx, |ui| {
            ui.horizontal(|ui| {
                ui.label(egui::RichText::new("全局").strong());
                ui.checkbox(&mut settings.enabled, "启用");
                if ui.small_button("重置").clicked() {
                    *settings = DebugGizmoSettings::default();
                }
            });

            ui.separator();
            ui.horizontal(|ui| {
                ui.checkbox(&mut settings.show_character_bounds, "角色包围盒");
                ui.checkbox(&mut settings.show_character_bounds_axes, "中心轴");
            });
            ui.checkbox(
                &mut settings.show_selected_primitives,
                "选中 primitive 线框",
            );
            ui.checkbox(&mut settings.show_debug_cameras, "调试摄像机");
            ui.checkbox(&mut settings.show_orbiting_light, "方向光辅助");
            ui.add_space(4.0);
            ui.small("热键 G 只切换全局开关。");
        });
}

fn collect_world_aabb_points(points: &mut Vec<Vec3>, aabb: &Aabb, transform: &GlobalTransform) {
    let min = Vec3::from(aabb.min());
    let max = Vec3::from(aabb.max());
    let corners = [
        Vec3::new(min.x, min.y, min.z),
        Vec3::new(max.x, min.y, min.z),
        Vec3::new(min.x, max.y, min.z),
        Vec3::new(max.x, max.y, min.z),
        Vec3::new(min.x, min.y, max.z),
        Vec3::new(max.x, min.y, max.z),
        Vec3::new(min.x, max.y, max.z),
        Vec3::new(max.x, max.y, max.z),
    ];

    points.extend(
        corners
            .into_iter()
            .map(|corner| transform.transform_point(corner)),
    );
}
