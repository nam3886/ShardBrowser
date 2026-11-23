//! Bounded page calls (Python `browser/session.py` `PageWatch`): a frozen renderer
//! never answers, so every call a flow makes on a page has a deadline, and a few
//! missed deadlines in a row mark the browser hung.

use std::future::Future;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::Arc;
use std::time::Duration;

use super::{WebError, WebResult};

/// One page call in a wait loop may take this long before that poll counts as "saw nothing".
pub const CALL_TIMEOUT: Duration = Duration::from_secs(10);
/// Timed-out polls in a row that mark the browser hung.
pub const HUNG_AFTER: u32 = 3;
/// A one-off page action (typing, pointer, a box) may take this long.
pub const ACT_TIMEOUT: Duration = Duration::from_secs(30);

/// Shared by every clone of a page: a miss seen through one clone counts for all.
#[derive(Clone, Debug)]
pub struct Watch {
    misses: Arc<AtomicU32>,
    call_timeout: Duration,
    act_timeout: Duration,
}

impl Default for Watch {
    fn default() -> Self {
        Self::with_timeouts(CALL_TIMEOUT, ACT_TIMEOUT)
    }
}

impl Watch {
    pub fn with_timeouts(call_timeout: Duration, act_timeout: Duration) -> Self {
        Self {
            misses: Arc::new(AtomicU32::new(0)),
            call_timeout,
            act_timeout,
        }
    }

    pub fn hung(&self) -> bool {
        self.misses.load(Ordering::SeqCst) >= HUNG_AFTER
    }

    /// `fut`'s value, or `default` when it misses its deadline. A call that answers
    /// resets the count. Once hung, gives `default` at once (the caller is on its way out).
    pub async fn call<T>(&self, fut: impl Future<Output = T>, default: T) -> T {
        if self.hung() {
            return default;
        }
        match tokio::time::timeout(self.call_timeout, fut).await {
            Ok(v) => {
                self.misses.store(0, Ordering::SeqCst);
                v
            }
            Err(_) => {
                self.misses.fetch_add(1, Ordering::SeqCst);
                default
            }
        }
    }

    /// A one-off action: past its deadline the browser counts as hung.
    pub async fn act<T>(&self, fut: impl Future<Output = WebResult<T>>) -> WebResult<T> {
        self.act_within(Duration::ZERO, fut).await
    }

    /// An action with a timeout of its own (a click's wait): `own` on top of the
    /// act deadline before the browser counts as hung, so a long but answering
    /// wait is never taken for a frozen renderer.
    pub async fn act_within<T>(
        &self,
        own: Duration,
        fut: impl Future<Output = WebResult<T>>,
    ) -> WebResult<T> {
        if self.hung() {
            return Err(WebError::Hung);
        }
        match tokio::time::timeout(self.act_timeout + own, fut).await {
            Ok(r) => r,
            Err(_) => {
                self.misses.store(HUNG_AFTER, Ordering::SeqCst);
                Err(WebError::Timeout("page action"))
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn quick() -> Watch {
        Watch::with_timeouts(Duration::from_millis(50), Duration::from_millis(50))
    }

    #[tokio::test]
    async fn hung_after_three_timeouts() {
        let w = quick();
        for _ in 0..3 {
            assert!(!w.hung());
            assert_eq!(w.call(std::future::pending::<u8>(), 7).await, 7);
        }
        assert!(w.hung());
        let t = std::time::Instant::now();
        assert_eq!(w.call(std::future::pending::<u8>(), 9).await, 9);
        assert!(
            t.elapsed() < Duration::from_millis(20),
            "a hung watch answers at once"
        );
    }

    #[tokio::test]
    async fn an_answer_resets_the_count_and_clones_share_it() {
        let w = quick();
        let other = w.clone();
        w.call(std::future::pending::<()>(), ()).await;
        w.call(std::future::pending::<()>(), ()).await;
        assert_eq!(other.call(async { 1 }, 0).await, 1);
        w.call(std::future::pending::<()>(), ()).await;
        assert!(!other.hung());
    }

    #[tokio::test]
    async fn act_within_adds_its_own_timeout() {
        let w = quick();
        let slow = async {
            tokio::time::sleep(Duration::from_millis(120)).await;
            Ok(1)
        };
        assert_eq!(w.act_within(Duration::from_millis(200), slow).await, Ok(1));
        assert!(!w.hung());
    }

    #[tokio::test]
    async fn act_timeout_marks_hung() {
        let w = quick();
        let r = w.act(std::future::pending::<WebResult<()>>()).await;
        assert!(matches!(r, Err(WebError::Timeout(_))));
        assert!(w.hung());
        assert!(matches!(w.act(async { Ok(()) }).await, Err(WebError::Hung)));
    }
}
