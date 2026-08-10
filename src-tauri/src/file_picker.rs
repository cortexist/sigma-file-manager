// SPDX-License-Identifier: GPL-3.0-or-later
// License: GNU GPLv3 or later. See the license file in the project root for more information.
// Copyright © 2026 Cortexist, LLC. All rights reserved.

//! A file-picker process: one dialog, one request, one answer.
//!
//! Launched as `sigma-file-manager --file-picker '<request json>'` by the portal backend in
//! the resident session (see `portal_file_chooser.rs`), never by a user. The process serves
//! exactly one request: the page bootstraps itself from `file_picker_request`, the user picks
//! or cancels, and `file_picker_finish` writes the answer to stdout — the process boundary is
//! the reply channel — and exits. Concurrent dialogs are simply concurrent processes, which
//! is also what lets each carry the picker's own identity instead of the file manager's.

use std::io::Write;

pub const FILE_PICKER_CLI_FLAG: &str = "--file-picker";

/// One choice in the caller's type filter: a display name and the patterns it admits. The
/// portal sends each pattern tagged as glob or MIME type; they are split here so the page
/// never re-parses tags.
#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct PickerFilter {
    /// The requesting application's own label, e.g. "Images".
    pub name: String,
    /// Glob patterns, e.g. `*.png`.
    pub globs: Vec<String>,
    /// MIME types, possibly wildcarded, e.g. `image/*`.
    pub mimes: Vec<String>,
}

/// What the caller asked for, reduced to what the picker honors.
#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct PickerRequest {
    /// The requesting application's own words, e.g. "Open Firmware Image".
    pub title: String,
    pub multiple: bool,
    /// Picking a directory rather than files.
    pub directory: bool,
    pub current_folder: Option<String>,
    /// Choosing a destination that may not exist yet, rather than an existing file. Brings
    /// the filename field, and existing names demand an explicit replace.
    pub save: bool,
    pub suggested_name: Option<String>,
    /// The caller's type filters, in its order; empty means everything is welcome.
    pub filters: Vec<PickerFilter>,
    /// Name of the filter the caller wants preselected.
    pub current_filter: Option<String>,
}

/// The request this process was launched for; `None` in every other kind of session.
pub struct PickerSession(pub Option<PickerRequest>);

/// The flag's presence decides the mode; a payload that fails to parse still opens a picker
/// with defaults rather than falling through to a full file-manager session, because whoever
/// passed the flag asked for a dialog, not a window full of tabs.
pub fn picker_request_from_args(args: &[String]) -> Option<PickerRequest> {
    let mut iter = args.iter();

    while let Some(arg) = iter.next() {
        if arg == FILE_PICKER_CLI_FLAG {
            let payload = iter.next().map(String::as_str).unwrap_or("");
            return Some(serde_json::from_str(payload).unwrap_or_default());
        }
    }

    None
}

/// How the picker page learns what it exists to ask.
#[tauri::command]
pub fn file_picker_request(state: tauri::State<PickerSession>) -> Option<PickerRequest> {
    state.0.clone()
}

/// The answer, written where the spawning backend is reading. An empty list is a cancel —
/// there is no third outcome a dialog can have.
#[tauri::command]
pub fn file_picker_finish(app: tauri::AppHandle, paths: Vec<String>) {
    let uris: Vec<String> = paths
        .iter()
        .filter_map(|path| url::Url::from_file_path(path).ok())
        .map(String::from)
        .collect();

    let reply = serde_json::json!({ "uris": uris });
    let mut stdout = std::io::stdout();
    let _ = writeln!(stdout, "{reply}");
    let _ = stdout.flush();

    app.exit(0);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(list: &[&str]) -> Vec<String> {
        std::iter::once("sigma-file-manager")
            .chain(list.iter().copied())
            .map(String::from)
            .collect()
    }

    #[test]
    fn a_launch_without_the_flag_is_not_a_picker() {
        assert!(picker_request_from_args(&args(&[])).is_none());
        assert!(picker_request_from_args(&args(&["/some/video.mp4"])).is_none());
    }

    #[test]
    fn the_request_rides_in_as_json() {
        let request = picker_request_from_args(&args(&[
            FILE_PICKER_CLI_FLAG,
            r#"{"title":"Open Image","multiple":true,"currentFolder":"/home/z/Pictures"}"#,
        ]))
        .expect("picker mode");

        assert_eq!(request.title, "Open Image");
        assert!(request.multiple);
        assert_eq!(request.current_folder.as_deref(), Some("/home/z/Pictures"));
        assert!(!request.directory);
    }

    /// The flag is a promise of a dialog; a mangled payload must not break it.
    #[test]
    fn a_bad_payload_still_opens_a_picker_with_defaults() {
        let request = picker_request_from_args(&args(&[FILE_PICKER_CLI_FLAG, "not json"]))
            .expect("picker mode");

        assert_eq!(request.title, "");
        assert!(!request.multiple);
    }

    #[test]
    fn a_missing_payload_still_opens_a_picker_with_defaults() {
        assert!(picker_request_from_args(&args(&[FILE_PICKER_CLI_FLAG])).is_some());
    }

    #[test]
    fn filters_ride_in_split_by_kind() {
        let request = picker_request_from_args(&args(&[
            FILE_PICKER_CLI_FLAG,
            r#"{"title":"Open","filters":[{"name":"Images","globs":["*.png"],"mimes":["image/*"]}],"currentFilter":"Images"}"#,
        ]))
        .expect("picker mode");

        assert_eq!(request.filters.len(), 1);
        assert_eq!(request.filters[0].name, "Images");
        assert_eq!(request.filters[0].globs, vec!["*.png"]);
        assert_eq!(request.filters[0].mimes, vec!["image/*"]);
        assert_eq!(request.current_filter.as_deref(), Some("Images"));
    }
}
