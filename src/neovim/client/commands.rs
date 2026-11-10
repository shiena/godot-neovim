use super::{ClientError, NeovimClient, Writer};
use nvim_rs::Neovim;
use rmpv::Value;
use std::sync::{
    atomic::{AtomicUsize, Ordering},
    mpsc, Arc,
};
use tokio::{runtime::Runtime, sync::mpsc::unbounded_channel, task::JoinHandle};

pub struct CommandResult {
    pub command: String,
    pub result: Result<Option<Value>, ClientError>,
}

pub(super) struct CommandQueue {
    sender: tokio::sync::mpsc::UnboundedSender<String>,
    results: mpsc::Receiver<CommandResult>,
    pending: Arc<AtomicUsize>,
    task: JoinHandle<()>,
}

async fn execute_command(
    nvim: &Neovim<Writer>,
    command: &str,
) -> Result<Option<Value>, ClientError> {
    if let Some(option) = command
        .strip_prefix("set ")
        .and_then(|s| s.strip_suffix('?'))
        .filter(|s| !s.is_empty())
    {
        let script = "local name = ...; return vim.api.nvim_get_option_value(name, { buf = 0 })";
        nvim.exec_lua(script, vec![Value::from(option)])
            .await
            .map(Some)
            .map_err(|e| ClientError::rpc("get option", e))
    } else {
        nvim.command(command)
            .await
            .map(|_| None)
            .map_err(|e| ClientError::rpc("execute command", e))
    }
}

impl CommandQueue {
    pub(super) fn new(
        runtime: &Runtime,
        nvim: Neovim<Writer>,
        barrier: Arc<tokio::sync::Notify>,
    ) -> Self {
        let (sender, mut receiver) = unbounded_channel::<String>();
        let (results_tx, results) = mpsc::channel();
        let pending = Arc::new(AtomicUsize::new(0));
        let task_pending = pending.clone();
        let task = runtime.spawn(async move {
            let channel = nvim
                .get_api_info()
                .await
                .ok()
                .and_then(|info| info.into_iter().next());
            while let Some(command) = receiver.recv().await {
                let mut result = execute_command(&nvim, &command).await;
                if let Some(channel) = channel.as_ref() {
                    // Completion must include notifications already queued by the IO task.
                    // Otherwise a tab switch can apply the old buffer's edits to the new one.
                    let reached = barrier.notified();
                    match nvim
                        .exec_lua(
                            "local channel = ...; vim.rpcnotify(channel, 'godot_command_barrier')",
                            vec![channel.clone()],
                        )
                        .await
                    {
                        Ok(_) => reached.await,
                        Err(error) if result.is_ok() => {
                            result = Err(ClientError::rpc("finish command", error))
                        }
                        Err(_) => {}
                    }
                }
                let _ = results_tx.send(CommandResult { command, result });
                task_pending.fetch_sub(1, Ordering::Release);
            }
        });
        Self {
            sender,
            results,
            pending,
            task,
        }
    }

    fn send(&self, command: String) -> Result<(), ClientError> {
        self.pending.fetch_add(1, Ordering::AcqRel);
        if self.sender.send(command).is_err() {
            self.pending.fetch_sub(1, Ordering::Release);
            return Err(ClientError::NotConnected);
        }
        Ok(())
    }

    fn is_pending(&self) -> bool {
        self.pending.load(Ordering::Acquire) != 0
    }
}

impl Drop for CommandQueue {
    fn drop(&mut self) {
        self.task.abort();
    }
}

impl NeovimClient {
    pub fn queue_command(&self, command: &str) -> Result<(), ClientError> {
        if self.rpc_timed_out.load(Ordering::Acquire) {
            return Err(ClientError::NotConnected);
        }
        self.commands
            .as_ref()
            .ok_or(ClientError::NotConnected)?
            .send(command.to_owned())
    }

    pub fn commands_pending(&self) -> bool {
        self.commands.as_ref().is_some_and(CommandQueue::is_pending)
    }

    pub fn take_command_results(&self) -> Vec<CommandResult> {
        if !self.commands_pending() {
            if let Some((width, height)) = self.pending_resize.take() {
                self.ui_try_resize(width, height);
            }
        }
        self.commands
            .as_ref()
            .map(|queue| queue.results.try_iter().collect())
            .unwrap_or_default()
    }

    pub(super) fn ensure_rpc_ready(&self) -> Result<(), ClientError> {
        if self.rpc_timed_out.load(Ordering::Acquire) {
            Err(ClientError::NotConnected)
        } else if self.commands_pending() {
            Err(ClientError::Busy)
        } else {
            Ok(())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::neovim::NeovimHandler;
    use nvim_rs::create::tokio::new_child_cmd;
    use std::time::{Duration, Instant};
    use tokio::{process::Command, runtime::Builder};

    fn with_session(test: impl FnOnce(&Runtime, &Neovim<Writer>, &CommandQueue, &NeovimHandler)) {
        let runtime = Builder::new_multi_thread()
            .worker_threads(1)
            .enable_all()
            .build()
            .unwrap();
        let program = std::env::var("NVIMRS_TEST_BIN").unwrap_or_else(|_| "nvim".into());
        let handler = NeovimHandler::new();
        let barrier = handler.command_barrier();
        let (nvim, io, mut child) = runtime
            .block_on(new_child_cmd(
                Command::new(program)
                    .args(["--embed", "--headless", "-u", "NONE", "-i", "NONE"])
                    .kill_on_drop(true),
                handler.clone(),
            ))
            .unwrap();
        runtime
            .block_on(nvim.ui_attach(80, 24, &nvim_rs::UiAttachOptions::new()))
            .unwrap();
        let queue = CommandQueue::new(&runtime, nvim.clone(), barrier);
        test(&runtime, &nvim, &queue, &handler);
        drop(queue);
        runtime.block_on(async {
            child.kill().await.unwrap();
            let _ = io.await;
        });
    }

    #[test]
    fn slow_commands_return_immediately_and_complete_in_order() {
        with_session(|runtime, nvim, queue, _| {
            let started = Instant::now();
            queue
                .send("sleep 300m | let g:async_order = 1".into())
                .unwrap();
            queue.send("let g:async_order += 1".into()).unwrap();
            assert!(started.elapsed() < Duration::from_millis(100));
            assert!(queue.is_pending());
            assert!(queue.results.try_recv().is_err());
            let first = queue.results.recv_timeout(Duration::from_secs(3)).unwrap();
            let second = queue.results.recv_timeout(Duration::from_secs(3)).unwrap();
            assert!(first.command.starts_with("sleep"));
            assert!(first.result.is_ok());
            assert_eq!(second.command, "let g:async_order += 1");
            assert!(second.result.is_ok());
            assert_eq!(
                runtime.block_on(nvim.eval("g:async_order")).unwrap(),
                Value::from(2)
            );
        });
    }

    #[test]
    fn input_can_release_a_command_waiting_for_a_key() {
        with_session(|runtime, nvim, queue, _| {
            queue.send("let g:async_key = getchar()".into()).unwrap();
            std::thread::sleep(Duration::from_millis(50));
            assert!(queue.results.try_recv().is_err());
            runtime.block_on(async {
                tokio::time::timeout(Duration::from_secs(2), nvim.input("x"))
                    .await
                    .unwrap()
                    .unwrap();
            });
            let completed = queue.results.recv_timeout(Duration::from_secs(3)).unwrap();
            assert!(completed.result.is_ok());
            assert_eq!(
                runtime.block_on(nvim.eval("g:async_key")).unwrap(),
                Value::from(120)
            );
        });
    }

    #[test]
    fn command_errors_do_not_stop_later_commands() {
        with_session(|_, _, queue, _| {
            queue.send("NoSuchNeovimCommand".into()).unwrap();
            queue.send("set tabstop?".into()).unwrap();
            assert!(queue
                .results
                .recv_timeout(Duration::from_secs(3))
                .unwrap()
                .result
                .is_err());
            let result = queue
                .results
                .recv_timeout(Duration::from_secs(3))
                .unwrap()
                .result
                .unwrap();
            assert!(result.unwrap().as_i64().is_some());
        });
    }
    #[test]
    fn completion_waits_for_pending_notifications() {
        with_session(|runtime, nvim, queue, handler| {
            let channel = runtime.block_on(nvim.get_api_info()).unwrap()[0]
                .as_i64()
                .unwrap();
            let state = handler.get_state();
            let locked = state.blocking_lock();
            queue.send(format!("lua vim.rpcnotify({channel}, 'godot_cursor_moved', 1, 2, 'n'); vim.g.barrier_done = 1")).unwrap();
            runtime.block_on(async {
                tokio::time::timeout(Duration::from_secs(2), async {
                    while nvim.eval("exists('g:barrier_done')").await.unwrap() != Value::from(1) {
                        tokio::task::yield_now().await;
                    }
                })
                .await
                .unwrap();
            });
            assert!(queue.results.try_recv().is_err());
            assert!(queue.is_pending());
            drop(locked);
            assert!(queue
                .results
                .recv_timeout(Duration::from_secs(2))
                .unwrap()
                .result
                .is_ok());
        });
    }
}
