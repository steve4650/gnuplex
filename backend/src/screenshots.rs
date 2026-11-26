use std::path::{Path, PathBuf};

use chrono::{DateTime, Utc};

use crate::error::{Error, Result};
use crate::models::Screenshot;
use crate::url::escape_path_segment;

pub fn sanitize_screenshot_source(filename: &str) -> String {
    let sanitized = filename.replace(['/', '\\'], "_");
    if sanitized.is_empty() {
        "unknown".to_string()
    } else {
        sanitized
    }
}

pub fn build_screenshot_filename(filename: &str, now: &DateTime<Utc>) -> String {
    format!(
        "{}.{}.png",
        now.format("%Y-%m-%d-%H:%M:%S"),
        sanitize_screenshot_source(filename)
    )
}

/// Build the URL clients use to fetch a screenshot.
pub fn screenshot_url(name: &str) -> String {
    format!("/screenshots/{}", escape_path_segment(name))
}

pub fn take_screenshot(
    mpv: &crate::mpv::Mpv,
    screenshots_dir: &Path,
    now: DateTime<Utc>,
) -> Result<Screenshot> {
    let current_filename = mpv.get_current_filename()?;
    let name = build_screenshot_filename(&current_filename, &now);
    let path = screenshots_dir.join(&name);
    let path = path
        .to_str()
        .ok_or_else(|| Error::NonUtf8Path(path.clone()))?;
    mpv.screenshot_to_file(path)?;
    let metadata = std::fs::metadata(path)?;
    Ok(Screenshot {
        name: name.clone(),
        url: screenshot_url(&name),
        modified_at: system_time_to_utc(metadata.modified()?),
    })
}

pub fn list_recent_screenshots(screenshots_dir: &Path, limit: i64) -> Result<Vec<Screenshot>> {
    let entries = std::fs::read_dir(screenshots_dir)?;
    let mut screenshots = Vec::new();
    for entry in entries {
        let entry = entry?;
        if entry.file_type()?.is_dir() {
            continue;
        }
        let name = entry.file_name().to_string_lossy().to_string();
        if !name.to_lowercase().ends_with(".png") {
            continue;
        }
        let metadata = entry.metadata()?;
        screenshots.push(Screenshot {
            url: screenshot_url(&name),
            name,
            modified_at: system_time_to_utc(metadata.modified()?),
        });
    }
    screenshots.sort_by(|a, b| {
        b.modified_at
            .cmp(&a.modified_at)
            .then_with(|| a.name.cmp(&b.name))
    });
    if let Ok(limit) = usize::try_from(limit)
        && screenshots.len() > limit
    {
        screenshots.truncate(limit);
    }
    Ok(screenshots)
}

pub fn system_time_to_utc(time: std::time::SystemTime) -> DateTime<Utc> {
    DateTime::from(time)
}

pub fn screenshots_dir(static_files: &Path) -> PathBuf {
    static_files.join("screenshots")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn screenshot_filename_matches_expected_format() {
        let now = chrono::TimeZone::with_ymd_and_hms(&Utc, 2026, 3, 29, 14, 5, 6).unwrap();
        assert_eq!(
            build_screenshot_filename("/media/movies/Alien (1979).mkv", &now),
            "2026-03-29-14:05:06._media_movies_Alien (1979).mkv.png"
        );
    }
}
