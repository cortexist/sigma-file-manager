// SPDX-License-Identifier: GPL-3.0-or-later
// License: GNU GPLv3 or later. See the license file in the project root for more information.
// Copyright © 2026 Cortexist, LLC. All rights reserved.

//! Sigma's picker for the web content Sigma hosts.
//!
//! An extension that asks for a file through the extension API gets Sigma's picker. An
//! extension that embeds a web application does not: the embedded page asks the *browser*
//! for a file, with `<input type="file">` or `showOpenFilePicker`, and the browser answers
//! with its own dialog. Excalidraw is the case in hand — saving goes through the API and
//! looks like Sigma, opening goes through the page and looks like GTK, and neither the
//! extension nor its author can do anything about it.
//!
//! WebKit offers `run-file-chooser` for exactly this: handle it and the native dialog
//! never appears. The request is translated into the same `PickerRequest` the portal
//! backend and the app's own dialogs use, so there is still one picker in the system
//! rather than a third way of asking for a file.

#[cfg(target_os = "linux")]
mod imp {
    use std::sync::mpsc::{self, TryRecvError};
    use std::time::Duration;

    use webkit2gtk::glib;
    use webkit2gtk::{FileChooserRequest, FileChooserRequestExt, WebViewExt};

    use crate::file_picker::{uris_to_paths, PickerFilter, PickerProcess, PickerRequest};

    /// How often the GTK main loop looks for the picker's answer. A person is choosing a
    /// file, so this is far below anything perceptible and costs nothing while idle.
    const ANSWER_POLL_INTERVAL: Duration = Duration::from_millis(50);

    /// The request carries no title of its own, and the page that raised it is not named
    /// anywhere the handler can see, so the picker gets a plain one.
    const DEFAULT_TITLE: &str = "Open File";

    /// Translates WebKit's request into the picker's own.
    ///
    /// MIME types come across directly. The `GtkFileFilter` a page may also supply cannot
    /// be read back out through GTK 3 — its patterns are write-only — so a page that
    /// filters purely by extension gets a picker showing everything rather than a wrong
    /// subset. Showing too much is recoverable; hiding the file someone wants is not.
    fn picker_request_from_chooser(request: &FileChooserRequest) -> PickerRequest {
        let mimes: Vec<String> = request
            .mime_types()
            .into_iter()
            .map(|mime| mime.to_string())
            .collect();

        build_picker_request(mimes, request.selects_multiple())
    }

    pub(super) fn build_picker_request(mime_types: Vec<String>, multiple: bool) -> PickerRequest {
        let mimes: Vec<String> = mime_types
            .into_iter()
            .filter(|mime| !mime.trim().is_empty())
            .collect();

        let filters = if mimes.is_empty() {
            Vec::new()
        } else {
            vec![PickerFilter {
                name: "Supported files".to_string(),
                globs: Vec::new(),
                mimes,
            }]
        };

        PickerRequest {
            title: DEFAULT_TITLE.to_string(),
            multiple,
            directory: false,
            current_folder: None,
            save: false,
            suggested_name: None,
            filters,
            current_filter: None,
        }
    }

    /// Waits for the picker off the main loop, then answers the request on it.
    ///
    /// The request is a GObject and belongs to the GTK thread, so it cannot travel to a
    /// worker. The wait happens on a plain thread that owns only the child process, and
    /// the answer comes back through a channel the main loop drains.
    fn answer_when_picker_finishes(request: FileChooserRequest, picker: PickerProcess) {
        let (sender, receiver) = mpsc::channel::<Vec<String>>();

        std::thread::spawn(move || {
            let _ = sender.send(picker.wait_for_uris_blocking());
        });

        glib::timeout_add_local(ANSWER_POLL_INTERVAL, move || match receiver.try_recv() {
            Ok(uris) => {
                let paths = uris_to_paths(&uris);

                if paths.is_empty() {
                    request.cancel();
                } else {
                    let borrowed: Vec<&str> = paths.iter().map(String::as_str).collect();
                    request.select_files(&borrowed);
                }

                glib::ControlFlow::Break
            }
            // The sending thread is gone without an answer: treat it as a cancel rather
            // than leaving the page waiting on a dialog that will never return.
            Err(TryRecvError::Disconnected) => {
                request.cancel();
                glib::ControlFlow::Break
            }
            Err(TryRecvError::Empty) => glib::ControlFlow::Continue,
        });
    }

    pub fn install(window: &tauri::WebviewWindow) {
        let _ = window.with_webview(|platform_webview| {
            platform_webview
                .inner()
                .connect_run_file_chooser(|_webview, request| {
                    let picker_request = picker_request_from_chooser(request);

                    match PickerProcess::spawn(&picker_request) {
                        Ok(picker) => {
                            answer_when_picker_finishes(request.clone(), picker);
                            // Handled: WebKit leaves its own dialog alone.
                            true
                        }
                        // A picker that will not start is a broken install, not a reason to
                        // leave the user with no dialog at all. WebKit shows its own.
                        Err(error) => {
                            log::warn!("Falling back to the platform file chooser: {error}");
                            false
                        }
                    }
                });
        });
    }
}

/// Call with a window as it is created.
///
/// Not at setup: the windows are declared `create: false` and built later in the setup
/// handler, so at setup time there is no webview to attach anything to and the lookup
/// quietly finds nothing.
#[cfg(target_os = "linux")]
pub fn install_webview_file_chooser(window: &tauri::WebviewWindow) {
    imp::install(window);
}

/// Only WebKitGTK routes its file dialogs through a signal the host can answer. The other
/// platforms' webviews show their native dialog with no way to intervene, which is also
/// what a user of those platforms expects to see.
#[cfg(not(target_os = "linux"))]
pub fn install_webview_file_chooser(_window: &tauri::WebviewWindow) {}

#[cfg(all(test, target_os = "linux"))]
mod tests {
    use super::imp::build_picker_request;

    #[test]
    fn a_page_asking_for_images_gets_a_matching_filter() {
        let request = build_picker_request(
            vec!["image/png".to_string(), "image/jpeg".to_string()],
            false,
        );

        assert_eq!(request.filters.len(), 1);
        assert_eq!(request.filters[0].mimes, vec!["image/png", "image/jpeg"]);
        assert!(!request.multiple);
    }

    /// A page that filters only by extension gives WebKit a GtkFileFilter, whose patterns
    /// GTK 3 will not hand back. Showing everything is the honest outcome; showing a
    /// guessed subset would hide the file the user came for.
    #[test]
    fn a_request_with_no_mime_types_filters_nothing() {
        let request = build_picker_request(Vec::new(), false);

        assert!(request.filters.is_empty());
    }

    #[test]
    fn empty_mime_entries_do_not_become_a_filter_that_matches_nothing() {
        let request = build_picker_request(vec![String::new(), "   ".to_string()], false);

        assert!(request.filters.is_empty());
    }

    #[test]
    fn multiple_selection_carries_across() {
        assert!(build_picker_request(Vec::new(), true).multiple);
    }

    /// This is an upload dialog: never a directory, never a save.
    #[test]
    fn it_is_always_an_open_request_for_files() {
        let request = build_picker_request(vec!["text/plain".to_string()], true);

        assert!(!request.directory);
        assert!(!request.save);
        assert!(request.suggested_name.is_none());
        assert!(!request.title.is_empty());
    }
}
