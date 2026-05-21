# Pi Dash Desktop

Thin Tauri v2 wrapper that loads the Pi Dash web UI in a native window. Builds
to `.AppImage` / `.deb` on Linux, `.dmg` / `.app` on macOS, `.msi` / `.exe` on
Windows.

The window simply navigates to the URL baked in at build time — there is no
desktop-specific UI code. To change behavior on desktop, edit `apps/web` (and
read `src-tauri/src/main.rs` for the small set of native concerns that live
here).

This package is **not** part of the pnpm workspace (excluded in
`pnpm-workspace.yaml`) — it's a standalone Rust crate, the same convention as
`runner/`. `pnpm build` at the repo root does not touch it.

## Build-time URL

The target URL is read from the `PI_DASH_URL` environment variable when the
binary is compiled. If unset, it defaults to `http://localhost:3000` (the
`apps/web` dev server). A bad URL fails the build (`build.rs` validates).

```bash
cd apps/desktop/src-tauri

# Dev: launches a window pointing at the local web dev server
cargo tauri dev

# Prod build pointing at your cloud instance
PI_DASH_URL=https://app.your-instance.com cargo tauri build
```

Artifacts land under `apps/desktop/src-tauri/target/release/bundle/`:

- `bundle/deb/pi-dash_<version>_amd64.deb`
- `bundle/appimage/pi-dash_<version>_amd64.AppImage`
- `bundle/dmg/pi-dash_<version>_x64.dmg` (macOS)
- `bundle/msi/pi-dash_<version>_x64_en-US.msi` (Windows)

## Running on Linux setups with GPU/driver quirks

WebKitGTK uses hardware-accelerated rendering by default. On some hosts
(VMs, remote-desktop sessions, or systems with mismatched NVIDIA drivers)
this fails with `EGL_NOT_INITIALIZED` or `failed to create dri2 screen`.
Fall back to software/compositor-less rendering:

```bash
WEBKIT_DISABLE_COMPOSITING_MODE=1 WEBKIT_DISABLE_DMABUF_RENDERER=1 \
  ./pi-dash_0.1.0_amd64.AppImage
```

## Prerequisites (Linux)

```
sudo apt install libwebkit2gtk-4.1-dev libgtk-3-dev librsvg2-dev \
                 libayatana-appindicator3-dev libssl-dev libsoup-3.0-dev \
                 patchelf build-essential
```

Rust toolchain (rustup) and `cargo-tauri` (`cargo install tauri-cli --version '^2'`).

## Regenerating icons

The icon set under `src-tauri/icons/` is generated from
`src-tauri/icons/icon.png` (a 512×512 source). The canonical master lives at
`apps/web/public/icons/icon-512x512.png` — keep both in sync when the brand
mark changes.

```
cd apps/desktop/src-tauri
cargo tauri icon icons/icon.png
```
