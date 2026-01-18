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
        vim.bo[bufnr].buftype = ''
        vim.bo[bufnr].swapfile = false
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
    vim.api.nvim_create_autocmd({'CursorMoved', 'CursorMovedI'}, {
        group = augroup,
        callback = function()
            local cursor = vim.api.nvim_win_get_cursor(0)  -- {row, col}, row is 1-indexed, col is 0-indexed byte position
            local mode = vim.api.nvim_get_mode().mode
            -- Send notification with actual cursor position
            vim.rpcnotify(0, "godot_cursor_moved", cursor[1], cursor[2], mode)
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

    -- Create user commands for debugging
    vim.api.nvim_create_user_command('GodotNeovimInfo', function()
        print('godot_neovim Lua plugin loaded')
        print('Buffer: ' .. vim.api.nvim_get_current_buf())
        print('Changedtick: ' .. M.get_changedtick(0))
    end, {})
end

-- Auto-setup on require
M.setup()

return M
