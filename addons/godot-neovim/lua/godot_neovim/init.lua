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

-- Setup function (called on plugin load)
function M.setup()
    -- Register global functions for RPC access
    _G.godot_neovim = M

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
