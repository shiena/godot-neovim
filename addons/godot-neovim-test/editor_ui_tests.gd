@tool
extends Node
## Exercise real editor controls without calling production discovery helpers.

const BASE := "res://addons/godot-neovim-test/fixtures/"
var plugin: Node
var host: EditorPlugin
var result: Dictionary
var originals: Dictionary = {}
var controls: Dictionary = {}


func run_case(test: Dictionary, target: Node, owner_plugin: EditorPlugin) -> Dictionary:
	plugin = target
	host = owner_plugin
	result = {"id": test.id, "name": test.name, "category": "editor_ui", "passed": true, "diff": {}}
	print("[godot-neovim-test] TEST %d: %s ..." % [test.id, test.name])
	for file in ["a.gdshader", "b.gdshader", "notes.txt"]:
		originals[file] = FileAccess.get_file_as_string(BASE + file).replace("\r\n", "\n")
	await call(String(test.ui_case))
	# Restore through each editor before returning to the script suite.
	for file in controls:
		if is_instance_valid(controls[file]):
			var ce: CodeEdit = controls[file]
			ce.text = originals[file]
			if file.ends_with("gdshader"):
				await open_shader(file)
			await focus_editor(ce)
			plugin.action_save()
			await pause(0.3)
	EditorInterface.edit_resource(load("res://addons/godot-neovim-test/sandbox.gd"))
	await pause(0.4)
	var current := EditorInterface.get_script_editor().get_current_editor()
	if current and current.get_base_editor() is CodeEdit:
		await focus_editor(current.get_base_editor())
	return result


func pause(seconds: float = 0.2) -> void:
	await get_tree().create_timer(seconds).timeout


func check(label: String, actual: Variant, expected: Variant) -> void:
	if actual != expected:
		result.passed = false
		result.diff[label] = {"actual": actual, "expected": expected}


func focus_editor(ce: CodeEdit) -> void:
	ce.grab_focus()
	await pause()
	await key(ce, "Escape")
	await pause()


func find_shader(node: Node) -> CodeEdit:
	if node is CodeEdit and node.is_visible_in_tree():
		var ancestor := node.get_parent()
		while ancestor:
			if "Shader" in ancestor.get_class():
				return node
			ancestor = ancestor.get_parent()
	for child in node.get_children():
		var found := find_shader(child)
		if found:
			return found
	return null


func open_shader(file: String) -> CodeEdit:
	EditorInterface.edit_resource(load(BASE + file))
	await pause(0.7)
	var ce := find_shader(get_tree().root)
	check("shader editor found", ce != null, true)
	if ce:
		controls[file] = ce
		await focus_editor(ce)
	return ce


func key(ce: CodeEdit, name: String, unicode: int = 0) -> void:
	var event := InputEventKey.new()
	event.pressed = true
	event.keycode = OS.find_keycode_from_string(name)
	event.physical_keycode = event.keycode
	event.shift_pressed = unicode >= 65 and unicode <= 90
	event.unicode = unicode
	ce.get_viewport().push_input(event)
	await pause(0.08)


func ex(ce: CodeEdit, command: String) -> void:
	plugin.action_open_command_line()
	for character in command:
		await key(ce, character.to_upper(), character.unicode_at(0))
	await key(ce, "Enter")
	await pause(0.5)


func append_marker(ce: CodeEdit) -> void:
	var last := ce.get_line_count() - 1
	while last > 0 and ce.get_line(last).is_empty():
		last -= 1
	for character in "%dGA" % (last + 1):
		await key(ce, character.to_upper(), character.unicode_at(0))
	await pause()
	await key(ce, "X", 120)
	await key(ce, "Escape")
	await pause()


func shader_edit() -> void:
	var ce := await open_shader("a.gdshader")
	if not ce:
		return
	var before := ce.text.strip_edges()
	await append_marker(ce)
	check("shader input", ce.text.strip_edges(), before + "x")


func shader_save() -> void:
	var ce := await open_shader("a.gdshader")
	if not ce:
		return
	await append_marker(ce)
	await ex(ce, "w")
	check(
		"shader saved",
		FileAccess.get_file_as_string(BASE + "a.gdshader").strip_edges(),
		ce.text.strip_edges()
	)


func shader_save_all() -> void:
	var a := await open_shader("a.gdshader")
	if not a:
		return
	await append_marker(a)
	var b := await open_shader("b.gdshader")
	if not b:
		return
	await append_marker(b)
	await ex(b, "wa")
	for file in ["a.gdshader", "b.gdshader"]:
		check(
			file + " saved",
			FileAccess.get_file_as_string(BASE + file).strip_edges(),
			controls[file].text.strip_edges()
		)


func shader_tabs() -> void:
	var a := await open_shader("a.gdshader")
	var b := await open_shader("b.gdshader")
	if not a or not b:
		return
	await key(b, "G", 103)
	await key(b, "T", 84)
	await pause(0.5)
	check("previous shader", a.is_visible_in_tree(), true)
	await append_marker(a)
	check(
		"previous shader input",
		a.text.strip_edges(),
		String(originals["a.gdshader"]).strip_edges() + "x"
	)
	await key(a, "G", 103)
	await key(a, "T", 116)
	await pause(0.5)
	check("next shader", b.is_visible_in_tree(), true)


func shader_close() -> void:
	var a := await open_shader("a.gdshader")
	var b := await open_shader("b.gdshader")
	if not a or not b:
		return
	await ex(b, "q")
	check("closed shader", not is_instance_valid(b) or not b.is_inside_tree(), true)
	check("remaining shader", is_instance_valid(a) and a.is_visible_in_tree(), true)


func activate_file(node: Node, path: String) -> bool:
	if node is Tree:
		var selected: TreeItem = node.get_selected()
		if selected and selected.get_metadata(0) == path:
			node.item_activated.emit()
			return true
	if node is ItemList:
		for index in node.get_selected_items():
			if node.get_item_metadata(index) == path:
				node.item_activated.emit(index)
				return true
	for child in node.get_children():
		if activate_file(child, path):
			return true
	return false


func text_edit_save() -> void:
	var dock := EditorInterface.get_file_system_dock()
	dock.navigate_to_path(BASE + "notes.txt")
	await pause(0.4)
	check("text file activated", activate_file(dock, BASE + "notes.txt"), true)
	await pause(0.6)
	var editor := EditorInterface.get_script_editor().get_current_editor()
	if not editor or editor.get_class() != "TextEditor":
		check("text editor", editor.get_class() if editor else "null", "TextEditor")
		return
	var ce := editor.get_base_editor() as CodeEdit
	controls["notes.txt"] = ce
	await focus_editor(ce)
	await append_marker(ce)
	check("text input", ce.text.strip_edges(), String(originals["notes.txt"]).strip_edges() + "x")
	await ex(ce, "w")
	check(
		"text saved",
		FileAccess.get_file_as_string(BASE + "notes.txt").strip_edges(),
		ce.text.strip_edges()
	)


func external_edit() -> void:
	var box := VBoxContainer.new()
	box.name = "ExternalCodeEditTest"
	var ce := CodeEdit.new()
	ce.custom_minimum_size = Vector2(220, 180)
	ce.text = "alpha beta"
	box.add_child(ce)
	host.add_control_to_bottom_panel(box, "External test")
	host.make_bottom_panel_item_visible(box)
	await focus_editor(ce)
	for character in "0dw":
		await key(ce, character.to_upper(), character.unicode_at(0))
	await pause()
	check("external delete", ce.text, "beta")
	await append_marker(ce)
	check("external insert", ce.text, "betax")
	host.remove_control_from_bottom_panel(box)
	box.queue_free()
	await pause()


func floating_edit() -> void:
	var window := Window.new()
	window.size = Vector2i(500, 300)
	var tabs := TabContainer.new()
	tabs.set_anchors_and_offsets_preset(Control.PRESET_FULL_RECT)
	var hidden := CodeEdit.new()
	hidden.text = "hidden"
	var active := CodeEdit.new()
	active.text = "active"
	tabs.add_child(hidden)
	tabs.add_child(active)
	window.add_child(tabs)
	add_child(window)
	var main_focus := EditorInterface.get_base_control().get_viewport().gui_get_focus_owner()
	if main_focus:
		main_focus.release_focus()
	window.popup()
	tabs.current_tab = 1
	await focus_editor(active)
	await append_marker(active)
	check("floating active", active.text, "activex")
	check("floating hidden", hidden.text, "hidden")
	window.queue_free()
	await pause()


func native_text_controls() -> void:
	var box := VBoxContainer.new()
	var text := TextEdit.new()
	text.custom_minimum_size = Vector2(220, 100)
	var line := LineEdit.new()
	box.add_child(text)
	box.add_child(line)
	host.add_control_to_bottom_panel(box, "Native text test")
	host.make_bottom_panel_item_visible(box)
	for control in [text, line]:
		control.grab_focus()
		await pause()
		var event := InputEventKey.new()
		event.pressed = true
		event.keycode = KEY_X
		event.unicode = 120
		control.get_viewport().push_input(event)
		await pause()
		check(control.get_class() + " input", control.text, "x")
	host.remove_control_from_bottom_panel(box)
	box.queue_free()
	await pause()
