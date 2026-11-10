# Comparison with GodotVim

[Back to README](../README.md)

| Category | Feature | godot-neovim | GodotVim |
|----------|---------|:------------:|:---------:|
| **Architecture** | Backend | Neovim (RPC) | GDExtension (vim-core) |
| | Language | Rust | Rust |
| | Auto-completion | ✅ | ✅ (`Ctrl+N/P/Space`) |
| **Modes** | Normal, Insert, Visual, V-Line | ✅ | ✅ |
| | Visual Block | ✅ (`Ctrl+V`, `gv`) | ✅ (with block I/A) |
| | Replace | ✅ (`R`) | ✅ |
| | Command-line | ✅ (`:` commands) | ✅ |
| | Select (`gh`) | ❌ | ✅ |
| **Navigation** | Basic (hjkl, w, b, e, gg, G) | ✅ | ✅ |
| | Paragraph/Sentence (`{`, `}`, `(`, `)`) | ✅ | ✅ |
| | Display lines (`gj`, `gk`, `g0`, `g$`, `g_`) | ✅ | ✅ |
| | Block jump (`[{`, `]}`, `[(`, `])`) | ✅ | ✅ |
| | Method jump (`[m`, `]m`) | ⚠️* | ❌ |
| | Indent jump (`[i`, `]i`) | ❌ | ✅ |
| | Change list (`g;`, `g,`) | ❌ | ✅ |
| **Scrolling** | Ctrl+F/B/D/U | ✅ | ✅ |
| | Ctrl+Y/E, zz/zt/zb, H/M/L | ✅ | ✅ |
| **Search** | `/`, `?`, `*`, `#`, `n`, `N` | ✅ | ✅ |
| | Incremental search highlighting | ❌ | ✅ |
| | `gd` (go to definition) | ✅ | ✅ |
| | `gx` (open URL) | ✅ | ❌ |
| | `K` (documentation/hover) | ✅ | ✅ |
| **Editing** | Basic (x, dd, yy, p, J, etc.) | ✅ | ✅ |
| | `gp`, `gP`, `[p`, `]p` | ✅ | ❌ |
| | `Ctrl+A`/`Ctrl+X` (numbers) | ✅ | ❌ |
| | `ga`, `gq`/`gw` | ✅ | ✅ |
| | Visual block insert (`I`/`A`) | ❌ | ✅ |
| | Multi-cursor | ❌ | ✅ (VS Code-style, `<C-S-Down>`, `:addcursor`) |
| **Insert Mode** | `Ctrl+W`/`Ctrl+U`/`Ctrl+H` (delete) | ✅ | ✅ |
| | `Ctrl+R` (insert from register) | ✅ | ✅ |
| | `Ctrl+A` (re-insert last text) | ✅ | ✅ |
| | `Ctrl+T`/`Ctrl+D` (indent) | ✅ | ✅ |
| | `Ctrl+O` (one normal command) | ✅ | ✅ |
| **Text Objects** | Words, quotes, brackets | ✅ | ✅ |
| | Sentence (`is`, `as`), Paragraph (`ip`, `ap`) | ✅ | ✅ |
| | Entire buffer (`ie`, `ae`) | ✅ | ✅ |
| | Indent (`ii`, `ai`) | ❌ | ✅ |
| **Registers** | Named (`"{a-z}`) | ✅ | ✅ |
| | Clipboard (`"+`, `"*`) | ✅ | ✅ |
| | Black hole (`"_`), Yank (`"0`) | ✅ | ✅ |
| | Numbered (`"1`-`"9`) | ✅ | ✅ |
| **Marks** | Local (`m{a-z}`, `'{a-z}`, `` `{a-z} ``) | ✅ | ✅ |
| | Global (`m{A-Z}`) | ❌ | ✅ |
| **Macros** | `q{a-z}`, `@{a-z}`, `@@` | ✅ | ✅ |
| **Folding** | `za`, `zo`, `zc`, `zM`, `zR` | ✅ | ✅ |
| **Ex Commands** | `:w`, `:q`, `:wq`, `:x` | ✅ | ✅ |
| | `:e`, `:e!`, `:wa`, `:qa` | ✅ | ✅ |
| | `:%s/old/new/g` | ✅ | ✅ |
| | `:g/{pattern}/d`, `:v` | ✅ | ✅ |
| | `:sort`, `:t`, `:m` | ✅ | ✅ |
| | `:bn`, `:bp`, `:bd`, `:ls` | ✅ | ✅ |
| | `ZZ`, `ZQ`, `@:`, `Ctrl+G` | ✅ | ❌ |
| | `:set` (options) | ❌ | ✅ |
| | `:earlier`, `:later` (undo tree) | ❌ | ✅ |
| | `:norm`, `:!` (shell) | ❌ | ✅ |
| **Godot Integration** | `:run`/`:stop` (scene control) | ❌ | ✅ |
| | Debugger (`:GodotBreakpoint`, step) | ❌ | ✅ |
| | Dock navigation (`Ctrl+H/J/K/L`) | ❌ | ✅ |
| | File explorer (FileSystem dock) | ❌ | ✅ (nvim-tree style) |
| | Editor actions (`:action`, `<Action>` mapping) | ❌ | ✅ |
| | Zen mode (`:zen`) | ❌ | ✅ |
| **Other** | Custom key mappings | ✅ (Neovim Keymaps panel) | ✅ (`.godot-vimrc`) |
| | Neovim config/plugins | ⚠️** | N/A |

\* `[m`/`]m` requires Neovim's treesitter or language-specific support. GDScript is not recognized by Neovim, so these commands may not work as expected.

\** Neovim config (`init.lua`) and plugins are not loaded by default (`neovim_clean = true`). Can be enabled but may cause compatibility issues.

**Summary:**
- **godot-neovim**: Real Neovim backend with full Ex command support and Godot auto-completion. Requires Neovim 0.9+ installation.
- **GodotVim**: Self-contained Vim engine (vim-core) with comprehensive Vim emulation (including multi-cursor and undo tree), Godot-specific commands (debugger, scene control, editor actions), and `.godot-vimrc` configuration. No external dependencies.
