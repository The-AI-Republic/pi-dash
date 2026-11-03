"""Contract tests: api-v1 assets, stickies, intake (D-21 oracle).

Covers the nine URL entries of ``pi_dash/api/urls/{asset,sticky,intake}.py``
against a live backend: user asset upload/patch/delete, server-side user
asset upload/patch/delete, generic workspace asset upload/download/patch,
the stickies viewset (create/list/retrieve/partial-update/destroy), and
intake-issue list/create/retrieve/patch/delete.

api-v1 authenticates only via ``X-Api-Key`` (``APIKeyAuthentication``), so
this suite seeds per-user ``api_tokens`` rows (see ``_harness.seed``) instead
of minting session cookies.

Run: ``cd rust-api/contract-tests && pytest api_assets_stickies_intake``
with ``BASE_URL``, ``DATABASE_URL`` and ``SECRET_KEY`` in the environment.
"""
