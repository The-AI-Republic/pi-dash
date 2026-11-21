# `apps/api` is deprecated

The Django backend is **deprecated**. It has been replaced by the Rust backend
in [`rust-api/`](../../rust-api/), which serves production as of 2026-11-12.

The code stays in the tree for reference and for the remaining responsibility
below. It is not deleted yet.

## What is still allowed here

- **Schema migrations.** Django still owns the database schema for OSS /
  self-hosted deployments: `rust-api/bin/docker-entrypoint-migrator.sh` runs
  `python manage.py migrate`. Until schema ownership moves to Rust, new
  migrations still go in `apps/api/pi_dash/*/migrations/`. Keep following
  `cloud-migration-port-check.yml` for those.
- Security fixes and fixes that unblock a deployment still on the legacy stack.

## What is not allowed

- New features, endpoints, models (other than migrations), Celery tasks or
  refactors. Implement them in `rust-api/` instead.

## Removal criteria

`apps/api` can be deleted once all of these hold:

1. Schema ownership has moved to Rust in OSS (the migrator no longer calls
   `manage.py migrate`).
2. `docker-compose.yml` and the community deployment no longer build `apps/api`.
3. No production traffic reaches the legacy Python stack.
