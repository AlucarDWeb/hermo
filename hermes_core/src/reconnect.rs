//! Reconnect backoff (use-case helper, PLAN §1.5): 1/2/4/8/15 s, +20% jitter.

/// Reconnect attempt delays in milliseconds (attempt 0 = first retry).
pub const BACKOFF_MS: [u64; 5] = [1_000, 2_000, 4_000, 8_000, 15_000];

/// Raw delay for `attempt` (0-based). Past the last step it holds at the
/// final 15 s step for the connection's remaining life.
pub fn delay_for_attempt(attempt: u32) -> u64 {
    BACKOFF_MS[usize::min(attempt as usize, BACKOFF_MS.len() - 1)]
}

/// Delay for `attempt` with caller-supplied `jitter` (factor in `0.0..=1.0`):
/// up to +20% of the base delay. The supervisor passes a random factor at
/// runtime; tests pass a fixed one — never a sleep, never a clock.
pub fn jittered_delay(attempt: u32, jitter: f64) -> u64 {
    let base = delay_for_attempt(attempt);
    let factor = jitter.clamp(0.0, 1.0);
    base + ((base as f64) * 0.2 * factor) as u64
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Rule pinned: the exact schedule 1/2/4/8/15 s (PLAN §1.5). A broken
    /// schedule (wrong step, wrong order) fails here.
    #[test]
    fn backoff_schedule_is_1_2_4_8_15_seconds() {
        let expected = [1_000u64, 2_000, 4_000, 8_000, 15_000];
        for (attempt, want) in expected.iter().enumerate() {
            assert_eq!(delay_for_attempt(attempt as u32), *want, "attempt {attempt}");
        }
    }

    /// Rule pinned: attempts past the last step hold at 15 s (bounded
    /// retries, no overflow, no restart of the ladder).
    #[test]
    fn backoff_caps_at_final_step() {
        assert_eq!(delay_for_attempt(5), 15_000);
        assert_eq!(delay_for_attempt(50), 15_000);
        assert_eq!(delay_for_attempt(u32::MAX), 15_000);
    }

    /// Rule pinned: jitter is bounded — base at factor 0, at most +20% at
    /// factor 1, monotone in between, and an out-of-range factor is clamped
    /// (never a negative or a doubled delay).
    #[test]
    fn jitter_is_bounded_to_twenty_percent() {
        assert_eq!(jittered_delay(0, 0.0), 1_000, "no jitter at factor 0");
        assert_eq!(jittered_delay(0, 1.0), 1_200, "at most +20%");
        assert_eq!(jittered_delay(4, 1.0), 18_000, "+20% of 15 s");
        assert_eq!(jittered_delay(4, 0.5), 16_500, "half jitter, half bonus");
        // Out-of-range factors clamp, they do not explode.
        assert_eq!(jittered_delay(0, -3.0), 1_000);
        assert_eq!(jittered_delay(0, 42.0), 1_200);
        // Monotone: a larger factor never yields a smaller delay.
        let mut prev = 0;
        for i in 0..=10 {
            let d = jittered_delay(2, i as f64 / 10.0);
            assert!(d >= prev);
            prev = d;
        }
    }
}
