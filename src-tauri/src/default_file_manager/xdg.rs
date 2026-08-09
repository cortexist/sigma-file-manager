// SPDX-License-Identifier: GPL-3.0-or-later
// License: GNU GPLv3 or later. See the license file in the project root for more information.
// Copyright © 2026 Cortexist, LLC. All rights reserved.

//! Linux: being the default file manager means owning the `inode/directory`
//! MIME association.
//!
//! Unlike Windows there is no registry to snapshot and restore. The whole of
//! the state is two things:
//!
//!   1. A desktop entry that names this executable and declares it handles
//!      directories. Associations are made against desktop *entries*, never
//!      against executables, so a build that is simply copied to `~/.local/bin`
//!      has nothing to associate until one exists.
//!   2. A line in `mimeapps.list` pointing `inode/directory` at that entry.
//!
//! `xdg-mime` owns the second: it knows the per-desktop quirks (KDE and GNOME
//! keep their own copies) that hand-editing one file would get wrong. The
//! first is ours to write, and is refreshed on every enable so that moving or
//! rebuilding the binary cannot leave the association pointing at a path that
//! no longer runs anything.
//!
//! Disabling removes our line rather than naming a replacement: which file
//! manager *should* inherit the association is not ours to decide, and with
//! the line gone the system falls back to whatever it would have used anyway.

use std::path::{Path, PathBuf};
use std::process::Command;

/// The entry we write when nothing else on the system already points here.
const DESKTOP_FILE_NAME: &str = "sigma-file-manager.desktop";
const DIRECTORY_MIME: &str = "inode/directory";
/// Sections of `mimeapps.list` that can name us; both are cleaned on disable.
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

fn applications_dir() -> Result<PathBuf, String> {
    Ok(data_home()?.join("applications"))
}

/// Where this app actually lives.
///
/// Inside an AppImage `current_exe` is a path in the throwaway mount, which
/// stops working the moment the image exits. `APPIMAGE` is the real file and
/// is the one an association has to name.
fn executable_path() -> Result<PathBuf, String> {
    if let Some(appimage) = std::env::var_os("APPIMAGE") {
        if !appimage.is_empty() {
            return Ok(PathBuf::from(appimage));
        }
    }
    std::env::current_exe().map_err(|error| format!("Failed to locate the executable: {error}"))
}

/// Quotes an `Exec=` program per the desktop entry spec, which needs it for
/// anything containing a space or one of the reserved characters.
fn quote_exec(path: &str) -> String {
    const RESERVED: [char; 12] = [
        ' ', '\t', '\n', '"', '\'', '\\', '>', '<', '~', '|', '&', ';',
    ];
    if !path.contains(RESERVED) {
        return path.to_string();
    }
    let escaped = path.replace('\\', r"\\").replace('"', r#"\""#);
    format!("\"{escaped}\"")
}

fn desktop_entry_contents(executable: &str) -> String {
    format!(
        "[Desktop Entry]\n\
         Type=Application\n\
         Name=Sigma File Manager\n\
         GenericName=File Manager\n\
         Comment=Browse and manage files\n\
         Exec={exec} %U\n\
         Icon=sigma-file-manager\n\
         Terminal=false\n\
         Categories=System;FileTools;FileManager;\n\
         MimeType={mime};\n\
         StartupNotify=true\n\
         StartupWMClass=sigma-file-manager\n",
        exec = quote_exec(executable),
        mime = DIRECTORY_MIME,
    )
}

fn write_desktop_entry() -> Result<(), String> {
    let executable = executable_path()?;
    let dir = applications_dir()?;
    std::fs::create_dir_all(&dir)
        .map_err(|error| format!("Failed to create {}: {error}", dir.display()))?;

    let path = dir.join(DESKTOP_FILE_NAME);
    let contents = desktop_entry_contents(&executable.to_string_lossy());
    std::fs::write(&path, contents)
        .map_err(|error| format!("Failed to write {}: {error}", path.display()))?;

    // Best effort: the association works without a refreshed cache, but menus
    // and "Open With" lists will not show the entry until this runs.
    let _ = Command::new("update-desktop-database").arg(&dir).status();
    Ok(())
}

fn run_xdg_mime(args: &[&str]) -> Result<String, String> {
    let output = Command::new("xdg-mime")
        .args(args)
        .output()
        .map_err(|error| {
            format!(
            "Failed to run xdg-mime: {error}. Install xdg-utils to set the default file manager."
        )
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

/// The desktop entry id currently associated with directories.
fn current_default_entry() -> Result<String, String> {
    run_xdg_mime(&["query", "default", DIRECTORY_MIME])
}

pub fn available() -> bool {
    // Only the tool is checked. Whether the association can actually be
    // written depends on the session, and finding that out means trying.
    Command::new("xdg-mime")
        .arg("--version")
        .output()
        .map(|output| output.status.success())
        .unwrap_or(false)
}

pub fn is_default() -> Result<bool, String> {
    Ok(current_default_entry()? == DESKTOP_FILE_NAME)
}

/// Strips `entry_id` from the association sections of a `mimeapps.list`,
/// returning `None` when nothing changed.
///
/// A key may list several handlers in preference order, so this removes one id
/// from the list rather than deleting the line outright, and drops the line
/// only when we were the last one on it.
fn without_entry(contents: &str, entry_id: &str) -> Option<String> {
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
            && trimmed.starts_with(DIRECTORY_MIME)
            && trimmed[DIRECTORY_MIME.len()..]
                .trim_start()
                .starts_with('=');

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

/// Deletes the entry written by `write_desktop_entry`.
///
/// Removing the `mimeapps.list` line alone does not undo anything: with no
/// explicit default, the lookup falls back to whichever installed entry
/// declares `inode/directory`, and ours does — so it would be picked straight
/// back up and the toggle would refuse to turn off. The entry exists only to
/// carry the association, so it goes with it.
///
/// A packaged install's own entry lives elsewhere and is left alone; this only
/// touches the file in the user's applications directory.
fn remove_desktop_entry() -> Result<(), String> {
    let dir = applications_dir()?;
    let path = dir.join(DESKTOP_FILE_NAME);
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

/// There is no `xdg-mime unset`, so the line is removed by hand. Only our own
/// id is touched; another handler listed alongside us keeps its place.
fn remove_association() -> Result<(), String> {
    let path = config_home()?.join("mimeapps.list");
    let contents = match std::fs::read_to_string(&path) {
        Ok(contents) => contents,
        // Nothing to remove is the desired end state, not a failure.
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(error) => return Err(format!("Failed to read {}: {error}", path.display())),
    };

    if let Some(updated) = without_entry(&contents, DESKTOP_FILE_NAME) {
        std::fs::write(&path, updated)
            .map_err(|error| format!("Failed to write {}: {error}", path.display()))?;
    }
    Ok(())
}

/// Applies the requested state and reports what the system says afterwards,
/// so a silent refusal surfaces as the toggle springing back rather than as a
/// setting that claims to be on.
pub fn set_default(enabled: bool) -> Result<bool, String> {
    if enabled {
        write_desktop_entry()?;
        run_xdg_mime(&["default", DESKTOP_FILE_NAME, DIRECTORY_MIME])?;
    } else {
        remove_association()?;
        remove_desktop_entry()?;
    }

    let now_default = is_default()?;
    if enabled && !now_default {
        return Err(
            "The desktop environment did not accept the change. Its own file manager setting may \
             be overriding it."
                .to_string(),
        );
    }
    Ok(now_default)
}

/// Exposed for the migration path: the entry we manage, if it is there.
#[allow(dead_code)]
pub fn desktop_entry_path() -> Result<PathBuf, String> {
    Ok(applications_dir()?.join(DESKTOP_FILE_NAME))
}

#[allow(dead_code)]
fn is_our_entry(path: &Path) -> bool {
    path.file_name()
        .map(|name| name == DESKTOP_FILE_NAME)
        .unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::*;

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
    fn the_entry_declares_directories_and_names_the_executable() {
        let entry = desktop_entry_contents("/home/z/.local/bin/sigma-file-manager");
        assert!(entry.contains("Exec=/home/z/.local/bin/sigma-file-manager %U"));
        assert!(entry.contains("MimeType=inode/directory;"));
        // Without this the compositor cannot match our window to the entry.
        assert!(entry.contains("StartupWMClass=sigma-file-manager"));
    }

    #[test]
    fn removing_our_id_drops_the_line_when_we_were_alone_on_it() {
        let before = "[Default Applications]\ninode/directory=sigma-file-manager.desktop;\n";
        let after = without_entry(before, DESKTOP_FILE_NAME).expect("should have changed");
        assert_eq!(after, "[Default Applications]\n");
    }

    #[test]
    fn removing_our_id_keeps_the_other_handlers() {
        let before =
            "[Default Applications]\ninode/directory=sigma-file-manager.desktop;nemo.desktop;\n";
        let after = without_entry(before, DESKTOP_FILE_NAME).expect("should have changed");
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
            without_entry(before, DESKTOP_FILE_NAME).is_none(),
            "nothing in an association section named us for directories"
        );
    }

    #[test]
    fn both_association_sections_are_cleaned() {
        let before = "[Default Applications]\n\
                      inode/directory=sigma-file-manager.desktop;\n\
                      [Added Associations]\n\
                      inode/directory=sigma-file-manager.desktop;nemo.desktop;\n";
        let after = without_entry(before, DESKTOP_FILE_NAME).expect("should have changed");
        assert_eq!(
            after,
            "[Default Applications]\n[Added Associations]\ninode/directory=nemo.desktop;\n"
        );
    }

    #[test]
    fn a_file_that_never_named_us_is_left_exactly_as_it_was() {
        let before = "[Default Applications]\ninode/directory=nemo.desktop;\n";
        assert!(without_entry(before, DESKTOP_FILE_NAME).is_none());
    }
}
