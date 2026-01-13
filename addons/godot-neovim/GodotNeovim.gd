## Neovim-powered Vim emulation for Godot's script editor.
##
## GodotNeovim provides Vim keybindings in Godot's script editor
## using an embedded Neovim instance as the backend.
##
## @tutorial(GitHub): https://github.com/shiena/godot-neovim
##
## [br][br][b]Modes[/b][br]
## [code]Normal[/code] - Default mode for navigation and commands[br]
## [code]Insert[/code] - Text input mode (i, a, o, etc.)[br]
## [code]Visual[/code] - Character selection (v)[br]
## [code]Visual Line[/code] - Line selection (V)[br]
## [code]Replace[/code] - Overwrite mode (R)[br]
##
## [br][b]Basic Motions[/b][br]
## [code]h j k l[/code] - Left, Down, Up, Right[br]
## [code]w b e[/code] - Word forward, Word backward, End of word[br]
## [code]W B E[/code] - WORD (whitespace-delimited)[br]
## [code]0 $ ^[/code] - Line start, Line end, First non-blank[br]
## [code]gg G[/code] - File start, File end[br]
## [code]{count}G[/code] - Go to line number[br]
## [code]%[/code] - Matching bracket[br]
##
## [br][b]Find Motions[/b][br]
## [code]f{char}[/code] - Find char forward[br]
## [code]F{char}[/code] - Find char backward[br]
## [code]t{char}[/code] - Till char forward[br]
## [code]T{char}[/code] - Till char backward[br]
## [code];[/code] - Repeat last find[br]
## [code],[/code] - Repeat last find (reverse)[br]
##
## [br][b]Operators[/b][br]
## [code]d{motion}[/code] - Delete[br]
## [code]c{motion}[/code] - Change (delete and enter insert)[br]
## [code]y{motion}[/code] - Yank (copy)[br]
## [code]>{motion}[/code] - Indent right[br]
## [code]<{motion}[/code] - Indent left[br]
## [code]={motion}[/code] - Auto-indent[br]
## [code]gq{motion}[/code] - Format[br]
##
## [br][b]Operator Shortcuts[/b][br]
## [code]dd cc yy[/code] - Operate on entire line[br]
## [code]D C[/code] - Operate to end of line[br]
## [code]x X[/code] - Delete char under/before cursor[br]
## [code]s S[/code] - Substitute char/line[br]
## [code]p P[/code] - Paste after/before[br]
## [code]J[/code] - Join lines[br]
##
## [br][b]Text Objects[/b][br]
## [code]iw aw[/code] - Inner/Around word[br]
## [code]iW aW[/code] - Inner/Around WORD[br]
## [code]i" a" i' a'[/code] - Inner/Around quotes[br]
## [code]i( a( i) a)[/code] - Inner/Around parentheses[br]
## [code]i{ a{ i} a}[/code] - Inner/Around braces[br]
## [code]i[ a[ i] a][/code] - Inner/Around brackets[br]
## [code]i< a< i> a>[/code] - Inner/Around angle brackets[br]
## [code]it at[/code] - Inner/Around XML tag[br]
##
## [br][b]Insert Mode Entry[/b][br]
## [code]i[/code] - Insert before cursor[br]
## [code]a[/code] - Append after cursor[br]
## [code]I[/code] - Insert at first non-blank[br]
## [code]A[/code] - Append at end of line[br]
## [code]o[/code] - Open line below[br]
## [code]O[/code] - Open line above[br]
## [code]gI[/code] - Insert at column 0[br]
## [code]gi[/code] - Insert at last insert position[br]
##
## [br][b]Visual Mode[/b][br]
## [code]v[/code] - Character-wise visual[br]
## [code]V[/code] - Line-wise visual[br]
## [code]gv[/code] - Reselect last visual[br]
##
## [br][b]Marks[/b][br]
## [code]m{a-z}[/code] - Set mark[br]
## [code]'{a-z}[/code] - Jump to mark (line)[br]
## [code]`{a-z}[/code] - Jump to mark (position)[br]
## [code]:marks[/code] - Show all marks[br]
##
## [br][b]Registers[/b][br]
## [code]"{a-z}{operator}[/code] - Use named register[br]
## [code]"+[/code] - System clipboard[br]
## [code]"*[/code] - Primary selection[br]
## [code]:registers[/code] - Show all registers[br]
##
## [br][b]Macros[/b][br]
## [code]q{a-z}[/code] - Start recording macro[br]
## [code]q[/code] - Stop recording[br]
## [code]@{a-z}[/code] - Play macro[br]
## [code]@@[/code] - Repeat last macro[br]
##
## [br][b]Search[/b][br]
## [code]/{pattern}[/code] - Search forward[br]
## [code]?{pattern}[/code] - Search backward[br]
## [code]n N[/code] - Next/Previous match[br]
## [code]*[/code] - Search word under cursor[br]
## [code]#[/code] - Search word backward[br]
##
## [br][b]Go Commands[/b][br]
## [code]gd[/code] - Go to definition (LSP)[br]
## [code]gf[/code] - Go to file under cursor[br]
## [code]gx[/code] - Open URL under cursor[br]
## [code]gt gT[/code] - Next/Previous tab[br]
##
## [br][b]Info Commands[/b][br]
## [code]K[/code] - Show hover info (LSP)[br]
## [code]ga[/code] - Show ASCII value[br]
## [code]Ctrl+G[/code] - Show file info[br]
##
## [br][b]Undo/Redo[/b][br]
## [code]u[/code] - Undo[br]
## [code]Ctrl+R[/code] - Redo[br]
## [code].[/code] - Repeat last change[br]
##
## [br][b]Ex Commands[/b][br]
## [code]:w[/code] - Save[br]
## [code]:q[/code] - Close[br]
## [code]:wq :x[/code] - Save and close[br]
## [code]:wa :qa :wqa[/code] - All buffers[br]
## [code]:e {file}[/code] - Open file[br]
## [code]:e![/code] - Reload from disk[br]
## [code]:bn :bp[/code] - Next/Previous buffer[br]
## [code]:{number}[/code] - Go to line[br]
##
## [br][b]Settings[/b][br]
## Configure via [code]Editor > Editor Settings > godot_neovim[/code]:[br]
## - [code]neovim_executable_path[/code] - Path to nvim[br]
## - [code]input_mode[/code] - Hybrid (IME) or Strict[br]
## - [code]neovim_clean[/code] - Start with --clean flag
class_name GodotNeovim
extends RefCounted
