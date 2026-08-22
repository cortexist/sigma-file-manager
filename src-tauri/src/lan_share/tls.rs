// SPDX-License-Identifier: GPL-3.0-or-later
// License: GNU GPLv3 or later. See the license file in the project root for more information.
// Copyright © 2021 - present Aleksey Hoffman. All rights reserved.
// Copyright © 2026 Cortexist, LLC (modifications). All rights reserved.

use std::net::{IpAddr, Ipv4Addr};
use std::path::Path;

use super::types::MDNS_DOMAIN;

fn install_crypto_provider() {
    let _ = rustls::crypto::ring::default_provider().install_default();
}

pub(super) async fn generate_self_signed_tls(
    ip: Ipv4Addr,
    extra_hostname: Option<&str>,
) -> Result<axum_server::tls_rustls::RustlsConfig, String> {
    install_crypto_provider();

    let mut san_names = vec![MDNS_DOMAIN.to_string()];

    if let Some(hostname) = extra_hostname {
        san_names.push(hostname.to_string());
    }

    let mut params = rcgen::CertificateParams::new(san_names)
        .map_err(|err| format!("Failed to create cert params: {err}"))?;

    params
        .subject_alt_names
        .push(rcgen::SanType::IpAddress(IpAddr::V4(ip)));

    let key_pair =
        rcgen::KeyPair::generate().map_err(|err| format!("Failed to generate key pair: {err}"))?;

    let cert = params
        .self_signed(&key_pair)
        .map_err(|err| format!("Failed to generate self-signed cert: {err}"))?;

    let cert_der = cert.der().to_vec();
    let key_der = key_pair.serialize_der();

    axum_server::tls_rustls::RustlsConfig::from_der(vec![cert_der], key_der)
        .await
        .map_err(|err| format!("Failed to create TLS config: {err}"))
}

/// Loads the user-supplied PEM certificate (leaf first, chain allowed) and private key.
pub(super) async fn load_tls_from_files(
    cert_path: &str,
    key_path: &str,
) -> Result<axum_server::tls_rustls::RustlsConfig, String> {
    install_crypto_provider();

    if !Path::new(cert_path).is_file() {
        return Err(format!("Certificate file not found: {cert_path}"));
    }

    if !Path::new(key_path).is_file() {
        return Err(format!("Private key file not found: {key_path}"));
    }

    // rustls cannot decrypt passphrase-protected keys, and its own parse failure names
    // neither the file nor the reason. The marker is right there in the PEM header, and
    // pointing at it turns a dead end into instructions (a CA key is the likely mistake).
    let key_pem = std::fs::read_to_string(key_path)
        .map_err(|err| format!("Failed to read private key file {key_path}: {err}"))?;

    if key_pem.contains("ENCRYPTED") {
        return Err(format!(
            "Private key file {key_path} is passphrase-protected. Use an unencrypted server key \
             (not the certificate authority's key)."
        ));
    }

    axum_server::tls_rustls::RustlsConfig::from_pem_file(cert_path, key_path)
        .await
        .map_err(|err| {
            format!("Failed to load certificate {cert_path} with key {key_path}: {err}")
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn issue_test_ca_and_leaf() -> (String, String, String) {
        let ca_key = rcgen::KeyPair::generate().unwrap();
        let mut ca_params = rcgen::CertificateParams::new(Vec::new()).unwrap();
        ca_params.is_ca = rcgen::IsCa::Ca(rcgen::BasicConstraints::Unconstrained);
        let ca_cert = ca_params.self_signed(&ca_key).unwrap();

        let leaf_key = rcgen::KeyPair::generate().unwrap();
        let leaf_params =
            rcgen::CertificateParams::new(vec!["localhost".to_string()]).unwrap();
        let issuer = rcgen::Issuer::from_params(&ca_params, &ca_key);
        let leaf_cert = leaf_params.signed_by(&leaf_key, &issuer).unwrap();

        (ca_cert.pem(), leaf_cert.pem(), leaf_key.serialize_pem())
    }

    #[tokio::test]
    async fn loads_a_pem_chain_and_key() {
        let (ca_pem, leaf_pem, key_pem) = issue_test_ca_and_leaf();

        let dir = tempfile::tempdir().unwrap();
        let cert_path = dir.path().join("server.crt");
        let key_path = dir.path().join("server.key");
        std::fs::write(&cert_path, format!("{leaf_pem}{ca_pem}")).unwrap();
        std::fs::write(&key_path, key_pem).unwrap();

        let result =
            load_tls_from_files(cert_path.to_str().unwrap(), key_path.to_str().unwrap()).await;

        assert!(result.is_ok(), "expected chain to load: {result:?}");
    }

    #[tokio::test]
    async fn missing_files_name_the_path() {
        let error = load_tls_from_files("/nonexistent/server.crt", "/nonexistent/server.key")
            .await
            .unwrap_err();

        assert!(error.contains("/nonexistent/server.crt"), "{error}");
    }

    #[tokio::test]
    async fn encrypted_key_is_called_out() {
        let (_, leaf_pem, _) = issue_test_ca_and_leaf();

        let dir = tempfile::tempdir().unwrap();
        let cert_path = dir.path().join("server.crt");
        let key_path = dir.path().join("server.key");
        std::fs::write(&cert_path, leaf_pem).unwrap();
        std::fs::write(
            &key_path,
            "-----BEGIN ENCRYPTED PRIVATE KEY-----\nabc\n-----END ENCRYPTED PRIVATE KEY-----\n",
        )
        .unwrap();

        let error =
            load_tls_from_files(cert_path.to_str().unwrap(), key_path.to_str().unwrap())
                .await
                .unwrap_err();

        assert!(error.contains("passphrase-protected"), "{error}");
    }
}
