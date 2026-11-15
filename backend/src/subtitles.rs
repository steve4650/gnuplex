use std::path::Path;

use subcue::{ComposeOptions, Format, LineEnding};

use crate::error::{Error, Result};

/// Shift every cue in a subtitle file by `delta_seconds` (positive means
/// later) and rewrite the file in place.
pub fn shift_subtitle_file(path: &Path, delta_seconds: f64) -> Result<()> {
    let format = match path
        .extension()
        .and_then(|extension| extension.to_str())
        .map(str::to_lowercase)
        .as_deref()
    {
        Some("srt") => Format::Srt,
        Some("vtt") => Format::Vtt,
        Some("ass") | Some("ssa") => Format::Ass,
        _ => {
            return Err(Error::Subtitle(format!(
                "unsupported subtitle format: {}",
                path.display()
            )));
        }
    };

    let bytes = std::fs::read(path)?;
    let mut subtitles =
        subcue::parse(&bytes, format).map_err(|err| Error::Subtitle(err.to_string()))?;
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
    std::fs::write(path, output.0)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    #[test]
    fn shifts_srt_like_astisub() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("test.srt");
        let mut file = std::fs::File::create(&path).unwrap();
        write!(
            file,
            "1\n00:00:20,000 --> 00:00:25,000\nHello\n\n2\n00:00:30,000 --> 00:00:35,000\nWorld\n"
        )
        .unwrap();
        drop(file);
        shift_subtitle_file(&path, -2.5).unwrap();
        let out = std::fs::read(&path).unwrap();
        assert_eq!(
            out,
            b"\xef\xbb\xbf1\n00:00:17,500 --> 00:00:22,500\nHello\n\n2\n00:00:27,500 --> 00:00:32,500\nWorld\n"
        );
    }
}
