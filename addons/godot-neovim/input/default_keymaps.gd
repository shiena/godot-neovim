## Default key mappings for GodotNeovim.
##
## Maps resolved key strings to action method names on the GodotNeovimPlugin.
## Keys not found in the keymap are sent directly to Neovim via action_send_keys.
##
## Key format follows Neovim notation:
## - Single chars: "a", "A", "0", "$", etc.
## - Control: "<C-a>", "<C-f>", etc.
## - Special: "<CR>", "<Esc>", "<Tab>", etc.
## - Sequences: "gg", "gd", "ZZ", etc.
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

		# NOTE: z-prefix commands (zo/zc/za/zR/zM folds, zz/zt/zb scrolls) are
		# intercepted in Rust (handle_scroll_command) before keymap dispatch,
		# so they cannot be remapped here.

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


## Insert mode keymap.
## The Rust dispatcher only routes Ctrl/Alt + letter keys here (see
## process_insert_key_event_impl). Plain characters and unmodified specials
## fall through to Godot directly, Ctrl/Alt + nav/delete keys (BS, Del,
## arrows, Home, End, PageUp/Down) are passed through to Godot's CodeEdit
## natively, and `<C-r>` is handled in Rust as a pending register-paste —
## none of those are customizable from this map.
##
## Entries below define Vim's standard insert-mode commands. The Rust
## dispatcher pushes Godot's current buffer to Neovim before sending these
## keys, so commands like `<C-w>` operate on the up-to-date text. Unmapped
## keys fall through to action_send_keys which forwards them verbatim.
static func get_insert_keymap() -> Dictionary:
	return {
		# Word/line deletion (Vim insert mode)
		"<C-w>": "action_send_keys",
		"<C-u>": "action_send_keys",
		"<C-h>": "action_send_keys",

		# Insertion helpers
		"<C-o>": "action_send_keys",
		"<C-v>": "action_send_keys",
		"<C-k>": "action_send_keys",
		"<C-a>": "action_send_keys",
		"<C-y>": "action_send_keys",
		"<C-e>": "action_send_keys",

		# Indent / unindent
		"<C-t>": "action_send_keys",
		"<C-d>": "action_send_keys",

		# Completion popup navigation
		"<C-n>": "action_send_keys",
		"<C-p>": "action_send_keys",
	}


## Replace mode keymap.
## Same routing rules as insert mode (see get_insert_keymap()).
static func get_replace_keymap() -> Dictionary:
	return {
		# Replace mode borrows most insert-mode bindings.
		"<C-w>": "action_send_keys",
		"<C-u>": "action_send_keys",
		"<C-h>": "action_send_keys",
		"<C-o>": "action_send_keys",
		"<C-v>": "action_send_keys",
	}
