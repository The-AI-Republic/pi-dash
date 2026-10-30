### Acceptance Criteria
- The token page at `/settings/profile/api-tokens` lists the signed-in user's own tokens, shows a loader until the list resolves, and offers a create control beside the heading.
- With no tokens, an illustrated empty state titled `No Personal token yet` offers `Add API token`, and that action opens the same create dialog.
- Creating a token rejects an empty or blank title inline with `Title is required`, rejects a 256-character title with `Title should be less than 255 characters`, and accepts a free-text description.
- Expiry offers `1 week`, `1 month`, `3 months`, `1 year`, `Custom` in that order; the custom calendar disables today and enables tomorrow; never-expires disables the picker and hides the preview; a picked expiry shows a live `Expires ... at HH:MM` line.
- Submitting with a title but no expiry while never-expires is off shows `Please select an expiration date` and keeps the dialog open.
- A successful create sends `POST /api/users/api-tokens/` with the trimmed label, description and the computed ISO expiry (or null for never-expires), reveals the raw `pi_dash_api_...` secret once under `Key created`, copies it with a `Token copied to clipboard` notice, and downloads `secret-key-<timestamp>.csv` with header `Title,Description,Expiry,Secret key`.
- Closing the reveal discards the secret from the page; only metadata stays listed and the list endpoint never returns it.
- Each row shows the label at heading level 5, an `Active` or `Expired` marker, the description when non-blank, and either `Expires <Mon D, YYYY> at HH:MM`, `Never expires`, or `Expired <relative> ago`.
- Deleting opens a `Delete personal access token` confirmation that says the action cannot be undone; Cancel keeps the row, Delete sends `DELETE /api/users/api-tokens/<id>` and shows `Personal access token successfully deleted`; a failed delete keeps the row and shows an error notice.

### How to Test
- Kind: AUTOMATED
- Setup: branch `pi-dash/newfront-544` at `b82910e3`; repo root; `pnpm install --frozen-lockfile`.
- Steps:
  - `pnpm --filter web_new typecheck`
  - `pnpm --filter web_new lint`
  - `pnpm --filter web_new test`
  - `pnpm --filter @pidash/kit typecheck && pnpm --filter @pidash/kit lint && pnpm --filter @pidash/kit test`
  - `pnpm --filter @pidash/api-client typecheck && pnpm --filter @pidash/api-client lint && pnpm --filter @pidash/api-client test`
  - `pnpm --filter web_new check:boundaries && pnpm --filter web_new check:headers && pnpm --filter web_new check:deps && pnpm --filter web_new check:coexistence`
  - `node apps/web_new/checks/check-similarity.mjs`
  - paths gate from the committed diff: fetch `web-new-dev`, export `HEAD_REF=pi-dash/newfront-544`, run its `newfront-check-paths.mjs`
  - Focused token proof: `pnpm --filter web_new exec vitest run src/features/workspace-settings/tokens src/routes/settings/profile/api-tokens.test.tsx`
- Expected: every command exits 0. The focused run reports 55 tests across model, API hooks, download, expiry picker, page/rows/delete, create/reveal, and route gate.
- Already validated here: all commands above on `b82910e3`; full `web_new` suite 651 files / 6809 tests passed, kit 208 passed, api-client 585 passed / 355 live-suite skipped, similarity and paths gates passed.
- Not covered: parity stack / browser scenarios were not run (build slices never do). The web-new parity driver's `wsSettingsApiToken*` methods are still TODO stubs; the workspace-settings area gate (NEWFRONT-550) must wire those methods before it runs the area suite. `@vitest/coverage-v8` is not installed in this checkout, so changed-line/branch coverage was audited manually against the test files.
