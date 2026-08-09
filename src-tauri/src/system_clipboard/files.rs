// SPDX-License-Identifier: GPL-3.0-or-later
// License: GNU GPLv3 or later. See the license file in the project root for more information.
// Copyright © 2021 - present Aleksey Hoffman. All rights reserved.
// Copyright © 2026 Cortexist, LLC (modifications). All rights reserved.

use std::path::PathBuf;

#[cfg(target_os = "windows")]
use std::iter::once;
#[cfg(target_os = "windows")]
use std::path::Path;

use super::types::SystemClipboardFiles;

#[cfg(target_os = "windows")]
use super::windows::{set_windows_clipboard_bytes, windows_open_clipboard, with_windows_clipboard};

pub(crate) fn set_system_clipboard_files_sync(
    paths: &[String],
    #[cfg_attr(
        not(any(target_os = "windows", target_os = "linux")),
        allow(unused_variables)
    )]
    operation: &str,
) -> Result<(), String> {
    if paths.is_empty() {
        return Ok(());
    }

    #[cfg(target_os = "windows")]
    {
        windows_set_file_clipboard(paths, operation == "move")?;
    }

    #[cfg(not(target_os = "windows"))]
    {
        // Wayland gets the full set of desktop targets, including the cut/copy marker.
        // Everything else — X11, macOS — keeps the plain `arboard` file list, which carries
        // the paths but cannot express cut. A Wayland failure falls through to the same
        // path, so a compositor without `wlr-data-control` is no worse off than before.
        #[cfg(target_os = "linux")]
        {
            if wayland_set_file_clipboard(paths, operation).is_ok() {
                return Ok(());
            }
        }

        unix_set_file_clipboard(paths)?;
    }

    Ok(())
}

pub(crate) fn clear_system_clipboard_files_sync() -> Result<(), String> {
    #[cfg(target_os = "windows")]
    {
        windows_clear_file_clipboard()?;
    }

    #[cfg(not(target_os = "windows"))]
    {
        unix_clear_file_clipboard()?;
    }

    Ok(())
}

pub(crate) fn read_system_clipboard_files_sync() -> Result<SystemClipboardFiles, String> {
    #[cfg(target_os = "windows")]
    {
        windows_read_file_clipboard()
    }

    #[cfg(not(target_os = "windows"))]
    {
        // Only Wayland can tell us whether the entry was a cut. Anything it cannot answer
        // falls through to `arboard`, which still yields the paths.
        #[cfg(target_os = "linux")]
        {
            if let Some(files) = wayland_read_file_clipboard() {
                return Ok(files);
            }
        }

        unix_read_file_clipboard()
    }
}

/// The target GNOME-family file managers read and write. Its body is a verb line —
/// `cut` or `copy` — followed by one `file://` URI per line, and it is the only place on a
/// Linux clipboard where the distinction is recorded.
#[cfg(target_os = "linux")]
const MIME_GNOME_COPIED_FILES: &str = "x-special/gnome-copied-files";

/// KDE records the same distinction out of band: `text/uri-list` plus this flag set to `1`.
#[cfg(target_os = "linux")]
const MIME_KDE_CUT_SELECTION: &str = "application/x-kde-cutselection";

#[cfg(target_os = "linux")]
const MIME_URI_LIST: &str = "text/uri-list";

/// Percent-encodes a path into a `file://` URI, leaving the separators intact.
///
/// Hand-rolled rather than pulled from a URL crate because only the unreserved set plus `/`
/// may survive: a `#` or `%` in a filename has to be escaped or the receiving file manager
/// truncates or mis-decodes the path.
#[cfg(target_os = "linux")]
fn path_to_file_uri(path: &str) -> String {
    let mut uri = String::from("file://");

    for byte in path.as_bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' | b'/' => {
                uri.push(*byte as char);
            }
            _ => uri.push_str(&format!("%{byte:02X}")),
        }
    }

    uri
}

/// Inverse of [`path_to_file_uri`]. Returns `None` for anything that is not a local file,
/// so remote URIs on the clipboard are ignored rather than turned into bogus local paths.
#[cfg(target_os = "linux")]
fn file_uri_to_path(uri: &str) -> Option<String> {
    let trimmed = uri.trim();

    if trimmed.is_empty() || trimmed.starts_with('#') {
        return None;
    }

    let rest = trimmed.strip_prefix("file://")?;

    // An authority may be present but must be empty or `localhost` for a local file.
    let encoded_path = if rest.starts_with('/') {
        rest
    } else {
        let (authority, path) = rest.split_once('/')?;

        if !authority.is_empty() && !authority.eq_ignore_ascii_case("localhost") {
            return None;
        }

        // `split_once` consumed the leading separator.
        return urlencoding::decode(&format!("/{path}"))
            .ok()
            .map(|decoded| decoded.into_owned());
    };

    urlencoding::decode(encoded_path)
        .ok()
        .map(|decoded| decoded.into_owned())
}

#[cfg(target_os = "linux")]
fn wayland_set_file_clipboard(paths: &[String], operation: &str) -> Result<(), String> {
    use wl_clipboard_rs::copy::{MimeSource, MimeType, Options, Source};

    let is_cut = operation == "move";
    let uris: Vec<String> = paths.iter().map(|path| path_to_file_uri(path)).collect();

    fn source_of(value: String) -> Source {
        Source::Bytes(value.into_bytes().into_boxed_slice())
    }

    let mut sources = vec![
        MimeSource {
            source: source_of(format!(
                "{}\n{}",
                if is_cut { "cut" } else { "copy" },
                uris.join("\n")
            )),
            mime_type: MimeType::Specific(MIME_GNOME_COPIED_FILES.to_string()),
        },
        // Offered alongside so managers that only understand the drag-and-drop target — and
        // `arboard` itself, when this app reads its own entry back — still see the paths.
        MimeSource {
            source: source_of(uris.join("\r\n")),
            mime_type: MimeType::Specific(MIME_URI_LIST.to_string()),
        },
        MimeSource {
            source: source_of(paths.join("\n")),
            mime_type: MimeType::Specific("text/plain".to_string()),
        },
    ];

    if is_cut {
        sources.push(MimeSource {
            source: source_of("1".to_string()),
            mime_type: MimeType::Specific(MIME_KDE_CUT_SELECTION.to_string()),
        });
    }

    let mut options = Options::new();
    // Serve the offer from a background thread. Blocking here would hold the clipboard
    // command until the next application took ownership.
    options.foreground(false);
    options
        .copy_multi(sources)
        .map_err(|error| error.to_string())
}

/// Reads one clipboard target, treating "not offered" as absence rather than failure.
#[cfg(target_os = "linux")]
fn wayland_read_target(mime_type: &str) -> Option<Vec<u8>> {
    use std::io::Read;
    use wl_clipboard_rs::paste::{get_contents, ClipboardType, Error, MimeType, Seat};

    match get_contents(
        ClipboardType::Regular,
        Seat::Unspecified,
        MimeType::Specific(mime_type),
    ) {
        Ok((mut reader, _)) => {
            let mut buffer = Vec::new();
            reader.read_to_end(&mut buffer).ok()?;
            Some(buffer)
        }
        Err(Error::NoMimeType | Error::ClipboardEmpty | Error::NoSeats) => None,
        Err(_) => None,
    }
}

/// `None` means "nothing this layer can answer" — no Wayland, or no file targets on the
/// clipboard — and the caller falls back to `arboard`.
#[cfg(target_os = "linux")]
fn wayland_read_file_clipboard() -> Option<SystemClipboardFiles> {
    if let Some(bytes) = wayland_read_target(MIME_GNOME_COPIED_FILES) {
        let body = String::from_utf8_lossy(&bytes);
        let mut lines = body.lines();
        let verb = lines.next().unwrap_or_default().trim().to_ascii_lowercase();
        let paths: Vec<String> = lines.filter_map(file_uri_to_path).collect();

        if !paths.is_empty() {
            return Some(SystemClipboardFiles {
                paths,
                operation: if verb == "cut" { "move" } else { "copy" }.to_string(),
            });
        }
    }

    let uri_list = wayland_read_target(MIME_URI_LIST)?;
    let paths: Vec<String> = String::from_utf8_lossy(&uri_list)
        .lines()
        .filter_map(file_uri_to_path)
        .collect();

    if paths.is_empty() {
        return None;
    }

    let is_kde_cut = wayland_read_target(MIME_KDE_CUT_SELECTION)
        .is_some_and(|bytes| String::from_utf8_lossy(&bytes).trim() == "1");

    Some(SystemClipboardFiles {
        paths,
        operation: if is_kde_cut { "move" } else { "copy" }.to_string(),
    })
}

#[cfg(not(target_os = "windows"))]
fn unix_clear_file_clipboard() -> Result<(), String> {
    let mut clipboard = arboard::Clipboard::new().map_err(|error| error.to_string())?;
    clipboard
        .set_text(String::new())
        .map_err(|error| error.to_string())?;
    Ok(())
}

#[cfg(not(target_os = "windows"))]
fn unix_set_file_clipboard(paths: &[String]) -> Result<(), String> {
    let path_bufs: Vec<PathBuf> = paths.iter().map(PathBuf::from).collect();
    let mut clipboard = arboard::Clipboard::new().map_err(|error| error.to_string())?;
    clipboard
        .set()
        .file_list(&path_bufs)
        .map_err(|error| error.to_string())?;
    Ok(())
}

#[cfg(not(target_os = "windows"))]
fn unix_read_file_clipboard() -> Result<SystemClipboardFiles, String> {
    let mut clipboard = arboard::Clipboard::new().map_err(|error| error.to_string())?;
    let file_paths = match clipboard.get().file_list() {
        Ok(paths) => paths,
        Err(arboard::Error::ContentNotAvailable) => Vec::new(),
        Err(error) => return Err(error.to_string()),
    };
    let paths = file_paths
        .into_iter()
        .map(|path| path.to_string_lossy().into_owned())
        .collect();

    Ok(SystemClipboardFiles {
        paths,
        operation: "copy".to_string(),
    })
}

#[cfg(target_os = "windows")]
fn format_hdrop_path(path: &Path) -> PathBuf {
    let mut path_string = path.to_string_lossy().to_string();
    if let Some(rest) = path_string.strip_prefix(r"\\?\UNC\") {
        path_string = format!(r"\\{rest}");
    } else if let Some(rest) = path_string.strip_prefix(r"\\?\") {
        path_string = rest.to_string();
    }
    path_string = path_string.replace('/', "\\");
    PathBuf::from(path_string)
}

#[cfg(target_os = "windows")]
fn prepare_clipboard_paths(paths: &[String]) -> Vec<PathBuf> {
    paths
        .iter()
        .map(|path| {
            let shell_path = dunce::canonicalize(path).unwrap_or_else(|_| PathBuf::from(path));
            format_hdrop_path(&shell_path)
        })
        .collect()
}

#[cfg(target_os = "windows")]
fn windows_clear_file_clipboard() -> Result<(), String> {
    with_windows_clipboard(|| {
        use windows::Win32::System::DataExchange::{CloseClipboard, EmptyClipboard};

        unsafe {
            windows_open_clipboard()?;
            let clipboard_result =
                EmptyClipboard().map_err(|error| format!("EmptyClipboard failed: {error}"));
            let _ = CloseClipboard();
            clipboard_result
        }
    })
}

#[cfg(target_os = "windows")]
fn windows_set_file_clipboard(paths: &[String], is_move: bool) -> Result<(), String> {
    use std::os::windows::ffi::OsStrExt;

    use windows::core::w;
    use windows::Win32::Foundation::{BOOL, HANDLE, POINT};
    use windows::Win32::System::DataExchange::{
        CloseClipboard, EmptyClipboard, RegisterClipboardFormatW, SetClipboardData,
    };
    use windows::Win32::System::Memory::{GlobalAlloc, GlobalLock, GlobalUnlock, GHND};
    use windows::Win32::System::Ole::{
        CF_HDROP, DROPEFFECT_COPY, DROPEFFECT_LINK, DROPEFFECT_MOVE,
    };
    use windows::Win32::UI::Shell::DROPFILES;

    let hdrop_paths = prepare_clipboard_paths(paths);
    if hdrop_paths.is_empty() {
        return Ok(());
    }

    with_windows_clipboard(|| unsafe {
        windows_open_clipboard()?;
        let clipboard_result = (|| {
            EmptyClipboard().map_err(|error| format!("EmptyClipboard failed: {error}"))?;

            let dropfiles_header_size = std::mem::size_of::<DROPFILES>();
            let mut wide_buffer: Vec<u16> = Vec::new();
            for path in &hdrop_paths {
                let wide_path: Vec<u16> = path.as_os_str().encode_wide().chain(once(0)).collect();
                wide_buffer.extend(wide_path);
            }
            wide_buffer.push(0);

            let allocation_size = dropfiles_header_size + wide_buffer.len() * 2;
            let global_handle = GlobalAlloc(GHND, allocation_size)
                .map_err(|error| format!("GlobalAlloc failed: {error}"))?;
            let locked_pointer = GlobalLock(global_handle);
            if locked_pointer.is_null() {
                return Err("GlobalLock failed".to_string());
            }

            let dropfiles = DROPFILES {
                pFiles: dropfiles_header_size as u32,
                pt: POINT { x: 0, y: 0 },
                fNC: BOOL(0),
                fWide: BOOL(1),
            };

            *(locked_pointer as *mut DROPFILES) = dropfiles;
            std::ptr::copy_nonoverlapping(
                wide_buffer.as_ptr(),
                locked_pointer.add(dropfiles_header_size) as *mut u16,
                wide_buffer.len(),
            );
            let _ = GlobalUnlock(global_handle);

            SetClipboardData(CF_HDROP.0 as u32, HANDLE(global_handle.0))
                .map_err(|error| format!("SetClipboardData CF_HDROP failed: {error}"))?;

            if let Some(first_path) = hdrop_paths.first() {
                let file_name_w_format = RegisterClipboardFormatW(w!("FileNameW"));
                let file_name_w: Vec<u16> = first_path
                    .as_os_str()
                    .encode_wide()
                    .chain(once(0))
                    .collect();
                let file_name_w_bytes = std::slice::from_raw_parts(
                    file_name_w.as_ptr() as *const u8,
                    file_name_w.len() * 2,
                );
                set_windows_clipboard_bytes(file_name_w_format, file_name_w_bytes)?;

                let file_name_format = RegisterClipboardFormatW(w!("FileName"));
                let file_name = format!("{}\0", first_path.to_string_lossy());
                set_windows_clipboard_bytes(file_name_format, file_name.as_bytes())?;
            }

            let preferred_drop_effect_format = RegisterClipboardFormatW(w!("Preferred DropEffect"));
            let drop_effect = if is_move {
                DROPEFFECT_MOVE.0
            } else {
                (DROPEFFECT_COPY | DROPEFFECT_LINK).0
            };

            set_windows_clipboard_bytes(preferred_drop_effect_format, &drop_effect.to_ne_bytes())?;

            Ok(())
        })();

        let _ = CloseClipboard();
        clipboard_result
    })
}

#[cfg(target_os = "windows")]
fn windows_read_file_clipboard() -> Result<SystemClipboardFiles, String> {
    use windows::core::w;
    use windows::Win32::Foundation::HGLOBAL;
    use windows::Win32::System::DataExchange::{
        CloseClipboard, GetClipboardData, IsClipboardFormatAvailable, RegisterClipboardFormatW,
    };
    use windows::Win32::System::Memory::{GlobalLock, GlobalUnlock};
    use windows::Win32::System::Ole::{CF_HDROP, DROPEFFECT_MOVE};
    use windows::Win32::UI::Shell::{DragQueryFileW, HDROP};

    with_windows_clipboard(|| unsafe {
        windows_open_clipboard()?;
        let clipboard_result = (|| {
            if IsClipboardFormatAvailable(CF_HDROP.0 as u32).is_err() {
                return Ok(SystemClipboardFiles {
                    paths: Vec::new(),
                    operation: "copy".to_string(),
                });
            }

            let clipboard_handle = GetClipboardData(CF_HDROP.0 as u32)
                .map_err(|error| format!("GetClipboardData CF_HDROP failed: {error}"))?;
            let hdrop = HDROP(clipboard_handle.0);
            let file_count = DragQueryFileW(hdrop, 0xffffffff, None);
            let mut paths = Vec::with_capacity(file_count as usize);

            for file_index in 0..file_count {
                let required_length = DragQueryFileW(hdrop, file_index, None);
                let mut wide_buffer = vec![0u16; required_length as usize + 1];
                let copied_length = DragQueryFileW(hdrop, file_index, Some(&mut wide_buffer));
                if copied_length > 0 {
                    wide_buffer.truncate(copied_length as usize);
                    paths.push(String::from_utf16_lossy(&wide_buffer));
                }
            }

            let preferred_drop_effect_format = RegisterClipboardFormatW(w!("Preferred DropEffect"));
            let mut operation = "copy".to_string();
            if IsClipboardFormatAvailable(preferred_drop_effect_format).is_ok() {
                let effect_handle = GetClipboardData(preferred_drop_effect_format)
                    .map_err(|error| format!("GetClipboardData drop effect failed: {error}"))?;
                let effect_pointer = GlobalLock(HGLOBAL(effect_handle.0));
                if !effect_pointer.is_null() {
                    let drop_effect = *(effect_pointer as *const u32);
                    if drop_effect & DROPEFFECT_MOVE.0 != 0 {
                        operation = "move".to_string();
                    }
                    let _ = GlobalUnlock(HGLOBAL(effect_handle.0));
                }
            }

            Ok(SystemClipboardFiles { paths, operation })
        })();

        let _ = CloseClipboard();
        clipboard_result
    })
}

#[cfg(all(test, target_os = "linux"))]
mod tests {
    use super::{
        file_uri_to_path, path_to_file_uri, read_system_clipboard_files_sync,
        set_system_clipboard_files_sync, MIME_GNOME_COPIED_FILES, MIME_URI_LIST,
    };
    use std::sync::{LazyLock, Mutex, MutexGuard};

    /// The system clipboard is a single global resource, and the harness runs tests in
    /// parallel, so the tests that touch it would otherwise overwrite each other's entries
    /// and fail at random. Poisoning is ignored: a panic in one of them says nothing about
    /// whether the next can run.
    static CLIPBOARD_LOCK: LazyLock<Mutex<()>> = LazyLock::new(|| Mutex::new(()));

    fn lock_clipboard() -> MutexGuard<'static, ()> {
        CLIPBOARD_LOCK
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    #[test]
    fn plain_paths_round_trip_through_a_file_uri() {
        let path = "/home/user/Pictures/photo.jpg";
        assert_eq!(
            path_to_file_uri(path),
            "file:///home/user/Pictures/photo.jpg"
        );
        assert_eq!(
            file_uri_to_path(&path_to_file_uri(path)).as_deref(),
            Some(path)
        );
    }

    /// Characters that would otherwise truncate or corrupt the path in the receiving file
    /// manager: `#` starts a comment in a uri-list, `%` introduces an escape, and a space
    /// ends the URI for some parsers.
    #[test]
    fn awkward_characters_survive_the_round_trip() {
        for path in [
            "/home/user/holiday photos/a b.jpg",
            "/home/user/100% done/report.pdf",
            "/home/user/notes#1.txt",
            "/home/user/Ünïcödé/文件.txt",
            "/home/user/quote'and\"double.txt",
            "/home/user/plus+amp&.txt",
        ] {
            let uri = path_to_file_uri(path);
            assert!(!uri.contains(' '), "space left unescaped in {uri}");
            assert_eq!(
                file_uri_to_path(&uri).as_deref(),
                Some(path),
                "failed for {path}"
            );
        }
    }

    #[test]
    fn separators_are_not_escaped() {
        // Escaping `/` would turn the path into a single opaque segment.
        assert_eq!(path_to_file_uri("/a/b/c"), "file:///a/b/c");
    }

    #[test]
    fn non_local_and_malformed_uris_are_ignored() {
        assert_eq!(file_uri_to_path("https://example.com/a.txt"), None);
        assert_eq!(file_uri_to_path("file://otherhost/a.txt"), None);
        assert_eq!(file_uri_to_path("# a uri-list comment"), None);
        assert_eq!(file_uri_to_path("   "), None);
    }

    #[test]
    fn localhost_authority_is_accepted() {
        assert_eq!(
            file_uri_to_path("file://localhost/home/user/a.txt").as_deref(),
            Some("/home/user/a.txt")
        );
    }

    /// End-to-end against the real compositor. Skipped off Wayland, so it does not fail in
    /// CI or an X11 session; run it locally to check interop for real.
    ///
    /// Both verbs live in one test on purpose: the system clipboard is global, and separate
    /// test functions race each other because the harness runs them in parallel. It also
    /// clobbers whatever the user had copied, which is why it clears up afterwards.
    #[test]
    fn wayland_round_trips_both_verbs_with_the_markers_other_managers_read() {
        let _guard = lock_clipboard();
        if std::env::var_os("WAYLAND_DISPLAY").is_none() {
            eprintln!("skipping: not a Wayland session");
            return;
        }

        fn gnome_target_body() -> String {
            let output = std::process::Command::new("wl-paste")
                .args(["-t", MIME_GNOME_COPIED_FILES])
                .output()
                .expect("wl-paste is required for this test");
            String::from_utf8_lossy(&output.stdout).into_owned()
        }

        // A cut, with a path awkward enough to catch encoding mistakes.
        let cut_paths = vec!["/tmp/sfm clipboard test/a b.txt".to_string()];
        set_system_clipboard_files_sync(&cut_paths, "move").expect("failed to set clipboard");

        let read = read_system_clipboard_files_sync().expect("failed to read clipboard");
        assert_eq!(read.operation, "move");
        assert_eq!(read.paths, cut_paths);

        let body = gnome_target_body();
        assert!(
            body.starts_with("cut\n"),
            "expected a cut verb, got: {body:?}"
        );
        assert!(
            body.contains("file:///tmp/sfm%20clipboard%20test/a%20b.txt"),
            "got: {body:?}"
        );

        // A copy over the top of it must not inherit the previous verb.
        let copy_paths = vec!["/tmp/sfm-clipboard-test/plain.txt".to_string()];
        set_system_clipboard_files_sync(&copy_paths, "copy").expect("failed to set clipboard");

        let read = read_system_clipboard_files_sync().expect("failed to read clipboard");
        assert_eq!(read.operation, "copy");
        assert_eq!(read.paths, copy_paths);
        assert!(gnome_target_body().starts_with("copy\n"));

        let _ = super::clear_system_clipboard_files_sync();
    }

    /// The inbound half of interop: publish exactly what a GNOME-family file manager puts on
    /// the clipboard for a cut, using an unrelated tool, and check this app reads it back as
    /// a move. Before this, such an entry was either invisible or silently read as a copy.
    #[test]
    fn reads_a_cut_published_by_another_file_manager() {
        let _guard = lock_clipboard();
        if std::env::var_os("WAYLAND_DISPLAY").is_none() {
            eprintln!("skipping: not a Wayland session");
            return;
        }

        fn publish(mime_type: &str, body: &str) {
            use std::io::Write;
            use std::process::{Command, Stdio};

            let mut child = Command::new("wl-copy")
                .args(["-t", mime_type])
                .stdin(Stdio::piped())
                .spawn()
                .expect("wl-copy is required for this test");
            child
                .stdin
                .as_mut()
                .expect("stdin")
                .write_all(body.as_bytes())
                .expect("write");
            child.wait().expect("wl-copy failed");
        }

        publish(
            MIME_GNOME_COPIED_FILES,
            "cut\nfile:///tmp/from%20nautilus/report.pdf",
        );

        let read = read_system_clipboard_files_sync().expect("failed to read clipboard");
        assert_eq!(read.operation, "move");
        assert_eq!(
            read.paths,
            vec!["/tmp/from nautilus/report.pdf".to_string()]
        );

        // A bare uri-list carries no verb, so it must be treated as a copy rather than
        // inheriting the cut above.
        publish(MIME_URI_LIST, "file:///tmp/from%20nautilus/report.pdf");

        let read = read_system_clipboard_files_sync().expect("failed to read clipboard");
        assert_eq!(read.operation, "copy");
        assert_eq!(
            read.paths,
            vec!["/tmp/from nautilus/report.pdf".to_string()]
        );

        let _ = super::clear_system_clipboard_files_sync();
    }
}
