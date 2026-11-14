//! Runtime scheduling without manufacturing an Instant before the clock origin.
//! An initial elapsed duration makes the first periodic action immediately due,
//! including in a freshly booted Windows guest. This is not a consensus clock
//! and must not be used for canonical proof timestamps or wire timestamps.
use std::time::{Duration, Instant};

#[derive(Clone, Copy, Debug)]
pub struct ElapsedTimer {
    started: Instant,
    initial_elapsed: Duration,
}

impl ElapsedTimer {
    pub fn now() -> Self {
        Self::from(Instant::now())
    }

    pub fn with_elapsed(initial_elapsed: Duration) -> Self {
        Self { started: Instant::now(), initial_elapsed }
    }

    pub fn elapsed(&self) -> Duration {
        self.elapsed_at(Instant::now())
    }

    fn elapsed_at(&self, now: Instant) -> Duration {
        self.initial_elapsed.saturating_add(now.saturating_duration_since(self.started))
    }
}

impl From<Instant> for ElapsedTimer {
    fn from(started: Instant) -> Self {
        Self { started, initial_elapsed: Duration::ZERO }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hf134_timer_initial_age_is_due_at_zero_elapsed() {
        // No time before this anchor needs to exist. Cover all startup offsets.
        let anchor = Instant::now();
        for seconds in [10, 15, 30, 60, 300, 1921] {
            let age = Duration::from_secs(seconds);
            let timer = ElapsedTimer { started: anchor, initial_elapsed: age };
            assert_eq!(timer.elapsed_at(anchor), age);
        }
    }

    #[test]
    fn hf134_timer_maximum_initial_age_cannot_underflow_clock() {
        // The old `Instant::now() - Duration::MAX` panics regardless of uptime.
        let timer = ElapsedTimer::with_elapsed(Duration::MAX);
        assert_eq!(timer.elapsed(), Duration::MAX);
        assert_eq!(timer.elapsed_at(timer.started + Duration::from_secs(1)), Duration::MAX);
    }

    #[test]
    fn hf134_timer_reset_discards_initial_age() {
        let anchor = Instant::now();
        let mut timer = ElapsedTimer { started: anchor, initial_elapsed: Duration::from_secs(300) };
        assert_eq!(timer.elapsed_at(anchor), Duration::from_secs(300));
        let reset = anchor + Duration::from_secs(2);
        timer = reset.into();
        assert_eq!(timer.elapsed_at(reset), Duration::ZERO);
        assert_eq!(timer.elapsed_at(reset + Duration::from_millis(25)), Duration::from_millis(25));
    }

    #[test]
    fn hf134_timer_preserves_periodic_threshold() {
        let anchor = Instant::now();
        let timer = ElapsedTimer { started: anchor, initial_elapsed: Duration::from_secs(59) };
        assert!(timer.elapsed_at(anchor + Duration::from_millis(999)) < Duration::from_secs(60));
        assert_eq!(timer.elapsed_at(anchor + Duration::from_secs(1)), Duration::from_secs(60));
    }

    #[test]
    fn hf134_timer_normal_clock_never_starts_due() {
        let timer = ElapsedTimer::now();
        assert_eq!(timer.elapsed_at(timer.started), Duration::ZERO);
        assert_eq!(timer.elapsed_at(timer.started + Duration::from_secs(12)), Duration::from_secs(12));
    }

    #[test]
    fn hf134_timer_reversed_observation_does_not_underflow() {
        let earlier = Instant::now();
        let later = earlier + Duration::from_secs(1);
        let timer = ElapsedTimer { started: later, initial_elapsed: Duration::from_secs(15) };
        assert_eq!(timer.elapsed_at(earlier), Duration::from_secs(15));
    }
}
