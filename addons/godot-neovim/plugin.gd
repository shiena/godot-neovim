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


func _enter_tree() -> void:
	_set_neovim_active(true)


func _exit_tree() -> void:
	_set_neovim_active(false)


func _disable_plugin() -> void:
	_set_neovim_active(false)


func _set_neovim_active(active: bool) -> void:
	var nodes := get_tree().get_nodes_in_group(&"godot_neovim")
	for node in nodes:
		if node.has_method(&"set_plugin_active"):
			node.set_plugin_active(active)
			if active:
				_setup_input_handler(node)
			else:
				_cleanup_input_handler()


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
