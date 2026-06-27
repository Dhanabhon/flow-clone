//! Optional throughput throttle.
//!
//! If a `max_bytes_per_sec` cap is set, the throttle sleeps briefly after each
//! block so the clone does not saturate a slow target or the system bus.

use std::thread;
use std::time::{Duration, Instant};

/// Cap throughput to a configured rate.
pub struct Throttle {
    max: Option<u64>,
    window_start: Instant,
    window_bytes: u64,
}

impl Throttle {
    pub fn new(max_bytes_per_sec: Option<u64>) -> Self {
        Self {
            max: max_bytes_per_sec,
            window_start: Instant::now(),
            window_bytes: 0,
        }
    }

    /// Account for `bytes` just written and sleep if we're ahead of the cap.
    pub fn wait(&mut self, bytes: u64) {
        let Some(max) = self.max else {
            return;
        };
        if max == 0 {
            return;
        }

        self.window_bytes += bytes;
        let elapsed = self.window_start.elapsed();
        let allowed = (max as f64 * elapsed.as_secs_f64()) as u64;

        if self.window_bytes > allowed {
            let overshoot = self.window_bytes - allowed;
            let sleep_secs = overshoot as f64 / max as f64;
            thread::sleep(Duration::from_secs_f64(sleep_secs.min(1.0)));
        }

        // Reset the accounting window roughly once per second to stay accurate.
        if self.window_start.elapsed() >= Duration::from_secs(1) {
            self.window_start = Instant::now();
            self.window_bytes = 0;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn no_cap_means_no_sleep() {
        let mut t = Throttle::new(None);
        t.wait(1_000_000_000); // should return immediately
    }

    #[test]
    fn zero_cap_means_no_sleep() {
        let mut t = Throttle::new(Some(0));
        t.wait(1_000_000_000);
    }
}
