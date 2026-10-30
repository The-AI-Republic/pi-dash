## Summary
- builds WSET-025–028 as the self-sufficient `/settings/profile/api-tokens` leaf page, written from `parity/specs/workspace-settings.md` §§25–28
- token list with loader, illustrated empty state and single create action; create dialog with inline title validation, preset/custom/never expiry and live expiry line
- one-time secret reveal with copy confirmation and automatic CSV backup; rows with active/expired wording and confirmed deletion
- reuses the existing `PersonalApiToken` contracts from NEWFRONT-533; no contract changes
- adds an optional kit `Dialog` `closeButton` prop so dialogs can intentionally order their first action

## Rows
- WSET-025, WSET-026, WSET-027, WSET-028 — inventory status `oracle green, built`

## Validation
- `pnpm install --frozen-lockfile`
- `pnpm --filter web_new typecheck`, `lint`, `test` (651 files / 6809 tests)
- `pnpm --filter @pidash/kit typecheck`, `lint`, `test` (208 tests)
- `pnpm --filter @pidash/api-client typecheck`, `lint`, `test` (585 passed / 355 skipped live suites)
- web_new `check:boundaries`, `check:headers`, `check:deps`, `check:coexistence`
- `node apps/web_new/checks/check-similarity.mjs`
- committed-diff paths gate

Tested head: `b82910e3`.
