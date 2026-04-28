pub mod outline;
pub mod post;
pub mod toon;

use bevy::prelude::*;

pub struct NprPlugin;

impl Plugin for NprPlugin {
    fn build(&self, app: &mut App) {
        app.add_plugins((toon::ToonMaterialPlugin, outline::OutlinePlugin));
    }
}
