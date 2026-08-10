// SPDX-License-Identifier: GPL-3.0-or-later
// License: GNU GPLv3 or later. See the license file in the project root for more information.
// Copyright © 2026 Cortexist, LLC. All rights reserved.

//! Linux: being the default file manager means owning the `inode/directory`
//! MIME association. The mechanics live in `crate::xdg_associations`; this
//! module only knows what the file-manager role's entry says.
//!
//! Disabling removes our line rather than naming a replacement: which file
//! manager *should* inherit the association is not ours to decide, and with
//! the line gone the system falls back to whatever it would have used anyway.

use std::path::PathBuf;

use crate::xdg_associations;

/// The entry we write when nothing else on the system already points here.
const DESKTOP_FILE_NAME: &str = "sigma-file-manager.desktop";
const DIRECTORY_MIME: &str = "inode/directory";

/// "Show in Folder" never consults MIME defaults: browsers call the `FileManager1` DBus name
/// and whoever activation resolves it to wins. A user-level service file outranks the system
/// file manager's, which is what actually redirects those clicks here.
const FILE_MANAGER1_SERVICE: &str = "dbus-1/services/org.freedesktop.FileManager1.service";

fn file_manager1_service_path() -> Result<PathBuf, String> {
    Ok(xdg_associations::data_home()?.join(FILE_MANAGER1_SERVICE))
}

fn file_manager1_service_contents(executable: &str) -> String {
    format!(
        "[D-BUS Service]\n\
         Name=org.freedesktop.FileManager1\n\
         Exec={executable} --sigma-autostart\n"
    )
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
        exec = xdg_associations::quote_exec(executable),
        mime = DIRECTORY_MIME,
    )
}

/// The desktop entry id currently associated with directories.
fn current_default_entry() -> Result<String, String> {
    xdg_associations::run_xdg_mime(&["query", "default", DIRECTORY_MIME])
}

pub fn available() -> bool {
    xdg_associations::xdg_mime_available()
}

pub fn is_default() -> Result<bool, String> {
    Ok(current_default_entry()? == DESKTOP_FILE_NAME)
}

/// Applies the requested state and reports what the system says afterwards,
/// so a silent refusal surfaces as the toggle springing back rather than as a
/// setting that claims to be on.
pub fn set_default(enabled: bool) -> Result<bool, String> {
    if enabled {
        let executable = xdg_associations::executable_path()?;
        xdg_associations::write_desktop_entry(
            DESKTOP_FILE_NAME,
            &desktop_entry_contents(&executable.to_string_lossy()),
        )?;
        xdg_associations::run_xdg_mime(&["default", DESKTOP_FILE_NAME, DIRECTORY_MIME])?;

        let service_path = file_manager1_service_path()?;
        if let Some(parent) = service_path.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|error| format!("Failed to create {}: {error}", parent.display()))?;
        }
        std::fs::write(
            &service_path,
            file_manager1_service_contents(&executable.to_string_lossy()),
        )
        .map_err(|error| format!("Failed to write {}: {error}", service_path.display()))?;
    } else {
        xdg_associations::remove_association(DESKTOP_FILE_NAME, &[DIRECTORY_MIME])?;
        xdg_associations::remove_desktop_entry(DESKTOP_FILE_NAME)?;

        let service_path = file_manager1_service_path()?;
        match std::fs::remove_file(&service_path) {
            Ok(()) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => {
                return Err(format!(
                    "Failed to remove {}: {error}",
                    service_path.display()
                ))
            }
        }
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
    Ok(xdg_associations::applications_dir()?.join(DESKTOP_FILE_NAME))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_entry_declares_directories_and_names_the_executable() {
        let entry = desktop_entry_contents("/home/z/.local/bin/sigma-file-manager");
        assert!(entry.contains("Exec=/home/z/.local/bin/sigma-file-manager %U"));
        assert!(entry.contains("MimeType=inode/directory;"));
        // Without this the compositor cannot match our window to the entry.
        assert!(entry.contains("StartupWMClass=sigma-file-manager"));
    }
}
