-- godot_neovim: Neovim plugin for godot-neovim integration
-- This module provides buffer management functions called from Rust

local M = {}

-- Register a buffer with initial content (clears undo history)
-- @param bufnr number: Buffer number (0 for current buffer)
-- @param lines table: Array of lines to set
-- @return number: changedtick after registration
function M.buffer_register(bufnr, lines)
    -- Use current buffer if bufnr is 0
    if bufnr == 0 then
        bufnr = vim.api.nvim_get_current_buf()
    end

    -- Save current undolevels
    local saved_ul = vim.bo[bufnr].undolevels

    -- Disable undo for this operation
    vim.bo[bufnr].undolevels = -1

    -- Set buffer content
    vim.api.nvim_buf_set_lines(bufnr, 0, -1, false, lines)

    -- Restore undolevels
    vim.bo[bufnr].undolevels = saved_ul

    -- Clear modified flag (this is initial content)
    vim.bo[bufnr].modified = false

    return vim.api.nvim_buf_get_changedtick(bufnr)
end

-- Register a buffer and attach for notifications atomically
-- This prevents race conditions between buffer_register and buf_attach
-- @param bufnr number: Buffer number (0 for current buffer)
-- @param lines table: Array of lines to set
-- @return table: { tick = changedtick, attached = boolean }
function M.buffer_register_and_attach(bufnr, lines)
    -- Use current buffer if bufnr is 0
    if bufnr == 0 then
        bufnr = vim.api.nvim_get_current_buf()
    end

    -- Save current undolevels
    local saved_ul = vim.bo[bufnr].undolevels

    -- Disable undo for this operation
    vim.bo[bufnr].undolevels = -1

    -- Set buffer content
    vim.api.nvim_buf_set_lines(bufnr, 0, -1, false, lines)

    -- Restore undolevels
    vim.bo[bufnr].undolevels = saved_ul

    -- Clear modified flag (this is initial content)
    vim.bo[bufnr].modified = false

    -- Get changedtick before attach
    local tick = vim.api.nvim_buf_get_changedtick(bufnr)

    -- Attach to buffer with send_buffer=false (we only want future notifications)
    local attached = vim.api.nvim_buf_attach(bufnr, false, {})

    return { tick = tick, attached = attached }
end

-- Update buffer content (preserves undo history)
-- @param bufnr number: Buffer number (0 for current buffer)
-- @param lines table: Array of lines to set
-- @return number: changedtick after update
function M.buffer_update(bufnr, lines)
    -- Use current buffer if bufnr is 0
    if bufnr == 0 then
        bufnr = vim.api.nvim_get_current_buf()
    end

    -- Set buffer content (this will be recorded in undo history)
    vim.api.nvim_buf_set_lines(bufnr, 0, -1, false, lines)

    return vim.api.nvim_buf_get_changedtick(bufnr)
end

-- Get current changedtick
-- @param bufnr number: Buffer number (0 for current buffer)
-- @return number: Current changedtick
function M.get_changedtick(bufnr)
    if bufnr == 0 then
        bufnr = vim.api.nvim_get_current_buf()
    end
    return vim.api.nvim_buf_get_changedtick(bufnr)
end

-- Track which buffers have been initialized by godot-neovim
M._initialized_buffers = {}

-- Track which buffers have been attached for notifications
M._attached_buffers = {}

-- Track last cursor position and mode for throttling RPC notifications
M._last_cursor = { 0, 0 }
M._last_mode = ""

-- Switch to buffer by path, creating and initializing if needed
-- @param path string: Absolute file path
-- @param lines table|nil: Lines to initialize with (only used for new buffers)
-- @return table: { bufnr, tick, is_new, cursor }
function M.switch_to_buffer(path, lines)
    -- Find existing buffer by path
    local bufnr = vim.fn.bufnr(path)
    local is_new = (bufnr == -1)

    if is_new then
        -- Create new buffer
        bufnr = vim.api.nvim_create_buf(true, false)  -- listed, not scratch
        vim.api.nvim_buf_set_name(bufnr, path)

        -- Set buffer options for code editing
        -- buftype=acwrite: like nofile but triggers BufWriteCmd for :w
        -- This allows us to intercept save commands and delegate to Godot
        vim.bo[bufnr].buftype = 'acwrite'
        vim.bo[bufnr].swapfile = false

        -- Setup BufWriteCmd autocmd for this buffer
        M._setup_buffer_autocmds(bufnr)
    end

    -- Switch to the buffer
    vim.api.nvim_set_current_buf(bufnr)

    -- Initialize content only for new/uninitialized buffers
    -- Don't re-init existing buffers - it would reset undo history
    -- External file changes should be handled via :e! command
    local should_init = false
    if lines then
        if not M._initialized_buffers[bufnr] then
            should_init = true
        end
        -- Note: Removed line count check that was causing undo history reset
        -- on buffer switch. Existing buffers keep their Neovim state.
    end

    if should_init and lines then
        -- Save current undolevels
        local saved_ul = vim.bo[bufnr].undolevels

        -- Disable undo for initial content
        vim.bo[bufnr].undolevels = -1

        -- Set buffer content
        vim.api.nvim_buf_set_lines(bufnr, 0, -1, false, lines)

        -- Restore undolevels
        vim.bo[bufnr].undolevels = saved_ul

        -- Clear modified flag
        vim.bo[bufnr].modified = false

        -- Mark as initialized
        M._initialized_buffers[bufnr] = true
    end

    -- Attach for notifications if not already attached
    local attached = false
    if not M._attached_buffers[bufnr] then
        attached = vim.api.nvim_buf_attach(bufnr, false, {
            on_lines = function(_, buf, tick, first_line, last_line, last_line_updated, byte_count)
                -- Get the new lines content
                local new_lines = vim.api.nvim_buf_get_lines(buf, first_line, last_line_updated, false)
                -- Send RPC notification with change details
                vim.rpcnotify(0, "godot_buf_lines", buf, tick, first_line, last_line, new_lines)
                return false  -- Continue receiving notifications
            end,
            on_detach = function()
                M._attached_buffers[bufnr] = nil
                M._initialized_buffers[bufnr] = nil
            end
        })
        if attached then
            M._attached_buffers[bufnr] = true
        end
    else
        attached = true
    end

    -- Get current state
    local tick = vim.api.nvim_buf_get_changedtick(bufnr)
    local cursor = vim.api.nvim_win_get_cursor(0)  -- {row, col}, 1-indexed row

    return {
        bufnr = bufnr,
        tick = tick,
        is_new = is_new,
        attached = attached,
        cursor = cursor
    }
end

-- Get buffer info without switching
-- @param path string: File path
-- @return table|nil: { bufnr, initialized, attached } or nil if not exists
function M.get_buffer_info(path)
    local bufnr = vim.fn.bufnr(path)
    if bufnr == -1 then
        return nil
    end
    return {
        bufnr = bufnr,
        initialized = M._initialized_buffers[bufnr] or false,
        attached = M._attached_buffers[bufnr] or false
    }
end

-- Send keys (async - keys are processed after RPC returns)
-- @param keys string: Keys to send (Neovim notation like "<Space>", "j", etc.)
-- @return table: { sent = true }
function M.send_keys(keys)
    -- Just queue the keys - they'll be processed by event loop after RPC returns
    vim.api.nvim_input(keys)
    return { sent = true }
end

-- Get current mode and cursor (for polling)
-- @return table: { mode, line, col }
function M.get_state()
    local mode_info = vim.api.nvim_get_mode()
    local cursor = vim.api.nvim_win_get_cursor(0)
    return {
        mode = mode_info.mode,
        line = cursor[1],
        col = cursor[2],
        blocking = mode_info.blocking
    }
end

-- Setup function (called on plugin load)
function M.setup()
    -- Register global functions for RPC access
    _G.godot_neovim = M

    -- Create autocmd group for godot-neovim
    local augroup = vim.api.nvim_create_augroup('godot_neovim', { clear = true })

    -- Send cursor position on cursor movement
    -- This sends actual byte position (not screen position like grid_cursor_goto)
    -- Throttled: only send notification when cursor or mode actually changed
    vim.api.nvim_create_autocmd({'CursorMoved', 'CursorMovedI'}, {
        group = augroup,
        callback = function()
            local cursor = vim.api.nvim_win_get_cursor(0)  -- {row, col}, row is 1-indexed, col is 0-indexed byte position
            local mode = vim.api.nvim_get_mode().mode

            -- Only send notification if cursor or mode changed (throttling)
            if cursor[1] ~= M._last_cursor[1] or cursor[2] ~= M._last_cursor[2] or mode ~= M._last_mode then
                M._last_cursor = cursor
                M._last_mode = mode
                vim.rpcnotify(0, "godot_cursor_moved", cursor[1], cursor[2], mode)
            end
        end
    })

    -- Send modified flag changes (for undo/redo dirty flag sync)
    -- This fires when buffer's modified flag changes (true->false or false->true)
    vim.api.nvim_create_autocmd('BufModifiedSet', {
        group = augroup,
        callback = function()
            local bufnr = vim.api.nvim_get_current_buf()
            local modified = vim.bo[bufnr].modified
            vim.rpcnotify(0, "godot_modified_changed", bufnr, modified)
        end
    })

    -- Send buffer enter notification (for Ctrl+O/Ctrl+I cross-buffer jumps)
    -- This fires when entering a buffer, allowing Godot to sync script tabs
    vim.api.nvim_create_autocmd('BufEnter', {
        group = augroup,
        callback = function()
            local bufnr = vim.api.nvim_get_current_buf()
            local path = vim.api.nvim_buf_get_name(bufnr)
            -- Only notify for initialized buffers (managed by godot-neovim)
            if M._initialized_buffers[bufnr] and path ~= '' then
                vim.rpcnotify(0, "godot_buf_enter", bufnr, path)
            end
        end
    })

    -- Create user commands for debugging
    vim.api.nvim_create_user_command('GodotNeovimInfo', function()
        print('godot_neovim Lua plugin loaded')
        print('Buffer: ' .. vim.api.nvim_get_current_buf())
        print('Changedtick: ' .. M.get_changedtick(0))
    end, {})

    -- Override :q, :qa, :wq, etc. to delegate to Godot
    -- This ensures Godot handles the close/save dialogs instead of Neovim
    M._setup_file_commands()
end

-- Setup file commands (:q, :wq, etc.) to delegate to Godot
-- Similar to vscode-neovim's vscode-file-commands.vim
function M._setup_file_commands()
    -- :q - Close current tab
    vim.api.nvim_create_user_command('Quit', function(opts)
        vim.rpcnotify(0, "godot_close_buffer", {
            bang = opts.bang,
            all = false,
        })
    end, { bang = true })

    -- :qa - Close all tabs
    vim.api.nvim_create_user_command('Qall', function(opts)
        vim.rpcnotify(0, "godot_close_buffer", {
            bang = opts.bang,
            all = true,
        })
    end, { bang = true })

    -- :wq - Save and close
    vim.api.nvim_create_user_command('Wq', function()
        local bufnr = vim.api.nvim_get_current_buf()
        vim.rpcnotify(0, "godot_save_and_close")
        vim.bo[bufnr].modified = false
    end, { bang = true })

    -- :wqa - Save all and close all
    vim.api.nvim_create_user_command('Wqall', function()
        vim.rpcnotify(0, "godot_save_all_and_close")
    end, { bang = true })

    -- Alias commands using cabbrev (like vscode-neovim's AlterCommand)
    -- This allows :q to work as :Quit
    vim.cmd([[
        cnoreabbrev <expr> q (getcmdtype() == ':' && getcmdline() ==# 'q') ? 'Quit' : 'q'
        cnoreabbrev <expr> q! (getcmdtype() == ':' && getcmdline() ==# 'q!') ? 'Quit!' : 'q!'
        cnoreabbrev <expr> qa (getcmdtype() == ':' && getcmdline() ==# 'qa') ? 'Qall' : 'qa'
        cnoreabbrev <expr> qa! (getcmdtype() == ':' && getcmdline() ==# 'qa!') ? 'Qall!' : 'qa!'
        cnoreabbrev <expr> qall (getcmdtype() == ':' && getcmdline() ==# 'qall') ? 'Qall' : 'qall'
        cnoreabbrev <expr> wq (getcmdtype() == ':' && getcmdline() ==# 'wq') ? 'Wq' : 'wq'
        cnoreabbrev <expr> wq! (getcmdtype() == ':' && getcmdline() ==# 'wq!') ? 'Wq!' : 'wq!'
        cnoreabbrev <expr> wqa (getcmdtype() == ':' && getcmdline() ==# 'wqa') ? 'Wqall' : 'wqa'
        cnoreabbrev <expr> wqall (getcmdtype() == ':' && getcmdline() ==# 'wqall') ? 'Wqall' : 'wqall'
        cnoreabbrev <expr> x (getcmdtype() == ':' && getcmdline() ==# 'x') ? 'Wq' : 'x'
        cnoreabbrev <expr> xa (getcmdtype() == ':' && getcmdline() ==# 'xa') ? 'Wqall' : 'xa'
        cnoreabbrev <expr> xall (getcmdtype() == ':' && getcmdline() ==# 'xall') ? 'Wqall' : 'xall'
    ]])

    -- ZZ and ZQ mappings
    vim.keymap.set('n', 'ZZ', '<Cmd>Wq<CR>', { silent = true })
    vim.keymap.set('n', 'ZQ', '<Cmd>Quit!<CR>', { silent = true })
end

-- Setup buffer-local autocmds for BufWriteCmd (vscode-neovim style)
-- This intercepts :w and delegates to Godot
-- @param bufnr number: Buffer number
function M._setup_buffer_autocmds(bufnr)
    local augroup = vim.api.nvim_create_augroup('godot_neovim_buf_' .. bufnr, { clear = true })

    -- Intercept :w, :w!
    vim.api.nvim_create_autocmd('BufWriteCmd', {
        group = augroup,
        buffer = bufnr,
        callback = function(ev)
            -- Send save request to Godot via RPC
            vim.rpcnotify(0, "godot_save_buffer")

            -- Mark buffer as not modified (Godot will handle actual save)
            -- This prevents "No write since last change" warnings
            vim.bo[ev.buf].modified = false
        end,
    })
end

-- Reload current buffer from disk (:e!) and re-attach for notifications
-- Returns the new buffer content and cursor position to sync to Godot
-- @return table: { lines = buffer lines, tick = changedtick, cursor = {row, col} }
function M.reload_buffer()
    local bufnr = vim.api.nvim_get_current_buf()

    -- Execute :e! to reload from disk
    vim.cmd('e!')

    -- Re-attach for notifications (e! causes detach)
    M._attached_buffers[bufnr] = nil  -- Clear old attachment flag
    local attached = vim.api.nvim_buf_attach(bufnr, false, {
        on_lines = function(_, buf, tick, first_line, last_line, last_line_updated, byte_count)
            local new_lines = vim.api.nvim_buf_get_lines(buf, first_line, last_line_updated, false)
            vim.rpcnotify(0, "godot_buf_lines", buf, tick, first_line, last_line, new_lines)
            return false
        end,
        on_detach = function()
            M._attached_buffers[bufnr] = nil
            M._initialized_buffers[bufnr] = nil
        end
    })
    if attached then
        M._attached_buffers[bufnr] = true
    end

    -- Get current buffer content and cursor position
    local lines = vim.api.nvim_buf_get_lines(bufnr, 0, -1, false)
    local tick = vim.api.nvim_buf_get_changedtick(bufnr)
    local cursor = vim.api.nvim_win_get_cursor(0)  -- {row, col}, row is 1-indexed

    return {
        lines = lines,
        tick = tick,
        attached = attached,
        cursor = cursor
    }
end

-- Join lines without space (gJ) while preserving comment leaders
-- Temporarily clears 'comments' option to prevent comment leader removal
function M.join_no_space()
    local saved_comments = vim.bo.comments
    vim.bo.comments = ''
    vim.cmd('normal! gJ')
    vim.bo.comments = saved_comments
end

-- Set visual selection atomically (for mouse drag selection sync)
-- This ensures cursor movement and visual mode entry happen in correct order
-- @param from_line number: Selection start line (1-indexed)
-- @param from_col number: Selection start column (0-indexed)
-- @param to_line number: Selection end line (1-indexed)
-- @param to_col number: Selection end column (0-indexed)
-- @return table: { mode = current mode after selection }
function M.set_visual_selection(from_line, from_col, to_line, to_col)
    -- Exit any existing visual mode first
    local mode = vim.api.nvim_get_mode().mode
    if mode:match('^[vV\x16]') then
        vim.api.nvim_feedkeys(vim.api.nvim_replace_termcodes('<Esc>', true, false, true), 'nx', false)
    end

    -- Move cursor to selection start
    vim.api.nvim_win_set_cursor(0, {from_line, from_col})

    -- Enter visual mode
    vim.cmd('normal! v')

    -- Move cursor to selection end
    vim.api.nvim_win_set_cursor(0, {to_line, to_col})

    return { mode = vim.api.nvim_get_mode().mode }
end

-- Auto-setup on require
M.setup()

return M
