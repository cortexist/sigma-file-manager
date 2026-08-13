// SPDX-License-Identifier: GPL-3.0-or-later
// License: GNU GPLv3 or later. See the license file in the project root for more information.
// Copyright © 2021 - present Aleksey Hoffman. All rights reserved.

use sha2::{Digest, Sha256};
use std::path::{Component, Path, PathBuf};
use std::sync::Arc;

pub fn authorize_extension_caller(
    caller_extension_id: Option<&str>,
    extension_id: &str,
) -> Result<(), String> {
    match caller_extension_id {
        Some(caller_id) if caller_id == extension_id => Ok(()),
        Some(_) => {
            Err("Access denied: caller extension does not match target extension".to_string())
        }
        None => Err("Access denied: missing caller extension identity".to_string()),
    }
}

pub async fn acquire_extension_install_lock(
    extension_id: &str,
) -> Result<tokio::sync::OwnedMutexGuard<()>, String> {
    validate_binary_path_component(extension_id, "extension id")?;

    let mutex = {
        let mut locks = super::state::EXTENSION_INSTALL_LOCKS.lock().await;
        locks
            .entry(extension_id.to_string())
            .or_insert_with(|| Arc::new(tokio::sync::Mutex::new(())))
            .clone()
    };

    Ok(mutex.lock_owned().await)
}

pub fn require_integrity(integrity: &Option<String>, label: &str) -> Result<(), String> {
    match integrity
        .as_ref()
        .map(|value| value.trim())
        .filter(|value| !value.is_empty())
    {
        Some(_) => Ok(()),
        None => Err(format!("Integrity is required for {}", label)),
    }
}

pub fn is_private_ip_address(ip_address: &std::net::IpAddr) -> bool {
    match ip_address {
        std::net::IpAddr::V4(ipv4) => {
            ipv4.is_private()
                || ipv4.is_loopback()
                || ipv4.is_link_local()
                || ipv4.is_broadcast()
                || *ipv4 == std::net::Ipv4Addr::UNSPECIFIED
                || *ipv4 == std::net::Ipv4Addr::new(169, 254, 169, 254)
        }
        std::net::IpAddr::V6(ipv6) => {
            ipv6.is_loopback()
                || ipv6.is_unspecified()
                || ipv6.is_unique_local()
                || ipv6.is_unicast_link_local()
        }
    }
}

pub fn validate_remote_url(url: &str) -> Result<reqwest::Url, String> {
    let parsed_url =
        reqwest::Url::parse(url).map_err(|error| format!("Invalid URL '{}': {}", url, error))?;
    let scheme = parsed_url.scheme();

    if scheme != "https" && scheme != "http" {
        return Err("Only http and https URLs are allowed".to_string());
    }

    let host = parsed_url
        .host_str()
        .ok_or_else(|| "URL host is required".to_string())?
        .to_ascii_lowercase();

    if host == "localhost"
        || host == "metadata.google.internal"
        || host == "metadata"
        || host.ends_with(".local")
    {
        return Err("Access denied: target host is not allowed".to_string());
    }

    if let Ok(ip_address) = host.parse::<std::net::IpAddr>() {
        if is_private_ip_address(&ip_address) {
            return Err("Access denied: target host is not allowed".to_string());
        }
    }

    Ok(parsed_url)
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum HostPatternPort {
    Any,
    Exact(u16),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct HostAllowlistPattern {
    scheme: String,
    host: String,
    port: HostPatternPort,
    /// Set by a leading `*.`, admitting the domain and anything under it.
    match_subdomains: bool,
}

fn default_port_for_scheme(scheme: &str) -> u16 {
    if scheme == "https" {
        443
    } else {
        80
    }
}

pub fn validate_http_host_pattern(pattern: &str) -> Result<(), String> {
    parse_host_allowlist_pattern(pattern).map(|_| ())
}

pub(crate) fn parse_host_allowlist_pattern(pattern: &str) -> Result<HostAllowlistPattern, String> {
    let trimmed_pattern = pattern.trim();

    if trimmed_pattern.is_empty() {
        return Err("HTTP host pattern cannot be empty".to_string());
    }

    let (pattern_without_wildcard, wildcard_port) =
        if let Some(prefix) = trimmed_pattern.strip_suffix(":*") {
            (prefix, true)
        } else {
            (trimmed_pattern, false)
        };

    // The `*.` is removed before parsing rather than handed to the URL parser, which has
    // no concept of a wildcard host and would either reject it or normalise it oddly.
    let (pattern_body, match_subdomains) = match strip_subdomain_wildcard(pattern_without_wildcard)
    {
        Some(body) => (body, true),
        None => (pattern_without_wildcard.to_string(), false),
    };

    let parsed_pattern = if wildcard_port {
        reqwest::Url::parse(&format!("{pattern_body}:0"))
    } else {
        reqwest::Url::parse(&pattern_body)
    }
    .map_err(|error| format!("Invalid HTTP host pattern '{}': {}", pattern, error))?;

    let scheme = parsed_pattern.scheme().to_string();
    if scheme != "http" && scheme != "https" {
        return Err("HTTP host patterns must use http or https".to_string());
    }

    let host = parsed_pattern
        .host_str()
        .ok_or_else(|| format!("HTTP host pattern '{}' is missing a host", pattern))?
        .to_ascii_lowercase();

    let port = if wildcard_port {
        HostPatternPort::Any
    } else if let Some(explicit_port) = parsed_pattern.port() {
        HostPatternPort::Exact(explicit_port)
    } else {
        HostPatternPort::Any
    };

    if host.contains('*') {
        return Err(format!(
            "HTTP host pattern '{}' may only use a wildcard as a leading '*.' label",
            pattern
        ));
    }

    Ok(HostAllowlistPattern {
        scheme,
        host,
        port,
        match_subdomains,
    })
}

/// Returns the pattern with a leading `*.` host label removed, or None when absent.
fn strip_subdomain_wildcard(pattern: &str) -> Option<String> {
    for scheme_prefix in ["https://", "http://"] {
        if let Some(rest) = pattern.strip_prefix(scheme_prefix) {
            if let Some(host_rest) = rest.strip_prefix("*.") {
                return Some(format!("{scheme_prefix}{host_rest}"));
            }
        }
    }

    None
}

fn url_matches_host_allowlist_pattern(url: &reqwest::Url, pattern: &HostAllowlistPattern) -> bool {
    if url.scheme() != pattern.scheme {
        return false;
    }

    let Some(url_host) = url.host_str() else {
        return false;
    };

    let url_host = url_host.to_ascii_lowercase();

    // A wildcard admits the domain itself as well as anything beneath it, so declaring
    // `*.archive.org` does not also require declaring `archive.org`.
    let host_matches = url_host == pattern.host
        || (pattern.match_subdomains && url_host.ends_with(&format!(".{}", pattern.host)));

    if !host_matches {
        return false;
    }

    match pattern.port {
        HostPatternPort::Any => true,
        HostPatternPort::Exact(expected_port) => {
            url.port().unwrap_or(default_port_for_scheme(url.scheme())) == expected_port
        }
    }
}

pub fn url_matches_host_allowlist(url: &reqwest::Url, allowed_hosts: &[String]) -> bool {
    allowed_hosts.iter().any(|pattern_text| {
        parse_host_allowlist_pattern(pattern_text)
            .ok()
            .is_some_and(|pattern| url_matches_host_allowlist_pattern(url, &pattern))
    })
}

pub fn validate_extension_http_url(
    url: &str,
    allowed_hosts: Option<&[String]>,
) -> Result<reqwest::Url, String> {
    let parsed_url =
        reqwest::Url::parse(url).map_err(|error| format!("Invalid URL '{}': {}", url, error))?;
    let scheme = parsed_url.scheme();

    if scheme != "https" && scheme != "http" {
        return Err("Only http and https URLs are allowed".to_string());
    }

    if let Some(hosts) = allowed_hosts {
        if hosts.is_empty() {
            return Err("Access denied: HTTP host allowlist is empty".to_string());
        }

        if !url_matches_host_allowlist(&parsed_url, hosts) {
            return Err("Access denied: target host is not allowed".to_string());
        }

        return Ok(parsed_url);
    }

    Err("Access denied: HTTP host allowlist is required".to_string())
}

fn normalize_integrity_value(value: &str) -> String {
    value
        .trim()
        .strip_prefix("sha256:")
        .unwrap_or(value.trim())
        .to_lowercase()
}

fn hex_encode_digest(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

pub fn compute_sha256_hex(bytes: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    hex_encode_digest(hasher.finalize().as_slice())
}

pub fn verify_integrity_sha256_digest(
    digest: &[u8; 32],
    expected_integrity: Option<&str>,
) -> Result<(), String> {
    if let Some(expected) = expected_integrity {
        let expected_hash = normalize_integrity_value(expected);
        let actual_hash = hex_encode_digest(digest);
        if actual_hash != expected_hash {
            return Err(format!(
                "Integrity verification failed: expected sha256:{}, got sha256:{}",
                expected_hash, actual_hash
            ));
        }
    }

    Ok(())
}

pub fn verify_integrity(bytes: &[u8], expected_integrity: Option<&str>) -> Result<(), String> {
    if let Some(expected) = expected_integrity {
        let expected_hash = normalize_integrity_value(expected);
        let actual_hash = compute_sha256_hex(bytes);
        if actual_hash != expected_hash {
            return Err(format!(
                "Integrity verification failed: expected sha256:{}, got sha256:{}",
                expected_hash, actual_hash
            ));
        }
    }

    Ok(())
}

pub fn is_safe_managed_relative_path(path: &Path) -> bool {
    path.components()
        .all(|component| matches!(component, Component::Normal(_)))
}

pub fn validate_binary_path_component(value: &str, label: &str) -> Result<(), String> {
    let trimmed_value = value.trim();
    if trimmed_value.is_empty() {
        return Err(format!("{} cannot be empty", label));
    }

    let path = Path::new(trimmed_value);
    if path.is_absolute() || !is_safe_managed_relative_path(path) || path.components().count() != 1
    {
        return Err(format!(
            "Invalid {}: must be a single safe path component",
            label
        ));
    }

    Ok(())
}

pub fn validate_binary_relative_path(value: &str, label: &str) -> Result<PathBuf, String> {
    let trimmed_value = value.trim();
    if trimmed_value.is_empty() {
        return Err(format!("{} cannot be empty", label));
    }

    let path = PathBuf::from(trimmed_value);
    if path.is_absolute() || !is_safe_managed_relative_path(&path) {
        return Err(format!("Invalid {}: must be a safe relative path", label));
    }

    Ok(path)
}

#[cfg(test)]
mod host_wildcard_tests {
    use super::*;

    fn matches(pattern: &str, url: &str) -> bool {
        url_matches_host_allowlist(&reqwest::Url::parse(url).unwrap(), &[pattern.to_string()])
    }

    /// The case this exists for: the Cover Art Archive redirects to a per-request
    /// archive.org CDN host, so an extension cannot name the host it will end up on.
    #[test]
    fn a_subdomain_wildcard_admits_a_dynamic_cdn_host() {
        assert!(matches(
            "https://*.archive.org",
            "https://dn721902.ca.archive.org/0/items/mbid-x/front.jpg"
        ));
        assert!(matches(
            "https://*.archive.org",
            "https://ia801504.us.archive.org/thing.jpg"
        ));
    }

    #[test]
    fn a_subdomain_wildcard_admits_the_domain_itself() {
        assert!(matches("https://*.archive.org", "https://archive.org/download/x"));
    }

    #[test]
    fn a_subdomain_wildcard_does_not_admit_a_lookalike_domain() {
        assert!(!matches("https://*.archive.org", "https://archive.org.evil.com/x"));
        assert!(!matches("https://*.archive.org", "https://notarchive.org/x"));
        assert!(!matches("https://*.archive.org", "https://evil-archive.org/x"));
    }

    #[test]
    fn a_wildcard_does_not_cross_schemes() {
        assert!(!matches("https://*.archive.org", "http://ia1.archive.org/x"));
    }

    #[test]
    fn an_exact_pattern_still_rejects_subdomains() {
        assert!(matches("https://coverartarchive.org", "https://coverartarchive.org/release/x"));
        assert!(!matches("https://coverartarchive.org", "https://cdn.coverartarchive.org/x"));
    }

    #[test]
    fn a_wildcard_in_any_other_position_is_rejected() {
        assert!(parse_host_allowlist_pattern("https://ia*.archive.org").is_err());
        assert!(parse_host_allowlist_pattern("https://archive.*").is_err());
    }

    #[test]
    fn a_bare_wildcard_host_is_rejected() {
        assert!(parse_host_allowlist_pattern("https://*.").is_err());
        assert!(parse_host_allowlist_pattern("https://*").is_err());
    }

    /// The extension's real allowlist against the real redirect target, end to end.
    #[test]
    fn the_id3_extensions_allowlist_admits_the_cover_art_redirect() {
        let allowlist = vec![
            "https://musicbrainz.org".to_string(),
            "https://coverartarchive.org".to_string(),
            "https://*.archive.org".to_string(),
        ];

        for url in [
            "https://musicbrainz.org/ws/2/recording?query=x",
            "https://coverartarchive.org/release/639a1486/front-500",
            "https://archive.org/download/mbid-639a1486/front.jpg",
            "https://dn721902.ca.archive.org/0/items/mbid-639a1486/front_thumb500.jpg",
        ] {
            assert!(
                url_matches_host_allowlist(&reqwest::Url::parse(url).unwrap(), &allowlist),
                "should admit {url}"
            );
        }

        for url in ["https://evil.com/x", "https://archive.org.evil.com/x"] {
            assert!(
                !url_matches_host_allowlist(&reqwest::Url::parse(url).unwrap(), &allowlist),
                "should refuse {url}"
            );
        }
    }

    #[test]
    fn a_wildcard_combines_with_a_port_wildcard() {
        assert!(matches("https://*.archive.org:*", "https://ia1.archive.org:8443/x"));
    }
}
