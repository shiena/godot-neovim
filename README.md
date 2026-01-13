<p align="center">
  <img src="addons/godot-neovim/icon.png" alt="godot-neovim logo" width="128" height="128">
</p>

<h1 align="center">godot-neovim</h1>

<p align="center">
  Godot editor plugin that uses Neovim as the backend for script editing.<br>
  Inspired by <a href="https://github.com/vscode-neovim/vscode-neovim">vscode-neovim</a> and <a href="https://github.com/igorgue/godot-vim">godot-vim</a>.
</p>

## Overview

godot-neovim integrates Neovim into Godot's script editor, allowing you to use the full power of Neovim for editing GDScript and other files within Godot. Unlike simple vim keybinding emulators, this plugin runs an actual Neovim process and synchronizes the buffer between Godot and Neovim.

## Features

- Real Neovim backend (not just keybinding emulation)
- Mode indicator with cursor position (e.g., `NORMAL 123:45`)
- Cursor synchronization between Godot and Neovim
- Support for count prefixes (e.g., `4j`, `10gg`)
- Support for operator-pending commands (e.g., `gg`, `dd`, `yy`)
- Configurable input mode (hybrid for IME support, strict for full Neovim control)
- Ctrl+[ as Escape alternative (terminal standard)
- Full/half page scrolling (`Ctrl+F`, `Ctrl+B`, `Ctrl+D`, `Ctrl+U`)
- Word search under cursor (`*`, `#`, `n`, `N`)
- Character find motions (`f`, `F`, `t`, `T`, `;`, `,`)
- Line navigation (`0`, `^`, `$`) and paragraph movement (`{`, `}`)
- Bracket matching (`%`) and go to definition (`gd`)
- Character editing (`x`, `X`, `r`, `~`) and line operations (`J`, `>>`, `<<`)
- Configurable Neovim executable path via Editor Settings
- Path validation on startup and settings change

### Current Status

This plugin is in early development. The following features are implemented:

- ✅ Normal mode navigation (`h`, `j`, `k`, `l`, `gg`, `G`, `w`, `b`, etc.)
- ✅ Mode switching (`i`, `a`, `o`, `v`, `Escape`, `Ctrl+[`, etc.)
- ✅ Mode indicator display with line:column
- ✅ Cursor position synchronization (Neovim ↔ Godot)
- ✅ Buffer synchronization (Godot ↔ Neovim, bidirectional)
- ✅ Operator-pending commands with timeout handling (`gg`, `dd`, `yy`, etc.)
- ✅ Insert mode with configurable input handling (hybrid/strict modes)
- ✅ Normal mode edits reflected in Godot (`dd`, `yy`, `p`, etc.)
- ✅ Visual mode selection display (`v`, `V`)
- ✅ Command-line mode (`:w`, `:q`, `:wq`, `ZZ`, `ZQ`, `:%s/old/new/g`)
- ✅ Search word under cursor (`*` forward, `#` backward, `n`/`N` repeat)
- ✅ Character find (`f`/`F`/`t`/`T`, `;`/`,` repeat)
- ✅ Line navigation (`0`, `^`, `$`)
- ✅ Paragraph movement (`{`, `}`)
- ✅ Bracket matching (`%`)
- ✅ Character editing (`x`, `X`, `r`, `~`)
- ✅ Line operations (`J` join, `>>` indent, `<<` unindent)
- ✅ Full/half page scrolling (`Ctrl+F`, `Ctrl+B`, `Ctrl+D`, `Ctrl+U`)
- ✅ Go to definition (`gd`)
- ✅ Dot repeat (`.`)
- ✅ Text objects (`ciw`, `di"`, `ya{`, etc.)
- ✅ Case operators (`gu`, `gU`, `g~` + motion)
- ✅ Command history (Up/Down in command-line mode)
- ✅ Marks (`m{a-z}`, `'{a-z}`, `` `{a-z} ``)
- ✅ Macros (`q{a-z}`, `@{a-z}`, `@@`)
- ✅ Named registers (`"{a-z}yy`, `"{a-z}dd`, `"{a-z}p`)
- ✅ Number increment/decrement (`Ctrl+A`, `Ctrl+X`)
- ✅ Jump list navigation (`Ctrl+O`, `Ctrl+I`)
- ✅ Replace mode (`R`)
- ✅ Count operations with registers (`"{a-z}3dd`, `"{a-z}5yy`)
- ✅ Change/substitute commands (`s`, `S`, `cc`, `C`, `D`, `Y`)
- ✅ Visual mode selection toggle (`o`)
- ✅ Additional g-commands (`gI`, `ga`, `gJ`, `ge`, `gp`, `gP`, `g&`)
- ✅ File operations (`:wa`, `:wqa`, `:e!`, `Ctrl+G`)
- ✅ Repeat last Ex command (`@:`)
- ✅ Show jump/change list (`:jumps`, `:changes`)

## Requirements

- Godot 4.3 - 4.5
- Neovim 0.9.0 or later
- Rust toolchain (for building from source)

## Installation

### From Release (Recommended)

1. Download the latest release for your platform
2. Extract to your Godot project's `addons/godot-neovim/` directory
3. Enable the plugin in `Project > Project Settings > Plugins`

### Building from Source

1. Clone the repository:
   ```bash
   git clone https://github.com/shiena/godot-neovim.git
   cd godot-neovim
   ```

2. Build the plugin:
   ```bash
   cargo build --release
   ```

3. Copy the built library to the addons folder:
   ```bash
   # Windows
   cp target/release/godot_neovim.dll addons/godot-neovim/

   # Linux
   cp target/release/libgodot_neovim.so addons/godot-neovim/

   # macOS
   cp target/release/libgodot_neovim.dylib addons/godot-neovim/
   ```

4. Copy the addons folder to your Godot project:
   ```bash
   cp -r addons /path/to/your/godot/project/
   ```

5. Enable the plugin in Godot: `Project > Project Settings > Plugins`

## Configuration

### Neovim Executable Path

You can configure the Neovim executable path in Godot's Editor Settings:

1. Open `Editor > Editor Settings`
2. Navigate to `Godot Neovim` section
3. Set `Neovim Executable Path` to your Neovim installation path

**Default values:**
- Windows: `nvim.exe`
- macOS/Linux: `nvim`

The plugin validates the path on startup and whenever settings change. Check the Output panel for validation results.

### Input Mode

You can choose how insert mode input is handled:

1. Open `Editor > Editor Settings`
2. Navigate to `Godot Neovim` section
3. Set `Input Mode` to your preferred mode

| Mode | Description | IME Support |
|------|-------------|-------------|
| `hybrid` (default) | Insert mode uses Godot's native input | ✅ Yes |
| `strict` | Insert mode also handled by Neovim | ❌ No |

**Hybrid mode** is recommended for most users as it provides IME support for non-ASCII input and Godot's auto-completion features.

**Strict mode** provides a more authentic Neovim experience where all keystrokes are processed by Neovim, but IME input is not supported.

## Usage

Once the plugin is enabled:

1. Open any script in Godot's script editor
2. The mode indicator will appear showing the current Neovim mode
3. Use Neovim keybindings to edit your code

### Mode Indicator Colors

| Mode    | Color  |
|---------|--------|
| NORMAL  | Green  |
| INSERT  | Blue   |
| VISUAL  | Orange |
| COMMAND | Yellow |
| REPLACE | Red    |

### Supported Commands

#### Navigation

| Command | Description |
|---------|-------------|
| `h`, `j`, `k`, `l` | Basic cursor movement |
| `w`, `b`, `e`, `ge` | Word movement |
| `0` | Go to start of line |
| `^` | Go to first non-blank character |
| `$` | Go to end of line |
| `gg` | Go to first line |
| `G` | Go to last line |
| `H` | Go to top of visible area |
| `M` | Go to middle of visible area |
| `L` | Go to bottom of visible area |
| `{` | Go to previous paragraph |
| `}` | Go to next paragraph |
| `%` | Jump to matching bracket |
| `Ctrl+F` | Full page down |
| `Ctrl+B` | Full page up |
| `Ctrl+D` | Half page down |
| `Ctrl+U` | Half page up |
| `Ctrl+Y` | Scroll viewport up (cursor stays) |
| `Ctrl+E` | Scroll viewport down (cursor stays) |
| `zz` | Center viewport on cursor |
| `zt` | Cursor line at top |
| `zb` | Cursor line at bottom |
| `gj` | Move down by display line (wrapped) |
| `gk` | Move up by display line (wrapped) |
| `[{` | Jump to previous unmatched `{` |
| `]}` | Jump to next unmatched `}` |
| `[(` | Jump to previous unmatched `(` |
| `])` | Jump to next unmatched `)` |
| `[m` | Jump to previous method start |
| `]m` | Jump to next method start |

#### Search

| Command | Description |
|---------|-------------|
| `/` | Open find dialog |
| `?` | Search backward (open find dialog) |
| `*` | Search forward for word under cursor |
| `#` | Search backward for word under cursor |
| `n` | Repeat last search (same direction) |
| `N` | Repeat last search (opposite direction) |
| `f{char}` | Find character forward on line |
| `F{char}` | Find character backward on line |
| `t{char}` | Move to before character forward |
| `T{char}` | Move to after character backward |
| `;` | Repeat last f/F/t/T (same direction) |
| `,` | Repeat last f/F/t/T (opposite direction) |
| `gd` | Go to definition |
| `gf` | Go to file under cursor |
| `gx` | Open URL/path under cursor in browser |
| `K` | Open Godot documentation for word under cursor |

#### Editing

| Command | Description |
|---------|-------------|
| `x` | Delete character under cursor |
| `X` | Delete character before cursor |
| `s` | Delete character and enter insert mode |
| `r{char}` | Replace character under cursor |
| `~` | Toggle case of character |
| `dd` | Delete line |
| `D` | Delete to end of line |
| `yy` | Yank (copy) line |
| `Y` | Yank to end of line |
| `cc`, `S` | Change entire line |
| `C` | Change to end of line |
| `p` | Paste after cursor |
| `P` | Paste before cursor |
| `gp` | Paste after and move cursor after text |
| `gP` | Paste before and move cursor after text |
| `[p` | Paste before with indent adjustment |
| `]p` | Paste after with indent adjustment |
| `J` | Join current line with next |
| `gJ` | Join lines without adding space |
| `>>` | Indent line |
| `<<` | Unindent line |
| `.` | Repeat last change |
| `gu{motion}` | Convert to lowercase |
| `gU{motion}` | Convert to uppercase |
| `g~{motion}` | Toggle case |
| `ga` | Display ASCII/Unicode of char under cursor |
| `gqq` | Format current line |

#### Text Objects

| Command | Description |
|---------|-------------|
| `iw`, `aw` | Inner/around word |
| `i"`, `a"` | Inner/around double quotes |
| `i'`, `a'` | Inner/around single quotes |
| `i(`, `a(` | Inner/around parentheses |
| `i[`, `a[` | Inner/around brackets |
| `i{`, `a{` | Inner/around braces |
| `i<`, `a<` | Inner/around angle brackets |
| `ie`, `ae` | Inner/around entire buffer |

#### Marks

| Command | Description |
|---------|-------------|
| `m{a-z}` | Set mark at current position |
| `'{a-z}` | Jump to mark line (first non-blank) |
| `` `{a-z} `` | Jump to exact mark position |

#### Macros

| Command | Description |
|---------|-------------|
| `q{a-z}` | Start recording macro to register |
| `q` | Stop recording macro (when recording) |
| `@{a-z}` | Play macro from register |
| `@@` | Replay last played macro |

#### Named Registers

| Command | Description |
|---------|-------------|
| `"{a-z}yy` | Yank line to named register |
| `"{a-z}dd` | Delete line to named register |
| `"{a-z}p` | Paste from named register (after) |
| `"{a-z}P` | Paste from named register (before) |
| `"+y`, `"*y` | Yank to system clipboard |
| `"+p`, `"*p` | Paste from system clipboard |
| `"_d` | Delete to black hole register (no save) |
| `"0p` | Paste from yank register |

#### Number Operations

| Command | Description |
|---------|-------------|
| `Ctrl+A` | Increment number under/after cursor |
| `Ctrl+X` | Decrement number under/after cursor |

#### Jump List

| Command | Description |
|---------|-------------|
| `Ctrl+O` | Jump back to previous position |
| `Ctrl+I` | Jump forward to newer position |

#### Mode Switching

| Command | Description |
|---------|-------------|
| `i`, `a`, `o`, `O` | Enter insert mode |
| `gi` | Insert at last insert position |
| `gI` | Insert at column 0 (ignore indent) |
| `R` | Enter replace mode (overwrite) |
| `v` | Enter visual mode |
| `V` | Enter visual line mode |
| `gv` | Enter visual block mode |
| `o` (visual) | Toggle selection direction |
| `Escape`, `Ctrl+[` | Return to normal mode |
| `:` | Enter command-line mode |

#### Folding

| Command | Description |
|---------|-------------|
| `za` | Toggle fold under cursor |
| `zo` | Open fold under cursor |
| `zc` | Close fold under cursor |
| `zM` | Close all folds |
| `zR` | Open all folds |

#### Command-Line Mode

| Command | Description |
|---------|-------------|
| `:e` | Open quick open dialog for scripts |
| `:e {file}` | Open specified script file |
| `:e!` | Discard changes and reload |
| `:w` | Save file |
| `:wa` | Save all open files |
| `:q` | Close current script tab |
| `:qa`, `:qall` | Close all script tabs |
| `:wq`, `:x` | Save and close |
| `:wqa` | Save all and close all |
| `ZZ` | Save and close (normal mode) |
| `ZQ` | Close without saving (discard changes) |
| `:%s/old/new/g` | Substitute all occurrences |
| `:g/{pattern}/d` | Delete lines matching pattern |
| `:sort` | Sort lines |
| `:t {line}` | Copy current line to after {line} |
| `:m {line}` | Move current line to after {line} |
| `:bn` | Next buffer (script tab) |
| `:bp` | Previous buffer (script tab) |
| `:bd` | Close current buffer |
| `:ls` | List open buffers |
| `g&` | Repeat last `:s` on entire file |
| `:{number}` | Jump to line number (e.g., `:123`) |
| `:marks` | Show all marks (output to console) |
| `:registers`, `:reg` | Show all registers (output to console) |
| `:jumps` | Show jump list (output to console) |
| `:changes` | Show change list (output to console) |
| `@:` | Repeat last Ex command |
| `Ctrl+G` | Show file info |
| `Up`/`Down` | Browse command history |

### Limitations

This plugin has architectural limitations due to using Godot's native CodeEdit for text editing.

#### Insert Mode (Hybrid Mode)

In hybrid mode (default), insert mode uses Godot's native input system to support IME and auto-completion. As a result, Vim's insert mode commands are **not available**:

| Not Supported | Description |
|---------------|-------------|
| `Ctrl+O` | Execute one normal mode command |
| `Ctrl+W` | Delete word backward |
| `Ctrl+U` | Delete to start of line |
| `Ctrl+R` | Insert from register |
| `Ctrl+A` | Insert previously inserted text |
| `Ctrl+N/P` | Keyword completion (use Godot's auto-completion instead) |

#### Not Implemented

| Feature | Description |
|---------|-------------|
| Neovim search | `/` opens Godot's find dialog |
| Neovim undo | Uses Godot's undo system |
| Neovim config | `init.lua` and plugins are not loaded |

### Roadmap

#### Implementation Candidates

Features requiring plugin-side implementation:

| Priority | Feature | Commands | Difficulty | Description |
|----------|---------|----------|------------|-------------|
| High | Confirm substitute | `:%s/old/new/gc` | ⭐⭐⭐ Hard | Confirm each replacement (requires UI) |
| Medium | Auto-indent | `=`, `==`, `=G` | ⭐⭐⭐ Hard | Requires GDScript syntax parsing (keyword analysis for `func`, `if`, `for`, etc.) |
| Medium | Change list | `g;`, `g,` | ⭐⭐ Medium | Navigate through change positions |
| Medium | Sequential increment | `gCtrl+A`, `gCtrl+X` | ⭐⭐ Medium | Generate number sequence in visual block |
| Medium | Indent text object | `ii`, `ai` | ⭐⭐ Medium | Select by indentation level |
| Medium | Argument text object | `ia`, `aa` | ⭐⭐⭐ Hard | Select function argument (requires parsing) |
| Low | Visual block insert | `I`/`A` (v-block) | ⭐⭐⭐ Hard | Insert/append on multiple lines |

#### Likely Already Working (Testing Needed)

These features may already work through Neovim backend:

| Category | Commands |
|----------|----------|
| Motions | `g0`, `g$`, `(`, `)`, `[[`, `]]` |
| Text Objects | `is`, `as` (sentence), `ip`, `ap` (paragraph), `it`, `at` (tag) |
| Line Range | `:1,10d`, `:.,$s/old/new/g` |

## Architecture

```
┌─────────────────────┐     RPC (msgpack)     ┌─────────────┐
│   Godot Editor      │◄────────────────────►│   Neovim    │
│   (GDExtension)     │                       │  (--embed)  │
│                     │                       │             │
│ ┌─────────────────┐ │                       │             │
│ │  CodeEdit       │ │  Buffer Sync          │             │
│ │  (Script Editor)│◄├──────────────────────►│   Buffer    │
│ └─────────────────┘ │                       │             │
│                     │                       │             │
│ ┌─────────────────┐ │  Mode Changes         │             │
│ │  Mode Label     │◄├──────────────────────►│   Mode      │
│ └─────────────────┘ │                       │             │
└─────────────────────┘                       └─────────────┘
```

## Dependencies

- [godot-rust/gdext](https://github.com/godot-rust/gdext) - Rust bindings for Godot 4
- [nvim-rs](https://github.com/KillTheMule/nvim-rs) - Neovim msgpack-RPC client for Rust
- [tokio](https://tokio.rs/) - Async runtime for Rust

## Related Projects

- [godot-vim](https://github.com/igorgue/godot-vim) - Vim keybinding emulator for Godot (GDScript)
- [vscode-neovim](https://github.com/vscode-neovim/vscode-neovim) - VSCode extension with Neovim backend

### Comparison with godot-vim

| Category | Feature | godot-neovim | godot-vim |
|----------|---------|:------------:|:---------:|
| **Architecture** | Backend | Neovim (RPC) | GDScript |
| | Language | Rust (GDExtension) | GDScript |
| | IME Support | Yes (hybrid mode) | No |
| **Modes** | Normal, Insert, Visual, V-Line | Yes | Yes |
| | Visual Block | Yes (`gv`) | No |
| | Replace | Yes (`R`) | No |
| | Command-line | Yes (`:` commands) | No |
| **Navigation** | Basic (hjkl, w, b, e, gg, G) | Yes | Yes |
| | Paragraph (`{`, `}`) | Yes | No |
| | Display lines (`gj`, `gk`) | Yes | No |
| | Block jump (`[{`, `]}`) | Yes | No |
| | Method jump (`[m`, `]m`) | Yes | No |
| **Scrolling** | Ctrl+F/B/D/U | Yes | Yes |
| | Ctrl+Y/E, zz/zt/zb | Yes | No |
| **Search** | `/`, `?` | Yes | `/` only |
| | `gd` (go to definition) | Yes | No |
| | `gx` (open URL) | Yes | No |
| | `K` (documentation) | Yes | No |
| **Editing** | Basic (x, dd, yy, p, J, etc.) | Yes | Yes |
| | `gp`, `gP`, `[p`, `]p` | Yes | No |
| | `Ctrl+A`/`Ctrl+X` (numbers) | Yes | No |
| | `ga`, `gqq` | Yes | No |
| **Text Objects** | Words, quotes, brackets | Yes | Yes |
| | Entire buffer (`ie`, `ae`) | Yes | No |
| **Registers** | Named (`"{a-z}`) | Yes | No |
| | Clipboard (`"+`, `"*`) | Yes | No |
| | Black hole (`"_`), Yank (`"0`) | Yes | No |
| **Marks** | `m{a-z}`, `'{a-z}` | Yes | Yes |
| | Exact position (`` `{a-z} ``) | Yes | No |
| **Macros** | `q{a-z}`, `@{a-z}`, `@@` | Yes | Yes |
| **Folding** | `za`, `zo`, `zc`, `zM`, `zR` | Yes | `za`, `zM`, `zR` |
| **Ex Commands** | `:w`, `:q`, `:wq`, `:x` | Yes | No |
| | `:e`, `:e!`, `:wa`, `:qa` | Yes | No |
| | `:%s/old/new/g` | Yes | No |
| | `:g/{pattern}/d` | Yes | No |
| | `:sort`, `:t`, `:m` | Yes | No |
| | `:bn`, `:bp`, `:bd`, `:ls` | Yes | No |
| | `ZZ`, `ZQ`, `@:`, `Ctrl+G` | Yes | No |

**Summary:**
- **godot-neovim**: Full Neovim backend with Ex commands, registers, IME support. Requires Neovim installation.
- **godot-vim**: Pure GDScript, easy setup, basic Vim keybindings. No external dependencies.

## License

Apache License 2.0 - See [LICENSE](LICENSE) for details.

## Contributing

Contributions are welcome! Please feel free to submit issues and pull requests.
