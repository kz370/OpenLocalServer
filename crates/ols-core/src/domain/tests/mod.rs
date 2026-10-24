use std::collections::BTreeMap;
use super::*;

pub(crate) fn domain(host: &str) -> Domain {
    Domain {
        hostname: host.into(),
        project_id: None,
        root: "C:/sites/x".into(),
        kind: SiteKind::Static,
        https: true,
        redirect_https: true,
        wildcard: false,
        enabled: true,
        ownership: Ownership::Managed,
        app: None,
        blocks: SiteBlocks::default(),
        generated_hashes: BTreeMap::new(),
        public_domain: None,
        tunnel_id: None,
        server: None,
        path_prefix: None,
        icon: None,
        ssl_provider: SslProvider::LocalCa,
        custom_cert_path: None,
        custom_key_path: None,
        lets_encrypt_email: None,
        auto_broadcast: false,
        broadcast_provider: None,
        ..Default::default()
    }
}

mod store_tests;
mod unit_tests;
