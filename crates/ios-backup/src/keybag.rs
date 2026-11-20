//! The keybag in Manifest.plist: a run of tag–length–value records, the
//! bag's own first and then one group per protection class, each begun by
//! its `UUID`. A password opens it by PBKDF2 twice, and then unwraps each
//! class key the password wraps.

use std::collections::BTreeMap;

use crate::BackupError;
use crate::crypto::unwrap_key;

const WRAP_PASSCODE: u32 = 2;

/// A keybag as read, before any password is tried.
#[derive(Debug, Clone)]
pub struct Keybag {
    salt: Vec<u8>,
    iterations: u32,
    double_protection_salt: Vec<u8>,
    double_protection_iterations: u32,
    classes: Vec<WrappedClass>,
}

#[derive(Debug, Clone)]
struct WrappedClass {
    class: u32,
    wrap: u32,
    wrapped_key: Vec<u8>,
}

/// The class keys a password unwrapped, by protection class.
#[derive(Debug, Clone)]
pub struct ClassKeys(BTreeMap<u32, [u8; 32]>);

impl ClassKeys {
    /// A key wrapped under a protection class unwrapped: the 4-byte
    /// little-endian class, then the wrapped key, as backups store them.
    pub(crate) fn unwrap_prefixed(&self, prefixed: &[u8]) -> Result<Vec<u8>, BackupError> {
        let (class_bytes, wrapped) = prefixed
            .split_first_chunk::<4>()
            .ok_or_else(|| keybag_error("a wrapped key is too short"))?;
        self.unwrap_for(u32::from_le_bytes(*class_bytes), wrapped)
    }

    pub(crate) fn unwrap_for(&self, class: u32, wrapped: &[u8]) -> Result<Vec<u8>, BackupError> {
        let kek = self
            .0
            .get(&class)
            .ok_or_else(|| keybag_error(&format!("no key for protection class {class}")))?;
        unwrap_key(kek, wrapped).ok_or_else(|| keybag_error("a file key did not unwrap"))
    }
}

fn keybag_error(detail: &str) -> BackupError {
    BackupError::Keybag { detail: detail.to_owned() }
}

type Record<'a> = ([u8; 4], &'a [u8]);

fn records(bytes: &[u8]) -> Result<Vec<Record<'_>>, BackupError> {
    let mut found = Vec::new();
    let mut rest = bytes;
    while !rest.is_empty() {
        let (tag, after_tag) = rest.split_first_chunk::<4>().ok_or_else(|| keybag_error("a record is cut short"))?;
        let (length, after_length) = after_tag.split_first_chunk::<4>().ok_or_else(|| keybag_error("a record is cut short"))?;
        let length = usize::try_from(u32::from_be_bytes(*length)).map_err(|_| keybag_error("a record is too long"))?;
        let (value, after_value) = after_length.split_at_checked(length).ok_or_else(|| keybag_error("a record runs past the end"))?;
        found.push((*tag, value));
        rest = after_value;
    }
    Ok(found)
}

fn number(value: &[u8]) -> u32 {
    value.iter().fold(0u32, |acc, &byte| acc.wrapping_shl(8) | u32::from(byte))
}

impl Keybag {
    /// A keybag read from its bytes.
    ///
    /// # Errors
    /// `Keybag` where the records do not read or the bag lacks its salts.
    pub fn parse(bytes: &[u8]) -> Result<Self, BackupError> {
        let mut top: BTreeMap<[u8; 4], &[u8]> = BTreeMap::new();
        let mut classes: Vec<BTreeMap<[u8; 4], &[u8]>> = Vec::new();
        let mut seen_uuid = false;
        for (tag, value) in records(bytes)? {
            if &tag == b"UUID" {
                if seen_uuid {
                    classes.push(BTreeMap::new());
                } else {
                    seen_uuid = true;
                }
            }
            match classes.last_mut() {
                Some(class) => class.insert(tag, value),
                None => top.insert(tag, value),
            };
        }
        let field = |tag: &[u8; 4]| top.get(tag).copied().ok_or_else(|| keybag_error(&format!("the bag lacks {}", String::from_utf8_lossy(tag))));
        Ok(Self {
            salt: field(b"SALT")?.to_vec(),
            iterations: number(field(b"ITER")?),
            double_protection_salt: field(b"DPSL")?.to_vec(),
            double_protection_iterations: number(field(b"DPIC")?),
            classes: classes
                .into_iter()
                .filter_map(|class| {
                    Some(WrappedClass {
                        class: number(class.get(b"CLAS")?),
                        wrap: class.get(b"WRAP").map_or(0, |value| number(value)),
                        wrapped_key: class.get(b"WPKY")?.to_vec(),
                    })
                })
                .collect(),
        })
    }

    /// The class keys the password unwraps.
    ///
    /// # Errors
    /// `WrongPassword` where a class key fails its integrity check.
    pub fn unlock(&self, password: &str) -> Result<ClassKeys, BackupError> {
        let mut first = [0u8; 32];
        pbkdf2::pbkdf2_hmac::<sha2::Sha256>(password.as_bytes(), &self.double_protection_salt, self.double_protection_iterations, &mut first);
        let mut key = [0u8; 32];
        pbkdf2::pbkdf2_hmac::<sha1::Sha1>(&first, &self.salt, self.iterations, &mut key);
        self.unlock_with(&key)
    }

    fn unlock_with(&self, key: &[u8; 32]) -> Result<ClassKeys, BackupError> {
        self.classes
            .iter()
            .filter(|class| class.wrap & WRAP_PASSCODE != 0)
            .map(|class| {
                let unwrapped = unwrap_key(key, &class.wrapped_key).ok_or(BackupError::WrongPassword)?;
                let fixed: [u8; 32] = unwrapped.try_into().map_err(|_| keybag_error("a class key is not 32 bytes"))?;
                Ok((class.class, fixed))
            })
            .collect::<Result<BTreeMap<_, _>, _>>()
            .map(ClassKeys)
    }
}

#[cfg(test)]
pub mod tests {
    use super::Keybag;
    use crate::crypto::tests::wrap_key;

    fn record(tag: [u8; 4], value: &[u8]) -> Vec<u8> {
        let mut out = tag.to_vec();
        out.extend_from_slice(&u32::try_from(value.len()).unwrap().to_be_bytes());
        out.extend_from_slice(value);
        out
    }

    /// A keybag as a backup with this password would write it, and its class keys by class.
    #[must_use]
    pub fn sample(password: &str, class_keys: &[(u32, [u8; 32])]) -> Vec<u8> {
        let (salt, iterations, dp_salt, dp_iterations) = (b"salt-salt-salt-salt!", 10u32, b"dpsl-dpsl-dpsl-dpsl!", 20u32);
        let mut first = [0u8; 32];
        pbkdf2::pbkdf2_hmac::<sha2::Sha256>(password.as_bytes(), dp_salt, dp_iterations, &mut first);
        let mut key = [0u8; 32];
        pbkdf2::pbkdf2_hmac::<sha1::Sha1>(&first, salt, iterations, &mut key);
        let mut bag = [
            record(*b"VERS", &4u32.to_be_bytes()),
            record(*b"TYPE", &1u32.to_be_bytes()),
            record(*b"UUID", &[1; 16]),
            record(*b"WRAP", &2u32.to_be_bytes()),
            record(*b"SALT", salt),
            record(*b"ITER", &iterations.to_be_bytes()),
            record(*b"DPWT", &1u32.to_be_bytes()),
            record(*b"DPIC", &dp_iterations.to_be_bytes()),
            record(*b"DPSL", dp_salt),
        ]
        .concat();
        for (class, class_key) in class_keys {
            bag.extend(record(*b"UUID", &[2; 16]));
            bag.extend(record(*b"CLAS", &class.to_be_bytes()));
            bag.extend(record(*b"WRAP", &3u32.to_be_bytes()));
            bag.extend(record(*b"KTYP", &0u32.to_be_bytes()));
            bag.extend(record(*b"WPKY", &wrap_key(&key, class_key)));
        }
        bag
    }

    #[test]
    fn unlocks_with_the_password_alone() {
        let bag = Keybag::parse(&sample("pass", &[(3, [5; 32]), (4, [6; 32])])).unwrap();
        let keys = bag.unlock("pass").unwrap();
        assert_eq!(keys.0.get(&3), Some(&[5; 32]));
        assert_eq!(keys.0.get(&4), Some(&[6; 32]));
        assert!(matches!(bag.unlock("wrong"), Err(crate::BackupError::WrongPassword)));
    }
}
