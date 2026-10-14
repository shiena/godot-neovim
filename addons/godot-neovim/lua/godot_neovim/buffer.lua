-- godot_neovim/buffer.lua: Buffer operations

local core = require('godot_neovim.core')

local M = {}

-- Will be set by init.lua to avoid circular dependency
M._setup_buffer_autocmds = nil

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

-- Drop one trailing empty line (the implicit end-of-line).
-- Godot's text keeps the final newline as an empty last line, while Neovim
-- buffers may or may not have it depending on how they were synced
-- (switch_to_buffer strips it, the ESC-time buffer_update keeps it).
local function content_end(lines)
    local n = #lines
    if n > 1 and lines[n] == "" then
        return n - 1
    end
    return n
end

-- Apply an edit that Godot made on its own (signal connection, drag & drop,
-- Replace in Files, hot reload, ...) to a buffer that Neovim already owns.
-- Only the changed line range is replaced, so this is a single, minimal undo
-- step, and nothing is done when the content already matches (ignoring the
-- implicit trailing empty line).
-- @param bufnr number: Buffer number (0 for current buffer)
-- @param lines table: Godot's lines
-- @param opts table|nil: {
--     expected_tick = number|nil: abort if b:changedtick differs (Neovim has
--         changes Godot has not applied yet; pushing would revert them),
--     unmodified = bool|nil: clear 'modified' afterwards (Godot has saved it) }
-- @return table: { status = "unchanged"|"updated"|"stale", tick, line_count }
function M.sync_external_lines(bufnr, lines, opts)
    if bufnr == 0 then
        bufnr = vim.api.nvim_get_current_buf()
    end
    opts = opts or {}

    local function result(status)
        return {
            status = status,
            tick = vim.api.nvim_buf_get_changedtick(bufnr),
            line_count = vim.api.nvim_buf_line_count(bufnr),
        }
    end

    if opts.expected_tick and opts.expected_tick ~= vim.api.nvim_buf_get_changedtick(bufnr) then
        return result("stale")
    end

    local current = vim.api.nvim_buf_get_lines(bufnr, 0, -1, false)
    local cur_end = content_end(current)
    local new_end = content_end(lines)

    -- Common prefix / suffix within the content range
    local prefix = 0
    while prefix < cur_end and prefix < new_end and current[prefix + 1] == lines[prefix + 1] do
        prefix = prefix + 1
    end
    local suffix = 0
    while suffix < cur_end - prefix and suffix < new_end - prefix
        and current[cur_end - suffix] == lines[new_end - suffix] do
        suffix = suffix + 1
    end

    if prefix == cur_end and prefix == new_end then
        return result("unchanged")
    end

    local replacement = {}
    for i = prefix + 1, new_end - suffix do
        replacement[#replacement + 1] = lines[i]
    end
    vim.api.nvim_buf_set_lines(bufnr, prefix, cur_end - suffix, false, replacement)

    if opts.unmodified then
        vim.bo[bufnr].modified = false
    end

    return result("updated")
end

-- Set indent options for a buffer
-- @param bufnr number: Buffer number (0 for current buffer)
-- @param use_spaces boolean: Use spaces instead of tabs
-- @param indent_size number: Indent size (number of spaces or tab width)
function M.set_indent_options(bufnr, use_spaces, indent_size)
    if bufnr == 0 then
        bufnr = vim.api.nvim_get_current_buf()
    end

    vim.bo[bufnr].expandtab = use_spaces
    vim.bo[bufnr].shiftwidth = indent_size
    vim.bo[bufnr].tabstop = indent_size
    vim.bo[bufnr].softtabstop = indent_size
end

-- Switch to buffer by path, creating and initializing if needed
-- @param path string: Absolute file path
-- @param lines table|nil: Godot's lines. Initializes new buffers; for existing
--   buffers, edits Godot made while the buffer was not current are applied
-- @param indent_opts table|nil: { use_spaces = bool, indent_size = number }
-- @param unmodified boolean|nil: Godot has no unsaved changes for this file
-- @return table: { bufnr, tick, is_new, content_updated, line_count, attached, cursor }
function M.switch_to_buffer(path, lines, indent_opts, unmodified)
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
        if M._setup_buffer_autocmds then
            M._setup_buffer_autocmds(bufnr)
        end
    end

    -- Switch to the buffer
    vim.api.nvim_set_current_buf(bufnr)

    -- Initialize content only for new/uninitialized buffers
    -- Don't re-init existing buffers - it would reset undo history
    -- External file changes should be handled via :e! command
    local should_init = false
    local content_updated = false
    if lines then
        if not core._initialized_buffers[bufnr] then
            should_init = true
        else
            -- Existing buffers keep their Neovim state and undo history.
            -- Godot may still have edited the file while it was not the
            -- current buffer (e.g. connecting a signal inserts a callback
            -- before the tab switch reaches us): apply just that difference.
            local synced = M.sync_external_lines(bufnr, lines, { unmodified = unmodified })
            content_updated = synced.status == "updated"
        end
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
        core._initialized_buffers[bufnr] = true
    end

    -- Attach for notifications if not already attached
    local attached = false
    if not core._attached_buffers[bufnr] then
        attached = vim.api.nvim_buf_attach(bufnr, false, {
            on_lines = function(_, buf, tick, first_line, last_line, last_line_updated, byte_count)
                -- Get the new lines content
                local new_lines = vim.api.nvim_buf_get_lines(buf, first_line, last_line_updated, false)
                -- Send RPC notification with change details
                vim.rpcnotify(0, "godot_buf_lines", buf, tick, first_line, last_line, new_lines)
                return false  -- Continue receiving notifications
            end,
            on_detach = function()
                core._attached_buffers[bufnr] = nil
                core._initialized_buffers[bufnr] = nil
            end
        })
        if attached then
            core._attached_buffers[bufnr] = true
        end
    else
        attached = true
    end

    -- Apply indent options if provided
    -- This ensures Neovim uses the same indent settings as Godot
    if indent_opts then
        M.set_indent_options(bufnr, indent_opts.use_spaces, indent_opts.indent_size)
    end

    -- Get current state
    local tick = vim.api.nvim_buf_get_changedtick(bufnr)
    local cursor = vim.api.nvim_win_get_cursor(0)  -- {row, col}, 1-indexed row

    return {
        bufnr = bufnr,
        tick = tick,
        is_new = is_new,
        content_updated = content_updated,
        line_count = vim.api.nvim_buf_line_count(bufnr),
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
        initialized = core._initialized_buffers[bufnr] or false,
        attached = core._attached_buffers[bufnr] or false
    }
end

-- Reload current buffer from disk (:e!) and re-attach for notifications
-- Returns the new buffer content and cursor position to sync to Godot
-- @return table: { lines = buffer lines, tick = changedtick, cursor = {row, col} }
function M.reload_buffer()
    local bufnr = vim.api.nvim_get_current_buf()

    -- Execute :e! to reload from disk
    vim.cmd('e!')

    -- Re-attach for notifications (e! causes detach)
    core._attached_buffers[bufnr] = nil  -- Clear old attachment flag
    local attached = vim.api.nvim_buf_attach(bufnr, false, {
        on_lines = function(_, buf, tick, first_line, last_line, last_line_updated, byte_count)
            local new_lines = vim.api.nvim_buf_get_lines(buf, first_line, last_line_updated, false)
            vim.rpcnotify(0, "godot_buf_lines", buf, tick, first_line, last_line, new_lines)
            return false
        end,
        on_detach = function()
            core._attached_buffers[bufnr] = nil
            core._initialized_buffers[bufnr] = nil
        end
    })
    if attached then
        core._attached_buffers[bufnr] = true
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
-- @param from_col number: Selection start column (0-indexed, CHARACTER position from Godot)
-- @param to_line number: Selection end line (1-indexed)
-- @param to_col number: Selection end column (0-indexed, CHARACTER position from Godot)
-- @return table: { mode = current mode after selection }
function M.set_visual_selection(from_line, from_col, to_line, to_col)
    -- Exit any existing visual mode first
    local mode = vim.api.nvim_get_mode().mode
    if mode:match('^[vV\x16]') then
        vim.api.nvim_feedkeys(vim.api.nvim_replace_termcodes('<Esc>', true, false, true), 'nx', false)
    end

    -- Get line contents for character-to-byte conversion
    local lines = vim.api.nvim_buf_get_lines(0, from_line - 1, to_line, false)
    local from_line_content = lines[1] or ""
    local to_line_content = lines[#lines] or ""

    -- Convert character columns to byte columns
    -- Note: Godot selection uses half-open interval [from, to) where to is exclusive
    -- Neovim visual mode uses closed interval [from, to] where both are inclusive
    -- So we need to subtract 1 from to_col to get the position OF the last character
    local from_byte_col = core.char_col_to_byte_col(from_line_content, from_col)
    local to_char_col = to_col > 0 and (to_col - 1) or 0
    local to_byte_col = core.char_col_to_byte_col(to_line_content, to_char_col)

    -- Move cursor to selection start (byte position)
    vim.api.nvim_win_set_cursor(0, {from_line, from_byte_col})

    -- Enter visual mode
    vim.cmd('normal! v')

    -- Move cursor to selection end (byte position)
    vim.api.nvim_win_set_cursor(0, {to_line, to_byte_col})

    return { mode = vim.api.nvim_get_mode().mode }
end

return M
