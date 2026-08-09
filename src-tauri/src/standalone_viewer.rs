// SPDX-License-Identifier: GPL-3.0-or-later
// License: GNU GPLv3 or later. See the license file in the project root for more information.
// Copyright © 2026 Cortexist, LLC. All rights reserved.

//! Launching sigma with a media file opens Quick View alone — no file manager.
//!
//! This is what lets sigma be registered as the system's image, video, and audio viewer:
//! a double-click in another application should cost one lightweight viewer window, not a
//! whole file-manager session. The main window is simply never created; Quick View already
//! knows how to resolve its filmstrip from disk, and the app's existing exit rule — quit
//! when nothing is visible — already treats no window's label as special.
//!
//! The same shape is the template for future self-standing windows (a file-open dialog,
//! say): declare the window `create: false` in `tauri.conf.json`, decide here at launch
//! which window a session gets, and let the page bootstrap itself from a command.

use std::path::{Path, PathBuf};

/// Mirrors `determineFileType` in `src/stores/runtime/quick-view.ts` — the page must agree
/// that whatever this accepts is something it can show. Keep the two in sync.
const MEDIA_EXTENSIONS: [&str; 29] = [
    // Images
    "jpg", "jpeg", "png", "gif", "webp", "svg", "bmp", "ico", "tiff", "tif", "avif",
    // Video
    "mp4", "webm", "ogg", "ogv", "mov", "avi", "mkv", "m4v", "wmv", "flv", // Audio
    "mp3", "wav", "oga", "flac", "aac", "m4a", "wma", "opus",
];

/// The file a standalone viewer session was launched for; `None` in a normal session.
pub struct StandaloneLaunchFile(pub Option<String>);

/// The first argument that is an existing media file, resolved against `cwd`.
///
/// Flags are skipped rather than rejected so `--flag file.mp4` still views the file, and a
/// path that merely *looks* like media but does not exist falls through to the normal launch
/// path, where the file manager's own handling can report it.
pub fn media_file_from_args(args: &[String], cwd: Option<&Path>) -> Option<PathBuf> {
    args.iter().skip(1).find_map(|arg| {
        if arg.starts_with('-') {
            return None;
        }

        let raw = Path::new(arg);
        let path = if raw.is_absolute() {
            raw.to_path_buf()
        } else {
            cwd?.join(raw)
        };

        let extension = path.extension()?.to_str()?.to_ascii_lowercase();

        if !MEDIA_EXTENSIONS.contains(&extension.as_str()) {
            return None;
        }

        if !path.is_file() {
            return None;
        }

        dunce::canonicalize(&path).ok()
    })
}

/// Builds one of the `create: false` windows declared in `tauri.conf.json`.
pub fn create_window_from_config(
    app: &tauri::AppHandle,
    label: &str,
) -> Result<tauri::WebviewWindow, Box<dyn std::error::Error>> {
    let config = app
        .config()
        .app
        .windows
        .iter()
        .find(|window| window.label == label)
        .cloned()
        .ok_or_else(|| format!("No window labeled {label} in the app config"))?;

    Ok(tauri::WebviewWindowBuilder::from_config(app, &config)?.build()?)
}

/// How the Quick View page learns it is a whole session rather than a limb of the main
/// window: `Some(path)` means show this file and treat closing as quitting.
#[tauri::command]
pub fn standalone_launch_file(state: tauri::State<StandaloneLaunchFile>) -> Option<String> {
    state.0.clone()
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

    /// A file that exists for the duration of the test, in a directory we can use as cwd.
    struct TempMedia {
        dir: PathBuf,
        path: PathBuf,
    }

    impl TempMedia {
        fn new(name: &str) -> Self {
            let dir =
                std::env::temp_dir().join(format!("sfm-standalone-test-{}", std::process::id()));
            std::fs::create_dir_all(&dir).expect("temp dir");
            let path = dir.join(name);
            std::fs::write(&path, b"x").expect("temp file");
            Self { dir, path }
        }
    }

    impl Drop for TempMedia {
        fn drop(&mut self) {
            let _ = std::fs::remove_file(&self.path);
            let _ = std::fs::remove_dir(&self.dir);
        }
    }

    #[test]
    fn finds_an_existing_media_file_by_absolute_path() {
        let media = TempMedia::new("clip.mp4");
        let found = media_file_from_args(&args(&[media.path.to_str().unwrap()]), None);

        assert_eq!(found, dunce::canonicalize(&media.path).ok());
    }

    #[test]
    fn resolves_a_relative_path_against_the_launch_directory() {
        let media = TempMedia::new("photo.jpg");
        let found = media_file_from_args(&args(&["photo.jpg"]), Some(&media.dir));

        assert_eq!(found, dunce::canonicalize(&media.path).ok());
    }

    /// `--flag video.mp4` views the file; the flag is someone else's business.
    #[test]
    fn skips_flags_rather_than_rejecting_the_launch() {
        let media = TempMedia::new("clip.mkv");
        let found =
            media_file_from_args(&args(&["--some-flag", media.path.to_str().unwrap()]), None);

        assert!(found.is_some());
    }

    /// A directory argument is a file-manager launch, not a viewer launch.
    #[test]
    fn ignores_directories_and_non_media_files() {
        let media = TempMedia::new("notes.txt");

        assert_eq!(
            media_file_from_args(&args(&[media.dir.to_str().unwrap()]), None),
            None
        );
        assert_eq!(
            media_file_from_args(&args(&[media.path.to_str().unwrap()]), None),
            None
        );
    }

    /// A media-looking path that does not exist falls through to the normal launch, where
    /// the file manager can say so, rather than opening a viewer on nothing.
    #[test]
    fn ignores_media_paths_that_do_not_exist() {
        assert_eq!(
            media_file_from_args(&args(&["/nonexistent/clip.mp4"]), None),
            None,
        );
    }

    #[test]
    fn a_bare_launch_is_not_a_viewer_launch() {
        assert_eq!(media_file_from_args(&args(&[]), None), None);
    }
}
