@tool
extends EditorPlugin
## Key sequence display overlay for godot-neovim
## This is an optional EditorPlugin that displays input keys as an overlay.
## Enable this plugin separately from godot-neovim in Project Settings > Plugins.

const FADE_DURATION := 1.0
const DISPLAY_DURATION := 0.5

var _label: Label
var _current_keys: String = ""
var _previous_keys: String = ""
var _display_timer: float = 0.0
var _fade_timer: float = 0.0
var _is_fading: bool = false
var _neovim_plugin: Node


func _enter_tree() -> void:
	call_deferred("_setup")


func _exit_tree() -> void:
	_disconnect_from_neovim_plugin()
	if _label:
		_label.queue_free()
		_label = null


func _setup() -> void:
	_connect_to_neovim_plugin()
	_create_label()


func _connect_to_neovim_plugin() -> void:
	# Find GodotNeovimPlugin via group
	var plugins := get_tree().get_nodes_in_group("godot_neovim")
	print("[KeySequenceDisplay] Found ", plugins.size(), " nodes in godot_neovim group")

	if plugins.size() > 0:
		_neovim_plugin = plugins[0]
		print("[KeySequenceDisplay] Found plugin: ", _neovim_plugin)
		if _neovim_plugin.has_signal("key_sent"):
			if not _neovim_plugin.is_connected("key_sent", _on_key_sent):
				_neovim_plugin.connect("key_sent", _on_key_sent)
				print("[KeySequenceDisplay] Connected to GodotNeovimPlugin.key_sent")
		else:
			push_warning("[KeySequenceDisplay] key_sent signal not found - update godot-neovim")
	else:
		print("[KeySequenceDisplay] GodotNeovimPlugin not found, retrying in 1s...")
		# Retry after a short delay
		get_tree().create_timer(1.0).timeout.connect(_connect_to_neovim_plugin)


func _disconnect_from_neovim_plugin() -> void:
	if _neovim_plugin and is_instance_valid(_neovim_plugin):
		if _neovim_plugin.is_connected("key_sent", _on_key_sent):
			_neovim_plugin.disconnect("key_sent", _on_key_sent)


func _find_node_by_class(node: Node, class_name_to_find: String) -> Node:
	if node.get_class() == class_name_to_find:
		return node
	for child in node.get_children():
		var result := _find_node_by_class(child, class_name_to_find)
		if result:
			return result
	return null


func _create_label() -> void:
	_label = Label.new()
	_label.name = "KeySequenceLabel"

	# Style - dark background with white text for visibility
	_label.add_theme_font_size_override("font_size", 24)
	_label.add_theme_constant_override("outline_size", 2)
	_label.add_theme_color_override("font_color", Color.WHITE)
	_label.add_theme_color_override("font_outline_color", Color.BLACK)

	var style_box := StyleBoxFlat.new()
	style_box.bg_color = Color(0.1, 0.1, 0.1, 0.85)
	style_box.set_corner_radius_all(4)
	style_box.set_content_margin_all(8.0)
	_label.add_theme_stylebox_override("normal", style_box)

	_label.horizontal_alignment = HORIZONTAL_ALIGNMENT_RIGHT
	_label.vertical_alignment = VERTICAL_ALIGNMENT_BOTTOM
	_label.visible = false

	# Add to script editor
	var script_editor := EditorInterface.get_script_editor()
	if script_editor:
		script_editor.add_child(_label)
		_label.set_anchors_preset(Control.PRESET_BOTTOM_RIGHT)
		_label.offset_left = -300
		_label.offset_top = -80
		_label.offset_right = -20
		_label.offset_bottom = -20
		print("[KeySequenceDisplay] Label created and added to ScriptEditor")
	else:
		print("[KeySequenceDisplay] ERROR: ScriptEditor not found")


func _on_key_sent(key: String) -> void:
	print("[KeySequenceDisplay] Key received: ", key)
	var display_key := _format_key(key)

	if _current_keys.is_empty():
		_current_keys = display_key
	else:
		_current_keys += " " + display_key

	_update_label()
	_reset_timers()

	if _label:
		_label.visible = true
		print("[KeySequenceDisplay] Label visible, text: ", _label.text)


func _format_key(key: String) -> String:
	match key:
		"<Esc>", "<Escape>":
			return "ESC"
		"<CR>", "<Enter>":
			return "RET"
		"<Tab>":
			return "TAB"
		"<BS>", "<Backspace>":
			return "BS"
		"<Space>":
			return "SPC"
		_:
			if key.begins_with("<") and key.ends_with(">"):
				return key.substr(1, key.length() - 2)
			return key


func _update_label() -> void:
	if not _label:
		return

	if _previous_keys.is_empty():
		_label.text = _current_keys
	else:
		_label.text = _previous_keys + "\n" + _current_keys


func _reset_timers() -> void:
	_display_timer = DISPLAY_DURATION
	_fade_timer = FADE_DURATION
	_is_fading = false
	if _label:
		_label.modulate.a = 1.0


func _process(delta: float) -> void:
	if not _label or not _label.visible:
		return

	if _display_timer > 0:
		_display_timer -= delta
		if _display_timer <= 0:
			_is_fading = true
			_previous_keys = _current_keys
			_current_keys = ""

	if _is_fading:
		_fade_timer -= delta
		if _fade_timer <= 0:
			_label.visible = false
			_previous_keys = ""
			_current_keys = ""
		else:
			_label.modulate.a = _fade_timer / FADE_DURATION
