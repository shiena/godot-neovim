## Default key mappings for GodotNeovim.
##
## Maps resolved key strings to action method names on the GodotNeovimPlugin.
## Keys not found in the keymap are sent directly to Neovim via action_send_keys.
##
## Key format follows Neovim notation:
## - Single chars: "a", "A", "0", "$", etc.
## - Control: "<C-a>", "<C-f>", etc.
## - Special: "<CR>", "<Esc>", "<Tab>", etc.
## - Sequences: "gg", "gd", "zo", "ZZ", etc.
class_name GodotNeovimDefaultKeymaps


## Normal mode keymap.
## Keys handled by Rust internally (insert/replace/command/search modes,
## pending operations like f/t/r/m/q/@/", count prefixes, and prefix keys
## g/[/]/z/Z/>/<) are NOT in this map - they are resolved by process_key_event.
static func get_normal_keymap() -> Dictionary:
	return {
		# --- Scrolling / Page navigation ---
		"<C-b>": "action_page_up",
		"<C-f>": "action_page_down",
		"<C-d>": "action_half_page_down",
		"<C-u>": "action_half_page_up",
		"<C-y>": "action_scroll_viewport_up",
		"<C-e>": "action_scroll_viewport_down",

		# --- Number increment/decrement ---
		"<C-a>": "action_increment",
		"<C-x>": "action_decrement",

		# --- Jump list ---
		"<C-o>": "action_jump_back",
		"<C-i>": "action_jump_forward",

		# --- File info ---
		"<C-g>": "action_show_file_info",

		# --- Search ---
		"/": "action_open_search_forward",
		"?": "action_open_search_backward",
		"n": "action_search_next",
		"N": "action_search_prev",
		"*": "action_search_word_forward",
		"#": "action_search_word_backward",

		# --- Command line ---
		":": "action_open_command_line",

		# --- Undo / Redo ---
		"u": "action_undo",
		"<C-r>": "action_redo",

		# --- Documentation ---
		"K": "action_open_documentation",

		# --- g-prefix commands (resolved as sequences) ---
		"gd": "action_goto_definition",
		"gf": "action_goto_file",
		"gx": "action_open_url",
		"gt": "action_next_tab",
		"gT": "action_prev_tab",
		"gv": "action_visual_block_toggle",
		"gj": "action_display_line_down",
		"gk": "action_display_line_up",
		"gI": "action_insert_at_column_zero",
		"gi": "action_insert_at_last_position",
		"ga": "action_show_char_info",
		"g&": "action_repeat_substitution",
		"gJ": "action_join_no_space",
		"gp": "action_paste_move_cursor",
		"gP": "action_paste_before_move_cursor",
		"ge": "action_word_end_backward",
		"g0": "action_display_line_start",
		"g$": "action_display_line_end",
		"g^": "action_display_line_first_non_blank",

		# --- z-prefix commands (resolved as sequences) ---
		"zo": "action_fold_open",
		"zc": "action_fold_close",
		"za": "action_fold_toggle",
		"zR": "action_fold_open_all",
		"zM": "action_fold_close_all",

		# --- Z-prefix commands (resolved as sequences) ---
		"ZZ": "action_save_and_close",
		"ZQ": "action_close_discard",
	}


## Visual mode keymap.
## Most keys fall through to action_send_keys (Neovim handles visual operations).
## Only keys with different behavior in visual mode are listed here.
static func get_visual_keymap() -> Dictionary:
	return {
		# Ctrl+B switches to visual block in visual mode (instead of page up)
		"<C-b>": "action_visual_block_toggle",

		# Scrolling (same as normal mode)
		"<C-f>": "action_page_down",
		"<C-d>": "action_half_page_down",
		"<C-u>": "action_half_page_up",
		"<C-y>": "action_scroll_viewport_up",
		"<C-e>": "action_scroll_viewport_down",

		# Search
		"/": "action_open_search_forward",
		"?": "action_open_search_backward",
		"n": "action_search_next",
		"N": "action_search_prev",
		"*": "action_search_word_forward",
		"#": "action_search_word_backward",

		# Command line
		":": "action_open_command_line",

		# g-prefix commands available in visual mode
		"gv": "action_visual_block_toggle",
		"gj": "action_display_line_down",
		"gk": "action_display_line_up",
	}
