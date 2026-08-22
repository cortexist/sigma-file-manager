// SPDX-License-Identifier: GPL-3.0-or-later
// License: GNU GPLv3 or later. See the license file in the project root for more information.
// Copyright © 2021 - present Aleksey Hoffman. All rights reserved.
// Copyright © 2026 Cortexist, LLC (modifications). All rights reserved.

use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::Arc;

use serde::{Deserialize, Serialize};
use tokio::sync::Mutex;

pub(super) const MDNS_SERVICE_TYPE: &str = "_http._tcp.local.";
pub(super) const MDNS_HOSTNAME: &str = "sfm.local.";
pub(super) const MDNS_DOMAIN: &str = "sfm.local";
pub(super) const MDNS_INSTANCE_NAME: &str = "Sigma File Manager";
pub(super) const HTTP_DEFAULT_PORT: u16 = 80;
pub(super) const HTTPS_DEFAULT_PORT: u16 = 443;
pub(super) const PORT_RANGE_START: u16 = 55000;
pub(super) const PORT_RANGE_END: u16 = 55999;
pub(super) const FTP_MAX_UPLOAD_BYTES: usize = 512 * 1024 * 1024;

pub(super) static FTP_HTML: &str = include_str!("../../assets/lan_share/lan_share_ftp.html");
pub(super) static STREAM_HTML: &str = include_str!("../../assets/lan_share/lan_share_stream.html");
pub(super) static APP_ICON_PNG: &[u8] = include_bytes!("../../icons/128x128.png");

#[derive(Clone)]
pub(super) struct ShareState {
    pub(super) share_path: PathBuf,
    pub(super) file_hub: Option<Vec<PathBuf>>,
}

/// How the share is served, from the LAN share section of the user's settings.
///
/// Absent fields fall back to the historical behavior — HTTP and HTTPS both on,
/// HTTPS under a generated self-signed certificate — so a caller that sends no
/// config gets the same server as before the settings existed.
#[derive(Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LanShareConfig {
    #[serde(default = "default_true")]
    pub enable_http: bool,
    #[serde(default = "default_true")]
    pub enable_https: bool,
    /// PEM certificate for HTTPS, leaf first, chain allowed. Paired with `key_path`;
    /// when both are unset a self-signed certificate is generated instead.
    #[serde(default)]
    pub cert_path: Option<String>,
    #[serde(default)]
    pub key_path: Option<String>,
    /// Hostname advertised in share URLs in place of the mDNS name — for networks
    /// whose DNS (and certificate) cover a name of the user's own.
    #[serde(default)]
    pub custom_hostname: Option<String>,
}

fn default_true() -> bool {
    true
}

impl Default for LanShareConfig {
    fn default() -> Self {
        Self {
            enable_http: true,
            enable_https: true,
            cert_path: None,
            key_path: None,
            custom_hostname: None,
        }
    }
}

pub(super) struct ActiveServer {
    pub(super) http_shutdown: Option<tokio::sync::watch::Sender<bool>>,
    pub(super) http_task: Option<tokio::task::JoinHandle<()>>,
    pub(super) https_handle: Option<axum_server::Handle<SocketAddr>>,
    pub(super) https_task: Option<tokio::task::JoinHandle<()>>,
    pub(super) mdns_daemon: Option<mdns_sd::ServiceDaemon>,
}

pub(super) static ACTIVE_SERVER: once_cell::sync::Lazy<Arc<Mutex<Option<ActiveServer>>>> =
    once_cell::sync::Lazy::new(|| Arc::new(Mutex::new(None)));

#[derive(Serialize)]
pub struct LanShareResult {
    pub address: String,
    /// Full URL under a friendly name (the custom hostname, or the mDNS name when
    /// a client following it would not hit a certificate for some other name).
    pub hostname_address: Option<String>,
    pub ios_address: Option<String>,
}

#[derive(Serialize)]
pub(super) struct DirEntryInfo {
    pub(super) name: String,
    pub(super) is_dir: bool,
    pub(super) size: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[serde(rename = "fileId")]
    pub(super) file_id: Option<String>,
}

#[derive(Deserialize)]
pub(super) struct ListQuery {
    pub(super) path: Option<String>,
}

#[derive(Deserialize)]
pub(super) struct UploadQuery {
    pub(super) path: Option<String>,
}
