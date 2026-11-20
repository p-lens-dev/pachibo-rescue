//! Where a computer keeps its iPhone backups, and which folders are backups.

use std::path::{Path, PathBuf};

use crate::BackupError;

/// The folders Finder, iTunes and Apple Devices keep backups in on this
/// computer, whether or not they exist. None on Linux, where a backup is
/// wherever `idevicebackup2` was told to put it.
#[must_use]
pub fn default_roots() -> Vec<PathBuf> {
    let env = |name: &str| std::env::var_os(name).map(PathBuf::from);
    if cfg!(target_os = "macos") {
        env("HOME").map(|home| home.join("Library/Application Support/MobileSync/Backup")).into_iter().collect()
    } else if cfg!(target_os = "windows") {
        [
            env("APPDATA").map(|app_data| app_data.join("Apple Computer").join("MobileSync").join("Backup")),
            env("USERPROFILE").map(|profile| profile.join("Apple").join("MobileSync").join("Backup")),
        ]
        .into_iter()
        .flatten()
        .collect()
    } else {
        Vec::new()
    }
}

fn is_backup(path: &Path) -> bool {
    path.join("Manifest.plist").is_file()
}

/// The backups in a folder: the folder itself where it is one, else each
/// folder inside it that is one, sorted. A root that does not exist holds none.
///
/// # Errors
/// `PermissionDenied` where the folder may not be read, as on a Mac before
/// the app is given Full Disk Access.
pub fn backups_in(root: &Path) -> Result<Vec<PathBuf>, BackupError> {
    if is_backup(root) {
        return Ok(vec![root.to_owned()]);
    }
    let entries = match std::fs::read_dir(root) {
        Ok(entries) => entries,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(error) => return Err(BackupError::io(root, error)),
    };
    let mut found: Vec<PathBuf> = entries.filter_map(Result::ok).map(|entry| entry.path()).filter(|path| is_backup(path)).collect();
    found.sort();
    Ok(found)
}
