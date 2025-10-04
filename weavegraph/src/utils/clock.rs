//! Injectable clock abstraction for checkpoints and time-based operations.
//!
//! Provides a mockable time source that wraps chrono for system time operations.
//! This abstraction enables deterministic testing and dependency injection for
//! time-sensitive functionality throughout the Weavegraph framework.

use chrono::{DateTime, Utc};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

/// Trait for time sources providing both Unix timestamps and DateTime objects.
pub trait Clock: Send + Sync {
    /// Get the current time as a Unix timestamp (seconds since epoch).
    fn now(&self) -> u64;

    /// Get the current time as a DateTime<Utc> for more complex time operations.
    fn now_datetime(&self) -> DateTime<Utc>;

    /// Get the current time as SystemTime for compatibility with std library.
    fn now_system_time(&self) -> SystemTime;

    /// Check if a duration has elapsed since the given timestamp.
    fn has_elapsed(&self, since: u64, duration: Duration) -> bool {
        let elapsed = self.now().saturating_sub(since);
        elapsed >= duration.as_secs()
    }

    /// Get the duration since a given timestamp.
    fn duration_since(&self, timestamp: u64) -> Duration {
        let elapsed_secs = self.now().saturating_sub(timestamp);
        Duration::from_secs(elapsed_secs)
    }
}

/// System clock implementation wrapping chrono for real time operations.
///
/// This is the default clock implementation that provides actual system time.
/// Use this in production environments where real time is required.
///
/// # Examples
///
/// ```rust
/// use weavegraph::utils::clock::{Clock, SystemClock};
///
/// let clock = SystemClock;
/// let timestamp = clock.now();
/// let datetime = clock.now_datetime();
/// println!("Current timestamp: {}", timestamp);
/// ```
#[derive(Debug, Clone, Copy, Default)]
pub struct SystemClock;

impl Clock for SystemClock {
    fn now(&self) -> u64 {
        Utc::now().timestamp() as u64
    }

    fn now_datetime(&self) -> DateTime<Utc> {
        Utc::now()
    }

    fn now_system_time(&self) -> SystemTime {
        SystemTime::now()
    }
}

/// Mock clock for deterministic testing and time manipulation.
///
/// This clock allows tests to control time progression and test time-dependent
/// behavior in a deterministic way. The time can be advanced manually to
/// simulate different scenarios.
///
/// # Examples
///
/// ```rust
/// use weavegraph::utils::clock::{Clock, MockClock};
/// use std::time::Duration;
///
/// let mut clock = MockClock::new(1000);
/// assert_eq!(clock.now(), 1000);
///
/// clock.advance(Duration::from_secs(30));
/// assert_eq!(clock.now(), 1030);
///
/// // Test timeout behavior
/// assert!(clock.has_elapsed(1000, Duration::from_secs(25)));
/// ```
#[derive(Debug, Clone)]
pub struct MockClock {
    current_time: u64,
}

impl MockClock {
    /// Create a new mock clock starting at the specified timestamp.
    ///
    /// # Parameters
    /// * `start_time` - Initial timestamp in seconds since Unix epoch
    ///
    /// # Returns
    /// A new MockClock instance set to the given time.
    #[must_use]
    pub fn new(start_time: u64) -> Self {
        Self {
            current_time: start_time,
        }
    }

    /// Create a mock clock starting at the current system time.
    ///
    /// Useful for tests that need to start from "now" but control progression.
    ///
    /// # Returns
    /// A new MockClock instance set to the current system time.
    #[must_use]
    pub fn now() -> Self {
        Self::new(SystemClock.now())
    }

    /// Advance the clock by the specified duration.
    ///
    /// This simulates the passage of time for testing purposes.
    ///
    /// # Parameters
    /// * `duration` - How much time to advance the clock
    ///
    /// # Examples
    /// ```rust
    /// use weavegraph::utils::clock::{Clock, MockClock};
    /// use std::time::Duration;
    ///
    /// let mut clock = MockClock::new(0);
    /// clock.advance(Duration::from_secs(60));
    /// assert_eq!(clock.now(), 60);
    /// ```
    pub fn advance(&mut self, duration: Duration) {
        self.current_time += duration.as_secs();
    }

    /// Advance the clock by the specified number of seconds.
    ///
    /// Convenience method for advancing by whole seconds.
    ///
    /// # Parameters
    /// * `seconds` - Number of seconds to advance
    pub fn advance_secs(&mut self, seconds: u64) {
        self.current_time += seconds;
    }

    /// Set the clock to a specific timestamp.
    ///
    /// This allows jumping to any point in time, useful for testing
    /// edge cases or specific time scenarios.
    ///
    /// # Parameters
    /// * `timestamp` - New timestamp to set the clock to
    pub fn set_time(&mut self, timestamp: u64) {
        self.current_time = timestamp;
    }

    /// Reset the clock to Unix epoch (timestamp 0).
    pub fn reset(&mut self) {
        self.current_time = 0;
    }
}

impl Clock for MockClock {
    fn now(&self) -> u64 {
        self.current_time
    }

    fn now_datetime(&self) -> DateTime<Utc> {
        DateTime::from_timestamp(self.current_time as i64, 0)
            .unwrap_or_else(|| DateTime::from_timestamp(0, 0).unwrap())
    }

    fn now_system_time(&self) -> SystemTime {
        UNIX_EPOCH + Duration::from_secs(self.current_time)
    }
}

/// Creates a boxed clock instance based on environment or testing needs.
///
/// This factory function provides a convenient way to create clock instances
/// with appropriate type erasure for dependency injection.
///
/// # Parameters
/// * `use_mock` - Whether to create a mock clock (true) or system clock (false)
/// * `mock_start_time` - Starting time for mock clock (ignored for system clock)
///
/// # Returns
/// A boxed Clock trait object
///
/// # Examples
///
/// ```rust
/// use weavegraph::utils::clock::create_clock;
///
/// // System clock for production
/// let sys_clock = create_clock(false, 0);
///
/// // Mock clock for testing
/// let mock_clock = create_clock(true, 1000);
/// ```
#[must_use]
pub fn create_clock(use_mock: bool, mock_start_time: u64) -> Box<dyn Clock> {
    if use_mock {
        Box::new(MockClock::new(mock_start_time))
    } else {
        Box::new(SystemClock)
    }
}

/// Utility functions for common time operations.
pub mod time_utils {
    use super::*;

    /// Format a timestamp as a human-readable string.
    ///
    /// # Parameters
    /// * `timestamp` - Unix timestamp to format
    ///
    /// # Returns
    /// Formatted time string in ISO 8601 format
    ///
    /// # Examples
    ///
    /// ```rust
    /// use weavegraph::utils::clock::time_utils::format_timestamp;
    ///
    /// let formatted = format_timestamp(1640995200); // 2022-01-01 00:00:00 UTC
    /// assert!(formatted.contains("2022"));
    /// ```
    #[must_use]
    pub fn format_timestamp(timestamp: u64) -> String {
        DateTime::from_timestamp(timestamp as i64, 0)
            .map(|dt| dt.format("%Y-%m-%d %H:%M:%S UTC").to_string())
            .unwrap_or_else(|| format!("invalid-timestamp-{}", timestamp))
    }

    /// Calculate the difference between two timestamps.
    ///
    /// # Parameters
    /// * `earlier` - Earlier timestamp
    /// * `later` - Later timestamp
    ///
    /// # Returns
    /// Duration between the timestamps, or zero if earlier > later
    #[must_use]
    pub fn duration_between(earlier: u64, later: u64) -> Duration {
        Duration::from_secs(later.saturating_sub(earlier))
    }

    /// Check if a timestamp is within a certain age.
    ///
    /// # Parameters
    /// * `timestamp` - Timestamp to check
    /// * `max_age` - Maximum acceptable age
    /// * `clock` - Clock instance to get current time
    ///
    /// # Returns
    /// True if the timestamp is within the specified age
    pub fn is_recent(timestamp: u64, max_age: Duration, clock: &dyn Clock) -> bool {
        let age = clock.duration_since(timestamp);
        age <= max_age
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[test]
    fn test_system_clock() {
        let clock = SystemClock;
        let now = clock.now();
        let datetime = clock.now_datetime();
        let system_time = clock.now_system_time();

        // Should be roughly the same time
        let system_timestamp = system_time.duration_since(UNIX_EPOCH).unwrap().as_secs();
        assert!((now as i64 - system_timestamp as i64).abs() < 2);

        // DateTime should also be close
        let datetime_timestamp = datetime.timestamp() as u64;
        assert!((now as i64 - datetime_timestamp as i64).abs() < 2);
    }

    #[test]
    fn test_mock_clock() {
        let mut clock = MockClock::new(1000);
        assert_eq!(clock.now(), 1000);

        clock.advance(Duration::from_secs(30));
        assert_eq!(clock.now(), 1030);

        clock.advance_secs(70);
        assert_eq!(clock.now(), 1100);

        clock.set_time(2000);
        assert_eq!(clock.now(), 2000);

        clock.reset();
        assert_eq!(clock.now(), 0);
    }

    #[test]
    fn test_clock_trait_methods() {
        let mut clock = MockClock::new(1000);

        // Test has_elapsed
        assert!(!clock.has_elapsed(1000, Duration::from_secs(10)));
        clock.advance_secs(15);
        assert!(clock.has_elapsed(1000, Duration::from_secs(10)));

        // Test duration_since
        let duration = clock.duration_since(1000);
        assert_eq!(duration.as_secs(), 15);
    }

    #[test]
    fn test_create_clock() {
        let sys_clock = create_clock(false, 0);
        let mock_clock = create_clock(true, 1000);

        // System clock should give current time
        let sys_time = sys_clock.now();
        assert!(sys_time > 1000); // Should be way past 1000 seconds since epoch

        // Mock clock should give exactly what we set
        let mock_time = mock_clock.now();
        assert_eq!(mock_time, 1000);
    }

    #[test]
    fn test_time_utils() {
        use super::time_utils::*;

        // Test format_timestamp
        let formatted = format_timestamp(0);
        assert!(formatted.contains("1970")); // Unix epoch

        // Test duration_between
        let duration = duration_between(1000, 1030);
        assert_eq!(duration.as_secs(), 30);

        let duration_reverse = duration_between(1030, 1000);
        assert_eq!(duration_reverse.as_secs(), 0); // Saturating sub

        // Test is_recent
        let clock = MockClock::new(1100);
        assert!(is_recent(1050, Duration::from_secs(60), &clock));
        assert!(!is_recent(1000, Duration::from_secs(60), &clock));
    }
}
