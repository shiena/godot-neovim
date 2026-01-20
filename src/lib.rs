mod lsp;
mod neovim;
mod plugin;
mod settings;
mod sync;

use godot::prelude::*;

/// Print to Godot console only when --verbose flag is used
#[macro_export]
macro_rules! verbose_print {
    ($($arg:tt)*) => {
        godot::global::print_verbose(&[
            godot::builtin::Variant::from(format!($($arg)*))
        ]);
    };
}

struct GodotNeovimExtension;

#[gdextension]
unsafe impl ExtensionLibrary for GodotNeovimExtension {}
