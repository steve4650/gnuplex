use std::io;

use chrono::{DateTime, NaiveDateTime, SecondsFormat, TimeZone, Utc};
use serde::Serialize;
use serde::ser::Serializer;

/// Format a timestamp exactly like Go's `time.Time.MarshalJSON`
/// (`time.RFC3339Nano`).
pub fn go_time_string(dt: &DateTime<Utc>) -> String {
    let base = dt.format("%Y-%m-%dT%H:%M:%S").to_string();
    let nanos = dt.timestamp_subsec_nanos();
    if nanos == 0 {
        format!("{base}Z")
    } else {
        let frac = format!("{nanos:09}");
        let frac = frac.trim_end_matches('0');
        format!("{base}.{frac}Z")
    }
}

/// Format a timestamp for SQLite storage.
///
/// RFC3339 with a numeric UTC offset is understood by SQLite's date functions,
/// by the old Go/GORM backend, and by this backend, so stored rows remain
/// usable in both directions.
pub fn db_time_string(dt: &DateTime<Utc>) -> String {
    dt.to_rfc3339_opts(SecondsFormat::AutoSi, false)
}

/// Parse timestamp strings written by this backend or the old Go/GORM one.
pub fn parse_db_time(s: &str) -> Option<DateTime<Utc>> {
    if let Ok(dt) = DateTime::parse_from_rfc3339(s) {
        return Some(dt.with_timezone(&Utc));
    }
    // Rows written by GORM use a space separator and may omit the offset.
    const LEGACY_FORMATS: &[&str] = &[
        "%Y-%m-%d %H:%M:%S%.f%:z",
        "%Y-%m-%d %H:%M:%S%.f%#z",
        "%Y-%m-%d %H:%M:%S%.f",
    ];
    for f in LEGACY_FORMATS {
        if let Ok(dt) = DateTime::parse_from_str(s, f) {
            return Some(dt.with_timezone(&Utc));
        }
        if let Ok(naive) = NaiveDateTime::parse_from_str(s, f) {
            return Some(Utc.from_utc_datetime(&naive));
        }
    }
    None
}

pub fn serialize_go_time<S>(dt: &DateTime<Utc>, serializer: S) -> Result<S::Ok, S::Error>
where
    S: Serializer,
{
    serializer.serialize_str(&go_time_string(dt))
}

pub fn serialize_opt_go_time<S>(
    dt: &Option<DateTime<Utc>>,
    serializer: S,
) -> Result<S::Ok, S::Error>
where
    S: Serializer,
{
    match dt {
        Some(dt) => serializer.serialize_some(&go_time_string(dt)),
        None => serializer.serialize_none(),
    }
}

/// Format a float the way Go's encoding/json does.
pub fn go_float_string(v: f64) -> String {
    if v == 0.0 {
        return if v.is_sign_negative() { "-0" } else { "0" }.to_string();
    }
    let abs = v.abs();
    if abs < 1e-6 || abs >= 1e21 {
        // Rust's LowerExp produces a shortest-round-trip mantissa, just like
        // Go's strconv with format 'e'. Add the '+' Go emits on positive
        // exponents.
        let s = format!("{v:e}");
        if let Some(pos) = s.find('e') {
            let (mantissa, exp) = s.split_at(pos);
            let exp = &exp[1..];
            if let Some(positive) = exp.strip_prefix('-') {
                return format!("{mantissa}e-{positive}");
            }
            return format!("{mantissa}e+{exp}");
        }
        s
    } else {
        format!("{v}")
    }
}

/// serde_json formatter which performs the same HTML escaping that
/// encoding/json does by default.
struct GoFormatter;

impl serde_json::ser::Formatter for GoFormatter {
    fn write_string_fragment<W>(&mut self, writer: &mut W, fragment: &str) -> io::Result<()>
    where
        W: ?Sized + io::Write,
    {
        let mut start = 0;
        for (idx, ch) in fragment.char_indices() {
            let escaped = match ch {
                '<' => Some("\\u003c"),
                '>' => Some("\\u003e"),
                '&' => Some("\\u0026"),
                '\u{2028}' => Some("\\u2028"),
                '\u{2029}' => Some("\\u2029"),
                _ => None,
            };
            if let Some(escaped) = escaped {
                writer.write_all(&fragment.as_bytes()[start..idx])?;
                writer.write_all(escaped.as_bytes())?;
                start = idx + ch.len_utf8();
            }
        }
        writer.write_all(&fragment.as_bytes()[start..])
    }
}

pub fn to_go_json<T: Serialize + ?Sized>(value: &T) -> Result<Vec<u8>, serde_json::Error> {
    let mut out = Vec::new();
    let mut serializer = serde_json::Serializer::with_formatter(&mut out, GoFormatter);
    value.serialize(&mut serializer)?;
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Timelike;

    #[test]
    fn go_time_formats_like_go() {
        let dt = Utc
            .with_ymd_and_hms(2026, 11, 14, 16, 15, 15)
            .unwrap()
            .with_nanosecond(127_656_750)
            .unwrap();
        assert_eq!(go_time_string(&dt), "2026-11-14T16:15:15.12765675Z");
        assert_eq!(db_time_string(&dt), "2026-11-14T16:15:15.127656750+00:00");
        assert_eq!(
            go_time_string(&Utc.with_ymd_and_hms(2026, 11, 14, 16, 15, 15).unwrap()),
            "2026-11-14T16:15:15Z"
        );
    }

    #[test]
    fn parses_db_times() {
        // Legacy GORM rows.
        let dt = parse_db_time("2026-11-14 16:15:15.151284955+00:00").unwrap();
        assert_eq!(go_time_string(&dt), "2026-11-14T16:15:15.151284955Z");
        // New RFC3339 rows.
        let dt = parse_db_time("2026-11-14T16:15:15.151284955+00:00").unwrap();
        assert_eq!(go_time_string(&dt), "2026-11-14T16:15:15.151284955Z");
        assert!(parse_db_time("not a time").is_none());
    }

    #[test]
    fn db_times_round_trip() {
        let dt = Utc
            .with_ymd_and_hms(2026, 11, 14, 16, 15, 15)
            .unwrap()
            .with_nanosecond(127_656_750)
            .unwrap();
        assert_eq!(parse_db_time(&db_time_string(&dt)), Some(dt));
    }

    #[test]
    fn floats_match_go() {
        assert_eq!(go_float_string(0.0), "0");
        assert_eq!(go_float_string(1.0), "1");
        assert_eq!(go_float_string(1.5), "1.5");
        assert_eq!(go_float_string(-2.5), "-2.5");
        assert_eq!(go_float_string(1e6), "1000000");
        assert_eq!(go_float_string(1e-7), "1e-7");
        assert_eq!(go_float_string(1e21), "1e+21");
    }
}
