// SPDX-License-Identifier: GPL-3.0-or-later
// License: GNU GPLv3 or later. See the license file in the project root for more information.
// Copyright © 2021 - present Aleksey Hoffman. All rights reserved.
// Copyright © 2026 Cortexist, LLC (modifications). All rights reserved.

mod app_updater;
mod archive;
mod audio_covers;
mod background_sources;
mod clipboard_source;
mod clipboard_watcher;
mod copy_move_job;
mod default_file_manager;
mod delete_job;
mod dir_reader;
mod dir_size;
mod dir_watcher;
mod extensions;
mod file_chooser_registration;
mod file_manager1;
mod file_operations;
mod file_picker;
mod global_search;
mod image_thumbnails;
mod input_simulation;
mod lan_share;
mod link_operations;
mod media_info;
mod media_server;
mod media_viewer_registration;
mod open_with;
#[cfg(target_os = "linux")]
mod portal_file_chooser;
mod process_runner;
mod standalone_viewer;
mod startup_storage_bootstrap;
mod system_clipboard;
mod system_icons;
mod system_tray;
mod terminal;
mod trash_bin;
#[cfg(windows)]
mod url_drop;
mod user_storage_files_config;
pub mod utils;
#[cfg(target_os = "linux")]
mod video_thumbnails;
mod webview_file_chooser;
mod window_manager;
mod windows_installation;
#[cfg(windows)]
mod windows_print_view_webview;
#[cfg(target_os = "linux")]
mod xdg_associations;

use serde::Serialize;
use tauri::{Emitter, Manager};

const SIGMA_AUTOSTART_CLI_FLAG: &str = "--sigma-autostart";
const AUXILIARY_WINDOW_RELEASE_EVENT: &str = "auxiliary-window:release";
/// Mirrors `QUICK_VIEW_RESTORED_EVENT` in `stores/runtime/quick-view.ts`.
const QUICK_VIEW_RESTORED_EVENT: &str = "quick-view:restored";
const PRINT_VIEW_NATIVE_CLOSE_REQUESTED_EVENT: &str = "print-view:native-close-requested";

/// Set while the user has dismissed the main window to the background — its own titlebar
/// close button, or an autostart configured to begin hidden. A resident session survives the
/// nothing-visible exit checks; the launcher, whose next activation focuses the main window,
/// is the way back. Ending the session for real is the window manager's close (the sweep in
/// the `CloseRequested` handler), which never sets this.
#[derive(Default)]
struct BackgroundResidency(std::sync::atomic::AtomicBool);

impl BackgroundResidency {
    fn set(&self, resident: bool) {
        self.0.store(resident, std::sync::atomic::Ordering::Relaxed);
    }

    fn active(&self) -> bool {
        self.0.load(std::sync::atomic::Ordering::Relaxed)
    }
}

/// Set while Quick View was dismissed with something still playing. The window is hidden but
/// its page runs on, so the media keeps going; the session ends when the user brings the
/// window back, closes it outright, or the file reaches its end.
#[derive(Default)]
struct QuickViewBackgroundPlayback(std::sync::atomic::AtomicBool);

impl QuickViewBackgroundPlayback {
    fn set(&self, playing: bool) {
        self.0.store(playing, std::sync::atomic::Ordering::Relaxed);
    }

    fn active(&self) -> bool {
        self.0.load(std::sync::atomic::Ordering::Relaxed)
    }
}

/// Reasons the app should outlive its last window; the lifetime rule lives in one place
/// instead of being re-derived at each close site.
///
/// Background residency is the first reason: the main window dismissed by its own close
/// button rather than by the window manager. Quick View playing on after being dismissed is
/// the second — the mini player this comment used to anticipate — as would be a running copy
/// job. Whatever registers a reason is also responsible for giving the user a way back to a
/// window, since quitting will no longer happen on its own; for playback that way back is
/// `show_playing_quick_view`, reached from the launcher and the tray.
fn should_keep_running_without_windows(app: &tauri::AppHandle) -> bool {
    app.state::<BackgroundResidency>().active()
        || app.state::<QuickViewBackgroundPlayback>().active()
}

/// Reports whether Quick View kept playing after being dismissed. Called by the page itself:
/// it is the only side that knows whether anything was playing when the window went away, and
/// it calls again when playback ends so a finished file stops holding the process open.
#[tauri::command]
fn set_quick_view_background_playback(app: tauri::AppHandle, playing: bool) {
    app.state::<QuickViewBackgroundPlayback>().set(playing);

    // A file that played itself out while hidden leaves nothing on screen and no reason to
    // stay, which is the moment the app would have quit had it not been playing.
    if !playing {
        exit_if_last_window_closed(&app, &[]);
    }
}

/// Whether a launch names nothing to open — the user asking for the app itself rather than
/// for a file. Flags carry no target, so they leave an activation bare.
fn is_bare_activation(argv: &[String]) -> bool {
    !argv.iter().skip(1).any(|arg| !arg.starts_with('-'))
}

/// Brings a backgrounded Quick View back, and reports whether there was one.
///
/// This is the way back that playing in the background obliges the app to provide. Activation
/// paths try it before falling back to the main window: a user who dismissed Quick View by
/// accident and reaches for the launcher is asking for what they can hear, not for the file
/// manager behind it.
pub(crate) fn show_playing_quick_view<R: tauri::Runtime>(app: &tauri::AppHandle<R>) -> bool {
    if !app.state::<QuickViewBackgroundPlayback>().active() {
        return false;
    }

    let Some(quick_view) = app.get_webview_window("quick-view") else {
        return false;
    };

    let _ = quick_view.show();
    let _ = quick_view.set_focus();
    app.state::<QuickViewBackgroundPlayback>().set(false);

    // The page keeps its own half of the session — what it tells the main window, and what it
    // does when the file ends — so being put back on screen has to reach it too.
    let _ = app.emit_to(
        tauri::EventTarget::webview_window("quick-view"),
        QUICK_VIEW_RESTORED_EVENT,
        (),
    );

    true
}

/// The main window's own close button: hide, and stay resident for an instant next open.
/// The window manager's close on the same window ends the whole session instead, which
/// keeps the two gestures the user has distinct — dismiss versus quit.
#[tauri::command]
fn dismiss_main_window_to_background(app: tauri::AppHandle) {
    if let Some(main_window) = app.get_webview_window("main") {
        let _ = main_window.hide();
    }

    // Dismissing sigma dismisses what sigma put up — its own quick view content goes to the
    // background with it. A viewer serving another application's file is not sigma's to hide.
    if !app.state::<QuickViewOwnership>().is_external() {
        if let Some(quick_view) = app.get_webview_window("quick-view") {
            let _ = quick_view.hide();
        }
    }

    app.state::<BackgroundResidency>().set(true);
}

/// Whether closing `closing_label` leaves the user with nothing, so the app should quit.
///
/// Takes the window list rather than an `AppHandle` purely so the rule can be tested; the
/// caller does the querying.
///
/// Judged on *visibility*, not existence: auxiliary windows are prelaunched and merely hidden
/// when dismissed, so a quick-view window is usually alive even when the user can see nothing
/// at all. The window being closed is excluded by label because it has only just been hidden
/// and may still report itself as visible.
///
/// Auxiliary windows can outlive the main window here — that is what a standalone viewer
/// session is — so only the absence of every other visible window ends the process. The one
/// asymmetry lives in the close handler, not in this rule: closing the *main* window hides
/// the session's auxiliary windows first, so dismissing the file manager ends the session
/// rather than leaving a forgotten viewer keeping the app alive.
fn should_exit_after_close<I, S>(windows: I, closing_labels: &[&str], keep_running: bool) -> bool
where
    I: IntoIterator<Item = (S, bool)>,
    S: AsRef<str>,
{
    if keep_running {
        return false;
    }

    !windows
        .into_iter()
        .any(|(label, is_visible)| !closing_labels.contains(&label.as_ref()) && is_visible)
}

/// Every window the main session can own. Closing the main window closes the owned ones,
/// which is also why they are excluded from the exit check at that moment: each was hidden a
/// breath ago and may still report itself visible.
const SESSION_WINDOW_LABELS: [&str; 3] = ["main", "quick-view", "print-view"];

/// Quick view belongs to its *last caller*. Content sigma's own browsing put up is the main
/// window's to sweep; content another application handed over is a viewing session sigma did
/// not start and must not end — the viewer stays, and the process stays with it, when the
/// main window closes. Ownership flips on every load, so whoever spoke last owns the window.
#[derive(Default)]
struct QuickViewOwnership {
    external: std::sync::atomic::AtomicBool,
}

impl QuickViewOwnership {
    fn set_external(&self, external: bool) {
        self.external
            .store(external, std::sync::atomic::Ordering::Relaxed);
    }

    fn is_external(&self) -> bool {
        self.external.load(std::sync::atomic::Ordering::Relaxed)
    }
}

#[tauri::command]
fn set_quick_view_ownership(app: tauri::AppHandle, external: bool) {
    app.state::<QuickViewOwnership>().set_external(external);
}

/// Which windows closing the main window takes with it: its own satellites, never a viewer
/// currently serving another application.
fn session_closing_labels(quick_view_owned_externally: bool) -> &'static [&'static str] {
    if quick_view_owned_externally {
        &["main", "print-view"]
    } else {
        &SESSION_WINDOW_LABELS
    }
}

/// Lets the frontend re-run the check after hiding a window itself.
///
/// Not every window disappears through a close request: auxiliary windows are hidden when
/// released or prelaunched, and the print view hides itself when finished. Those paths never
/// reach `CloseRequested`, so without this the app could be left running with nothing on
/// screen. Nothing is excluded here — no window is mid-close, the caller has already hidden
/// whatever it hid.
#[tauri::command]
fn exit_if_no_windows_left(app: tauri::AppHandle) {
    exit_if_last_window_closed(&app, &[]);
}

/// Quits once the last visible window goes away. Called after the closing windows are hidden.
fn exit_if_last_window_closed(app: &tauri::AppHandle, closing_labels: &[&str]) {
    let windows: Vec<(String, bool)> = app
        .webview_windows()
        .into_iter()
        .map(|(label, window)| (label, window.is_visible().unwrap_or(false)))
        .collect();

    if should_exit_after_close(
        windows,
        closing_labels,
        should_keep_running_without_windows(app),
    ) {
        app.exit(0);
    }
}

fn handle_auxiliary_window_close_requested(window: &tauri::Window, api: &tauri::CloseRequestApi) {
    let label = window.label();

    if label != "print-view" && label != "quick-view" {
        return;
    }

    api.prevent_close();

    if label == "print-view" {
        let _ = window.emit_to("print-view", PRINT_VIEW_NATIVE_CLOSE_REQUESTED_EVENT, ());
    }

    let _ = window.hide();
    let _ = window.emit_to(
        "main",
        AUXILIARY_WINDOW_RELEASE_EVENT,
        serde_json::json!({ "label": label }),
    );
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct OpenMediaRequest {
    path: String,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct LaunchContext {
    args: Vec<String>,
    cwd: Option<String>,
    executable_dir: Option<String>,
    had_absorbed_shell_paths: bool,
    had_delegated_shell_paths: bool,
}

#[cfg(windows)]
fn is_shell_namespace_path(path: &str) -> bool {
    let lower = path.to_ascii_lowercase();
    path.starts_with("::{") || lower.starts_with("shell:")
}

#[cfg(windows)]
fn is_sfm_absorbed_shell_path(path: &str) -> bool {
    let lower = path.to_ascii_lowercase();
    matches!(
        lower.as_str(),
        "::{20d04fe0-3aea-1069-a2d8-08002b30309d}"
            | "shell:mycomputerfolder"
            | "shell:::{20d04fe0-3aea-1069-a2d8-08002b30309d}"
    )
}

#[cfg(windows)]
fn open_in_native_explorer(path: &str) {
    let _ = std::process::Command::new("explorer.exe").arg(path).spawn();
}

#[cfg(windows)]
struct ShellFilterResult {
    filtered_args: Vec<String>,
    had_absorbed_paths: bool,
    delegated_paths: Vec<String>,
}

#[cfg(windows)]
impl ShellFilterResult {
    fn had_delegated_paths(&self) -> bool {
        !self.delegated_paths.is_empty()
    }
}

#[cfg(windows)]
fn filter_shell_namespace_args(args: Vec<String>) -> ShellFilterResult {
    let mut filtered = Vec::with_capacity(args.len());
    let mut had_absorbed_paths = false;
    let mut delegated_paths = Vec::new();

    for (index, arg) in args.into_iter().enumerate() {
        if index == 0 {
            filtered.push(arg);
            continue;
        }

        if is_shell_namespace_path(&arg) {
            if is_sfm_absorbed_shell_path(&arg) {
                had_absorbed_paths = true;
            } else {
                delegated_paths.push(arg);
            }
            continue;
        }

        filtered.push(arg);
    }

    ShellFilterResult {
        filtered_args: filtered,
        had_absorbed_paths,
        delegated_paths,
    }
}

#[cfg(windows)]
fn delegate_shell_namespace_paths(paths: &[String]) {
    for path in paths {
        open_in_native_explorer(path);
    }
}

fn launched_from_autostart(args: &[String]) -> bool {
    args.iter().any(|arg| arg == SIGMA_AUTOSTART_CLI_FLAG)
}

fn build_launch_context(
    args: Vec<String>,
    cwd: Option<String>,
    had_absorbed_shell_paths: bool,
    had_delegated_shell_paths: bool,
) -> LaunchContext {
    let executable_dir = std::env::current_exe().ok().and_then(|path| {
        path.parent()
            .map(|parent| parent.to_string_lossy().into_owned())
    });

    let resolved_cwd = cwd.or_else(|| {
        std::env::current_dir()
            .ok()
            .map(|path| path.to_string_lossy().into_owned())
    });

    LaunchContext {
        args,
        cwd: resolved_cwd,
        executable_dir,
        had_absorbed_shell_paths,
        had_delegated_shell_paths,
    }
}

#[tauri::command]
fn configure_webview_hide_pdf_more_settings(window: tauri::WebviewWindow) {
    #[cfg(windows)]
    {
        match window.label() {
            "print-view" | "quick-view" => {
                windows_print_view_webview::hide_pdf_more_settings_toolbar(&window);
            }
            _ => {}
        }
    }

    #[cfg(not(windows))]
    {
        let _ = window;
    }
}

#[tauri::command]
fn get_launch_context() -> LaunchContext {
    let raw_args: Vec<String> = std::env::args().collect();

    #[cfg(windows)]
    {
        let filter_result = filter_shell_namespace_args(raw_args);
        let had_delegated_shell_paths = filter_result.had_delegated_paths();
        build_launch_context(
            filter_result.filtered_args,
            None,
            filter_result.had_absorbed_paths,
            had_delegated_shell_paths,
        )
    }
    #[cfg(not(windows))]
    {
        build_launch_context(raw_args, None, false, false)
    }
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    // A portal backend must not consume the portals it provides: with GTK_USE_PORTAL=1 in the
    // session environment, any GTK dialog in any sigma process would route through
    // xdg-desktop-portal back into sigma itself. Forced off before GTK reads the environment.
    #[cfg(target_os = "linux")]
    std::env::set_var("GTK_USE_PORTAL", "0");

    let raw_args: Vec<String> = std::env::args().collect();

    // The portal-service launch serves file dialogs and nothing else: claim the backend name,
    // spawn a picker process per request — no Tauri, no GTK, no windows, no webviews. An
    // application asking for a file dialog must never boot a file-manager session; this is
    // the same standalone rule the viewer and picker processes follow. Never returns.
    #[cfg(target_os = "linux")]
    if portal_file_chooser::launched_as_portal_service(&raw_args) {
        portal_file_chooser::run_service();
    }

    // A picker process is one dialog answering one request; the single-instance lock must not
    // apply, or a second concurrent dialog would forward its request to the first and exit.
    let picker_request = file_picker::picker_request_from_args(&raw_args);
    let is_picker_process = picker_request.is_some();

    // The portal role is claimed here, before any GTK or webview work, because
    // xdg-desktop-portal blocks its own startup waiting for this bus name (25-second
    // activation timeout, after which the session simply has no FileChooser), while GTK init
    // below can synchronously call into the still-blocked xdg-desktop-portal. Claiming first
    // breaks the cycle; a duplicate claim from a doomed second instance only queues and
    // evaporates with the process, since the name is requested without replacement flags.
    #[cfg(target_os = "linux")]
    if !is_picker_process
        && standalone_viewer::media_file_from_args(
            &raw_args,
            std::env::current_dir().ok().as_deref(),
        )
        .is_none()
    {
        portal_file_chooser::start();
    }

    let builder = tauri::Builder::default()
        .manage(startup_storage_bootstrap::StartupStorageBootstrapState::default())
        .manage(BackgroundResidency::default())
        .manage(QuickViewOwnership::default())
        .manage(QuickViewBackgroundPlayback::default())
        .manage(file_picker::PickerSession(picker_request))
        .manage(file_manager1::PendingShowRequests::default());

    let builder = if is_picker_process {
        builder
    } else {
        builder.plugin(tauri_plugin_single_instance::init(|app, argv, cwd| {
            #[cfg(windows)]
            {
                let filter_result = filter_shell_namespace_args(argv);
                delegate_shell_namespace_paths(&filter_result.delegated_paths);
                let has_filesystem_paths = filter_result.filtered_args.len() > 1;
                let should_focus = has_filesystem_paths
                    || filter_result.had_absorbed_paths
                    || !filter_result.had_delegated_paths();

                if should_focus {
                    // Same rule as the other platforms: a bare activation prefers whatever is
                    // still playing, anything naming paths belongs to the main window.
                    if has_filesystem_paths || !show_playing_quick_view(app) {
                        system_tray::focus_main_window(app);
                    }
                }

                if has_filesystem_paths {
                    let had_delegated_shell_paths = filter_result.had_delegated_paths();
                    let launch_context = build_launch_context(
                        filter_result.filtered_args,
                        Some(cwd),
                        filter_result.had_absorbed_paths,
                        had_delegated_shell_paths,
                    );
                    let _ = app.emit("app-launch-args", launch_context);
                }
            }

            #[cfg(not(windows))]
            {
                // A second launch carrying a media file is a request to *view* it, wherever it
                // came from — usually another application once sigma is the registered viewer.
                // The running session decides who shows it: the main window routes it into
                // Quick View, or a standalone viewer swaps its file.
                if let Some(media_file) =
                    standalone_viewer::media_file_from_args(&argv, Some(std::path::Path::new(&cwd)))
                {
                    let _ = app.emit(
                        "open-media-request",
                        OpenMediaRequest {
                            path: media_file.to_string_lossy().into_owned(),
                        },
                    );
                    return;
                }

                // An activation carrying nothing to open is a request for the session the user
                // can already hear, when there is one — that is the way back a backgrounded
                // Quick View owes them, and the launcher is where they will reach for it.
                // Anything naming a path belongs to the main window as before.
                if !(is_bare_activation(&argv) && show_playing_quick_view(app)) {
                    system_tray::focus_main_window(app);
                }

                let launch_context = build_launch_context(argv, Some(cwd), false, false);
                let _ = app.emit("app-launch-args", launch_context);
            }
        }))
    };

    builder
        .plugin(tauri_plugin_store::Builder::new().build())
        .plugin({
            use tauri_plugin_window_state::StateFlags;
            tauri_plugin_window_state::Builder::default()
                .with_state_flags(StateFlags::all() & !StateFlags::VISIBLE)
                // A dialog's size is a design decision, not a preference to remember: the
                // picker once ran tiled, the plugin memorized the tile's dimensions, and
                // every floating dialog after that restored as a full-height tower.
                .with_denylist(&["quick-view", "file-picker"])
                .build()
        })
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_os::init())
        .plugin(window_manager::init())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_global_shortcut::Builder::new().build())
        .plugin(tauri_plugin_system_fonts::init())
        .plugin(tauri_plugin_drag::init())
        .plugin(tauri_plugin_clipboard_manager::init())
        .plugin(
            tauri_plugin_autostart::Builder::new()
                .args([SIGMA_AUTOSTART_CLI_FLAG])
                .build(),
        )
        .invoke_handler(tauri::generate_handler![
            configure_webview_hide_pdf_more_settings,
            get_launch_context,
            standalone_viewer::standalone_launch_file,
            startup_storage_bootstrap::get_startup_storage_bootstrap,
            default_file_manager::default_file_manager_available,
            default_file_manager::is_default_file_manager,
            media_viewer_registration::media_viewer_registration_available,
            media_viewer_registration::is_default_media_viewer,
            media_viewer_registration::set_default_media_viewer,
            file_chooser_registration::file_chooser_registration_available,
            file_chooser_registration::is_default_file_chooser,
            file_chooser_registration::set_default_file_chooser,
            default_file_manager::set_default_file_manager,
            app_updater::check_for_updates,
            app_updater::download_release_installer,
            app_updater::app_updates_managed_externally,
            system_tray::reload_webview,
            system_tray::update_tray_shortcut,
            dismiss_main_window_to_background,
            set_quick_view_background_playback,
            exit_if_no_windows_left,
            file_manager1::drain_show_in_folder_requests,
            file_picker::file_picker_finish,
            file_picker::file_picker_open,
            file_picker::file_picker_request,
            set_quick_view_ownership,
            dir_reader::read_dir,
            dir_reader::read_dir_with_timeout,
            dir_reader::get_dir_entry_with_timeout,
            dir_reader::get_link_metadata_batch,
            dir_reader::get_dir_item_counts_batch,
            dir_reader::resolve_windows_directory_shortcut,
            dir_reader::get_system_drives,
            dir_reader::get_parent_dir,
            dir_reader::path_exists,
            dir_reader::path_is_regular_file,
            dir_reader::path_exists_with_timeout,
            dir_reader::paths_are_directories,
            dir_reader::path_volume_is_case_sensitive,
            dir_reader::path_comparison_volume_roots,
            dir_reader::get_mountable_devices,
            dir_reader::mount_drive,
            dir_reader::unmount_drive,
            dir_reader::disconnect_drive,
            dir_reader::mount_network_share,
            dir_size::get_dir_size,
            dir_size::get_dir_sizes_batch,
            dir_size::get_dir_size_progress,
            dir_size::get_active_calculations,
            dir_size::invalidate_dir_size_cache,
            dir_size::clear_dir_size_cache,
            dir_size::cancel_dir_size,
            file_operations::check_conflicts,
            file_operations::copy_items,
            file_operations::ensure_directory,
            file_operations::move_items,
            file_operations::rename_item,
            file_operations::delete_items,
            file_operations::create_item,
            trash_bin::trash_is_listable,
            trash_bin::trash_list,
            trash_bin::trash_sizes,
            trash_bin::trash_restore,
            trash_bin::trash_purge,
            trash_bin::trash_empty,
            link_operations::create_links,
            archive::jobs::start_archive_job,
            archive::jobs::cancel_archive_job,
            archive::encoding::check_archive,
            copy_move_job::start_copy_move_job,
            copy_move_job::cancel_copy_move_job,
            delete_job::start_delete_job,
            delete_job::cancel_delete_job,
            global_search::global_search_init,
            global_search::global_search_get_status,
            global_search::global_search_start_scan,
            global_search::global_search_cancel_scan,
            global_search::global_search_index_paths,
            global_search::global_search_query,
            global_search::global_search_query_paths,
            image_thumbnails::cache_video_thumbnail,
            image_thumbnails::generate_video_thumbnail,
            image_thumbnails::generate_image_thumbnail,
            image_thumbnails::extract_audio_cover,
            image_thumbnails::get_cached_video_thumbnail,
            media_info::media_info,
            open_with::get_associated_programs,
            open_with::open_with_program,
            open_with::open_with_default,
            open_with::open_native_open_with_dialog,
            open_with::open_native_properties,
            system_clipboard::set_system_clipboard_files,
            system_clipboard::read_system_clipboard_files,
            system_clipboard::clear_system_clipboard_files,
            system_clipboard::read_system_clipboard_image_info,
            system_clipboard::save_system_clipboard_image_to_temp,
            system_clipboard::paste_system_clipboard_image,
            system_clipboard::paste_saved_clipboard_image,
            system_clipboard::set_system_clipboard_image_from_png_bytes,
            system_clipboard::set_system_clipboard_image_from_path,
            system_clipboard::copy_video_frame_to_system_clipboard,
            input_simulation::simulate_paste_shortcut,
            system_clipboard::read_system_clipboard_text,
            system_clipboard::read_system_clipboard_change_token,
            system_clipboard::read_system_clipboard_image_png_bytes,
            clipboard_source::get_clipboard_source_context,
            clipboard_watcher::ensure_system_clipboard_watcher,
            system_icons::get_system_icon,
            terminal::get_available_terminals,
            terminal::get_terminal_icons,
            terminal::open_terminal,
            dir_watcher::watch_directory,
            dir_watcher::unwatch_directory,
            dir_watcher::get_watched_directories,
            extensions::register_extension_install_cancellation,
            extensions::cancel_extension_install_cancellation,
            extensions::clear_extension_install_cancellation,
            extensions::get_extensions_dir,
            extensions::get_extension_path,
            extensions::get_extension_storage_path,
            extensions::download_extension,
            extensions::delete_extension,
            extensions::install_local_extension,
            extensions::read_local_extension_manifest,
            extensions::get_installed_extensions,
            extensions::read_extension_manifest,
            extensions::read_extension_file,
            extensions::read_text_preview,
            extensions::read_file_binary,
            extensions::write_file_binary,
            extensions::import_extension_storage_file,
            extensions::delete_file_binary,
            extensions::is_path_within_directory,
            extensions::extension_path_exists,
            extensions::run_extension_command,
            extensions::download_extension_file,
            extensions::extension_http_request,
            extensions::start_extension_command,
            extensions::cancel_extension_command,
            extensions::cancel_all_extension_commands,
            extensions::rename_part_files_to_ts,
            extensions::get_platform_info,
            extensions::get_extension_binary_path,
            extensions::download_extension_binary,
            extensions::remove_extension_binary,
            extensions::extension_binary_exists,
            extensions::download_and_extract_extension_binary,
            extensions::get_shared_binary_path,
            extensions::download_shared_binary,
            extensions::download_and_extract_shared_binary,
            extensions::remove_shared_binary,
            extensions::shared_binary_exists,
            extensions::get_shared_binaries_base_dir,
            extensions::fetch_github_tags,
            extensions::fetch_url_text,
            background_sources::resolve_background_source_to_cache,
            background_sources::download_url_to_path,
            background_sources::copy_files_to_backgrounds,
            lan_share::start_lan_share,
            lan_share::stop_lan_share,
            lan_share::get_local_ip,
            media_server::get_media_server_origin,
        ])
        .setup(setup_handler)
        .on_window_event(|window, event| {
            if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                if window.label() == "main" {
                    // Still hidden rather than destroyed: the window is reused if the app
                    // turns out to have a reason to stay alive, and hiding it first makes it
                    // disappear immediately either way.
                    let _ = window.hide();
                    api.prevent_close();

                    // The satellites this session owns go with it — but only those. A quick
                    // view serving another application's file is that caller's viewing
                    // session, not sigma's to end; it stays up, and the visible-window rule
                    // below keeps the process alive to serve it. See `QuickViewOwnership`.
                    let quick_view_external = window
                        .app_handle()
                        .state::<QuickViewOwnership>()
                        .is_external();
                    let session_labels = session_closing_labels(quick_view_external);

                    for label in &session_labels[1..] {
                        if let Some(auxiliary) = window.app_handle().get_webview_window(label) {
                            let _ = auxiliary.hide();
                        }
                    }
                } else {
                    handle_auxiliary_window_close_requested(window, api);
                }

                let own_label = [window.label()];
                let closing_labels: &[&str] = if window.label() == "main" {
                    session_closing_labels(
                        window
                            .app_handle()
                            .state::<QuickViewOwnership>()
                            .is_external(),
                    )
                } else {
                    &own_label
                };
                exit_if_last_window_closed(window.app_handle(), closing_labels);
            }
            // The main window coming back on screen ends residency, whichever path showed it —
            // launcher relaunch, tray, launch args — since all of them focus it.
            if let tauri::WindowEvent::Focused(true) = event {
                if window.label() == "main" {
                    window
                        .app_handle()
                        .state::<BackgroundResidency>()
                        .set(false);
                }
            }
            if let tauri::WindowEvent::Destroyed = event {
                if window.label() == "main" {
                    tokio::spawn(async { lan_share::stop_lan_share().await.ok() });
                }
            }
        })
        .on_menu_event(system_tray::handle_menu_event)
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}

fn setup_handler(app: &mut tauri::App) -> Result<(), Box<dyn std::error::Error>> {
    // Seeded ahead of the standalone branches below, because every process role draws
    // file icons and each one would otherwise probe from an uninitialized directory.
    if let Ok(app_data_dir) = app.path().app_data_dir() {
        system_icons::set_icon_probe_dir(&app_data_dir);
    }

    // A picker process is one dialog answering one request: its window, its own identity, and
    // none of the app's furniture — no tray, no storage preload, no media-arg interpretation.
    if app.state::<file_picker::PickerSession>().0.is_some() {
        standalone_viewer::adopt_process_identity("sigma-file-picker");
        let picker_window =
            standalone_viewer::create_window_from_config(app.handle(), "file-picker")?;

        // This process returns before the shared devtools hook below, and the dialog suppresses
        // its context menu like every other window, so without this the picker has no way in.
        #[cfg(feature = "devtools")]
        picker_window.open_devtools();
        let _ = &picker_window;

        return Ok(());
    }

    if cfg!(debug_assertions) {
        app.handle().plugin(
            tauri_plugin_log::Builder::default()
                .level(log::LevelFilter::Info)
                .build(),
        )?;
    }

    system_tray::setup_system_tray(app.handle())?;
    startup_storage_bootstrap::migrate_legacy_user_storage_filenames(app.handle());
    #[cfg(windows)]
    if let Err(error) = default_file_manager::migrate_legacy_default_file_manager(app.handle()) {
        eprintln!("Failed to migrate legacy default file manager integration: {error}");
    }
    startup_storage_bootstrap::start_preload(
        app.handle().clone(),
        app.state::<startup_storage_bootstrap::StartupStorageBootstrapState>()
            .inner()
            .clone(),
    );

    let raw_args: Vec<String> = std::env::args().collect();

    // A media-file launch gets a standalone Quick View and no file manager; anything else gets
    // the main window. Both are `create: false` in the config, so this is the one place that
    // decides what kind of session a launch becomes. The viewer page bootstraps itself from
    // `standalone_launch_file`, and the existing nothing-visible exit rule makes closing the
    // viewer quit the app.
    let standalone_media_file =
        standalone_viewer::media_file_from_args(&raw_args, std::env::current_dir().ok().as_deref());
    let is_standalone_viewer = standalone_media_file.is_some();
    app.manage(standalone_viewer::StandaloneLaunchFile(
        standalone_media_file.map(|path| path.to_string_lossy().into_owned()),
    ));

    // Before the window exists, so its surface is stamped with the viewer's own identity.
    if is_standalone_viewer {
        standalone_viewer::adopt_process_identity("sigma-quick-view");
    }

    // FileManager1 — the interface behind every "Show in Folder" click — belongs to the
    // process that is resident and single. (The portal file-chooser role is claimed much
    // earlier, in `run`, before GTK init; see the note there.)
    #[cfg(target_os = "linux")]
    if !is_standalone_viewer {
        file_manager1::start(app.handle().clone());
    }

    let session_window = standalone_viewer::create_window_from_config(
        app.handle(),
        if is_standalone_viewer {
            "quick-view"
        } else {
            "main"
        },
    )?;

    // Web content Sigma hosts asks the webview for files directly and never the extension
    // API, so the webview is the only place those requests can be caught.
    webview_file_chooser::install_webview_file_chooser(&session_window);

    #[cfg(windows)]
    let should_hide_main_window_on_startup = {
        let filter_result = filter_shell_namespace_args(raw_args.clone());
        delegate_shell_namespace_paths(&filter_result.delegated_paths);
        url_drop::setup(app.handle());

        let launched_only_with_delegated_paths = !filter_result.had_absorbed_paths
            && filter_result.had_delegated_paths()
            && filter_result.filtered_args.len() <= 1;

        launched_from_autostart(&raw_args) || launched_only_with_delegated_paths
    };

    #[cfg(not(windows))]
    let should_hide_main_window_on_startup = launched_from_autostart(&raw_args);

    if !should_hide_main_window_on_startup {
        if let Some(main_window) = app.get_webview_window("main") {
            let _ = main_window.show();
        }
    } else {
        // An autostart that begins hidden is resident from its first breath. Without this the
        // first quick-view open-and-close would trip the nothing-visible exit and quietly end
        // a session the user set up to wait in the background.
        app.state::<BackgroundResidency>().set(true);
    }

    #[cfg(feature = "devtools")]
    {
        use tauri::Manager;
        if let Some(window) = app.get_webview_window("main") {
            window.open_devtools();
        }
    }

    Ok(())
}

#[cfg(all(test, windows))]
mod tests {
    use super::filter_shell_namespace_args;

    #[test]
    fn delegated_shell_namespace_paths_are_removed_from_launch_args() {
        let result = filter_shell_namespace_args(vec![
            "sigma-file-manager.exe".to_string(),
            "shell:downloads".to_string(),
            "C:\\Users\\aleks\\Documents".to_string(),
        ]);

        assert_eq!(
            result.filtered_args,
            vec![
                "sigma-file-manager.exe".to_string(),
                "C:\\Users\\aleks\\Documents".to_string(),
            ]
        );
        assert_eq!(result.delegated_paths, vec!["shell:downloads".to_string()]);
        assert!(!result.had_absorbed_paths);
        assert!(result.had_delegated_paths());
    }

    #[test]
    fn absorbed_shell_namespace_paths_are_tracked_without_delegation() {
        let result = filter_shell_namespace_args(vec![
            "sigma-file-manager.exe".to_string(),
            "shell:MyComputerFolder".to_string(),
        ]);

        assert_eq!(
            result.filtered_args,
            vec!["sigma-file-manager.exe".to_string()]
        );
        assert!(result.delegated_paths.is_empty());
        assert!(result.had_absorbed_paths);
        assert!(!result.had_delegated_paths());
    }
}

#[cfg(test)]
mod activation_tests {
    use super::is_bare_activation;

    fn argv(args: &[&str]) -> Vec<String> {
        args.iter().map(|arg| (*arg).to_string()).collect()
    }

    #[test]
    fn a_launch_with_no_arguments_is_bare() {
        assert!(is_bare_activation(&argv(&["sigma-file-manager"])));
    }

    /// Autostart and friends say how to start, not what to open.
    #[test]
    fn flags_leave_an_activation_bare() {
        assert!(is_bare_activation(&argv(&[
            "sigma-file-manager",
            "--autostart"
        ])));
    }

    /// The case that must not steal the launch: opening a folder is for the main window, even
    /// while something is playing in the background.
    #[test]
    fn a_path_is_not_a_bare_activation() {
        assert!(!is_bare_activation(&argv(&[
            "sigma-file-manager",
            "/home/user/Documents"
        ])));
    }
}

#[cfg(test)]
mod window_lifetime_tests {
    use super::should_exit_after_close;

    /// `(label, is_visible)` pairs as the running app would report them.
    fn windows(entries: &[(&str, bool)]) -> Vec<(String, bool)> {
        entries
            .iter()
            .map(|(label, visible)| ((*label).to_string(), *visible))
            .collect()
    }

    #[test]
    fn quits_when_the_only_window_is_closed() {
        assert!(should_exit_after_close(
            windows(&[("main", true)]),
            &["main"],
            false
        ));
    }

    /// The case that makes existence checks wrong: quick-view is prelaunched and merely
    /// hidden, so a window exists even though the user can see nothing.
    #[test]
    fn quits_when_the_only_other_window_is_a_hidden_prelaunched_one() {
        assert!(should_exit_after_close(
            windows(&[("main", true), ("quick-view", false), ("print-view", false)]),
            &["main"],
            false
        ));
    }

    /// Closing the main window closes the whole session, viewer included. The viewer was
    /// hidden a breath before this check and may still report itself visible — counting it
    /// would leave a windowless process running, which is how "sigma cannot be closed" was
    /// reported: a viewer forgotten on another workspace kept the app alive, and every
    /// launcher click resurrected the main window.
    #[test]
    fn closing_the_main_session_quits_over_a_viewer_still_reporting_visible() {
        assert!(should_exit_after_close(
            windows(&[("main", true), ("quick-view", true)]),
            &super::SESSION_WINDOW_LABELS,
            false
        ));
    }

    /// The rule itself stays symmetric: a caller closing *only* the main window leaves a
    /// visible viewer running. The session sweep is the close handler's decision, made by
    /// passing every session label, not something baked in here.
    #[test]
    fn keeps_running_when_only_main_is_closed_and_a_viewer_is_visible() {
        assert!(!should_exit_after_close(
            windows(&[("main", true), ("quick-view", true)]),
            &["main"],
            false
        ));
    }

    #[test]
    fn keeps_running_when_the_main_window_outlives_quick_view() {
        assert!(!should_exit_after_close(
            windows(&[("main", true), ("quick-view", true)]),
            &["quick-view"],
            false
        ));
    }

    /// Closing the last visible window quits even when the main window still exists hidden,
    /// which is how it is left after being closed earlier.
    #[test]
    fn quits_when_the_last_visible_window_closes_over_a_hidden_main() {
        assert!(should_exit_after_close(
            windows(&[("main", false), ("quick-view", true)]),
            &["quick-view"],
            false
        ));
    }

    #[test]
    fn counts_the_print_view_as_a_window_worth_staying_for() {
        assert!(!should_exit_after_close(
            windows(&[("main", true), ("print-view", true)]),
            &["main"],
            false
        ));
    }

    /// A registered reason outranks the window count entirely. Quick View playing on after
    /// being dismissed is the case this was written for, and now registers one.
    #[test]
    fn stays_alive_while_something_asks_it_to() {
        assert!(!should_exit_after_close(
            windows(&[("main", true)]),
            &["main"],
            true
        ));
    }

    /// The closing window is excluded by label because it has only just been hidden and may
    /// still report itself visible.
    #[test]
    fn ignores_the_closing_window_even_if_it_still_reports_visible() {
        assert!(should_exit_after_close(
            windows(&[("main", true)]),
            &["main"],
            false
        ));
    }

    /// Quick view belongs to its last caller: sigma sweeps its own content on close, but a
    /// viewer serving another application's file is left out of the sweep — and being left
    /// out means a visible one keeps the process alive to serve it.
    #[test]
    fn an_externally_owned_viewer_is_not_swept_and_keeps_the_app_alive() {
        let closing = super::session_closing_labels(true);

        assert!(!closing.contains(&"quick-view"));
        assert!(!should_exit_after_close(
            windows(&[("main", true), ("quick-view", true)]),
            closing,
            false
        ));
    }

    #[test]
    fn a_session_owned_viewer_is_swept_with_the_main_window() {
        let closing = super::session_closing_labels(false);

        assert!(closing.contains(&"quick-view"));
        assert!(should_exit_after_close(
            windows(&[("main", true), ("quick-view", true)]),
            closing,
            false
        ));
    }
}
