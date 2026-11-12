**Brief for counsel (prepared 2026-09-29).** Status: sent to counsel; this issue stays open until their answer is recorded here.

## Background

- Pi Dash's frontend (`apps/web`, `apps/admin`, `apps/space`, shared `packages/*`) is derived from Plane, licensed **AGPL-3.0**.
- We are writing a replacement frontend (`apps/web_new`, `packages/kit`, `packages/api-client`) and intend to license it differently. Our current preference is Apache-2.0 (not final; NEWFRONT-2).
- The new code is written mostly by AI coding agents, working through tracked issues. Every change is a reviewed PR.

## The process

1. **No imports.** The new code imports nothing from the old frontend (components, utilities, types, styles). CI enforces this.
2. **No copying.** No code, CSS, UI text, icons or assets are copied, and old files are not translated line by line into the new stack.
3. **Reading is allowed.** Agents may read the old code and run the old app to learn *behavior*: what a screen does, validation rules, permissions, and which backend API calls it makes.
4. **Spec step.** What is learned is written in prose in a per-area spec (behavior only, no code). New code is written from the spec and our own design documents.
5. **Similarity check.** CI compares the new code with the old code token by token and fails any PR with a duplicated block of 50 tokens or more.
6. **Records.** Specs, PRs, reviews and CI results are kept in git and in the issue tracker.
7. **Same functionality.** The new app must reproduce every feature of the old one (the goal is a drop-in replacement), with a new visual design and a new architecture.

## What stays AGPL

The backend (`apps/api`, Django), the collaboration server (`apps/live`) and the remaining shared packages are also Plane-derived and are **not** covered by this project. The new frontend talks to that backend over HTTP.

## Questions for counsel

1. Is the new frontend, written under this process, free of AGPL obligations so it can carry a different license?
2. Is letting implementers read the original code acceptable, or do some or all areas need a stricter separation (one agent reads the old code and writes the spec; a different agent, with no access to the old code, implements from the spec)?
3. Are there constraints from reproducing the same features and the same API calls (the backend's HTTP interface is unchanged)?
4. Is there anything in the records above we should keep differently, or keep longer?
5. What is needed for the backend and the other Plane-derived parts before the product as a whole can be offered under the new license?
