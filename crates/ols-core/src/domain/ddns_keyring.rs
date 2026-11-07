//! Where a site's Dynamic DNS credentials live: the OS credential store, never the database.
//!
//! Two fields are credentials: `ddns_token`, and `ddns_zone_id` for every provider except
//! Cloudflare (a zone ID) and Custom (the webhook URL), where that second field holds an API
//! secret or a password (GoDaddy, Porkbun, IONOS, the `user:password` providers, ...).
//!
//! In memory a `Domain` still carries both fields, so DDNS sync and the site's Domains panel
//! work exactly as before. Only what is written changes:
//! - `DomainStore::persist` moves each credential into the keyring and writes the row
//!   without it. A value leaves the row **only after** the keyring answered with the same
//!   value, so a keyring that refuses (a temporary Windows profile, a locked store) leaves
//!   it where it was instead of losing it;
//! - `DomainStore::load` puts each credential back from the keyring, for sites using DDNS;
//! - rows written by older versions still hold them in plain text. They are read as they
//!   are and moved on the first save (`load` saves straight away when it finds one).
//!
//! Keys are `ddns:<install id>:<hostname>` (token) and `ddns-secret:<install id>:<hostname>`
//! (second field). The install id is random, made once and kept in the app's own database,
//! so two installs (or the test suite's throwaway homes) never read or delete each other's
//! values, and moving a portable install keeps them.

use std::collections::{BTreeMap, BTreeSet};

use crate::db;
use crate::paths::AppPaths;

const COLLECTION: &str = "keyring";
const NAMESPACE_ID: &str = "namespace";

#[cfg(test)]
thread_local! {
    /// Tests: make the credential store refuse writes on this thread.
    pub(crate) static REFUSE_WRITES: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

#[derive(serde::Serialize, serde::Deserialize)]
struct Namespace {
    id: String,
}

/// This install's namespace, made on first use. `None` only when the database cannot be
/// read or written; callers then keep credentials in the row (the old behaviour) rather
/// than risk writing them under a key the next start would not find.
pub(crate) fn namespace(paths: &AppPaths) -> Option<String> {
    if let Some(ns) = db::load_docs::<Namespace>(paths, COLLECTION)
        .ok()?
        .into_iter()
        .next()
        .filter(|n| !n.id.is_empty())
    {
        return Some(ns.id);
    }
    let mut bytes = [0u8; 16];
    getrandom::fill(&mut bytes).ok()?;
    let id: String = bytes.iter().map(|b| format!("{b:02x}")).collect();
    let ns = Namespace { id: id.clone() };
    db::save_docs(paths, COLLECTION, &[(NAMESPACE_ID.to_string(), &ns)]).ok()?;
    Some(id)
}

/// Keyring key of a site's DDNS token.
pub(crate) fn key(namespace: &str, hostname: &str) -> String {
    format!("ddns:{namespace}:{hostname}")
}

/// Keyring key of a site's second DDNS credential (`ddns_zone_id`).
pub(crate) fn secret_key(namespace: &str, hostname: &str) -> String {
    format!("ddns-secret:{namespace}:{hostname}")
}

fn provider(d: &super::Domain) -> Option<String> {
    d.ddns_provider
        .as_deref()
        .map(|p| p.trim().to_ascii_lowercase())
        .filter(|p| !p.is_empty() && p != "disabled")
}

/// A site whose credentials are worth looking up: DDNS is configured for it.
pub(crate) fn uses_ddns(d: &super::Domain) -> bool {
    provider(d).is_some()
}

/// `ddns_zone_id` is a credential for this site's provider (not a Cloudflare zone ID or a
/// Custom webhook URL).
pub(crate) fn zone_is_secret(d: &super::Domain) -> bool {
    provider(d).is_some_and(|p| !matches!(p.as_str(), "cloudflare" | "custom" | "webhook"))
}

/// Removes the DDNS credentials from a copy of a site that is about to leave this computer
/// (a snapshot zip, a site bundle).
pub(crate) fn without_credentials(mut d: super::Domain) -> super::Domain {
    if zone_is_secret(&d) {
        d.ddns_zone_id = None;
    }
    d.ddns_token = None;
    d
}

fn non_empty(v: Option<&str>) -> Option<&str> {
    v.map(str::trim).filter(|v| !v.is_empty())
}

/// What the keyring holds for this install, by keyring key, as far as this process knows.
/// Lets `persist` skip a keyring call for every site on every save.
#[derive(Default)]
pub(crate) struct Stored(BTreeMap<String, String>);

impl Stored {
    /// Writes `value` under `key` and reads it back. `true` only when the keyring now holds
    /// exactly that value.
    fn put(&mut self, key: &str, hostname: &str, value: &str) -> bool {
        if self.0.get(key).map(String::as_str) == Some(value) {
            return true;
        }
        #[cfg(test)]
        if REFUSE_WRITES.with(|r| r.get()) {
            tracing::warn!(%hostname, "test: the credential store refuses writes");
            return false;
        }
        let ok = crate::secrets::set_secret(key, value).is_ok()
            && crate::secrets::get_secret(key).ok().flatten().as_deref() == Some(value);
        if ok {
            self.0.insert(key.to_string(), value.to_string());
        } else {
            // The value is deliberately not logged.
            tracing::warn!(
                %hostname,
                "a Dynamic DNS credential could not be stored in the OS credential store; it stays in the site settings"
            );
        }
        ok
    }

    fn fetch(&mut self, key: &str, hostname: &str) -> Option<String> {
        match crate::secrets::get_secret(key) {
            Ok(Some(v)) => {
                self.0.insert(key.to_string(), v.clone());
                Some(v)
            }
            Ok(None) => None,
            Err(e) => {
                tracing::warn!(%hostname, error = %e, "a Dynamic DNS credential could not be read from the OS credential store");
                None
            }
        }
    }

    /// Fills a site loaded from the database with its keyring values. Returns `true` when
    /// the row still held a credential in plain text (written by an older version).
    pub(crate) fn fill(&mut self, ns: &str, d: &mut super::Domain) -> bool {
        let mut plaintext = false;
        // A site without a provider keeps whatever its row holds (see `take`).
        if !uses_ddns(d) {
            return false;
        }
        if non_empty(d.ddns_token.as_deref()).is_some() {
            plaintext = true;
        } else {
            d.ddns_token = self.fetch(&key(ns, &d.hostname), &d.hostname);
        }
        if zone_is_secret(d) {
            if non_empty(d.ddns_zone_id.as_deref()).is_some() {
                plaintext = true;
            } else if let Some(v) = self.fetch(&secret_key(ns, &d.hostname), &d.hostname) {
                d.ddns_zone_id = Some(v);
            }
        }
        plaintext
    }

    /// Moves a site's credentials out of the row about to be written (`stored`), recording
    /// in `wanted` every keyring key that must stay. A value the keyring refused stays in
    /// the row.
    pub(crate) fn take(
        &mut self,
        ns: &str,
        d: &super::Domain,
        stored: &mut super::Domain,
        wanted: &mut BTreeSet<String>,
    ) {
        // Only sites using DDNS: `fill` reads values back for exactly those, so a value
        // moved for any other site would not come back on the next start.
        if !uses_ddns(d) {
            return;
        }
        if let Some(token) = non_empty(d.ddns_token.as_deref()) {
            let k = key(ns, &d.hostname);
            if self.put(&k, &d.hostname, token) {
                stored.ddns_token = None;
                wanted.insert(k);
            }
        }
        if zone_is_secret(d) {
            if let Some(secret) = non_empty(d.ddns_zone_id.as_deref()) {
                let k = secret_key(ns, &d.hostname);
                if self.put(&k, &d.hostname, secret) {
                    stored.ddns_zone_id = None;
                    wanted.insert(k);
                }
            }
        }
    }

    /// After the rows are saved: removes every keyring value no site needs any more (a
    /// cleared field, a renamed or removed site, a provider whose second field is not a
    /// secret). Run only after a successful save, so a failed save never costs a value.
    pub(crate) fn forget_all_but(&mut self, wanted: &BTreeSet<String>) {
        let gone: Vec<String> = self
            .0
            .keys()
            .filter(|k| !wanted.contains(*k))
            .cloned()
            .collect();
        for k in gone {
            match crate::secrets::delete_secret(&k) {
                Ok(()) => {
                    self.0.remove(&k);
                }
                Err(e) => {
                    tracing::warn!(error = %e, "an old Dynamic DNS credential could not be removed from the OS credential store")
                }
            }
        }
    }
}
