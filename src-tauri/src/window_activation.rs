// SPDX-License-Identifier: GPL-3.0-or-later
// License: GNU GPLv3 or later. See the license file in the project root for more information.
// Copyright © 2026 Cortexist, LLC. All rights reserved.

//! Raising a window for a launch that happened somewhere else.
//!
//! On Wayland an application cannot simply take focus: it presents an *activation token*, and
//! the compositor grants the raise only because that token stands for a click the user just
//! made. A request with no token is a window jumping in front of somebody's work uninvited,
//! and compositors refuse it — sway flags the workspace urgent and leaves focus where it was.
//!
//! Every launcher-driven raise runs into the same gap. The click starts the binary, the
//! compositor issues *that launch* a token in `XDG_ACTIVATION_TOKEN`, and then the new process
//! discovers an instance is already running, hands over its arguments, and exits — taking the
//! token with it. Single-instance forwarding carries argv and a working directory, nothing
//! else, so the running instance is left presenting itself with empty hands.
//!
//! So the token travels the way the arguments do: stashed by the launching process before it
//! can discover it is a second instance, spent by the running one when it decides which window
//! to raise. It is deliberately one-shot and short-lived — a saved token is not a key the app
//! may use to seize focus at some later moment of its own choosing.
//!
//! Other platforms have no such handshake, so everywhere else this is `set_focus` and nothing
//! more.

#[cfg(target_os = "linux")]
use std::path::{Path, PathBuf};
#[cfg(target_os = "linux")]
use std::time::{Duration, SystemTime};

/// A token is a receipt for a click that has just happened. Past this, the launch it stands
/// for is one the user has moved on from, and a window raised on it would arrive out of
/// nowhere. Generous enough to cover a cold webview waking up, short enough that it cannot
/// serve some unrelated activation later in the session.
#[cfg(target_os = "linux")]
const LAUNCH_TOKEN_LIFETIME: Duration = Duration::from_secs(10);

#[cfg(target_os = "linux")]
fn token_path() -> Option<PathBuf> {
    // The runtime directory is per-user and cleared at logout, which is exactly the lifetime a
    // handoff between two processes of one session wants. It is also where the single-instance
    // lock this token accompanies lives.
    std::env::var_os("XDG_RUNTIME_DIR")
        .map(|dir| PathBuf::from(dir).join("sigma-file-manager-activation-token"))
}

/// Leaves this launch's activation token where the running instance will look for it.
///
/// Called before the single-instance handoff, because a second instance never gets to run code
/// of its own again: the plugin forwards its arguments and exits from inside the Tauri
/// builder. A first instance stashes a token nobody reads and clears it in `setup`.
pub fn stash_launch_token() {
    #[cfg(target_os = "linux")]
    {
        // `DESKTOP_STARTUP_ID` is the same value under the name X11 gave it, and some launchers
        // still set only that one.
        let Some(token) = std::env::var("XDG_ACTIVATION_TOKEN")
            .or_else(|_| std::env::var("DESKTOP_STARTUP_ID"))
            .ok()
            .filter(|token| !token.is_empty())
        else {
            return;
        };

        let Some(path) = token_path() else {
            return;
        };

        // Written aside and renamed into place: the reader takes the file whole or not at all,
        // never a half-written token that the compositor would reject.
        let staged = path.with_file_name(format!(
            "sigma-file-manager-activation-token.{}",
            std::process::id()
        ));
        if std::fs::write(&staged, token).is_ok() && std::fs::rename(&staged, &path).is_err() {
            let _ = std::fs::remove_file(&staged);
        }
    }
}

/// Drops any stashed token, called by the process that turns out to own the session.
///
/// Its own launch token was already spent by GTK on its own first window. Leaving it on disk
/// would mean the next launcher click could find a token from whenever this session started
/// and raise a window on it — the compositor would refuse it, and the refusal would look
/// exactly like the bug this module exists to fix.
pub fn discard_launch_token() {
    #[cfg(target_os = "linux")]
    if let Some(path) = token_path() {
        let _ = std::fs::remove_file(path);
    }
}

/// Reads and removes the stashed token, if one is waiting and still stands for a recent click.
///
/// Removal happens either way: a token too old to spend is also too old to keep.
#[cfg(target_os = "linux")]
fn take_launch_token() -> Option<String> {
    let path = token_path()?;
    take_launch_token_at(&path, SystemTime::now())
}

#[cfg(target_os = "linux")]
fn take_launch_token_at(path: &Path, now: SystemTime) -> Option<String> {
    let modified = std::fs::metadata(path).and_then(|metadata| metadata.modified());
    let token = std::fs::read_to_string(path).ok();
    let _ = std::fs::remove_file(path);

    let modified = modified.ok()?;
    if !is_fresh(modified, now) {
        return None;
    }

    token
        .map(|token| token.trim().to_string())
        .filter(|token| !token.is_empty())
}

/// A file stamped in the future is a clock that moved, not a stale token; the runtime
/// directory it sits in is this session's either way, so it counts as fresh.
#[cfg(target_os = "linux")]
fn is_fresh(modified: SystemTime, now: SystemTime) -> bool {
    now.duration_since(modified)
        .map(|age| age <= LAUNCH_TOKEN_LIFETIME)
        .unwrap_or(true)
}

/// Raises `window`, spending a stashed launch token when there is one.
///
/// Callers show the window first; this is the focus half, and the only part that needs the
/// token.
pub fn focus<R: tauri::Runtime>(window: &tauri::WebviewWindow<R>) {
    #[cfg(target_os = "linux")]
    if let Some(token) = take_launch_token() {
        use tauri::Manager;

        let window = window.clone();
        let handle = window.app_handle().clone();

        // Both halves in one closure, on the main thread, because GTK belongs to that thread
        // alone and single-instance callbacks arrive on the DBus listener's. Split across two
        // hops they would be two queues with no order between them, and a token that lands
        // after the raise it belongs to is a token that was never presented.
        let dispatched = handle
            .run_on_main_thread(move || match window.gtk_window() {
                Ok(gtk_window) => {
                    use gtk::prelude::GtkWindowExt;

                    gtk_window.set_startup_id(&token);
                    gtk_window.present();
                }
                Err(_) => {
                    let _ = window.set_focus();
                }
            })
            .is_ok();

        if dispatched {
            return;
        }
    }

    let _ = window.set_focus();
}

#[cfg(all(test, target_os = "linux"))]
mod tests {
    use super::{is_fresh, take_launch_token_at, LAUNCH_TOKEN_LIFETIME};
    use std::time::{Duration, SystemTime};

    #[test]
    fn a_stashed_token_is_read_back() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("token");
        std::fs::write(&path, "token-from-the-click").unwrap();

        assert_eq!(
            take_launch_token_at(&path, SystemTime::now()).as_deref(),
            Some("token-from-the-click")
        );
    }

    /// One click, one raise: a token left behind would raise a window at a user who asked for
    /// nothing.
    #[test]
    fn taking_a_token_consumes_it() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("token");
        std::fs::write(&path, "token-from-the-click").unwrap();

        assert!(take_launch_token_at(&path, SystemTime::now()).is_some());
        assert!(take_launch_token_at(&path, SystemTime::now()).is_none());
        assert!(!path.exists());
    }

    #[test]
    fn a_token_older_than_its_lifetime_is_not_spent() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("token");
        std::fs::write(&path, "token-from-an-old-launch").unwrap();

        let much_later = SystemTime::now() + LAUNCH_TOKEN_LIFETIME + Duration::from_secs(1);
        assert!(take_launch_token_at(&path, much_later).is_none());
        assert!(!path.exists());
    }

    #[test]
    fn nothing_stashed_is_not_an_error() {
        let dir = tempfile::tempdir().unwrap();
        assert!(take_launch_token_at(&dir.path().join("token"), SystemTime::now()).is_none());
    }

    /// A clock that moved backwards must not be read as a token from the future.
    #[test]
    fn a_stamp_ahead_of_the_clock_counts_as_fresh() {
        let now = SystemTime::now();
        assert!(is_fresh(now + Duration::from_secs(60), now));
    }
}
