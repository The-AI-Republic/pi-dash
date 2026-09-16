# Pi Dash Desktop

The Pi Dash desktop app: a Tauri v2 wrapper that bundles the Pi Dash web UI
as static assets and runs it in a native window, together with a bundled
`pidash` runner and agent engine so work can run on the user's own machine
("Pi Dash Agent"). Builds to `.AppImage` / `.deb` on Linux, `.dmg` / `.app`
on macOS, `.msi` / `.exe` on Windows.

AI Republic publishes the official, signed and auto-updating build from this
source (with its cloud edition's overrides and release configuration, kept
in its own repository). Everything needed to build and run the app yourself
is here.

## Layout

```
desktop/
  src-tauri/          the Rust app (window, deep links, updater, managed runner)
  scripts/            dev-prep.sh (bundle the SPA), prepare-agent.sh (stage the
                      runner + engine locally), bundle_agents.py (release staging),
                      merge-web-tree.sh, test-overlay.sh
  tests/              packaging and engine smoke tests
desktop-overlay/      web files that replace apps/web files in the desktop bundle
apps/web/ce/components/desktop/
                      edition seams the desktop bundle reads (sign-in card,
                      agent-runtime messages)
```

## How the frontend is built

The static SPA is produced from layers merged in order (last writer wins):

1. this repository's `apps/web/` (and the rest of the workspace);
2. any directories listed in `PIDASH_DESKTOP_EXTRA_OVERLAYS` (colon-separated)
   — how an edition layers its own overrides in; unset for the plain build;
3. `desktop-overlay/` — desktop-specific overrides (the agent runtime, the
   bare sign-in screen, routing pinned to desktop defaults).

`scripts/merge-web-tree.sh` assembles that tree in `desktop/.dev-tree/`
(gitignored); `pnpm exec turbo run build --filter=web` over it produces
`apps/web/build/client/`, which is copied into `desktop/src-tauri/dist/`. The
Tauri binary then loads from `dist/` via `WebviewUrl::App`.

Because `desktop-overlay/` is applied last, it must not contain anything that
differs by edition. Edition-specific behaviour goes through the seams in
`apps/web/ce/components/desktop/`, which `desktop-overlay` imports and never
overrides — see `desktop-overlay/README.md`.

### Two dev modes — bundled (default) vs hot-reload

**Bundled (default)** — `cargo tauri dev` runs the same code path as
release: `WebviewUrl::App` loading from `dist/`. The `beforeDevCommand`
hook automatically runs `scripts/dev-prep.sh`, which stages the bundled
runner and agent engine (`prepare-agent.sh`), merges the web tree, runs
`pnpm install` + `turbo run build --filter=web`, and copies the output into
`dist/`. First run is ~3–5 min cold (pnpm install); subsequent runs are
~10–30 s warm. Catches bundle-only bugs (`tauri://localhost` scheme quirks,
absolute base URLs, asset resolution) that hot-reload mode never exercises.

> **Sharp edge — webview origin differs between dev and release.** In
> `cargo tauri dev` the bundled SPA is served from Tauri's internal HTTP
> server at `http://127.0.0.1:<port>`, so `window.location.origin` is
> that loopback URL. In `cargo tauri build` (release) the SPA is served
> from the `tauri://` custom scheme, so `window.location.origin` is
> `tauri://localhost`. Anything that constructs a URL against
> `window.location.origin` and then passes it to a Rust command — e.g.
> `open_in_browser`, which only accepts `http`/`https` schemes — will
> _appear_ to work in dev and silently fail in release. When you need
> an absolute API URL inside the SPA, use the baked-in `API_BASE_URL`
> (from `VITE_API_BASE_URL`), not `window.location.origin`.

**Hot-reload** — `PIDASH_DESKTOP_HOT_RELOAD=1 cargo tauri dev` flips the
binary to `WebviewUrl::External(PI_DASH_URL)` and skips the prep script.
You run `pnpm dev` in `apps/web/` separately, and the window points at that
dev server (default `http://localhost:3000`, override via `PI_DASH_URL`).
HMR works; `desktop-overlay/` is **not** applied (you're seeing the plain
web UI). Use this when iterating on layout/styling.

```bash
cd desktop/src-tauri

# Bundled-dev (default) — matches release. Prep script runs automatically.
cargo tauri dev

# Hot-reload — fast iteration on apps/web. Start the dev server first:
#   (in another terminal) cd apps/web && pnpm dev
PIDASH_DESKTOP_HOT_RELOAD=1 cargo tauri dev

# Release build against your own server. Both variables are required, and
# they must share a cookie domain (see below).
PI_DASH_URL=https://pidash.example.com \
  VITE_API_BASE_URL=https://pidash.example.com \
  cargo tauri build

# Release build against a pre-built dist/. dev-prep.sh skips the merge and
# rebuild and reuses dist/ as-is; the baked API base is whatever the prior
# build used — dev-prep prints dist/bake-info.txt so you can verify.
PIDASH_SKIP_DEV_PREP=1 \
  PI_DASH_URL=https://pidash.example.com \
  VITE_API_BASE_URL=https://pidash.example.com \
  cargo tauri build
```

### Environment variables

| Var                                              | What it controls                                                                                                                                                             | Default (if unset)                                                           |
| ------------------------------------------------ | ---------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------- |
| `PI_DASH_URL`                                    | Server origin `main.rs` uses for the sign-in hand-off (`/api/auth/desktop-exchange/`) and for bouncing server-hosted pages back into the bundle. `env!`-enforced in release. | `http://localhost:3000` (hot-reload) / `http://localhost:8000` (bundled-dev) |
| `VITE_API_BASE_URL`                              | API origin baked into the SPA bundle at build time. SPA `axios` calls go here.                                                                                               | `http://localhost:8000` (bundled-dev)                                        |
| `PIDASH_DESKTOP_HOT_RELOAD`                      | `=1` flips main.rs to `WebviewUrl::External(PI_DASH_URL)`, skips dev-prep.sh.                                                                                                | unset                                                                        |
| `PIDASH_SKIP_DEV_PREP`                           | `=1` makes dev-prep.sh reuse whatever `dist/` already contains (CI that pre-populates dist/, or a hand-managed dist/).                                                       | unset                                                                        |
| `PIDASH_DESKTOP_EXTRA_OVERLAYS`                  | Colon-separated directories applied after the workspace and before `desktop-overlay/`.                                                                                       | unset                                                                        |
| `PIDASH_DESKTOP_DEV_TREE`                        | Where the merged web tree is built (dev-prep.sh, test-overlay.sh).                                                                                                           | `desktop/.dev-tree`                                                          |
| `PIDASH_DESKTOP_VERIFY_HOOK`                     | Script dev-prep.sh runs with the built client directory after its own checks.                                                                                                | unset                                                                        |
| `PIDASH_DESKTOP_EXTERNAL_SIGNIN`                 | `=1` makes the release `build.rs` require that the bundled sign-in screen hands off to the system browser (`open_in_browser`).                                               | unset                                                                        |
| `PIDASH_RUNNER_PROFILE` / `CODEX_BUNDLE_VERSION` | Cargo profile for the locally built runner / pinned agent-engine release staged by prepare-agent.sh.                                                                         | `dev` / `rust-v0.153.4`                                                      |

Set both together for non-default targets. A release build with
`VITE_API_BASE_URL` unset does not silently fall back to a default — `build.rs`
fails the build, and fails it again if the built bundle does not actually
contain the URL you passed.

**`PI_DASH_URL` and `VITE_API_BASE_URL` must share a cookie domain.** The
sign-in hand-off navigates the webview to `PI_DASH_URL` +
`/api/auth/desktop-exchange/`, so the session cookies are set on the
`PI_DASH_URL` origin, while the bundled SPA makes credentialed calls to
`VITE_API_BASE_URL`. Give them the same origin, or — if they are sibling
subdomains, as they are in AI Republic's cloud build — make sure the server
issues its session cookies on the parent domain that covers both. Otherwise the
exchange appears to succeed and every subsequent API call is unauthenticated.

### Server requirements

The bundled SPA loads from `tauri://localhost` (Linux/macOS) or
`http://tauri.localhost` (Windows; Tauri's `useHttpsScheme` is off) and makes
credentialed XHRs to `VITE_API_BASE_URL`. The server's CORS and CSRF
allowlists must include those origins (for the community server, add them to
`CORS_ALLOWED_ORIGINS`).

Signing in from the desktop requires a server that implements the desktop
sign-in hand-off: the identity provider redirects to
`pidash://auth/callback?code=…&state=…`, and `main.rs` navigates the webview
to `PI_DASH_URL/api/auth/desktop-exchange/?code=…&state=…`, which must set the
session cookies and mark the session as a desktop session (see
`apps/api/pi_dash/ee/authentication/desktop.py`). The community server does
not implement this yet, so the plain build shows a sign-in card that points
users at the web app.

The bundled agent additionally needs the server to offer a model lane for the
desktop engine (`agent_model_profile_for_user` in
`apps/api/pi_dash/ee/assistant/model_provider.py`). The community edition
serves your own API key (BYOK) to Pi Dash AI and cloud runs only, so Pi Dash
Agent reports itself unavailable there.

## Auto-update

Auto-update is opt-in per build. `tauri.conf.json` ships without a
`plugins.updater` section, and `main.rs` registers the updater plugin and
checks for updates at launch only when the (merged) config has one. A
distributor that publishes signed updates supplies it with
`cargo tauri build --config <file>.json`:

```json
{
  "bundle": { "createUpdaterArtifacts": true },
  "plugins": {
    "updater": {
      "pubkey": "<minisign public key>",
      "endpoints": ["https://updates.example.com/{{target}}/{{current_version}}/"]
    }
  }
}
```

`createUpdaterArtifacts` then requires `TAURI_SIGNING_PRIVATE_KEY` /
`TAURI_SIGNING_PRIVATE_KEY_PASSWORD` at build time (`cargo tauri signer
generate` creates a keypair). A distributor should also set its own
`identifier`: it determines the app-data directory (where the managed runner
keeps its state) and the OS install identity.

## Tests

```bash
# Rust unit tests (debug profile; the committed dist/ placeholder is enough)
(cd desktop/src-tauri && cargo test --locked)

# Release packaging helpers
python -m unittest discover -s desktop/tests -p 'test_*.py'

# Desktop web tests on the merged tree (OSS + desktop-overlay)
bash desktop/scripts/test-overlay.sh
```

## Running on Linux setups with GPU/driver quirks

WebKitGTK uses hardware-accelerated rendering by default. On some hosts
(VMs, remote-desktop sessions, or systems with mismatched NVIDIA drivers)
this fails with `EGL_NOT_INITIALIZED` or `failed to create dri2 screen`.
Fall back to software/compositor-less rendering:

```bash
WEBKIT_DISABLE_COMPOSITING_MODE=1 WEBKIT_DISABLE_DMABUF_RENDERER=1 \
  ./Pi\ Dash_0.3.0_amd64.AppImage
```

## Prerequisites (Linux build host)

```
sudo apt install libwebkit2gtk-4.1-dev libgtk-3-dev librsvg2-dev \
                 libayatana-appindicator3-dev libssl-dev libsoup-3.0-dev \
                 patchelf build-essential
```

Rust toolchain (rustup; `src-tauri/rust-toolchain.toml` pins the version),
`cargo-tauri` (`cargo install tauri-cli --version '^2'`), and the GitHub CLI
(`gh`, used by prepare-agent.sh to fetch the pinned agent engine).

## Regenerating icons

`src-tauri/icons/icon.png` (1024×1024) is the master from which Tauri's
`cargo tauri icon` derives all the platform variants. It's generated by
`src-tauri/icons/generate_icon.py` — a black squircle with the white
dot-dash-dash-dot mark from [`pi-symbol-dark.svg`](../pi-symbol-dark.svg)
centered on it.

```
cd desktop/src-tauri
python3 icons/generate_icon.py   # regenerates icon.png (1024×1024)
cargo tauri icon icons/icon.png  # regenerates 32/64/128/128@2x/.icns/.ico
```
