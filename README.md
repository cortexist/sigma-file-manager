# Sigma File Manager — Linux / Wayland fork

A fork of [aleksey-hoffman/sigma-file-manager](https://github.com/aleksey-hoffman/sigma-file-manager),
originally created and maintained by [Aleksey Hoffman](https://github.com/aleksey-hoffman).

**This fork is developed and tested exclusively on Linux, under Wayland with Sway, and is
coded for that environment.** Fixes and features here are written against that stack first;
X11, Windows and macOS paths are inherited from upstream and are not exercised. The fork has
diverged far enough that it is not intended to be merged back.

For the feature list, screenshots and community links, see the upstream README.

## Requirements

- **Rust** — stable toolchain
- **Node** — 24.16.0 or newer
- System libraries, below

### Arch

```bash
sudo pacman -S --needed base-devel rust nodejs npm \
  webkit2gtk-4.1 gtk3 librsvg libayatana-appindicator \
  gstreamer gst-plugins-base gst-plugins-good gst-libav
```

GStreamer is needed at build *and* run time: video thumbnails and embedded audio cover art go
through it. `gst-plugins-good` and `gst-libav` supply the decoders for common formats — without
them the app still builds and runs, but video thumbnails fail.

Optional:

```bash
sudo pacman -S patchelf      # only to build AppImage/deb bundles
sudo pacman -S wl-clipboard  # only to run the clipboard tests
```

### Debian / Ubuntu

```bash
sudo apt install -y \
  libwebkit2gtk-4.1-dev libjavascriptcoregtk-4.1-dev \
  libayatana-appindicator3-dev librsvg2-dev patchelf \
  libgstreamer1.0-dev libgstreamer-plugins-base1.0-dev \
  gstreamer1.0-plugins-good gstreamer1.0-libav
```

On Ubuntu 24.04, pin WebKitGTK to `2.44.0-2` — later builds throw `EGL_BAD_PARAMETER` at
runtime. See `.github/actions/install-linux-tauri-deps` for the exact pinned set CI uses.

## Setup

```bash
npm install
```

## Development build

```bash
npm run tauri:dev
```

Starts Vite and opens the app with hot reload. If WebKit renders a blank or black window on
your GPU, try:

```bash
npm run tauri:dev:webkit-igpu
```

## Release build

```bash
npx tauri build --no-bundle
```

Builds the frontend and the optimised binary, and skips packaging. The result is:

```
src-tauri/target/release/sigma-file-manager
```

Install it wherever you keep local binaries:

```bash
install -Dm755 src-tauri/target/release/sigma-file-manager ~/.local/bin/sigma-file-manager
```

### Rebuilding only the Rust side

When the frontend has not changed, this is much faster:

```bash
cargo build --release --features tauri/custom-protocol --manifest-path src-tauri/Cargo.toml
```

**`--features tauri/custom-protocol` is required.** Without it Tauri builds the binary in dev
mode: it starts, but loads the frontend from `http://localhost:1420` instead of its embedded
assets, and the window shows `Could not connect to localhost: Connection refused`. The `tauri`
CLI passes this feature itself, which is why it only has to be given when calling `cargo`
directly.

## Bundles (AppImage / deb)

Only needed to produce distributables. **Requires `patchelf`** — `linuxdeploy`'s GStreamer
plugin shells out to it, and without it the bundle fails.

```bash
npm run tauri:build:linux
```

**Use this exact script — not `npm run tauri:build` or `npx tauri build`.** The `:linux`
variant sets `NO_STRIP=true`, and that is load-bearing: `linuxdeploy` ships its own old
`strip`, which cannot parse the `.relr.dyn` sections in modern Arch libraries and fails on
every library it touches (`unknown type [0x13] section '.relr.dyn'`). `NO_STRIP` skips
stripping entirely.

Produces both, under `src-tauri/target/release/bundle/`:

```
appimage/Sigma File Manager_2.2.0_amd64.AppImage
deb/Sigma File Manager_2.2.0_amd64.deb
```

No other setup is needed. In particular `APPIMAGE_EXTRACT_AND_RUN=1` is *not* required — Tauri
already passes `--appimage-extract-and-run` to `linuxdeploy`, so its own libfuse2 dependency
never comes into play — and the output directory does not need clearing between runs, because
each run replaces it. For that reason, do not run two builds at once: the second wipes the
first one's AppImage.

Tauri reports every bundling failure as the same generic `failed to run linuxdeploy`,
regardless of cause. **Re-run with `--verbose`** to see the real error — it is the only
practical way to diagnose this step.

## Tests and checks

```bash
npm run test:unit:run                                   # frontend unit tests
cargo test --manifest-path src-tauri/Cargo.toml --lib   # Rust tests
npm run check                                           # types + lint + unit tests
```

The clipboard tests in `src-tauri/src/system_clipboard/files.rs` talk to the real compositor.
They skip themselves unless `WAYLAND_DISPLAY` is set, and they need `wl-clipboard`. While they
run, they take over the system clipboard.

## Linux notes

- **Cut / copy with other file managers** works on Wayland: the clipboard carries
  `x-special/gnome-copied-files` and KDE's `application/x-kde-cutselection` alongside
  `text/uri-list`. On X11 the fallback offers `text/uri-list` only, so a cut made in another
  application is read as a copy.
- **The app quits when its last visible window closes.** The main window and Quick View can
  each outlive the other; only the absence of both ends the process. A single hook,
  `should_keep_running_without_windows` in `src-tauri/src/lib.rs`, is where anything that
  should outlive the windows would register.
- **Debugging a blank window:** run with `WEBKIT_DISABLE_DMABUF_RENDERER=1`. On AMD + Wayland,
  WebKit sometimes fails to composite its own error page, so a real error can show as plain
  white.

## Licence

GNU GPLv3, unchanged from upstream. See [LICENSE.md](LICENSE.md).
