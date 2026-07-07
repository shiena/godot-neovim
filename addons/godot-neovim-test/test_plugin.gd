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
	if OS.get_environment("GODOT_NEOVIM_TEST_AUTORUN") != "":
		_autorun.call_deferred()


## Headless-ish CI entry point: when GODOT_NEOVIM_TEST_AUTORUN is set, open the
## sandbox script (or GODOT_NEOVIM_TEST_SCRIPT), run the whole suite
## (or only GODOT_NEOVIM_TEST_CATEGORY), then quit the editor. Results land in
## user://godot_neovim_test_results.json as usual.
func _autorun() -> void:
	# Give the editor and the neovim plugin time to finish starting up.
	await get_tree().create_timer(3.0).timeout
	# Deterministic indent behavior for indent-sensitive tests (Ctrl+T/Ctrl+D
	# expect 4-space steps): force Spaces/4 regardless of local settings.
	# convert_indent_on_save MUST be off: the editor saves every open script
	# on quit, and with conversion enabled that rewrites their indentation
	# on disk (real project files!) as collateral damage.
	var es := EditorInterface.get_editor_settings()
	es.set_setting("text_editor/behavior/indent/type", 1)  # 1 = Spaces
	es.set_setting("text_editor/behavior/indent/size", 4)
	es.set_setting("text_editor/behavior/files/convert_indent_on_save", false)
	var script_path := OS.get_environment("GODOT_NEOVIM_TEST_SCRIPT")
	if script_path == "":
		script_path = "res://addons/godot-neovim-test/sandbox.gd"
	var res := load(script_path)
	if res:
		EditorInterface.edit_resource(res)
	await get_tree().create_timer(1.0).timeout
	# Focus the CodeEdit so the neovim plugin's focus-based editor detection
	# tracks the same CodeEdit the tests mutate (the window may be unfocused
	# when launched from a script).
	var se := EditorInterface.get_script_editor()
	if se and se.get_current_editor():
		var ce := se.get_current_editor().get_base_editor()
		if ce:
			ce.grab_focus()
	await get_tree().create_timer(1.0).timeout
	if not _prepare_runner():
		print("[godot-neovim-test] autorun: neovim plugin not found, aborting")
		get_tree().quit(1)
		return
	var test_id := OS.get_environment("GODOT_NEOVIM_TEST_ID")
	var category := OS.get_environment("GODOT_NEOVIM_TEST_CATEGORY")
	if test_id != "":
		await runner.run_test(int(test_id))
	elif category != "":
		await runner.run_filtered(category, "")
	else:
		await runner.run_tests()
	await get_tree().create_timer(0.5).timeout
	# Restore the sandbox buffers to their on-disk content so the editor's
	# save-on-quit doesn't overwrite the files with test residue. sandbox_b
	# first, then the primary sandbox so it ends up as the current tab.
	for restore_path in ["res://addons/godot-neovim-test/sandbox_b.gd", script_path]:
		if not ResourceLoader.exists(restore_path):
			continue
		var restore_res = load(restore_path)
		if not restore_res:
			continue
		EditorInterface.edit_resource(restore_res)
		await get_tree().create_timer(0.3).timeout
		var se_restore := EditorInterface.get_script_editor()
		if se_restore and se_restore.get_current_editor():
			var ce_restore := se_restore.get_current_editor().get_base_editor()
			if ce_restore:
				ce_restore.set_text(FileAccess.get_file_as_string(restore_path))
	print("[godot-neovim-test] autorun: done, quitting")
	get_tree().quit()


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
