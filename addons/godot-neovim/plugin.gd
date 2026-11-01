@tool
extends EditorPlugin
## Lifecycle manager for godot-neovim.
##
## GDExtension EditorPlugin subclasses are auto-loaded by Godot regardless of the
## addon's enabled/disabled state in Project Settings > Plugins. This script acts
## as the addon entry point so that Godot can properly control the lifecycle through
## the standard plugin enable/disable mechanism.
##
## It finds the GodotNeovimPlugin instance (registered in the "godot_neovim" group)
## and calls set_plugin_active(true/false) to initialize or clean up the plugin.

## GDScript input handler for customizable keybindings
var _input_handler: GodotNeovimInput
## Keymap editor dock panel
var _keymap_editor: GodotNeovimKeymapEditor
var _toolbar_button: MenuButton
var _neovim_active := false


func _enter_tree() -> void:
	var settings := EditorInterface.get_editor_settings()
	var active := bool(settings.get_project_metadata("godot_neovim", "active", true))
	_toolbar_button = MenuButton.new()
	_toolbar_button.name = &"NeovimToggle"
	_toolbar_button.icon = preload("res://addons/godot-neovim/icon.svg")
	_toolbar_button.flat = false
	_toolbar_button.theme_type_variation = &"RunBarButton"
	_toolbar_button.icon_alignment = HORIZONTAL_ALIGNMENT_CENTER
	_toolbar_button.size_flags_horizontal = Control.SIZE_SHRINK_CENTER
	_toolbar_button.size_flags_vertical = Control.SIZE_SHRINK_CENTER
	for color_name in [
		&"icon_normal_color", &"icon_hover_color", &"icon_pressed_color", &"icon_hover_pressed_color"
	]:
		_toolbar_button.add_theme_color_override(color_name, Color.WHITE)
	_toolbar_button.focus_mode = Control.FOCUS_NONE
	var popup := _toolbar_button.get_popup()
	popup.add_item("Enable Neovim", 0)
	popup.set_item_tooltip(0, "Toggle Neovim editing. When off, use Godot's standard editor controls.")
	popup.id_pressed.connect(_on_toolbar_item_pressed)
	popup.about_to_popup.connect(_refresh_toolbar_theme)
	_toolbar_button.theme_changed.connect(_refresh_toolbar_theme)
	add_control_to_container(CONTAINER_TOOLBAR, _toolbar_button)
	_set_neovim_active(active)
	_update_toolbar(active)


func _exit_tree() -> void:
	_set_neovim_active(false)
	if is_instance_valid(_toolbar_button):
		remove_control_from_container(CONTAINER_TOOLBAR, _toolbar_button)
		_toolbar_button.queue_free()
		_toolbar_button = null


func _disable_plugin() -> void:
	_set_neovim_active(false)


func _on_toolbar_item_pressed(_id: int) -> void:
	var active := not _neovim_active
	_set_neovim_active(active)
	EditorInterface.get_editor_settings().set_project_metadata("godot_neovim", "active", active)
	_update_toolbar(active)


func _update_toolbar(active: bool) -> void:
	_toolbar_button.tooltip_text = "Neovim: %s — Open menu" % ("ON" if active else "OFF")
	_refresh_toolbar_theme()


func _refresh_toolbar_theme() -> void:
	if not is_instance_valid(_toolbar_button):
		return
	var theme := EditorInterface.get_editor_theme()
	var icon_width := theme.get_icon(&"Tools", &"EditorIcons").get_width()
	# Keep the full-size SVG within the editor's standard toolbar icon dimensions.
	if _toolbar_button.get_theme_constant(&"icon_max_width") != icon_width:
		_toolbar_button.add_theme_constant_override(&"icon_max_width", icon_width)
	var popup := _toolbar_button.get_popup()
	var toggle_icon := &"checked" if _neovim_active else &"unchecked"
	popup.set_item_icon(0, theme.get_icon(toggle_icon, &"CheckButton"))
	popup.set_item_icon_max_width(0, icon_width)


func _set_neovim_active(active: bool) -> void:
	if _neovim_active == active:
		return
	_neovim_active = active
	if not active:
		_cleanup_input_handler()
	var nodes := get_tree().get_nodes_in_group(&"godot_neovim")
	for node in nodes:
		if node.has_method(&"set_plugin_active"):
			node.set_plugin_active(active)
			if active:
				_setup_input_handler(node)


func _setup_input_handler(plugin_node: Node) -> void:
	_input_handler = GodotNeovimInput.new()
	_input_handler.setup(plugin_node)
	plugin_node.set_input_handler(Callable(_input_handler, &"_on_dispatch"))
	print("[godot-neovim] GDScript input handler registered")

	# Add keymap editor as dockable panel (right side, like Inspector)
	_keymap_editor = GodotNeovimKeymapEditor.new()
	_keymap_editor.name = &"Neovim Keymaps"
	_keymap_editor.setup(plugin_node, _input_handler)
	add_control_to_dock(DOCK_SLOT_RIGHT_BL, _keymap_editor)
	print("[godot-neovim] Keymap editor dock added")


func _cleanup_input_handler() -> void:
	if _keymap_editor:
		remove_control_from_docks(_keymap_editor)
		_keymap_editor.queue_free()
		_keymap_editor = null
	if _input_handler:
		_input_handler.disconnect_handler()
		_input_handler = null
