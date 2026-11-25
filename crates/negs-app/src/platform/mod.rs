//! Small OS helpers shared by `negs-app` across Windows, macOS, and Linux.

use std::path::PathBuf;
use std::time::SystemTime;

#[cfg(windows)]
mod windows;
#[cfg(windows)]
pub use self::windows::*;

#[cfg(not(windows))]
mod unix;
#[cfg(not(windows))]
pub use self::unix::*;

/// Error returned by [`move_no_replace`].
#[derive(Debug)]
pub enum MoveError {
    Exists,
    InUse,
    Other(String),
}

impl std::fmt::Display for MoveError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Exists => f.write_str("file already exists"),
            Self::InUse => f.write_str("file is in use"),
            Self::Other(msg) => f.write_str(msg),
        }
    }
}

impl std::error::Error for MoveError {}

/// Aliases matching the original Windows naming so callers need minimal churn.
pub fn qpc() -> i64 {
    monotonic_ticks()
}

pub fn qpc_frequency() -> i64 {
    ticks_per_second()
}

pub fn qpc_ms(from: i64, to: i64) -> f64 {
    ticks_ms(from, to)
}

/// `NEGS_PROFILE_DIR`, or in test builds a scratch folder, so no test can ever
/// write the user's real profile.
pub(crate) fn profile_override() -> Option<PathBuf> {
    if let Some(dir) = std::env::var_os("NEGS_PROFILE_DIR") {
        return Some(PathBuf::from(dir));
    }
    if cfg!(test) {
        return Some(std::env::temp_dir().join(format!("negs-test-profile-{}", std::process::id())));
    }
    None
}

/// ISO 8601 UTC, as the Windows event log's XPath compares `SystemTime`.
pub fn iso_utc(time: SystemTime) -> String {
    let secs = time.duration_since(SystemTime::UNIX_EPOCH).map(|d| d.as_secs() as i64).unwrap_or(0);
    let (days, rem) = (secs.div_euclid(86_400), secs.rem_euclid(86_400));
    // Civil from days (Howard Hinnant).
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + i64::from(month <= 2);
    format!("{year:04}-{month:02}-{day:02}T{:02}:{:02}:{:02}.000Z", rem / 3600, rem / 60 % 60, rem % 60)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[test]
    fn iso_dates() {
        assert_eq!(iso_utc(SystemTime::UNIX_EPOCH), "1970-01-01T00:00:00.000Z");
        assert_eq!(iso_utc(SystemTime::UNIX_EPOCH + Duration::from_secs(1_792_713_600 + 3661)), "2026-10-23T01:01:01.000Z");
    }

    #[test]
    fn no_reset_in_the_future() {
        assert!(!display_reset_since(SystemTime::now() + Duration::from_secs(3600)));
    }
}
