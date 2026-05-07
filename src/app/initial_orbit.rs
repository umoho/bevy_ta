use bevy::camera::Projection;
use bevy::{camera::primitives::Aabb, prelude::*, transform::TransformSystems};

use crate::{app::OrbitCamera, npr::toon::ToonMaterialTarget, utils::world_aabb_for_root};

const TARGET_HEIGHT_RATIO: f32 = 0.68;
const DISTANCE_PADDING: f32 = 1.15;
const MIN_INITIAL_DISTANCE: f32 = 2.0;
const MAX_INITIAL_DISTANCE: f32 = 18.0;

#[derive(Resource, Default)]
struct InitialOrbitFramingState {
    framed_root: Option<Entity>,
}

pub struct InitialOrbitFramingPlugin;

impl Plugin for InitialOrbitFramingPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<InitialOrbitFramingState>().add_systems(
            PostUpdate,
            frame_initial_orbit.after(TransformSystems::Propagate),
        );
    }
}

fn frame_initial_orbit(
    mut state: ResMut<InitialOrbitFramingState>,
    roots: Query<Entity, With<ToonMaterialTarget>>,
    children: Query<&Children>,
    mesh_boxes: Query<(&Aabb, &GlobalTransform), With<Mesh3d>>,
    mut cameras: Query<(&mut Transform, &mut OrbitCamera, &Projection), With<Camera3d>>,
) {
    let Some(root) = roots.iter().next() else {
        return;
    };
    if state.framed_root == Some(root) {
        return;
    }

    let Some(world_aabb) = world_aabb_for_root(root, &children, &mesh_boxes) else {
        return;
    };

    let Some((mut transform, mut orbit, projection)) = cameras.iter_mut().next() else {
        return;
    };
    let Projection::Perspective(perspective) = projection else {
        return;
    };

    let target = biased_target_from_aabb(&world_aabb);
    let rotation = Quat::from_euler(EulerRot::YXZ, orbit.yaw, orbit.pitch, 0.0);
    let distance = estimate_orbit_distance(
        &world_aabb,
        target,
        rotation,
        perspective.fov,
        perspective.aspect_ratio,
    )
    .clamp(MIN_INITIAL_DISTANCE, MAX_INITIAL_DISTANCE);

    orbit.target = target;
    orbit.distance = distance;
    orbit.orbit_velocity = Vec2::ZERO;
    orbit.zoom_velocity = 0.0;
    orbit.pan_velocity = Vec2::ZERO;
    orbit.apply_to_transform(&mut transform);

    state.framed_root = Some(root);
    info!(
        "Initial orbit framed for root={} target={:?} distance={}",
        root.to_bits(),
        orbit.target,
        orbit.distance
    );
}

fn biased_target_from_aabb(aabb: &Aabb) -> Vec3 {
    let min = Vec3::from(aabb.min());
    let max = Vec3::from(aabb.max());
    let center = (min + max) * 0.5;
    let target_y = min.y + (max.y - min.y) * TARGET_HEIGHT_RATIO;
    Vec3::new(center.x, target_y, center.z)
}

fn estimate_orbit_distance(
    aabb: &Aabb,
    target: Vec3,
    rotation: Quat,
    vertical_fov: f32,
    aspect_ratio: f32,
) -> f32 {
    let tan_vertical = (vertical_fov * 0.5).tan();
    if !tan_vertical.is_finite() || tan_vertical <= 0.0 {
        return MIN_INITIAL_DISTANCE;
    }

    let tan_horizontal = tan_vertical * aspect_ratio.max(0.01);
    let right = rotation * Vec3::X;
    let up = rotation * Vec3::Y;
    let forward = -(rotation * Vec3::Z);
    let mut required_distance: f32 = 0.0;

    for corner in aabb_corners(aabb) {
        let offset = corner - target;
        let depth = offset.dot(forward);
        let horizontal = offset.dot(right).abs();
        let vertical = offset.dot(up).abs();

        required_distance = required_distance.max(-depth);
        required_distance = required_distance.max(horizontal / tan_horizontal - depth);
        required_distance = required_distance.max(vertical / tan_vertical - depth);
    }

    (required_distance.max(0.0_f32) * DISTANCE_PADDING).max(MIN_INITIAL_DISTANCE)
}

fn aabb_corners(aabb: &Aabb) -> [Vec3; 8] {
    let min = Vec3::from(aabb.min());
    let max = Vec3::from(aabb.max());
    [
        Vec3::new(min.x, min.y, min.z),
        Vec3::new(max.x, min.y, min.z),
        Vec3::new(min.x, max.y, min.z),
        Vec3::new(max.x, max.y, min.z),
        Vec3::new(min.x, min.y, max.z),
        Vec3::new(max.x, min.y, max.z),
        Vec3::new(min.x, max.y, max.z),
        Vec3::new(max.x, max.y, max.z),
    ]
}
