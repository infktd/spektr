//! Outstanding-work accounting, so `spektr run` can wait for quiescence.
//!
//! Every queued job, every ingestion message and every planning request
//! counts as one unit from the moment it is created until it is fully
//! processed, including the follow-up work it causes (which is counted
//! before the original unit is released). Zero means idle.

use std::sync::Arc;
use std::sync::atomic::{AtomicI64, Ordering};
use std::time::Duration;

use tokio::sync::Notify;

#[derive(Clone, Default)]
pub struct Work {
    inner: Arc<(AtomicI64, Notify)>,
}

impl Work {
    pub fn add(&self, n: i64) {
        self.inner.0.fetch_add(n, Ordering::SeqCst);
    }

    pub fn done(&self, n: i64) {
        let left = self.inner.0.fetch_sub(n, Ordering::SeqCst) - n;
        if left <= 0 {
            self.inner.1.notify_waiters();
        }
    }

    pub fn outstanding(&self) -> i64 {
        self.inner.0.load(Ordering::SeqCst)
    }

    /// Wait until nothing is outstanding. `false` on timeout.
    pub async fn idle(&self, timeout: Duration) -> bool {
        let deadline = tokio::time::Instant::now() + timeout;
        loop {
            let notified = self.inner.1.notified();
            tokio::pin!(notified);
            notified.as_mut().enable();
            if self.outstanding() <= 0 {
                return true;
            }
            if tokio::time::timeout_at(deadline, notified).await.is_err() {
                return self.outstanding() <= 0;
            }
        }
    }
}
