# godot-neovim

Godot editor plugin that uses Neovim as the backend for script editing.

Inspired by [vscode-neovim](https://github.com/vscode-neovim/vscode-neovim).

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
- ✅ Command-line mode (`:w`, `:q`, `:%s/old/new/g`)
- ✅ Search word under cursor (`*` forward, `#` backward, `n`/`N` repeat)
- ✅ Character find (`f`/`F`/`t`/`T`, `;`/`,` repeat)
- ✅ Line navigation (`0`, `^`, `$`)
- ✅ Paragraph movement (`{`, `}`)
- ✅ Bracket matching (`%`)
- ✅ Character editing (`x`, `X`, `r`, `~`)
- ✅ Line operations (`J` join, `>>` indent, `<<` unindent)
- ✅ Full/half page scrolling (`Ctrl+F`, `Ctrl+B`, `Ctrl+D`, `Ctrl+U`)
- ✅ Go to definition (`gd`)

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

3. Copy the built library and configuration files to your Godot project:
   ```bash
   mkdir -p /path/to/your/godot/project/addons/godot-neovim/

   # Windows
   cp target/release/godot_neovim.dll /path/to/your/godot/project/addons/godot-neovim/

   # Linux
   cp target/release/libgodot_neovim.so /path/to/your/godot/project/addons/godot-neovim/

   # macOS
   cp target/release/libgodot_neovim.dylib /path/to/your/godot/project/addons/godot-neovim/

   # Copy configuration files
   cp godot-neovim.gdextension /path/to/your/godot/project/addons/godot-neovim/
   cp plugin.cfg /path/to/your/godot/project/addons/godot-neovim/
   ```

4. Enable the plugin in Godot: `Project > Project Settings > Plugins`

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
| `w`, `b`, `e` | Word movement |
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

#### Search

| Command | Description |
|---------|-------------|
| `/` | Open find dialog |
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

#### Editing

| Command | Description |
|---------|-------------|
| `x` | Delete character under cursor |
| `X` | Delete character before cursor |
| `r{char}` | Replace character under cursor |
| `~` | Toggle case of character |
| `dd` | Delete line |
| `yy` | Yank (copy) line |
| `p` | Paste |
| `J` | Join current line with next |
| `>>` | Indent line |
| `<<` | Unindent line |

#### Mode Switching

| Command | Description |
|---------|-------------|
| `i`, `a`, `o`, `O` | Enter insert mode |
| `v` | Enter visual mode |
| `V` | Enter visual line mode |
| `gv` | Enter visual block mode |
| `Escape`, `Ctrl+[` | Return to normal mode |
| `:` | Enter command-line mode |

#### Command-Line Mode

| Command | Description |
|---------|-------------|
| `:w` | Save file |
| `:q` | Close current script tab |
| `:qa`, `:qall` | Close all script tabs |
| `:wq`, `:x` | Save and close |
| `:%s/old/new/g` | Substitute all occurrences |

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
| Text objects | `ciw`, `da"`, `yi(`, etc. |
| Named registers | `"a`, `"b`, etc. (uses system clipboard only) |
| Macros | `q`, `@` |
| Marks | `m`, `'`, `` ` `` |
| Neovim search | `/` opens Godot's find dialog |
| Neovim undo | Uses Godot's undo system |
| Neovim config | `init.lua` and plugins are not loaded |

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

## License

Apache License 2.0 - See [LICENSE](LICENSE) for details.

## Contributing

Contributions are welcome! Please feel free to submit issues and pull requests.
