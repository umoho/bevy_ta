#![cfg(feature = "dev_ui")]

use std::{
    collections::HashMap,
    path::{Path, PathBuf},
};

use bevy::prelude::*;
use bevy_egui::{
    EguiContexts, EguiPlugin, EguiPrimaryContextPass, EguiTextureHandle, PrimaryEguiContext, egui,
};
use bevy_material_preview::{MaterialPreviewAppExt, MaterialPreviewPlugin, MaterialPreviewSession};

use crate::npr::{
    profile::{
        CharacterRenderProfile, ModelBinding, PROFILE_VERSION, RenderPartBinding,
        RenderPartResources, SceneInteractionParams, ShaderProfileRegistry, SurfaceProfileParams,
        SurfaceRegionMaskMode, SurfaceRegionParams, character_render_profile_path,
        ron_value_from_serializable, ron_value_into,
    },
    toon::{
        RampData, RampInterpolation, RampStop, ToonMaterial, ToonMaterialBindingSource,
        ToonMaterialTarget, ToonParams, default_ramp_data, rebuild_ramp_texture,
    },
};

pub struct MaterialEditorPlugin;

impl Plugin for MaterialEditorPlugin {
    fn build(&self, app: &mut App) {
        app.add_plugins((EguiPlugin::default(), MaterialPreviewPlugin::default()))
            .register_material_preview::<ToonMaterial>()
            .init_resource::<SelectedToonMaterial>()
            .add_systems(
                Update,
                (
                    setup_chinese_fonts
                        .run_if(any_with_component::<PrimaryEguiContext>)
                        .run_if(run_once),
                    spawn_material_icon_previews,
                ),
            )
            .add_systems(
                EguiPrimaryContextPass,
                (
                    bridge_preview_to_egui::<ToonMaterial>,
                    bridge_ramp_texture_to_egui,
                    spawn_property_preview,
                    despawn_property_previews,
                    (show_material_library_panel, show_material_property_panel).chain(),
                ),
            );
    }
}

#[derive(Resource, Default)]
struct SelectedToonMaterial {
    icon_entity: Option<Entity>,
    property_preview_entity: Option<Entity>,
}

#[derive(Component, Clone)]
struct MaterialHandle<M: Material>(Handle<M>);

#[derive(Component)]
struct RampTextureHandle(Handle<Image>);

#[derive(Component)]
struct MaterialSourceInfo {
    source_nodes: Vec<MaterialSourceNode>,
    scene_asset_path: Option<String>,
    binding_file_path: Option<PathBuf>,
    shader_key: String,
}

#[derive(Clone)]
struct MaterialSourceNode {
    mesh_entity: Entity,
    node_name: String,
}

#[derive(Component)]
struct PreviewTextureId(egui::TextureId);

#[derive(Component)]
struct RampTextureId(egui::TextureId);

#[derive(Component)]
struct PropertyPreview;

#[derive(Default)]
struct RampEditorState {
    selected_stop_index: usize,
}

#[derive(Default)]
struct MaterialPersistenceState {
    status_message: Option<String>,
}

#[derive(Default)]
struct SurfaceProfileEditorState {
    selected_entity: Option<Entity>,
    resources: RenderPartResources,
    params: SurfaceProfileParams,
}

fn setup_chinese_fonts(mut contexts: EguiContexts) {
    let Ok(ctx) = contexts.ctx_mut() else {
        return;
    };
    if let Err(err) = egui_chinese_font::setup_chinese_fonts(ctx) {
        warn!("不能设置中文字体: {err:?}");
    }
}

fn spawn_material_icon_previews(
    mut commands: Commands,
    parent_query: Query<&ChildOf>,
    toon_targets: Query<(), With<ToonMaterialTarget>>,
    mesh_materials: Query<
        (
            Entity,
            &MeshMaterial3d<ToonMaterial>,
            Option<&Name>,
            Option<&ToonMaterialBindingSource>,
        ),
        Added<MeshMaterial3d<ToonMaterial>>,
    >,
    existing_previews: Query<&MaterialHandle<ToonMaterial>, Without<PropertyPreview>>,
    materials: Res<Assets<ToonMaterial>>,
) {
    let mut existing_materials = existing_previews
        .iter()
        .map(|handle| handle.0.id())
        .collect::<Vec<_>>();
    let mut preview_sources = HashMap::<
        AssetId<ToonMaterial>,
        (
            Handle<ToonMaterial>,
            Vec<MaterialSourceNode>,
            Option<String>,
            Option<PathBuf>,
            String,
        ),
    >::new();

    for (mesh_entity, material_handle, name, binding_source) in mesh_materials.iter() {
        // 只收集真正业务模型上的 toon 网格，避免材质预览球自己再次触发预览递归生成。
        let is_target_mesh = toon_targets.contains(mesh_entity)
            || parent_query
                .iter_ancestors(mesh_entity)
                .any(|ancestor| toon_targets.contains(ancestor));
        if !is_target_mesh {
            continue;
        }

        let material_id = material_handle.0.id();
        if existing_materials.contains(&material_id) {
            continue;
        }

        let node_name = binding_source
            .map(|source| source.node_name.clone())
            .or_else(|| name.map(ToString::to_string))
            .unwrap_or_else(|| format!("节点 {}", mesh_entity.index()));
        let scene_asset_path = binding_source.and_then(|source| source.scene_asset_path.clone());
        let shader_key = binding_source
            .map(|source| source.shader_key.clone())
            .unwrap_or_else(|| "surface".to_string());
        let binding_file_path = scene_asset_path
            .as_deref()
            .map(character_render_profile_path);

        preview_sources
            .entry(material_id)
            .or_insert_with(|| {
                (
                    material_handle.0.clone(),
                    Vec::new(),
                    scene_asset_path.clone(),
                    binding_file_path.clone(),
                    shader_key.clone(),
                )
            })
            .1
            .push(MaterialSourceNode {
                mesh_entity,
                node_name,
            });
        existing_materials.push(material_id);
    }

    for (
        material_id,
        (material_handle, mut source_nodes, scene_asset_path, binding_file_path, shader_key),
    ) in preview_sources
    {
        let Some(material) = materials.get(material_id) else {
            continue;
        };
        source_nodes.sort_by(|left, right| {
            left.node_name
                .cmp(&right.node_name)
                .then_with(|| left.mesh_entity.index().cmp(&right.mesh_entity.index()))
        });

        commands.spawn((
            MaterialPreviewSession {
                material: material_handle.clone(),
                size: UVec2::splat(192),
                ..Default::default()
            },
            MaterialHandle(material_handle),
            RampTextureHandle(material.ramp_texture.clone()),
            MaterialSourceInfo {
                source_nodes,
                scene_asset_path,
                binding_file_path,
                shader_key,
            },
        ));
    }
}

fn bridge_preview_to_egui<M: Material>(
    mut commands: Commands,
    mut contexts: EguiContexts,
    query: Query<(Entity, &MaterialPreviewSession<M>), Without<PreviewTextureId>>,
) {
    for (entity, session) in query.iter() {
        let Some(image_handle) = &session.target else {
            continue;
        };
        let texture_id = contexts.add_image(EguiTextureHandle::Weak(image_handle.id()));
        commands.entity(entity).insert(PreviewTextureId(texture_id));
    }
}

fn bridge_ramp_texture_to_egui(
    mut commands: Commands,
    mut contexts: EguiContexts,
    query: Query<(Entity, &RampTextureHandle), Without<RampTextureId>>,
) {
    for (entity, ramp_texture) in query.iter() {
        let texture_id = contexts.add_image(EguiTextureHandle::Weak(ramp_texture.0.id()));
        commands.entity(entity).insert(RampTextureId(texture_id));
    }
}

fn spawn_property_preview(
    mut commands: Commands,
    mut selected: ResMut<SelectedToonMaterial>,
    material_handles: Query<&MaterialHandle<ToonMaterial>>,
    mut tracker: Local<Option<AssetId<ToonMaterial>>>,
) {
    let Some(selected_entity) = selected.icon_entity else {
        return;
    };
    let Ok(handle) = material_handles.get(selected_entity) else {
        return;
    };
    let asset_id = handle.0.id();
    if tracker.as_ref().copied() == Some(asset_id) {
        return;
    }

    let entity = commands
        .spawn((
            MaterialPreviewSession {
                material: handle.0.clone(),
                size: UVec2::new(400, 320),
                with_plane: true,
                distance_offset: 0.5,
                ..Default::default()
            },
            PropertyPreview,
        ))
        .id();
    selected.property_preview_entity = Some(entity);
    *tracker = Some(asset_id);
}

fn despawn_property_previews(
    mut commands: Commands,
    selected: Res<SelectedToonMaterial>,
    property_previews: Query<Entity, With<PropertyPreview>>,
) {
    let Some(selected_entity) = selected.property_preview_entity else {
        return;
    };
    for entity in property_previews.iter() {
        if entity != selected_entity {
            commands.entity(entity).despawn();
        }
    }
}

fn show_material_library_panel(
    mut contexts: EguiContexts,
    previews: Query<(Entity, &PreviewTextureId, &MaterialSourceInfo), Without<PropertyPreview>>,
    mut selected: ResMut<SelectedToonMaterial>,
) {
    let Ok(ctx) = contexts.ctx_mut() else {
        return;
    };

    let mut entries = previews.iter().collect::<Vec<_>>();
    entries.sort_by(|left, right| {
        match (left.2.source_nodes.first(), right.2.source_nodes.first()) {
            (Some(left_node), Some(right_node)) => left_node
                .node_name
                .cmp(&right_node.node_name)
                .then_with(|| {
                    left_node
                        .mesh_entity
                        .index()
                        .cmp(&right_node.mesh_entity.index())
                }),
            (Some(_), None) => std::cmp::Ordering::Less,
            (None, Some(_)) => std::cmp::Ordering::Greater,
            (None, None) => std::cmp::Ordering::Equal,
        }
    });

    if selected.icon_entity.is_none() {
        selected.icon_entity = entries.first().map(|(entity, _, _)| *entity);
    }

    egui::TopBottomPanel::bottom("toon_material_library_panel")
        .resizable(true)
        .default_height(148.0)
        .show(ctx, |ui| {
            ui.heading("当前模型材质");
            egui::ScrollArea::horizontal()
                .id_salt("toon_material_library_scroll")
                .show(ui, |ui| {
                    ui.horizontal(|ui| {
                        for (entity, preview, source_info) in entries {
                            show_material_entry(ui, &mut selected, entity, preview.0, source_info);
                        }
                    });
                });
        });
}

fn show_material_entry(
    ui: &mut egui::Ui,
    selected: &mut SelectedToonMaterial,
    entity: Entity,
    preview_texture: egui::TextureId,
    source_info: &MaterialSourceInfo,
) {
    ui.vertical(|ui| {
        let is_selected = selected.icon_entity == Some(entity);
        #[allow(deprecated)]
        let image_button = egui::ImageButton::new(egui::load::SizedTexture::new(
            preview_texture,
            egui::Vec2::splat(84.0),
        ))
        .selected(is_selected);

        if ui.add(image_button).clicked() {
            selected.icon_entity = Some(entity);
        }
        let label = match source_info.source_nodes.first() {
            Some(first) if source_info.source_nodes.len() > 1 => {
                format!(
                    "{} 等 {} 个节点",
                    first.node_name,
                    source_info.source_nodes.len()
                )
            }
            Some(first) => first.node_name.clone(),
            None => "未命名节点".to_string(),
        };
        ui.add_sized(
            egui::vec2(96.0, 0.0),
            egui::Label::new(egui::RichText::new(label).small()),
        );
    });
}

fn show_material_property_panel(
    mut contexts: EguiContexts,
    selected: Res<SelectedToonMaterial>,
    material_handles: Query<&MaterialHandle<ToonMaterial>>,
    source_infos: Query<&MaterialSourceInfo>,
    property_previews: Query<&PreviewTextureId, With<PropertyPreview>>,
    ramp_textures: Query<&RampTextureId>,
    mut materials: ResMut<Assets<ToonMaterial>>,
    mut images: ResMut<Assets<Image>>,
    profile_registry: Res<ShaderProfileRegistry>,
    mut surface_profile_editor_state: Local<SurfaceProfileEditorState>,
    mut ramp_editor_state: Local<RampEditorState>,
    mut persistence_state: Local<MaterialPersistenceState>,
) {
    let Some(selected_entity) = selected.icon_entity else {
        return;
    };
    let Ok(material_handle) = material_handles.get(selected_entity) else {
        return;
    };
    let Ok(source_info) = source_infos.get(selected_entity) else {
        return;
    };
    let Some(material) = materials.get_mut(material_handle.0.id()) else {
        return;
    };
    let property_preview_texture = selected
        .property_preview_entity
        .and_then(|entity| property_previews.get(entity).ok())
        .map(|preview| preview.0);
    let ramp_texture = ramp_textures
        .get(selected_entity)
        .ok()
        .map(|texture| texture.0);
    sync_surface_profile_editor_state(
        selected_entity,
        source_info,
        material,
        &mut surface_profile_editor_state,
    );
    let Ok(ctx) = contexts.ctx_mut() else {
        return;
    };

    egui::SidePanel::right("toon_material_property_panel")
        .resizable(true)
        .default_width(380.0)
        .show(ctx, |ui| {
            show_selected_material_preview(ui, property_preview_texture, source_info);
            egui::ScrollArea::vertical()
                .id_salt("toon_material_property_scroll")
                .show(ui, |ui| {
                    show_base_editor(ui, material);
                    ui.add_space(10.0);
                    ui.separator();
                    ui.add_space(10.0);
                    show_model_binding_editor(
                        ui,
                        &mut images,
                        material,
                        source_info,
                        &profile_registry,
                        &mut surface_profile_editor_state,
                        &mut persistence_state,
                    );
                    ui.add_space(10.0);
                    ui.separator();
                    ui.add_space(10.0);
                    show_ramp_editor(
                        ui,
                        &mut images,
                        material,
                        ramp_texture,
                        &mut ramp_editor_state.selected_stop_index,
                    );
                    ui.add_space(10.0);
                    ui.separator();
                    ui.add_space(10.0);
                    show_toon_params_editor(ui, &mut material.params);
                });
        });
}

fn show_selected_material_preview(
    ui: &mut egui::Ui,
    preview_texture: Option<egui::TextureId>,
    source_info: &MaterialSourceInfo,
) {
    ui.vertical_centered(|ui| {
        let display_size = egui::vec2(220.0, 176.0);
        if let Some(preview_texture) = preview_texture {
            #[allow(deprecated)]
            ui.add(
                egui::Image::new(egui::load::SizedTexture::new(preview_texture, display_size))
                    .rounding(8.0),
            );
        } else {
            ui.add_sized(display_size, egui::Spinner::new());
        }
        ui.add_space(10.0);
        ui.heading("当前节点材质");
        if let Some(first_source) = source_info.source_nodes.first() {
            if source_info.source_nodes.len() > 1 {
                ui.label(
                    egui::RichText::new(format!(
                        "{} 等 {} 个节点",
                        first_source.node_name,
                        source_info.source_nodes.len()
                    ))
                    .italics(),
                );
            } else {
                ui.label(egui::RichText::new(&first_source.node_name).italics());
            }
        } else {
            ui.label(egui::RichText::new("未命名节点").italics());
        }
        for source_node in source_info.source_nodes.iter().take(4) {
            ui.small(format!(
                "{} ({:?})",
                source_node.node_name, source_node.mesh_entity
            ));
        }
        if source_info.source_nodes.len() > 4 {
            ui.small(format!(
                "还有 {} 个节点...",
                source_info.source_nodes.len() - 4
            ));
        }
        if let Some(path) = &source_info.binding_file_path {
            ui.small(format!("模型绑定 {}", path.display()));
        } else {
            ui.small("当前节点没有模型绑定文件");
        }
        ui.small(format!("着色器类型 {}", source_info.shader_key));
        ui.add_space(6.0);
    });
}

fn show_base_editor(ui: &mut egui::Ui, material: &mut ToonMaterial) {
    ui.group(|ui| {
        ui.label(egui::RichText::new("基础属性").strong());
        ui.add_space(5.0);
        show_linear_rgba_editor(ui, "基础颜色", &mut material.params.base_color);
        ui.add(egui::Slider::new(&mut material.params.alpha_cutoff, 0.0..=1.0).text("透明裁剪"));
        show_u32_checkbox(
            ui,
            "使用基础色贴图",
            &mut material.params.use_base_color_texture,
        );
    });
}

fn show_model_binding_editor(
    ui: &mut egui::Ui,
    images: &mut Assets<Image>,
    material: &mut ToonMaterial,
    source_info: &MaterialSourceInfo,
    profile_registry: &ShaderProfileRegistry,
    surface_profile_editor_state: &mut SurfaceProfileEditorState,
    persistence_state: &mut MaterialPersistenceState,
) {
    ui.group(|ui| {
        ui.label(egui::RichText::new("模型绑定").strong());
        if let Some(scene_asset_path) = &source_info.scene_asset_path {
            ui.small(format!("模型资源 {}", scene_asset_path));
        }
        if let Some(path) = &source_info.binding_file_path {
            ui.small(format!("绑定文件 {}", path.display()));
            ui.horizontal(|ui| {
                if ui.button("保存到模型").clicked() {
                    persistence_state.status_message = Some(save_material_profile(
                        path,
                        source_info,
                        profile_registry,
                        material,
                        surface_profile_editor_state,
                    ));
                }

                if ui.button("从模型恢复").clicked() {
                    persistence_state.status_message = Some(
                        match load_material_profile(
                            path,
                            source_info,
                            profile_registry,
                            material,
                            images,
                            surface_profile_editor_state,
                        ) {
                            Ok(()) => format!("已恢复 {}", path.display()),
                            Err(err) => err,
                        },
                    );
                }

                if ui.button("恢复默认").clicked() {
                    let base_color = material.params.base_color;
                    material.params = ToonParams {
                        base_color,
                        use_base_color_texture: material.params.use_base_color_texture,
                        ..Default::default()
                    };
                    material.ramp_data = default_ramp_data();
                    rebuild_ramp_texture(images, &material.ramp_texture, &material.ramp_data);
                    persistence_state.status_message = Some("已恢复默认 toon 参数".to_string());
                }
            });
        } else {
            ui.small("当前材质不来自可绑定的模型资源，只能临时调试。");
        }

        if let Some(status_message) = &persistence_state.status_message {
            ui.small(status_message);
        }
    });

    show_surface_profile_editor(ui, surface_profile_editor_state);
}

fn show_ramp_editor(
    ui: &mut egui::Ui,
    images: &mut Assets<Image>,
    material: &mut ToonMaterial,
    ramp_texture: Option<egui::TextureId>,
    selected_stop_index: &mut usize,
) {
    ui.group(|ui| {
        ui.label(egui::RichText::new("渐变纹理 (Ramp)").strong());
        ui.add_space(5.0);
        if let Some(ramp_texture) = ramp_texture {
            ui.add(egui::Image::new(egui::load::SizedTexture::new(
                ramp_texture,
                egui::vec2(280.0, 28.0),
            )));
        } else {
            ui.add_sized(egui::vec2(280.0, 28.0), egui::Spinner::new());
        }
        ui.add_space(6.0);

        let mut changed = show_ramp_stop_track(ui, &mut material.ramp_data, selected_stop_index);
        *selected_stop_index =
            (*selected_stop_index).min(material.ramp_data.stops.len().saturating_sub(1));

        if let Some(stop) = material.ramp_data.stops.get_mut(*selected_stop_index) {
            ui.horizontal(|ui| {
                ui.label("当前位置");
                changed |= ui
                    .add(egui::Slider::new(&mut stop.position, 0.0..=1.0).show_value(true))
                    .changed();
            });
            changed |= show_linear_rgba_editor(ui, "颜色", &mut stop.color);
        }

        ui.horizontal(|ui| {
            if ui.button("+").clicked() {
                material.ramp_data.stops.push(RampStop {
                    position: 0.5,
                    color: LinearRgba::WHITE,
                });
                *selected_stop_index = material.ramp_data.stops.len() - 1;
                changed = true;
            }

            let can_remove = material.ramp_data.stops.len() > 2;
            if ui.add_enabled(can_remove, egui::Button::new("-")).clicked() {
                material.ramp_data.stops.remove(*selected_stop_index);
                *selected_stop_index =
                    (*selected_stop_index).min(material.ramp_data.stops.len().saturating_sub(1));
                changed = true;
            }

            if ui.button("重置默认").clicked() {
                material.ramp_data = default_ramp_data();
                *selected_stop_index = 0;
                changed = true;
            }
        });

        ui.horizontal(|ui| {
            ui.label("插值模式");
            changed |= ui
                .selectable_value(
                    &mut material.ramp_data.interpolation,
                    RampInterpolation::Constant,
                    "常量",
                )
                .changed();
            changed |= ui
                .selectable_value(
                    &mut material.ramp_data.interpolation,
                    RampInterpolation::Linear,
                    "线性",
                )
                .changed();
        });

        changed |= ui
            .add(egui::Slider::new(&mut material.ramp_data.resolution, 2..=64).text("分辨率"))
            .changed();

        if changed {
            normalize_ramp_data(&mut material.ramp_data);
            rebuild_ramp_texture(images, &material.ramp_texture, &material.ramp_data);
        }
    });
}

fn show_toon_params_editor(ui: &mut egui::Ui, params: &mut ToonParams) {
    show_param_group(ui, "toon_shadow_group", "阴影", None, true, |ui| {
        ui.add(egui::Slider::new(&mut params.shade_threshold, 0.0..=1.0).text("阴影阈值"));
        ui.add(egui::Slider::new(&mut params.shade_softness, 0.0..=1.0).text("阴影柔和度"));
        ui.add(egui::Slider::new(&mut params.shadow_strength, 0.0..=1.0).text("暗部强度"));
        ui.add(egui::Slider::new(&mut params.lit_boost, 0.0..=2.0).text("亮部增强"));
        ui.add(egui::Slider::new(&mut params.ambient_strength, 0.0..=1.0).text("环境光"));
    });

    show_param_group(
        ui,
        "toon_specular_group",
        "高光",
        Some(&mut params.specular_enabled),
        false,
        |ui| {
            ui.add(egui::Slider::new(&mut params.specular_strength, 0.0..=1.0).text("高光强度"));
            render_skewed_slider(
                ui,
                &mut params.specular_threshold,
                "高光阈值",
                0.5,
                1.0,
                2.0,
                true,
            );
            ui.add(egui::Slider::new(&mut params.specular_softness, 0.0..=0.5).text("高光柔和度"));
            show_linear_rgba_editor(ui, "高光颜色", &mut params.specular_color);
        },
    );

    show_param_group(
        ui,
        "toon_outline_group",
        "描边",
        Some(&mut params.outline_enabled),
        true,
        |ui| {
            ui.add(egui::Slider::new(&mut params.outline_width, 0.0..=0.2).text("描边宽度"));
            show_linear_rgba_editor(ui, "描边颜色", &mut params.outline_color);
        },
    );

    show_param_group(
        ui,
        "toon_rim_group",
        "边缘光",
        Some(&mut params.rim_enabled),
        false,
        |ui| {
            ui.add(egui::Slider::new(&mut params.rim_strength, 0.0..=2.0).text("边缘光强度"));
            ui.add(egui::Slider::new(&mut params.rim_threshold, 0.0..=1.0).text("边缘光阈值"));
            ui.add(egui::Slider::new(&mut params.rim_softness, 0.0..=1.0).text("边缘光柔和度"));
            show_linear_rgba_editor(ui, "边缘光颜色", &mut params.rim_color);
        },
    );
}

fn show_param_group(
    ui: &mut egui::Ui,
    id_source: &'static str,
    title: &'static str,
    toggle: Option<&mut u32>,
    default_open: bool,
    add_body: impl FnOnce(&mut egui::Ui),
) {
    let id = ui.make_persistent_id(id_source);
    egui::collapsing_header::CollapsingState::load_with_default_open(ui.ctx(), id, default_open)
        .show_header(ui, |ui| {
            if let Some(toggle) = toggle {
                let mut enabled = *toggle != 0;
                if ui.checkbox(&mut enabled, "").changed() {
                    *toggle = u32::from(enabled);
                }
            }
            ui.label(egui::RichText::new(title).strong());
        })
        .body(|ui| {
            ui.add_space(4.0);
            add_body(ui);
        });
    ui.add_space(6.0);
}

fn show_linear_rgba_editor(ui: &mut egui::Ui, label: &str, color: &mut LinearRgba) -> bool {
    let mut changed = false;
    ui.horizontal(|ui| {
        ui.label(label);
        let mut rgba = color.to_f32_array();
        if ui.color_edit_button_rgba_unmultiplied(&mut rgba).changed() {
            *color = LinearRgba::from_f32_array(rgba);
            changed = true;
        }
    });
    changed
}

fn show_u32_checkbox(ui: &mut egui::Ui, label: &str, value: &mut u32) -> bool {
    let mut checked = *value != 0;
    let changed = ui.checkbox(&mut checked, label).changed();
    if changed {
        *value = u32::from(checked);
    }
    changed
}

fn show_ramp_stop_track(
    ui: &mut egui::Ui,
    ramp_data: &mut RampData,
    selected_stop_index: &mut usize,
) -> bool {
    const TRACK_WIDTH: f32 = 280.0;
    const TRACK_HEIGHT: f32 = 34.0;
    const STOP_MARKER_WIDTH: f32 = 12.0;
    const STOP_MARKER_HEIGHT: f32 = 14.0;
    const TRACK_PADDING_X: f32 = 10.0;
    const TRACK_PADDING_Y: f32 = 10.0;

    let mut changed = false;
    let desired_size = egui::vec2(TRACK_WIDTH, TRACK_HEIGHT);
    let (rect, _) = ui.allocate_exact_size(desired_size, egui::Sense::hover());
    let painter = ui.painter_at(rect);

    let track_rect = egui::Rect::from_min_max(
        rect.left_top() + egui::vec2(TRACK_PADDING_X, TRACK_PADDING_Y),
        rect.right_bottom() - egui::vec2(TRACK_PADDING_X, TRACK_PADDING_Y),
    );
    painter.rect_filled(track_rect, 3.0, ui.visuals().faint_bg_color);

    for (index, stop) in ramp_data.stops.iter_mut().enumerate() {
        let center_x = egui::lerp(track_rect.x_range(), stop.position.clamp(0.0, 1.0));
        let marker_rect = egui::Rect::from_center_size(
            egui::pos2(center_x, rect.center().y),
            egui::vec2(STOP_MARKER_WIDTH, STOP_MARKER_HEIGHT),
        );
        let response = ui.interact(
            marker_rect.expand2(egui::vec2(4.0, 4.0)),
            ui.id().with(("ramp_stop", index)),
            egui::Sense::click_and_drag(),
        );
        if response.clicked() {
            *selected_stop_index = index;
        }
        if response.dragged() {
            *selected_stop_index = index;
            if let Some(pointer_pos) = response.interact_pointer_pos() {
                let width = track_rect.width().max(1.0);
                stop.position = ((pointer_pos.x - track_rect.left()) / width).clamp(0.0, 1.0);
                changed = true;
            }
        }

        painter.rect_filled(marker_rect, 2.0, to_egui_color(stop.color));
        let stroke_color = if index == *selected_stop_index {
            ui.visuals().selection.stroke.color
        } else {
            ui.visuals().widgets.noninteractive.bg_stroke.color
        };
        painter.rect_stroke(
            marker_rect,
            2.0,
            egui::Stroke::new(1.5, stroke_color),
            egui::StrokeKind::Middle,
        );
    }

    changed
}

fn sync_surface_profile_editor_state(
    selected_entity: Entity,
    source_info: &MaterialSourceInfo,
    material: &ToonMaterial,
    state: &mut SurfaceProfileEditorState,
) {
    if state.selected_entity == Some(selected_entity) {
        return;
    }

    state.selected_entity = Some(selected_entity);
    state.resources = RenderPartResources::default();
    state.params = SurfaceProfileParams::from_material(material);

    let Some(scene_asset_path) = &source_info.scene_asset_path else {
        return;
    };
    let Ok(profile) = CharacterRenderProfile::load_for_scene_asset_path(scene_asset_path) else {
        return;
    };
    let Some(part) = source_info
        .source_nodes
        .iter()
        .find_map(|source_node| profile.find_part(&source_node.node_name))
    else {
        return;
    };

    state.resources = part.resources.clone();
    if let Ok(params) = ron_value_into::<SurfaceProfileParams>(part.params.clone()) {
        state.params = params;
    }
}

fn save_material_profile(
    path: &Path,
    source_info: &MaterialSourceInfo,
    profile_registry: &ShaderProfileRegistry,
    material: &ToonMaterial,
    surface_profile_editor_state: &SurfaceProfileEditorState,
) -> String {
    if profile_registry.get(&source_info.shader_key).is_none() {
        return format!("未注册的着色器类型: {}", source_info.shader_key);
    }
    let mut surface_params = surface_profile_editor_state.params.clone();
    surface_params.toon = crate::npr::toon::ToonMaterialData::from_material(material);
    let params = match ron_value_from_serializable(&surface_params) {
        Ok(params) => params,
        Err(err) => return err,
    };
    let mut profile = source_info
        .scene_asset_path
        .as_deref()
        .and_then(|scene_asset_path| {
            CharacterRenderProfile::load_for_scene_asset_path(scene_asset_path).ok()
        })
        .or_else(|| CharacterRenderProfile::load_from_path(path).ok())
        .unwrap_or_else(|| CharacterRenderProfile {
            version: PROFILE_VERSION,
            model_binding: ModelBinding {
                scene_asset_path: source_info.scene_asset_path.clone(),
            },
            shared: Default::default(),
            parts: Vec::new(),
        });
    profile.model_binding.scene_asset_path = source_info.scene_asset_path.clone();

    for source_node in &source_info.source_nodes {
        profile.upsert_part(RenderPartBinding {
            binding_key: source_node.node_name.clone(),
            shader_key: source_info.shader_key.clone(),
            resources: normalize_render_part_resources(
                surface_profile_editor_state.resources.clone(),
            ),
            params: params.clone(),
        });
    }

    match profile.save_to_path(path) {
        Ok(()) => format!("已保存 {}", path.display()),
        Err(err) => err,
    }
}

fn load_material_profile(
    path: &Path,
    source_info: &MaterialSourceInfo,
    profile_registry: &ShaderProfileRegistry,
    material: &mut ToonMaterial,
    images: &mut Assets<Image>,
    surface_profile_editor_state: &mut SurfaceProfileEditorState,
) -> Result<(), String> {
    let profile = if let Some(scene_asset_path) = &source_info.scene_asset_path {
        CharacterRenderProfile::load_for_scene_asset_path(scene_asset_path)?
    } else {
        CharacterRenderProfile::load_from_path(path)?
    };
    let Some(part) = source_info
        .source_nodes
        .iter()
        .find_map(|source_node| profile.find_part(&source_node.node_name))
    else {
        return Err("当前材质节点没有保存过绑定参数".to_string());
    };
    let Some(handler) = profile_registry.get(&part.shader_key) else {
        return Err(format!("未注册的着色器类型: {}", part.shader_key));
    };
    handler.apply_to_toon_material(&part.params, material, images)?;
    surface_profile_editor_state.resources = part.resources.clone();
    surface_profile_editor_state.params =
        ron_value_into::<SurfaceProfileParams>(part.params.clone())?;
    Ok(())
}

fn show_surface_profile_editor(ui: &mut egui::Ui, state: &mut SurfaceProfileEditorState) {
    ui.group(|ui| {
        ui.label(egui::RichText::new("Surface Profile").strong());
        ui.add_space(6.0);

        ui.horizontal(|ui| {
            ui.label("分区 Mask");
            ui.selectable_value(
                &mut state.params.region_mask_mode,
                SurfaceRegionMaskMode::None,
                "无",
            );
            ui.selectable_value(
                &mut state.params.region_mask_mode,
                SurfaceRegionMaskMode::ChannelsRgba,
                "RGBA 通道",
            );
        });

        ui.horizontal(|ui| {
            ui.label("法线贴图");
            ui.text_edit_singleline(
                state
                    .resources
                    .normal_texture
                    .get_or_insert_with(String::new),
            );
        });
        ui.horizontal(|ui| {
            ui.label("Region Mask");
            ui.text_edit_singleline(
                state
                    .resources
                    .region_mask_texture
                    .get_or_insert_with(String::new),
            );
        });
        ui.horizontal(|ui| {
            ui.label("光照控制");
            ui.text_edit_singleline(
                state
                    .resources
                    .lighting_control_texture
                    .get_or_insert_with(String::new),
            );
        });

        show_scene_interaction_editor(ui, &mut state.params.scene_interaction);
        show_surface_region_editor(ui, "布料", &mut state.params.regions.fabric);
        show_surface_region_editor(ui, "硬表面", &mut state.params.regions.hard_surface);
        show_surface_region_editor(ui, "金属", &mut state.params.regions.metal);
        show_surface_region_editor(ui, "皮革", &mut state.params.regions.leather);
    });
}

fn normalize_render_part_resources(mut resources: RenderPartResources) -> RenderPartResources {
    normalize_optional_string(&mut resources.base_color_texture);
    normalize_optional_string(&mut resources.normal_texture);
    normalize_optional_string(&mut resources.region_mask_texture);
    normalize_optional_string(&mut resources.lighting_control_texture);
    resources
}

fn normalize_optional_string(value: &mut Option<String>) {
    if value.as_ref().is_some_and(|text| text.trim().is_empty()) {
        *value = None;
    }
}

fn show_scene_interaction_editor(ui: &mut egui::Ui, params: &mut SceneInteractionParams) {
    show_param_group(
        ui,
        "surface_scene_interaction_group",
        "环境交互",
        None,
        false,
        |ui| {
            ui.add(egui::Slider::new(&mut params.direct_light_weight, 0.0..=2.0).text("主光权重"));
            ui.add(egui::Slider::new(&mut params.env_light_weight, 0.0..=2.0).text("环境光权重"));
            ui.add(
                egui::Slider::new(&mut params.shadow_receive_weight, 0.0..=1.0).text("阴影接收"),
            );
            ui.add(egui::Slider::new(&mut params.ambient_floor, 0.0..=1.0).text("暗部底亮"));
            ui.add(
                egui::Slider::new(&mut params.light_color_influence, 0.0..=1.0).text("灯色影响"),
            );
        },
    );
}

fn show_surface_region_editor(
    ui: &mut egui::Ui,
    title: &'static str,
    params: &mut SurfaceRegionParams,
) {
    let id_source = match title {
        "布料" => "surface_fabric_region_group",
        "硬表面" => "surface_hard_surface_region_group",
        "金属" => "surface_metal_region_group",
        "皮革" => "surface_leather_region_group",
        _ => "surface_region_group",
    };
    show_param_group(ui, id_source, title, None, false, |ui| {
        ui.add(egui::Slider::new(&mut params.specular_boost, 0.0..=2.0).text("高光增强"));
        ui.add(egui::Slider::new(&mut params.rim_boost, 0.0..=2.0).text("边缘光增强"));
        ui.add(egui::Slider::new(&mut params.shadow_bias, -0.5..=0.5).text("阴影偏移"));
        ui.add(egui::Slider::new(&mut params.detail_normal_weight, 0.0..=1.0).text("细节法线"));
    });
}

fn normalize_ramp_data(ramp_data: &mut RampData) {
    for stop in &mut ramp_data.stops {
        stop.position = stop.position.clamp(0.0, 1.0);
    }
    ramp_data
        .stops
        .sort_by(|left, right| left.position.total_cmp(&right.position));
    ramp_data.resolution = ramp_data.resolution.max(2);
}

fn render_skewed_slider(
    ui: &mut egui::Ui,
    value: &mut f32,
    label: &str,
    min: f32,
    max: f32,
    exponent: f32,
    reverse: bool,
) {
    let map_to_value = |t: f32| -> f32 {
        let t = t.clamp(0.0, 1.0);
        if reverse {
            max - (max - min) * (1.0 - t).powf(exponent)
        } else {
            min + (max - min) * t.powf(exponent)
        }
    };
    let map_to_slider = |v: f32| -> f32 {
        let normalized = if max > min {
            ((v - min) / (max - min)).clamp(0.0, 1.0)
        } else {
            0.0
        };
        if reverse {
            1.0 - (1.0 - normalized).powf(1.0 / exponent)
        } else {
            normalized.powf(1.0 / exponent)
        }
    };

    ui.add(
        egui::Slider::from_get_set(0.0..=1.0, |slider_value| {
            if let Some(slider_value) = slider_value {
                *value = map_to_value(slider_value as f32);
            }
            map_to_slider(*value) as f64
        })
        .text(label)
        .custom_formatter(|slider_value, _| format!("{:.3}", map_to_value(slider_value as f32))),
    );
}

fn to_egui_color(color: LinearRgba) -> egui::Color32 {
    egui::Color32::from_rgba_unmultiplied(
        (color.red.clamp(0.0, 1.0) * 255.0).round() as u8,
        (color.green.clamp(0.0, 1.0) * 255.0).round() as u8,
        (color.blue.clamp(0.0, 1.0) * 255.0).round() as u8,
        (color.alpha.clamp(0.0, 1.0) * 255.0).round() as u8,
    )
}
