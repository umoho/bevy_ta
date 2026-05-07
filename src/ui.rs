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

use crate::debug_gizmos::DebugSceneSelection;
use crate::npr::{
    profile::{
        CHARACTER_FACE_SDF_SHADER_KEY, CHARACTER_HAIR_SHADER_KEY, CHARACTER_SURFACE_SHADER_KEY,
        CharacterMaterialProfile, CharacterRenderProfile, MaterialShadingParams, ModelBinding,
        PROFILE_VERSION, RenderPartBinding, RenderPartResources, SceneInteractionParams,
        ShaderProfileRegistry, character_render_profile_path, ron_value_from_serializable,
        ron_value_into,
    },
    toon::{
        FaceSdfParamsData, RampData, RampDataFile, RampInterpolation, RampStop, ToonMaterial,
        ToonMaterialBindingSource, ToonMaterialData, ToonMaterialTarget, ToonParams,
        ToonParamsData, default_ramp_data, rebuild_ramp_texture,
    },
};
use crate::selection::{
    MaterialPanelEntryRef, MaterialPanelSelectionEntry, MaterialSelectionState,
};

pub struct MaterialEditorPlugin;

impl Plugin for MaterialEditorPlugin {
    fn build(&self, app: &mut App) {
        app.add_plugins((EguiPlugin::default(), MaterialPreviewPlugin::default()))
            .register_material_preview::<ToonMaterial>()
            .init_resource::<MaterialPropertyPreviewState>()
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
                    show_dev_top_bar,
                    bridge_preview_to_egui::<ToonMaterial>,
                    bridge_ramp_texture_to_egui,
                    spawn_property_preview,
                    despawn_property_previews,
                    (show_material_library_window, show_material_property_window).chain(),
                ),
            );
    }
}

#[derive(Resource, Default)]
struct MaterialPropertyPreviewState {
    property_preview_entity: Option<Entity>,
}

#[derive(Resource, Debug, Clone)]
pub(crate) struct DevWindowState {
    pub light_control_open: bool,
    pub gizmos_open: bool,
    pub material_library_open: bool,
    pub material_property_open: bool,
}

impl Default for DevWindowState {
    fn default() -> Self {
        Self {
            light_control_open: true,
            gizmos_open: true,
            material_library_open: true,
            material_property_open: true,
        }
    }
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
    primitive_entity: Entity,
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
struct CharacterSurfaceContainerEditorState {
    selected_entity: Option<Entity>,
    shader_key: String,
    resources: RenderPartResources,
    profile: CharacterMaterialProfile,
}

fn setup_chinese_fonts(mut contexts: EguiContexts) {
    let Ok(ctx) = contexts.ctx_mut() else {
        return;
    };
    if let Err(err) = egui_chinese_font::setup_chinese_fonts(ctx) {
        warn!("不能设置中文字体: {err:?}");
    }
}

fn show_dev_top_bar(mut contexts: EguiContexts, mut windows: ResMut<DevWindowState>) {
    let Ok(ctx) = contexts.ctx_mut() else {
        return;
    };

    egui::TopBottomPanel::top("dev_top_bar").show(ctx, |ui| {
        ui.horizontal(|ui| {
            show_window_menu(ui, ctx, &mut windows);
        });
    });
}

fn show_window_menu(ui: &mut egui::Ui, ctx: &egui::Context, windows: &mut DevWindowState) {
    let popup_id = ui.make_persistent_id("window_menu_popup");
    let is_open = egui::Popup::is_id_open(ctx, popup_id);
    let response = ui.add(
        egui::Button::new("窗口")
            .frame(false)
            .selected(is_open)
            .min_size(egui::vec2(0.0, 24.0)),
    );

    let _ = egui::Popup::menu(&response).id(popup_id).show(|ui| {
        ui.checkbox(&mut windows.light_control_open, "光源控制");
        ui.checkbox(&mut windows.gizmos_open, "Gizmos");
        ui.checkbox(&mut windows.material_library_open, "当前模型材质");
        ui.checkbox(&mut windows.material_property_open, "材质属性");
    });
}

pub(crate) fn clamp_window_pos(
    ctx: &egui::Context,
    desired_pos: egui::Pos2,
    size: egui::Vec2,
) -> egui::Pos2 {
    let rect = ctx.available_rect();
    let margin = 8.0;
    let min_x = rect.left() + margin;
    let min_y = rect.top() + margin;
    let max_x = (rect.right() - size.x - margin).max(min_x);
    let max_y = (rect.bottom() - size.y - margin).max(min_y);
    egui::pos2(
        desired_pos.x.clamp(min_x, max_x),
        desired_pos.y.clamp(min_y, max_y),
    )
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

    for (primitive_entity, material_handle, name, binding_source) in mesh_materials.iter() {
        // 只收集真正业务模型上的 toon 网格，避免材质预览球自己再次触发预览递归生成。
        let is_target_mesh = toon_targets.contains(primitive_entity)
            || parent_query
                .iter_ancestors(primitive_entity)
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
            .unwrap_or_else(|| format!("primitive {}", primitive_entity.index()));
        let scene_asset_path = binding_source.and_then(|source| source.scene_asset_path.clone());
        let shader_key = binding_source
            .map(|source| source.shader_key.clone())
            .unwrap_or_else(|| CHARACTER_SURFACE_SHADER_KEY.to_string());
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
                primitive_entity,
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
            left.node_name.cmp(&right.node_name).then_with(|| {
                left.primitive_entity
                    .index()
                    .cmp(&right.primitive_entity.index())
            })
        });
        let primitive_entities = source_nodes
            .iter()
            .map(|source_node| source_node.primitive_entity)
            .collect::<Vec<_>>();

        let preview_entity = commands
            .spawn((
                MaterialPreviewSession {
                    material: material_handle.clone(),
                    size: UVec2::splat(192),
                    ..Default::default()
                },
                MaterialHandle(material_handle),
                RampTextureHandle(material.ramp_texture.clone()),
                MaterialPanelSelectionEntry {
                    primitive_entities: primitive_entities.clone(),
                },
                MaterialSourceInfo {
                    source_nodes,
                    scene_asset_path,
                    binding_file_path,
                    shader_key,
                },
            ))
            .id();

        for primitive_entity in primitive_entities {
            commands
                .entity(primitive_entity)
                .insert(MaterialPanelEntryRef(preview_entity));
        }
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
    selection: Res<MaterialSelectionState>,
    mut preview_state: ResMut<MaterialPropertyPreviewState>,
    material_handles: Query<&MaterialHandle<ToonMaterial>>,
    mut tracker: Local<Option<AssetId<ToonMaterial>>>,
) {
    let Some(selected_entity) = selection.selected_panel_entity else {
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
    preview_state.property_preview_entity = Some(entity);
    *tracker = Some(asset_id);
}

fn despawn_property_previews(
    mut commands: Commands,
    preview_state: Res<MaterialPropertyPreviewState>,
    property_previews: Query<Entity, With<PropertyPreview>>,
) {
    let Some(selected_entity) = preview_state.property_preview_entity else {
        return;
    };
    for entity in property_previews.iter() {
        if entity != selected_entity {
            commands.entity(entity).despawn();
        }
    }
}

fn show_material_library_window(
    mut contexts: EguiContexts,
    previews: Query<
        (
            Entity,
            &PreviewTextureId,
            &MaterialSourceInfo,
            &MaterialPanelSelectionEntry,
        ),
        Without<PropertyPreview>,
    >,
    mut selection: ResMut<MaterialSelectionState>,
    mut window_state: ResMut<DevWindowState>,
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
                        .primitive_entity
                        .index()
                        .cmp(&right_node.primitive_entity.index())
                }),
            (Some(_), None) => std::cmp::Ordering::Less,
            (None, Some(_)) => std::cmp::Ordering::Greater,
            (None, None) => std::cmp::Ordering::Equal,
        }
    });

    let has_valid_selection = selection
        .selected_panel_entity
        .is_some_and(|selected_entity| {
            entries
                .iter()
                .any(|(entity, _, _, _)| *entity == selected_entity)
        });
    if !has_valid_selection && let Some((entry_entity, _, _, panel_entry)) = entries.first() {
        selection.select_panel_entry(*entry_entity, panel_entry);
    }

    egui::Window::new("当前模型材质")
        .open(&mut window_state.material_library_open)
        .resizable(true)
        .default_pos(clamp_window_pos(
            ctx,
            egui::pos2(16.0, 760.0),
            egui::vec2(1120.0, 180.0),
        ))
        .default_size([1120.0, 180.0])
        .show(ctx, |ui| {
            ui.label(egui::RichText::new("当前模型材质").strong());
            ui.add_space(6.0);
            egui::ScrollArea::horizontal()
                .id_salt("toon_material_library_scroll")
                .show(ui, |ui| {
                    ui.horizontal(|ui| {
                        for (entity, preview, source_info, panel_entry) in entries {
                            show_material_entry(
                                ui,
                                &mut selection,
                                entity,
                                preview.0,
                                source_info,
                                panel_entry,
                            );
                        }
                    });
                });
        });
}

fn show_material_entry(
    ui: &mut egui::Ui,
    selection: &mut MaterialSelectionState,
    entity: Entity,
    preview_texture: egui::TextureId,
    source_info: &MaterialSourceInfo,
    panel_entry: &MaterialPanelSelectionEntry,
) {
    ui.vertical(|ui| {
        let is_selected = selection.selected_panel_entity == Some(entity);
        #[allow(deprecated)]
        let image_button = egui::ImageButton::new(egui::load::SizedTexture::new(
            preview_texture,
            egui::Vec2::splat(84.0),
        ))
        .selected(is_selected);

        if ui.add(image_button).clicked() {
            selection.select_panel_entry(entity, panel_entry);
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

fn show_material_property_window(
    mut contexts: EguiContexts,
    selection: Res<MaterialSelectionState>,
    preview_state: Res<MaterialPropertyPreviewState>,
    mut debug_selection: ResMut<DebugSceneSelection>,
    asset_server: Res<AssetServer>,
    material_handles: Query<&MaterialHandle<ToonMaterial>>,
    mut source_infos: Query<&mut MaterialSourceInfo>,
    property_previews: Query<&PreviewTextureId, With<PropertyPreview>>,
    ramp_textures: Query<&RampTextureId>,
    mut materials: ResMut<Assets<ToonMaterial>>,
    mut images: ResMut<Assets<Image>>,
    profile_registry: Res<ShaderProfileRegistry>,
    mut surface_profile_editor_state: Local<CharacterSurfaceContainerEditorState>,
    mut ramp_editor_state: Local<RampEditorState>,
    mut persistence_state: Local<MaterialPersistenceState>,
    mut window_state: ResMut<DevWindowState>,
) {
    let Ok(ctx) = contexts.ctx_mut() else {
        return;
    };

    let Some(selected_entity) = selection.selected_panel_entity else {
        debug_selection.clear_selected_material();
        egui::Window::new("材质属性")
            .open(&mut window_state.material_property_open)
            .resizable(true)
            .default_pos(clamp_window_pos(
                ctx,
                egui::pos2(1300.0, 76.0),
                egui::vec2(380.0, 720.0),
            ))
            .default_size([380.0, 720.0])
            .show(ctx, |ui| {
                ui.label("先从当前模型材质中选择一个材质。");
            });
        return;
    };
    let Ok(material_handle) = material_handles.get(selected_entity) else {
        debug_selection.clear_selected_material();
        egui::Window::new("材质属性")
            .open(&mut window_state.material_property_open)
            .resizable(true)
            .default_pos(clamp_window_pos(
                ctx,
                egui::pos2(1300.0, 76.0),
                egui::vec2(380.0, 720.0),
            ))
            .default_size([380.0, 720.0])
            .show(ctx, |ui| {
                ui.label("选中的材质已经不存在。");
            });
        return;
    };
    let Ok(mut source_info) = source_infos.get_mut(selected_entity) else {
        debug_selection.clear_selected_material();
        egui::Window::new("材质属性")
            .open(&mut window_state.material_property_open)
            .resizable(true)
            .default_pos(clamp_window_pos(
                ctx,
                egui::pos2(1300.0, 76.0),
                egui::vec2(380.0, 720.0),
            ))
            .default_size([380.0, 720.0])
            .show(ctx, |ui| {
                ui.label("当前材质信息不可用。");
            });
        return;
    };
    let Some(material) = materials.get_mut(material_handle.0.id()) else {
        debug_selection.clear_selected_material();
        egui::Window::new("材质属性")
            .open(&mut window_state.material_property_open)
            .resizable(true)
            .default_pos(clamp_window_pos(
                ctx,
                egui::pos2(1300.0, 76.0),
                egui::vec2(380.0, 720.0),
            ))
            .default_size([380.0, 720.0])
            .show(ctx, |ui| {
                ui.label("当前材质资源不可用。");
            });
        return;
    };
    let selected_primitive_entity = selection
        .selected_primitive_entity
        .filter(|primitive_entity| {
            source_info
                .source_nodes
                .iter()
                .any(|source_node| source_node.primitive_entity == *primitive_entity)
        })
        .or_else(|| {
            source_info
                .source_nodes
                .first()
                .map(|source_node| source_node.primitive_entity)
        });
    if let Some(selected_primitive_entity) = selected_primitive_entity {
        debug_selection.set_selected_material(selected_entity, [selected_primitive_entity]);
    } else {
        debug_selection.clear_selected_material();
    }
    let property_preview_texture = preview_state
        .property_preview_entity
        .and_then(|entity| property_previews.get(entity).ok())
        .map(|preview| preview.0);
    sync_character_surface_profile_editor_state(
        selected_entity,
        &source_info,
        material,
        &mut surface_profile_editor_state,
    );
    egui::Window::new("材质属性")
        .open(&mut window_state.material_property_open)
        .resizable(true)
        .default_pos(clamp_window_pos(
            ctx,
            egui::pos2(1300.0, 76.0),
            egui::vec2(380.0, 720.0),
        ))
        .default_size([380.0, 720.0])
        .show(ctx, |ui| {
            show_selected_material_preview(
                ui,
                property_preview_texture,
                &source_info,
                selected_primitive_entity,
            );
            egui::ScrollArea::vertical()
                .id_salt("toon_material_property_scroll")
                .show(ui, |ui| {
                    show_base_editor(ui, material);
                    ui.add_space(10.0);
                    ui.separator();
                    ui.add_space(10.0);
                    show_model_binding_editor(
                        ui,
                        &asset_server,
                        &mut images,
                        material,
                        &source_info,
                        &profile_registry,
                        &mut surface_profile_editor_state,
                        &mut persistence_state,
                    );
                    ui.add_space(10.0);
                    ui.separator();
                    ui.add_space(10.0);
                    show_character_surface_material_editor(
                        ui,
                        &asset_server,
                        &mut images,
                        material,
                        ramp_textures
                            .get(selected_entity)
                            .ok()
                            .map(|texture| texture.0),
                        &mut surface_profile_editor_state,
                        &mut ramp_editor_state.selected_stop_index,
                    );
                });
        });
    source_info.shader_key = surface_profile_editor_state.shader_key.clone();
}

fn show_selected_material_preview(
    ui: &mut egui::Ui,
    preview_texture: Option<egui::TextureId>,
    source_info: &MaterialSourceInfo,
    selected_primitive_entity: Option<Entity>,
) {
    let selected_source = selected_primitive_entity
        .and_then(|primitive_entity| {
            source_info
                .source_nodes
                .iter()
                .find(|source_node| source_node.primitive_entity == primitive_entity)
        })
        .or_else(|| source_info.source_nodes.first());

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
        ui.heading("当前 primitive 材质");
        if let Some(first_source) = selected_source {
            if source_info.source_nodes.len() > 1 {
                ui.label(
                    egui::RichText::new(format!(
                        "{} 等 {} 个 primitive",
                        first_source.node_name,
                        source_info.source_nodes.len()
                    ))
                    .italics(),
                );
            } else {
                ui.label(egui::RichText::new(&first_source.node_name).italics());
            }
        } else {
            ui.label(egui::RichText::new("未命名 primitive").italics());
        }
        let mut source_nodes = Vec::new();
        if let Some(selected_source) = selected_source {
            source_nodes.push(selected_source);
        }
        source_nodes.extend(source_info.source_nodes.iter().filter(|source_node| {
            Some(source_node.primitive_entity)
                != selected_source.map(|selected_source| selected_source.primitive_entity)
        }));
        for source_node in source_nodes.into_iter().take(4) {
            ui.small(format!(
                "{} ({:?})",
                source_node.node_name, source_node.primitive_entity
            ));
        }
        if source_info.source_nodes.len() > 4 {
            ui.small(format!(
                "还有 {} 个 primitive...",
                source_info.source_nodes.len() - 4
            ));
        }
        if let Some(path) = &source_info.binding_file_path {
            ui.small(format!("模型绑定 {}", path.display()));
        } else {
            ui.small("当前 primitive 没有模型绑定文件");
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
    asset_server: &AssetServer,
    images: &mut Assets<Image>,
    material: &mut ToonMaterial,
    source_info: &MaterialSourceInfo,
    profile_registry: &ShaderProfileRegistry,
    surface_profile_editor_state: &mut CharacterSurfaceContainerEditorState,
    persistence_state: &mut MaterialPersistenceState,
) {
    ui.group(|ui| {
        ui.label(egui::RichText::new("模型绑定").strong());
        show_shader_key_selector(ui, &mut surface_profile_editor_state.shader_key);
        ui.add_space(6.0);
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
                            asset_server,
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
}

fn show_character_surface_material_editor(
    ui: &mut egui::Ui,
    asset_server: &AssetServer,
    images: &mut Assets<Image>,
    material: &mut ToonMaterial,
    ramp_texture: Option<egui::TextureId>,
    state: &mut CharacterSurfaceContainerEditorState,
    selected_stop_index: &mut usize,
) {
    // 当前材质统一使用同一套 toon 骨架；不同部位通过材质实例参数拉开表现。
    state.profile.toon = ToonMaterialData::from_material(material);
    state.profile.face_sdf = FaceSdfParamsData::from_runtime(&material.face_sdf, &state.shader_key);

    ui.group(|ui| {
        ui.label(egui::RichText::new("统一 Toon 材质").strong());
        ui.add_space(6.0);

        show_container_binding_resources(ui, state);
        show_scene_interaction_editor(ui, &mut state.profile.scene_interaction);
        show_container_ramp_editor(
            ui,
            &mut state.profile.toon,
            ramp_texture,
            selected_stop_index,
        );
        show_container_toon_editor(ui, &mut state.profile.toon.params);
        show_material_shading_editor(ui, &mut state.profile.shading);
        if state.shader_key == CHARACTER_FACE_SDF_SHADER_KEY {
            show_face_sdf_editor(ui, &mut state.profile.face_sdf);
        }
    });

    apply_character_surface_editor_state_to_material(asset_server, images, material, state);
}

fn show_container_ramp_editor(
    ui: &mut egui::Ui,
    toon: &mut ToonMaterialData,
    ramp_texture: Option<egui::TextureId>,
    selected_stop_index: &mut usize,
) {
    let mut runtime_ramp = toon.ramp.clone().into_runtime();
    show_ramp_editor_runtime(ui, &mut runtime_ramp, ramp_texture, selected_stop_index);
    toon.ramp = RampDataFile::from_runtime(&runtime_ramp);
}

fn show_ramp_editor_runtime(
    ui: &mut egui::Ui,
    ramp_data: &mut RampData,
    ramp_texture: Option<egui::TextureId>,
    selected_stop_index: &mut usize,
) {
    ui.group(|ui| {
        ui.label(egui::RichText::new("容器级 Ramp").strong());
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

        let mut changed = show_ramp_stop_track(ui, ramp_data, selected_stop_index);
        *selected_stop_index = (*selected_stop_index).min(ramp_data.stops.len().saturating_sub(1));

        if let Some(stop) = ramp_data.stops.get_mut(*selected_stop_index) {
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
                ramp_data.stops.push(RampStop {
                    position: 0.5,
                    color: LinearRgba::WHITE,
                });
                *selected_stop_index = ramp_data.stops.len() - 1;
                changed = true;
            }

            let can_remove = ramp_data.stops.len() > 2;
            if ui.add_enabled(can_remove, egui::Button::new("-")).clicked() {
                ramp_data.stops.remove(*selected_stop_index);
                *selected_stop_index =
                    (*selected_stop_index).min(ramp_data.stops.len().saturating_sub(1));
                changed = true;
            }

            if ui.button("重置默认").clicked() {
                *ramp_data = default_ramp_data();
                *selected_stop_index = 0;
                changed = true;
            }
        });

        ui.horizontal(|ui| {
            ui.label("插值模式");
            changed |= ui
                .selectable_value(
                    &mut ramp_data.interpolation,
                    RampInterpolation::Constant,
                    "常量",
                )
                .changed();
            changed |= ui
                .selectable_value(
                    &mut ramp_data.interpolation,
                    RampInterpolation::Linear,
                    "线性",
                )
                .changed();
        });

        changed |= ui
            .add(egui::Slider::new(&mut ramp_data.resolution, 2..=64).text("分辨率"))
            .changed();

        if changed {
            normalize_ramp_data(ramp_data);
        }
    });
}

fn show_container_binding_resources(
    ui: &mut egui::Ui,
    state: &mut CharacterSurfaceContainerEditorState,
) {
    show_param_group(
        ui,
        "character_surface_container_resources_group",
        "贴图资源",
        None,
        true,
        |ui| {
            ui.horizontal(|ui| {
                ui.label("基础色贴图覆盖");
                ui.text_edit_singleline(
                    state
                        .resources
                        .base_color_texture
                        .get_or_insert_with(String::new),
                );
            });
            if state.shader_key == CHARACTER_FACE_SDF_SHADER_KEY {
                ui.horizontal(|ui| {
                    ui.label("面部阴影贴图");
                    ui.text_edit_singleline(
                        state
                            .resources
                            .face_shadow_texture
                            .get_or_insert_with(String::new),
                    );
                });
            }
        },
    );
}

fn show_container_toon_editor(ui: &mut egui::Ui, params: &mut ToonParamsData) {
    let mut runtime_params = params.clone().into_runtime();
    show_param_group(
        ui,
        "character_surface_shared_toon_group",
        "共享 Toon",
        None,
        true,
        |ui| {
            show_toon_params_editor(ui, &mut runtime_params);
        },
    );
    *params = ToonParamsData::from_runtime(&runtime_params);
}

fn show_material_shading_editor(ui: &mut egui::Ui, shading: &mut MaterialShadingParams) {
    show_param_group(
        ui,
        "character_material_shading_group",
        "附加受光",
        None,
        true,
        |ui| {
            ui.add(egui::Slider::new(&mut shading.specular_scale, 0.0..=2.5).text("高光比例"));
            ui.add(egui::Slider::new(&mut shading.rim_scale, 0.0..=2.5).text("边缘光比例"));
            ui.add(egui::Slider::new(&mut shading.shadow_offset, -0.2..=0.2).text("阴影偏移"));
            ui.add(
                egui::Slider::new(&mut shading.shadow_softness_bias, -0.4..=0.4)
                    .text("阴影柔和偏移"),
            );
            ui.add(egui::Slider::new(&mut shading.shadow_color_mix, 0.0..=1.0).text("阴影染色"));
            ui.add(egui::Slider::new(&mut shading.highlight_boost, 0.0..=1.0).text("亮部增强"));
        },
    );
}

fn show_face_sdf_editor(ui: &mut egui::Ui, params: &mut FaceSdfParamsData) {
    show_param_group(
        ui,
        "character_face_sdf_group",
        "面部 SDF",
        None,
        true,
        |ui| {
            ui.checkbox(&mut params.enabled, "启用面部 SDF");
            ui.checkbox(&mut params.use_texture, "使用面部阴影贴图");
            ui.checkbox(&mut params.uv_mirror_enabled, "按左右光向镜像 U");
            ui.add(egui::Slider::new(&mut params.specular_preserve, 0.0..=1.0).text("保留高光"));
            ui.add(egui::Slider::new(&mut params.shadow_strength, 0.0..=1.0).text("阴影强度"));
            ui.add(egui::Slider::new(&mut params.blend_weight, 0.0..=1.0).text("替代权重"));
            ui.add(egui::Slider::new(&mut params.threshold_bias, -0.5..=0.5).text("阈值偏移"));
            ui.add(egui::Slider::new(&mut params.softness, 0.0..=0.2).text("边界柔和"));
            ui.add(egui::Slider::new(&mut params.horizontal_scale, 0.0..=2.0).text("水平光影响"));
            ui.add(egui::Slider::new(&mut params.horizontal_bias, -1.0..=1.0).text("水平偏移"));
            ui.add(egui::Slider::new(&mut params.vertical_influence, 0.0..=1.0).text("垂直光影响"));
            ui.add(egui::Slider::new(&mut params.backlight_clamp, 0.0..=1.0).text("背光钳制"));
            ui.add(
                egui::Slider::new(&mut params.procedural_terminator_softness, 0.0..=0.5)
                    .text("程序化边界宽度"),
            );
            ui.add(
                egui::Slider::new(&mut params.procedural_vertical_curve, 0.0..=1.0)
                    .text("程序化垂直修正"),
            );
            ui.horizontal(|ui| {
                ui.label("调试模式");
                ui.selectable_value(&mut params.debug_mode, 0, "关闭");
                ui.selectable_value(&mut params.debug_mode, 1, "采样");
                ui.selectable_value(&mut params.debug_mode, 2, "阈值");
                ui.selectable_value(&mut params.debug_mode, 3, "结果");
            });
        },
    );
}

fn show_shader_key_selector(ui: &mut egui::Ui, shader_key: &mut String) {
    ui.horizontal(|ui| {
        ui.label("着色器类型");
        ui.selectable_value(
            shader_key,
            CHARACTER_SURFACE_SHADER_KEY.to_string(),
            CHARACTER_SURFACE_SHADER_KEY,
        );
        ui.selectable_value(
            shader_key,
            CHARACTER_HAIR_SHADER_KEY.to_string(),
            CHARACTER_HAIR_SHADER_KEY,
        );
        ui.selectable_value(
            shader_key,
            CHARACTER_FACE_SDF_SHADER_KEY.to_string(),
            CHARACTER_FACE_SDF_SHADER_KEY,
        );
    });
}

fn apply_character_surface_editor_state_to_material(
    asset_server: &AssetServer,
    images: &mut Assets<Image>,
    material: &mut ToonMaterial,
    state: &mut CharacterSurfaceContainerEditorState,
) {
    state.resources = normalize_render_part_resources(state.resources.clone());
    let use_base_color_texture = material.params.use_base_color_texture;
    state
        .profile
        .toon
        .clone()
        .apply_to_material(material, images);
    material.params.use_base_color_texture = use_base_color_texture;
    material.character_material =
        crate::npr::toon::CharacterMaterialParams::from_profile(&state.profile, &state.shader_key);
    material.face_sdf = state.profile.face_sdf.clone().into_runtime();
    material.apply_render_part_resources(&state.resources, asset_server);
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

fn sync_character_surface_profile_editor_state(
    selected_entity: Entity,
    source_info: &MaterialSourceInfo,
    material: &ToonMaterial,
    state: &mut CharacterSurfaceContainerEditorState,
) {
    if state.selected_entity == Some(selected_entity) {
        return;
    }

    state.selected_entity = Some(selected_entity);
    state.shader_key = source_info.shader_key.clone();
    state.resources = RenderPartResources::default();
    state.profile = CharacterMaterialProfile::from_material(material, &source_info.shader_key);

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
    state.shader_key = part.shader_key.clone();
    if let Ok(profile) = ron_value_into::<CharacterMaterialProfile>(part.params.clone()) {
        state.profile = profile;
    }
}

fn save_material_profile(
    path: &Path,
    source_info: &MaterialSourceInfo,
    profile_registry: &ShaderProfileRegistry,
    material: &ToonMaterial,
    surface_profile_editor_state: &CharacterSurfaceContainerEditorState,
) -> String {
    if profile_registry
        .get(&surface_profile_editor_state.shader_key)
        .is_none()
    {
        return format!(
            "未注册的着色器类型: {}",
            surface_profile_editor_state.shader_key
        );
    }
    let mut material_profile = surface_profile_editor_state.profile.clone();
    material_profile.toon = crate::npr::toon::ToonMaterialData::from_material(material);
    material_profile.face_sdf = FaceSdfParamsData::from_runtime(
        &material.face_sdf,
        &surface_profile_editor_state.shader_key,
    );
    let params = match ron_value_from_serializable(&material_profile) {
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
            shader_key: surface_profile_editor_state.shader_key.clone(),
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
    asset_server: &AssetServer,
    material: &mut ToonMaterial,
    images: &mut Assets<Image>,
    surface_profile_editor_state: &mut CharacterSurfaceContainerEditorState,
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
    handler.apply_to_toon_material(
        &part.params,
        &part.resources,
        material,
        images,
        asset_server,
    )?;
    surface_profile_editor_state.resources = part.resources.clone();
    surface_profile_editor_state.shader_key = part.shader_key.clone();
    surface_profile_editor_state.profile =
        ron_value_into::<CharacterMaterialProfile>(part.params.clone())?;
    Ok(())
}

fn normalize_render_part_resources(mut resources: RenderPartResources) -> RenderPartResources {
    normalize_optional_string(&mut resources.base_color_texture);
    normalize_optional_string(&mut resources.face_shadow_texture);
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
        "character_surface_scene_interaction_group",
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
