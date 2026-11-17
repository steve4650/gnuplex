use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::timestamps::{serialize_optional_time, serialize_time};

pub const MEDIA_TYPE_FILE: i64 = 1;
pub const MEDIA_TYPE_URL: i64 = 2;

fn queue_id_absent(value: &Option<i64>) -> bool {
    matches!(value, None | Some(0))
}

#[derive(Debug, Clone, Serialize)]
pub struct MediaItem {
    #[serde(rename = "ID")]
    pub id: i64,
    #[serde(rename = "CreatedAt", serialize_with = "serialize_time")]
    pub created_at: DateTime<Utc>,
    #[serde(rename = "UpdatedAt", serialize_with = "serialize_time")]
    pub updated_at: DateTime<Utc>,
    #[serde(rename = "DeletedAt", serialize_with = "serialize_optional_time")]
    pub deleted_at: Option<DateTime<Utc>>,
    #[serde(rename = "Path")]
    pub path: String,
    #[serde(rename = "LastPlayed")]
    pub last_played: String,
    #[serde(rename = "Temp")]
    pub temp: bool,
    #[serde(rename = "Type")]
    pub media_type: i64,
    #[serde(rename = "LastScanUUID")]
    pub last_scan_uuid: String,
    #[serde(rename = "queue_id", skip_serializing_if = "queue_id_absent")]
    pub queue_id: Option<i64>,
}

#[derive(Debug, Clone, Serialize)]
pub struct MediaDir {
    #[serde(rename = "ID")]
    pub id: i64,
    #[serde(rename = "CreatedAt", serialize_with = "serialize_time")]
    pub created_at: DateTime<Utc>,
    #[serde(rename = "UpdatedAt", serialize_with = "serialize_time")]
    pub updated_at: DateTime<Utc>,
    #[serde(rename = "DeletedAt", serialize_with = "serialize_optional_time")]
    pub deleted_at: Option<DateTime<Utc>>,
    #[serde(rename = "Path")]
    pub path: String,
    #[serde(rename = "LastScanned")]
    pub last_scanned: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct FileExtension {
    #[serde(rename = "ID")]
    pub id: i64,
    #[serde(rename = "CreatedAt", serialize_with = "serialize_time")]
    pub created_at: DateTime<Utc>,
    #[serde(rename = "UpdatedAt", serialize_with = "serialize_time")]
    pub updated_at: DateTime<Utc>,
    #[serde(rename = "DeletedAt", serialize_with = "serialize_optional_time")]
    pub deleted_at: Option<DateTime<Utc>>,
    #[serde(rename = "Extension")]
    pub extension: String,
}

/// An mpv subtitle track. This is not stored in the database.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Track {
    pub id: i64,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub title: String,
    #[serde(rename = "type")]
    pub track_type: String,
    pub selected: bool,
    pub external: bool,
    #[serde(rename = "external-filename", skip_serializing_if = "String::is_empty")]
    pub external_filename: String,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct PlaylistEntry {
    pub filename: String,
    pub id: i64,
    pub current: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct Screenshot {
    pub name: String,
    pub url: String,
    #[serde(serialize_with = "serialize_time")]
    pub modified_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize)]
pub struct MediaItemsRes {
    pub res: Vec<MediaItem>,
    pub count: i64,
}

#[derive(Debug, Clone, Serialize)]
pub struct VersionInfo {
    pub version: String,
    pub source_hash: String,
    pub platform: String,
    pub compiler_version: String,
}

/*
 * Request bodies. Absent fields use the type's default rather than being
 * rejected, so partial request bodies stay valid.
 */

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct PlayMediaBody {
    pub id: u64,
    pub play_next: bool,
    pub play_last: bool,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct CastBody {
    pub url: String,
    pub temp: bool,
    pub play_next: bool,
    pub play_last: bool,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct VolBody {
    pub vol: i64,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct PosBody {
    pub pos: i64,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct SubBody {
    pub visible: bool,
    pub id: i64,
    pub delay: f64,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct FilterBody {
    pub filter: String,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct SkipBody {
    pub skip: i64,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn missing_fields_default_to_zero_values() {
        // Absent fields keep their zero values. Same for mpv responses.
        let body: PlayMediaBody = serde_json::from_str(r#"{"play_next":true}"#).unwrap();
        assert_eq!(body.id, 0);
        assert!(body.play_next);
        assert!(!body.play_last);

        let track: Track = serde_json::from_str(r#"{"id":3,"type":"sub"}"#).unwrap();
        assert_eq!(track.id, 3);
        assert_eq!(track.track_type, "sub");
        assert_eq!(track.title, "");
        assert!(!track.external);
    }
}
