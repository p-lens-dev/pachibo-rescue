//! One domain's files written out into a folder, under the paths they had.

use std::path::{Component, Path, PathBuf};

use crate::BackupError;
use crate::manifest::{Backup, EntryKind};

/// What a domain written out came to.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Extracted {
    pub files: usize,
    pub directories: usize,
    /// Paths left out: links, and paths that would leave the folder.
    pub skipped: Vec<String>,
}

/// A phone's relative path as a path inside the folder, each character no
/// file system takes put as `_`; none where it would climb out of it.
fn safe_path(relative: &str) -> Option<PathBuf> {
    let cleaned: String = relative.chars().map(|character| if "<>:\"\\|?*".contains(character) || character.is_control() { '_' } else { character }).collect();
    PathBuf::from(cleaned)
        .components()
        .map(|component| match component {
            Component::Normal(part) => Some(part.to_owned()),
            _ => None,
        })
        .collect()
}

/// Every file of the domain written into `out`, under the path it had.
///
/// # Errors
/// Where a file does not read out of the backup or cannot be written.
pub fn extract_domain(backup: &Backup, domain: &str, out: &Path) -> Result<Extracted, BackupError> {
    let mut done = Extracted::default();
    std::fs::create_dir_all(out).map_err(|error| BackupError::io(out, error))?;
    for entry in backup.entries(domain)? {
        if entry.relative_path.is_empty() {
            continue;
        }
        let Some(relative) = safe_path(&entry.relative_path) else {
            done.skipped.push(entry.relative_path);
            continue;
        };
        let target = out.join(relative);
        match entry.kind {
            EntryKind::Directory => {
                std::fs::create_dir_all(&target).map_err(|error| BackupError::io(&target, error))?;
                done.directories += 1;
            }
            EntryKind::File => {
                if let Some(parent) = target.parent() {
                    std::fs::create_dir_all(parent).map_err(|error| BackupError::io(parent, error))?;
                }
                std::fs::write(&target, backup.read(&entry)?).map_err(|error| BackupError::io(&target, error))?;
                done.files += 1;
            }
            EntryKind::Symlink => done.skipped.push(entry.relative_path),
        }
    }
    Ok(done)
}

#[cfg(test)]
mod tests {
    use super::safe_path;

    #[test]
    fn keeps_paths_inside() {
        assert_eq!(safe_path("Library/Preferences/a.plist").unwrap().to_str(), Some("Library/Preferences/a.plist"));
        assert_eq!(safe_path("Documents/a:b?.db").unwrap().to_str(), Some("Documents/a_b_.db"));
        assert!(safe_path("../escape").is_none());
        assert!(safe_path("/etc/passwd").is_none());
        assert!(safe_path("Documents/./x").is_some_and(|path| path.to_str() == Some("Documents/x")));
    }
}
