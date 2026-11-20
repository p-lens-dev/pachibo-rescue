use std::path::Path;

use plist::{Dictionary, Uid, Value};

use crate::BackupError;
use crate::crypto::tests::{encrypt_cbc, wrap_key};
use crate::discovery::backups_in;
use crate::extract::extract_domain;
use crate::manifest::Backup;
use crate::summary::summarize;

const DOMAIN: &str = "AppDomain-jp.garakuta.Pachibo";
const FILE_ID: &str = "ab0123456789abcdef0123456789abcdef012345";
const CONTENT: &[u8] = b"Integer,count,1,7,";

fn dictionary(pairs: Vec<(&str, Value)>) -> Value {
    Value::Dictionary(pairs.into_iter().map(|(key, value)| (key.to_owned(), value)).collect::<Dictionary>())
}

fn file_blob(size: u64, encryption_key: Option<Vec<u8>>) -> Vec<u8> {
    let mut file = vec![("Size", Value::from(size)), ("ProtectionClass", Value::from(3u64))];
    let mut objects = vec![Value::from("$null")];
    if let Some(key) = encryption_key {
        file.push(("EncryptionKey", Value::Uid(Uid::new(2))));
        objects.push(dictionary(file));
        objects.push(dictionary(vec![("NS.data", Value::Data(key))]));
    } else {
        objects.push(dictionary(file));
    }
    let archive = dictionary(vec![("$top", dictionary(vec![("root", Value::Uid(Uid::new(1)))])), ("$objects", Value::Array(objects))]);
    let mut bytes = Vec::new();
    archive.to_writer_binary(&mut bytes).unwrap();
    bytes
}

fn manifest_db(path: &Path, blob: &[u8]) {
    let connection = rusqlite::Connection::open(path).unwrap();
    connection
        .execute_batch("CREATE TABLE Files (fileID TEXT PRIMARY KEY, domain TEXT, relativePath TEXT, flags INTEGER, file BLOB);")
        .unwrap();
    connection
        .execute("INSERT INTO Files VALUES (?1, ?2, '', 2, NULL), ('cd00', ?2, 'Library', 2, NULL), ('ef00', ?2, '../escape', 1, NULL), (?3, ?2, 'Library/Preferences/jp.garakuta.Pachibo.plist', 1, ?4)", rusqlite::params!["00ff", DOMAIN, FILE_ID, blob])
        .unwrap();
}

fn store(root: &Path, data: &[u8]) {
    let shard = root.join(&FILE_ID[..2]);
    std::fs::create_dir_all(&shard).unwrap();
    std::fs::write(shard.join(FILE_ID), data).unwrap();
}

fn write_plists(root: &Path, manifest: Vec<(&str, Value)>) {
    let mut manifest = manifest;
    manifest.push(("Applications", dictionary(vec![("jp.garakuta.Pachibo", dictionary(vec![]))])));
    dictionary(manifest).to_file_binary(root.join("Manifest.plist")).unwrap();
    dictionary(vec![("Device Name", Value::from("Test iPhone")), ("Product Version", Value::from("18.1"))])
        .to_file_xml(root.join("Info.plist"))
        .unwrap();
}

fn unencrypted(root: &Path) {
    write_plists(root, vec![("IsEncrypted", Value::Boolean(false))]);
    manifest_db(&root.join("Manifest.db"), &file_blob(CONTENT.len() as u64, None));
    store(root, CONTENT);
}

fn encrypted(root: &Path, password: &str) {
    let (class_key, database_key, file_key) = ([3u8; 32], [4u8; 32], [5u8; 32]);
    let prefixed = |key: &[u8; 32]| [3u32.to_le_bytes().to_vec(), wrap_key(&class_key, key)].concat();
    write_plists(
        root,
        vec![
            ("IsEncrypted", Value::Boolean(true)),
            ("BackupKeyBag", Value::Data(crate::keybag::tests::sample(password, &[(3, class_key)]))),
            ("ManifestKey", Value::Data(prefixed(&database_key))),
        ],
    );
    let plain = tempfile::NamedTempFile::new().unwrap();
    manifest_db(plain.path(), &file_blob(CONTENT.len() as u64, Some(prefixed(&file_key))));
    std::fs::write(root.join("Manifest.db"), encrypt_cbc(&database_key, &std::fs::read(plain.path()).unwrap())).unwrap();
    store(root, &encrypt_cbc(&file_key, CONTENT));
}

fn extracted_content(backup: &Backup) -> (crate::extract::Extracted, Vec<u8>) {
    let out = tempfile::tempdir().unwrap();
    let done = extract_domain(backup, DOMAIN, out.path()).unwrap();
    (done, std::fs::read(out.path().join("Library/Preferences/jp.garakuta.Pachibo.plist")).unwrap())
}

#[test]
fn reads_an_unencrypted_backup() {
    let parent = tempfile::tempdir().unwrap();
    let root = parent.path().join("00008110-TEST");
    std::fs::create_dir(&root).unwrap();
    unencrypted(&root);
    assert_eq!(backups_in(parent.path()).unwrap(), vec![root.clone()]);
    let summary = summarize(&root).unwrap();
    assert!(!summary.encrypted);
    assert_eq!(summary.device_name.as_deref(), Some("Test iPhone"));
    assert_eq!(summary.apps, vec!["jp.garakuta.Pachibo".to_owned()]);
    let backup = Backup::open(&root, None).unwrap();
    assert_eq!(backup.app_domains().unwrap(), vec![DOMAIN.to_owned()]);
    let (done, content) = extracted_content(&backup);
    assert_eq!(content, CONTENT);
    assert_eq!((done.files, done.directories, done.skipped), (1, 1, vec!["../escape".to_owned()]));
}

#[test]
fn reads_an_encrypted_backup_with_its_password() {
    let root = tempfile::tempdir().unwrap();
    encrypted(root.path(), "secret");
    assert!(summarize(root.path()).unwrap().encrypted);
    assert!(matches!(Backup::open(root.path(), None), Err(BackupError::PasswordNeeded)));
    assert!(matches!(Backup::open(root.path(), Some("wrong")), Err(BackupError::WrongPassword)));
    let backup = Backup::open(root.path(), Some("secret")).unwrap();
    let (_, content) = extracted_content(&backup);
    assert_eq!(content, CONTENT);
}
