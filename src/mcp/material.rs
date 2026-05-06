use std::path::{Path, PathBuf};

use bevy::prelude::*;
use bevy_remote::{BrpError, BrpResult, RemoteMethodSystemId, RemoteMethods};
use serde::Deserialize;
use serde_json::{Value, json};

use crate::npr::{
    profile::{
        CHARACTER_SURFACE_SHADER_KEY, CharacterRenderProfile, ModelBinding, PROFILE_VERSION,
        RenderPartBinding, RenderPartResources, ShaderProfileRegistry,
        character_render_profile_path,
    },
    toon::{ToonMaterial, ToonMaterialBindingSource},
};

use super::{
    METHOD_PREFIX, McpSaveToonProfile, McpSetMaterialParam, absolute_path, internal_error,
    invalid_params, parse_entity_bits, parse_params,
};

pub(crate) fn register_mcp_methods(world: &mut World) {
    let methods = [
        (
            "list_material_params",
            world.register_system(list_material_params_handler),
        ),
        (
            "set_material_param",
            world.register_system(set_material_param_handler),
        ),
    ];

    let mut remote_methods = world.resource_mut::<RemoteMethods>();
    for (name, system_id) in methods {
        remote_methods.insert(
            format!("{METHOD_PREFIX}{name}"),
            RemoteMethodSystemId::Instant(system_id),
        );
    }
}

fn list_material_params_handler(In(_params): In<Option<Value>>, world: &mut World) -> BrpResult {
    let mut query = world.query::<(
        Entity,
        Option<&Name>,
        &MeshMaterial3d<ToonMaterial>,
        Option<&ToonMaterialBindingSource>,
    )>();

    let material_refs = query
        .iter(world)
        .map(|(entity, name, handle, source)| {
            (
                entity,
                name.map(|name| name.as_str().to_string()),
                handle.id(),
                source.map(|source| source.node_name.clone()),
                normalized_shader_key(source).to_string(),
            )
        })
        .collect::<Vec<_>>();

    let materials = world.resource::<Assets<ToonMaterial>>();
    let profile_registry = world.resource::<ShaderProfileRegistry>();
    let mut entries = Vec::new();
    for (entity, name, material_id, node_name, shader_key) in material_refs {
        let Some(material) = materials.get(material_id) else {
            continue;
        };
        let params = profile_registry
            .snapshot_material_params(&shader_key, material)
            .map_err(internal_error)?;
        entries.push(json!({
            "entity": entity.to_bits(),
            "name": name,
            "material_id": format!("{:?}", material_id),
            "node_name": node_name,
            "shader_key": shader_key,
            "params": params,
        }));
    }

    Ok(json!({ "materials": entries }))
}

#[derive(Deserialize)]
struct SetMaterialParamParams {
    entity: Option<u64>,
    node_name: Option<String>,
    shader_key: Option<String>,
    field: String,
    value: Value,
    #[serde(default)]
    apply_all: bool,
}

fn set_material_param_handler(In(params): In<Option<Value>>, world: &mut World) -> BrpResult {
    let params: SetMaterialParamParams = parse_params(params)?;
    let target_entity = params.entity.map(parse_entity_bits).transpose()?;

    let target_materials = {
        let mut query = world.query::<(
            Entity,
            &MeshMaterial3d<ToonMaterial>,
            Option<&ToonMaterialBindingSource>,
        )>();

        query
            .iter(world)
            .filter_map(|(entity, handle, source)| {
                if material_source_matches(
                    entity,
                    source,
                    target_entity,
                    params.node_name.as_deref(),
                    params.shader_key.as_deref(),
                    params.apply_all,
                ) {
                    Some((handle.id(), normalized_shader_key(source).to_string()))
                } else {
                    None
                }
            })
            .collect::<Vec<_>>()
    };

    if target_materials.is_empty() {
        return Err(invalid_params(
            "No matching material found. Pass `entity`, `node_name`, `shader_key`, or `apply_all: true`.",
        ));
    }

    let changed = world.resource_scope(|world, mut materials: Mut<Assets<ToonMaterial>>| {
        let profile_registry = world.resource::<ShaderProfileRegistry>();
        let mut changed = 0usize;
        for (id, shader_key) in target_materials {
            if let Some(material) = materials.get_mut(id) {
                profile_registry
                    .set_material_param(&shader_key, material, &params.field, &params.value)
                    .map_err(invalid_params)?;
                changed += 1;
            }
        }
        Ok::<usize, BrpError>(changed)
    })?;

    Ok(json!({
        "success": true,
        "field": params.field,
        "changed_count": changed,
    }))
}

pub(crate) fn handle_mcp_set_material_param(
    event: On<McpSetMaterialParam>,
    query: Query<(
        Entity,
        &MeshMaterial3d<ToonMaterial>,
        Option<&ToonMaterialBindingSource>,
    )>,
    mut materials: ResMut<Assets<ToonMaterial>>,
    profile_registry: Res<ShaderProfileRegistry>,
) {
    let target_entity = match event.entity.map(parse_entity_bits).transpose() {
        Ok(entity) => entity,
        Err(error) => {
            error!("MCP set material param event failed: {}", error.message);
            return;
        }
    };

    let Some(value) = material_param_event_value(&event) else {
        error!("MCP set material param event failed: pass exactly one of number, boolean, or vec4");
        return;
    };

    let target_materials = query
        .iter()
        .filter_map(|(entity, handle, source)| {
            if material_source_matches(
                entity,
                source,
                target_entity,
                event.node_name.as_deref(),
                event.shader_key.as_deref(),
                event.apply_all,
            ) {
                Some((handle.id(), normalized_shader_key(source).to_string()))
            } else {
                None
            }
        })
        .collect::<Vec<_>>();

    if target_materials.is_empty() {
        error!("MCP set material param event failed: no matching material found");
        return;
    }

    let mut changed = 0usize;
    for (id, shader_key) in target_materials {
        if let Some(material) = materials.get_mut(id) {
            match profile_registry.set_material_param(&shader_key, material, &event.field, &value) {
                Ok(()) => changed += 1,
                Err(error) => {
                    error!("MCP set material param event failed: {error}");
                    return;
                }
            }
        }
    }

    info!(
        "MCP set material param field={} changed_count={}",
        event.field, changed
    );
}

pub(crate) fn handle_mcp_save_toon_profile(
    event: On<McpSaveToonProfile>,
    query: Query<(
        Entity,
        &MeshMaterial3d<ToonMaterial>,
        Option<&ToonMaterialBindingSource>,
    )>,
    materials: Res<Assets<ToonMaterial>>,
    profile_registry: Res<ShaderProfileRegistry>,
) {
    let target_entity = match event.entity.map(parse_entity_bits).transpose() {
        Ok(entity) => entity,
        Err(error) => {
            error!("MCP save toon profile event failed: {}", error.message);
            return;
        }
    };

    let mut saved_count = 0usize;
    for (entity, handle, source) in &query {
        if !material_source_matches(
            entity,
            source,
            target_entity,
            event.node_name.as_deref(),
            None,
            event.apply_all,
        ) {
            continue;
        }

        let Some(material) = materials.get(handle) else {
            continue;
        };
        let Some(source) = source else {
            continue;
        };

        let path = match toon_profile_save_path(event.path.as_deref(), source) {
            Ok(path) => path,
            Err(error) => {
                error!("MCP save toon profile event failed: {error}");
                return;
            }
        };

        match save_toon_material_profile(&path, source, material, &profile_registry) {
            Ok(()) => {
                saved_count += 1;
                info!(
                    "MCP saved toon profile entity={} node={} path={}",
                    entity.to_bits(),
                    source.node_name,
                    path.display()
                );
            }
            Err(error) => {
                error!("MCP save toon profile event failed: {error}");
                return;
            }
        }
    }

    if saved_count == 0 {
        error!("MCP save toon profile event failed: no matching ToonMaterial found");
    } else {
        info!("MCP save toon profile completed saved_count={saved_count}");
    }
}

fn material_source_matches(
    entity: Entity,
    source: Option<&ToonMaterialBindingSource>,
    target_entity: Option<Entity>,
    target_node_name: Option<&str>,
    target_shader_key: Option<&str>,
    apply_all: bool,
) -> bool {
    let has_target =
        target_entity.is_some() || target_node_name.is_some() || target_shader_key.is_some();
    let should_apply = apply_all || has_target;
    should_apply
        && target_entity.is_none_or(|target| target == entity)
        && target_node_name
            .is_none_or(|target| source.is_some_and(|source| source.node_name == target))
        && target_shader_key.is_none_or(|target| normalized_shader_key(source) == target)
}

fn toon_profile_save_path(
    explicit_path: Option<&str>,
    source: &ToonMaterialBindingSource,
) -> Result<PathBuf, String> {
    if let Some(path) = explicit_path.map(str::trim).filter(|path| !path.is_empty()) {
        return absolute_path(path).map_err(|error| error.message);
    }

    let Some(scene_asset_path) = &source.scene_asset_path else {
        return Err(
            "missing scene_asset_path; pass `path` or use a material loaded from a scene"
                .to_string(),
        );
    };

    absolute_path(&character_render_profile_path(scene_asset_path).to_string_lossy())
        .map_err(|error| error.message)
}

fn save_toon_material_profile(
    path: &Path,
    source: &ToonMaterialBindingSource,
    material: &ToonMaterial,
    profile_registry: &ShaderProfileRegistry,
) -> Result<(), String> {
    let mut profile =
        CharacterRenderProfile::load_from_path(path).unwrap_or_else(|_| CharacterRenderProfile {
            version: PROFILE_VERSION,
            model_binding: ModelBinding {
                scene_asset_path: source.scene_asset_path.clone(),
            },
            shared: Default::default(),
            parts: Vec::new(),
        });
    profile.version = PROFILE_VERSION;
    if source.scene_asset_path.is_some() {
        profile.model_binding.scene_asset_path = source.scene_asset_path.clone();
    }

    let existing_part = profile.find_part(&source.node_name).cloned();
    let resources = existing_part
        .as_ref()
        .map(|part| part.resources.clone())
        .unwrap_or_else(RenderPartResources::default);
    let shader_key = normalized_shader_key(Some(source)).to_string();
    let params = profile_registry
        .get(&shader_key)
        .ok_or_else(|| format!("未注册的 shader_key: {shader_key}"))?
        .capture_toon_material(material)?;

    profile.upsert_part(RenderPartBinding {
        binding_key: source.node_name.clone(),
        shader_key,
        resources,
        params,
    });
    profile.save_to_path(path)
}

fn material_param_event_value(event: &McpSetMaterialParam) -> Option<Value> {
    let provided_count = usize::from(event.number.is_some())
        + usize::from(event.boolean.is_some())
        + usize::from(event.vec4.is_some());
    if provided_count != 1 {
        return None;
    }

    if let Some(value) = event.number {
        Some(json!(value))
    } else if let Some(value) = event.boolean {
        Some(json!(value))
    } else {
        event.vec4.map(|value| json!(value))
    }
}

fn normalized_shader_key(source: Option<&ToonMaterialBindingSource>) -> &str {
    source
        .map(|source| source.shader_key.as_str())
        .filter(|shader_key| !shader_key.is_empty())
        .unwrap_or(CHARACTER_SURFACE_SHADER_KEY)
}
