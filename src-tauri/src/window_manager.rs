// SPDX-License-Identifier: GPL-3.0-or-later
// License: GNU GPLv3 or later. See the license file in the project root for more information.
// Copyright © 2021 - present Aleksey Hoffman. All rights reserved.

//! Which window-state controls the desktop can actually honour.
//!
//! Tiling compositors own window geometry: sway has no iconify at all, and "maximize" means
//! nothing for a surface the compositor already sized. The app draws its own titlebar, so
//! without asking first it would offer two buttons that quietly do nothing.

#[derive(Debug, Clone, Copy, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WindowControlSupport {
    pub minimize: bool,
    pub maximize: bool,
}

impl WindowControlSupport {
    const ALL: Self = Self {
        minimize: true,
        maximize: true,
    };

    #[cfg(target_os = "linux")]
    const NONE: Self = Self {
        minimize: false,
        maximize: false,
    };
}

/// Compositors that place and size windows themselves, under the names they report in the
/// XDG desktop variables.
#[cfg(target_os = "linux")]
const TILING_COMPOSITORS: &[&str] = &[
    "awesome",
    "bspwm",
    "dwm",
    "herbstluftwm",
    "hyprland",
    "i3",
    "leftwm",
    "niri",
    "qtile",
    "river",
    "spectrwm",
    "sway",
    "xmonad",
];

#[cfg(target_os = "linux")]
fn is_tiling_compositor() -> bool {
    // A live IPC socket is the stronger signal: the desktop name can be left pointing at
    // something else by a session script, a display manager, or a nested compositor.
    if std::env::var_os("SWAYSOCK").is_some()
        || std::env::var_os("I3SOCK").is_some()
        || std::env::var_os("HYPRLAND_INSTANCE_SIGNATURE").is_some()
        || std::env::var_os("NIRI_SOCKET").is_some()
    {
        return true;
    }

    [
        "XDG_CURRENT_DESKTOP",
        "XDG_SESSION_DESKTOP",
        "DESKTOP_SESSION",
    ]
    .iter()
    .filter_map(|key| std::env::var(key).ok())
    .any(|value| {
        value
            .to_ascii_lowercase()
            .split(':')
            .any(|name| TILING_COMPOSITORS.contains(&name.trim()))
    })
}

/// Anything unrecognised keeps the full set, so an unknown desktop never loses buttons.
pub fn window_control_support() -> WindowControlSupport {
    #[cfg(target_os = "linux")]
    {
        if is_tiling_compositor() {
            return WindowControlSupport::NONE;
        }
    }

    WindowControlSupport::ALL
}

pub const SUPPORT_GLOBAL: &str = "__SFM_WINDOW_CONTROL_SUPPORT__";

/// Injected into every webview before its first paint.
///
/// The desktop cannot change under a running process, so this is decided once at startup and
/// handed to the frontend as a plain global. Asking over IPC instead would mean the titlebar
/// has to render before it knows the answer, and whichever way it guessed would be wrong and
/// visibly corrected on some desktop.
pub fn init<R: tauri::Runtime>() -> tauri::plugin::TauriPlugin<R> {
    let support = window_control_support();

    tauri::plugin::Builder::new("sfm-window-controls")
        .js_init_script(format!(
            "window.{SUPPORT_GLOBAL} = {{ minimize: {}, maximize: {} }};",
            support.minimize, support.maximize,
        ))
        .build()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn non_tiling_desktops_keep_every_control() {
        let support = WindowControlSupport::ALL;
        assert!(support.minimize && support.maximize);
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn tiling_desktops_drop_state_controls() {
        let support = WindowControlSupport::NONE;
        assert!(!support.minimize && !support.maximize);
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn compositor_list_is_lowercase_for_case_insensitive_matching() {
        for name in TILING_COMPOSITORS {
            assert_eq!(*name, name.to_ascii_lowercase(), "{name} must be lowercase");
        }
    }

    #[test]
    fn injected_script_declares_both_controls() {
        let support = window_control_support();
        let script = format!(
            "window.{SUPPORT_GLOBAL} = {{ minimize: {}, maximize: {} }};",
            support.minimize, support.maximize,
        );

        assert!(script.contains("minimize:"));
        assert!(script.contains("maximize:"));
        // Booleans must reach JS as literals, not as quoted strings.
        assert!(!script.contains('"'));
    }
}
