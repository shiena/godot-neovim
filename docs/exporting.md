# Export a project

[Back to README](../README.md)

> [!IMPORTANT]
> This is an **editor-only** plugin. Its `.gdextension` only declares `editor`-tagged libraries, so it ships no binaries for export platforms (iOS, Android, Web, etc.).
>
> If you export a project **without excluding this addon**, the `.gdextension` file is still packaged and stays registered in `.godot/extension_list.cfg`. The exported game then tries to load it at startup and fails. For example, a Web build opened in a browser logs:
>
> ```
> ERROR: No GDExtension library found for current OS and architecture (web.wasm32)
>        in configuration file: res://addons/godot-neovim/godot-neovim.gdextension
> ERROR: GDExtension dynamic library not found: 'res://addons/godot-neovim/godot-neovim.gdextension'.
> ERROR: Error loading extension: 'res://addons/godot-neovim/godot-neovim.gdextension'.
> ```
>
> To prevent this, exclude the addon from every export preset:
>
> 1. Open **Project > Export…**
> 2. Select your export preset and switch to the **Resources** tab
> 3. Add `addons/godot-neovim/*` to the **Filters to exclude files/folders from project** field (`exclude_filter` in `export_presets.cfg`)
>
> On **Godot 4.4+**, excluding via this filter also drops the entry from `extension_list.cfg` automatically ([godotengine/godot#97216](https://github.com/godotengine/godot/pull/97216)), so the exported game no longer attempts to load the extension. Excluding with an `EditorExportPlugin.skip()` does **not** clean up `extension_list.cfg` ([godotengine/godot#99698](https://github.com/godotengine/godot/issues/99698)), which is why the resource filter above is the recommended approach.
