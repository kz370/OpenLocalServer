//! Secrets Manager (§104, §141 — Stage 5). Every subsystem that needs a password, API
//! key, or token goes through this instead of writing it to disk itself — backed by the
//! OS-native credential store (Windows Credential Manager here), never a plain file.

use keyring::Entry;

/// The keyring "service" namespace every OLS secret is stored under, so it
/// shows up as one recognizable group in the OS credential manager rather than scattered
/// entries.
const SERVICE_NAME: &str = "OpenLocalServer";

fn entry(key: &str) -> Result<Entry, String> {
    Entry::new(SERVICE_NAME, key).map_err(|e| e.to_string())
}

pub fn set_secret(key: &str, value: &str) -> Result<(), String> {
    entry(key)?.set_password(value).map_err(|e| e.to_string())
}

/// `Ok(None)` means "no secret stored under this key" — distinct from an actual error
/// talking to the credential store, which callers should surface, not silently swallow.
pub fn get_secret(key: &str) -> Result<Option<String>, String> {
    match entry(key)?.get_password() {
        Ok(value) => Ok(Some(value)),
        Err(keyring::Error::NoEntry) => Ok(None),
        Err(e) => Err(e.to_string()),
    }
}

pub fn delete_secret(key: &str) -> Result<(), String> {
    match entry(key)?.delete_credential() {
        Ok(()) => Ok(()),
        Err(keyring::Error::NoEntry) => Ok(()),
        Err(e) => Err(e.to_string()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Real Windows Credential Manager access — not mocked. Uses a key unlikely to
    /// collide with anything real, and always cleans up after itself even on failure.
    #[test]
    fn set_get_delete_round_trip_through_the_real_os_credential_store() {
        let key = "ols-core-test-secret-do-not-use";
        let _ = delete_secret(key); // clean slate in case a previous run failed mid-test

        assert_eq!(get_secret(key).unwrap(), None);

        set_secret(key, "hunter2").unwrap();
        assert_eq!(get_secret(key).unwrap(), Some("hunter2".to_string()));

        delete_secret(key).unwrap();
        assert_eq!(get_secret(key).unwrap(), None);
    }
}
