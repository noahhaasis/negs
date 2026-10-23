//! Display driver resets (System log, event 4101 from `Display`): a GPU
//! render that returns success while Windows reset the driver is not proof of
//! a good image, as `scripts/golden/benchmark.py` found.

use std::time::SystemTime;

use windows::Win32::System::EventLog::*;
use windows::core::PCWSTR;

use crate::platform::wide;

/// ISO 8601 UTC, as the event log's XPath compares `SystemTime`.
fn iso(time: SystemTime) -> String {
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

/// Whether Windows logged a display driver reset at or after `since`.
pub fn display_reset_since(since: SystemTime) -> bool {
    let query = format!(
        "*[System[Provider[@Name='Display'] and (EventID=4101) and TimeCreated[@SystemTime>='{}']]]",
        iso(since - std::time::Duration::from_secs(1))
    );
    let (channel, query) = (wide("System"), wide(query));
    unsafe {
        let Ok(results) = EvtQuery(None, PCWSTR(channel.as_ptr()), PCWSTR(query.as_ptr()), (EvtQueryChannelPath.0 | EvtQueryReverseDirection.0) as u32) else {
            return false;
        };
        let mut events = [0isize; 1];
        let mut returned = 0u32;
        let found = EvtNext(results, &mut events, 0, 0, &mut returned).is_ok() && returned > 0;
        if found {
            let _ = EvtClose(EVT_HANDLE(events[0]));
        }
        let _ = EvtClose(results);
        found
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[test]
    fn iso_dates() {
        assert_eq!(iso(SystemTime::UNIX_EPOCH), "1970-01-01T00:00:00.000Z");
        assert_eq!(iso(SystemTime::UNIX_EPOCH + Duration::from_secs(1_792_713_600 + 3661)), "2026-10-23T01:01:01.000Z");
    }

    #[test]
    fn no_reset_in_the_future() {
        assert!(!display_reset_since(SystemTime::now() + Duration::from_secs(3600)));
    }
}
