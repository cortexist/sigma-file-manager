// SPDX-License-Identifier: GPL-3.0-or-later
// License: GNU GPLv3 or later. See the license file in the project root for more information.
// Copyright © 2026 Cortexist, LLC. All rights reserved.

//! `org.freedesktop.FileManager1`: the interface behind every "Show in Folder".
//!
//! Browsers and download managers do not open a directory when they reveal a file — they
//! want it *selected* — so they call this interface first and only fall back to a plain
//! directory open without it. Whoever owns the bus name gets those clicks; without this
//! service they went to whichever file manager's activation file shipped with the system,
//! MIME defaults notwithstanding.
//!
//! Requests are translated to paths here and handed to the main window's JS, which already
//! knows how to open a folder with a file revealed — the same flow CLI file arguments use.
//! Because a request can arrive over DBus activation *before* that listener exists (the
//! activated app is still booting), every request is also queued, and the frontend drains
//! the queue once its listener is up. Late delivery beats lost clicks.

pub const SHOW_IN_FOLDER_EVENT: &str = "file-manager:show";

/// What a request asks the file manager to display.
#[derive(Debug, Clone, Default, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ShowRequest {
    /// Files to reveal: open the parent folder with the file selected.
    pub items: Vec<String>,
    /// Folders to open outright.
    pub folders: Vec<String>,
}

impl ShowRequest {
    fn is_empty(&self) -> bool {
        self.items.is_empty() && self.folders.is_empty()
    }
}

/// Requests that arrived before the frontend was listening.
#[derive(Default)]
pub struct PendingShowRequests(std::sync::Mutex<Vec<ShowRequest>>);

/// The frontend calls this once its listener is registered; anything queued during boot is
/// returned in arrival order. Live requests keep flowing through the event.
#[tauri::command]
pub fn drain_show_in_folder_requests(state: tauri::State<PendingShowRequests>) -> Vec<ShowRequest> {
    std::mem::take(&mut *state.0.lock().expect("pending show requests lock"))
}

#[cfg(target_os = "linux")]
mod service {
    use super::{PendingShowRequests, ShowRequest, SHOW_IN_FOLDER_EVENT};
    use tauri::{Emitter, Manager};

    const BUS_NAME: &str = "org.freedesktop.FileManager1";
    const OBJECT_PATH: &str = "/org/freedesktop/FileManager1";

    fn uris_to_paths(uris: Vec<String>) -> Vec<String> {
        uris.iter()
            .filter_map(|uri| url::Url::parse(uri).ok())
            .filter_map(|uri| uri.to_file_path().ok())
            .map(|path| path.to_string_lossy().into_owned())
            .collect()
    }

    struct FileManager1 {
        app: tauri::AppHandle,
    }

    impl FileManager1 {
        fn dispatch(&self, request: ShowRequest) {
            if request.is_empty() {
                return;
            }

            self.app
                .state::<PendingShowRequests>()
                .0
                .lock()
                .expect("pending show requests lock")
                .push(request.clone());

            let _ = self.app.emit(SHOW_IN_FOLDER_EVENT, request);
        }
    }

    #[zbus::interface(name = "org.freedesktop.FileManager1")]
    impl FileManager1 {
        fn show_items(&self, uris: Vec<String>, _startup_id: String) {
            self.dispatch(ShowRequest {
                items: uris_to_paths(uris),
                folders: Vec::new(),
            });
        }

        fn show_folders(&self, uris: Vec<String>, _startup_id: String) {
            self.dispatch(ShowRequest {
                items: Vec::new(),
                folders: uris_to_paths(uris),
            });
        }

        /// No properties dialog exists yet; revealing the file is the honest approximation
        /// rather than ignoring the click.
        fn show_item_properties(&self, uris: Vec<String>, _startup_id: String) {
            self.dispatch(ShowRequest {
                items: uris_to_paths(uris),
                folders: Vec::new(),
            });
        }
    }

    /// Claims the name and serves for the session's life. Losing the claim — another file
    /// manager already running and holding it — is logged, not fatal.
    pub fn start(app: tauri::AppHandle) {
        tauri::async_runtime::spawn(async move {
            let backend = FileManager1 { app };
            let connection = zbus::connection::Builder::session()
                .and_then(|builder| builder.name(BUS_NAME))
                .and_then(|builder| builder.serve_at(OBJECT_PATH, backend));

            let connection = match connection {
                Ok(builder) => builder.build().await,
                Err(error) => Err(error),
            };

            match connection {
                Ok(_connection) => {
                    log::info!("Serving {BUS_NAME}");
                    std::future::pending::<()>().await;
                }
                Err(error) => {
                    log::warn!("FileManager1 service not started: {error}");
                }
            }
        });
    }
}

#[cfg(target_os = "linux")]
pub use service::start;

#[cfg(not(target_os = "linux"))]
pub fn start(_app: tauri::AppHandle) {}
