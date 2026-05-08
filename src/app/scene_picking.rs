#![cfg(feature = "dev_ui")]

use bevy::{prelude::*, window::PrimaryWindow};
use bevy_egui::input::EguiWantsInput;
use bevy_picking::mesh_picking::ray_cast::{MeshRayCast, MeshRayCastSettings};

use crate::selection::{MaterialPanelEntryRef, MaterialSelectionState};
#[cfg(feature = "dev_ui")]
use crate::ui::DevWindowState;

use super::OrbitCamera;

const CLICK_DRAG_THRESHOLD: f32 = 5.0;

pub struct ScenePickingPlugin;

impl Plugin for ScenePickingPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<PendingPrimitiveClick>().add_systems(
            Update,
            handle_scene_selection_input.before(super::orbit_camera),
        );
    }
}

#[derive(Resource, Default)]
pub(crate) struct PendingPrimitiveClick {
    primitive_entity: Option<Entity>,
    panel_entity: Option<Entity>,
    press_position: Option<Vec2>,
    drag_started: bool,
}

impl PendingPrimitiveClick {
    fn begin(&mut self, primitive_entity: Entity, panel_entity: Entity, press_position: Vec2) {
        self.primitive_entity = Some(primitive_entity);
        self.panel_entity = Some(panel_entity);
        self.press_position = Some(press_position);
        self.drag_started = false;
    }

    fn clear(&mut self) {
        *self = Self::default();
    }

    fn blocks_orbit(&self) -> bool {
        self.primitive_entity.is_some() && !self.drag_started
    }
}

pub(crate) fn left_button_orbit_is_blocked(state: Option<&PendingPrimitiveClick>) -> bool {
    state.is_some_and(PendingPrimitiveClick::blocks_orbit)
}

fn handle_scene_selection_input(
    mouse_buttons: Res<ButtonInput<MouseButton>>,
    #[cfg(feature = "dev_ui")] egui_wants_input: Option<Res<EguiWantsInput>>,
    #[cfg(feature = "dev_ui")] mut window_state: ResMut<DevWindowState>,
    windows: Query<&Window, With<PrimaryWindow>>,
    cameras: Query<(&Camera, &GlobalTransform), With<OrbitCamera>>,
    primitive_refs: Query<&MaterialPanelEntryRef>,
    mut ray_cast: MeshRayCast,
    mut selection: ResMut<MaterialSelectionState>,
    mut pending: ResMut<PendingPrimitiveClick>,
) {
    #[cfg(feature = "dev_ui")]
    if egui_wants_input
        .as_ref()
        .is_some_and(|egui_wants_input| egui_wants_input.wants_any_pointer_input())
    {
        if mouse_buttons.just_released(MouseButton::Left) {
            pending.clear();
        }
        return;
    }

    let Ok(window) = windows.single() else {
        return;
    };

    if mouse_buttons.just_pressed(MouseButton::Left) {
        let Some(cursor_position) = window.cursor_position() else {
            pending.clear();
            return;
        };
        let Some((primitive_entity, panel_entity)) =
            pick_scene_primitive(cursor_position, &cameras, &primitive_refs, &mut ray_cast)
        else {
            pending.clear();
            return;
        };
        pending.begin(primitive_entity, panel_entity, cursor_position);
    }

    if mouse_buttons.pressed(MouseButton::Left)
        && let (Some(press_position), Some(cursor_position)) =
            (pending.press_position, window.cursor_position())
        && cursor_position.distance_squared(press_position)
            > CLICK_DRAG_THRESHOLD * CLICK_DRAG_THRESHOLD
    {
        pending.drag_started = true;
    }

    if mouse_buttons.just_released(MouseButton::Left) {
        let candidate = (
            pending.primitive_entity,
            pending.panel_entity,
            pending.press_position,
            pending.drag_started,
        );
        pending.clear();

        let (Some(expected_primitive), Some(panel_entity), Some(_), false) = candidate else {
            return;
        };

        let Some(cursor_position) = window.cursor_position() else {
            return;
        };

        let Some((primitive_entity, _)) =
            pick_scene_primitive(cursor_position, &cameras, &primitive_refs, &mut ray_cast)
        else {
            return;
        };

        if primitive_entity == expected_primitive {
            selection.select_primitive(panel_entity, primitive_entity);
            #[cfg(feature = "dev_ui")]
            {
                window_state.material_property_open = true;
            }
        }
    }
}

fn pick_scene_primitive(
    cursor_position: Vec2,
    cameras: &Query<(&Camera, &GlobalTransform), With<OrbitCamera>>,
    primitive_refs: &Query<&MaterialPanelEntryRef>,
    ray_cast: &mut MeshRayCast,
) -> Option<(Entity, Entity)> {
    let (camera, camera_transform) = cameras.iter().next()?;
    let ray = camera
        .viewport_to_world(camera_transform, cursor_position)
        .ok()?;
    let filter = |entity| primitive_refs.get(entity).is_ok();
    let settings = MeshRayCastSettings::default()
        .with_filter(&filter)
        .always_early_exit();

    ray_cast
        .cast_ray(ray, &settings)
        .iter()
        .find_map(|(primitive_entity, _)| {
            primitive_refs
                .get(*primitive_entity)
                .ok()
                .map(|panel_ref| (*primitive_entity, panel_ref.0))
        })
}
