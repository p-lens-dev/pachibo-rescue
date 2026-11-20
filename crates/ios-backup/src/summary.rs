//! What a backup says of itself in Info.plist and Manifest.plist, read
//! without any password: whose device, when, whether it is encrypted, and
//! which apps it holds.

use std::path::{Path, PathBuf};
use std::time::UNIX_EPOCH;

use plist::{Dictionary, Value};

use crate::BackupError;

/// A backup as it describes itself.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Summary {
    pub path: PathBuf,
    pub device_name: Option<String>,
    pub product_type: Option<String>,
    pub ios_version: Option<String>,
    /// When it was made, in UNIX seconds.
    pub made_at: Option<i64>,
    pub encrypted: bool,
    /// The bundle ids of the apps it holds, in order.
    pub apps: Vec<String>,
}

pub(crate) fn read_dictionary(path: &Path) -> Result<Dictionary, BackupError> {
    let bytes = std::fs::read(path).map_err(|error| BackupError::io(path, error))?;
    Value::from_reader(std::io::Cursor::new(bytes))
        .map_err(|error| BackupError::Plist { path: path.to_owned(), detail: error.to_string() })?
        .into_dictionary()
        .ok_or_else(|| BackupError::Plist { path: path.to_owned(), detail: "not a dictionary".to_owned() })
}

fn text(dictionary: &Dictionary, key: &str) -> Option<String> {
    dictionary.get(key).and_then(Value::as_string).map(str::to_owned)
}

fn unix_seconds(value: &Value) -> Option<i64> {
    let time: std::time::SystemTime = value.as_date()?.into();
    i64::try_from(time.duration_since(UNIX_EPOCH).ok()?.as_secs()).ok()
}

/// What the backup in this folder says of itself.
///
/// # Errors
/// Where Manifest.plist is missing, unreadable or no property list.
/// Info.plist is read where it is there and readable, and left out where not.
pub fn summarize(path: &Path) -> Result<Summary, BackupError> {
    let manifest = read_dictionary(&path.join("Manifest.plist"))?;
    let info = read_dictionary(&path.join("Info.plist")).unwrap_or_default();
    let mut apps: Vec<String> = manifest
        .get("Applications")
        .and_then(Value::as_dictionary)
        .map(|applications| applications.keys().cloned().collect())
        .unwrap_or_default();
    apps.sort();
    Ok(Summary {
        path: path.to_owned(),
        device_name: text(&info, "Device Name").or_else(|| text(&info, "Display Name")),
        product_type: text(&info, "Product Type"),
        ios_version: text(&info, "Product Version"),
        made_at: info.get("Last Backup Date").and_then(unix_seconds),
        encrypted: manifest.get("IsEncrypted").and_then(Value::as_boolean).unwrap_or(false),
        apps,
    })
}
