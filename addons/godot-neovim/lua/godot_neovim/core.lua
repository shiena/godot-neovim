-- godot_neovim/core.lua: State management and basic utilities

local M = {}

-- Track which buffers have been initialized by godot-neovim
M._initialized_buffers = {}

-- Track which buffers have been attached for notifications
M._attached_buffers = {}

-- Track last cursor position and mode for throttling RPC notifications
M._last_cursor = { 0, 0 }
M._last_mode = ""

-- Get current changedtick
-- @param bufnr number: Buffer number (0 for current buffer)
-- @return number: Current changedtick
function M.get_changedtick(bufnr)
    if bufnr == 0 then
        bufnr = vim.api.nvim_get_current_buf()
    end
    return vim.api.nvim_buf_get_changedtick(bufnr)
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

-- Convert character column to byte column for a given line
-- Godot uses character positions, Neovim uses byte positions
-- For multi-byte characters (e.g., Japanese), this conversion is essential
-- @param line string: The line content
-- @param char_col number: Character column (0-indexed)
-- @return number: Byte column (0-indexed)
function M.char_col_to_byte_col(line, char_col)
    if not line or char_col <= 0 then
        return 0
    end
    -- Use vim.fn.byteidx to convert character index to byte index
    -- byteidx returns the byte index of the {char_col}-th character (0-indexed)
    local byte_col = vim.fn.byteidx(line, char_col)
    -- byteidx returns -1 if char_col is out of range
    if byte_col < 0 then
        return #line  -- Return end of line
    end
    return byte_col
end

return M
