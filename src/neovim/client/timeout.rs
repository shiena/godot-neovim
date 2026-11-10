use super::{ClientError, NeovimClient, RPC_EXTENDED_TIMEOUT_MS};
use std::future::Future;
use std::sync::atomic::Ordering;
use std::time::Duration;

impl NeovimClient {
    pub fn rpc_timed_out(&self) -> bool {
        self.rpc_timed_out.load(Ordering::Acquire)
    }

    /// Bound synchronous writes, including waiting for the connection mutex.
    pub(super) fn wait_for_rpc<T>(
        &self,
        operation: &'static str,
        request: impl Future<Output = Result<T, ClientError>>,
    ) -> Result<T, ClientError> {
        self.runtime.block_on(async {
            match tokio::time::timeout(Duration::from_millis(RPC_EXTENDED_TIMEOUT_MS), request)
                .await
            {
                Ok(result) => result,
                Err(_) => {
                    // Cancellation does not undo an RPC already delivered to Neovim.
                    // Do not apply its late notifications or retry a write against this session.
                    self.rpc_timed_out.store(true, Ordering::Release);
                    if let Some(task) = &self.io_handle {
                        task.abort();
                    }
                    if let Some(task) = &self.key_input_handle {
                        task.abort();
                    }
                    Err(ClientError::Timeout(operation))
                }
            }
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::neovim::NeovimHandler;
    use std::sync::{atomic::AtomicU64, Arc};
    use std::time::Instant;
    use tokio::{process::Command, sync::Mutex};

    fn client() -> NeovimClient {
        let runtime = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(1)
            .enable_all()
            .build()
            .unwrap();
        let handler = NeovimHandler::new();
        let (nvim, io, child) = runtime
            .block_on(nvim_rs::create::tokio::new_child_cmd(
                Command::new(std::env::var("NVIMRS_TEST_BIN").unwrap_or_else(|_| "nvim".into()))
                    .args(["--headless", "--embed", "-u", "NONE", "-i", "NONE"])
                    .kill_on_drop(true),
                handler.clone(),
            ))
            .unwrap();
        runtime.block_on(nvim.exec_lua("_G.godot_neovim = { buffer_update = function() vim.wait(5000); return 1 end, set_indent_options = function() vim.wait(5000) end }", vec![])).unwrap();
        NeovimClient {
            runtime,
            commands: None,
            rpc_timed_out: false.into(),
            pending_resize: None.into(),
            child: Some(child),
            neovim: Arc::new(Mutex::new(Some(nvim))),
            state: handler.get_state(),
            has_updates: handler.get_updates_flag(),
            handler,
            nvim_path: String::new(),
            clean: true,
            io_handle: Some(io),
            stderr_handle: None,
            key_input_tx: None,
            key_input_handle: None,
            keys_processed: Arc::new(AtomicU64::new(0)),
            keys_processed_seen: AtomicU64::new(0),
        }
    }

    #[test]
    fn unresponsive_sync_calls_return_and_quarantine_the_session() {
        for operation in 0..4 {
            let client = client();
            let began = Instant::now();
            let result = match operation {
                0 => client
                    .buffer_update(vec!["unsaved edit".into()])
                    .map(|_| ()),
                1 => client.set_indent_options(true, 4),
                2 => client.command("sleep 5"),
                _ => client.execute_lua_with_result("vim.wait(5000)").map(|_| ()),
            };
            assert!(matches!(result, Err(ClientError::Timeout(_))));
            assert!(began.elapsed() < Duration::from_secs(2));
            let began = Instant::now();
            assert!(matches!(
                client.buffer_update(vec![]),
                Err(ClientError::NotConnected)
            ));
            assert!(!client.send_key_via_channel("x"));
            assert!(began.elapsed() < Duration::from_millis(100));
        }
    }
}
