# Limitations and known issues

[Back to README](../README.md)

This plugin has architectural limitations due to using Godot's native CodeEdit for text editing.

## Insert mode

Insert mode uses Godot's native input system for text editing (to keep auto-completion, IME, and undo working), while still forwarding Vim's insert-mode commands to Neovim. The plugin pushes the current Godot buffer to Neovim before each `Ctrl`/`Alt`-modified key so commands operate on the up-to-date text.

**Supported Vim insert-mode commands**: `Ctrl+W`, `Ctrl+U`, `Ctrl+H`, `Ctrl+R{reg}`, `Ctrl+O`, `Ctrl+T`, `Ctrl+D`, `Ctrl+V`, `Ctrl+K`, `Ctrl+A`, `Ctrl+Y`, `Ctrl+E`.

**Keys passed through to Godot's CodeEdit** (cannot be remapped to send to Neovim):

| Key | Godot native action |
|-----|---------------------|
| `Ctrl+Backspace` / `Ctrl+Delete` | Delete word backward / forward |
| `Ctrl+Left` / `Ctrl+Right` | Move by word |
| `Ctrl+Up` / `Ctrl+Down` | Move/scroll by line |
| `Ctrl+Home` / `Ctrl+End` | Jump to file start / end |
| `Ctrl+PageUp` / `Ctrl+PageDown` | Jump by page |
| `Alt`-modified variants of the above | Same as `Ctrl` variants where Godot supports them |

**Known limitations within insert mode**:

| Limitation | Detail |
|-----------|--------|
| `Ctrl+N` / `Ctrl+P` (Neovim completion) | Forwarded to Neovim, but Godot's native auto-completion popup is usually preferred. The two completion systems do not interact. |
| `<C-r>=` (expression register) | Opens Neovim's expression command line, which Godot is not aware of — typed text goes to Godot instead of Neovim's prompt. |
| `<C-r><C-r>{reg}` / `<C-r><C-o>{reg}` / `<C-r><C-p>{reg}` | Not supported. The `<C-r>` prefix pending state clears after one key, so the variant modifier becomes the register name. |
| Dot repeat (`.`) of inserted text | Only structural commands (e.g., `cw`) are replayed; the inserted text itself is not re-typed because Neovim doesn't see individual keystrokes. |
| Macro recording with IME | Only confirmed characters are recorded; intermediate composition states are not captured. |

## Other limitations

| Feature | Description |
|---------|-------------|
| Neovim undo | Uses Godot's undo system |
| Neovim config | `init.lua` and plugins are not loaded by default (`neovim_clean = true`). Can be enabled but may cause compatibility issues with some plugins (e.g., copilot.vim, lexima.vim). |
| `K` for signals | Signal documentation lookup not supported (class/method/property/constant only) |
| `[m`/`]m` for GDScript | Method jump commands require Neovim treesitter or language support. GDScript is not recognized by Neovim. |

## Toggle comments (`Ctrl+/`)

`Ctrl+/` (`Cmd+/` on macOS) toggles comments using Godot's native CodeEdit feature. Since this is a Godot passthrough (not a Neovim command), the following limitations apply:

- **Macro recording**: `Ctrl+/` is not recorded during macro recording (`q{a-z}`)
- **Dot repeat**: `.` does not repeat `Ctrl+/`
- **Undo**: Uses Godot's undo system, not Neovim's `u`

## Alt key composition (Option / AltGr)

When the OS produces a composed character via the Alt modifier (e.g. macOS Option+Q → `@`, Windows AltGr+Q → `@` on German layout), the plugin inserts the composed character as plain text instead of sending `<A-x>` / `<C-A-x>` to Neovim. This makes layouts that rely on Alt for character composition usable in Insert and Replace modes.

As a side effect, the following Vim mappings are unreachable on those layouts:

- **macOS**: Any `<A-letter>` mapping where Option+letter produces a character on the active layout. Configure macOS to "Use Option as Meta key" in your terminal/editor or switch to a layout that does not compose to use these mappings.
- **Windows (AltGr layouts, e.g. German/French/Spanish)**: `<C-A-letter>` mappings on keys that AltGr composes. Windows reports AltGr as Ctrl+Alt simultaneously at the OS level, so AltGr+Q and Ctrl+Alt+Q are indistinguishable from any application. Use a different modifier combination or disable AltGr to use these mappings.

## Known issues

| Issue | Workaround |
|-------|------------|
| Dirty flag not set after some operations (named register paste, macro playback) | Switch to another script tab and back (`gt` then `gT`) |
| `(*)` marker remains after `:e!` reload | Switch tabs with `gt`/`gT` to clear the marker |
| Macro recording with IME | Only confirmed characters are recorded; intermediate compositions are not captured |
