## Keymap editor dock panel for GodotNeovim.
##
## Provides a GUI to view and customize key bindings.
## Changes are persisted to EditorSettings and applied in real-time.
class_name GodotNeovimKeymapEditor
extends Control

const SETTINGS_KEY := "godot_neovim/custom_keymaps"

var plugin: Object
var input_handler: Object  # GodotNeovimInput instance

var mode_button: OptionButton
var reset_button: Button
var import_button: Button
var tree: Tree
var status_label: Label

## Current mode being displayed ("n" or "v")
var current_display_mode: String = "n"

## Tracked changes: { "n": { "set": {}, "removed": [] }, "v": { "set": {}, "removed": [] } }
var changes: Dictionary = {}


## Initialize the editor with plugin and input handler references.
## Called before _ready() (before entering the scene tree).
func setup(p_plugin: Object, p_input_handler: Object) -> void:
	plugin = p_plugin
	input_handler = p_input_handler


func _ready() -> void:
	_init_changes()
	_build_ui()
	_load_from_editor_settings()
	_refresh_tree()


func _init_changes() -> void:
	changes = {
		"n": {"set": {}, "removed": []},
		"v": {"set": {}, "removed": []},
	}


func _build_ui() -> void:
	var vbox := VBoxContainer.new()
	vbox.set_anchors_preset(Control.PRESET_FULL_RECT)
	add_child(vbox)

	# Toolbar
	var toolbar := HBoxContainer.new()
	vbox.add_child(toolbar)

	mode_button = OptionButton.new()
	mode_button.add_item("Normal", 0)
	mode_button.add_item("Visual", 1)
	mode_button.item_selected.connect(_on_mode_selected)
	toolbar.add_child(mode_button)

	reset_button = Button.new()
	reset_button.text = "Reset"
	reset_button.pressed.connect(_on_reset_pressed)
	toolbar.add_child(reset_button)

	import_button = Button.new()
	import_button.text = "Import from Neovim"
	import_button.pressed.connect(_on_import_pressed)
	toolbar.add_child(import_button)

	# Tree
	tree = Tree.new()
	tree.columns = 3
	tree.set_column_title(0, "Key")
	tree.set_column_title(1, "Action")
	tree.set_column_title(2, "")
	tree.set_column_titles_visible(true)
	tree.set_column_expand(0, true)
	tree.set_column_expand(1, true)
	tree.set_column_expand(2, false)
	tree.set_column_custom_minimum_width(2, 30)
	tree.size_flags_vertical = Control.SIZE_EXPAND_FILL
	tree.item_edited.connect(_on_tree_item_edited)
	vbox.add_child(tree)

	# Key input for tree
	tree.gui_input.connect(_on_tree_gui_input)

	# Status label
	status_label = Label.new()
	status_label.text = ""
	vbox.add_child(status_label)


func _on_mode_selected(index: int) -> void:
	current_display_mode = "n" if index == 0 else "v"
	_refresh_tree()


func _on_reset_pressed() -> void:
	changes[current_display_mode] = {"set": {}, "removed": []}
	_save_and_apply()
	_refresh_tree()
	status_label.text = "Reset %s mode to defaults" % ("Normal" if current_display_mode == "n" else "Visual")


func _on_import_pressed() -> void:
	if not plugin or not plugin.has_method(&"get_neovim_keymaps"):
		status_label.text = "Plugin not available"
		return

	var mode := current_display_mode
	var nvim_maps: Dictionary = plugin.get_neovim_keymaps(mode)
	if nvim_maps.is_empty():
		status_label.text = "No Neovim keymaps found (Neovim not connected or --clean mode)"
		return

	# Show import dialog with Neovim keymaps
	var dialog := AcceptDialog.new()
	dialog.title = "Import from Neovim"
	dialog.min_size = Vector2i(500, 400)

	var vbox := VBoxContainer.new()
	dialog.add_child(vbox)

	var label := Label.new()
	label.text = "Select keymaps to import (%s mode, %d found):" % [
		"Normal" if mode == "n" else "Visual", nvim_maps.size()
	]
	vbox.add_child(label)

	var import_tree := Tree.new()
	import_tree.columns = 3
	import_tree.set_column_title(0, "")
	import_tree.set_column_title(1, "Key")
	import_tree.set_column_title(2, "Mapping")
	import_tree.set_column_titles_visible(true)
	import_tree.set_column_expand(0, false)
	import_tree.set_column_custom_minimum_width(0, 30)
	import_tree.set_column_expand(1, true)
	import_tree.set_column_expand(2, true)
	import_tree.size_flags_vertical = Control.SIZE_EXPAND_FILL
	vbox.add_child(import_tree)

	var root := import_tree.create_item()
	var sorted_keys: Array = nvim_maps.keys()
	sorted_keys.sort()
	for key in sorted_keys:
		var item := import_tree.create_item(root)
		item.set_cell_mode(0, TreeItem.CELL_MODE_CHECK)
		item.set_checked(0, false)
		item.set_editable(0, true)
		item.set_text(1, key)
		item.set_text(2, nvim_maps[key])

	dialog.confirmed.connect(func():
		var mode_changes: Dictionary = changes[current_display_mode]
		var set_dict: Dictionary = mode_changes["set"]
		var item := root.get_first_child()
		var count := 0
		while item:
			if item.is_checked(0):
				var lhs: String = item.get_text(1)
				var rhs: String = item.get_text(2)
				# Import as action_send_keys mapping (sends rhs to Neovim)
				set_dict[lhs] = "action_send_keys"
				count += 1
			item = item.get_next()
		if count > 0:
			_save_and_apply()
			_refresh_tree()
			status_label.text = "Imported %d keymaps from Neovim" % count
		dialog.queue_free()
	)
	dialog.canceled.connect(func():
		dialog.queue_free()
	)

	add_child(dialog)
	dialog.popup_centered()


func _on_tree_gui_input(event: InputEvent) -> void:
	if event is InputEventKey and event.is_pressed() and event.keycode == KEY_DELETE:
		var selected := tree.get_selected()
		if selected == null or selected == tree.get_root():
			return

		var key_str: String = selected.get_text(0)
		var is_custom: bool = selected.get_text(2) != ""

		var mode_changes: Dictionary = changes[current_display_mode]
		var set_dict: Dictionary = mode_changes["set"]
		var removed_arr: Array = mode_changes["removed"]

		if set_dict.has(key_str):
			# Remove custom override
			set_dict.erase(key_str)
		else:
			# Remove default binding
			if key_str not in removed_arr:
				removed_arr.append(key_str)

		_save_and_apply()
		_refresh_tree()
		status_label.text = "Removed binding: %s" % key_str


func _on_tree_item_edited() -> void:
	var item := tree.get_edited()
	if item == null:
		return

	var column := tree.get_edited_column()
	var old_key: String = item.get_metadata(0) if item.get_metadata(0) else ""
	var new_text: String = item.get_text(column)

	if new_text.is_empty():
		_refresh_tree()
		return

	var mode_changes: Dictionary = changes[current_display_mode]
	var set_dict: Dictionary = mode_changes["set"]
	var removed_arr: Array = mode_changes["removed"]

	if column != 0:
		return

	# Validate key format
	var validation_error := _validate_key(new_text)
	if not validation_error.is_empty():
		status_label.text = validation_error
		_refresh_tree()
		return

	# Check for duplicate key (skip if unchanged)
	if new_text != old_key and _is_key_duplicate(new_text, old_key):
		status_label.text = "Key '%s' is already mapped in %s mode" % [
			new_text, "Normal" if current_display_mode == "n" else "Visual"
		]
		_refresh_tree()
		return

	# Key column edited - look up action from current data
	var default_map: Dictionary
	if current_display_mode == "n":
		default_map = GodotNeovimDefaultKeymaps.get_normal_keymap()
	else:
		default_map = GodotNeovimDefaultKeymaps.get_visual_keymap()

	var action: String = set_dict.get(old_key, default_map.get(old_key, "action_send_keys"))

	# If old key was a custom override, remove it
	if set_dict.has(old_key):
		set_dict.erase(old_key)
	elif old_key != "" and old_key not in removed_arr:
		# Old key was a default, mark as removed
		removed_arr.append(old_key)
	# Add new key -> action mapping
	set_dict[new_text] = action

	_save_and_apply()
	_refresh_tree()
	status_label.text = "Key changed: '%s' -> '%s'" % [old_key, new_text]


func _refresh_tree() -> void:
	tree.clear()
	var root := tree.create_item()

	# Build effective keymap: start with defaults, apply changes
	var default_map: Dictionary
	if current_display_mode == "n":
		default_map = GodotNeovimDefaultKeymaps.get_normal_keymap()
	else:
		default_map = GodotNeovimDefaultKeymaps.get_visual_keymap()

	var mode_changes: Dictionary = changes[current_display_mode]
	var set_dict: Dictionary = mode_changes["set"]
	var removed_arr: Array = mode_changes["removed"]

	# Collect all keys (defaults + custom overrides)
	var all_keys: Dictionary = {}

	# Add default keys (not removed)
	for key in default_map:
		if key not in removed_arr:
			var action: String = set_dict[key] if set_dict.has(key) else default_map[key]
			var is_custom: bool = set_dict.has(key)
			all_keys[key] = {"action": action, "custom": is_custom}

	# Add custom-only keys (not in defaults)
	for key in set_dict:
		if not all_keys.has(key):
			all_keys[key] = {"action": set_dict[key], "custom": true}

	# Sort by key for display
	var sorted_keys: Array = all_keys.keys()
	sorted_keys.sort()

	for key in sorted_keys:
		var info: Dictionary = all_keys[key]
		var item := tree.create_item(root)
		item.set_text(0, key)
		item.set_editable(0, true)
		item.set_metadata(0, key)  # Store original key for edit tracking
		item.set_text(1, _display_action_name(info["action"]))
		item.set_editable(1, false)
		if info["custom"]:
			item.set_text(2, "*")
		item.set_selectable(2, false)


## Convert internal action name to display name.
## e.g. "action_search_word_backward" -> "search word backward"
static func _display_action_name(action: String) -> String:
	var name := action
	if name.begins_with("action_"):
		name = name.substr(7)  # len("action_") == 7
	return name.replace("_", " ")


## Valid special key names in Neovim notation (inside angle brackets).
const VALID_SPECIAL_KEYS: PackedStringArray = [
	"Esc", "CR", "Tab", "BS", "Del", "Space",
	"Up", "Down", "Left", "Right",
	"Home", "End", "PageUp", "PageDown",
	"F1", "F2", "F3", "F4", "F5", "F6",
	"F7", "F8", "F9", "F10", "F11", "F12",
	"Bar", "Bslash", "LT",
]

## Valid modifier prefixes.
const VALID_MODIFIERS: PackedStringArray = ["C", "A", "S", "C-A", "C-S", "A-S", "C-A-S"]

## Printable characters valid as single-key bindings.
const VALID_SINGLE_CHARS := (
	"abcdefghijklmnopqrstuvwxyz"
	+ "ABCDEFGHIJKLMNOPQRSTUVWXYZ"
	+ "0123456789"
	+ "!@#$%^&*()-_=+[]{};:'\",.<>/?\\|`~ "
)


## Validate a key string in Neovim notation.
## Returns an empty string if valid, or an error message if invalid.
static func _validate_key(key: String) -> String:
	if key.is_empty():
		return "Key cannot be empty"

	# Special key with angle brackets: <C-a>, <CR>, <C-A-Del>, etc.
	if key.begins_with("<") and key.ends_with(">"):
		var inner := key.substr(1, key.length() - 2)
		if inner.is_empty():
			return "Invalid key: empty angle brackets"

		# Check for modifier prefix: <C-x>, <A-x>, <C-A-x>, etc.
		var last_dash := inner.rfind("-")
		if last_dash > 0:
			var modifier_part := inner.substr(0, last_dash)
			var key_part := inner.substr(last_dash + 1)
			if modifier_part not in VALID_MODIFIERS:
				return "Invalid modifier '%s' in '%s'. Valid: %s" % [
					modifier_part, key, ", ".join(VALID_MODIFIERS)
				]
			if key_part.is_empty():
				return "Missing key after modifier in '%s'" % key
			# Key part after modifier: single char or special key name
			if key_part.length() == 1:
				return ""  # e.g., <C-a>, <A-x>
			if key_part in VALID_SPECIAL_KEYS:
				return ""  # e.g., <C-Del>, <C-A-Tab>
			return "Invalid key '%s' in '%s'. Expected single char or special key" % [key_part, key]

		# No modifier: <CR>, <Esc>, <F1>, etc.
		if inner in VALID_SPECIAL_KEYS:
			return ""
		return "Invalid special key '<%s>'. Valid: %s" % [inner, ", ".join(VALID_SPECIAL_KEYS)]

	# Multi-char sequence (no angle brackets): gd, zo, ZZ, [[, etc.
	if key.length() >= 2:
		for i in key.length():
			var ch := key[i]
			if VALID_SINGLE_CHARS.find(ch) == -1:
				return "Invalid character '%s' in key sequence '%s'" % [ch, key]
		return ""

	# Single character
	if key.length() == 1:
		if VALID_SINGLE_CHARS.find(key) != -1:
			return ""
		return "Invalid key character '%s'" % key

	return "Invalid key format: '%s'" % key


## Check if a key is already mapped in the current mode's effective keymap.
## [param new_key] The key to check for duplicates.
## [param old_key] The key being replaced (excluded from duplicate check).
func _is_key_duplicate(new_key: String, old_key: String) -> bool:
	var default_map: Dictionary
	if current_display_mode == "n":
		default_map = GodotNeovimDefaultKeymaps.get_normal_keymap()
	else:
		default_map = GodotNeovimDefaultKeymaps.get_visual_keymap()

	var mode_changes: Dictionary = changes[current_display_mode]
	var set_dict: Dictionary = mode_changes["set"]
	var removed_arr: Array = mode_changes["removed"]

	# Collect all currently active keys (excluding old_key)
	# Default keys (not removed)
	for key in default_map:
		if key == old_key:
			continue
		if key not in removed_arr:
			if key == new_key:
				return true

	# Custom keys
	for key in set_dict:
		if key == old_key:
			continue
		if key == new_key:
			return true

	return false


func _save_and_apply() -> void:
	_save_to_editor_settings()
	if input_handler and input_handler.has_method("apply_keymap_changes"):
		input_handler.apply_keymap_changes(changes)


func _save_to_editor_settings() -> void:
	var json_str := JSON.stringify(changes)
	var settings := EditorInterface.get_editor_settings()
	settings.set_setting(SETTINGS_KEY, json_str)


func _load_from_editor_settings() -> void:
	var settings := EditorInterface.get_editor_settings()
	if not settings.has_setting(SETTINGS_KEY):
		return

	var json_str: String = settings.get_setting(SETTINGS_KEY)
	if json_str.is_empty():
		return

	var json := JSON.new()
	if json.parse(json_str) != OK:
		push_warning("[godot-neovim] Failed to parse custom_keymaps from EditorSettings")
		return

	var data: Dictionary = json.data
	# Merge loaded data into changes
	for mode_key in data:
		if changes.has(mode_key):
			var loaded: Dictionary = data[mode_key]
			changes[mode_key]["set"] = loaded.get("set", {})
			var removed = loaded.get("removed", [])
			if removed is Array:
				changes[mode_key]["removed"] = removed
			else:
				changes[mode_key]["removed"] = []
