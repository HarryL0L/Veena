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
/// Uses the bundled Mozilla root certificates so TLS works the same on every
/// platform. If `SSL_CERT_FILE` points to a PEM bundle, those roots are used
/// instead, which supports corporate or intercepting proxies.
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
    match custom {
        Some(certs) => RootCerts::new_with_certs(&certs),
        None => RootCerts::WebPki,
    }
}
