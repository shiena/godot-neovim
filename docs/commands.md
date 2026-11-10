# Command reference

[Back to README](../README.md)

Use `:help` or press F1 and search for **GodotNeovim** for help inside the editor.

## Navigation

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

## Search

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
| `gd` | Go to definition (requires [LSP setup](configuration.md#enable-go-to-definition)) |
| `gf` | Go to file under cursor |
| `gx` | Open URL/path under cursor in browser |
| `K` | Open Godot documentation (class, method, property, constant via LSP) |

## Editing

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
| `Ctrl+/` | Toggle comment (uses Godot's native comment toggle) |
| `ga` | Display ASCII/Unicode of char under cursor |
| `gqq` | Format current line |

## Text objects

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

## Marks

| Command | Description |
|---------|-------------|
| `m{a-z}` | Set mark at current position |
| `'{a-z}` | Jump to mark line (first non-blank) |
| `` `{a-z} `` | Jump to exact mark position |

## Macros

| Command | Description |
|---------|-------------|
| `q{a-z}` | Start recording macro to register |
| `q` | Stop recording macro (when recording) |
| `@{a-z}` | Play macro from register |
| `@@` | Replay last played macro |

## Named registers

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

## Number operations

| Command | Description |
|---------|-------------|
| `Ctrl+A` | Increment number under/after cursor |
| `Ctrl+X` | Decrement number under/after cursor |

## Jump list

| Command | Description |
|---------|-------------|
| `Ctrl+O` | Jump back to previous position |
| `Ctrl+I` | Jump forward to newer position |

## Mode switching

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

## Folding

| Command | Description |
|---------|-------------|
| `za` | Toggle fold under cursor |
| `zo` | Open fold under cursor |
| `zc` | Close fold under cursor |
| `zM` | Close all folds |
| `zR` | Open all folds |

## Command-line mode

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

## Mode indicator colors

| Mode    | Color  |
|---------|--------|
| NORMAL  | Green  |
| INSERT  | Blue   |
| VISUAL  | Orange |
| COMMAND | Yellow |
| REPLACE | Red    |
