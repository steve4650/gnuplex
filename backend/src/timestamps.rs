use chrono::{DateTime, NaiveDateTime, SecondsFormat, TimeZone, Utc};
use serde::ser::Serializer;

/// RFC 3339 with nanosecond precision and a `Z` suffix. This is the format the
/// HTTP API has always returned.
pub fn rfc3339_string(dt: &DateTime<Utc>) -> String {
    let base = dt.format("%Y-%m-%dT%H:%M:%S").to_string();
    let nanos = dt.timestamp_subsec_nanos();
    if nanos == 0 {
        format!("{base}Z")
    } else {
        let fraction = format!("{nanos:09}");
        let fraction = fraction.trim_end_matches('0');
        format!("{base}.{fraction}Z")
    }
}

/// Format a timestamp for SQLite storage. SQLite's date functions only
/// understand RFC 3339, so always include an explicit UTC offset.
pub fn to_db_time(dt: &DateTime<Utc>) -> String {
    dt.to_rfc3339_opts(SecondsFormat::AutoSi, false)
}

/// Parse timestamps written by this backend or by the old Go/GORM backend.
pub fn parse_db_time(s: &str) -> Option<DateTime<Utc>> {
    if let Ok(dt) = DateTime::parse_from_rfc3339(s) {
        return Some(dt.with_timezone(&Utc));
    }
    // Legacy rows use a space separator and may omit the offset.
    const LEGACY_FORMATS: &[&str] = &[
        "%Y-%m-%d %H:%M:%S%.f%:z",
        "%Y-%m-%d %H:%M:%S%.f%#z",
        "%Y-%m-%d %H:%M:%S%.f",
    ];
    for format in LEGACY_FORMATS {
        if let Ok(dt) = DateTime::parse_from_str(s, format) {
            return Some(dt.with_timezone(&Utc));
        }
        if let Ok(naive) = NaiveDateTime::parse_from_str(s, format) {
            return Some(Utc.from_utc_datetime(&naive));
        }
    }
    None
}

pub fn serialize_time<S>(dt: &DateTime<Utc>, serializer: S) -> Result<S::Ok, S::Error>
where
    S: Serializer,
{
    serializer.serialize_str(&rfc3339_string(dt))
}

pub fn serialize_optional_time<S>(
    dt: &Option<DateTime<Utc>>,
    serializer: S,
) -> Result<S::Ok, S::Error>
where
    S: Serializer,
{
    match dt {
        Some(dt) => serializer.serialize_some(&rfc3339_string(dt)),
        None => serializer.serialize_none(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Timelike;

    #[test]
    fn formats_rfc3339_with_trimmed_nanos() {
        let dt = Utc
            .with_ymd_and_hms(2026, 11, 14, 16, 15, 15)
            .unwrap()
            .with_nanosecond(127_656_750)
            .unwrap();
        assert_eq!(rfc3339_string(&dt), "2026-11-14T16:15:15.12765675Z");
        assert_eq!(to_db_time(&dt), "2026-11-14T16:15:15.127656750+00:00");
        assert_eq!(
            rfc3339_string(&Utc.with_ymd_and_hms(2026, 11, 14, 16, 15, 15).unwrap()),
            "2026-11-14T16:15:15Z"
        );
    }

    #[test]
    fn parses_legacy_and_current_db_times() {
        let dt = parse_db_time("2026-11-14 16:15:15.151284955+00:00").unwrap();
        assert_eq!(rfc3339_string(&dt), "2026-11-14T16:15:15.151284955Z");
        let dt = parse_db_time("2026-11-14T16:15:15.151284955+00:00").unwrap();
        assert_eq!(rfc3339_string(&dt), "2026-11-14T16:15:15.151284955Z");
        assert!(parse_db_time("not a time").is_none());
    }

    #[test]
    fn db_times_round_trip() {
        let dt = Utc
            .with_ymd_and_hms(2026, 11, 14, 16, 15, 15)
            .unwrap()
            .with_nanosecond(127_656_750)
            .unwrap();
        assert_eq!(parse_db_time(&to_db_time(&dt)), Some(dt));
    }
}
