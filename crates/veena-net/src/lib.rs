//! Shared HTTP client setup for Veena.

use std::sync::OnceLock;

use ureq::Agent;
use ureq::config::ConfigBuilder;
use ureq::tls::{PemItem, RootCerts, TlsConfig};
use ureq::typestate::AgentScope;

pub use ureq;

/// Identifies Veena to servers.
pub const USER_AGENT: &str = concat!("Veena/", env!("CARGO_PKG_VERSION"));

/// The shared client used for audio streams. Follows redirects.
pub fn agent() -> &'static Agent {
    static AGENT: OnceLock<Agent> = OnceLock::new();
    AGENT.get_or_init(|| config().build().into())
}

/// Base configuration with Veena's TLS roots and user agent.
///
/// On Android, uses the certificates the device trusts (its system store),
/// like other apps. Elsewhere, uses the bundled Mozilla root certificates.
/// If `SSL_CERT_FILE` points to a PEM bundle, those roots are used instead,
/// which supports corporate or intercepting proxies.
pub fn config() -> ConfigBuilder<AgentScope> {
    Agent::config_builder()
        .tls_config(TlsConfig::builder().root_certs(root_certs()).build())
        .user_agent(USER_AGENT)
}

fn root_certs() -> RootCerts {
    let custom = std::env::var_os("SSL_CERT_FILE")
        .and_then(|path| std::fs::read(path).ok())
        .map(|pem| {
            ureq::tls::parse_pem(&pem)
                .filter_map(|item| match item {
                    Ok(PemItem::Certificate(cert)) => Some(cert),
                    _ => None,
                })
                .collect::<Vec<_>>()
        })
        .filter(|certs| !certs.is_empty());
    match custom.or_else(system_store) {
        Some(certs) => RootCerts::new_with_certs(&certs),
        None => RootCerts::WebPki,
    }
}

/// The device's trusted certificates on Android: one PEM file each, in the
/// updatable store (Android 14+) or the system image. User-added
/// certificates are not included, matching Android's default for apps.
#[cfg(target_os = "android")]
fn system_store() -> Option<Vec<ureq::tls::Certificate<'static>>> {
    ["/apex/com.android.conscrypt/cacerts", "/system/etc/security/cacerts"]
        .iter()
        .find_map(|dir| {
            let certs: Vec<_> = std::fs::read_dir(dir)
                .ok()?
                .filter_map(|entry| std::fs::read(entry.ok()?.path()).ok())
                .flat_map(|pem| {
                    ureq::tls::parse_pem(&pem)
                        .filter_map(|item| match item {
                            Ok(PemItem::Certificate(cert)) => Some(cert),
                            _ => None,
                        })
                        .collect::<Vec<_>>()
                })
                .collect();
            (!certs.is_empty()).then_some(certs)
        })
}

#[cfg(not(target_os = "android"))]
fn system_store() -> Option<Vec<ureq::tls::Certificate<'static>>> {
    None
}
