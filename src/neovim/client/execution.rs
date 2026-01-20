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
}
