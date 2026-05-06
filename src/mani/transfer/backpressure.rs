use std::sync::Arc;

use tokio::sync::Semaphore;

#[derive(Debug, Clone)]
pub struct BackpressureBank {
    sem: Arc<Semaphore>,
}

impl BackpressureBank {
    pub fn new(initial_credits: usize) -> Self {
        Self {
            sem: Arc::new(Semaphore::new(initial_credits)),
        }
    }

    pub fn increase_credits(&self, amount: usize) {
        tracing::trace!("Increasing credits by {}", amount);
        self.sem.add_permits(amount);
    }

    pub fn signal_shutdown(&self) {
        self.sem.close();
    }

    /// Acquires one credit. Returns `true` if a credit was obtained,
    /// `false` if the bank was shut down.
    pub async fn wait_for_credit(&self) -> bool {
        match self.sem.acquire().await {
            Ok(permit) => {
                permit.forget();
                true
            }
            Err(_) => false,
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
    async fn wait_for_credit_consumes_one_credit_per_call() {
        let bank = BackpressureBank::new(1);
        assert!(bank.wait_for_credit().await);

        // Second call must block: no credits left.
        let bank2 = bank.clone();
        let blocked = timeout(Duration::from_millis(50), async move {
            bank2.wait_for_credit().await
        })
        .await;
        assert!(blocked.is_err(), "second wait_for_credit should have blocked");
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

    /// Stress test: no producer/consumer interleaving should lose a wakeup.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn no_missed_wakeups_under_contention() {
        let bank = BackpressureBank::new(0);
        let bank2 = bank.clone();

        const N: usize = 10_000;

        let consumer = tokio::spawn(async move {
            for _ in 0..N {
                assert!(bank2.wait_for_credit().await);
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
