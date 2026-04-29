use std::time::{Duration, Instant};

#[derive(Debug)]
pub struct DoubleShiftDetector {
    timeout: Duration,
    max_hold: Duration,
    tap_count: u8,
    last_release: Option<Instant>,
    press_started: Option<Instant>,
}

impl DoubleShiftDetector {
    pub fn new(timeout_ms: u64, max_hold_ms: u64) -> Self {
        Self {
            timeout: Duration::from_millis(timeout_ms),
            max_hold: Duration::from_millis(max_hold_ms),
            tap_count: 0,
            last_release: None,
            press_started: None,
        }
    }

    pub fn on_shift_press(&mut self, now: Instant) {
        self.press_started = Some(now);
    }

    pub fn on_shift_release(&mut self, now: Instant) -> bool {
        let Some(press_started) = self.press_started.take() else {
            return false;
        };

        if now.duration_since(press_started) > self.max_hold {
            self.reset();
            return false;
        }

        if let Some(last_release) = self.last_release {
            if now.duration_since(last_release) <= self.timeout {
                self.tap_count += 1;
            } else {
                self.tap_count = 1;
            }
        } else {
            self.tap_count = 1;
        }

        self.last_release = Some(now);
        let triggered = self.tap_count >= 2;
        if triggered {
            self.reset();
        }
        triggered
    }

    pub fn invalidate_sequence(&mut self) {
        self.reset();
    }

    pub fn reset(&mut self) {
        self.tap_count = 0;
        self.last_release = None;
        self.press_started = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn triggers_on_two_fast_taps() {
        let base = Instant::now();
        let mut detector = DoubleShiftDetector::new(300, 250);
        detector.on_shift_press(base);
        assert!(!detector.on_shift_release(base + Duration::from_millis(50)));
        detector.on_shift_press(base + Duration::from_millis(110));
        assert!(detector.on_shift_release(base + Duration::from_millis(140)));
    }

    #[test]
    fn invalidates_when_other_key_interrupts() {
        let base = Instant::now();
        let mut detector = DoubleShiftDetector::new(300, 250);
        detector.on_shift_press(base);
        assert!(!detector.on_shift_release(base + Duration::from_millis(50)));
        detector.invalidate_sequence();
        detector.on_shift_press(base + Duration::from_millis(100));
        assert!(!detector.on_shift_release(base + Duration::from_millis(120)));
    }

    #[test]
    fn starts_new_sequence_after_invalidation() {
        let base = Instant::now();
        let mut detector = DoubleShiftDetector::new(300, 250);

        detector.invalidate_sequence();

        detector.on_shift_press(base);
        assert!(!detector.on_shift_release(base + Duration::from_millis(40)));
        detector.on_shift_press(base + Duration::from_millis(100));
        assert!(detector.on_shift_release(base + Duration::from_millis(140)));
    }
}
