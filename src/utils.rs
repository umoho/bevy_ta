use bevy::{camera::primitives::Aabb, prelude::*};

pub(crate) fn world_aabb_for_root(
    root: Entity,
    children: &Query<&Children>,
    mesh_boxes: &Query<(&Aabb, &GlobalTransform), With<Mesh3d>>,
) -> Option<Aabb> {
    let mut points = Vec::new();
    for descendant in children
        .iter_descendants(root)
        .filter_map(|entity| mesh_boxes.get(entity).ok())
    {
        collect_world_aabb_points(&mut points, descendant.0, descendant.1);
    }

    Aabb::enclosing(points.iter().copied())
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
