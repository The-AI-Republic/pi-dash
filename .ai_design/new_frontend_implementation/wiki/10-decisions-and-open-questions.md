# Decisions and open questions

**Read this when:** you hit a question that looks undecided. If it is listed as open, do not decide it yourself: note it in your workpad, leave a `process:` comment on NEWFRONT-1, and continue with work that does not depend on it (or wait on the decision issue if one exists).

## Settled

| # | Decision |
|---|---|
| D1 | Stay on React. The old app's weight is architectural, not React's. |
| D2 | New app at `apps/web_new/`, package `web_new`. New packages `@pidash/kit`, `@pidash/api-client`. |
| D3 | Stack: Vite + React 19 (+ Compiler), TanStack Router, TanStack Query, Zustand, Tailwind 4, Base UI. See *Stack and dependencies*. |
| D4 | Not a 1:1 translation. Old code is a reference, never a source. See *Reference, not source*. |
| D5 | Side-by-side migration: desktop switches first (end of Phase 2), web routes move through Caddy, then `apps/web` is removed. |
| D6 | `apps/web` is frozen to bug fixes after Phase 0. New features only in `apps/web_new`. |
| D7 | Full functional parity with `apps/web` (OSS, cloud, desktop). No area is dropped. See *Parity*. |
| D8 | API contracts are hand-written with zod (the OpenAPI schema covers only `/api/v1/`, not the internal endpoints). |
| D9 | Static SPA only; no server-side rendering. |
| D10 | All work goes into the `web-new-dev` branch through PRs; a human merges it into `main` at phase exits. |
| D11 | No human merge gate: the In Test run that passes merges its own PR (Test rule in the project description). Human involvement is limited to decisions and sign-offs (issues labeled `human`). |
| D12 | Visual direction: dense and neutral, Inter, indigo accent, follow the OS theme, compact on desktop. Token spec on *UI kit and editor* (NEWFRONT-5). |
| D13 | `apps/admin` and `apps/space` are in scope (NEWFRONT-4). They become route subtrees `/god-mode/*` and `/spaces/*` inside `apps/web_new`, web build only, and are removed with `apps/web` at the end. |

## Open

| # | Question | Owner |
|---|---|---|
| Q1 | License for the new tree and exact header text | human · NEWFRONT-2 |
| Q2 | Legal review of the reference-not-source process; whether some areas need split spec/implementation runs | human · NEWFRONT-3 |
| Q4 | Real-time issue updates: is there a server push channel today beyond runner/assistant SSE? Parity requires whatever live behavior `apps/web` has. | investigated by NEWFRONT-27 |
| Q5 | Annotate internal Django views for OpenAPI (`@extend_schema`) so contracts can be generated later? | human |
| Q6 | Charts library for analytics | human · NEWFRONT-7 |
| Q7 | Telemetry on the new app (the old app loads Microsoft Clarity) | human · NEWFRONT-6 |
| Q9 | Similarity-check threshold (starting value ≥ 50 duplicated tokens) | set by NEWFRONT-13 |

## Risks to keep in mind

- **Scope is large.** Everything heavy is lazy; areas are independent epics that run in parallel once Phase 1 patterns exist.
- **Missed features.** Build the inventory from code and the running app; give easy-to-miss behavior its own rows.
- **Old bugs as requirements.** Mark such scenarios `bug:` with a linked issue.
- **Reading turns into copying.** Read → spec → write; the similarity check catches the rest.
- **Performance drift.** Budgets are enforced from the first PR.
- **Editor incompatibility with stored HTML.** Fixture round-trip tests.
