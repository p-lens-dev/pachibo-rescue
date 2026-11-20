//! Reading an iPhone backup as Finder, iTunes and Apple Devices write it:
//! finding the backups a computer holds, saying what each is, unlocking an
//! encrypted one with its password, and taking one app's files out.

mod crypto;
pub mod discovery;
pub mod extract;
pub mod keybag;
pub mod manifest;
pub mod summary;
#[cfg(test)]
mod tests;

use std::path::PathBuf;

/// Why a backup could not be read. Each keeps what happened, never a sentence.
#[derive(Debug)]
pub enum BackupError {
    /// The folder may not be read: on a Mac, the app lacks Full Disk Access.
    PermissionDenied { path: PathBuf },
    /// A file the backup needs is not there.
    Missing { path: PathBuf },
    /// A file could not be read or written for another reason.
    Io { path: PathBuf, error: std::io::Error },
    /// A property list did not read, or lacked what a backup's has.
    Plist { path: PathBuf, detail: String },
    /// The backup is encrypted and no password was given.
    PasswordNeeded,
    /// The password does not unlock the backup.
    WrongPassword,
    /// The keybag or a wrapped key is not as a backup writes it.
    Keybag { detail: String },
    /// Manifest.db did not read as SQLite, or lacked its table.
    Manifest { detail: String },
}

impl BackupError {
    fn io(path: impl Into<PathBuf>, error: std::io::Error) -> Self {
        let path = path.into();
        match error.kind() {
            std::io::ErrorKind::PermissionDenied => Self::PermissionDenied { path },
            std::io::ErrorKind::NotFound => Self::Missing { path },
            _ => Self::Io { path, error },
        }
    }
}

impl std::fmt::Display for BackupError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{self:?}")
    }
}

impl std::error::Error for BackupError {}
