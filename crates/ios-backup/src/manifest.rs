//! A backup opened: its Manifest.db, decrypted where it is encrypted, and
//! the files it lists, read back as they were on the phone.

use std::io::Cursor;
use std::path::{Path, PathBuf};

use plist::{Dictionary, Value};
use rusqlite::{Connection, OpenFlags};
use tempfile::NamedTempFile;

use crate::BackupError;
use crate::crypto::{decrypt_cbc, strip_padding};
use crate::keybag::{ClassKeys, Keybag};
use crate::summary::read_dictionary;

/// What a listed path is on the phone.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EntryKind {
    File,
    Directory,
    Symlink,
}

/// A path the backup lists, under its domain.
#[derive(Debug, Clone)]
pub struct Entry {
    pub file_id: String,
    pub domain: String,
    pub relative_path: String,
    pub kind: EntryKind,
    pub size: Option<u64>,
    encryption_key: Option<Vec<u8>>,
}

/// A backup opened for reading.
pub struct Backup {
    root: PathBuf,
    keys: Option<ClassKeys>,
    database: Connection,
    _decrypted: Option<NamedTempFile>,
}

#[allow(clippy::needless_pass_by_value)]
fn manifest_error(detail: impl ToString) -> BackupError {
    BackupError::Manifest { detail: detail.to_string() }
}

fn open_read_only(path: &Path) -> Result<Connection, BackupError> {
    Connection::open_with_flags(path, OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX).map_err(manifest_error)
}

fn data<'a>(dictionary: &'a Dictionary, key: &str, path: &Path) -> Result<&'a [u8], BackupError> {
    dictionary
        .get(key)
        .and_then(Value::as_data)
        .ok_or_else(|| BackupError::Plist { path: path.to_owned(), detail: format!("no {key}") })
}

fn kind_of(flags: i64) -> Option<EntryKind> {
    match flags {
        1 => Some(EntryKind::File),
        2 => Some(EntryKind::Directory),
        4 => Some(EntryKind::Symlink),
        _ => None,
    }
}

/// The size and wrapped key an `MBFile`, archived by `NSKeyedArchiver`, holds.
fn file_properties(blob: &[u8]) -> (Option<u64>, Option<Vec<u8>>) {
    let Ok(archive) = Value::from_reader(Cursor::new(blob)) else {
        return (None, None);
    };
    let objects = archive.as_dictionary().and_then(|archive| archive.get("$objects")).and_then(Value::as_array);
    let object = |uid: &Value| -> Option<&Value> {
        let index = usize::try_from(uid.as_uid()?.get()).ok()?;
        objects?.get(index)
    };
    let root = archive
        .as_dictionary()
        .and_then(|archive| archive.get("$top"))
        .and_then(Value::as_dictionary)
        .and_then(|top| top.get("root"))
        .and_then(object)
        .and_then(Value::as_dictionary);
    let size = root.and_then(|root| root.get("Size")).and_then(Value::as_unsigned_integer);
    let key = root
        .and_then(|root| root.get("EncryptionKey"))
        .and_then(object)
        .and_then(Value::as_dictionary)
        .and_then(|key| key.get("NS.data"))
        .and_then(Value::as_data)
        .map(<[u8]>::to_vec);
    (size, key)
}

impl Backup {
    /// The backup in this folder opened, unlocked by the password where it is encrypted.
    ///
    /// # Errors
    /// `PasswordNeeded` where it is encrypted and none was given,
    /// `WrongPassword` where the one given does not unlock it, and any error
    /// reading Manifest.plist or Manifest.db.
    pub fn open(root: &Path, password: Option<&str>) -> Result<Self, BackupError> {
        let manifest_plist = root.join("Manifest.plist");
        let manifest = read_dictionary(&manifest_plist)?;
        let encrypted = manifest.get("IsEncrypted").and_then(Value::as_boolean).unwrap_or(false);
        let database_path = root.join("Manifest.db");
        if !encrypted {
            return Ok(Self { root: root.to_owned(), keys: None, database: open_read_only(&database_path)?, _decrypted: None });
        }
        let password = password.ok_or(BackupError::PasswordNeeded)?;
        let keys = Keybag::parse(data(&manifest, "BackupKeyBag", &manifest_plist)?)?.unlock(password)?;
        let database_key = keys.unwrap_prefixed(data(&manifest, "ManifestKey", &manifest_plist)?)?;
        let sealed = std::fs::read(&database_path).map_err(|error| BackupError::io(&database_path, error))?;
        let opened = decrypt_cbc(&database_key, &sealed).ok_or_else(|| manifest_error("Manifest.db did not decrypt"))?;
        let mut decrypted = NamedTempFile::new().map_err(|error| BackupError::io(std::env::temp_dir(), error))?;
        std::io::Write::write_all(&mut decrypted, &strip_padding(opened)).map_err(|error| BackupError::io(decrypted.path(), error))?;
        Ok(Self { root: root.to_owned(), keys: Some(keys), database: open_read_only(decrypted.path())?, _decrypted: Some(decrypted) })
    }

    /// The paths listed under a domain, such as `AppDomain-jp.garakuta.Pachibo`, by path.
    ///
    /// # Errors
    /// `Manifest` where Manifest.db lacks its `Files` table or does not read.
    pub fn entries(&self, domain: &str) -> Result<Vec<Entry>, BackupError> {
        let mut statement = self
            .database
            .prepare("SELECT fileID, domain, relativePath, flags, file FROM Files WHERE domain = ?1 ORDER BY relativePath")
            .map_err(manifest_error)?;
        let rows = statement
            .query_map([domain], |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?, row.get::<_, String>(2)?, row.get::<_, i64>(3)?, row.get::<_, Option<Vec<u8>>>(4)?))
            })
            .map_err(manifest_error)?;
        rows.filter_map(|row| match row {
            Ok((file_id, domain, relative_path, flags, blob)) => kind_of(flags).map(|kind| {
                let (size, encryption_key) = blob.as_deref().map_or((None, None), file_properties);
                Ok(Entry { file_id, domain, relative_path, kind, size, encryption_key })
            }),
            Err(error) => Some(Err(manifest_error(error))),
        })
        .collect()
    }

    /// The domains of apps' own containers the backup lists, sorted.
    ///
    /// # Errors
    /// `Manifest` where Manifest.db does not read.
    pub fn app_domains(&self) -> Result<Vec<String>, BackupError> {
        let mut statement = self
            .database
            .prepare("SELECT DISTINCT domain FROM Files WHERE domain LIKE 'AppDomain-%' ORDER BY domain")
            .map_err(manifest_error)?;
        let rows = statement.query_map([], |row| row.get::<_, String>(0)).map_err(manifest_error)?;
        rows.map(|row| row.map_err(manifest_error)).collect()
    }

    /// A file's bytes as they were on the phone.
    ///
    /// # Errors
    /// Where its stored copy is missing or unreadable, or does not decrypt.
    pub fn read(&self, entry: &Entry) -> Result<Vec<u8>, BackupError> {
        let shard = entry.file_id.get(..2).ok_or_else(|| manifest_error("a file id is too short"))?;
        let stored_path = self.root.join(shard).join(&entry.file_id);
        let stored = std::fs::read(&stored_path).map_err(|error| BackupError::io(&stored_path, error))?;
        let Some(keys) = &self.keys else {
            return Ok(stored);
        };
        let wrapped = entry.encryption_key.as_deref().ok_or_else(|| manifest_error("an encrypted file has no key"))?;
        let key = keys.unwrap_prefixed(wrapped)?;
        let mut opened = strip_padding(decrypt_cbc(&key, &stored).ok_or_else(|| manifest_error("a file did not decrypt"))?);
        if let Some(size) = entry.size.and_then(|size| usize::try_from(size).ok()) {
            opened.truncate(size);
        }
        Ok(opened)
    }
}
