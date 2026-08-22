// SPDX-License-Identifier: GPL-3.0-or-later
// License: GNU GPLv3 or later. See the license file in the project root for more information.
// Copyright © 2021 - present Aleksey Hoffman. All rights reserved.

use std::net::{IpAddr, Ipv4Addr, SocketAddr};

use super::types::{HTTPS_DEFAULT_PORT, HTTP_DEFAULT_PORT, PORT_RANGE_END, PORT_RANGE_START};

pub(super) fn get_local_ipv4() -> Result<Ipv4Addr, String> {
    let interfaces = local_ip_address::list_afinet_netifas()
        .map_err(|err| format!("Failed to enumerate network interfaces: {err}"))?;

    let mut best_ip: Option<Ipv4Addr> = None;
    let mut best_priority: u8 = 0;

    for (_name, ip) in &interfaces {
        if let IpAddr::V4(ipv4) = ip {
            if ipv4.is_loopback() || ipv4.is_link_local() || ipv4.is_unspecified() {
                continue;
            }

            if !is_private_lan_ip(ipv4) {
                continue;
            }

            let priority = lan_ip_priority(ipv4);
            if priority > best_priority {
                best_priority = priority;
                best_ip = Some(*ipv4);
            }
        }
    }

    best_ip.ok_or_else(|| {
        "No suitable LAN IPv4 address found. Make sure you are connected to a local network.".into()
    })
}

fn is_private_lan_ip(ip: &Ipv4Addr) -> bool {
    let octets = ip.octets();
    matches!(octets, [192, 168, ..] | [10, ..] | [172, 16..=31, ..])
}

fn lan_ip_priority(ip: &Ipv4Addr) -> u8 {
    let octets = ip.octets();
    match octets {
        [192, 168, ..] => 3,
        [10, ..] => 2,
        [172, 16..=31, ..] => 2,
        _ => 0,
    }
}

/// Resolves the port to serve on: a `fixed` port is used exactly or the share fails —
/// drifting to a neighboring port would silently invalidate the bookmarks, QR codes,
/// and DNS entries a pinned port exists for. Without one, falls back to the scan.
pub(super) fn resolve_port(
    fixed: Option<u16>,
    preferred: u16,
    exclude: &[u16],
) -> Result<u16, String> {
    let Some(port) = fixed else {
        return find_available_port(preferred, exclude);
    };

    if exclude.contains(&port) {
        return Err(format!("Port {port} is already used by the other protocol"));
    }

    if std::net::TcpListener::bind(SocketAddr::from(([0, 0, 0, 0], port))).is_err() {
        return Err(format!("Port {port} is already in use"));
    }

    Ok(port)
}

pub(super) fn find_available_port(preferred: u16, exclude: &[u16]) -> Result<u16, String> {
    if !exclude.contains(&preferred)
        && std::net::TcpListener::bind(SocketAddr::from(([0, 0, 0, 0], preferred))).is_ok()
    {
        return Ok(preferred);
    }

    for port in PORT_RANGE_START..=PORT_RANGE_END {
        if exclude.contains(&port) {
            continue;
        }
        if std::net::TcpListener::bind(SocketAddr::from(([0, 0, 0, 0], port))).is_ok() {
            return Ok(port);
        }
    }
    Err("No available port found".into())
}

pub(super) fn format_http_url(host: &str, port: u16) -> String {
    if port == HTTP_DEFAULT_PORT {
        format!("http://{host}")
    } else {
        format!("http://{host}:{port}")
    }
}

pub(super) fn format_https_url(host: &str, port: u16) -> String {
    if port == HTTPS_DEFAULT_PORT {
        format!("https://{host}")
    } else {
        format!("https://{host}:{port}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn grab_free_port() -> (std::net::TcpListener, u16) {
        let listener = std::net::TcpListener::bind("0.0.0.0:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        (listener, port)
    }

    #[test]
    fn fixed_port_is_used_exactly_when_free() {
        let (listener, port) = grab_free_port();
        drop(listener);

        assert_eq!(resolve_port(Some(port), 80, &[]), Ok(port));
    }

    #[test]
    fn fixed_port_in_use_fails_naming_the_port() {
        let (_listener, port) = grab_free_port();

        let error = resolve_port(Some(port), 80, &[]).unwrap_err();
        assert!(error.contains(&port.to_string()), "{error}");
        assert!(error.contains("already in use"), "{error}");
    }

    #[test]
    fn fixed_port_clashing_with_the_other_protocol_fails() {
        let error = resolve_port(Some(55001), 80, &[55001]).unwrap_err();
        assert!(error.contains("other protocol"), "{error}");
    }
}
