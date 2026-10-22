//! ACME (Let's Encrypt) Automated Certificate Management (RFC 8555).
//!
//! Provides automated HTTP-01 challenge verification, account registration,
//! certificate ordering, CSR finalization, and certificate chain downloading
//! for real public domains.

use std::path::Path;
use std::time::Duration;

use rcgen::{CertificateParams, DistinguishedName, DnType, KeyPair};
use ring::rand::SystemRandom;
use ring::signature::{EcdsaKeyPair, KeyPair as _, ECDSA_P256_SHA256_FIXED_SIGNING};
use serde::Deserialize;
use serde_json::{json, Value};

use crate::ca::IssuedCert;
use crate::error::CoreError;

pub const LETS_ENCRYPT_PRODUCTION_DIR: &str = "https://acme-v02.api.letsencrypt.org/directory";
pub const LETS_ENCRYPT_STAGING_DIR: &str = "https://acme-staging-v02.api.letsencrypt.org/directory";

fn base64_url(data: impl AsRef<[u8]>) -> String {
    use base64::engine::general_purpose::URL_SAFE_NO_PAD;
    use base64::Engine;
    URL_SAFE_NO_PAD.encode(data)
}

fn err(msg: impl Into<String>) -> CoreError {
    CoreError::CertError(msg.into())
}

#[derive(Debug, Clone, Deserialize)]
struct AcmeDirectory {
    #[serde(rename = "newNonce")]
    new_nonce: String,
    #[serde(rename = "newAccount")]
    new_account: String,
    #[serde(rename = "newOrder")]
    new_order: String,
}

pub struct AcmeAccount {
    key_pair: EcdsaKeyPair,
    jwk: Value,
    thumbprint: String,
    pub account_url: Option<String>,
}

impl AcmeAccount {
    pub fn load_or_create(key_file: &Path) -> Result<Self, CoreError> {
        let rng = SystemRandom::new();
        let pkcs8_bytes = if key_file.is_file() {
            std::fs::read(key_file)
                .map_err(|e| err(format!("failed to read ACME account key: {e}")))?
        } else {
            if let Some(parent) = key_file.parent() {
                std::fs::create_dir_all(parent)
                    .map_err(|e| err(format!("failed to create ACME key dir: {e}")))?;
            }
            let pkcs8 = EcdsaKeyPair::generate_pkcs8(&ECDSA_P256_SHA256_FIXED_SIGNING, &rng)
                .map_err(|e| err(format!("failed to generate ACME key: {e}")))?;
            let bytes = pkcs8.as_ref().to_vec();
            std::fs::write(key_file, &bytes)
                .map_err(|e| err(format!("failed to save ACME key: {e}")))?;
            bytes
        };

        let key_pair =
            EcdsaKeyPair::from_pkcs8(&ECDSA_P256_SHA256_FIXED_SIGNING, &pkcs8_bytes, &rng)
                .map_err(|e| err(format!("failed to parse ACME key: {e}")))?;

        let pub_key = key_pair.public_key().as_ref();
        if pub_key.len() != 65 || pub_key[0] != 0x04 {
            return Err(err("unexpected public key format for P-256"));
        }

        let x = base64_url(&pub_key[1..33]);
        let y = base64_url(&pub_key[33..65]);

        let jwk = json!({
            "crv": "P-256",
            "kty": "EC",
            "x": x,
            "y": y
        });

        // Canonical JSON for thumbprint (RFC 7638)
        let canonical = format!(r#"{{"crv":"P-256","kty":"EC","x":"{x}","y":"{y}"}}"#);
        use sha2::{Digest, Sha256};
        let mut hasher = Sha256::new();
        hasher.update(canonical.as_bytes());
        let thumbprint = base64_url(hasher.finalize());

        Ok(Self {
            key_pair,
            jwk,
            thumbprint,
            account_url: None,
        })
    }

    pub fn thumbprint(&self) -> &str {
        &self.thumbprint
    }

    pub fn sign_jws(&self, url: &str, nonce: &str, payload: &Value) -> Result<Value, CoreError> {
        let rng = SystemRandom::new();
        let mut protected = json!({
            "alg": "ES256",
            "nonce": nonce,
            "url": url,
        });

        if let Some(account_url) = &self.account_url {
            protected["kid"] = json!(account_url);
        } else {
            protected["jwk"] = self.jwk.clone();
        }

        let protected_b64 = base64_url(protected.to_string());
        let payload_b64 = if payload.is_null() {
            String::new()
        } else {
            base64_url(payload.to_string())
        };

        let signing_input = format!("{protected_b64}.{payload_b64}");
        let sig = self
            .key_pair
            .sign(&rng, signing_input.as_bytes())
            .map_err(|e| err(format!("JWS signing failed: {e}")))?;

        Ok(json!({
            "protected": protected_b64,
            "payload": payload_b64,
            "signature": base64_url(sig.as_ref()),
        }))
    }
}

async fn read_json<T: serde::de::DeserializeOwned>(
    resp: reqwest::Response,
) -> Result<T, CoreError> {
    let bytes = resp
        .bytes()
        .await
        .map_err(|e| err(format!("failed to read response bytes: {e}")))?;
    serde_json::from_slice(&bytes).map_err(|e| err(format!("failed to parse JSON response: {e}")))
}

pub struct AcmeClient {
    client: reqwest::Client,
    directory_url: String,
    account: AcmeAccount,
    nonce: Option<String>,
}

impl AcmeClient {
    pub fn new(directory_url: Option<&str>, account_key_file: &Path) -> Result<Self, CoreError> {
        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(20))
            .user_agent("OpenLocalServer-ACME/2.3.0")
            .build()
            .map_err(|e| err(format!("failed to build HTTP client: {e}")))?;

        let account = AcmeAccount::load_or_create(account_key_file)?;
        let directory_url = directory_url
            .unwrap_or(LETS_ENCRYPT_PRODUCTION_DIR)
            .to_string();

        Ok(Self {
            client,
            directory_url,
            account,
            nonce: None,
        })
    }

    async fn get_directory(&self) -> Result<AcmeDirectory, CoreError> {
        let resp = self
            .client
            .get(&self.directory_url)
            .send()
            .await
            .map_err(|e| err(format!("failed to fetch ACME directory: {e}")))?;

        if !resp.status().is_success() {
            return Err(err(format!(
                "ACME directory request failed: HTTP {}",
                resp.status()
            )));
        }

        read_json(resp).await
    }

    async fn get_nonce(&mut self, new_nonce_url: &str) -> Result<String, CoreError> {
        if let Some(n) = self.nonce.take() {
            return Ok(n);
        }
        let resp = self
            .client
            .head(new_nonce_url)
            .send()
            .await
            .map_err(|e| err(format!("failed to fetch new ACME nonce: {e}")))?;

        resp.headers()
            .get("replay-nonce")
            .and_then(|h| h.to_str().ok())
            .map(str::to_string)
            .ok_or_else(|| err("ACME server did not return Replay-Nonce header"))
    }

    async fn post_jws(
        &mut self,
        url: &str,
        new_nonce_url: &str,
        payload: &Value,
    ) -> Result<reqwest::Response, CoreError> {
        let nonce = self.get_nonce(new_nonce_url).await?;
        let jws = self.account.sign_jws(url, &nonce, payload)?;
        let body = serde_json::to_string(&jws)
            .map_err(|e| err(format!("failed to serialize JWS: {e}")))?;

        let resp = self
            .client
            .post(url)
            .header("Content-Type", "application/jose+json")
            .body(body)
            .send()
            .await
            .map_err(|e| err(format!("ACME request to {url} failed: {e}")))?;

        if let Some(n) = resp
            .headers()
            .get("replay-nonce")
            .and_then(|h| h.to_str().ok())
        {
            self.nonce = Some(n.to_string());
        }

        Ok(resp)
    }

    /// Registers the ACME account or fetches existing account URL.
    pub async fn ensure_account(
        &mut self,
        new_account_url: &str,
        new_nonce_url: &str,
        email: Option<&str>,
    ) -> Result<(), CoreError> {
        let mut body = json!({
            "termsOfServiceAgreed": true,
        });
        if let Some(e) = email.filter(|e| !e.is_empty()) {
            body["contact"] = json!([format!("mailto:{e}")]);
        }

        let resp = self.post_jws(new_account_url, new_nonce_url, &body).await?;
        if resp.status().is_success()
            || resp.status() == reqwest::StatusCode::OK
            || resp.status() == reqwest::StatusCode::CREATED
        {
            if let Some(loc) = resp.headers().get("location").and_then(|h| h.to_str().ok()) {
                self.account.account_url = Some(loc.to_string());
                return Ok(());
            }
        }

        let status = resp.status();
        let body = resp.text().await.unwrap_or_default();
        Err(err(format!(
            "ACME account registration failed: HTTP {status} - {body}"
        )))
    }

    /// Complete automated issuance flow for a real domain using HTTP-01 challenge.
    pub async fn issue_certificate(
        &mut self,
        hostname: &str,
        doc_root: &Path,
        email: Option<&str>,
    ) -> Result<IssuedCert, CoreError> {
        let dir = self.get_directory().await?;
        self.ensure_account(&dir.new_account, &dir.new_nonce, email)
            .await?;

        // 1. Create order
        let order_payload = json!({
            "identifiers": [{ "type": "dns", "value": hostname }]
        });
        let order_resp = self
            .post_jws(&dir.new_order, &dir.new_nonce, &order_payload)
            .await?;
        let order_url = order_resp
            .headers()
            .get("location")
            .and_then(|h| h.to_str().ok())
            .map(str::to_string)
            .ok_or_else(|| err("ACME server did not return order Location"))?;

        let order_json: Value = read_json(order_resp).await?;

        let auth_urls = order_json["authorizations"]
            .as_array()
            .ok_or_else(|| err("order response missing authorizations"))?;

        // 2. Handle HTTP-01 challenge for each authorization
        let mut created_challenges = Vec::new();
        for auth_val in auth_urls {
            let auth_url = auth_val
                .as_str()
                .ok_or_else(|| err("invalid authorization URL"))?;
            let auth_resp = self
                .post_jws(auth_url, &dir.new_nonce, &Value::Null)
                .await?;
            let auth_json: Value = read_json(auth_resp).await?;

            let challenges = auth_json["challenges"]
                .as_array()
                .ok_or_else(|| err("auth missing challenges"))?;

            let http_challenge = challenges
                .iter()
                .find(|c| c["type"].as_str() == Some("http-01"))
                .ok_or_else(|| err("no http-01 challenge found"))?;

            let token = http_challenge["token"]
                .as_str()
                .ok_or_else(|| err("challenge missing token"))?;
            let challenge_url = http_challenge["url"]
                .as_str()
                .ok_or_else(|| err("challenge missing url"))?;

            let key_auth = format!("{token}.{}", self.account.thumbprint());

            // Write to <doc_root>/.well-known/acme-challenge/<token>
            let challenge_dir = doc_root.join(".well-known").join("acme-challenge");
            std::fs::create_dir_all(&challenge_dir).map_err(|e| {
                err(format!(
                    "failed to create ACME challenge dir {}: {e}",
                    challenge_dir.display()
                ))
            })?;
            let token_file = challenge_dir.join(token);
            std::fs::write(&token_file, key_auth.as_bytes()).map_err(|e| {
                err(format!(
                    "failed to write ACME challenge token {}: {e}",
                    token_file.display()
                ))
            })?;

            created_challenges.push(token_file);

            // Notify ACME server challenge is ready
            self.post_jws(challenge_url, &dir.new_nonce, &json!({}))
                .await?;
        }

        // 3. Poll order until status is "ready"
        let finalize_url = match self.poll_order_ready(&order_url, &dir.new_nonce).await {
            Ok(url) => url,
            Err(e) => {
                // Cleanup tokens on failure
                for f in created_challenges {
                    let _ = std::fs::remove_file(f);
                }
                return Err(e);
            }
        };

        // 4. Generate key and CSR
        let domain_key =
            KeyPair::generate().map_err(|e| err(format!("failed to generate domain key: {e}")))?;
        let mut params = CertificateParams::new(vec![hostname.to_string()])
            .map_err(|e| err(format!("failed to create CSR params: {e}")))?;
        let mut dn = DistinguishedName::new();
        dn.push(DnType::CommonName, hostname);
        params.distinguished_name = dn;

        let csr = params
            .serialize_request(&domain_key)
            .map_err(|e| err(format!("failed to serialize CSR: {e}")))?;
        let csr_der = csr.der();

        // 5. Finalize order
        let finalize_payload = json!({
            "csr": base64_url(csr_der),
        });
        let _ = self
            .post_jws(&finalize_url, &dir.new_nonce, &finalize_payload)
            .await?;

        // 6. Poll order until "valid" and get certificate URL
        let cert_url = self.poll_order_valid(&order_url, &dir.new_nonce).await?;

        // 7. Download certificate chain
        let cert_resp = self
            .post_jws(&cert_url, &dir.new_nonce, &Value::Null)
            .await?;
        let cert_pem = cert_resp
            .text()
            .await
            .map_err(|e| err(format!("failed to download certificate chain: {e}")))?;

        // Cleanup challenge tokens
        for f in created_challenges {
            let _ = std::fs::remove_file(f);
        }

        Ok(IssuedCert {
            cert_pem,
            key_pem: domain_key.serialize_pem(),
        })
    }

    async fn poll_order_ready(
        &mut self,
        order_url: &str,
        new_nonce_url: &str,
    ) -> Result<String, CoreError> {
        for _ in 0..15 {
            tokio::time::sleep(Duration::from_millis(1500)).await;
            let resp = self
                .post_jws(order_url, new_nonce_url, &Value::Null)
                .await?;
            let json: Value = read_json(resp).await?;

            match json["status"].as_str() {
                Some("ready") => {
                    return json["finalize"]
                        .as_str()
                        .map(str::to_string)
                        .ok_or_else(|| err("order ready but missing finalize URL"));
                }
                Some("valid") => {
                    return json["finalize"]
                        .as_str()
                        .map(str::to_string)
                        .ok_or_else(|| err("order valid but missing finalize URL"));
                }
                Some("invalid") => {
                    let reason = json["error"]["detail"]
                        .as_str()
                        .unwrap_or("order authorization invalid");
                    return Err(err(format!(
                        "Let's Encrypt validation failed: {reason}. Ensure your domain is pointing to this machine or broadcasting online."
                    )));
                }
                _ => {}
            }
        }
        Err(err("timed out waiting for ACME challenge validation"))
    }

    async fn poll_order_valid(
        &mut self,
        order_url: &str,
        new_nonce_url: &str,
    ) -> Result<String, CoreError> {
        for _ in 0..15 {
            tokio::time::sleep(Duration::from_millis(1500)).await;
            let resp = self
                .post_jws(order_url, new_nonce_url, &Value::Null)
                .await?;
            let json: Value = read_json(resp).await?;

            if json["status"].as_str() == Some("valid") {
                return json["certificate"]
                    .as_str()
                    .map(str::to_string)
                    .ok_or_else(|| err("order valid but missing certificate URL"));
            }
        }
        Err(err("timed out waiting for certificate issuance"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_base64_url() {
        assert_eq!(base64_url("hello"), "aGVsbG8");
        assert_eq!(base64_url(""), "");
    }

    #[test]
    fn test_acme_account_generation_and_jwk() {
        let temp_dir = tempfile::tempdir().unwrap();
        let key_file = temp_dir.path().join("account.key");
        let account = AcmeAccount::load_or_create(&key_file).unwrap();

        assert_eq!(account.jwk["kty"], "EC");
        assert_eq!(account.jwk["crv"], "P-256");
        assert!(!account.thumbprint().is_empty());

        // Reloads the same key and matches thumbprint
        let again = AcmeAccount::load_or_create(&key_file).unwrap();
        assert_eq!(account.thumbprint(), again.thumbprint());
    }

    #[test]
    fn test_jws_signing() {
        let temp_dir = tempfile::tempdir().unwrap();
        let key_file = temp_dir.path().join("account.key");
        let account = AcmeAccount::load_or_create(&key_file).unwrap();

        let jws = account
            .sign_jws(
                "https://example.com/acme/order",
                "test-nonce-123",
                &json!({"test": "value"}),
            )
            .unwrap();

        assert!(jws["protected"].is_string());
        assert!(jws["payload"].is_string());
        assert!(jws["signature"].is_string());
    }
}
