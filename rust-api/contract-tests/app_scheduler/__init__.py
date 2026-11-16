"""Contract tests: app scheduler (D-36 oracle).

Covers the five endpoints of ``pi_dash/app/urls/scheduler.py`` against a
live backend: workspace scheduler list/create, scheduler detail
get/patch/delete, project binding list/install, binding detail
get/patch/delete, and the occurrences calendar endpoint.

Run: ``cd rust-api/contract-tests && pytest app_scheduler``
with ``BASE_URL``, ``DATABASE_URL`` and ``SECRET_KEY`` in the environment.
"""
