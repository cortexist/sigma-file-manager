// SPDX-License-Identifier: GPL-3.0-or-later
// License: GNU GPLv3 or later. See the license file in the project root for more information.
// Copyright © 2021 - present Aleksey Hoffman. All rights reserved.
// Copyright © 2026 Cortexist, LLC (modifications). All rights reserved.

use std::net::SocketAddr;
use std::path::PathBuf;

use super::handlers::{build_ftp_router, build_stream_dir_router, build_stream_router};
use super::mdns::{register_mdns, unregister_mdns};
use super::network::{format_http_url, format_https_url, get_local_ipv4, resolve_port};
use super::streaming::canonicalize_hub_paths;
use super::tls::{generate_self_signed_tls, load_tls_from_files};
use super::types::{
    ActiveServer, LanShareConfig, LanShareResult, ShareState, ACTIVE_SERVER, HTTPS_DEFAULT_PORT,
    HTTP_DEFAULT_PORT, MDNS_DOMAIN,
};

pub async fn start_lan_share(
    path: String,
    share_mode: String,
    hub_paths: Option<Vec<String>>,
    config: Option<LanShareConfig>,
) -> Result<LanShareResult, String> {
    let config = config.unwrap_or_default();

    if !config.enable_http && !config.enable_https {
        return Err("LAN share needs HTTP or HTTPS enabled".into());
    }

    let custom_hostname = config
        .custom_hostname
        .as_deref()
        .map(str::trim)
        .filter(|hostname| !hostname.is_empty());

    let cert_path = config
        .cert_path
        .as_deref()
        .map(str::trim)
        .filter(|path| !path.is_empty());
    let key_path = config
        .key_path
        .as_deref()
        .map(str::trim)
        .filter(|path| !path.is_empty());

    let uses_custom_cert = match (cert_path, key_path) {
        (Some(_), Some(_)) => true,
        (None, None) => false,
        _ => {
            return Err(
                "A custom certificate needs both the certificate file and the key file".into(),
            )
        }
    };

    stop_lan_share_inner().await?;

    let hub_paths = hub_paths.filter(|paths| paths.len() >= 2);

    let state = if let Some(paths) = hub_paths {
        if share_mode != "stream" {
            return Err("Multi-file share requires stream mode".into());
        }
        let canonical = canonicalize_hub_paths(&paths)?;
        let share_path = canonical[0]
            .parent()
            .ok_or_else(|| "Invalid hub path".to_string())?
            .to_path_buf();
        ShareState {
            share_path,
            file_hub: Some(canonical),
        }
    } else {
        let share_path = PathBuf::from(&path);
        if !share_path.exists() {
            return Err("Path does not exist".into());
        }
        ShareState {
            share_path,
            file_hub: None,
        }
    };

    let local_ip = get_local_ipv4()?;

    let is_directory = state.share_path.is_dir();
    let router = match share_mode.as_str() {
        "stream" if state.file_hub.is_some() => build_stream_dir_router(state.clone()),
        "stream" if is_directory => build_stream_dir_router(state.clone()),
        "stream" => build_stream_router(state.clone()),
        "ftp" => build_ftp_router(state.clone()),
        _ => return Err(format!("Unknown share mode: {share_mode}")),
    };

    let https_router = router.clone();

    let (http_shutdown, http_task, http_port) = if config.enable_http {
        // A fixed HTTPS port is reserved before the HTTP scan so an automatic HTTP
        // port cannot land on it and doom the HTTPS bind that follows.
        let http_exclude: Vec<u16> = if config.enable_https {
            config.https_port.into_iter().collect()
        } else {
            Vec::new()
        };
        let http_port = resolve_port(config.http_port, HTTP_DEFAULT_PORT, &http_exclude)?;
        let (http_shutdown_tx, mut http_shutdown_rx) = tokio::sync::watch::channel(false);
        let http_addr = SocketAddr::from(([0, 0, 0, 0], http_port));
        let http_listener = tokio::net::TcpListener::bind(http_addr)
            .await
            .map_err(|err| format!("Failed to bind HTTP port {http_port}: {err}"))?;

        let http_task = tokio::spawn(async move {
            axum::serve(http_listener, router)
                .with_graceful_shutdown(async move {
                    while http_shutdown_rx.changed().await.is_ok() {
                        if *http_shutdown_rx.borrow() {
                            break;
                        }
                    }
                })
                .await
                .ok();
        });

        (Some(http_shutdown_tx), Some(http_task), Some(http_port))
    } else {
        (None, None, None)
    };

    let https_setup = if config.enable_https {
        let exclude: Vec<u16> = http_port.into_iter().collect();

        let setup = async {
            let https_port = resolve_port(config.https_port, HTTPS_DEFAULT_PORT, &exclude)?;

            let tls_config = if let (Some(cert_path), Some(key_path)) = (cert_path, key_path) {
                load_tls_from_files(cert_path, key_path).await?
            } else {
                generate_self_signed_tls(local_ip, custom_hostname).await?
            };

            Ok::<_, String>((https_port, tls_config))
        }
        .await;

        match setup {
            Ok((https_port, tls_config)) => {
                let handle = axum_server::Handle::new();
                let shutdown_handle = handle.clone();
                let https_addr = SocketAddr::from(([0, 0, 0, 0], https_port));

                let task = tokio::spawn(async move {
                    axum_server::bind_rustls(https_addr, tls_config)
                        .handle(handle)
                        .serve(https_router.into_make_service())
                        .await
                        .ok();
                });

                Some((shutdown_handle, https_port, task))
            }
            Err(err) => {
                // With HTTP on, a share without TLS is degraded; alone, it is no share at all.
                if !config.enable_http {
                    return Err(format!("Failed to start HTTPS server: {err}"));
                }
                log::warn!("TLS setup failed (HTTP still works): {err}");
                None
            }
        }
    } else {
        None
    };

    let https_port = https_setup.as_ref().map(|(_, port, _)| *port);

    let mdns_port = http_port
        .or(https_port)
        .ok_or_else(|| "No server started".to_string())?;

    // The mDNS name is only usable where following it works: always over HTTP, but over
    // HTTPS only under the self-signed certificate, which names it in a SAN. A share no
    // client could reach through the name does not broadcast it at all — a network kept
    // on names of its own (a .lan domain, say) stays free of a stray .local one.
    let mdns_name_usable = http_port.is_some() || !uses_custom_cert;

    let mdns_daemon = if mdns_name_usable {
        match register_mdns(mdns_port, local_ip) {
            Ok(daemon) => Some(daemon),
            Err(err) => {
                log::warn!("mDNS registration failed (sharing still works via IP): {err}");
                None
            }
        }
    } else {
        None
    };

    let has_mdns = mdns_daemon.is_some();
    let local_ip_string = local_ip.to_string();

    // HTTP keeps being the door most clients get pointed at while it is on, since the
    // self-signed default makes plain HTTPS visits stop on a browser warning.
    let primary_url = |host: &str| match http_port {
        Some(port) => format_http_url(host, port),
        None => format_https_url(host, https_port.unwrap_or(HTTPS_DEFAULT_PORT)),
    };

    let hostname_address = custom_hostname
        .map(&primary_url)
        .or_else(|| has_mdns.then(|| primary_url(MDNS_DOMAIN)));

    let ios_address = match (https_port, http_port) {
        (Some(https_port), Some(_)) => {
            let host = custom_hostname.unwrap_or(if has_mdns && !uses_custom_cert {
                MDNS_DOMAIN
            } else {
                &local_ip_string
            });
            Some(format_https_url(host, https_port))
        }
        _ => None,
    };

    let mut server_lock = ACTIVE_SERVER.lock().await;
    *server_lock = Some(ActiveServer {
        http_shutdown,
        http_task,
        https_handle: https_setup.as_ref().map(|(handle, _, _)| handle.clone()),
        https_task: https_setup.map(|(_, _, task)| task),
        mdns_daemon,
    });

    Ok(LanShareResult {
        address: primary_url(&local_ip_string),
        hostname_address,
        ios_address,
    })
}

pub async fn stop_lan_share_inner() -> Result<(), String> {
    let mut server_lock = ACTIVE_SERVER.lock().await;
    if let Some(server) = server_lock.take() {
        if let Some(shutdown) = server.http_shutdown {
            let _ = shutdown.send(true);
        }
        if let Some(handle) = server.https_handle {
            handle.graceful_shutdown(Some(std::time::Duration::from_secs(2)));
        }
        if let Some(ref daemon) = server.mdns_daemon {
            unregister_mdns(daemon);
        }
        if let Some(task) = server.http_task {
            let _ = task.await;
        }
        if let Some(task) = server.https_task {
            let _ = task.await;
        }
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    }
    Ok(())
}

pub fn get_local_ip() -> Result<String, String> {
    get_local_ipv4().map(|ip| ip.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Needs a LAN interface and free ports, so it is run by hand:
    /// `cargo test -- --ignored lan_share::server`.
    #[tokio::test(flavor = "multi_thread")]
    #[ignore]
    async fn https_only_share_serves_under_the_supplied_certificate() {
        let ca_key = rcgen::KeyPair::generate().unwrap();
        let mut ca_params = rcgen::CertificateParams::new(Vec::new()).unwrap();
        ca_params.is_ca = rcgen::IsCa::Ca(rcgen::BasicConstraints::Unconstrained);
        let ca_cert = ca_params.self_signed(&ca_key).unwrap();

        let leaf_key = rcgen::KeyPair::generate().unwrap();
        let leaf_params = rcgen::CertificateParams::new(vec!["localhost".to_string()]).unwrap();
        let issuer = rcgen::Issuer::from_params(&ca_params, &ca_key);
        let leaf_cert = leaf_params.signed_by(&leaf_key, &issuer).unwrap();

        let dir = tempfile::tempdir().unwrap();
        let cert_path = dir.path().join("server.crt");
        let key_path = dir.path().join("server.key");
        std::fs::write(&cert_path, format!("{}{}", leaf_cert.pem(), ca_cert.pem())).unwrap();
        std::fs::write(&key_path, leaf_key.serialize_pem()).unwrap();
        std::fs::write(dir.path().join("shared.txt"), "shared").unwrap();

        let config = LanShareConfig {
            enable_http: false,
            enable_https: true,
            http_port: None,
            https_port: Some(55911),
            cert_path: Some(cert_path.to_string_lossy().into_owned()),
            key_path: Some(key_path.to_string_lossy().into_owned()),
            custom_hostname: Some("localhost".to_string()),
        };

        let result = start_lan_share(
            dir.path().to_string_lossy().into_owned(),
            "ftp".to_string(),
            None,
            Some(config),
        )
        .await
        .unwrap();

        assert!(result.address.starts_with("https://"), "{}", result.address);
        assert_eq!(result.ios_address, None);
        let hostname_address = result.hostname_address.unwrap();
        assert_eq!(hostname_address, "https://localhost:55911");

        // A client that trusts only the test CA must be able to fetch the share page.
        let client = reqwest::Client::builder()
            .add_root_certificate(reqwest::Certificate::from_pem(ca_cert.pem().as_bytes()).unwrap())
            .build()
            .unwrap();
        let response = client.get(&hostname_address).send().await.unwrap();
        assert!(response.status().is_success(), "{}", response.status());

        // The HTTP port is off: nothing may answer plaintext on the HTTPS port's plain scheme.
        let https_port: u16 = hostname_address.rsplit(':').next().unwrap().parse().unwrap();
        let plain = client
            .get(format!("http://localhost:{https_port}"))
            .send()
            .await;
        assert!(plain.is_err() || !plain.unwrap().status().is_success());

        stop_lan_share_inner().await.unwrap();
    }
}
