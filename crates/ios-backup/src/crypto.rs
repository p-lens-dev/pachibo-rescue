use aes::Aes256;
use aes::cipher::{BlockDecryptMut, KeyIvInit, block_padding::NoPadding};
use aes_kw::KekAes256;

/// A key unwrapped by RFC 3394 under a 256-bit key; none where the
/// integrity check fails, which is how a wrong password shows.
pub fn unwrap_key(kek: &[u8; 32], wrapped: &[u8]) -> Option<Vec<u8>> {
    KekAes256::from(*kek).unwrap_vec(wrapped).ok()
}

/// Bytes decrypted by AES-256-CBC with a zero IV, as a backup encrypts
/// its files; none where the length is no whole number of blocks.
pub fn decrypt_cbc(key: &[u8], data: &[u8]) -> Option<Vec<u8>> {
    let decryptor = cbc::Decryptor::<Aes256>::new_from_slices(key, &[0u8; 16]).ok()?;
    decryptor.decrypt_padded_vec_mut::<NoPadding>(data).ok()
}

/// Bytes with PKCS#7 padding taken off where it reads as such, else as they are.
pub fn strip_padding(mut data: Vec<u8>) -> Vec<u8> {
    let pad = data.last().copied().map_or(0, usize::from);
    let valid = (1..=16).contains(&pad)
        && data.len() >= pad
        && data.iter().rev().take(pad).all(|&byte| usize::from(byte) == pad);
    if valid {
        data.truncate(data.len() - pad);
    }
    data
}

#[cfg(test)]
pub mod tests {
    use aes::Aes256;
    use aes::cipher::{BlockEncryptMut, KeyIvInit, block_padding::Pkcs7};
    use aes_kw::KekAes256;

    pub fn wrap_key(kek: &[u8; 32], key: &[u8]) -> Vec<u8> {
        KekAes256::from(*kek).wrap_vec(key).unwrap()
    }

    pub fn encrypt_cbc(key: &[u8], data: &[u8]) -> Vec<u8> {
        cbc::Encryptor::<Aes256>::new_from_slices(key, &[0u8; 16])
            .unwrap()
            .encrypt_padded_vec_mut::<Pkcs7>(data)
    }

    #[test]
    fn round_trips() {
        let kek = [7u8; 32];
        let key = [9u8; 32];
        let wrapped = wrap_key(&kek, &key);
        assert_eq!(super::unwrap_key(&kek, &wrapped).unwrap(), key);
        assert!(super::unwrap_key(&[8u8; 32], &wrapped).is_none());
        let sealed = encrypt_cbc(&key, b"hello pachibo");
        let opened = super::strip_padding(super::decrypt_cbc(&key, &sealed).unwrap());
        assert_eq!(opened, b"hello pachibo");
    }
}
