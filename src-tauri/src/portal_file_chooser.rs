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
//! The service has two hosts. DBus activation launches the dedicated headless process
//! (`--sigma-portal-service` → `run_service()`): no GTK, no windows, no webviews — an
//! application asking for a file dialog must never boot a file-manager session, the same
//! standalone rule the viewer and picker processes follow. A resident session also claims
//! the name at startup (`start()`); its claim queues behind the service's and takes over if
//! the service ever dies. System-wide wiring (the `.portal` file and the DBus activation
//! service) is `file_chooser_registration.rs`. Being DBus-activated puts a deadline on the
//! claim: see the doc comment on `start()`.

use std::collections::HashMap;

use zbus::zvariant::{ObjectPath, OwnedValue, Value};

const BUS_NAME: &str = "org.freedesktop.impl.portal.desktop.sigma";
const OBJECT_PATH: &str = "/org/freedesktop/portal/desktop";

/// The CLI flag the portal's DBus activation service launches with. A process carrying it is
/// diverted into `run_service()` before any Tauri, GTK, or webview work.
pub const PORTAL_SERVICE_CLI_FLAG: &str = "--sigma-portal-service";

pub fn launched_as_portal_service(args: &[String]) -> bool {
    args.iter().any(|arg| arg == PORTAL_SERVICE_CLI_FLAG)
}

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
            save: false,
            suggested_name: None,
        }
    }

    /// A save request suggests a name and a folder. `current_name` is the app's suggestion
    /// for a fresh save; `current_file` is the path being saved over, whose folder and
    /// basename are the suggestion when nothing else says otherwise.
    fn save_request_from_options(
        title: &str,
        options: &HashMap<String, OwnedValue>,
    ) -> crate::file_picker::PickerRequest {
        let mut request = Self::request_from_options(title, options);
        request.save = true;
        request.multiple = false;
        request.directory = false;

        request.suggested_name = options
            .get("current_name")
            .and_then(|value| String::try_from(value.clone()).ok())
            .filter(|name| !name.is_empty());

        if let Some(current_file) = options
            .get("current_file")
            .and_then(|value| <Vec<u8>>::try_from(value.clone()).ok())
            .map(|mut bytes| {
                if bytes.last() == Some(&0) {
                    bytes.pop();
                }
                std::path::PathBuf::from(String::from_utf8_lossy(&bytes).into_owned())
            })
        {
            if request.suggested_name.is_none() {
                request.suggested_name = current_file
                    .file_name()
                    .map(|name| name.to_string_lossy().into_owned());
            }
            if request.current_folder.is_none() {
                request.current_folder = current_file
                    .parent()
                    .map(|parent| parent.to_string_lossy().into_owned());
            }
        }

        request
    }

    /// Packs a URI list into the portal's reply shape; an empty list is the user's cancel.
    fn reply(uris: Vec<String>) -> (u32, HashMap<String, OwnedValue>) {
        if uris.is_empty() {
            return (RESPONSE_CANCELLED, HashMap::new());
        }

        let mut results = HashMap::new();
        if let Ok(value) = OwnedValue::try_from(Value::new(uris)) {
            results.insert("uris".to_string(), value);
        }

        (RESPONSE_SUCCESS, results)
    }

    /// Runs one dialog to completion: spawn the picker process, wait for its answer on
    /// stdout, hand back the chosen URIs — empty when cancelled or anything failed.
    async fn run_picker(
        request: crate::file_picker::PickerRequest,
        handle: ObjectPath<'_>,
        object_server: &zbus::ObjectServer,
    ) -> Vec<String> {
        let payload = match serde_json::to_string(&request) {
            Ok(payload) => payload,
            Err(_) => return Vec::new(),
        };

        let executable = match crate::xdg_associations::executable_path() {
            Ok(path) => path,
            Err(_) => return Vec::new(),
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
                return Vec::new();
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
            _ => return Vec::new(),
        };

        serde_json::from_slice::<serde_json::Value>(&stdout)
            .ok()
            .and_then(|reply| {
                reply
                    .get("uris")
                    .and_then(|list| serde_json::from_value(list.clone()).ok())
            })
            .unwrap_or_default()
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
        Self::reply(Self::run_picker(request, handle, object_server).await)
    }

    async fn save_file(
        &self,
        handle: ObjectPath<'_>,
        _app_id: String,
        _parent_window: String,
        title: String,
        options: HashMap<String, OwnedValue>,
        #[zbus(object_server)] object_server: &zbus::ObjectServer,
    ) -> (u32, HashMap<String, OwnedValue>) {
        let request = Self::save_request_from_options(&title, &options);
        Self::reply(Self::run_picker(request, handle, object_server).await)
    }

    /// Several files land in one chosen folder: the dialog picks the destination directory,
    /// and the requested names are joined onto it.
    async fn save_files(
        &self,
        handle: ObjectPath<'_>,
        _app_id: String,
        _parent_window: String,
        title: String,
        options: HashMap<String, OwnedValue>,
        #[zbus(object_server)] object_server: &zbus::ObjectServer,
    ) -> (u32, HashMap<String, OwnedValue>) {
        let mut request = Self::request_from_options(&title, &options);
        request.directory = true;
        request.multiple = false;
        request.save = false;

        let chosen = Self::run_picker(request, handle, object_server).await;
        let Some(folder) = chosen
            .first()
            .and_then(|uri| url::Url::parse(uri).ok())
            .and_then(|uri| uri.to_file_path().ok())
        else {
            return (RESPONSE_CANCELLED, HashMap::new());
        };

        let names: Vec<Vec<u8>> = options
            .get("files")
            .and_then(|value| <Vec<Vec<u8>>>::try_from(value.clone()).ok())
            .unwrap_or_default();

        let uris: Vec<String> = names
            .into_iter()
            .map(|mut bytes| {
                if bytes.last() == Some(&0) {
                    bytes.pop();
                }
                folder.join(String::from_utf8_lossy(&bytes).into_owned())
            })
            .filter_map(|path| url::Url::from_file_path(path).ok())
            .map(String::from)
            .collect();

        Self::reply(uris)
    }

    #[zbus(property)]
    fn version(&self) -> u32 {
        1
    }
}

/// Claims the backend name and serves until the session ends. A failure is logged rather
/// than fatal: the file manager works fine without the portal role.
///
/// Must be called before GTK init, and nothing here may wait on the frontend:
/// xdg-desktop-portal blocks its own startup until this name appears (giving up after the
/// 25-second DBus activation timeout and leaving the session without a FileChooser), while
/// GTK init can itself call synchronously into the still-blocked xdg-desktop-portal. The
/// picker-process-per-dialog design is what makes the early claim safe — serving a dialog
/// never needs this process's own window or webview.
pub fn start() {
    tauri::async_runtime::spawn(async {
        match build_connection().await {
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

/// The headless portal service: claims the backend name and serves dialogs. The picker
/// process a request spawns is the only UI this process ever causes.
///
/// Blocks its caller until the service has no reason to exist, so nothing after it in
/// `run()` — Tauri, GTK, windows — ever happens. `eprintln!` rather than `log`: no logger is
/// initialized on this path, and stderr is what the journal captures for a DBus-activated
/// unit.
pub fn run_service() -> ! {
    // Its own process identity, like the viewer and picker: status indicators must not
    // report the file manager running when only the dialog service is resident.
    crate::standalone_viewer::adopt_process_identity("sigma-portal");

    let exit_code = tauri::async_runtime::block_on(serve());
    std::process::exit(exit_code);
}

/// Serves until the bus name is lost or the connection dies, then returns the exit code.
///
/// Exiting instead of lingering is what keeps the fleet clean: a service that does not own
/// the name is useless (observed 2026-08-10 — ownership moved while the old service parked
/// forever, one 50 MB orphan per activation), and DBus activation starts a fresh one the
/// next time a dialog needs it.
async fn serve() -> i32 {
    use futures_util::StreamExt;
    use zbus::fdo::{DBusProxy, RequestNameFlags, RequestNameReply};

    let connection = zbus::connection::Builder::session()
        .and_then(|builder| builder.serve_at(OBJECT_PATH, FileChooserBackend));
    let connection = match connection {
        Ok(builder) => builder.build().await,
        Err(error) => Err(error),
    };
    let connection = match connection {
        Ok(connection) => connection,
        Err(error) => {
            eprintln!("File chooser portal service not started: {error}");
            return 1;
        }
    };

    // Subscribed before the claim so a loss in between is never missed.
    let mut name_lost = match DBusProxy::new(&connection).await {
        Ok(proxy) => match proxy.receive_name_lost().await {
            Ok(stream) => stream,
            Err(error) => {
                eprintln!("File chooser portal service not started: {error}");
                return 1;
            }
        },
        Err(error) => {
            eprintln!("File chooser portal service not started: {error}");
            return 1;
        }
    };

    match connection
        .request_name_with_flags(BUS_NAME, RequestNameFlags::DoNotQueue.into())
        .await
    {
        Ok(RequestNameReply::PrimaryOwner) => {}
        Ok(_) => {
            // A resident session already serves dialogs; this activation was redundant.
            eprintln!("File chooser portal service: {BUS_NAME} already has an owner");
            return 0;
        }
        Err(error) => {
            eprintln!("File chooser portal service failed to claim {BUS_NAME}: {error}");
            return 1;
        }
    }

    eprintln!("File chooser portal service serving as {BUS_NAME}");

    while let Some(signal) = name_lost.next().await {
        if let Ok(args) = signal.args() {
            if args.name.as_str() == BUS_NAME {
                eprintln!("File chooser portal service lost {BUS_NAME}; exiting for reactivation");
                return 0;
            }
        }
    }

    eprintln!("File chooser portal service bus connection closed; exiting");
    1
}

async fn build_connection() -> zbus::Result<zbus::Connection> {
    zbus::connection::Builder::session()?
        .name(BUS_NAME)?
        .serve_at(OBJECT_PATH, FileChooserBackend)?
        .build()
        .await
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(list: &[&str]) -> Vec<String> {
        list.iter().map(|value| value.to_string()).collect()
    }

    #[test]
    fn only_the_portal_service_flag_selects_the_service_role() {
        assert!(launched_as_portal_service(&args(&[
            "sigma-file-manager",
            PORTAL_SERVICE_CLI_FLAG,
        ])));
        assert!(!launched_as_portal_service(&args(&[
            "sigma-file-manager",
            "--sigma-autostart",
        ])));
        assert!(!launched_as_portal_service(&args(&["sigma-file-manager"])));
    }
}
