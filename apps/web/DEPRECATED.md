# `apps/web` is deprecated

The legacy frontend is **deprecated**. It has been replaced by the new
frontend in [`apps/web_new/`](../web_new/), which serves production as of
2026-11-12.

The code stays in the tree for reference and is not deleted yet.

## What is still allowed here

- Security fixes and fixes that unblock a deployment still on the legacy stack.

## What is not allowed

- New features, routes, components or refactors. Implement them in
  `apps/web_new/` (and shared code in `packages/kit` / `packages/api-client`).

## Removal criteria

`apps/web` can be deleted once all of these hold:

1. `docker-compose.yml` and the community deployment no longer build `apps/web`.
2. No production traffic reaches the legacy frontend.
3. No other workspace package imports from `apps/web`.
