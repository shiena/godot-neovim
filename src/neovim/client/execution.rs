//! Command and Lua execution

use super::NeovimClient;

impl NeovimClient {
    /// Execute Neovim command
    pub fn command(&self, cmd: &str) -> Result<(), String> {
        let neovim_arc = self.neovim.clone();
        let cmd = cmd.to_string();

        self.runtime.block_on(async {
            let nvim_lock = neovim_arc.lock().await;
            if let Some(neovim) = nvim_lock.as_ref() {
                neovim
                    .command(&cmd)
                    .await
                    .map_err(|e| format!("Failed to execute command: {}", e))?;
                Ok(())
            } else {
                Err("Neovim not connected".to_string())
            }
        })
    }

    /// Execute Lua code and return the result
    pub fn execute_lua_with_result(&self, lua_code: &str) -> Result<rmpv::Value, String> {
        let neovim_arc = self.neovim.clone();
        let lua_code = lua_code.to_string();

        self.runtime.block_on(async {
            let nvim_lock = neovim_arc.lock().await;
            if let Some(neovim) = nvim_lock.as_ref() {
                neovim
                    .exec_lua(&lua_code, vec![])
                    .await
                    .map_err(|e| format!("Failed to execute Lua: {}", e))
            } else {
                Err("Neovim not connected".to_string())
            }
        })
    }

    /// Debug: Get current indent settings from Neovim
    #[allow(dead_code)]
    pub fn debug_get_indent_settings(&self) -> Result<String, String> {
        let lua_code = r#"
            return string.format(
                "expandtab=%s shiftwidth=%d tabstop=%d softtabstop=%d",
                tostring(vim.bo.expandtab),
                vim.bo.shiftwidth,
                vim.bo.tabstop,
                vim.bo.softtabstop
            )
        "#;

        match self.execute_lua_with_result(lua_code) {
            Ok(value) => {
                if let Some(s) = value.as_str() {
                    Ok(s.to_string())
                } else {
                    Ok(format!("{:?}", value))
                }
            }
            Err(e) => Err(e),
        }
    }
}
