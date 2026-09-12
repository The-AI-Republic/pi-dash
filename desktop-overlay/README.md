# `desktop-overlay/` — desktop overrides at matching `apps/web` paths

Files here are copied **on top of** the corresponding paths in the web
workspace when the desktop app's frontend is built
(`desktop/scripts/merge-web-tree.sh`, used by `dev-prep.sh`). The bundled
desktop frontend is:

```
apps/web (this repo)  →  edition overlays, if any  →  desktop-overlay/
                         (PIDASH_DESKTOP_EXTRA_OVERLAYS)
```

The plain build has no edition overlay. An edition (for example AI
Republic's cloud build, which applies its private `ee-overlay/`) slots its
overrides in the middle. `desktop-overlay/` always wins.

## Why this layer exists

The desktop app bundles the web UI as static assets inside a Tauri webview.
Most screens stay unchanged, but the desktop needs things the web app must
not ship: invoking Tauri commands (the bundled agent runtime, opening the
system browser), a bare sign-in screen instead of whatever the web build
shows at `/`, and routing without web-only pages. Putting that into
`apps/web` itself would ship Tauri-only code paths to every web build.

## The rule: nothing edition-specific here

Because this layer is applied **after** every edition overlay, a file here
silently replaces the edition's version. So:

- **Allowed:** desktop behaviour that is the same for every edition — the
  agent runtime, the desktop sign-in page frame, and routing files pinned to
  desktop defaults (`app/routes/extended.ts` empty, `react-router.config.ts`
  plain SPA, `/login` and `/sign-in` rendering the desktop sign-in page).
  These match the community defaults, and on an edition they drop the
  edition's web-only routes from the desktop bundle.
- **Not allowed:** anything that differs by edition. Route it through a seam
  in `apps/web/ce/components/desktop/` instead: import the seam from here,
  never override it here, and let editions replace it.

Current seams:

| Seam (`@/pi-dash-web/components/desktop/…`) | Used by | Community default |
|---|---|---|
| `sign-in-card.tsx` — `DesktopSignInCard` | `app/(home)/page.tsx` | Explains that desktop sign-in isn't available for this server yet and opens the web app |
| `agent-runtime-edition.ts` — `AGENT_RUNTIME_REASON_MESSAGES`, `CSRF_TOKEN_PATH` | `core/services/agent-runtime.ts` | Generic messages for managed-runner reason codes; `/auth/get-csrf-token/` |

## Path convention

Every file in `desktop-overlay/<path>` overrides the workspace file at
`<path>`:

```
desktop-overlay/apps/web/core/services/agent-runtime.ts
                                     │
                                     ▼  (merge-web-tree.sh, after any edition overlay)
<tree>/apps/web/core/services/agent-runtime.ts
                                     │
                                     ▼  (turbo run build --filter=web)
desktop/src-tauri/dist/
```

## What's here today

- `apps/web/app/(home)/page.tsx` — `/` renders the bare sign-in screen
  (signed-out) or forwards to the workspace (signed-in), with any `?error=`
  from a failed sign-in shown above the card.
- `apps/web/app/routes/redirects/core/login.tsx`, `sign-in.tsx` — `/login`
  (where sign-out lands) and `/sign-in` (where a failed sign-in hand-off
  lands, carrying `?error=`) render that same screen.
- `apps/web/app/routes/extended.ts` — empty: no web-only routes in the
  desktop bundle. Desktop-only routes go here.
- `apps/web/react-router.config.ts` — `ssr: false`, no prerendering.
- `apps/web/core/services/agent-runtime.ts`, `core/components/agent-runtime.tsx`
  — the real implementations of the `apps/web` agent-runtime stubs: enroll
  this machine, write the engine config and model credential, supervise the
  bundled daemon.
- `apps/web/tests/desktop/` — tests for the above; run them with
  `bash desktop/scripts/test-overlay.sh`.

## What does NOT go here

- Rust code — that's `desktop/src-tauri/`. New Tauri commands go there and
  are exposed via `invoke_handler!`; the web side that calls them goes here.
- Backend changes — a desktop endpoint belongs in `apps/api` with an `ee/`
  seam if editions need to change it.
