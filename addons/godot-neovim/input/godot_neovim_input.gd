## GDScript input handler for GodotNeovim.
##
## Dispatches resolved key sequences to plugin action methods based on keymaps.
## Register this handler with the plugin to enable GDScript-based key dispatch,
## which allows customizing key bindings without recompiling the GDExtension.
##
## Usage (automatic):
##   The plugin automatically instantiates and sets up this handler on startup.
##
## Usage (manual):
##   var input_handler = GodotNeovimInput.new()
##   input_handler.setup(plugin)
class_name GodotNeovimInput
extends RefCounted

## Reference to the GodotNeovimPlugin instance.
var plugin: Object

## Mode-specific keymaps: { mode_string: { key_string: action_or_callable } }
## Modes: "n" (normal), "v"/"V"/"\x16" (visual variants)
var keymaps: Dictionary = {}

## Visual mode keys (all visual variants share the same keymap)
const VISUAL_MODES: PackedStringArray = ["v", "V", "\u0016"]


func _init() -> void:
	pass


## Initialize the input handler with a plugin reference.
## Call this after instantiation (ClassDB.instantiate cannot pass constructor args).
func setup(p_plugin: Object) -> void:
	plugin = p_plugin

	# Load default keymaps
	keymaps["n"] = GodotNeovimDefaultKeymaps.get_normal_keymap()
	var visual_map := GodotNeovimDefaultKeymaps.get_visual_keymap()
	for mode in VISUAL_MODES:
		keymaps[mode] = visual_map.duplicate()

	# Load custom keymaps from EditorSettings (godot_neovim/custom_keymaps)
	_load_custom_keymaps_from_settings()

	# Note: input handler registration is done by plugin.gd after setup() returns,
	# to avoid re-entrant borrow issues with &mut self.



## Dispatch callback, invoked via call_deferred from Rust input() to avoid re-entrant borrow.
## Rust handles mouse, key filtering, mode routing, and key resolution in input().
## This method only does keymap lookup and action dispatch.
## [param resolved_key] Key string in Neovim notation (e.g., "gd", "<C-f>", "ZZ")
## [param mode] Current vim mode ("n", "v", "V", etc.)
func _on_dispatch(resolved_key: String, mode: String) -> void:
	# Look up action in keymap
	var keymap: Dictionary = _get_keymap_for_mode(mode)
	var action = keymap.get(resolved_key, "")

	# All plugin calls use call_deferred to avoid re-entrant &mut self borrow.
	# _dispatch_key_to_gdscript holds &mut self while calling this method,
	# so direct plugin calls would cause a borrow conflict in godot-rs.
	if action is Callable:
		action.call_deferred()
	elif action is String and action != "":
		plugin.call_deferred(action)
	else:
		# Key not in keymap - send directly to Neovim
		plugin.call_deferred(&"action_send_keys", resolved_key)


## Get the keymap for a given mode.
func _get_keymap_for_mode(mode: String) -> Dictionary:
	if keymaps.has(mode):
		return keymaps[mode]
	# Fallback to normal mode keymap
	return keymaps.get("n", {})



## Apply keymap changes from the keymap editor.
## Called by GodotNeovimKeymapEditor when changes are saved.
## [param p_changes] Dictionary with format: { "n": { "set": {}, "removed": [] }, "v": ... }
func apply_keymap_changes(p_changes: Dictionary) -> void:
	# Rebuild keymaps from defaults
	keymaps["n"] = GodotNeovimDefaultKeymaps.get_normal_keymap()
	var visual_map := GodotNeovimDefaultKeymaps.get_visual_keymap()
	for mode in VISUAL_MODES:
		keymaps[mode] = visual_map.duplicate()

	# Apply changes (set overrides, remove bindings)
	for mode_key in p_changes:
		if not keymaps.has(mode_key):
			# Apply to all visual mode variants if mode_key is "v"
			if mode_key == "v":
				for vm in VISUAL_MODES:
					_apply_mode_changes(vm, p_changes[mode_key])
			continue
		_apply_mode_changes(mode_key, p_changes[mode_key])



## Apply changes for a single mode.
func _apply_mode_changes(mode: String, mode_changes: Dictionary) -> void:
	if not keymaps.has(mode):
		return
	var removed: Array = mode_changes.get("removed", [])
	for key in removed:
		keymaps[mode].erase(key)
	var set_dict: Dictionary = mode_changes.get("set", {})
	for key in set_dict:
		keymaps[mode][key] = set_dict[key]


## Load custom keymaps from EditorSettings (godot_neovim/custom_keymaps).
## Same format as keymap_editor.gd saves.
func _load_custom_keymaps_from_settings() -> void:
	var settings := EditorInterface.get_editor_settings()
	if not settings:
		return
	var settings_key := "godot_neovim/custom_keymaps"
	if not settings.has_setting(settings_key):
		return
	var json_str: String = settings.get_setting(settings_key)
	if json_str.is_empty():
		return
	var json := JSON.new()
	if json.parse(json_str) != OK:
		push_warning("[godot-neovim] Failed to parse custom_keymaps from EditorSettings")
		return
	var data: Dictionary = json.data
	for mode_key in data:
		if mode_key == "v":
			for vm in VISUAL_MODES:
				_apply_mode_changes(vm, data[mode_key])
		elif keymaps.has(mode_key):
			_apply_mode_changes(mode_key, data[mode_key])


## Disconnect this input handler from the plugin.
func disconnect_handler() -> void:
	if plugin:
		plugin.clear_input_handler()
