//! ぱち簿's records rescued from an iPhone backup.
//!
//! ぱち簿 keeps everything in `Documents/data.plist`: one dictionary,
//! `user`, of values by `key:Type`, each value a text. Its own export, the
//! `.pcb` file, is the same values written one after another as
//! `Type,key,length,value,` — the length in characters — encoded as UTF-8,
//! each byte turned over (`255 - byte`). P-Lens takes that file in.

use std::fmt::Write as _;

use ios_backup::BackupError;
use ios_backup::manifest::{Backup, EntryKind};
use plist::Value;

/// ぱち簿's bundle id on the App Store.
pub const BUNDLE_ID: &str = "jp.garakuta.Pachibo";

/// The domain a backup keeps ぱち簿's container under.
pub const DOMAIN: &str = "AppDomain-jp.garakuta.Pachibo";

/// Where in its container ぱち簿 keeps its records.
pub const DATA_PATH: &str = "Documents/data.plist";

/// Why ぱち簿's records could not be rescued.
#[derive(Debug)]
pub enum RescueError {
    /// The backup holds no file of ぱち簿's records.
    NotInBackup,
    /// `data.plist` is not a property list holding a `user` dictionary.
    NotPachiboData { detail: String },
    /// The backup itself could not be read.
    Backup(BackupError),
}

impl From<BackupError> for RescueError {
    fn from(error: BackupError) -> Self {
        Self::Backup(error)
    }
}

impl std::fmt::Display for RescueError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{self:?}")
    }
}

impl std::error::Error for RescueError {}

/// ぱち簿's records as its `.pcb` file, and how many tallies they hold.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Rescued {
    pub pcb: Vec<u8>,
    pub tallies: u64,
}

fn not_pachibo(detail: &str) -> RescueError {
    RescueError::NotPachiboData { detail: detail.to_owned() }
}

/// A value as the text ぱち簿 keeps it as; none where it is no plain value.
fn text_of(value: &Value) -> Option<String> {
    match value {
        Value::String(text) => Some(text.clone()),
        Value::Integer(number) => Some(number.to_string()),
        Value::Boolean(truth) => Some(truth.to_string()),
        Value::Real(number) => Some(number.to_string()),
        _ => None,
    }
}

/// How many tallies a day keeps, where the key is a day's count (`item20261026_count`).
fn day_count(key: &str, value: &str) -> Option<u64> {
    let digits = key.strip_prefix("item")?.strip_suffix("_count")?;
    (digits.len() == 8 && digits.bytes().all(|byte| byte.is_ascii_digit())).then(|| value.parse().ok()).flatten()
}

/// ぱち簿's `data.plist` as its `.pcb` file.
///
/// # Errors
/// `NotPachiboData` where the bytes are no property list with a `user` dictionary.
pub fn rescue_data(data_plist: &[u8]) -> Result<Rescued, RescueError> {
    let root = Value::from_reader(std::io::Cursor::new(data_plist)).map_err(|error| not_pachibo(&error.to_string()))?;
    let user = root
        .as_dictionary()
        .and_then(|root| root.get("user"))
        .and_then(Value::as_dictionary)
        .ok_or_else(|| not_pachibo("no user dictionary"))?;
    let mut text = String::new();
    let mut tallies = 0u64;
    for (typed_key, value) in user {
        let (Some((key, kind)), Some(value)) = (typed_key.rsplit_once(':'), text_of(value)) else {
            continue;
        };
        tallies += day_count(key, &value).unwrap_or(0);
        let _ = write!(text, "{kind},{key},{},{value},", value.chars().count());
    }
    Ok(Rescued { pcb: text.into_bytes().into_iter().map(|byte| 255 - byte).collect(), tallies })
}

/// ぱち簿's records out of an opened backup, as its `.pcb` file.
///
/// # Errors
/// `NotInBackup` where the backup holds no `data.plist` of ぱち簿's, and
/// any error reading or decrypting it.
pub fn rescue(backup: &Backup) -> Result<Rescued, RescueError> {
    let entry = backup
        .entries(DOMAIN)?
        .into_iter()
        .find(|entry| entry.kind == EntryKind::File && entry.relative_path == DATA_PATH)
        .ok_or(RescueError::NotInBackup)?;
    rescue_data(&backup.read(&entry)?)
}

#[cfg(test)]
mod tests {
    use plist::{Dictionary, Value};

    use super::rescue_data;

    fn data_plist(pairs: &[(&str, &str)]) -> Vec<u8> {
        let user: Dictionary = pairs.iter().map(|(key, value)| ((*key).to_owned(), Value::from(*value))).collect();
        let mut root = Dictionary::new();
        root.insert("user".to_owned(), Value::Dictionary(user));
        let mut bytes = Vec::new();
        Value::Dictionary(root).to_writer_xml(&mut bytes).unwrap();
        bytes
    }

    fn turned_back(pcb: &[u8]) -> String {
        String::from_utf8(pcb.iter().map(|byte| 255 - byte).collect()).unwrap()
    }

    #[test]
    fn writes_each_value_as_pachibo_does() {
        let rescued = rescue_data(&data_plist(&[
            ("item20261026_count:Integer", "2"),
            ("item20261028_count:Integer", "1"),
            ("item20261026_0_memo:String", "めも,改行\nも"),
            ("graph:Boolean", "true"),
            ("no type", "left out"),
        ]))
        .unwrap();
        assert_eq!(rescued.tallies, 3);
        assert_eq!(
            turned_back(&rescued.pcb),
            "Integer,item20261026_count,1,2,Integer,item20261028_count,1,1,String,item20261026_0_memo,7,めも,改行\nも,Boolean,graph,4,true,"
        );
    }

    #[test]
    fn refuses_what_is_not_pachibo() {
        assert!(rescue_data(b"not a plist").is_err());
        let mut bytes = Vec::new();
        Value::Dictionary(Dictionary::new()).to_writer_xml(&mut bytes).unwrap();
        assert!(rescue_data(&bytes).is_err());
    }
}
