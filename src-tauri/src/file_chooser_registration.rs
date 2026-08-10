// SPDX-License-Identifier: GPL-3.0-or-later
// License: GNU GPLv3 or later. See the license file in the project root for more information.
// Copyright © 2026 Cortexist, LLC. All rights reserved.

//! Registers sigma's FileChooser backend with the desktop portal, so applications that ask
//! the portal for a file dialog get sigma's.
//!
//! Three artifacts, all user-level:
//!
//!   1. A `.portal` file declaring the backend and its bus name, which is how the portal
//!      daemon learns the backend exists. User-level files are read (verified against 1.22
//!      with `--verbose`: the user copy loads first and a system duplicate is skipped).
//!   2. A preference line in `portals.conf` naming sigma for the FileChooser interface. The
//!      file is *edited*, never clobbered: other interfaces — the screencast backend on this
//!      compositor, say — must keep whatever routing they had. When the file has to be
//!      created from scratch, `default=*` is written alongside, preserving the "any installed
//!      backend" behavior the absence of the file meant.
//!   3. A DBus activation service file, so a dialog request can start sigma when it is not
//!      already running. The exec carries `--sigma-autostart`, the existing start-quietly
//!      flag; whether the main window stays hidden then follows the user's own startup
//!      setting, which is the closest honest behavior without inventing a new launch mode.
//!
//! The portal daemon reads all of this at startup, so applying either direction ends with a
//! best-effort `systemctl --user try-restart xdg-desktop-portal`.

#[cfg(target_os = "linux")]
mod imp {
    use std::path::PathBuf;
    use std::process::Command;

    use crate::xdg_associations;

    const PORTAL_NAME: &str = "sigma";
    const BUS_NAME: &str = "org.freedesktop.impl.portal.desktop.sigma";
    const INTERFACE: &str = "org.freedesktop.impl.portal.FileChooser";
    const PREFERRED_SECTION: &str = "[preferred]";

    fn portal_file_path() -> Result<PathBuf, String> {
        Ok(xdg_associations::data_home()?.join("xdg-desktop-portal/portals/sigma.portal"))
    }

    /// The file the daemon will actually consult. A desktop-specific configuration —
    /// `sway-portals.conf`, say — takes precedence over the generic `portals.conf`, so a
    /// preference written to the generic file while a desktop-specific one exists is silently
    /// ignored; the edit has to land in whichever file wins.
    fn portals_conf_path() -> Result<PathBuf, String> {
        let dir = xdg_associations::config_home()?.join("xdg-desktop-portal");

        if let Some(desktops) = std::env::var_os("XDG_CURRENT_DESKTOP") {
            for desktop in desktops
                .to_string_lossy()
                .split(':')
                .filter(|desktop| !desktop.is_empty())
            {
                let candidate = dir.join(format!("{}-portals.conf", desktop.to_lowercase()));
                if candidate.exists() {
                    return Ok(candidate);
                }
            }
        }

        Ok(dir.join("portals.conf"))
    }

    fn dbus_service_path() -> Result<PathBuf, String> {
        Ok(xdg_associations::data_home()?.join(format!("dbus-1/services/{BUS_NAME}.service")))
    }

    fn portal_file_contents() -> String {
        // `UseIn` is the pre-1.17 daemon's matching mechanism; harmless beside portals.conf.
        format!(
            "[portal]\n\
             DBusName={BUS_NAME}\n\
             Interfaces={INTERFACE}\n\
             UseIn=sway\n"
        )
    }

    fn dbus_service_contents(executable: &str) -> String {
        format!(
            "[D-BUS Service]\n\
             Name={BUS_NAME}\n\
             Exec={executable} --sigma-autostart\n"
        )
    }

    /// Sets `org.freedesktop.impl.portal.FileChooser=sigma` in the `[preferred]` section,
    /// creating the section — and, for a brand-new file, the `default=*` line — as needed.
    /// Everything else in the file is carried through untouched.
    fn with_preference(contents: Option<&str>) -> String {
        let Some(contents) = contents else {
            return format!("{PREFERRED_SECTION}\ndefault=*\n{INTERFACE}={PORTAL_NAME}\n");
        };

        let mut out: Vec<String> = Vec::new();
        let mut in_preferred = false;
        let mut wrote_preference = false;

        for line in contents.lines() {
            let trimmed = line.trim();

            if trimmed.starts_with('[') {
                // Leaving the preferred section without having found the key: add it.
                if in_preferred && !wrote_preference {
                    out.push(format!("{INTERFACE}={PORTAL_NAME}"));
                    wrote_preference = true;
                }
                in_preferred = trimmed == PREFERRED_SECTION;
                out.push(line.to_string());
                continue;
            }

            if in_preferred && trimmed.starts_with(INTERFACE) {
                let is_key = trimmed[INTERFACE.len()..].trim_start().starts_with('=');
                if is_key {
                    out.push(format!("{INTERFACE}={PORTAL_NAME}"));
                    wrote_preference = true;
                    continue;
                }
            }

            out.push(line.to_string());
        }

        if in_preferred && !wrote_preference {
            out.push(format!("{INTERFACE}={PORTAL_NAME}"));
            wrote_preference = true;
        }

        if !wrote_preference {
            out.push(PREFERRED_SECTION.to_string());
            out.push(format!("{INTERFACE}={PORTAL_NAME}"));
        }

        let mut result = out.join("\n");
        result.push('\n');
        result
    }

    /// Removes the preference line only when it still names sigma; a line the user has since
    /// pointed elsewhere is theirs. Returns `None` when nothing changed.
    fn without_preference(contents: &str) -> Option<String> {
        let mut out: Vec<String> = Vec::new();
        let mut in_preferred = false;
        let mut changed = false;

        for line in contents.lines() {
            let trimmed = line.trim();

            if trimmed.starts_with('[') {
                in_preferred = trimmed == PREFERRED_SECTION;
                out.push(line.to_string());
                continue;
            }

            if in_preferred {
                if let Some(value) = trimmed
                    .strip_prefix(INTERFACE)
                    .and_then(|rest| rest.trim_start().strip_prefix('='))
                {
                    if value.trim().trim_end_matches(';') == PORTAL_NAME {
                        changed = true;
                        continue;
                    }
                }
            }

            out.push(line.to_string());
        }

        if !changed {
            return None;
        }

        let mut result = out.join("\n");
        result.push('\n');
        Some(result)
    }

    fn preference_names_sigma(contents: &str) -> bool {
        let mut in_preferred = false;

        for line in contents.lines() {
            let trimmed = line.trim();

            if trimmed.starts_with('[') {
                in_preferred = trimmed == PREFERRED_SECTION;
                continue;
            }

            if in_preferred {
                if let Some(value) = trimmed
                    .strip_prefix(INTERFACE)
                    .and_then(|rest| rest.trim_start().strip_prefix('='))
                {
                    // The first name in the list is the one the daemon tries first.
                    return value.split(';').next().map(str::trim) == Some(PORTAL_NAME);
                }
            }
        }

        false
    }

    /// The daemon reads its configuration once at startup. `try-restart` touches nothing if
    /// it is not running, and portals reconnect transparently when it is.
    fn restart_portal_daemon() {
        let _ = Command::new("systemctl")
            .args(["--user", "try-restart", "xdg-desktop-portal.service"])
            .status();
    }

    fn write_file(path: &PathBuf, contents: &str) -> Result<(), String> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|error| format!("Failed to create {}: {error}", parent.display()))?;
        }
        std::fs::write(path, contents)
            .map_err(|error| format!("Failed to write {}: {error}", path.display()))
    }

    fn remove_file(path: &PathBuf) -> Result<(), String> {
        match std::fs::remove_file(path) {
            Ok(()) => Ok(()),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(error) => Err(format!("Failed to remove {}: {error}", path.display())),
        }
    }

    pub fn is_default() -> Result<bool, String> {
        let path = portals_conf_path()?;

        match std::fs::read_to_string(&path) {
            Ok(contents) => Ok(preference_names_sigma(&contents)),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(false),
            Err(error) => Err(format!("Failed to read {}: {error}", path.display())),
        }
    }

    pub fn set_default(enabled: bool) -> Result<bool, String> {
        let conf_path = portals_conf_path()?;

        if enabled {
            let executable = xdg_associations::executable_path()?;
            write_file(&portal_file_path()?, &portal_file_contents())?;
            write_file(
                &dbus_service_path()?,
                &dbus_service_contents(&executable.to_string_lossy()),
            )?;

            let existing = match std::fs::read_to_string(&conf_path) {
                Ok(contents) => Some(contents),
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
                Err(error) => {
                    return Err(format!("Failed to read {}: {error}", conf_path.display()))
                }
            };
            write_file(&conf_path, &with_preference(existing.as_deref()))?;
        } else {
            if let Ok(contents) = std::fs::read_to_string(&conf_path) {
                if let Some(updated) = without_preference(&contents) {
                    write_file(&conf_path, &updated)?;
                }
            }
            remove_file(&portal_file_path()?)?;
            remove_file(&dbus_service_path()?)?;
        }

        restart_portal_daemon();
        is_default()
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        #[test]
        fn a_missing_conf_is_created_with_the_wildcard_default() {
            assert_eq!(
                with_preference(None),
                "[preferred]\ndefault=*\norg.freedesktop.impl.portal.FileChooser=sigma\n"
            );
        }

        /// The other interfaces' routing — the screencast backend, say — must survive.
        #[test]
        fn an_existing_conf_keeps_everything_it_already_said() {
            let existing = "[preferred]\n\
                            default=gtk\n\
                            org.freedesktop.impl.portal.ScreenCast=wlr\n";
            let updated = with_preference(Some(existing));

            assert!(updated.contains("default=gtk"));
            assert!(updated.contains("org.freedesktop.impl.portal.ScreenCast=wlr"));
            assert!(updated.contains("org.freedesktop.impl.portal.FileChooser=sigma"));
            // No wildcard invented for a file the user already shaped.
            assert!(!updated.contains("default=*"));
        }

        #[test]
        fn an_existing_preference_is_replaced_not_duplicated() {
            let existing = "[preferred]\norg.freedesktop.impl.portal.FileChooser=gtk\n";
            let updated = with_preference(Some(existing));

            assert_eq!(
                updated
                    .matches("org.freedesktop.impl.portal.FileChooser")
                    .count(),
                1
            );
            assert!(preference_names_sigma(&updated));
        }

        #[test]
        fn disabling_removes_only_a_line_that_still_names_sigma() {
            let ours = "[preferred]\ndefault=*\norg.freedesktop.impl.portal.FileChooser=sigma\n";
            let theirs = "[preferred]\norg.freedesktop.impl.portal.FileChooser=gtk\n";

            assert_eq!(
                without_preference(ours),
                Some("[preferred]\ndefault=*\n".to_string())
            );
            assert_eq!(without_preference(theirs), None);
        }

        #[test]
        fn the_first_name_in_a_list_is_what_counts() {
            assert!(preference_names_sigma(
                "[preferred]\norg.freedesktop.impl.portal.FileChooser=sigma;gtk\n"
            ));
            assert!(!preference_names_sigma(
                "[preferred]\norg.freedesktop.impl.portal.FileChooser=gtk;sigma\n"
            ));
        }
    }
}

#[tauri::command]
pub fn file_chooser_registration_available() -> bool {
    cfg!(target_os = "linux")
}

#[tauri::command]
pub fn is_default_file_chooser() -> Result<bool, String> {
    #[cfg(target_os = "linux")]
    {
        imp::is_default()
    }

    #[cfg(not(target_os = "linux"))]
    {
        Ok(false)
    }
}

#[tauri::command]
pub fn set_default_file_chooser(enabled: bool) -> Result<bool, String> {
    #[cfg(target_os = "linux")]
    {
        imp::set_default(enabled)
    }

    #[cfg(not(target_os = "linux"))]
    {
        let _ = enabled;
        Ok(false)
    }
}
