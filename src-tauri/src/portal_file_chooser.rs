// SPDX-License-Identifier: GPL-3.0-or-later
// License: GNU GPLv3 or later. See the license file in the project root for more information.
// Copyright © 2026 Cortexist, LLC. All rights reserved.

//! Sigma as the system's file-open dialog: an `org.freedesktop.impl.portal.FileChooser`
//! backend.
//!
//! Applications do not launch file dialogs; they call the desktop portal over DBus and a
//! *backend* supplies the UI. This service is that backend. Each incoming request spawns one
//! `--file-picker` process (see `file_picker.rs`) and blocks its DBus reply on the process's
//! answer — concurrency, isolation, and the picker's own desktop identity all fall out of the
//! process boundary. The method call staying open for the dialog's whole life is the portal
//! protocol's own design: the daemon calls with an infinite timeout.
//!
//! The service runs only in the resident session process. Wiring it up system-wide (the
//! `.portal` file and `portals.conf` preference) is a registration step like the other two
//! defaults, not yet built; until then the service answers anyone who addresses it directly.

use std::collections::HashMap;

use zbus::zvariant::{ObjectPath, OwnedValue, Value};

const BUS_NAME: &str = "org.freedesktop.impl.portal.desktop.sigma";
const OBJECT_PATH: &str = "/org/freedesktop/portal/desktop";

/// Response codes the portal defines: success, cancelled by the user, ended otherwise.
const RESPONSE_SUCCESS: u32 = 0;
const RESPONSE_CANCELLED: u32 = 1;

/// One in-flight dialog, exported at the caller's handle path so the *application* can cancel
/// a dialog it no longer wants — closing its own window, say. Killing the picker process makes
/// the spawn return with no answer, which the caller reads as a cancel.
struct PickerDbusRequest {
    picker_pid: u32,
}

#[zbus::interface(name = "org.freedesktop.impl.portal.Request")]
impl PickerDbusRequest {
    fn close(&self) {
        unsafe {
            libc::kill(self.picker_pid as i32, libc::SIGTERM);
        }
    }
}

struct FileChooserBackend;

impl FileChooserBackend {
    /// Reduces the portal's option soup to what the picker honors.
    fn request_from_options(
        title: &str,
        options: &HashMap<String, OwnedValue>,
    ) -> crate::file_picker::PickerRequest {
        let flag = |key: &str| {
            options
                .get(key)
                .and_then(|value| bool::try_from(value.clone()).ok())
                .unwrap_or(false)
        };

        // The spec sends the folder as a NUL-terminated byte string, paths not being text.
        let current_folder = options
            .get("current_folder")
            .and_then(|value| <Vec<u8>>::try_from(value.clone()).ok())
            .map(|mut bytes| {
                if bytes.last() == Some(&0) {
                    bytes.pop();
                }
                String::from_utf8_lossy(&bytes).into_owned()
            })
            .filter(|folder| !folder.is_empty());

        crate::file_picker::PickerRequest {
            title: title.to_string(),
            multiple: flag("multiple"),
            directory: flag("directory"),
            current_folder,
        }
    }

    /// Runs one dialog to completion: spawn the picker process, wait for its answer on
    /// stdout, translate to the portal's reply shape.
    async fn run_picker(
        request: crate::file_picker::PickerRequest,
        handle: ObjectPath<'_>,
        object_server: &zbus::ObjectServer,
    ) -> (u32, HashMap<String, OwnedValue>) {
        let payload = match serde_json::to_string(&request) {
            Ok(payload) => payload,
            Err(_) => return (RESPONSE_CANCELLED, HashMap::new()),
        };

        let executable = match crate::xdg_associations::executable_path() {
            Ok(path) => path,
            Err(_) => return (RESPONSE_CANCELLED, HashMap::new()),
        };

        let child = std::process::Command::new(executable)
            .arg(crate::file_picker::FILE_PICKER_CLI_FLAG)
            .arg(payload)
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::null())
            .spawn();

        let child = match child {
            Ok(child) => child,
            Err(error) => {
                log::warn!("Failed to spawn a file picker: {error}");
                return (RESPONSE_CANCELLED, HashMap::new());
            }
        };

        // Exported for the dialog's lifetime so the caller can abandon it.
        let handle = handle.into_owned();
        let request_object = PickerDbusRequest {
            picker_pid: child.id(),
        };
        let exported = object_server.at(&handle, request_object).await.is_ok();

        let output = tauri::async_runtime::spawn_blocking(move || child.wait_with_output()).await;

        if exported {
            let _ = object_server.remove::<PickerDbusRequest, _>(&handle).await;
        }

        let stdout = match output {
            Ok(Ok(output)) => output.stdout,
            _ => return (RESPONSE_CANCELLED, HashMap::new()),
        };

        let uris: Vec<String> = serde_json::from_slice::<serde_json::Value>(&stdout)
            .ok()
            .and_then(|reply| {
                reply
                    .get("uris")
                    .and_then(|list| serde_json::from_value(list.clone()).ok())
            })
            .unwrap_or_default();

        if uris.is_empty() {
            return (RESPONSE_CANCELLED, HashMap::new());
        }

        let mut results = HashMap::new();
        if let Ok(value) = OwnedValue::try_from(Value::new(uris)) {
            results.insert("uris".to_string(), value);
        }

        (RESPONSE_SUCCESS, results)
    }
}

#[zbus::interface(name = "org.freedesktop.impl.portal.FileChooser")]
impl FileChooserBackend {
    async fn open_file(
        &self,
        handle: ObjectPath<'_>,
        _app_id: String,
        _parent_window: String,
        title: String,
        options: HashMap<String, OwnedValue>,
        #[zbus(object_server)] object_server: &zbus::ObjectServer,
    ) -> (u32, HashMap<String, OwnedValue>) {
        let request = Self::request_from_options(&title, &options);
        Self::run_picker(request, handle, object_server).await
    }

    /// Not built yet; a clean cancel lets applications fall back gracefully rather than hang.
    async fn save_file(
        &self,
        _handle: ObjectPath<'_>,
        _app_id: String,
        _parent_window: String,
        _title: String,
        _options: HashMap<String, OwnedValue>,
    ) -> (u32, HashMap<String, OwnedValue>) {
        (RESPONSE_CANCELLED, HashMap::new())
    }

    #[zbus(property)]
    fn version(&self) -> u32 {
        1
    }
}

/// Claims the backend name and serves until the session ends. A failure is logged rather
/// than fatal: the file manager works fine without the portal role.
pub fn start() {
    tauri::async_runtime::spawn(async {
        let connection = zbus::connection::Builder::session()
            .and_then(|builder| builder.name(BUS_NAME))
            .and_then(|builder| builder.serve_at(OBJECT_PATH, FileChooserBackend));

        let connection = match connection {
            Ok(builder) => builder.build().await,
            Err(error) => Err(error),
        };

        match connection {
            Ok(_connection) => {
                log::info!("File chooser portal backend serving as {BUS_NAME}");
                // Held for the life of the session; dropping it would drop the bus name.
                std::future::pending::<()>().await;
            }
            Err(error) => {
                log::warn!("File chooser portal backend not started: {error}");
            }
        }
    });
}
