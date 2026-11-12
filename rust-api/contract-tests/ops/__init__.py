# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""D-37 ops CLI-parity suites (PIDASHCONV-806..810).

Suite A — boot + storage (D-37 oracle, F37-01/F37-02) — drives the built
``pidash-api ops <command>`` binary directly (no Django):
``wait_for_db`` / ``wait_for_migrations`` against a scratch ``DATABASE_URL``,
``clear_cache`` against ``REDIS_URL``, and ``create_bucket`` /
``update_bucket`` against S3. ``stdout``/``stderr``/exit codes are asserted
byte for byte against the Django goldens in
``rust-api/fixtures/ops/commands/``.

S3 runs against two backends. The happy paths use a real S3-compatible
server (``AWS_S3_ENDPOINT_URL`` — LocalStack in CI, which like the real
MinIO the issue names speaks SigV4 over HTTP but, unlike MinIO, enforces
no credentials). The 403/denied/drop branches run against an in-test
SigV4-*verifying* stub: every stubbed call re-verifies the request
signature (the verifier itself is pinned against a botocore-generated
vector), so those tests prove the Rust signer, not just the branches.
"""
