use std::fmt::Write as _;

/// Percent-encode a single URL path segment.
pub fn escape_path_segment(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    for byte in value.as_bytes() {
        let byte = *byte;
        let allowed = byte.is_ascii_alphanumeric()
            || matches!(
                byte,
                b'-' | b'.' | b'_' | b'~' | b'$' | b'&' | b'+' | b',' | b';' | b':' | b'=' | b'@'
            );
        if allowed {
            out.push(byte as char);
        } else {
            write!(out, "%{byte:02X}").expect("writing to a String cannot fail");
        }
    }
    out
}

/// Escape a segment for use as the first component of a relative URL. A
/// leading `./` keeps a colon from being mistaken for a URL scheme.
pub fn escape_relative_segment(value: &str) -> String {
    let escaped = escape_path_segment(value);
    if escaped.contains(':') {
        format!("./{escaped}")
    } else {
        escaped
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn path_escaping_matches_url_path_escape() {
        assert_eq!(
            escape_path_segment("2026-03-29-14:05:06._media_movies_Alien (1979).mkv.png"),
            "2026-03-29-14:05:06._media_movies_Alien%20%281979%29.mkv.png"
        );
        assert_eq!(escape_path_segment("a+b&c=d.png"), "a+b&c=d.png");
        assert_eq!(escape_path_segment("a#b?.png"), "a%23b%3F.png");
        assert_eq!(escape_path_segment("héllo.png"), "h%C3%A9llo.png");
    }

    #[test]
    fn relative_segments_with_colons_get_a_dot_prefix() {
        assert_eq!(escape_relative_segment("a:b"), "./a:b");
        assert_eq!(escape_relative_segment("plain"), "plain");
    }
}
