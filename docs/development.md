# Development guide

[Back to README](../README.md)

Build from source with a Rust toolchain. The copy commands use a Bash-compatible shell; choose only the library for your platform.

## Build from source

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
   cp -r addons/godot-neovim /path/to/your/godot/project/addons/
   ```

5. Enable the plugin in Godot: **Project > Project Settings > Plugins**

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

## Roadmap

### Implementation candidates

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

### Verified through the Neovim backend

These features work through the Neovim backend:

| Category | Commands |
|----------|----------|
| Motions | `(`, `)` (sentence), `[[`, `]]`, `[]`, `][` (section) |
| Text Objects | `is`, `as` (sentence), `ip`, `ap` (paragraph), `it`, `at` (tag) |
| Line Range | `:1,10d`, `:.,$s/old/new/g`, `:'<,'>d` |

## Related projects

### Godot Vim plugins
- [hmdfrds/godot-vim](https://github.com/hmdfrds/godot-vim) - GDExtension/Rust, text objects, custom key mappings
- [bernardo-bruning/godot-vim](https://github.com/bernardo-bruning/godot-vim) - GDScript, extensive Ex commands
- [joshnajera/godot-vim](https://github.com/joshnajera/godot-vim) - GDScript, operator-motion architecture
- [wovri-github/godot-vim-joshnajera](https://github.com/wovri-github/godot-vim-joshnajera) - GDScript, joshnajera fork with K (docs)

### Other
- [ComradeNeovim](https://github.com/beeender/ComradeNeovim) - IntelliJ/CLion plugin with Neovim backend
- [vscode-neovim](https://github.com/vscode-neovim/vscode-neovim) - VSCode extension with Neovim backend
- [neovide](https://github.com/neovide/neovide) - Neovim GUI client in Rust (architecture reference)
