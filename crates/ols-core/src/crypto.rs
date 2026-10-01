//! Password encryption for exported site bundles (§165).
//!
//! A bundle can hold `.env` files and database dumps, and those hold passwords and API keys.
//! The user chooses whether the export is encrypted; when it is, every sensitive entry in the
//! zip is sealed with ChaCha20-Poly1305 under a key derived from the export password with
//! Argon2id. Sealing is per entry, not per zip, because the zip format's own AES scheme is
//! ZipCrypto-derived and weak, and because a reader can then unlock exactly the entry it
//! needs and nothing else.
//!
//! The key is derived **once per bundle**: the random salt lives in the bundle manifest and
//! each entry carries its own random nonce. Deriving per entry would cost ~60 ms times the
//! number of files for no extra safety, and a nonce reused across two entries under one key
//! would break ChaCha20-Poly1305 entirely, so the nonce must be fresh every time — which it is.

use argon2::{Algorithm, Argon2, Params, Version};
use chacha20poly1305::aead::{Aead, KeyInit, Payload};
use chacha20poly1305::{ChaCha20Poly1305, Key, Nonce};

/// Bytes of random salt per bundle, and per-entry nonce (96 bits, the AEAD's own size).
pub const SALT_LEN: usize = 16;
const NONCE_LEN: usize = 12;

/// Argon2id, the parameters OWASP recommends for password storage: 19 MiB, 2 passes,
/// 1 lane. Deliberately the same on every machine, so a bundle written on one opens on
/// another.
fn hasher() -> Argon2<'static> {
    let params = Params::new(19 * 1024, 2, 1, None).expect("the OWASP parameters are valid");
    Argon2::new(Algorithm::Argon2id, Version::V0x13, params)
}

#[derive(Debug)]
pub enum CryptoError {
    /// The password is not the one the bundle was sealed with, or the bytes are damaged.
    /// An AEAD cannot tell those apart, and neither can we — so the message must not claim
    /// to know which it was.
    Unseal,
    /// The entry is not an OLS sealed blob (truncated, or written by something else).
    Format(String),
    Random(String),
}

impl std::fmt::Display for CryptoError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            CryptoError::Unseal => write!(
                f,
                "it did not unlock: the password is wrong, or the file is damaged"
            ),
            CryptoError::Format(what) => write!(f, "it is not an OLS sealed entry ({what})"),
            CryptoError::Random(e) => write!(f, "no random bytes were available ({e})"),
        }
    }
}

/// An unlocked bundle: one key, many entries.
pub struct BundleCipher {
    key: Key,
}

impl BundleCipher {
    /// Turns the export password into a key. The salt is the bundle's, so the same password
    /// and salt always give the same key.
    pub fn new(password: &str, salt: &[u8]) -> Result<Self, CryptoError> {
        if salt.len() < SALT_LEN {
            return Err(CryptoError::Format(format!(
                "the salt is {} bytes, {SALT_LEN} expected",
                salt.len()
            )));
        }
        let mut key = [0u8; 32];
        hasher()
            .hash_password_into(password.as_bytes(), &salt[..SALT_LEN], &mut key)
            .map_err(|e| {
                CryptoError::Format(format!("the password could not be turned into a key ({e})"))
            })?;
        Ok(BundleCipher {
            key: *Key::from_slice(&key),
        })
    }

    /// `nonce || ciphertext + tag`.
    pub fn seal(&self, plaintext: &[u8]) -> Result<Vec<u8>, CryptoError> {
        let cipher = ChaCha20Poly1305::new(&self.key);
        let nonce_bytes = random_bytes(NONCE_LEN)?;
        let nonce = Nonce::from_slice(&nonce_bytes);
        let sealed = cipher
            .encrypt(
                nonce,
                Payload {
                    msg: plaintext,
                    aad: b"ols-bundle",
                },
            )
            .map_err(|_| CryptoError::Format("the entry could not be sealed".into()))?;
        let mut out = Vec::with_capacity(NONCE_LEN + sealed.len());
        out.extend_from_slice(&nonce_bytes);
        out.extend_from_slice(&sealed);
        Ok(out)
    }

    /// The inverse of `seal`. Every byte is checked, so a damaged entry is refused rather
    /// than half-read.
    pub fn open(&self, blob: &[u8]) -> Result<Vec<u8>, CryptoError> {
        if blob.len() <= NONCE_LEN + 16 {
            return Err(CryptoError::Format("the sealed entry is too short".into()));
        }
        let cipher = ChaCha20Poly1305::new(&self.key);
        let nonce = Nonce::from_slice(&blob[..NONCE_LEN]);
        cipher
            .decrypt(
                nonce,
                Payload {
                    msg: &blob[NONCE_LEN..],
                    aad: b"ols-bundle",
                },
            )
            .map_err(|_| CryptoError::Unseal)
    }
}

pub fn random_bytes(n: usize) -> Result<Vec<u8>, CryptoError> {
    let mut buf = vec![0u8; n];
    getrandom::fill(&mut buf).map_err(|e| CryptoError::Random(e.to_string()))?;
    Ok(buf)
}

/// A fresh bundle salt.
pub fn random_salt() -> Result<Vec<u8>, CryptoError> {
    random_bytes(SALT_LEN)
}

/// A password to write down, when the user asks for one instead of typing their own.
/// Alphabet without `0/O/1/l/I`, so it survives being read off a screen and typed back.
const READABLE: &[u8] = b"abcdefghijkmnopqrstuvwxyzABCDEFGHJKLMNPQRSTUVWXYZ23456789";

/// 20 characters of about 105 bits. Short enough to copy, long enough that guessing is out
/// of the question; the user can replace it with their own password, which is the point —
/// the app never stores it.
pub fn random_password() -> Result<String, CryptoError> {
    let bytes = random_bytes(20)?;
    Ok(bytes
        .iter()
        .map(|b| READABLE[*b as usize % READABLE.len()] as char)
        .collect())
}

pub fn to_hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

pub fn from_hex(text: &str) -> Option<Vec<u8>> {
    if text.len() % 2 != 0 {
        return None;
    }
    (0..text.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&text[i..i + 2], 16).ok())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cipher() -> BundleCipher {
        BundleCipher::new("correct horse", &random_salt().unwrap()).unwrap()
    }

    #[test]
    fn a_sealed_entry_only_opens_with_the_same_password() {
        let c = cipher();
        let sealed = c.seal(b"DB_PASSWORD=hunter2").unwrap();
        assert_ne!(
            sealed,
            b"DB_PASSWORD=hunter2".to_vec(),
            "the plaintext must not be in the blob"
        );
        assert!(!String::from_utf8_lossy(&sealed).contains("hunter2"));
        assert_eq!(c.open(&sealed).unwrap(), b"DB_PASSWORD=hunter2");

        let wrong = BundleCipher::new("battery staple", &random_salt().unwrap()).unwrap();
        assert!(matches!(wrong.open(&sealed), Err(CryptoError::Unseal)));
    }

    #[test]
    fn the_salt_makes_the_key_purpose_specific() {
        let salt = random_salt().unwrap();
        let a = BundleCipher::new("same password", &salt).unwrap();
        let b = BundleCipher::new("same password", &salt).unwrap();
        let sealed = a.seal(b"secret").unwrap();
        assert_eq!(
            b.open(&sealed).unwrap(),
            b"secret",
            "the same salt gives the same key"
        );
    }

    #[test]
    fn two_entries_of_the_same_plaintext_differ() {
        let c = cipher();
        let one = c.seal(b"same").unwrap();
        let two = c.seal(b"same").unwrap();
        assert_ne!(one, two, "each entry needs its own nonce");
        assert_eq!(c.open(&one).unwrap(), c.open(&two).unwrap());
    }

    #[test]
    fn a_damaged_entry_is_refused_rather_than_half_read() {
        let c = cipher();
        let mut sealed = c.seal(b"DB_PASSWORD=hunter2").unwrap();
        let last = sealed.len() - 1;
        sealed[last] ^= 0x01;
        assert!(matches!(c.open(&sealed), Err(CryptoError::Unseal)));
    }

    #[test]
    fn a_short_blob_is_reported_as_a_format_problem_not_a_wrong_password() {
        let c = cipher();
        assert!(matches!(c.open(b"too short"), Err(CryptoError::Format(_))));
    }

    #[test]
    fn a_short_salt_is_refused() {
        assert!(matches!(
            BundleCipher::new("p", &[0u8; 4]),
            Err(CryptoError::Format(_))
        ));
    }

    #[test]
    fn a_generated_password_is_20_readable_characters() {
        let p = random_password().unwrap();
        assert_eq!(p.chars().count(), 20);
        assert!(p
            .chars()
            .all(|c| c.is_ascii_alphanumeric() && !"01lIO".contains(c)));
        assert_ne!(p, random_password().unwrap());
    }

    #[test]
    fn hex_round_trips() {
        let bytes = vec![0u8, 1, 15, 16, 128, 255];
        assert_eq!(from_hex(&to_hex(&bytes)).unwrap(), bytes);
        assert_eq!(to_hex(&bytes), "00010f1080ff");
        assert!(from_hex("abc").is_none(), "an odd length is not hex");
        assert!(from_hex("zz").is_none());
    }

    #[test]
    fn an_empty_entry_still_round_trips() {
        let c = cipher();
        assert!(c.open(&c.seal(b"").unwrap()).unwrap().is_empty());
    }
}
