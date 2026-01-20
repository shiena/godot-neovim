<p align="center">
  <img src="addons/godot-neovim/icon.png" alt="godot-neovim logo" width="128" height="128">
</p>

<h1 align="center">godot-neovim</h1>

<p align="center">
  Godot editor plugin that uses Neovim as the backend for script editing.<br>
  Inspired by <a href="https://github.com/vscode-neovim/vscode-neovim">vscode-neovim</a> and <a href="https://github.com/hmdfrds/godot-vim">GodotVim</a>.
</p>

## Overview

godot-neovim integrates Neovim into Godot's script editor, allowing you to use the full power of Neovim for editing GDScript and other files within Godot. Unlike simple vim keybinding emulators, this plugin runs an actual Neovim process and synchronizes the buffer between Godot and Neovim.

## Features

- Real Neovim backend (not just keybinding emulation)
- Mode indicator with cursor position (e.g., `NORMAL 123:45`)
- Cursor synchronization between Godot and Neovim
- Mouse drag selection syncs to Neovim visual mode
- Support for count prefixes (e.g., `4j`, `10gg`)
- Support for operator-pending commands (e.g., `gg`, `dd`, `yy`)
- Ctrl+[ as Escape alternative (terminal standard)
- Full/half page scrolling (`Ctrl+F`, `Ctrl+B`, `Ctrl+D`, `Ctrl+U`)
- Word search under cursor (`*`, `#`, `n`, `N`)
- Character find motions (`f`, `F`, `t`, `T`, `;`, `,`)
- Line navigation (`0`, `^`, `$`) and paragraph movement (`{`, `}`)
- Bracket matching (`%`) and go to definition (`gd`)
- Character editing (`x`, `X`, `r`, `~`) and line operations (`J`, `>>`, `<<`)
- Configurable Neovim executable path via Editor Settings
- Path validation on startup and settings change

## Comparison with GodotVim

| Category | Feature | godot-neovim | GodotVim |
|----------|---------|:------------:|:---------:|
| **Architecture** | Backend | Neovim (RPC) | GDExtension |
| | Language | Rust | Rust |
| | Auto-completion | ✅ | ❌ |
| **Modes** | Normal, Insert, Visual, V-Line | ✅ | ✅ |
| | Visual Block | ✅ (`Ctrl+V`, `gv`) | ✅ |
| | Replace | ✅ (`R`) | ✅ |
| | Command-line | ✅ (`:` commands) | ✅ |
| **Navigation** | Basic (hjkl, w, b, e, gg, G) | ✅ | ✅ |
| | Paragraph (`{`, `}`) | ✅ | ✅ |
| | Display lines (`gj`, `gk`, `g0`, `g$`, `g_`) | ✅ | ❌ |
| | Block jump (`[{`, `]}`) | ✅ | ❌ |
| | Method jump (`[m`, `]m`) | ⚠️* | ❌ |
| **Scrolling** | Ctrl+F/B/D/U | ✅ | ✅ |
| | Ctrl+Y/E, zz/zt/zb | ✅ | ✅ |
| **Search** | `/`, `?` | ✅ | ✅ |
| | `gd` (go to definition) | ✅ | ❌ |
| | `gx` (open URL) | ✅ | ❌ |
| | `K` (documentation) | ✅ | ❌ |
| **Editing** | Basic (x, dd, yy, p, J, etc.) | ✅ | ✅ |
| | `gp`, `gP`, `[p`, `]p` | ✅ | ❌ |
| | `Ctrl+A`/`Ctrl+X` (numbers) | ✅ | ❌ |
| | `ga`, `gqq` | ✅ | ✅ |
| **Text Objects** | Words, quotes, brackets | ✅ | ✅ |
| | Entire buffer (`ie`, `ae`) | ✅ | ❌ |
| **Registers** | Named (`"{a-z}`) | ✅ | ❌ |
| | Clipboard (`"+`, `"*`) | ✅ | ✅ |
| | Black hole (`"_`), Yank (`"0`) | ✅ | ❌ |
| **Marks** | `m{a-z}`, `'{a-z}` | ✅ | ✅ |
| | Exact position (`` `{a-z} ``) | ✅ | ✅ |
| **Macros** | `q{a-z}`, `@{a-z}`, `@@` | ✅ | ✅ |
| **Folding** | `za`, `zo`, `zc`, `zM`, `zR` | ✅ | ❌ |
| **Ex Commands** | `:w`, `:q`, `:wq`, `:x` | ✅ | ✅ |
| | `:e`, `:e!`, `:wa`, `:qa` | ✅ | `:e` only |
| | `:%s/old/new/g` | ✅ | ✅ |
| | `:g/{pattern}/d` | ✅ | ❌ |
| | `:sort`, `:t`, `:m` | ✅ | ❌ |
| | `:bn`, `:bp`, `:bd`, `:ls` | ✅ | ❌ |
| | `ZZ`, `ZQ`, `@:`, `Ctrl+G` | ✅ | ❌ |
| **Other** | Custom key mappings | ❌ | ✅ |

\* `[m`/`]m` requires Neovim's treesitter or language-specific support. GDScript is not recognized by Neovim, so these commands may not work as expected.

**Summary:**
- **godot-neovim**: Full Neovim backend with all Ex commands, named registers, Godot auto-completion support. Requires Neovim 0.9+ installation.
- **GodotVim**: GDExtension-based emulator with custom key mapping support. No external dependencies.

## Requirements

- Godot 4.2 - 4.5
- Neovim 0.9.0 or later
- Rust toolchain (for building from source)

## Installation

### From Release (Recommended)

1. Download the latest release for your platform
2. Extract to your Godot project's `addons/godot-neovim/` directory
3. **macOS only**: Remove the quarantine attribute to avoid Gatekeeper blocking:
   ```bash
   xattr -rc addons/godot-neovim
   ```
4. Enable the plugin in `Project > Project Settings > Plugins`

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
   # Windows (x64)
   mkdir -p addons/godot-neovim/bin/windows
   cp target/release/godot_neovim.dll addons/godot-neovim/bin/windows/

   # Linux (x64)
   mkdir -p addons/godot-neovim/bin/linux
   cp target/release/libgodot_neovim.so addons/godot-neovim/bin/linux/

   # Linux (ARM64)
   mkdir -p addons/godot-neovim/bin/linux-arm64
   cp target/release/libgodot_neovim.so addons/godot-neovim/bin/linux-arm64/

   # macOS (Apple Silicon)
   mkdir -p addons/godot-neovim/bin/macos-arm64
   cp target/release/libgodot_neovim.dylib addons/godot-neovim/bin/macos-arm64/

   # macOS (Intel)
   mkdir -p addons/godot-neovim/bin/macos-x86_64
   cp target/release/libgodot_neovim.dylib addons/godot-neovim/bin/macos-x86_64/
   ```

4. Copy the addons folder to your Godot project:
   ```bash
   cp -r addons /path/to/your/godot/project/
   ```

5. Enable the plugin in Godot: `Project > Project Settings > Plugins`

## Configuration

All settings are available in `Editor > Editor Settings > Godot Neovim`.

> **Note**: You need to enable **"Advanced Settings"** toggle in the Editor Settings to see the `godot_neovim` section.

| Setting | Description | Default |
|---------|-------------|---------|
| Neovim Executable Path | Path to Neovim executable. The plugin validates this path on startup. | `nvim.exe` (Windows) / `nvim` (macOS/Linux) |
| Neovim Clean | Equivalent to the `--clean` startup option. When enabled, Neovim starts without loading any config files (init.lua, plugins, etc.). Recommended to keep enabled to avoid plugin compatibility issues. | true |
| Timeoutlen *(advanced)* | Time in milliseconds to wait for a mapped key sequence to complete. This setting appears when "Advanced Settings" is enabled in Editor Settings. | 1000 |

### Go to Definition (gd)

The `gd` command uses Godot's built-in LSP server for accurate navigation. To enable this feature:

1. Open `Editor > Editor Settings`
2. Navigate to `Network > Language Server`
3. Enable **Use Thread** option

When this setting is disabled, `gd` will show a message prompting you to enable it.

## Usage

Once the plugin is enabled:

1. Open any script in Godot's script editor
2. The mode indicator will appear showing the current Neovim mode
3. Use Neovim keybindings to edit your code

### In-Editor Help

- Press `F1` in Godot and search for **"GodotNeovim"** to view the command reference
- Or use `:help` (`:h`) in command-line mode to open the help directly

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
| `W`, `B`, `E` | WORD movement (whitespace-delimited) |
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
| `g0` | Go to start of display line |
| `g$` | Go to end of display line |
| `g^` | Go to first non-blank of display line |
| `g_` | Go to last non-blank character of line |
| `[{` | Jump to previous unmatched `{` |
| `]}` | Jump to next unmatched `}` |
| `[(` | Jump to previous unmatched `(` |
| `])` | Jump to next unmatched `)` |
| `[m` | Jump to previous method start |
| `]m` | Jump to next method start |

#### Search

| Command | Description |
|---------|-------------|
| `/` | Search forward |
| `?` | Search backward |
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
| `gd` | Go to definition (requires LSP, see below) |
| `gf` | Go to file under cursor |
| `gx` | Open URL/path under cursor in browser |
| `K` | Open Godot documentation (class, method, property, constant via LSP) |

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
| `I` | Insert at first non-blank character |
| `A` | Insert at end of line |
| `gi` | Insert at last insert position |
| `gI` | Insert at column 0 (ignore indent) |
| `R` | Enter replace mode (overwrite) |
| `v` | Enter visual mode |
| `V` | Enter visual line mode |
| `Ctrl+V`, `gv` | Enter visual block mode (`gv` as alternative since Godot intercepts Ctrl+V) |
| `Ctrl+B` (visual) | Switch to visual block mode |
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
| `:help`, `:h` | Open GodotNeovim help |
| `:version`, `:ver` | Show version in status label |
| `:e` | Open quick open dialog for scripts |
| `:e {file}` | Open specified script file |
| `:e!`, `:edit!` | Discard changes and reload |
| `:w` | Save file |
| `:wa`, `:wall` | Save all open files |
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
| `:jumps`, `:ju` | Show jump list (output to console) |
| `:changes` | Show change list (output to console) |
| `@:` | Repeat last Ex command |
| `Ctrl+G` | Show file info |
| `Up`/`Down` | Browse command history |

## Limitations

This plugin has architectural limitations due to using Godot's native CodeEdit for text editing.

### Insert Mode

Insert mode uses Godot's native input system to support auto-completion and other editor features. As a result, Vim's insert mode commands are **not available**:

| Not Supported | Description |
|---------------|-------------|
| `Ctrl+O` | Execute one normal mode command |
| `Ctrl+W` | Delete word backward |
| `Ctrl+U` | Delete to start of line |
| `Ctrl+R` | Insert from register |
| `Ctrl+A` | Insert previously inserted text |
| `Ctrl+N/P` | Keyword completion (use Godot's auto-completion instead) |

### Not Implemented

| Feature | Description |
|---------|-------------|
| Neovim undo | Uses Godot's undo system |
| Neovim config | `init.lua` and plugins are not loaded by default (`neovim_clean = true`). Can be enabled but may cause compatibility issues with some plugins (e.g., copilot.vim, lexima.vim). |
| `K` for signals | Signal documentation lookup not supported (class/method/property/constant only) |
| `[m`/`]m` for GDScript | Method jump commands require Neovim treesitter or language support. GDScript is not recognized by Neovim. |

### Known Issues

| Issue | Workaround |
|-------|------------|
| Dirty flag not set after some operations (named register paste, macro playback) | Switch to another script tab and back (`gt` then `gT`) |
| `(*)` marker remains after `:e!` reload | Switch tabs with `gt`/`gT` to clear the marker |

## Roadmap

### Implementation Candidates

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

### Verified Working Through Neovim Backend

These features work through the Neovim backend:

| Category | Commands |
|----------|----------|
| Motions | `(`, `)` (sentence), `[[`, `]]`, `[]`, `][` (section) |
| Text Objects | `is`, `as` (sentence), `ip`, `ap` (paragraph), `it`, `at` (tag) |
| Line Range | `:1,10d`, `:.,$s/old/new/g`, `:'<,'>d` |

## Architecture

```
┌─────────────────────┐     RPC (msgpack)     ┌─────────────┐
│   Godot Editor      │◄─────────────────────►│   Neovim    │
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
│                     │                       └─────────────┘
│ ┌─────────────────┐ │  TCP (JSON-RPC)       ┌─────────────┐
│ │  LSP Client     │◄├──────────────────────►│ Godot LSP   │
│ │  (gd command)   │ │                       │ (port 6005) │
│ └─────────────────┘ │                       │             │
└─────────────────────┘                       └─────────────┘
```

## Dependencies

- [godot-rust/gdext](https://github.com/godot-rust/gdext) - Rust bindings for Godot 4
- [nvim-rs](https://github.com/KillTheMule/nvim-rs) - Neovim msgpack-RPC client for Rust
- [tokio](https://tokio.rs/) - Async runtime for Rust
- [lsp-types](https://github.com/gluon-lang/lsp-types) - LSP protocol types for Godot LSP integration

## Related Projects

### Godot Vim Plugins
- [hmdfrds/godot-vim](https://github.com/hmdfrds/godot-vim) - GDExtension/Rust, text objects, custom key mappings
- [bernardo-bruning/godot-vim](https://github.com/bernardo-bruning/godot-vim) - GDScript, extensive Ex commands
- [joshnajera/godot-vim](https://github.com/joshnajera/godot-vim) - GDScript, operator-motion architecture
- [wovri-github/godot-vim-joshnajera](https://github.com/wovri-github/godot-vim-joshnajera) - GDScript, joshnajera fork with K (docs)

### Other
- [ComradeNeovim](https://github.com/beeender/ComradeNeovim) - IntelliJ/CLion plugin with Neovim backend
- [vscode-neovim](https://github.com/vscode-neovim/vscode-neovim) - VSCode extension with Neovim backend
- [neovide](https://github.com/neovide/neovide) - Neovim GUI client in Rust (architecture reference)

## License

Licensed under either of

* Apache License, Version 2.0 ([LICENSE-APACHE](LICENSE-APACHE) or http://www.apache.org/licenses/LICENSE-2.0)
* MIT license ([LICENSE-MIT](LICENSE-MIT) or http://opensource.org/licenses/MIT)

at your option.

### Contribution

Unless you explicitly state otherwise, any contribution intentionally submitted
for inclusion in the work by you, as defined in the Apache-2.0 license, shall be
dual licensed as above, without any additional terms or conditions.

## Contributing

Contributions are welcome! Please feel free to submit issues and pull requests.
