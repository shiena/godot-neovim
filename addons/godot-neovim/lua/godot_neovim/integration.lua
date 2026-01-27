-- godot_neovim/integration.lua: Godot integration (autocmds, commands)

local core = require('godot_neovim.core')

local M = {}

-- Setup global autocmds
function M.setup_autocmds()
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
            if cursor[1] ~= core._last_cursor[1] or cursor[2] ~= core._last_cursor[2] or mode ~= core._last_mode then
                core._last_cursor = cursor
                core._last_mode = mode
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
            if core._initialized_buffers[bufnr] and path ~= '' then
                vim.rpcnotify(0, "godot_buf_enter", bufnr, path)
            end
        end
    })
end

-- Setup file commands (:q, :wq, etc.) to delegate to Godot
-- Similar to vscode-neovim's vscode-file-commands.vim
function M.setup_file_commands()
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
function M.setup_buffer_autocmds(bufnr)
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

-- Create debug command
function M.setup_debug_command()
    vim.api.nvim_create_user_command('DebugShowInfo', function()
        vim.rpcnotify(0, "godot_debug_print", "godot_neovim Lua plugin loaded")
        vim.rpcnotify(0, "godot_debug_print", "Buffer: " .. vim.api.nvim_get_current_buf())
        vim.rpcnotify(0, "godot_debug_print", "Changedtick: " .. core.get_changedtick(0))
    end, {})
end

return M
