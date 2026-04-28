#import bevy_pbr::{
    mesh_bindings::mesh,
    mesh_functions,
    morph::morph,
    skinning,
    forward_io::{Vertex, VertexOutput},
    mesh_view_bindings::view,
    view_transformations::position_world_to_clip,
}

struct OutlineParams {
    width: f32,
    _padding: vec3<f32>,
    color: vec4<f32>,
}

@group(#{MATERIAL_BIND_GROUP}) @binding(0) var<uniform> params: OutlineParams;

#ifdef MORPH_TARGETS
fn morph_vertex(vertex_in: Vertex) -> Vertex {
    var vertex = vertex_in;
    let first_vertex = mesh[vertex.instance_index].first_vertex_index;
    let vertex_index = vertex.index - first_vertex;

    let weight_count = bevy_pbr::morph::layer_count();
    for (var i: u32 = 0u; i < weight_count; i++) {
        let weight = bevy_pbr::morph::weight_at(i);
        if weight == 0.0 {
            continue;
        }
        vertex.position += weight * morph(vertex_index, bevy_pbr::morph::position_offset, i);
#ifdef VERTEX_NORMALS
        vertex.normal += weight * morph(vertex_index, bevy_pbr::morph::normal_offset, i);
#endif
    }

    return vertex;
}
#endif

@vertex
fn vertex(vertex_no_morph: Vertex) -> VertexOutput {
    var out: VertexOutput;

#ifdef MORPH_TARGETS
    var vertex = morph_vertex(vertex_no_morph);
#else
    var vertex = vertex_no_morph;
#endif

    let mesh_world_from_local = mesh_functions::get_world_from_local(vertex_no_morph.instance_index);

#ifdef SKINNED
    var world_from_local = skinning::skin_model(
        vertex.joint_indices,
        vertex.joint_weights,
        vertex_no_morph.instance_index
    );
#else
    var world_from_local = mesh_world_from_local;
#endif

#ifdef VERTEX_NORMALS
#ifdef SKINNED
    out.world_normal = normalize(skinning::skin_normals(world_from_local, vertex.normal));
#else
    out.world_normal = normalize(mesh_functions::mesh_normal_local_to_world(
        vertex.normal,
        vertex_no_morph.instance_index
    ));
#endif
#else
    out.world_normal = vec3<f32>(0.0, 1.0, 0.0);
#endif

    let world_position = mesh_functions::mesh_position_local_to_world(
        world_from_local,
        vec4<f32>(vertex.position, 1.0)
    );

    // 世界空间描边在近景容易过粗，这里按相机距离做一个温和补偿。
    let camera_distance = distance(world_position.xyz, view.world_position);
    let distance_scale = clamp(camera_distance * 0.08, 0.35, 1.25);
    out.world_position = vec4<f32>(
        world_position.xyz + out.world_normal * params.width * distance_scale,
        world_position.w
    );
    out.position = position_world_to_clip(out.world_position.xyz);

#ifdef VERTEX_OUTPUT_INSTANCE_INDEX
    out.instance_index = vertex_no_morph.instance_index;
#endif

    return out;
}

@fragment
fn fragment(_in: VertexOutput) -> @location(0) vec4<f32> {
    return params.color;
}
