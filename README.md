# godot-neovim

Godot editor plugin that uses Neovim as the backend for script editing.

## Overview

godot-neovim integrates Neovim into Godot's script editor, allowing you to use the full power of Neovim for editing GDScript and other files within Godot. Unlike simple vim keybinding emulators, this plugin runs an actual Neovim process and synchronizes the buffer between Godot and Neovim.

## Features

- Real Neovim backend (not just keybinding emulation)
- Mode indicator with cursor position (e.g., `NORMAL 123:45`)
- Cursor synchronization between Godot and Neovim
- Support for count prefixes (e.g., `4j`, `10gg`)
- Support for operator-pending commands (e.g., `gg`, `dd`, `yy`)
- Configurable Neovim executable path via Editor Settings
- Path validation on startup and settings change

### Current Status

This plugin is in early development. The following features are implemented:

- ✅ Normal mode navigation (`h`, `j`, `k`, `l`, `gg`, `G`, `w`, `b`, etc.)
- ✅ Mode switching (`i`, `a`, `o`, `v`, etc.)
- ✅ Mode indicator display with line:column
- ✅ Cursor position synchronization (Neovim → Godot)
- ✅ Buffer synchronization (Godot → Neovim on script open)
- ✅ Operator-pending commands with timeout handling (`gg`, `dd`, `yy`, etc.)
- 🚧 Insert mode text input (keys forwarded to Neovim, no native editing)
- ⬜ Buffer synchronization (Neovim → Godot)
- ⬜ Visual mode selection display
- ⬜ Command-line mode

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
