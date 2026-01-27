-- godot_neovim: Neovim plugin for godot-neovim integration
-- This module provides buffer management functions called from Rust

local M = {}

-- Load submodules
local core = require('godot_neovim.core')
local buffer = require('godot_neovim.buffer')
local integration = require('godot_neovim.integration')

-- Inject integration function into buffer module to avoid circular dependency
buffer._setup_buffer_autocmds = integration.setup_buffer_autocmds

-- Export submodules for direct access
M.core = core
M.buffer = buffer
M.integration = integration

-- Backward-compatible API: Buffer operations
M.buffer_register = buffer.buffer_register
M.buffer_register_and_attach = buffer.buffer_register_and_attach
M.buffer_update = buffer.buffer_update
M.switch_to_buffer = buffer.switch_to_buffer
M.get_buffer_info = buffer.get_buffer_info
M.reload_buffer = buffer.reload_buffer
M.set_indent_options = buffer.set_indent_options
M.set_visual_selection = buffer.set_visual_selection
M.join_no_space = buffer.join_no_space

-- Backward-compatible API: Core functions
M.send_keys = core.send_keys
M.get_state = core.get_state
M.get_changedtick = core.get_changedtick

-- Backward-compatible API: State (direct reference to core tables)
M._initialized_buffers = core._initialized_buffers
M._attached_buffers = core._attached_buffers
M._last_cursor = core._last_cursor
M._last_mode = core._last_mode

-- Setup function (called on plugin load)
function M.setup()
    -- Register global functions for RPC access
    _G.godot_neovim = M

    -- Setup autocmds and commands
    integration.setup_autocmds()
    integration.setup_file_commands()
    integration.setup_debug_command()
end

-- Auto-setup on require
M.setup()

return M
