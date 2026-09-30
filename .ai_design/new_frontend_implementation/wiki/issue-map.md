# NEWFRONT issue map

Generated when the project was set up (2026-09-29). Pi Dash is the source of truth; this is a record.

| Key | Issue | Title | Blocked by |
|---|---|---|---|
| RULEBOOK | NEWFRONT-1 | Rulebook: build apps/web_new (independent code, full functional parity) |  |
| H-license | NEWFRONT-2 | Decision: license for the new frontend and the file header text |  |
| H-legal | NEWFRONT-3 | Decision: legal review of the reference-not-source process |  |
| H-scope | NEWFRONT-4 | Decision: are apps/admin and apps/space in scope? |  |
| H-visual | NEWFRONT-5 | Decision: visual design direction (tokens, type, density, shell and issue mockups) |  |
| H-telemetry | NEWFRONT-6 | Decision: telemetry in the new app |  |
| H-charts | NEWFRONT-7 | Decision: charts library for analytics |  |
| H-signoff-1 | NEWFRONT-8 | Sign-off: feature inventories for Phase 1 areas | NEWFRONT-23, NEWFRONT-25, NEWFRONT-27, NEWFRONT-29, NEWFRONT-31 |
| H-signoff-2 | NEWFRONT-9 | Sign-off: feature inventories for Phase 2 areas | NEWFRONT-33, NEWFRONT-35, NEWFRONT-37, NEWFRONT-39 |
| H-signoff-3 | NEWFRONT-10 | Sign-off: feature inventories for Phase 3 areas | NEWFRONT-41, NEWFRONT-43, NEWFRONT-45, NEWFRONT-47, NEWFRONT-49, NEWFRONT-51, NEWFRONT-53, NEWFRONT-55, NEWFRONT-57, NEWFRONT-59, NEWFRONT-61, NEWFRONT-63, NEWFRONT-65, NEWFRONT-67, NEWFRONT-94, NEWFRONT-96 |
| H-accept | NEWFRONT-11 | Acceptance pass: sign off full parity before apps/web is removed | NEWFRONT-69, NEWFRONT-70, NEWFRONT-71, NEWFRONT-72, NEWFRONT-73, NEWFRONT-74, NEWFRONT-75, NEWFRONT-76, NEWFRONT-77, NEWFRONT-78, NEWFRONT-79, NEWFRONT-80, NEWFRONT-81, NEWFRONT-82, NEWFRONT-83, NEWFRONT-84, NEWFRONT-85, NEWFRONT-86, NEWFRONT-87, NEWFRONT-88, NEWFRONT-89, NEWFRONT-90, NEWFRONT-91, NEWFRONT-98, NEWFRONT-99, NEWFRONT-92 |
| F-01 | NEWFRONT-12 | F-01: scaffold apps/web_new, packages/kit, packages/api-client + CI |  |
| F-02 | NEWFRONT-13 | F-02: CI similarity check against the old frontend | NEWFRONT-12 |
| F-03 | NEWFRONT-14 | F-03: @pidash/api-client core, first contracts, contract-test harness | NEWFRONT-12 |
| F-04 | NEWFRONT-15 | F-04: core/* — platform, edition, query client, session | NEWFRONT-12, NEWFRONT-14 |
| F-05 | NEWFRONT-16 | F-05: @pidash/kit v0 from the approved visual direction | NEWFRONT-12, NEWFRONT-5 |
| F-06 | NEWFRONT-17 | F-06: shell, sign-in and a read-only issue list (first vertical slice) | NEWFRONT-15, NEWFRONT-16 |
| F-07 | NEWFRONT-18 | F-07: desktop build of apps/web_new behind a flag + smoke test | NEWFRONT-17 |
| F-08 | NEWFRONT-19 | F-08: parity harness — seeded stack, driver interface, drivers/web, parity report | NEWFRONT-12 |
| F-09 | NEWFRONT-20 | F-09: web coexistence — migrated-route list, Caddy routing, cross-app links | NEWFRONT-17 |
| F-10 | NEWFRONT-21 | F-10: baseline and performance CI (web load, desktop startup) | NEWFRONT-17, NEWFRONT-18 |
| F-11 | NEWFRONT-22 | F-11: apply the final license header | NEWFRONT-12, NEWFRONT-2 |
| I-auth | NEWFRONT-23 | Inventory: Auth, sign-up, invitations, onboarding, create workspace |  |
| O-auth | NEWFRONT-24 | Oracle scenarios: Auth, sign-up, invitations, onboarding, create workspace | NEWFRONT-23, NEWFRONT-19 |
| I-shell | NEWFRONT-25 | Inventory: Workspace shell, home, projects list, command palette / Power K, search |  |
| O-shell | NEWFRONT-26 | Oracle scenarios: Workspace shell, home, projects list, command palette / Power K, search | NEWFRONT-25, NEWFRONT-19 |
| I-issues | NEWFRONT-27 | Inventory: Issues: layouts, detail, peek, filters, bulk edit, relations, attachments |  |
| O-issues | NEWFRONT-28 | Oracle scenarios: Issues: layouts, detail, peek, filters, bulk edit, relations, attachments | NEWFRONT-27, NEWFRONT-19 |
| I-comments | NEWFRONT-29 | Inventory: Comments, activity, mentions, reactions |  |
| O-comments | NEWFRONT-30 | Oracle scenarios: Comments, activity, mentions, reactions | NEWFRONT-29, NEWFRONT-19 |
| I-drafts | NEWFRONT-31 | Inventory: Drafts |  |
| O-drafts | NEWFRONT-32 | Oracle scenarios: Drafts | NEWFRONT-31, NEWFRONT-19 |
| I-runners | NEWFRONT-33 | Inventory: Runners, runs, approvals, runner chat, AI dev machines |  |
| O-runners | NEWFRONT-34 | Oracle scenarios: Runners, runs, approvals, runner chat, AI dev machines | NEWFRONT-33, NEWFRONT-19 |
| I-agents | NEWFRONT-35 | Inventory: Schedulers, prompts, assistant |  |
| O-agents | NEWFRONT-36 | Oracle scenarios: Schedulers, prompts, assistant | NEWFRONT-35, NEWFRONT-19 |
| I-notifications | NEWFRONT-37 | Inventory: Notifications |  |
| O-notifications | NEWFRONT-38 | Oracle scenarios: Notifications | NEWFRONT-37, NEWFRONT-19 |
| I-desktop | NEWFRONT-39 | Inventory: Desktop-only behavior: agent runtime, bare sign-in, updater, deep links, native shell |  |
| O-desktop | NEWFRONT-40 | Oracle scenarios: Desktop-only behavior: agent runtime, bare sign-in, updater, deep links, native shell | NEWFRONT-39, NEWFRONT-19 |
| I-views | NEWFRONT-41 | Inventory: Views (project and workspace) |  |
| O-views | NEWFRONT-42 | Oracle scenarios: Views (project and workspace) | NEWFRONT-41, NEWFRONT-19 |
| I-archives | NEWFRONT-43 | Inventory: Archives |  |
| O-archives | NEWFRONT-44 | Oracle scenarios: Archives | NEWFRONT-43, NEWFRONT-19 |
| I-intake | NEWFRONT-45 | Inventory: Intake |  |
| O-intake | NEWFRONT-46 | Oracle scenarios: Intake | NEWFRONT-45, NEWFRONT-19 |
| I-cycles | NEWFRONT-47 | Inventory: Cycles and active cycles |  |
| O-cycles | NEWFRONT-48 | Oracle scenarios: Cycles and active cycles | NEWFRONT-47, NEWFRONT-19 |
| I-modules | NEWFRONT-49 | Inventory: Modules |  |
| O-modules | NEWFRONT-50 | Oracle scenarios: Modules | NEWFRONT-49, NEWFRONT-19 |
| I-pages | NEWFRONT-51 | Inventory: Pages (collaborative documents) |  |
| O-pages | NEWFRONT-52 | Oracle scenarios: Pages (collaborative documents) | NEWFRONT-51, NEWFRONT-19 |
| I-estimates | NEWFRONT-53 | Inventory: Estimates |  |
| O-estimates | NEWFRONT-54 | Oracle scenarios: Estimates | NEWFRONT-53, NEWFRONT-19 |
| I-analytics | NEWFRONT-55 | Inventory: Analytics |  |
| O-analytics | NEWFRONT-56 | Oracle scenarios: Analytics | NEWFRONT-55, NEWFRONT-19 |
| I-stickies | NEWFRONT-57 | Inventory: Stickies |  |
| O-stickies | NEWFRONT-58 | Oracle scenarios: Stickies | NEWFRONT-57, NEWFRONT-19 |
| I-exports | NEWFRONT-59 | Inventory: Exports (CSV, PDF) |  |
| O-exports | NEWFRONT-60 | Oracle scenarios: Exports (CSV, PDF) | NEWFRONT-59, NEWFRONT-19 |
| I-project-settings | NEWFRONT-61 | Inventory: Project settings |  |
| O-project-settings | NEWFRONT-62 | Oracle scenarios: Project settings | NEWFRONT-61, NEWFRONT-19 |
| I-workspace-settings | NEWFRONT-63 | Inventory: Workspace settings (members, integrations, webhooks, API tokens, billing, exports entry) |  |
| O-workspace-settings | NEWFRONT-64 | Oracle scenarios: Workspace settings (members, integrations, webhooks, API tokens, billing, exports entry) | NEWFRONT-63, NEWFRONT-19 |
| I-profile | NEWFRONT-65 | Inventory: Profile, account, appearance, notification preferences |  |
| O-profile | NEWFRONT-66 | Oracle scenarios: Profile, account, appearance, notification preferences | NEWFRONT-65, NEWFRONT-19 |
| I-cloud | NEWFRONT-67 | Inventory: Cloud edition: home, docs, downloads, pricing, login, apps, profile tabs |  |
| O-cloud | NEWFRONT-68 | Oracle scenarios: Cloud edition: home, docs, downloads, pricing, login, apps, profile tabs | NEWFRONT-67, NEWFRONT-19 |
| I-admin | NEWFRONT-94 | Inventory: Instance admin (god-mode): general, email, auth providers, AI, images, loop, workspaces |  |
| O-admin | NEWFRONT-95 | Oracle scenarios: Instance admin (god-mode): general, email, auth providers, AI, images, loop, workspaces | NEWFRONT-94, NEWFRONT-19 |
| I-space | NEWFRONT-96 | Inventory: Public boards (space): published project boards and public issue view |  |
| O-space | NEWFRONT-97 | Oracle scenarios: Public boards (space): published project boards and public issue view | NEWFRONT-96, NEWFRONT-19 |
| E-auth | NEWFRONT-69 | Epic: Auth, sign-up, invitations, onboarding, create workspace | NEWFRONT-24, NEWFRONT-8, NEWFRONT-17, NEWFRONT-20 |
| E-shell | NEWFRONT-70 | Epic: Workspace shell, home, projects list, command palette / Power K, search | NEWFRONT-26, NEWFRONT-8, NEWFRONT-17, NEWFRONT-20 |
| E-issues | NEWFRONT-71 | Epic: Issues: layouts, detail, peek, filters, bulk edit, relations, attachments | NEWFRONT-28, NEWFRONT-8, NEWFRONT-17, NEWFRONT-20, NEWFRONT-70 |
| E-comments | NEWFRONT-72 | Epic: Comments, activity, mentions, reactions | NEWFRONT-30, NEWFRONT-8, NEWFRONT-17, NEWFRONT-20, NEWFRONT-71 |
| E-drafts | NEWFRONT-73 | Epic: Drafts | NEWFRONT-32, NEWFRONT-8, NEWFRONT-17, NEWFRONT-20, NEWFRONT-71 |
| E-runners | NEWFRONT-74 | Epic: Runners, runs, approvals, runner chat, AI dev machines | NEWFRONT-34, NEWFRONT-9, NEWFRONT-17, NEWFRONT-20, NEWFRONT-70, NEWFRONT-102 |
| E-agents | NEWFRONT-75 | Epic: Schedulers, prompts, assistant | NEWFRONT-36, NEWFRONT-9, NEWFRONT-17, NEWFRONT-20, NEWFRONT-70, NEWFRONT-102 |
| E-notifications | NEWFRONT-76 | Epic: Notifications | NEWFRONT-38, NEWFRONT-9, NEWFRONT-17, NEWFRONT-20, NEWFRONT-70 |
| E-desktop | NEWFRONT-77 | Epic: Desktop-only behavior: agent runtime, bare sign-in, updater, deep links, native shell | NEWFRONT-40, NEWFRONT-9, NEWFRONT-17, NEWFRONT-20, NEWFRONT-70, NEWFRONT-18, NEWFRONT-102 |
| E-views | NEWFRONT-78 | Epic: Views (project and workspace) | NEWFRONT-42, NEWFRONT-10, NEWFRONT-17, NEWFRONT-20, NEWFRONT-71 |
| E-archives | NEWFRONT-79 | Epic: Archives | NEWFRONT-44, NEWFRONT-10, NEWFRONT-17, NEWFRONT-20, NEWFRONT-71 |
| E-intake | NEWFRONT-80 | Epic: Intake | NEWFRONT-46, NEWFRONT-10, NEWFRONT-17, NEWFRONT-20, NEWFRONT-71 |
| E-cycles | NEWFRONT-81 | Epic: Cycles and active cycles | NEWFRONT-48, NEWFRONT-10, NEWFRONT-17, NEWFRONT-20, NEWFRONT-71 |
| E-modules | NEWFRONT-82 | Epic: Modules | NEWFRONT-50, NEWFRONT-10, NEWFRONT-17, NEWFRONT-20, NEWFRONT-71 |
| E-pages | NEWFRONT-83 | Epic: Pages (collaborative documents) | NEWFRONT-52, NEWFRONT-10, NEWFRONT-17, NEWFRONT-20, NEWFRONT-70 |
| E-estimates | NEWFRONT-84 | Epic: Estimates | NEWFRONT-54, NEWFRONT-10, NEWFRONT-17, NEWFRONT-20, NEWFRONT-71 |
| E-analytics | NEWFRONT-85 | Epic: Analytics | NEWFRONT-56, NEWFRONT-10, NEWFRONT-17, NEWFRONT-20, NEWFRONT-71, NEWFRONT-7 |
| E-stickies | NEWFRONT-86 | Epic: Stickies | NEWFRONT-58, NEWFRONT-10, NEWFRONT-17, NEWFRONT-20, NEWFRONT-70 |
| E-exports | NEWFRONT-87 | Epic: Exports (CSV, PDF) | NEWFRONT-60, NEWFRONT-10, NEWFRONT-17, NEWFRONT-20, NEWFRONT-71 |
| E-project-settings | NEWFRONT-88 | Epic: Project settings | NEWFRONT-62, NEWFRONT-10, NEWFRONT-17, NEWFRONT-20, NEWFRONT-70 |
| E-workspace-settings | NEWFRONT-89 | Epic: Workspace settings (members, integrations, webhooks, API tokens, billing, exports entry) | NEWFRONT-64, NEWFRONT-10, NEWFRONT-17, NEWFRONT-20, NEWFRONT-70 |
| E-profile | NEWFRONT-90 | Epic: Profile, account, appearance, notification preferences | NEWFRONT-66, NEWFRONT-10, NEWFRONT-17, NEWFRONT-20, NEWFRONT-70 |
| E-cloud | NEWFRONT-91 | Epic: Cloud edition: home, docs, downloads, pricing, login, apps, profile tabs | NEWFRONT-68, NEWFRONT-10, NEWFRONT-17, NEWFRONT-20, NEWFRONT-70, NEWFRONT-69 |
| E-admin | NEWFRONT-98 | Epic: Instance admin (god-mode): general, email, auth providers, AI, images, loop, workspaces | NEWFRONT-95, NEWFRONT-10, NEWFRONT-17, NEWFRONT-20 |
| E-space | NEWFRONT-99 | Epic: Public boards (space): published project boards and public issue view | NEWFRONT-97, NEWFRONT-10, NEWFRONT-17, NEWFRONT-20, NEWFRONT-71 |
| M-01 | NEWFRONT-92 | M-01: switch the desktop app to apps/web_new; remove desktop-overlay | NEWFRONT-69, NEWFRONT-70, NEWFRONT-71, NEWFRONT-72, NEWFRONT-73, NEWFRONT-74, NEWFRONT-75, NEWFRONT-76, NEWFRONT-77, NEWFRONT-18, NEWFRONT-21 |
| M-02 | NEWFRONT-93 | M-02: remove apps/web, apps/admin, apps/space and the old frontend packages | NEWFRONT-69, NEWFRONT-70, NEWFRONT-71, NEWFRONT-72, NEWFRONT-73, NEWFRONT-74, NEWFRONT-75, NEWFRONT-76, NEWFRONT-77, NEWFRONT-78, NEWFRONT-79, NEWFRONT-80, NEWFRONT-81, NEWFRONT-82, NEWFRONT-83, NEWFRONT-84, NEWFRONT-85, NEWFRONT-86, NEWFRONT-87, NEWFRONT-88, NEWFRONT-89, NEWFRONT-90, NEWFRONT-91, NEWFRONT-98, NEWFRONT-99, NEWFRONT-92, NEWFRONT-11, NEWFRONT-3 |
| F-12 | NEWFRONT-102 | F-12: similarity check — exclude AI Republic's own code from the old-code corpus |  |
| AUDIT | NEWFRONT-103 | Coverage audit: every route, component, service method and shortcut of the old frontends is in some inventory | all 25 inventory issues (blocks NEWFRONT-8, -9, -10) |
| D17-API | NEWFRONT-104 | Pages API: serve child pages to the web app (nested pages, D17) | — (blocks NEWFRONT-83) |
| F-12 | NEWFRONT-102 | F-12: similarity check — exclude AI Republic own code | — (blocks NEWFRONT-74, -75, -77) |
