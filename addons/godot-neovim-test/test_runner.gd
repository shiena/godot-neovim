@tool
extends Node

## Test runner for godot-neovim.
##
## Test cases are JSON files in `tests/`. Each case is a Dictionary with:
##
##   id            int    Unique identifier (sorted ascending)
##   name          str    Human-readable test name
##   text          str    Initial CodeEdit text (set via direct API + sync)
##   cursor        [l,c]  Initial cursor position (0-indexed line, col)
##   keys          [str]  Optional: Neovim-notation keys via action_send_keys
##   steps         [dict] Optional: mixed key/event sequence (see _run_step)
##                          {"send": "..."} → action_send_keys
##                          {"event": "BACKSPACE", "ctrl": true} → InputEventKey
##                          {"wait": 0.1} → delay seconds
##   interval      float  Optional: delay between keys/events (default 0.05s)
##   sync_wait     float  Optional: post-keys Neovim sync wait (default 0.3s)
##   category      str    Optional: tag override (defaults to filename stem)
##
##   expect_text          str    Expected CodeEdit text after run
##   expect_cursor        [l,c]  Expected cursor position
##   expect_mode          str    Expected vim mode (n/i/v/V/R/c)
##   expect_macro_buffer  [str]  Expected macro buffer contents
##   expect_pending_insert_register  bool  Expected pending <C-r> state
##
## Either `keys` or `steps` (not both) is required; presence of any expect_*
## determines which axes are checked.

var plugin: Node
var test_plugin: EditorPlugin
var results: Array[Dictionary] = []
var running: bool = false

# Diagnostics: ring buffer of recent steps for failure reports
var _recent_actions: Array[String] = []
const RECENT_ACTIONS_CAP := 20


func setup(p_plugin: Node, p_test_plugin: EditorPlugin) -> void:
	plugin = p_plugin
	test_plugin = p_test_plugin


# ---- Public entry points ----

func run_tests(filter: String = "") -> void:
	var cases := _load_test_cases()
	if filter != "":
		cases = cases.filter(func(t: Dictionary) -> bool: return str(t.id) == filter)
	await _run_cases(cases)


func run_test(id: int) -> void:
	var cases := _load_test_cases()
	var picked := cases.filter(func(t: Dictionary) -> bool: return int(t.id) == id)
	if picked.is_empty():
		_log("[color=red]Test %d not found[/color]" % id)
		return
	await _run_cases(picked)


## Run tests filtered by category (JSON filename stem, e.g. "insert_mode")
## and optional substring match against test name.
func run_filtered(category: String, name_pattern: String) -> void:
	var cases := _load_test_cases()
	if category != "" and category != "all":
		cases = cases.filter(func(t: Dictionary) -> bool:
			return t.get("category", "") == category)
	if name_pattern != "":
		var lower := name_pattern.to_lower()
		cases = cases.filter(func(t: Dictionary) -> bool:
			return String(t.get("name", "")).to_lower().contains(lower))
	await _run_cases(cases)


## List all test categories (filename stems) for UI population.
func list_categories() -> PackedStringArray:
	var cats := PackedStringArray()
	var base_path := "res://addons/godot-neovim-test/tests/"
	var dir := DirAccess.open(base_path)
	if not dir:
		return cats
	dir.list_dir_begin()
	var file_name := dir.get_next()
	while file_name != "":
		if file_name.ends_with(".json"):
			cats.append(file_name.trim_suffix(".json"))
		file_name = dir.get_next()
	cats.sort()
	return cats


# ---- Core run loop ----

func _run_cases(cases: Array) -> void:
	if running:
		return
	running = true
	test_plugin.set_running(true)
	results.clear()

	_log("=== Running %d tests ===" % cases.size())

	for i in range(cases.size()):
		test_plugin.update_progress(i, cases.size())
		await _run_single(cases[i])

	test_plugin.update_progress(cases.size(), cases.size())
	_print_summary()
	_save_results()
	running = false
	test_plugin.set_running(false)


func _run_single(t: Dictionary) -> void:
	_log("TEST %d: %s ..." % [t.id, t.name])
	_recent_actions.clear()

	# 1. Ensure normal mode + clean buffer setup
	await _reset_to_normal()
	await _setup_buffer(t)

	# 2. Set cursor
	if t.has("cursor"):
		await _set_cursor(t.cursor[0], t.cursor[1])

	# 3. Execute steps (preferred) or keys (legacy)
	var interval: float = float(t.get("interval", 0.05))
	if t.has("steps"):
		for step in t.steps:
			await _run_step(step, interval)
	elif t.has("keys"):
		for key in t.keys:
			_record_recent("send", str(key))
			plugin.action_send_keys(key)
			await get_tree().create_timer(interval).timeout

	# 4. Wait for Neovim → Godot sync to settle
	var sync_wait: float = float(t.get("sync_wait", 0.3))
	await get_tree().create_timer(sync_wait).timeout

	# 5. Verify
	var result := _check(t)
	results.append(result)

	# 6. Log result with diagnostics on failure
	var status := "[color=green]PASS[/color]" if result.passed else "[color=red]FAIL[/color]"
	_log("  %s: %s" % [status, t.name])
	if not result.passed:
		_log_failure_diagnostics(t, result)


# ---- Step execution ----

func _run_step(step: Dictionary, interval: float) -> void:
	if step.has("send"):
		_record_recent("send", str(step.send))
		plugin.action_send_keys(step.send)
		await get_tree().create_timer(interval).timeout
	elif step.has("event"):
		var modifiers := ""
		if step.get("ctrl", false): modifiers += "C-"
		if step.get("alt", false): modifiers += "A-"
		if step.get("shift", false): modifiers += "S-"
		if step.get("meta", false): modifiers += "M-"
		_record_recent("event", "<%s%s>" % [modifiers, step.event])
		_inject_key_event(step)
		await get_tree().create_timer(interval).timeout
	elif step.has("wait"):
		await get_tree().create_timer(float(step.wait)).timeout
	else:
		_log("[color=yellow]Unknown step type: %s[/color]" % JSON.stringify(step))


## Build an InputEventKey from a step Dictionary and dispatch it through
## Godot's input system so the plugin's input() / process_*_key_event_impl
## chain runs exactly as it would for a real keypress.
func _inject_key_event(step: Dictionary) -> void:
	var event := InputEventKey.new()
	var keycode_name := String(step.event)
	event.keycode = OS.find_keycode_from_string(keycode_name)
	if event.keycode == 0:
		_log("[color=yellow]Unknown keycode: %s[/color]" % keycode_name)
		return
	event.physical_keycode = event.keycode
	event.ctrl_pressed = step.get("ctrl", false)
	event.alt_pressed = step.get("alt", false)
	event.shift_pressed = step.get("shift", false)
	event.meta_pressed = step.get("meta", false)
	event.pressed = true

	# Set unicode for printable letter keys so key_event_to_nvim_notation
	# can resolve them (Godot doesn't auto-fill this for synthetic events).
	if step.has("unicode"):
		event.unicode = int(step.unicode)
	elif event.keycode >= KEY_A and event.keycode <= KEY_Z:
		var ch := char(event.keycode)
		event.unicode = (ch.to_lower() if not event.shift_pressed else ch).unicode_at(0)

	# push_input (not Input.parse_input_event): deliver straight to the editor
	# viewport's shortcut/gui phases. parse_input_event goes through the OS
	# input queue where some modified nav keys (Ctrl+Left/Home) get consumed
	# before reaching the CodeEdit.
	var vp := EditorInterface.get_base_control().get_viewport()
	vp.push_input(event)


# ---- Buffer setup ----

func _reset_to_normal() -> void:
	# Cancel any pending operator / register / macro state, exit to normal.
	plugin.action_send_keys("<Esc>")
	await get_tree().create_timer(0.05).timeout


func _setup_buffer(t: Dictionary) -> void:
	if not t.has("text"):
		return
	var code_edit := _get_code_edit()
	if not code_edit:
		return
	# Direct API set is robust for any text content (no <, \n escaping needed).
	code_edit.set_text(String(t.text))
	# Force Godot → Neovim sync so the test starts with both buffers identical.
	if plugin.has_method("sync_buffer_for_test"):
		plugin.sync_buffer_for_test()
	else:
		# Backwards compat: fall back to the older buffer-after-toggle-comment hook.
		plugin.call_deferred("sync_buffer_after_toggle_comment")
	await get_tree().create_timer(0.15).timeout


func _set_cursor(line: int, col: int) -> void:
	plugin.action_send_keys(":call cursor(%d,%d)<CR>" % [line + 1, col + 1])
	await get_tree().create_timer(0.1).timeout


# ---- Verification ----

func _check(t: Dictionary) -> Dictionary:
	var result: Dictionary = {
		"id": t.id,
		"name": t.name,
		"category": t.get("category", ""),
		"passed": true,
		"diff": {},
	}
	var code_edit := _get_code_edit()
	if not code_edit:
		result.passed = false
		result.diff["error"] = {"expected": "CodeEdit found", "actual": "null"}
		return result

	if t.has("expect_text"):
		# Strip only trailing newlines (NOT spaces/tabs) so expectations
		# with trailing whitespace like "hello " remain testable.
		var actual: String = code_edit.get_text()
		while actual.ends_with("\n"):
			actual = actual.substr(0, actual.length() - 1)
		if actual != t.expect_text:
			result.passed = false
			result.diff["text"] = {"expected": t.expect_text, "actual": actual}

	if t.has("expect_cursor"):
		var line := code_edit.get_caret_line()
		var col := code_edit.get_caret_column()
		if line != t.expect_cursor[0] or col != t.expect_cursor[1]:
			result.passed = false
			result.diff["cursor"] = {
				"expected": t.expect_cursor,
				"actual": [line, col],
			}

	if t.has("expect_mode"):
		var mode: String = plugin.get_vim_mode()
		if mode != t.expect_mode:
			result.passed = false
			result.diff["mode"] = {"expected": t.expect_mode, "actual": mode}

	if t.has("expect_macro_buffer") and plugin.has_method("get_macro_buffer_for_test"):
		var actual_macro: Array = Array(plugin.get_macro_buffer_for_test())
		var expected_macro: Array = t.expect_macro_buffer
		if actual_macro != expected_macro:
			result.passed = false
			result.diff["macro_buffer"] = {
				"expected": expected_macro,
				"actual": actual_macro,
			}

	if t.has("expect_pending_insert_register") \
			and plugin.has_method("is_pending_insert_register_for_test"):
		var actual_pending: bool = plugin.is_pending_insert_register_for_test()
		var expected_pending: bool = bool(t.expect_pending_insert_register)
		if actual_pending != expected_pending:
			result.passed = false
			result.diff["pending_insert_register"] = {
				"expected": expected_pending,
				"actual": actual_pending,
			}

	return result


# ---- Failure diagnostics ----

func _log_failure_diagnostics(t: Dictionary, result: Dictionary) -> void:
	for key in result.diff:
		var d: Dictionary = result.diff[key]
		_log("    [color=red]%s[/color]: expected=%s actual=%s" % [
			key, _short(d.expected), _short(d.actual)
		])

	# Buffer snapshot with cursor visualization.
	var code_edit := _get_code_edit()
	if code_edit:
		var line := code_edit.get_caret_line()
		var col := code_edit.get_caret_column()
		_log("    cursor: line=%d col=%d  mode=%s" % [line, col, plugin.get_vim_mode()])
		var lines: PackedStringArray = code_edit.get_text().split("\n")
		var first := max(0, line - 2)
		var last := min(lines.size() - 1, line + 2)
		for i in range(first, last + 1):
			var marker := ">" if i == line else " "
			var content := lines[i]
			if i == line:
				# Highlight caret column
				var safe_col := clamp(col, 0, content.length())
				content = content.substr(0, safe_col) + "|" + content.substr(safe_col)
			_log("    %s %4d: %s" % [marker, i, content])

	# Recent action trail (last N steps).
	if not _recent_actions.is_empty():
		_log("    recent: %s" % ", ".join(_recent_actions))

	# Attach diagnostics to the result for the JSON dump.
	result["diagnostics"] = {
		"buffer": code_edit.get_text() if code_edit else "",
		"cursor": [code_edit.get_caret_line(), code_edit.get_caret_column()] if code_edit else [],
		"mode": plugin.get_vim_mode(),
		"recent_actions": _recent_actions.duplicate(),
	}


func _record_recent(kind: String, payload: String) -> void:
	_recent_actions.append("%s(%s)" % [kind, payload])
	if _recent_actions.size() > RECENT_ACTIONS_CAP:
		_recent_actions.pop_front()


func _short(value) -> String:
	var s := JSON.stringify(value)
	if s.length() > 80:
		return s.substr(0, 77) + "..."
	return s


# ---- Editor helpers ----

func _get_code_edit() -> CodeEdit:
	var se := EditorInterface.get_script_editor()
	if not se:
		return null
	var editor := se.get_current_editor()
	if not editor:
		return null
	return editor.get_base_editor() as CodeEdit


# ---- Test Case Loading ----

func _load_test_cases() -> Array[Dictionary]:
	var cases: Array[Dictionary] = []
	var base_path := "res://addons/godot-neovim-test/tests/"
	var dir := DirAccess.open(base_path)
	if not dir:
		_log("[color=red]ERROR: Cannot open tests/ directory[/color]")
		return cases

	dir.list_dir_begin()
	var file_name := dir.get_next()
	while file_name != "":
		if file_name.ends_with(".json"):
			var content := FileAccess.get_file_as_string(base_path + file_name)
			var parsed = JSON.parse_string(content)
			if parsed is Array:
				var category := file_name.trim_suffix(".json")
				for c in parsed:
					if c is Dictionary:
						if not c.has("category"):
							c["category"] = category
						cases.append(c)
				_log("Loaded %d tests from %s" % [parsed.size(), file_name])
			else:
				_log("[color=yellow]WARNING: Invalid format in %s[/color]" % file_name)
		file_name = dir.get_next()

	cases.sort_custom(func(a: Dictionary, b: Dictionary) -> bool: return a.id < b.id)
	return cases


# ---- Logging & Results ----

func _log(msg: String) -> void:
	# Strip bbcode for console output, keep for the in-editor log panel.
	var plain := msg
	for tag in ["[color=green]", "[color=red]", "[color=yellow]", "[/color]"]:
		plain = plain.replace(tag, "")
	print("[godot-neovim-test] ", plain)
	test_plugin.append_log(msg)


func _print_summary() -> void:
	var passed := results.filter(func(r: Dictionary) -> bool: return r.passed).size()
	var total := results.size()
	var color := "green" if passed == total else "red"
	_log("[color=%s]=== %d / %d passed ===[/color]" % [color, passed, total])
	if passed != total:
		var failures := results.filter(func(r: Dictionary) -> bool: return not r.passed)
		for f in failures:
			_log("  failed: #%d %s" % [f.id, f.name])


func _save_results() -> void:
	var file := FileAccess.open("user://godot_neovim_test_results.json", FileAccess.WRITE)
	if file:
		file.store_string(JSON.stringify(results, "  "))
		_log("Results saved to user://godot_neovim_test_results.json")
