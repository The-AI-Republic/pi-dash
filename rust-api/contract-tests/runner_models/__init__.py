"""Contract tests: runner LLM model catalog (NEWPIDASH-169 oracle).

New-system-only feature (no Django counterpart before this suite):
`GET /api/runners/model-catalog/` plus instance-admin CRUD under
`/api/instances/runner-models/`. On Django the paths 404 (recorded in
the differences ledger as a new feature); on Rust they pin the full
contract (grouped read, validation, preselect transaction, 401/403).
"""
