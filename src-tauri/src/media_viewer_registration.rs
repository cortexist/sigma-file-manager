// SPDX-License-Identifier: GPL-3.0-or-later
// License: GNU GPLv3 or later. See the license file in the project root for more information.
// Copyright © 2026 Cortexist, LLC. All rights reserved.

//! Registers Quick View as the system's image, video, and audio viewer.
//!
//! The role gets its own desktop entry — "Sigma Quick View", taking a single
//! file — rather than piggybacking on the file manager's: a viewer should read
//! as a viewer in other applications' Open With menus, and the two roles must
//! be independently revocable. Opening a file through the entry lands in the
//! standalone viewer (see `standalone_viewer`), never in a file-manager
//! session.
//!
//! Only Linux is wired: the commands exist everywhere so the settings page can
//! ask, and answer "unavailable" elsewhere.

#[cfg(target_os = "linux")]
use crate::xdg_associations;

/// Separate from the file manager's entry so each role stands alone.
#[cfg(target_os = "linux")]
const DESKTOP_FILE_NAME: &str = "sigma-quick-view.desktop";

/// The types Quick View is offered for, mirroring `MEDIA_EXTENSIONS` in
/// `standalone_viewer.rs` — registering for a type the viewer would bounce
/// helps nobody. Keep the two in sync.
#[cfg(target_os = "linux")]
const MEDIA_MIME_TYPES: [&str; 26] = [
    // Images
    "image/jpeg",
    "image/png",
    "image/gif",
    "image/webp",
    "image/svg+xml",
    "image/bmp",
    "image/vnd.microsoft.icon",
    "image/tiff",
    "image/avif",
    // Video
    "video/mp4",
    "video/webm",
    "video/ogg",
    "video/quicktime",
    "video/x-msvideo",
    "video/x-matroska",
    "video/x-m4v",
    "video/x-ms-wmv",
    "video/x-flv",
    // Audio
    "audio/mpeg",
    "audio/x-wav",
    "audio/ogg",
    "audio/flac",
    "audio/aac",
    "audio/mp4",
    "audio/x-ms-wma",
    "audio/x-opus+ogg",
];

/// `%f` rather than `%U`: the standalone viewer takes one local file, and
/// declaring only what is honored keeps launchers from batching several files
/// into a call that would view just the first. `NoDisplay` keeps the entry out
/// of app menus — it exists to be *offered for files*, not launched bare.
#[cfg(target_os = "linux")]
fn desktop_entry_contents(executable: &str) -> String {
    format!(
        "[Desktop Entry]\n\
         Type=Application\n\
         Name=Sigma Quick View\n\
         GenericName=Media Viewer\n\
         Comment=View images, videos, and audio\n\
         Exec={exec} %f\n\
         Icon=sigma-file-manager\n\
         Terminal=false\n\
         NoDisplay=true\n\
         Categories=AudioVideo;Viewer;Graphics;\n\
         MimeType={mimes};\n\
         StartupNotify=true\n\
         StartupWMClass=sigma-quick-view\n",
        exec = xdg_associations::quote_exec(executable),
        mimes = MEDIA_MIME_TYPES.join(";"),
    )
}

/// Default means default for *everything* the viewer registers: a toggle that
/// reads on while some types still open elsewhere would be lying about the
/// thing it exists to say.
#[cfg(target_os = "linux")]
fn is_default() -> Result<bool, String> {
    for mime in MEDIA_MIME_TYPES {
        if xdg_associations::run_xdg_mime(&["query", "default", mime])? != DESKTOP_FILE_NAME {
            return Ok(false);
        }
    }

    Ok(true)
}

#[cfg(target_os = "linux")]
fn set_default(enabled: bool) -> Result<bool, String> {
    if enabled {
        let executable = xdg_associations::executable_path()?;
        xdg_associations::write_desktop_entry(
            DESKTOP_FILE_NAME,
            &desktop_entry_contents(&executable.to_string_lossy()),
        )?;

        for mime in MEDIA_MIME_TYPES {
            xdg_associations::run_xdg_mime(&["default", DESKTOP_FILE_NAME, mime])?;
        }
    } else {
        // The previous defaults are not restored — `xdg-mime default` replaced
        // those lines and kept no copy. With ours gone the lookup falls back to
        // the installed-handlers cache, which nearly always lands on the same
        // application the user had before.
        xdg_associations::remove_association(DESKTOP_FILE_NAME, &MEDIA_MIME_TYPES)?;
        xdg_associations::remove_desktop_entry(DESKTOP_FILE_NAME)?;
    }

    let now_default = is_default()?;
    if enabled && !now_default {
        return Err(
            "The desktop environment did not accept the change. Another application may be \
             enforcing its own associations."
                .to_string(),
        );
    }
    Ok(now_default)
}

#[tauri::command]
pub fn media_viewer_registration_available() -> bool {
    #[cfg(target_os = "linux")]
    {
        xdg_associations::xdg_mime_available()
    }

    #[cfg(not(target_os = "linux"))]
    {
        false
    }
}

#[tauri::command]
pub fn is_default_media_viewer() -> Result<bool, String> {
    #[cfg(target_os = "linux")]
    {
        is_default()
    }

    #[cfg(not(target_os = "linux"))]
    {
        Ok(false)
    }
}

#[tauri::command]
pub fn set_default_media_viewer(enabled: bool) -> Result<bool, String> {
    #[cfg(target_os = "linux")]
    {
        set_default(enabled)
    }

    #[cfg(not(target_os = "linux"))]
    {
        let _ = enabled;
        Ok(false)
    }
}

#[cfg(all(test, target_os = "linux"))]
mod tests {
    use super::*;

    #[test]
    fn the_entry_reads_as_a_viewer_taking_one_file() {
        let entry = desktop_entry_contents("/home/z/.local/bin/sigma-file-manager");

        assert!(entry.contains("Name=Sigma Quick View\n"));
        assert!(entry.contains("Exec=/home/z/.local/bin/sigma-file-manager %f\n"));
        // Offered for files, not shown in app menus.
        assert!(entry.contains("NoDisplay=true\n"));
        // Every registered type is declared, none twice.
        assert!(entry.contains("MimeType=image/jpeg;"));
        assert!(entry.contains("video/x-matroska;"));
        assert!(entry.contains("audio/x-opus+ogg;\n"));
    }

    #[test]
    fn the_type_list_carries_no_duplicates() {
        let mut seen = std::collections::HashSet::new();

        for mime in MEDIA_MIME_TYPES {
            assert!(seen.insert(mime), "{mime} is listed twice");
        }
    }
}
