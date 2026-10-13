//! Keep process initialization and session publication inside the shutdown barrier.
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::OnceLock;

use tokio::sync::{Mutex, MutexGuard};

#[derive(Default)]
pub(super) struct WorkspaceSpawnGate {
    closing: AtomicBool,
    initialization: Mutex<()>,
}

impl WorkspaceSpawnGate {
    pub(super) async fn acquire(&self) -> Result<MutexGuard<'_, ()>, String> {
        let guard = self.initialization.lock().await;
        if self.closing.load(Ordering::SeqCst) {
            return Err(
                "Hopper is shutting down; no new workspace sessions can be started.".into(),
            );
        }
        Ok(guard)
    }

    async fn close(&self) -> MutexGuard<'_, ()> {
        // Mark closed before waiting so callers already queued on the lock also
        // fail. The active caller keeps the guard through session publication.
        self.closing.store(true, Ordering::SeqCst);
        self.initialization.lock().await
    }
}

pub(super) fn workspace_session_spawn_lock() -> &'static WorkspaceSpawnGate {
    static GATE: OnceLock<WorkspaceSpawnGate> = OnceLock::new();
    GATE.get_or_init(WorkspaceSpawnGate::default)
}

pub(crate) async fn close_workspace_session_spawning() -> MutexGuard<'static, ()> {
    workspace_session_spawn_lock().close().await
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::future::Future;
    use std::task::Poll;

    #[test]
    fn shutdown_waits_for_initializing_session_to_be_published() {
        tokio::runtime::Builder::new_current_thread()
            .build()
            .unwrap()
            .block_on(async {
                let gate = WorkspaceSpawnGate::default();
                let initialization = gate.acquire().await.unwrap();
                let mut shutdown = Box::pin(gate.close());
                std::future::poll_fn(|context| {
                    assert!(shutdown.as_mut().poll(context).is_pending());
                    Poll::Ready(())
                })
                .await;

                // connect/add/respawn hold initialization until the process is
                // recorded, so shutdown's subsequent drain includes this session.
                let mut sessions = Vec::new();
                sessions.push("initialized session");
                drop(initialization);
                let shutdown_guard = shutdown.await;
                assert_eq!(
                    sessions.drain(..).collect::<Vec<_>>(),
                    vec!["initialized session"]
                );
                drop(shutdown_guard);
                assert!(gate.acquire().await.unwrap_err().contains("shutting down"));
            });
    }

    #[test]
    fn shutdown_rejects_a_spawn_queued_before_it_started() {
        tokio::runtime::Builder::new_current_thread()
            .build()
            .unwrap()
            .block_on(async {
                let gate = WorkspaceSpawnGate::default();
                let initialization = gate.acquire().await.unwrap();
                let mut queued_spawn = Box::pin(gate.acquire());
                std::future::poll_fn(|context| {
                    assert!(queued_spawn.as_mut().poll(context).is_pending());
                    Poll::Ready(())
                })
                .await;
                let mut shutdown = Box::pin(gate.close());
                std::future::poll_fn(|context| {
                    assert!(shutdown.as_mut().poll(context).is_pending());
                    Poll::Ready(())
                })
                .await;
                drop(initialization);
                assert!(queued_spawn.await.is_err());
                drop(shutdown.await);
                assert!(gate.acquire().await.is_err());
            });
    }
}
