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
