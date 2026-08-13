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

/// One running picker, from spawn to answer.
///
/// Both callers that need a dialog go through this: the portal backend answering another
/// application, and the file manager answering itself. Sharing the spawn means there is one
/// picker in the system rather than a second, quietly divergent one — and it is what keeps a
/// dialog raised inside Sigma looking like the dialog Sigma raises for everyone else.
pub struct PickerProcess {
    child: std::process::Child,
}

impl PickerProcess {
    /// Launches a picker for `request`. The process boundary is the reply channel, so its
    /// stdout is piped; stderr is not, because a picker has nothing to say there.
    pub fn spawn(request: &PickerRequest) -> Result<Self, String> {
        let payload = serde_json::to_string(request)
            .map_err(|error| format!("Failed to encode the picker request: {error}"))?;
        let executable = crate::xdg_associations::executable_path()?;

        let child = std::process::Command::new(executable)
            .arg(FILE_PICKER_CLI_FLAG)
            .arg(payload)
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::null())
            .spawn()
            .map_err(|error| format!("Failed to spawn a file picker: {error}"))?;

        Ok(Self { child })
    }

    /// The picker's process id, which is how a caller abandons a dialog it no longer wants.
    pub fn pid(&self) -> u32 {
        self.child.id()
    }

    /// Waits for the answer. An empty list is a cancel, and so is any failure to read one:
    /// a dialog that could not be completed has chosen nothing, which is what a caller
    /// already knows how to handle.
    pub async fn wait_for_uris(self) -> Vec<String> {
        let child = self.child;
        let output = tauri::async_runtime::spawn_blocking(move || child.wait_with_output()).await;

        let stdout = match output {
            Ok(Ok(output)) => output.stdout,
            _ => return Vec::new(),
        };

        parse_picker_reply(&stdout)
    }

    /// The same wait for callers that already have a thread to spare and no async runtime
    /// to reach — the GTK main loop's helper being the one that does.
    pub fn wait_for_uris_blocking(self) -> Vec<String> {
        match self.child.wait_with_output() {
            Ok(output) => parse_picker_reply(&output.stdout),
            Err(_) => Vec::new(),
        }
    }
}

/// Reads the `{"uris": [...]}` line a finished picker writes to stdout.
pub(crate) fn parse_picker_reply(stdout: &[u8]) -> Vec<String> {
    serde_json::from_slice::<serde_json::Value>(stdout)
        .ok()
        .and_then(|reply| {
            reply
                .get("uris")
                .and_then(|list| serde_json::from_value(list.clone()).ok())
        })
        .unwrap_or_default()
}

/// Converts the picker's reply to local paths. Anything that is not a local file is dropped:
/// the in-app callers all go on to read or write the result through the filesystem.
pub fn uris_to_paths(uris: &[String]) -> Vec<String> {
    uris.iter()
        .filter_map(|uri| url::Url::parse(uri).ok())
        .filter_map(|url| url.to_file_path().ok())
        .map(|path| path.to_string_lossy().to_string())
        .collect()
}

/// Raises Sigma's own picker for Sigma's own dialogs, in place of the platform one.
///
/// Returns the chosen paths, empty for a cancel. Spawning failures are reported rather than
/// flattened into a cancel, so the caller can fall back to a platform dialog instead of
/// leaving the user with a button that silently does nothing.
#[tauri::command]
pub async fn file_picker_open(request: PickerRequest) -> Result<Vec<String>, String> {
    let picker = PickerProcess::spawn(&request)?;

    Ok(uris_to_paths(&picker.wait_for_uris().await))
}

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
    fn a_reply_yields_the_chosen_uris() {
        let uris = parse_picker_reply(br#"{"uris":["file:///home/z/a.mp3"]}"#);

        assert_eq!(uris, vec!["file:///home/z/a.mp3".to_string()]);
    }

    /// A picker that died, or wrote nothing, chose nothing. Callers read that as a cancel.
    #[test]
    fn an_unreadable_reply_is_an_empty_choice() {
        assert!(parse_picker_reply(b"").is_empty());
        assert!(parse_picker_reply(b"not json").is_empty());
        assert!(parse_picker_reply(br#"{"error":"nope"}"#).is_empty());
    }

    #[test]
    fn a_cancel_is_an_empty_uri_list() {
        assert!(parse_picker_reply(br#"{"uris":[]}"#).is_empty());
    }

    #[test]
    fn uris_come_back_as_local_paths() {
        let paths = uris_to_paths(&["file:///home/z/My%20Music/a.mp3".to_string()]);

        assert_eq!(paths, vec!["/home/z/My Music/a.mp3".to_string()]);
    }

    /// The in-app callers all go on to read or write through the filesystem, so anything
    /// that is not a local file has no path for them to use.
    #[test]
    fn remote_and_malformed_uris_are_dropped() {
        let paths = uris_to_paths(&[
            "https://example.com/a.mp3".to_string(),
            "not a uri".to_string(),
            "file:///home/z/keep.mp3".to_string(),
        ]);

        assert_eq!(paths, vec!["/home/z/keep.mp3".to_string()]);
    }

    /// The request the file manager sends itself has to survive the same argv round trip
    /// the portal backend's does, since both spawn the identical picker.
    #[test]
    fn an_in_app_request_round_trips_through_the_command_line() {
        let request = PickerRequest {
            title: "Select extension folder".to_string(),
            directory: true,
            current_folder: Some("/home/z/Workspaces".to_string()),
            ..Default::default()
        };

        let payload = serde_json::to_string(&request).unwrap();
        let parsed = picker_request_from_args(&args(&[FILE_PICKER_CLI_FLAG, &payload]))
            .expect("picker mode");

        assert_eq!(parsed.title, "Select extension folder");
        assert!(parsed.directory);
        assert!(!parsed.save);
        assert_eq!(parsed.current_folder.as_deref(), Some("/home/z/Workspaces"));
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
