//! Clock skew detection for audit log integrity.
//!
//! Detects anomalous timestamp jumps caused by NTP drift, VM clock
//! adjustments, or other system clock issues. This helps prevent silent
//! corruption of audit-log timestamps and rebalance windowing logic.

use chrono::{DateTime, Duration, Utc};

/// Result of a clock skew check.
#[derive(Debug, Clone, PartialEq)]
pub enum SkewResult {
    /// No skew detected.
    Ok,
    /// Clock jumped backward.
    BackwardJump { duration: Duration },
    /// Clock jumped forward too fast.
    ForwardJump { duration: Duration, rate: f64 },
}

/// Detects clock skew by tracking the last seen timestamp.
///
/// The detector maintains the last timestamp it saw and checks each new
/// timestamp for anomalous jumps. Backward jumps (clock went backward)
/// and excessive forward jumps (clock moving faster than real time) are
/// detected.
///
/// # Why clock skew is dangerous
///
/// Clock skew can corrupt audit log integrity by:
/// - Creating out-of-order event sequences
/// - Making time-based windowing unreliable
/// - Causing timestamps to appear before they actually occurred
///
/// This detector warns when skew is detected but does not block
/// operations — the audit log continues recording events, but the
/// warning alerts operators to investigate.
#[derive(Debug)]
pub struct ClockSkewDetector {
    last_ts: Option<DateTime<Utc>>,
    /// Maximum allowed backward jump (default: 30 seconds).
    threshold_sec: i64,
    /// Maximum allowed forward jump rate in seconds per second (default: 2.0).
    max_jump_rate_sec_per_sec: f64,
}

impl ClockSkewDetector {
    /// Create a new detector with default thresholds.
    pub fn new() -> Self {
        Self {
            last_ts: None,
            threshold_sec: 30,
            max_jump_rate_sec_per_sec: 2.0,
        }
    }

    /// Create a new detector with custom thresholds.
    ///
    /// # Arguments
    ///
    /// * `threshold_sec` - Maximum allowed backward jump in seconds.
    /// * `max_jump_rate_sec_per_sec` - Maximum forward jump rate (e.g., 2.0 = 2x real time).
    pub fn with_thresholds(threshold_sec: i64, max_jump_rate_sec_per_sec: f64) -> Self {
        Self {
            last_ts: None,
            threshold_sec,
            max_jump_rate_sec_per_sec,
        }
    }

    /// Check a timestamp for clock skew.
    ///
    /// Compares the provided timestamp against the last seen timestamp
    /// and detects anomalous jumps. The first call always returns `Ok`
    /// since there's no previous timestamp to compare against.
    ///
    /// # Arguments
    ///
    /// * `ts` - The timestamp to check.
    ///
    /// # Returns
    ///
    /// A `SkewResult` indicating whether skew was detected and details
    /// about the jump if so.
    pub fn check(&mut self, ts: DateTime<Utc>) -> SkewResult {
        match self.last_ts {
            None => {
                self.last_ts = Some(ts);
                SkewResult::Ok
            }
            Some(last) => {
                let diff = ts - last;

                // Check for backward jump
                if diff < Duration::zero() {
                    let backward_duration = -diff;
                    if backward_duration > Duration::seconds(self.threshold_sec) {
                        // Update last_ts even on skew to avoid repeated warnings
                        self.last_ts = Some(ts);
                        return SkewResult::BackwardJump {
                            duration: backward_duration,
                        };
                    }
                }

                // Check for excessive forward jump
                if diff > Duration::zero() {
                    let elapsed_secs = diff.num_seconds() as f64;
                    // Calculate rate: seconds jumped / actual seconds since last check
                    // Since we don't track actual wall-clock time between checks,
                    // we use the jump duration itself as a proxy.
                    // A forward jump of more than max_jump_rate_sec_per_sec per second
                    // of elapsed time is considered suspicious.
                    // For simplicity, we treat any jump > threshold as suspicious
                    // if it exceeds the rate limit.
                    let max_allowed = Duration::seconds(self.threshold_sec);
                    if diff > max_allowed {
                        // Calculate rate as (jump duration / threshold)
                        // This is a simplified rate check
                        let rate = elapsed_secs / self.threshold_sec as f64;
                        if rate > self.max_jump_rate_sec_per_sec {
                            self.last_ts = Some(ts);
                            return SkewResult::ForwardJump {
                                duration: diff,
                                rate,
                            };
                        }
                    }
                }

                self.last_ts = Some(ts);
                SkewResult::Ok
            }
        }
    }

    /// Reset the detector, clearing the last timestamp.
    pub fn reset(&mut self) {
        self.last_ts = None;
    }

    /// Set the last timestamp directly (for testing only).
    #[cfg(test)]
    pub fn set_last_timestamp(&mut self, ts: DateTime<Utc>) {
        self.last_ts = Some(ts);
    }
}

impl Default for ClockSkewDetector {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn first_call_always_ok() {
        let mut detector = ClockSkewDetector::new();
        let ts = Utc::now();
        assert_eq!(detector.check(ts), SkewResult::Ok);
    }

    #[test]
    fn normal_forward_progress_ok() {
        let mut detector = ClockSkewDetector::new();
        let ts1 = Utc::now();
        detector.check(ts1);

        let ts2 = ts1 + Duration::seconds(1);
        assert_eq!(detector.check(ts2), SkewResult::Ok);
    }

    #[test]
    fn backward_jump_detected() {
        let mut detector = ClockSkewDetector::new();
        let ts1 = Utc::now();
        detector.check(ts1);

        let ts2 = ts1 - Duration::seconds(35); // 35 seconds backward
        match detector.check(ts2) {
            SkewResult::BackwardJump { duration } => {
                assert_eq!(duration, Duration::seconds(35));
            }
            other => panic!("Expected BackwardJump, got {:?}", other),
        }
    }

    #[test]
    fn small_backward_jump_allowed() {
        let mut detector = ClockSkewDetector::new();
        let ts1 = Utc::now();
        detector.check(ts1);

        let ts2 = ts1 - Duration::seconds(10); // 10 seconds backward (under threshold)
        assert_eq!(detector.check(ts2), SkewResult::Ok);
    }

    #[test]
    fn forward_jump_detected() {
        let mut detector = ClockSkewDetector::with_thresholds(30, 2.0);
        let ts1 = Utc::now();
        detector.check(ts1);

        let ts2 = ts1 + Duration::seconds(90); // 90 seconds forward (3x threshold)
        match detector.check(ts2) {
            SkewResult::ForwardJump { duration, rate } => {
                assert_eq!(duration, Duration::seconds(90));
                assert!(rate > 2.0);
            }
            other => panic!("Expected ForwardJump, got {:?}", other),
        }
    }

    #[test]
    fn moderate_forward_jump_allowed() {
        let mut detector = ClockSkewDetector::with_thresholds(30, 2.0);
        let ts1 = Utc::now();
        detector.check(ts1);

        let ts2 = ts1 + Duration::seconds(40); // 40 seconds forward (under rate limit)
        assert_eq!(detector.check(ts2), SkewResult::Ok);
    }

    #[test]
    fn reset_clears_state() {
        let mut detector = ClockSkewDetector::new();
        let ts1 = Utc::now();
        detector.check(ts1);
        detector.reset();

        let ts2 = ts1 - Duration::seconds(100); // Large backward jump
        // After reset, first call is always OK
        assert_eq!(detector.check(ts2), SkewResult::Ok);
    }

    #[test]
    fn custom_thresholds() {
        let mut detector = ClockSkewDetector::with_thresholds(10, 5.0);
        let ts1 = Utc::now();
        detector.check(ts1);

        // 15 seconds backward (exceeds custom threshold of 10)
        let ts2 = ts1 - Duration::seconds(15);
        match detector.check(ts2) {
            SkewResult::BackwardJump { duration } => {
                assert_eq!(duration, Duration::seconds(15));
            }
            other => panic!("Expected BackwardJump, got {:?}", other),
        }
    }
}
