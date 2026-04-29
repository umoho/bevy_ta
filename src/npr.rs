pub mod outline;
pub mod post;
pub mod profile;
pub mod toon;

use bevy::prelude::*;

pub struct NprPlugin;

impl Plugin for NprPlugin {
    fn build(&self, app: &mut App) {
        app.add_plugins((
            profile::CharacterRenderProfilePlugin,
            toon::ToonMaterialPlugin,
            outline::OutlinePlugin,
        ));
    }
}
