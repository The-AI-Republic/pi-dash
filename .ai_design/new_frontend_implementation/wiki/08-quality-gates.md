# Quality gates

**Read this when:** you are finishing an issue, reviewing, or testing.

## Definition of done

An issue is Done only when all of these hold:

1. Every inventory row the issue names is green on `apps/web_new` in the parity suite.
2. Typecheck, lint (including import bans), unit, component and contract tests pass.
3. Performance budgets pass.
4. Similarity check and license-header check pass.
5. The area spec is updated (read → spec → write).
6. No stubs, no `TODO` placeholders for required behavior, no skipped or weakened tests.

## Tests

| Level | Tool | What |
|---|---|---|
| Unit | Vitest | key factories, optimistic updaters, search schemas, utils |
| Component | Vitest + Testing Library | kit components; feature components with a mocked client |
| Contract | Vitest against the Django test server | every `@pidash/api-client` contract parses real responses |
| Editor compat | Vitest | stored `description_html` fixtures round-trip without loss |
| Parity | Playwright, two drivers | every inventory row, against `apps/web` (oracle) and `apps/web_new` |
| E2E | Playwright | `apps/web_new`-only behavior: shell, peek, keyboard, cache start |
| Desktop smoke | Tauri + WebDriver | launch, sign-in, cached start, runner controls, deep link |
| Performance | size-limit, Playwright traces | budgets below |

## Performance budgets (CI; a PR over budget fails)

| Metric | Budget |
|---|---|
| Initial JS (shell + first route), gzipped | ≤ 150 KB |
| Any route chunk, gzipped (excluding named heavy chunks) | ≤ 50 KB |
| Editor chunk | tracked; alert on +10% |
| Initial CSS, gzipped | ≤ 30 KB |
| Desktop first meaningful paint, warm cache | ≤ 500 ms |
| Issue list with 2,000 rows, scrolling | no frame > 50 ms |

Raising a budget needs a `process:` comment on NEWFRONT-1 and a human decision; never raise one in the same PR that exceeds it.

Baseline for comparison (old app, desktop build): opening a project's issue list loads 245 JS files, 7.6 MB raw / 2.3 MB gzipped, including both editor chunks and the charts library.

## Structural CI checks

- Import-boundary lint (layers, feature `index.ts`, old-frontend ban).
- Heavy dependencies only via `import()`.
- Web bundle contains no Tauri code; desktop bundle contains no web-only routes.
- Runtime dependency allowlist.
- Similarity check and license header check.

## Review checklist

- Does the diff follow *Architecture* (layers, import rules, file placement)?
- Is server data only accessed through feature query/mutation hooks?
- Does anything look translated from the old files the spec cites? (Reject if so.)
- Are the named inventory rows covered by parity scenarios that assert both UI and server state?
- Are permissions, empty/error states and shortcuts from the inventory handled?
- Two independent passes before deciding.
