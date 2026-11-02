use std::time::Duration;

/// Exponential backoff with "full jitter" (AWS architecture blog, 2015):
/// `sleep = random(0, min(cap, base * 2^attempt))`.
///
/// Full jitter spreads reconnect storms out when many sessions drop at once
/// (VPN blip, laptop wake), instead of every tab retrying in lock-step.
#[derive(Debug, Clone)]
pub struct Backoff {
    base: Duration,
    cap: Duration,
    attempt: u32,
    max_attempts: Option<u32>,
    seed: u64,
}

impl Backoff {
    pub fn new(base: Duration, cap: Duration) -> Self {
        let seed = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos() as u64)
            .unwrap_or(0x9E37_79B9_7F4A_7C15);
        Self {
            base,
            cap,
            attempt: 0,
            max_attempts: None,
            seed: seed | 1,
        }
    }

    /// Stop yielding delays after `n` attempts.
    pub fn with_max_attempts(mut self, n: u32) -> Self {
        self.max_attempts = Some(n);
        self
    }

    /// Deterministic jitter, for tests.
    pub fn with_seed(mut self, seed: u64) -> Self {
        self.seed = seed | 1;
        self
    }

    pub fn attempt(&self) -> u32 {
        self.attempt
    }

    pub fn reset(&mut self) {
        self.attempt = 0;
    }

    /// The upper bound for the next delay, before jitter.
    pub fn ceiling(&self) -> Duration {
        let factor = 1u32.checked_shl(self.attempt.min(31)).unwrap_or(u32::MAX);
        self.base.saturating_mul(factor).min(self.cap)
    }

    /// Returns the next delay, or `None` once `max_attempts` is reached.
    pub fn next_delay(&mut self) -> Option<Duration> {
        if self.max_attempts.is_some_and(|max| self.attempt >= max) {
            return None;
        }
        let ceiling = self.ceiling();
        self.attempt = self.attempt.saturating_add(1);
        // xorshift64*: good enough for jitter, no dependency on `rand`.
        self.seed ^= self.seed >> 12;
        self.seed ^= self.seed << 25;
        self.seed ^= self.seed >> 27;
        let r = self.seed.wrapping_mul(0x2545_F491_4F6C_DD1D);
        let nanos = ceiling.as_nanos().min(u64::MAX as u128) as u64;
        let jittered = if nanos == 0 { 0 } else { r % (nanos + 1) };
        Some(Duration::from_nanos(jittered))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn delays_stay_under_exponential_ceiling_and_cap() {
        let mut b = Backoff::new(Duration::from_millis(100), Duration::from_secs(5)).with_seed(42);
        let mut ceilings = vec![];
        for _ in 0..10 {
            let ceiling = b.ceiling();
            let d = b.next_delay().unwrap_or_default();
            assert!(d <= ceiling, "{d:?} > {ceiling:?}");
            ceilings.push(ceiling);
        }
        assert_eq!(ceilings[0], Duration::from_millis(100));
        assert_eq!(ceilings[3], Duration::from_millis(800));
        assert_eq!(
            *ceilings.last().unwrap_or(&Duration::ZERO),
            Duration::from_secs(5)
        );
    }

    #[test]
    fn max_attempts_is_respected() {
        let mut b =
            Backoff::new(Duration::from_millis(1), Duration::from_millis(10)).with_max_attempts(3);
        assert!(b.next_delay().is_some());
        assert!(b.next_delay().is_some());
        assert!(b.next_delay().is_some());
        assert!(b.next_delay().is_none());
        b.reset();
        assert!(b.next_delay().is_some());
    }
}
