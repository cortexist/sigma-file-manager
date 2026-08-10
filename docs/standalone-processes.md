# Standalone process roles

Sigma presents several faces to the desktop besides the file manager itself: a media viewer,
a file-picking dialog, and the DBus services behind them. The design rule for all of them:

> Opening a viewer or a file dialog must never boot the file manager. Every auxiliary role
> runs as its own process, with its own identity, owning only the window (if any) that role
> needs.

This document records how that is achieved. All paths are relative to `src-tauri/src/`.

## The roles

| Role | Trigger | Detection | Creates | Identity (prgname / kernel comm) | Lifetime |
| --- | --- | --- | --- | --- | --- |
| Session | launcher, CLI with no special args | fallthrough | main window (+ prelaunched aux windows) | `sigma-file-manager` / `sigma-file-mana` | until WM close, or resident in background |
| Standalone viewer | CLI/MIME launch with a media file | `standalone_viewer::media_file_from_args` | quick-view window only | `sigma-quick-view` / `sigma-quick-vie` | until its window closes |
| File picker | spawned by the portal service, one per dialog | `file_picker::picker_request_from_args` (`--file-picker <json>`) | file-picker window only | `sigma-file-picker` / `sigma-file-pick` | one dialog |
| Portal service | DBus activation of `org.freedesktop.impl.portal.desktop.sigma` | `portal_file_chooser::launched_as_portal_service` (`--sigma-portal-service`) | **nothing** — no Tauri, no GTK, no windows, no webviews | `sigma-portal` / `sigma-portal` | the session (holds the bus name) |

## Role selection order in `run()` (`lib.rs`)

The order is load-bearing:

1. `GTK_USE_PORTAL=0` is forced in-process before GTK can read the environment. A portal
   backend must not consume the portal it provides; without this, any GTK dialog inside any
   sigma process would route through xdg-desktop-portal back into sigma itself.
2. **Portal service divert.** `--sigma-portal-service` enters
   `portal_file_chooser::run_service()` and never returns. Everything below — Tauri, GTK,
   plugins, windows — never happens in that process.
3. **Picker detection.** A picker process skips the single-instance plugin: two concurrent
   dialogs must be two processes, not a forward-and-exit.
4. **Viewer detection** (`media_file_from_args`): a launch argument with a media extension
   that exists on disk makes the process a standalone viewer. Extensions mirror
   `determineFileType` in `src/stores/runtime/quick-view.ts`; keep the two lists in sync.
5. Everything else is a session. Sessions also claim the portal bus name (`start()`) as a
   fallback — the claim queues behind the dedicated service's and takes over if it dies.

Then in `setup_handler`, a picker process adopts its identity, creates the `file-picker`
window from config, and returns — no tray, no storage preload, no FileManager1, none of the
session's furniture. A viewer process does the same with the `quick-view` window.

## Process identity (`standalone_viewer::adopt_process_identity`)

Two renames, because two different observers ask:

- `glib::set_prgname` — GTK stamps each Wayland surface's `app_id` from the program name.
  Launchers, window rules, and compositor config match windows by it.
- `libc::prctl(PR_SET_NAME, …)` — the kernel process name, truncated to 15 bytes:
  `sigma-file-mana`, `sigma-quick-vie`, `sigma-file-pick`, `sigma-portal`. Process monitors
  and status indicators (`pgrep`) ask for this one, and must not report the file manager
  running when only a viewer or dialog service is.

Both renames are process-wide — which is exactly why every standalone role is its own
process. A session's windows keep the session's identity.

## Window lifetime and background residency (`lib.rs`)

Auxiliary windows are prelaunched and *hidden*, not destroyed, so existence is meaningless
for lifetime decisions. The one rule (`should_exit_after_close`) is judged on **visibility**:
the app exits when the last visible window goes away, unless something registered a reason to
keep running (`BackgroundResidency` — the main window dismissed by its own close button, or
an autostart configured to begin hidden). The window manager's close on the main window ends
the session for real and never sets residency: dismiss and quit stay distinct gestures.

`QuickViewOwnership` records whether the quick-view window is serving sigma's own browsing or
another application's file. Dismissing sigma hides its own quick-view content; a viewer
serving someone else's file is not sigma's to hide, and can outlive the main window — that is
what a standalone viewer session is.

## The portal service (`portal_file_chooser.rs`)

Applications do not open file dialogs; they call `org.freedesktop.portal.FileChooser` on
xdg-desktop-portal, and x-d-p forwards to whichever backend the routing config names. Sigma
is that backend on this setup.

- **Headless by construction.** `run_service()` builds a zbus connection on the Tauri async
  runtime (available without building the app) and claims the bus name with `DoNotQueue`.
  Measured footprint: ~50 MB RSS, zero child processes, vs ~1.3 GB peak when a full hidden
  session used to serve this role.
- **Exits when it has no reason to exist.** The service subscribes to `NameLost` before
  claiming; if the claim finds an existing owner, if ownership ever moves, or if the bus
  connection dies, the process exits and the next dialog request DBus-activates a fresh one.
  A service without the name is useless, and lingering is worse than useless: on 2026-08-10
  ownership was observed moving off a live service, and the parked survivor was a 50 MB
  orphan per activation. Exit-on-loss makes any such event self-healing.
- **One picker process per dialog.** Each incoming `OpenFile`/`SaveFile` spawns
  `sigma-file-manager --file-picker <json>` and awaits its stdout; the reply *is* the
  dialog's answer (empty = cancel). Concurrency, crash isolation, and the picker's own
  desktop identity all fall out of the process boundary. The impl method staying open for
  the dialog's life is the portal protocol's own design — x-d-p calls with an infinite
  timeout.
- **Cancellation.** The backend exports an `org.freedesktop.impl.portal.Request` object at
  the caller's handle path; `Close()` SIGTERMs the picker by pid, and the vanished answer
  reads as a cancel. x-d-p also calls `Close()` itself when the requesting application drops
  off the bus.
- **The activation deadline.** xdg-desktop-portal blocks its own startup waiting for the
  backend's bus name (25-second DBus activation timeout; on failure the session simply has
  no FileChooser). The dedicated service claims the name in milliseconds since it never
  touches GTK. Sessions that claim as fallback do so at the very top of `run()`, before GTK
  init, because GTK init can itself call synchronously into the still-blocked x-d-p — the
  early claim breaks the cycle.

## DBus and portal wiring (all user-level)

Written by the in-app toggles; no packaging or root involved:

| File | Written by | Content |
| --- | --- | --- |
| `~/.local/share/xdg-desktop-portal/portals/sigma.portal` | `file_chooser_registration.rs` | declares the backend and its bus name |
| `~/.config/xdg-desktop-portal/<desktop>-portals.conf` | `file_chooser_registration.rs` | `FileChooser=sigma` preference; the file is edited, never clobbered, and desktop-specific files shadow the generic `portals.conf` |
| `~/.local/share/dbus-1/services/org.freedesktop.impl.portal.desktop.sigma.service` | `file_chooser_registration.rs` | `Exec=… --sigma-portal-service` — activation starts the headless service |
| `~/.local/share/dbus-1/services/org.freedesktop.FileManager1.service` | `default_file_manager/xdg.rs` | `Exec=… --sigma-autostart` — "Show in Folder" *needs* the frontend, so this one boots a session; requests queue in Rust (`file_manager1.rs`) and drain at frontend boot so activation cannot lose clicks |

Deployment order matters when the service flag changes: install the binary before updating
the service file — an older binary treats an unknown flag as a plain launch and would show
the main window on activation.

## Verifying a role is standalone

- `pgrep -l sigma` — expect only the comm names the scenario calls for
  (e.g. a dialog: `sigma-portal` + `sigma-file-pick`, nothing else).
- `swaymsg -t get_tree` — window `app_id`s and visibility.
- `journalctl --user -f` — DBus activation units
  (`dbus-…-org.freedesktop.impl.portal.desktop.sigma@N.service`) show starts, stops, and
  memory peaks.
- **Do not test the portal with `gdbus call`.** It disconnects as soon as `OpenFile` returns
  the request handle, x-d-p treats the vanished caller as an abandoned dialog and closes it,
  and the picker dies within milliseconds — indistinguishable from "backend never called". A
  valid test client holds its connection for the dialog's life (e.g. python3 + Gio with a
  sleep after the call).
