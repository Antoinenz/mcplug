//! Background jobs feeding messages back to the UI. The UI thread never awaits network I/O.

use std::future::Future;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

use tokio::sync::mpsc;

pub type JobId = u64;

#[derive(Clone)]
pub struct JobRunner<M: Send + 'static> {
    tx: mpsc::UnboundedSender<M>,
    next: Arc<AtomicU64>,
}

impl<M: Send + 'static> JobRunner<M> {
    pub fn new() -> (Self, mpsc::UnboundedReceiver<M>) {
        let (tx, rx) = mpsc::unbounded_channel();
        (Self { tx, next: Arc::new(AtomicU64::new(1)) }, rx)
    }

    pub fn send(&self, m: M) {
        let _ = self.tx.send(m);
    }

    /// Run `fut` on the runtime; whatever it returns is sent as a message.
    pub fn spawn<F>(&self, fut: F) -> JobId
    where
        F: Future<Output = M> + Send + 'static,
    {
        let id = self.next.fetch_add(1, Ordering::Relaxed);
        let tx = self.tx.clone();
        tokio::spawn(async move {
            let m = fut.await;
            let _ = tx.send(m);
        });
        id
    }
}
