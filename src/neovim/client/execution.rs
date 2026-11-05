//! Command and Lua execution

use super::{ClientError, NeovimClient};

impl NeovimClient {
    /// Execute Neovim command
    pub fn command(&self, cmd: &str) -> Result<(), ClientError> {
        self.ensure_rpc_ready()?;
        let neovim_arc = self.neovim.clone();
        let cmd = cmd.to_string();

        self.runtime.block_on(async {
            let nvim = neovim_arc.lock().await.as_ref().cloned();
            if let Some(neovim) = nvim.as_ref() {
                neovim
                    .command(&cmd)
                    .await
                    .map_err(|e| ClientError::rpc("execute command", e))?;
                Ok(())
            } else {
                Err(ClientError::NotConnected)
            }
        })
    }

    /// Execute Lua code and return the result
    pub fn execute_lua_with_result(&self, lua_code: &str) -> Result<rmpv::Value, ClientError> {
        self.execute_lua_with_args(lua_code, vec![])
    }

    /// Execute Lua code with arguments and return the result
    pub fn execute_lua_with_args(
        &self,
        lua_code: &str,
        args: Vec<rmpv::Value>,
    ) -> Result<rmpv::Value, ClientError> {
        self.ensure_rpc_ready()?;
        let neovim_arc = self.neovim.clone();
        let lua_code = lua_code.to_string();

        self.runtime.block_on(async {
            let nvim = neovim_arc.lock().await.as_ref().cloned();
            if let Some(neovim) = nvim.as_ref() {
                neovim
                    .exec_lua(&lua_code, args)
                    .await
                    .map_err(|e| ClientError::rpc("execute Lua", e))
            } else {
                Err(ClientError::NotConnected)
            }
        })
    }

    /// Debug: Get current indent settings from Neovim
    #[allow(dead_code)]
    pub fn debug_get_indent_settings(&self) -> Result<String, ClientError> {
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
