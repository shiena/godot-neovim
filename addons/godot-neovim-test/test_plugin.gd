@tool
extends EditorPlugin

var dock: Control
var runner: Node
var neovim_plugin: Node

var _log_label: RichTextLabel
var _progress_label: Label
var _run_btn: Button
var _run_all_btn: Button
var _run_filtered_btn: Button
var _test_id_spin: SpinBox
var _category_option: OptionButton
var _name_filter_edit: LineEdit


func _enter_tree() -> void:
	dock = _create_dock()
	add_control_to_dock(DOCK_SLOT_RIGHT_BL, dock)
	_find_neovim_plugin()


func _exit_tree() -> void:
	if runner:
		runner.queue_free()
		runner = null
	remove_control_from_docks(dock)
	dock.queue_free()


func _find_neovim_plugin() -> void:
	var nodes := get_tree().get_nodes_in_group("godot_neovim")
	if nodes.size() > 0:
		neovim_plugin = nodes[0]


func _create_dock() -> Control:
	var vbox := VBoxContainer.new()
	vbox.name = "GodotNeovimTest"
	vbox.custom_minimum_size = Vector2(220, 0)

	# Title
	var title := Label.new()
	title.text = "godot-neovim Test"
	title.add_theme_font_size_override("font_size", 16)
	vbox.add_child(title)

	vbox.add_child(HSeparator.new())

	# --- Run by ID ---
	var by_id_label := Label.new()
	by_id_label.text = "Run by ID:"
	vbox.add_child(by_id_label)

	var id_row := HBoxContainer.new()
	_test_id_spin = SpinBox.new()
	_test_id_spin.min_value = 1
	_test_id_spin.max_value = 9999
	_test_id_spin.value = 1
	_test_id_spin.size_flags_horizontal = Control.SIZE_EXPAND_FILL
	id_row.add_child(_test_id_spin)

	_run_btn = Button.new()
	_run_btn.text = "Run"
	_run_btn.pressed.connect(_on_run_pressed)
	id_row.add_child(_run_btn)
	vbox.add_child(id_row)

	vbox.add_child(HSeparator.new())

	# --- Filters ---
	var filter_label := Label.new()
	filter_label.text = "Filter:"
	vbox.add_child(filter_label)

	var cat_row := HBoxContainer.new()
	var cat_text := Label.new()
	cat_text.text = "Category:"
	cat_row.add_child(cat_text)

	_category_option = OptionButton.new()
	_category_option.size_flags_horizontal = Control.SIZE_EXPAND_FILL
	cat_row.add_child(_category_option)
	vbox.add_child(cat_row)

	var name_row := HBoxContainer.new()
	var name_text := Label.new()
	name_text.text = "Name:"
	name_row.add_child(name_text)
	_name_filter_edit = LineEdit.new()
	_name_filter_edit.placeholder_text = "substring..."
	_name_filter_edit.size_flags_horizontal = Control.SIZE_EXPAND_FILL
	name_row.add_child(_name_filter_edit)
	vbox.add_child(name_row)

	_run_filtered_btn = Button.new()
	_run_filtered_btn.text = "Run Filtered"
	_run_filtered_btn.pressed.connect(_on_run_filtered_pressed)
	vbox.add_child(_run_filtered_btn)

	vbox.add_child(HSeparator.new())

	# --- Run All ---
	_run_all_btn = Button.new()
	_run_all_btn.text = "Run All"
	_run_all_btn.pressed.connect(_on_run_all_pressed)
	vbox.add_child(_run_all_btn)

	vbox.add_child(HSeparator.new())

	# --- Progress + log ---
	_progress_label = Label.new()
	_progress_label.text = "Ready"
	vbox.add_child(_progress_label)

	_log_label = RichTextLabel.new()
	_log_label.bbcode_enabled = true
	_log_label.scroll_following = true
	_log_label.size_flags_vertical = Control.SIZE_EXPAND_FILL
	_log_label.custom_minimum_size = Vector2(0, 220)
	vbox.add_child(_log_label)

	var clear_btn := Button.new()
	clear_btn.text = "Clear Log"
	clear_btn.pressed.connect(func(): _log_label.clear())
	vbox.add_child(clear_btn)

	return vbox


# ---- Button handlers ----

func _on_run_pressed() -> void:
	if not _prepare_runner():
		return
	runner.run_test(int(_test_id_spin.value))


func _on_run_all_pressed() -> void:
	if not _prepare_runner():
		return
	runner.run_tests()


func _on_run_filtered_pressed() -> void:
	if not _prepare_runner():
		return
	var cat: String = ""
	if _category_option.item_count > 0:
		cat = _category_option.get_item_text(_category_option.selected)
	runner.run_filtered(cat, _name_filter_edit.text)


# ---- Runner lifecycle ----

func _prepare_runner() -> bool:
	if not neovim_plugin:
		_find_neovim_plugin()
	if not neovim_plugin:
		append_log("[color=red]ERROR: godot-neovim plugin not found[/color]")
		return false
	if not runner:
		runner = preload("test_runner.gd").new()
		add_child(runner)
		runner.setup(neovim_plugin, self)
		_populate_categories()
	return true


func _populate_categories() -> void:
	_category_option.clear()
	_category_option.add_item("all")
	if not runner:
		return
	var cats: PackedStringArray = runner.list_categories()
	for c in cats:
		_category_option.add_item(c)


# ---- External API ----

func append_log(msg: String) -> void:
	_log_label.append_text(msg + "\n")


func update_progress(current: int, total: int) -> void:
	_progress_label.text = "%d / %d" % [current, total]


func set_running(is_running: bool) -> void:
	_run_btn.disabled = is_running
	_run_all_btn.disabled = is_running
	_run_filtered_btn.disabled = is_running
	if not is_running:
		_progress_label.text = "Done"
