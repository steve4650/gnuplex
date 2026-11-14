use std::path::Path;

use subcue::{ComposeOptions, Format, LineEnding};

/// Shift every cue in a subtitle file by `delta_seconds` (positive means
/// later) and rewrite the file in place.
pub fn shift_subtitle_file(filename: &str, delta_seconds: f64) -> Result<(), String> {
    let path = Path::new(filename);
    let format = match path
        .extension()
        .and_then(|e| e.to_str())
        .map(|e| e.to_lowercase())
        .as_deref()
    {
        Some("srt") => Format::Srt,
        Some("vtt") => Format::Vtt,
        Some("ass") | Some("ssa") => Format::Ass,
        _ => return Err(format!("unsupported subtitle format: {filename}")),
    };

    let bytes = std::fs::read(path).map_err(|e| e.to_string())?;
    let mut subtitles = subcue::parse(&bytes, format).map_err(|e| e.to_string())?;
    let delta_ms = (delta_seconds * 1000.0).trunc() as i64;
    for cue in &mut subtitles.cues {
        cue.start.0 = cue.start.0.saturating_add(delta_ms);
        cue.end.0 = cue.end.0.saturating_add(delta_ms);
    }
    for event in &mut subtitles.events {
        event.start.0 = event.start.0.saturating_add(delta_ms);
        event.end.0 = event.end.0.saturating_add(delta_ms);
    }
    let mut output = subtitles.compose_with(&ComposeOptions {
        line_ending: LineEnding::Lf,
        // astisub always writes a UTF-8 BOM for SRT.
        bom: format == Format::Srt,
    });
    // astisub terminates every cue with a blank line except the final one.
    if format == Format::Srt && output.0.ends_with(b"\n\n") {
        output.0.pop();
    }
    std::fs::write(path, output.0).map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    #[test]
    fn shifts_srt_like_astisub() {
        let dir = std::env::temp_dir().join(format!("gnuplex-sub-test-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("test.srt");
        let mut f = std::fs::File::create(&path).unwrap();
        write!(
            f,
            "1\n00:00:20,000 --> 00:00:25,000\nHello\n\n2\n00:00:30,000 --> 00:00:35,000\nWorld\n"
        )
        .unwrap();
        drop(f);
        shift_subtitle_file(path.to_str().unwrap(), -2.5).unwrap();
        let out = std::fs::read(&path).unwrap();
        assert_eq!(
            out,
            b"\xef\xbb\xbf1\n00:00:17,500 --> 00:00:22,500\nHello\n\n2\n00:00:27,500 --> 00:00:32,500\nWorld\n"
        );
        std::fs::remove_dir_all(&dir).ok();
    }
}
