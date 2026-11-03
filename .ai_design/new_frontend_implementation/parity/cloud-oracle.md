# Cloud oracle — overlay pin and skew (NEWFRONT-436)

The cloud oracle (see "Cloud oracle" in `apps/web_new/e2e/parity/stack/README.md`)
builds the current checkout's old app with the private overlay copied on top.
This note records which overlay the stack was reconciled against and where the
overlay disagrees with this branch. No overlay code lives here — file lists and
SHA values only.

## Pin used

- Overlay repo: `git@github.com:The-AI-Republic/private-pi-dash.git`,
  reconciled at overlay HEAD `4a8e921` (2026-10-13).
- Overlay's pinned OSS SHA (`.oss-pin` at that HEAD): `2b46339`
  (`feat(web): project Schedulers settings tab + install/toggle/uninstall
flow (#91)`, 2026-04-30, on `main`).
- Checked out branch at reconcile time: `web-new-dev` at `6052c5c3`
  (NEWFRONT-425, 2026-10-13).

Bring-up does not enforce the pin: the stack builds whatever the
runner-local overlay checkout holds and prints its HEAD plus `.oss-pin`
for provenance. Re-run the skew check below after pulling either side.

## Skew

The overlay replaces 26 OSS files outright (19 under `apps/web`, 6 under
`apps/api/pi_dash/ee`, 1 under `packages/services`, plus i18n locale
files) and adds the rest. The `.oss-pin` is five months old, so a naive
pin-to-branch diff looks alarming — but the overlay tracks recent `main`
(the cloud production build resolves the overlay against current `main`,
not the pin), and the branch-vs-branch comparison is what matters:

- `main` vs `web-new-dev` on all 26 replaced files: identical except ONE
  file, `apps/web/core/services/auth-signout.ts` (web-new-dev refactored
  the desktop sign-out path; see below). Everything else the overlay
  shadows is byte-identical between the two branches, so the overlay
  applies to this branch exactly as it applies to `main`.

Reproduce (known-skew check — the full 26-file comparison was run at
reconcile time by diffing each replaced file between `origin/main` and
`origin/web-new-dev`; only the file below differed):

```sh
git diff --name-only origin/main origin/web-new-dev -- apps/web/core/services/auth-signout.ts
```

### Known skew: `apps/web/core/services/auth-signout.ts`

Web-new-dev simplified the OSS desktop sign-out (removed the dedicated
desktop transport branch; the app now clears its own session store and
submits the same form). The overlay replaces the whole file with the
`main`-era shape, so a cloud oracle carries the older desktop sign-out
path. Web sign-out behavior is the same either way; the cloud oracle
scenarios run on web, so this skew is accepted and recorded rather than
worked around. If a cloud desktop-sign-out row ever needs the newer
shape, the fix belongs in the overlay repo (rebase the override onto the
new OSS file), not in the parity stack.

## What the oracle bakes in

- Anonymous throttle widened to 600/minute via
  `stack/parity_cloud_scratch_settings.py` (mounted read-only into
  `api-cloud`), mirroring the OSS stack's `parity_scratch_settings.py`.
  The cloud settings inherit the 30/minute production `anon` budget
  unchanged, which saturates minutes into an oracle run; the
  authentication throttle is untouched, so AUTH-006 still trips it.
- OpenHub app lane ON in both halves (`OPENHUB_APPS_ENABLED=1` on the
  API, `VITE_OPENHUB_APPS_ENABLED=1` in the oracle bundle). The flag-off
  shape (CLOUD-021's second half: sidebar byte-identical to OSS, route
  404s) is a documented follow-up: it needs a compose override file plus
  a build-arg-driven oracle rebuild, and the owning scenario issue
  (NEWFRONT-439) may instead prove it against the OSS oracle's sidebar.
- Cloud API without a cloud worker: nothing in the CLOUD-\* rows needs
  background fan-out (mentions, notifications, exports all belong to
  other areas).
- Production oracle only: `PARITY_ORACLE_MODE=dev` with the cloud
  edition refuses. The dev-mode cloud oracle is a follow-up if a
  scenario author needs StrictMode/unminified behavior.
- `OPENHUB_LLM_ENABLED` stays off (deployed default). The assistant
  connections endpoint responds regardless; only gateway-routed turns
  need the lane.
