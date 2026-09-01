// SPDX-License-Identifier: GPL-3.0-or-later
// License: GNU GPLv3 or later. See the license file in the project root for more information.
// Copyright © 2021 - present Aleksey Hoffman. All rights reserved.

use base64::engine::general_purpose::STANDARD as BASE64_STANDARD;
use base64::Engine;
use std::path::Path;

pub fn canonicalize_path(path: &Path) -> String {
    #[cfg(windows)]
    {
        return prepare_shell_path(&path.to_string_lossy());
    }

    #[cfg(not(windows))]
    match path.canonicalize() {
        Ok(canonical) => {
            let path_str = canonical.to_string_lossy().to_string();
            if let Some(stripped) = path_str.strip_prefix(r"\\?\") {
                stripped.to_string()
            } else {
                path_str
            }
        }
        Err(_) => path.to_string_lossy().to_string(),
    }
}

#[cfg(windows)]
pub fn path_for_selection(file_path: &str) -> String {
    file_path.replace('/', "\\")
}

#[cfg(windows)]
pub fn parent_directory_for_selection(file_path: &str) -> Option<std::path::PathBuf> {
    Path::new(&path_for_selection(file_path))
        .parent()
        .map(std::path::Path::to_path_buf)
}

#[cfg(windows)]
pub fn normalize_selection_path_for_comparison(file_path: &str) -> String {
    let mut normalized_path = path_for_selection(file_path).replace('\\', "/");

    while normalized_path.len() > 1 && normalized_path.ends_with('/') {
        normalized_path.pop();
    }

    normalized_path
}

#[cfg(windows)]
pub fn common_parent_directory_for_selections(
    file_paths: &[String],
) -> Result<std::path::PathBuf, String> {
    if file_paths.is_empty() {
        return Err("No paths provided".to_string());
    }

    let parent_directories: Vec<std::path::PathBuf> = file_paths
        .iter()
        .map(|file_path| {
            parent_directory_for_selection(file_path)
                .ok_or_else(|| format!("Path has no parent: {}", path_for_selection(file_path)))
        })
        .collect::<Result<Vec<_>, _>>()?;

    let expected_parent = &parent_directories[0];
    if !parent_directories
        .iter()
        .all(|parent_directory| parent_directory == expected_parent)
    {
        return Err(
            "Selected items must be in the same folder for combined properties".to_string(),
        );
    }

    Ok(expected_parent.clone())
}

#[cfg(windows)]
pub fn strip_extended_path_prefix(path: &str) -> String {
    if let Some(rest) = path.strip_prefix(r"\\?\UNC\") {
        format!(r"\\{rest}")
    } else if let Some(rest) = path.strip_prefix(r"\\?\") {
        rest.to_string()
    } else {
        path.to_string()
    }
}

#[cfg(windows)]
pub fn prepare_shell_path(file_path: &str) -> String {
    let native_path = path_for_selection(file_path);
    let path = Path::new(&native_path);
    let resolved = dunce::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
    let resolved_path = strip_extended_path_prefix(&resolved.to_string_lossy());
    resolved_path.replace('/', "\\")
}

#[cfg(windows)]
pub fn shell_path_candidates(file_path: &str) -> Vec<String> {
    let native_path = path_for_selection(file_path);
    let path = Path::new(&native_path);
    let mut candidates: Vec<String> = Vec::new();

    let mut push_candidate = |candidate: String| {
        if !candidate.is_empty() && !candidates.iter().any(|existing| existing == &candidate) {
            candidates.push(candidate);
        }
    };

    push_candidate(file_path.replace('/', "\\"));

    let prepared_path = prepare_shell_path(file_path);
    push_candidate(prepared_path.clone());

    if path.is_dir() {
        let directory_path = prepared_path.trim_end_matches('\\');
        push_candidate(format!("{directory_path}\\"));
    }

    candidates
}

pub fn get_program_icon(exe_path: &str) -> Option<String> {
    crate::system_icons::get_path_icon_data_url(Path::new(exe_path), 32)
}

pub fn load_png_as_base64(path: &std::path::Path) -> Option<String> {
    let data = std::fs::read(path).ok()?;

    if data.starts_with(&[0x89, 0x50, 0x4E, 0x47]) {
        let base64_png = BASE64_STANDARD.encode(&data);
        return Some(format!("data:image/png;base64,{base64_png}"));
    }

    None
}

/// Load an icon as a data URL, accepting both formats a freedesktop icon theme
/// actually ships: raster PNG and scalable SVG.
///
/// SVG is passed through rather than rasterised because the only consumer is an
/// `<img>` in the webview, which renders it at whatever density the display
/// asks for — sharper than the fixed-size PNG it replaces, and no new
/// dependency. `<img>` is also what makes this safe: it renders SVG in a
/// restricted mode with no script execution and no external fetches, so a
/// third-party `.desktop` pointing at a hostile SVG has nothing to reach.
#[cfg(target_os = "linux")]
pub fn load_icon_as_data_url(path: &std::path::Path) -> Option<String> {
    if let Some(png) = load_png_as_base64(path) {
        return Some(png);
    }
    load_svg_as_data_url(path)
}

#[cfg(target_os = "linux")]
fn load_svg_as_data_url(path: &std::path::Path) -> Option<String> {
    if !path
        .extension()
        .is_some_and(|ext| ext.eq_ignore_ascii_case("svg"))
    {
        return None;
    }

    let data = std::fs::read(path).ok()?;

    // The PNG branch above proves its format from magic bytes; SVG has none, so
    // confirm the file actually opens as XML before trusting the extension. A
    // raster file misnamed `.svg` would otherwise be handed to the webview
    // labelled `image/svg+xml` and render as a broken image.
    if !looks_like_svg(&data) {
        return None;
    }

    let base64_svg = BASE64_STANDARD.encode(&data);
    Some(format!("data:image/svg+xml;base64,{base64_svg}"))
}

/// True when the bytes open like an SVG document: an `<svg` root, possibly
/// behind a BOM, an XML declaration, a doctype or comments. Only the head of
/// the file is examined — enough to cover the preamble without walking a large
/// icon.
#[cfg(target_os = "linux")]
fn looks_like_svg(data: &[u8]) -> bool {
    let head = &data[..data.len().min(1024)];
    let text = String::from_utf8_lossy(head);
    let text = text.trim_start_matches('\u{feff}');
    text.contains("<svg") || (text.contains("<?xml") && text.contains("svg"))
}

#[cfg(all(test, windows))]
mod shell_path_tests {
    use super::{path_for_selection, prepare_shell_path, strip_extended_path_prefix};

    #[test]
    fn strip_extended_path_prefix_strips_extended_unc_prefix() {
        assert_eq!(
            strip_extended_path_prefix(r"\\?\UNC\wsl.localhost\Ubuntu-24.04\"),
            r"\\wsl.localhost\Ubuntu-24.04\",
        );
        assert_eq!(
            strip_extended_path_prefix(r"\\?\UNC\wsl.localhost\docker-desktop\"),
            r"\\wsl.localhost\docker-desktop\",
        );
    }

    #[test]
    fn strip_extended_path_prefix_strips_extended_local_prefix() {
        assert_eq!(
            strip_extended_path_prefix(r"\\?\C:\Users\aleks\file.txt"),
            r"C:\Users\aleks\file.txt",
        );
    }

    #[test]
    fn strip_extended_path_prefix_leaves_regular_paths_unchanged() {
        assert_eq!(
            strip_extended_path_prefix(r"\\wsl.localhost\Ubuntu-24.04\"),
            r"\\wsl.localhost\Ubuntu-24.04\",
        );
    }

    #[test]
    fn path_for_selection_converts_unc_paths_to_backslashes() {
        assert_eq!(
            path_for_selection("//server.lan/share/file.png"),
            r"\\server.lan\share\file.png",
        );
    }

    #[test]
    fn prepare_shell_path_converts_unc_paths_to_backslashes() {
        assert_eq!(
            prepare_shell_path("//server.lan/share/file.png"),
            r"\\server.lan\share\file.png",
        );
    }

    #[test]
    fn prepare_shell_path_converts_local_windows_paths_to_backslashes() {
        assert_eq!(
            prepare_shell_path("C:/Users/aleks/Documents/file.txt"),
            r"C:\Users\aleks\Documents\file.txt",
        );
    }
}
