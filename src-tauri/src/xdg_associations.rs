// SPDX-License-Identifier: GPL-3.0-or-later
// License: GNU GPLv3 or later. See the license file in the project root for more information.
// Copyright © 2026 Cortexist, LLC. All rights reserved.

//! The mechanics of owning a MIME association on Linux, shared by every role
//! this app can register for — default file manager, default media viewer.
//!
//! An association is two things: a desktop entry in the user's applications
//! directory naming this executable, and a line in `mimeapps.list` pointing a
//! MIME type at that entry. `xdg-mime` owns writing the line — it knows the
//! per-desktop quirks that hand-editing would get wrong — while the entry, and
//! the *removal* of lines (there is no `xdg-mime unset`), are handled here.

use std::path::PathBuf;
use std::process::Command;

/// Sections of `mimeapps.list` that can name an entry; both are cleaned on disable.
const ASSOCIATION_SECTIONS: [&str; 2] = ["[Default Applications]", "[Added Associations]"];

fn home_dir() -> Result<PathBuf, String> {
    std::env::var_os("HOME")
        .map(PathBuf::from)
        .ok_or_else(|| "HOME is not set, so the desktop entry has nowhere to live".to_string())
}

fn data_home() -> Result<PathBuf, String> {
    match std::env::var_os("XDG_DATA_HOME") {
        Some(value) if !value.is_empty() => Ok(PathBuf::from(value)),
        _ => Ok(home_dir()?.join(".local/share")),
    }
}

fn config_home() -> Result<PathBuf, String> {
    match std::env::var_os("XDG_CONFIG_HOME") {
        Some(value) if !value.is_empty() => Ok(PathBuf::from(value)),
        _ => Ok(home_dir()?.join(".config")),
    }
}

pub fn applications_dir() -> Result<PathBuf, String> {
    Ok(data_home()?.join("applications"))
}

/// Where this app actually lives.
///
/// Inside an AppImage `current_exe` is a path in the throwaway mount, which
/// stops working the moment the image exits. `APPIMAGE` is the real file and
/// is the one an association has to name.
pub fn executable_path() -> Result<PathBuf, String> {
    if let Some(appimage) = std::env::var_os("APPIMAGE") {
        if !appimage.is_empty() {
            return Ok(PathBuf::from(appimage));
        }
    }
    std::env::current_exe().map_err(|error| format!("Failed to locate the executable: {error}"))
}

/// Quotes an `Exec=` program per the desktop entry spec, which needs it for
/// anything containing a space or one of the reserved characters.
pub fn quote_exec(path: &str) -> String {
    const RESERVED: [char; 12] = [
        ' ', '\t', '\n', '"', '\'', '\\', '>', '<', '~', '|', '&', ';',
    ];
    if !path.contains(RESERVED) {
        return path.to_string();
    }
    let escaped = path.replace('\\', r"\\").replace('"', r#"\""#);
    format!("\"{escaped}\"")
}

/// Writes an entry into the user's applications directory, refreshed on every
/// enable so a moved or rebuilt binary cannot leave an association pointing at
/// a path that no longer runs anything.
pub fn write_desktop_entry(file_name: &str, contents: &str) -> Result<(), String> {
    let dir = applications_dir()?;
    std::fs::create_dir_all(&dir)
        .map_err(|error| format!("Failed to create {}: {error}", dir.display()))?;

    let path = dir.join(file_name);
    std::fs::write(&path, contents)
        .map_err(|error| format!("Failed to write {}: {error}", path.display()))?;

    // Best effort: the association works without a refreshed cache, but menus
    // and "Open With" lists will not show the entry until this runs.
    let _ = Command::new("update-desktop-database").arg(&dir).status();
    Ok(())
}

/// Deletes an entry written by `write_desktop_entry`.
///
/// Removing the `mimeapps.list` lines alone does not undo anything: with no
/// explicit default, the lookup falls back to whichever installed entry
/// declares the type, and ours does — so it would be picked straight back up
/// and the toggle would refuse to turn off. The entry exists only to carry
/// the association, so it goes with it.
///
/// A packaged install's own entry lives elsewhere and is left alone; this only
/// touches the file in the user's applications directory.
pub fn remove_desktop_entry(file_name: &str) -> Result<(), String> {
    let dir = applications_dir()?;
    let path = dir.join(file_name);
    match std::fs::remove_file(&path) {
        Ok(()) => {}
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(error) => return Err(format!("Failed to remove {}: {error}", path.display())),
    }
    // The cache still lists the entry until this runs, and a stale cache is
    // enough for the association to resolve back to it.
    let _ = Command::new("update-desktop-database").arg(&dir).status();
    Ok(())
}

pub fn run_xdg_mime(args: &[&str]) -> Result<String, String> {
    let output = Command::new("xdg-mime")
        .args(args)
        .output()
        .map_err(|error| {
            format!("Failed to run xdg-mime: {error}. Install xdg-utils to set associations.")
        })?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        let detail = stderr.trim();
        return Err(if detail.is_empty() {
            "xdg-mime failed".to_string()
        } else {
            format!("xdg-mime failed: {detail}")
        });
    }

    Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
}

pub fn xdg_mime_available() -> bool {
    // Only the tool is checked. Whether an association can actually be written
    // depends on the session, and finding that out means trying.
    Command::new("xdg-mime")
        .arg("--version")
        .output()
        .map(|output| output.status.success())
        .unwrap_or(false)
}

/// Strips `entry_id` from the lines of the association sections that concern
/// `mimes`, returning `None` when nothing changed.
///
/// A key may list several handlers in preference order, so this removes one id
/// from the list rather than deleting the line outright, and drops the line
/// only when we were the last one on it.
fn without_entry(contents: &str, entry_id: &str, mimes: &[&str]) -> Option<String> {
    let mut section = String::new();
    let mut changed = false;
    let mut out: Vec<String> = Vec::new();

    for line in contents.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with('[') && trimmed.ends_with(']') {
            section = trimmed.to_string();
            out.push(line.to_string());
            continue;
        }

        let is_association = ASSOCIATION_SECTIONS.contains(&section.as_str())
            && mimes.iter().any(|mime| {
                trimmed.starts_with(mime) && trimmed[mime.len()..].trim_start().starts_with('=')
            });

        if !is_association {
            out.push(line.to_string());
            continue;
        }

        let (key, value) = match line.split_once('=') {
            Some(parts) => parts,
            None => {
                out.push(line.to_string());
                continue;
            }
        };

        let kept: Vec<&str> = value
            .split(';')
            .map(str::trim)
            .filter(|id| !id.is_empty() && *id != entry_id)
            .collect();

        if kept.len() == value.split(';').filter(|id| !id.trim().is_empty()).count() {
            out.push(line.to_string());
            continue;
        }

        changed = true;
        if !kept.is_empty() {
            out.push(format!("{key}={};", kept.join(";")));
        }
    }

    if !changed {
        return None;
    }

    let mut result = out.join("\n");
    if contents.ends_with('\n') {
        result.push('\n');
    }
    Some(result)
}

/// There is no `xdg-mime unset`, so lines are removed by hand. Only our own id
/// is touched; another handler listed alongside us keeps its place.
pub fn remove_association(entry_id: &str, mimes: &[&str]) -> Result<(), String> {
    let path = config_home()?.join("mimeapps.list");
    let contents = match std::fs::read_to_string(&path) {
        Ok(contents) => contents,
        // Nothing to remove is the desired end state, not a failure.
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(error) => return Err(format!("Failed to read {}: {error}", path.display())),
    };

    if let Some(updated) = without_entry(&contents, entry_id, mimes) {
        std::fs::write(&path, updated)
            .map_err(|error| format!("Failed to write {}: {error}", path.display()))?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    const ENTRY: &str = "sigma-file-manager.desktop";
    const DIRECTORY: [&str; 1] = ["inode/directory"];

    #[test]
    fn a_plain_path_is_left_alone() {
        assert_eq!(
            quote_exec("/usr/bin/sigma-file-manager"),
            "/usr/bin/sigma-file-manager"
        );
    }

    #[test]
    fn a_path_with_a_space_is_quoted() {
        assert_eq!(
            quote_exec("/opt/Sigma File Manager/sfm"),
            "\"/opt/Sigma File Manager/sfm\""
        );
    }

    #[test]
    fn removing_our_id_drops_the_line_when_we_were_alone_on_it() {
        let before = "[Default Applications]\ninode/directory=sigma-file-manager.desktop;\n";
        let after = without_entry(before, ENTRY, &DIRECTORY).expect("should have changed");
        assert_eq!(after, "[Default Applications]\n");
    }

    #[test]
    fn removing_our_id_keeps_the_other_handlers() {
        let before =
            "[Default Applications]\ninode/directory=sigma-file-manager.desktop;nemo.desktop;\n";
        let after = without_entry(before, ENTRY, &DIRECTORY).expect("should have changed");
        assert_eq!(
            after,
            "[Default Applications]\ninode/directory=nemo.desktop;\n"
        );
    }

    #[test]
    fn other_sections_and_other_types_are_untouched() {
        let before = "[Default Applications]\n\
                      text/plain=sigma-file-manager.desktop;\n\
                      inode/directory=nemo.desktop;\n\
                      \n\
                      [Removed Associations]\n\
                      inode/directory=sigma-file-manager.desktop;\n";
        assert!(
            without_entry(before, ENTRY, &DIRECTORY).is_none(),
            "nothing in an association section named us for directories"
        );
    }

    #[test]
    fn both_association_sections_are_cleaned() {
        let before = "[Default Applications]\n\
                      inode/directory=sigma-file-manager.desktop;\n\
                      [Added Associations]\n\
                      inode/directory=sigma-file-manager.desktop;nemo.desktop;\n";
        let after = without_entry(before, ENTRY, &DIRECTORY).expect("should have changed");
        assert_eq!(
            after,
            "[Default Applications]\n[Added Associations]\ninode/directory=nemo.desktop;\n"
        );
    }

    #[test]
    fn a_file_that_never_named_us_is_left_exactly_as_it_was() {
        let before = "[Default Applications]\ninode/directory=nemo.desktop;\n";
        assert!(without_entry(before, ENTRY, &DIRECTORY).is_none());
    }

    /// One disable sweeps every type the role registered, and only those.
    #[test]
    fn a_multi_type_role_is_cleaned_across_all_its_lines() {
        let viewer = "sigma-quick-view.desktop";
        let mimes = ["video/mp4", "image/png"];
        let before = "[Default Applications]\n\
                      video/mp4=sigma-quick-view.desktop;\n\
                      image/png=sigma-quick-view.desktop;loupe.desktop;\n\
                      audio/mpeg=mpv.desktop;\n";
        let after = without_entry(before, viewer, &mimes).expect("should have changed");
        assert_eq!(
            after,
            "[Default Applications]\nimage/png=loupe.desktop;\naudio/mpeg=mpv.desktop;\n"
        );
    }
}
