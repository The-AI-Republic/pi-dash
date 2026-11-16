# Reference, not source

**Read this when:** you are about to read old frontend code, or write any code in `apps/web_new`, `packages/kit` or `packages/api-client`.

The old frontend is a **reference**: read it and run it to learn what the product does. It is never a **source**: nothing is imported from it, nothing is copied out of it.

"Old frontend" means (with the exception of your own code, below): `@pi-dash/*` packages (`ui`, `propel`, `editor`, `utils`, `constants`, `types`, `services`, `shared-state`, `hooks`, `i18n`, …), `apps/web/**`, `apps/web/ce/**`, `apps/admin/**`, `apps/space/**`, `desktop-overlay/**`, and the cloud `private-pi-dash/ee-overlay/apps/{web,admin}/**`.

## Exception: your own code (porting allowed)

These paths were written by AI Republic after the Plane import (verified in git history: added after the initial commit of 2026-04-17, AI Republic commits only). The copying ban does **not** apply to them: you may port their logic into `apps/web_new` and adapt it. For these paths, read → spec → write is optional.

- `apps/web/core/components/{runners,chat,assistant,schedulers}/**`, `apps/web/core/components/agent-runtime.tsx`, `apps/web/core/components/desktop-update-button.tsx`
- routes under `apps/web/app/(all)/[workspaceSlug]/`: `runners/**`, `schedulers/**`, `prompts/**`, `assistant/**`, `ai-dev-machines/**`, `(projects)/projects/(detail)/[projectId]/{runners,schedulers}/**`, `(settings)/settings/projects/[projectId]/schedulers/**`
- `apps/web/core/store/{scheduler,prompt-section}.store.ts`
- `apps/web/core/services/{runner/**,agent-runtime.ts,desktop-session.ts}`
- `packages/services/src/{runner,assistant,scheduler,prompt-section,auto-pm}/**`, `packages/services/src/{desktop-api-adapter,desktop-event-source}.ts`
- `desktop-overlay/**`

Still required for ported code:

1. **No runtime imports** from these paths. Move the code into the new trees; the old apps will be deleted.
2. **Adapt it to the new architecture:** kit instead of `@pi-dash/ui`/`propel`, TanStack Query and contracts instead of MobX stores and axios services, `core/platform` for Tauri calls.
3. **Anything these files use from Plane code stays reference-only:** imports from `@pi-dash/*` packages, Plane stores, helpers and components. Rewrite those parts. The similarity check still compares your code against Plane's.
4. **Parity is unchanged:** inventory rows, oracle scenarios and area gates apply as everywhere else.
5. Ported files get the new license header.

Everything not listed here, including every file present in the initial import (for example `core/components/automation` and parts of `ce/components/desktop`), is treated as Plane code. If you believe another path is AI Republic's own, leave a `process:` comment on NEWFRONT-1; do not treat it as own code until this list is updated.

## Not allowed

1. Importing anything from the old frontend: components, hooks, stores, services, utils, constants, types, styles.
2. Copying code, even a snippet you then edit.
3. Mechanical translation: rewriting an old file into the new stack while keeping its structure, names and control flow.
4. Copying Tailwind class strings, CSS, i18n keys or copy text, SVG icons, images or other assets.

## Allowed

1. Reading old code to learn behavior: what a screen does, validation rules, edge cases, permission checks, empty and error states, keyboard shortcuts, and which API calls it makes with which parameters.
2. Running `apps/web` and observing it.
3. Writing what you learned **in prose** in the area spec and inventory.

## Read → spec → write

Every implementation issue follows these steps:

1. **Read** the relevant old code and run the old screen if needed.
2. **Spec:** update `parity/specs/<area>.md` with the behavior in your own words, citing inventory IDs. No code in specs, except API paths and field names.
3. **Write** the new code from the spec, the wiki pages and the kit. Do not have old files open while writing new ones.

API shapes: if you learn an endpoint's shape from old code, write it as a contract in `@pidash/api-client` (see *Data layer*), not as ported code.

## Enforcement (CI; never weaken, skip or work around)

- **Import ban lint:** any import from the old frontend fails.
- **Similarity check:** a token-based duplicate detector compares the new trees with the old frontend. A duplicated block of ≥ 50 tokens fails the build. The threshold is set during Phase 0; see *Decisions and open questions*.
- **License header check:** every new file carries the new header; no new file carries the old `SPDX-License-Identifier: AGPL-3.0-only` header.
- **Review:** the reviewer compares the diff with the old files the spec cites and rejects anything that looks translated rather than written.

If the similarity check flags code you wrote independently (boilerplate, imports), do not rewrite around it blindly: leave a `process:` comment on NEWFRONT-1 so the threshold or ignore list can be adjusted.

## Why

The product is moving off AGPL. Code derived from Plane's frontend would carry AGPL with it. Reading for behavior while writing independently, with a spec step and a similarity check in between, is how we keep the new code ours.
