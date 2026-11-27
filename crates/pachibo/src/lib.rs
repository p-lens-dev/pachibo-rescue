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

/// The day a day's count is kept for (`20261026` of `item20261026_count`); none where the key is no day's count.
fn day_of(key: &str) -> Option<&str> {
    let digits = key.strip_prefix("item")?.strip_suffix("_count")?;
    (digits.len() == 8 && digits.bytes().all(|byte| byte.is_ascii_digit())).then_some(digits)
}

/// How many tallies a day keeps, where the key is a day's count (`item20261026_count`).
fn day_count(key: &str, value: &str) -> Option<u64> {
    day_of(key).and_then(|_| value.parse().ok())
}

/// The sums of yen a tally keeps, savings and cash, in and back.
const SUMS: [&str; 4] = ["invest", "invest2", "recovery", "recovery2"];

/// The keys of the sums each tally keeps, by the days' counts: a day of two
/// keeps `item20261026_0_invest`, …, `item20261026_1_recovery2`.
fn sum_keys(days: &[(String, u64)]) -> Vec<String> {
    days.iter()
        .flat_map(|(day, count)| (0..*count).flat_map(move |index| SUMS.iter().map(move |sum| format!("item{day}_{index}_{sum}"))))
        .collect()
}

/// ぱち簿's `data.plist` as its `.pcb` file. A sum ぱち簿 left blank —
/// kept as an empty text, or not kept at all — is written as `0`, as
/// ぱち簿 itself shows it, so that P-Lens takes the tally in.
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
    let values: Vec<(String, String, String)> = user
        .iter()
        .filter_map(|(typed_key, value)| {
            let (key, kind) = typed_key.rsplit_once(':')?;
            Some((kind.to_owned(), key.to_owned(), text_of(value)?))
        })
        .collect();
    let days: Vec<(String, u64)> = values.iter().filter_map(|(_, key, value)| Some((day_of(key)?.to_owned(), day_count(key, value)?))).collect();
    let sums = sum_keys(&days);
    let blank = |key: &str, value: &str| value.trim().is_empty() && sums.iter().any(|sum| sum == key);
    let kept = values.iter().filter(|(_, key, value)| !blank(key, value)).map(|(kind, key, value)| (kind.as_str(), key.as_str(), value.as_str()));
    let filled = sums.iter().filter(|sum| !values.iter().any(|(_, key, value)| key == *sum && !blank(key, value))).map(|sum| ("Integer", sum.as_str(), "0"));
    let mut text = String::new();
    for (kind, key, value) in kept.chain(filled) {
        let _ = write!(text, "{kind},{key},{},{value},", value.chars().count());
    }
    Ok(Rescued { pcb: text.into_bytes().into_iter().map(|byte| 255 - byte).collect(), tallies: days.iter().map(|(_, count)| count).sum() })
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
            "Integer,item20261026_count,1,2,Integer,item20261028_count,1,1,String,item20261026_0_memo,7,めも,改行\nも,Boolean,graph,4,true,\
             Integer,item20261026_0_invest,1,0,Integer,item20261026_0_invest2,1,0,Integer,item20261026_0_recovery,1,0,Integer,item20261026_0_recovery2,1,0,\
             Integer,item20261026_1_invest,1,0,Integer,item20261026_1_invest2,1,0,Integer,item20261026_1_recovery,1,0,Integer,item20261026_1_recovery2,1,0,\
             Integer,item20261028_0_invest,1,0,Integer,item20261028_0_invest2,1,0,Integer,item20261028_0_recovery,1,0,Integer,item20261028_0_recovery2,1,0,"
        );
    }

    #[test]
    fn writes_a_sum_left_blank_as_nothing() {
        let rescued = rescue_data(&data_plist(&[
            ("item20261106_count:Integer", "2"),
            ("item20261106_0_invest:Integer", ""),
            ("item20261106_0_invest2:Integer", "10000"),
            ("item20261106_0_recovery2:Integer", "3000"),
            ("item20261106_1_invest:Integer", "1000"),
        ]))
        .unwrap();
        assert_eq!(rescued.tallies, 2);
        assert_eq!(
            turned_back(&rescued.pcb),
            "Integer,item20261106_count,1,2,Integer,item20261106_0_invest2,5,10000,Integer,item20261106_0_recovery2,4,3000,Integer,item20261106_1_invest,4,1000,\
             Integer,item20261106_0_invest,1,0,Integer,item20261106_0_recovery,1,0,\
             Integer,item20261106_1_invest2,1,0,Integer,item20261106_1_recovery,1,0,Integer,item20261106_1_recovery2,1,0,"
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
