# Reference, not source

**Read this when:** you are about to read old frontend code, or write any code in `apps/web_new`, `packages/kit` or `packages/api-client`.

The old frontend is a **reference**: read it and run it to learn what the product does. It is never a **source**: nothing is imported from it, nothing is copied out of it.

"Old frontend" means: `@pi-dash/*` packages (`ui`, `propel`, `editor`, `utils`, `constants`, `types`, `services`, `shared-state`, `hooks`, `i18n`, …), `apps/web/**`, `apps/web/ce/**`, `apps/admin/**`, `apps/space/**`, `desktop-overlay/**`, and the cloud `private-pi-dash/ee-overlay/apps/{web,admin}/**`.

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
