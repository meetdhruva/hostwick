use std::sync::Arc;
use std::time::Duration;

use tokio::time::Instant;

use tokio::sync::Mutex;

/// Token bucket shared by all transfers: caps total bandwidth.
#[derive(Clone)]
pub struct RateLimiter {
    inner: Arc<Mutex<Bucket>>,
}

struct Bucket {
    bytes_per_sec: u64,
    tokens: f64,
    last: Instant,
}

impl RateLimiter {
    pub fn new(bytes_per_sec: u64) -> Self {
        Self {
            inner: Arc::new(Mutex::new(Bucket {
                bytes_per_sec: bytes_per_sec.max(1),
                // Allow a burst of ~100 ms worth.
                tokens: bytes_per_sec as f64 / 10.0,
                last: Instant::now(),
            })),
        }
    }

    pub async fn set_rate(&self, bytes_per_sec: u64) {
        self.inner.lock().await.bytes_per_sec = bytes_per_sec.max(1);
    }

    /// Wait until `n` bytes may be sent.
    pub async fn acquire(&self, n: usize) {
        let wait = {
            let mut b = self.inner.lock().await;
            let now = Instant::now();
            let rate = b.bytes_per_sec as f64;
            let cap = (rate / 10.0).max(n as f64);
            b.tokens = (b.tokens + now.duration_since(b.last).as_secs_f64() * rate).min(cap);
            b.last = now;
            b.tokens -= n as f64;
            if b.tokens >= 0.0 {
                Duration::ZERO
            } else {
                Duration::from_secs_f64(-b.tokens / rate)
            }
        };
        if !wait.is_zero() {
            tokio::time::sleep(wait).await;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test(start_paused = true)]
    async fn throttles_to_configured_rate() {
        let rl = RateLimiter::new(1_000_000);
        let start = tokio::time::Instant::now();
        for _ in 0..30 {
            rl.acquire(100_000).await; // 3 MB total at 1 MB/s
        }
        let secs = start.elapsed().as_secs_f64();
        assert!((2.7..=3.2).contains(&secs), "took {secs}s");
    }
}
