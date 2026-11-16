# Pi Dash Desktop

The Pi Dash desktop app: a Tauri v2 wrapper that bundles the Pi Dash web UI
as static assets and runs it in a native window. Work on the user's own
machine runs on that computer's Pi Dash daemon — the `pidash` CLI, run as a
user service, with the built-in engine it ships — and the app is a client of
that daemon over its local socket. It carries a copy of the CLI so it can set
the daemon up when there is none. Builds to `.AppImage` / `.deb` on Linux,
`.dmg` / `.app` on macOS, `.msi` / `.exe` on Windows.

### The app and the computer's daemon

(`.ai_design/built_engine_as_runner/design.md` §4, §10, §12, §13.)

- **Discovery.** The app connects to the daemon's standard control socket
  (the path an override-less `pidash` resolves) and sends `Hello`
  (`src-tauri/src/daemon.rs`). No answer means a daemon too old for the app,
  which offers "Update Pi Dash on this computer" (never silently, and not for
  an install it cannot write); a newer protocol asks the user to update the
  app; a daemon signed in to another account or cloud is refused, with offers
  to switch either side. The app never starts a second daemon, and never
  starts, stops or re-roots the one there is.
- **Set-up.** With no daemon, the app asks before setting one up (the
  service is required; declining sets nothing up): it installs the CLI it
  carries into the official location (the network installer on Windows),
  signs it in with a device grant it approves with its own session, runs
  `pidash install` with the user's login-shell `PATH` and `pidash start`, and
  `pidash engine install` when the engine is missing (`src-tauri/src/setup.rs`).
- **Sign-in.** The app renews the engine credential silently only when the
  daemon reports it `expiring` or `expired`; `never` gets "Turn on the
  built-in engine", `signed_out` only an explicit "Sign in the built-in
  engine". Signing out of the app signs out only the app, unless the user
  ticks "Also sign out Pi Dash on this computer" (`pidash auth logout`).
- **Chat.** "This computer" chats go to the daemon over the socket; every
  other runner's chat goes through the server.
- **Migration.** Builds before this ran a private daemon per workspace under
  the app's data. On launch the app merges each such legacy machine into the
  daemon's, re-adds its engine runners with their folders, moves the
  account's engine home, and deletes the old config and credentials, keeping
  task folders (`src-tauri/src/migration.rs`). Retried on each launch until it
  can run. Afterwards it shows one-time notices, kept in the app's per-account
  settings (`src-tauri/src/account_settings.rs`) until dismissed: legacy task
  folders left unused because the daemon already ran the engine for that
  project, and projects whose engine became a pod member on upgrade, each with
  a switch to turn pod work off (§13, PO-11).
- **Replaced machine.** When the daemon signed in as a new machine (its data
  was wiped) and the user has an offline machine with the same hostname and
  no live session, the app asks "Is this <label>, last seen <date>? Replace
  it". Confirming merges the old machine into this one and re-adds its engine
  runners to the daemon; declining leaves it, and the app does not ask about
  it again (§4.10).

AI Republic publishes the official, signed and auto-updating build from this
source (with its cloud edition's overrides and release configuration, kept
in its own repository). Everything needed to build and run the app yourself
is here.

## Layout

```
desktop/
  src-tauri/          the Rust app (window, deep links, updater, daemon client)
  scripts/            dev-prep.sh (bundle the SPA), prepare-agent.sh (stage the
                      bundled CLI locally), bundle_agents.py (release staging)
  tests/              packaging tests
apps/web_new/         the frontend this app ships (built for the desktop target)
```

## How the frontend is built

The desktop app ships one frontend: `apps/web_new`, built with
`PIDASH_TARGET=desktop`. Platform differences live behind
`apps/web_new/src/core/platform` and are selected at build time, so there is
no file swapping and no overlaid tree.

`scripts/dev-prep.sh` runs before every `cargo tauri dev` and `cargo tauri
build` (from `tauri.conf.json`'s `beforeDevCommand` / `beforeBuildCommand`).
It stages the bundled CLI (`prepare-agent.sh`), runs
`pnpm install --frozen-lockfile`, builds `@pidash/kit` (the app imports its
built stylesheet, which the source alias cannot stand in for) and
`@pidash/api-client`, then runs
`pnpm --filter web_new build:desktop` with `VITE_API_BASE_URL` and
`VITE_WEB_BASE_URL` exported. The result in `apps/web_new/dist/desktop/` is
copied into `desktop/src-tauri/dist/`, which the Tauri binary loads via
`WebviewUrl::App`.

Before the swap, the script refuses a bundle that does not look like a
desktop build of `apps/web_new`: the shell must reference `assets/`, the
asset directory must be non-empty, the desktop platform marker
(`desktop_api_request`) must be present, and both baked origins must appear.
`PIDASH_DESKTOP_VERIFY_HOOK` then gets the last word — it runs with the built
client directory and can reject it.

`desktop/src-tauri/build.rs` repeats the origin checks for release builds and
fails `cargo` early with the same context.

### Screens the app does not carry

The bundle carries every screen `apps/web_new` owns. An address it does not
own (the admin console, a public board, a screen the hosted app still serves
from the old frontend) cannot render correctly inside the app window — the
native shell refuses to load a server-hosted page remotely. When routing
lands on such an address, the window hands it to the system browser at the
same path on the hosted web app (`VITE_WEB_BASE_URL`) and explains what
happened; it never claims the page does not exist. Addresses the bundle does
own keep working in the window, including ones that legitimately resolve to
nothing (an unknown workspace), which keep the normal not-found page.

The decision reads `apps/web_new/migrated-routes.json`, so a screen starts
rendering in the window as soon as its route joins that list.

### Two dev modes — bundled (default) vs hot-reload

**Bundled (default)** — `cargo tauri dev` runs the same code path as
release: `WebviewUrl::App` loading from `dist/`. The `beforeDevCommand`
hook automatically runs `scripts/dev-prep.sh`, which stages the bundled
CLI (`prepare-agent.sh`), runs `pnpm install`, builds `@pidash/kit`,
`@pidash/api-client` and `apps/web_new` for the desktop target, and copies
the output into `dist/`. First run is a few minutes cold (pnpm install, kit
build); subsequent runs are much faster. Catches bundle-only bugs (`tauri://localhost` scheme quirks,
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
> (from `VITE_API_BASE_URL`), not `window.location.origin`. Shareable links
> and the browser hand-off use the baked web origin the same way.

**Hot-reload** — `PIDASH_DESKTOP_HOT_RELOAD=1 cargo tauri dev` flips the
binary to `WebviewUrl::External(PI_DASH_URL)` and skips the prep script.
You run the `apps/web_new` dev server separately
(`pnpm --filter web_new dev`, port 3010), and the window points at that dev
server (override via `PI_DASH_URL`). HMR works for layout and styling work.
Two things differ from the bundled build: the dev server has no web origin
baked in, so shareable links and the browser hand-off cannot resolve an
address; and it is the web build, so desktop-only screens behave as they do
on the web. Use the bundled mode to check desktop behaviour.

```bash
cd desktop/src-tauri

# Bundled-dev (default) — matches release. Prep script runs automatically.
cargo tauri dev

# Hot-reload — fast iteration on apps/web_new. Start the dev server first:
#   (in another terminal) pnpm --filter web_new dev
export PIDASH_DESKTOP_HOT_RELOAD=1
cargo tauri dev

# Release build against your own server. Both variables are required, and
# they must share a cookie domain (see below).
PI_DASH_URL=https://pidash.example.com \
  VITE_API_BASE_URL=https://pidash.example.com \
  cargo tauri build

# Release build against a pre-built dist/. dev-prep.sh skips the build and
# reuses dist/ as-is; the baked origins are whatever the prior build used —
# dev-prep prints dist/bake-info.txt so you can verify.
PIDASH_SKIP_DEV_PREP=1 \
  PI_DASH_URL=https://pidash.example.com \
  VITE_API_BASE_URL=https://pidash.example.com \
  cargo tauri build
```

### Environment variables

| Var                              | What it controls                                                                                                                                                             | Default (if unset)                                                           |
| -------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------- |
| `PI_DASH_URL`                    | Server origin `main.rs` uses for the sign-in hand-off (`/api/auth/desktop-exchange/`) and for bouncing server-hosted pages back into the bundle. `env!`-enforced in release. | `http://localhost:3000` (hot-reload) / `http://localhost:8000` (bundled-dev) |
| `VITE_API_BASE_URL`              | API origin baked into the SPA bundle at build time. The SPA reads it as its API origin.                                                                                      | `http://localhost:8000` (bundled-dev)                                        |
| `VITE_WEB_BASE_URL`              | Public web origin baked into the SPA: shareable links and the browser hand-off resolve against it. Release builds refuse to ship without one.                                | `PI_DASH_URL`, else `http://localhost:8000`                                  |
| `PIDASH_DESKTOP_WEB`             | Retired frontend selector. Unset or `web_new` builds `apps/web_new`; any other value fails the prep instead of silently building web_new.                                    | unset (web_new)                                                              |
| `PIDASH_DESKTOP_HOT_RELOAD`      | `=1` flips main.rs to `WebviewUrl::External(PI_DASH_URL)`, skips dev-prep.sh.                                                                                                | unset                                                                        |
| `PIDASH_SKIP_DEV_PREP`           | `=1` makes dev-prep.sh reuse whatever `dist/` already contains (CI that pre-populates dist/, or a hand-managed dist/).                                                       | unset                                                                        |
| `PIDASH_DESKTOP_VERIFY_HOOK`     | Script dev-prep.sh runs with the built client directory after its own checks.                                                                                                | unset                                                                        |
| `PIDASH_DESKTOP_EXTERNAL_SIGNIN` | `=1` makes the release `build.rs` require that the bundled sign-in screen hands off to the system browser (`open_in_browser`).                                               | unset                                                                        |
| `PIDASH_RUNNER_PROFILE`          | Cargo profile for the locally built runner staged by prepare-agent.sh.                                                                                                       | `release` in a release `cargo tauri build`, else `dev`                       |

Set `VITE_API_BASE_URL` and `VITE_WEB_BASE_URL` together for non-default
targets. A release build with either unset does not silently fall back to a
default — `dev-prep.sh` refuses a bundle that does not contain the values,
and `build.rs` fails the build too.

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
`http://tauri.localhost` (Windows; Tauri's `useHttpsScheme` is off). These are
production asset origins. Cross-site XHRs can omit the API's cookies even
when the login navigation stored them successfully and CORS is configured.

The API services use `getDesktopApiAdapter()` to send requests through
`desktop_api_request` in Rust. It shares the webview's persistent cookie
store with the login navigation, applies cookie domain/path/expiry rules,
and writes refreshed cookies back. HttpOnly cookie values stay in native
code. The command accepts only the main app UI and `/api/` or `/auth/` URLs
on the compiled `VITE_API_BASE_URL` origin, including redirects. Sign-out
clears the store and prevents pending requests from restoring old cookies.
Bodies cross IPC as raw bytes (a length-prefixed JSON head, then the body).
Canceling or timing out a request aborts it natively; like Axios, requests
have no deadline unless `timeout` is set.

Credentialed Server-Sent Events have the same cookie problem, so chat
streams open through `createApiEventSource()`, which uses
`desktop_api_stream` on desktop and a plain `EventSource` elsewhere. Use it
for any new credentialed API stream.

Edition code that makes standalone Axios requests, such as token refresh,
must also pass `adapter: getDesktopApiAdapter()`. Browser builds and older
desktop binaries keep the default adapter. External storage uploads stay
on the browser adapter so upload progress and cancellation keep working.

Native requests send the app's actual `Origin`; the server's CSRF allowlist
must still permit it. Keep the CORS allowlist for older desktop clients and
any browser requests (for the community server, configure
`CORS_ALLOWED_ORIGINS`). Changing `SameSite` alone is not a reliable fix
for platform-specific third-party cookie policies.

On a self-hosted HTTPS server, listing `http://tauri.localhost` in
`CORS_ALLOWED_ORIGINS` does not disable `Secure` on the session/CSRF cookies.
Both the Django settings (`apps/api/pi_dash/settings/common.py`) and the
Rust settings (`rust-api/crates/db/src/config/settings.rs`) treat the desktop
bundle origins (`tauri://localhost`, `http://tauri.localhost`,
`https://tauri.localhost`) as local webview asset origins, not plain-HTTP
deployments, so an HTTPS web origin plus the desktop origin still yields
`SESSION_COOKIE_SECURE` / `CSRF_COOKIE_SECURE` / `cookie_secure` = `True` (pinned
by `rust-api/contract-tests/foundation/test_cookie_secure_desktop.py`).

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
`identifier`: it determines the app-data directory (where the app keeps its
local chat history and settings, and where older builds kept their private
daemon trees, which the migration reads) and the OS install identity.

### Manifest entries each install type needs

The updater plugin resolves the install-type-specific entry first
(`linux-x86_64-deb`, `windows-x86_64-msi`, …) and falls back to the generic
`{os}-{arch}` entry. The client treats a static manifest that carries no
entry of an install's own type as "nothing to update" for that install: a
deb install is never handed the AppImage payload (it cannot install it),
and an MSI install is never handed the NSIS installer (it would silently
convert the install). AppImage and NSIS installs may resolve through the
generic entries. A distributor that wants every install type to auto-update
must therefore publish the specific entries too:

| Install type   | Specific entry the client requires                          |
| -------------- | ----------------------------------------------------------- |
| Linux `.deb`   | `linux-x86_64-deb` (and per-arch)                           |
| Linux AppImage | `linux-x86_64-appimage` (generic `linux-x86_64` also works) |
| Windows MSI    | `windows-x86_64-msi` (and per-arch)                         |
| Windows NSIS   | `windows-x86_64-nsis` (generic `windows-x86_64` also works) |

Entry names are the plugin's `{os}-{arch}-{installer}` form; `arch` here is
`x86_64` (see `updates.rs` for the full mapping).

## Tests

```bash
# Rust unit tests (debug profile; the committed dist/ placeholder is enough)
(cd desktop/src-tauri && cargo test --locked)

# Release packaging helpers, dev-prep.sh's default path and its guards
python3 -m unittest discover -s desktop/tests -p 'test_*.py'

# The frontend this app ships, with its desktop tests
pnpm --filter web_new test
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
`cargo-tauri` (`cargo install tauri-cli --version '^2'`).

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
