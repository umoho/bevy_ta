use bevy::{
    camera::visibility::RenderLayers,
    light::NotShadowCaster,
    mesh::{MeshVertexBufferLayoutRef, skinning::SkinnedMesh},
    pbr::{Material, MaterialPipeline, MaterialPipelineKey},
    prelude::*,
    reflect::TypePath,
    render::render_resource::{
        AsBindGroup, Face, RenderPipelineDescriptor, ShaderType, SpecializedMeshPipelineError,
    },
    shader::ShaderRef,
};

use crate::npr::toon::ToonMaterial;

const OUTLINE_SHADER_PATH: &str = "shaders/npr/outline.wgsl";

pub struct OutlinePlugin;

impl Plugin for OutlinePlugin {
    fn build(&self, app: &mut App) {
        app.add_plugins(MaterialPlugin::<OutlineMaterial>::default())
            .add_systems(Update, (spawn_outline_meshes, sync_outline_materials));
    }
}

#[derive(Component)]
struct OutlineTarget {
    child: Entity,
}

#[derive(Component)]
struct OutlineMesh;

#[derive(Asset, TypePath, AsBindGroup, Debug, Clone)]
struct OutlineMaterial {
    #[uniform(0)]
    params: OutlineParams,
}

#[derive(Clone, Debug, ShaderType)]
struct OutlineParams {
    width: f32,
    _padding: Vec3,
    color: LinearRgba,
}

impl Material for OutlineMaterial {
    fn vertex_shader() -> ShaderRef {
        OUTLINE_SHADER_PATH.into()
    }

    fn fragment_shader() -> ShaderRef {
        OUTLINE_SHADER_PATH.into()
    }

    fn enable_prepass() -> bool {
        false
    }

    fn enable_shadows() -> bool {
        false
    }

    fn specialize(
        _pipeline: &MaterialPipeline,
        descriptor: &mut RenderPipelineDescriptor,
        _layout: &MeshVertexBufferLayoutRef,
        _key: MaterialPipelineKey<Self>,
    ) -> Result<(), SpecializedMeshPipelineError> {
        descriptor.primitive.cull_mode = Some(Face::Front);
        Ok(())
    }
}

fn spawn_outline_meshes(
    mut commands: Commands,
    mut outline_materials: ResMut<Assets<OutlineMaterial>>,
    toon_meshes: Query<
        (
            Entity,
            &Mesh3d,
            &MeshMaterial3d<ToonMaterial>,
            Option<&SkinnedMesh>,
            Option<&RenderLayers>,
        ),
        (
            Added<MeshMaterial3d<ToonMaterial>>,
            Without<OutlineTarget>,
            Without<OutlineMesh>,
        ),
    >,
    toon_materials: Res<Assets<ToonMaterial>>,
) {
    for (entity, mesh, toon_material_handle, skinned_mesh, render_layers) in toon_meshes {
        let Some(toon_material) = toon_materials.get(toon_material_handle.0.id()) else {
            continue;
        };

        let mut child = commands.spawn((
            Mesh3d(mesh.0.clone()),
            MeshMaterial3d(outline_materials.add(OutlineMaterial {
                params: OutlineParams {
                    width: toon_material.params.outline_width,
                    _padding: Vec3::ZERO,
                    color: toon_material.params.outline_color,
                },
            })),
            Visibility::Hidden,
            Transform::default(),
            OutlineMesh,
            NotShadowCaster,
        ));

        // 蒙皮和渲染层必须跟随原网格，否则角色动画或分层相机会让描边错位。
        if let Some(skinned_mesh) = skinned_mesh {
            child.insert(skinned_mesh.clone());
        }
        if let Some(render_layers) = render_layers {
            child.insert(render_layers.clone());
        }
        let child = child.id();

        commands
            .entity(entity)
            .add_child(child)
            .insert(OutlineTarget { child });
    }
}

fn sync_outline_materials(
    toon_meshes: Query<
        (&Mesh3d, &MeshMaterial3d<ToonMaterial>, &OutlineTarget),
        Without<OutlineMesh>,
    >,
    toon_materials: Res<Assets<ToonMaterial>>,
    mut outline_meshes: Query<
        (
            &mut Mesh3d,
            &MeshMaterial3d<OutlineMaterial>,
            &mut Visibility,
        ),
        With<OutlineMesh>,
    >,
    mut outline_materials: ResMut<Assets<OutlineMaterial>>,
) {
    for (mesh, toon_material_handle, outline_target) in toon_meshes {
        let Some(toon_material) = toon_materials.get(toon_material_handle.0.id()) else {
            continue;
        };
        let Ok((mut outline_mesh, outline_material_handle, mut visibility)) =
            outline_meshes.get_mut(outline_target.child)
        else {
            continue;
        };

        outline_mesh.0 = mesh.0.clone();

        if let Some(outline_material) = outline_materials.get_mut(outline_material_handle.0.id()) {
            outline_material.params.width = toon_material.params.outline_width;
            outline_material.params.color = toon_material.params.outline_color;
        }

        *visibility = if toon_material.params.outline_enabled != 0
            && toon_material.params.outline_width > 0.0
        {
            Visibility::Inherited
        } else {
            Visibility::Hidden
        };
    }
}
