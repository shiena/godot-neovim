mod neovim;
mod plugin;
mod settings;

use godot::prelude::*;

struct GodotNeovimExtension;

#[gdextension]
unsafe impl ExtensionLibrary for GodotNeovimExtension {}
