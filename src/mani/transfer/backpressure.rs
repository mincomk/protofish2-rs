use std::sync::{
    Arc,
    atomic::{AtomicBool, AtomicUsize, Ordering},
};

use tokio::sync::Notify;

#[derive(Debug, Clone)]
pub struct BackpressureBank {
    credits: Arc<AtomicUsize>,
    notify: Arc<Notify>,
    closed: Arc<AtomicBool>,
}

impl BackpressureBank {
    pub fn new(initial_credits: usize) -> Self {
        Self {
            credits: Arc::new(AtomicUsize::new(initial_credits)),
            notify: Arc::new(Notify::new()),
            closed: Arc::new(AtomicBool::new(false)),
        }
    }

    pub fn increase_credits(&self, amount: usize) {
        tracing::trace!("Increasing credits by {}", amount);

        self.credits.fetch_add(amount, Ordering::SeqCst);
        self.notify.notify_one();
    }

    pub fn decrease_credits(&self, amount: usize) {
        tracing::trace!("Decreasing credits by {}", amount);
        self.credits.fetch_sub(amount, Ordering::SeqCst);
    }

    pub fn signal_shutdown(&self) {
        self.closed.store(true, Ordering::SeqCst);
        self.notify.notify_one();
    }

    /// Returns `true` if credit was obtained, `false` if the bank was shut down.
    pub async fn wait_for_credit(&self) -> bool {
        loop {
            // Register as a waiter *before* re-checking state, so a notification
            // issued by a concurrent `increase_credits` / `signal_shutdown`
            // between the check and the await is captured as a permit on this
            // future rather than dropped.
            let notified = self.notify.notified();
            tokio::pin!(notified);
            notified.as_mut().enable();

            if self.closed.load(Ordering::SeqCst) {
                return false;
            }
            let current_credits = self.credits.load(Ordering::SeqCst);
            tracing::trace!("Current credits: {}", current_credits);
            if current_credits > 0 {
                return true;
            }
            notified.await;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;
    use tokio::time::timeout;

    #[tokio::test]
    async fn wait_for_credit_returns_immediately_when_credit_available() {
        let bank = BackpressureBank::new(1);
        assert!(bank.wait_for_credit().await);
    }

    #[tokio::test]
    async fn wait_for_credit_wakes_on_increase() {
        let bank = BackpressureBank::new(0);
        let bank2 = bank.clone();

        let handle = tokio::spawn(async move { bank2.wait_for_credit().await });

        tokio::task::yield_now().await;
        bank.increase_credits(1);

        let got = timeout(Duration::from_secs(1), handle)
            .await
            .expect("wait_for_credit timed out")
            .expect("task panicked");
        assert!(got);
    }

    #[tokio::test]
    async fn wait_for_credit_returns_false_on_shutdown() {
        let bank = BackpressureBank::new(0);
        let bank2 = bank.clone();

        let handle = tokio::spawn(async move { bank2.wait_for_credit().await });

        tokio::task::yield_now().await;
        bank.signal_shutdown();

        let got = timeout(Duration::from_secs(1), handle)
            .await
            .expect("wait_for_credit timed out")
            .expect("task panicked");
        assert!(!got);
    }

    /// Stress test that exercises the producer/consumer interleaving the bug
    /// hit. Without `Notified::enable()` before re-checking, this hangs because
    /// `notify_one` (or `notify_waiters`) issued between the consumer's
    /// credit-check and `.await` would be missed.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn no_missed_wakeups_under_contention() {
        let bank = BackpressureBank::new(0);
        let bank2 = bank.clone();

        const N: usize = 10_000;

        let consumer = tokio::spawn(async move {
            for _ in 0..N {
                assert!(bank2.wait_for_credit().await);
                bank2.decrease_credits(1);
            }
        });

        let producer = tokio::spawn(async move {
            for _ in 0..N {
                bank.increase_credits(1);
                tokio::task::yield_now().await;
            }
        });

        timeout(Duration::from_secs(10), async {
            producer.await.unwrap();
            consumer.await.unwrap();
        })
        .await
        .expect("producer/consumer did not complete; missed wakeup?");
    }
}
