use bevy::prelude::*;

#[derive(Resource, Default, Debug, Clone)]
pub struct MaterialSelectionState {
    pub selected_panel_entity: Option<Entity>,
    pub selected_primitive_entity: Option<Entity>,
}

impl MaterialSelectionState {
    pub fn select_panel_entry(
        &mut self,
        panel_entity: Entity,
        panel_entry: &MaterialPanelSelectionEntry,
    ) {
        self.selected_panel_entity = Some(panel_entity);
        if self
            .selected_primitive_entity
            .is_none_or(|entity| !panel_entry.primitive_entities.contains(&entity))
        {
            self.selected_primitive_entity = panel_entry.primary_primitive();
        }
    }

    pub fn select_primitive(&mut self, panel_entity: Entity, primitive_entity: Entity) {
        self.selected_panel_entity = Some(panel_entity);
        self.selected_primitive_entity = Some(primitive_entity);
    }

    pub fn clear_selected_primitive(&mut self) {
        self.selected_primitive_entity = None;
    }
}

#[derive(Component, Debug, Clone)]
pub struct MaterialPanelSelectionEntry {
    pub primitive_entities: Vec<Entity>,
}

impl MaterialPanelSelectionEntry {
    pub fn primary_primitive(&self) -> Option<Entity> {
        self.primitive_entities.first().copied()
    }
}

#[derive(Component, Debug, Clone, Copy)]
pub struct MaterialPanelEntryRef(pub Entity);
