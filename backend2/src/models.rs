use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::gotime::{serialize_go_time, serialize_opt_go_time};

pub const MEDIA_TYPE_FILE: i64 = 1;
pub const MEDIA_TYPE_URL: i64 = 2;

fn queue_id_absent(value: &Option<i64>) -> bool {
    match value {
        None | Some(0) => true,
        Some(_) => false,
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct MediaItem {
    #[serde(rename = "ID")]
    pub id: i64,
    #[serde(rename = "CreatedAt", serialize_with = "serialize_go_time")]
    pub created_at: DateTime<Utc>,
    #[serde(rename = "UpdatedAt", serialize_with = "serialize_go_time")]
    pub updated_at: DateTime<Utc>,
    #[serde(rename = "DeletedAt", serialize_with = "serialize_opt_go_time")]
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
    #[serde(rename = "CreatedAt", serialize_with = "serialize_go_time")]
    pub created_at: DateTime<Utc>,
    #[serde(rename = "UpdatedAt", serialize_with = "serialize_go_time")]
    pub updated_at: DateTime<Utc>,
    #[serde(rename = "DeletedAt", serialize_with = "serialize_opt_go_time")]
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
    #[serde(rename = "CreatedAt", serialize_with = "serialize_go_time")]
    pub created_at: DateTime<Utc>,
    #[serde(rename = "UpdatedAt", serialize_with = "serialize_go_time")]
    pub updated_at: DateTime<Utc>,
    #[serde(rename = "DeletedAt", serialize_with = "serialize_opt_go_time")]
    pub deleted_at: Option<DateTime<Utc>>,
    #[serde(rename = "Extension")]
    pub extension: String,
}

/// An mpv subtitle track. This is not stored in the database.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Track {
    #[serde(default)]
    pub id: i64,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub title: String,
    #[serde(rename = "type", default)]
    pub track_type: String,
    #[serde(default)]
    pub selected: bool,
    #[serde(default)]
    pub external: bool,
    #[serde(
        rename = "external-filename",
        default,
        skip_serializing_if = "String::is_empty"
    )]
    pub external_filename: String,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct PlaylistEntry {
    #[serde(default)]
    pub filename: String,
    #[serde(default)]
    pub id: i64,
    #[serde(default)]
    pub current: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct Screenshot {
    pub name: String,
    pub url: String,
    #[serde(serialize_with = "serialize_go_time")]
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
    pub go_version: String,
}

/*
 * Request bodies. Go's encoding/json leaves a struct untouched for a JSON
 * `null` body and zeroes absent fields, so these all default every field.
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
