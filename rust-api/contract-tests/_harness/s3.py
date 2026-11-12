# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""Test-side S3 client for worker-plane oracles (PIDASHCONV-858).

Minimal SigV4-signed PUT/HEAD/DELETE over ``requests`` (a pinned
contract-tests dependency) against the real S3-compatible endpoint the
contract environment provides (LocalStack in CI). The worker-plane
oracle jobs set ``USE_MINIO=1`` with ``AWS_ACCESS_KEY_ID`` /
``AWS_SECRET_ACCESS_KEY`` / ``AWS_S3_ENDPOINT_URL``; both backends
resolve the bucket as ``AWS_S3_BUCKET_NAME`` defaulting to
``"uploads"`` (Django ``common.py:378`` = Rust ``StorageSettings``).

The signer mirrors the suite-local one in
``ops/test_boot_storage.py::sign_s3`` (same header set, real payload
hash) without importing across suites. Every S3 call here executes
for real: a missing endpoint fails loudly, never skips.
"""

import hashlib
import hmac
import os
import time
import urllib.parse

import requests

from . import config

SIGNED_HEADERS = "host;x-amz-content-sha256;x-amz-date"


def _hmac_sha256(key: bytes, message: bytes) -> bytes:
    return hmac.new(key, message, hashlib.sha256).digest()


def _signing_key(secret: str, date_stamp: str, region: str) -> bytes:
    key = ("AWS4" + secret).encode()
    for part in (date_stamp, region, "s3", "aws4_request"):
        key = _hmac_sha256(key, part.encode())
    return key


def sign_s3(method: str, url: str, body: bytes = b"") -> dict:
    """Sign one S3 request; return the auth headers."""
    parsed = urllib.parse.urlsplit(url)
    payload_hash = hashlib.sha256(body).hexdigest()
    now = time.gmtime()
    amz_date = time.strftime("%Y%m%dT%H%M%SZ", now)
    date_stamp = time.strftime("%Y%m%d", now)
    # LocalStack enforces no credentials, so any region signs; default
    # to the S3 partition default when AWS_REGION is unset (as it is in
    # the worker-plane oracle jobs).
    region = os.environ.get("AWS_REGION") or "us-east-1"
    canonical = "\n".join(
        [
            method,
            parsed.path or "/",
            "",
            f"host:{parsed.netloc}",
            f"x-amz-content-sha256:{payload_hash}",
            f"x-amz-date:{amz_date}",
            "",
            SIGNED_HEADERS,
            payload_hash,
        ]
    )
    scope = f"{date_stamp}/{region}/s3/aws4_request"
    string_to_sign = (
        f"AWS4-HMAC-SHA256\n{amz_date}\n{scope}\n"
        f"{hashlib.sha256(canonical.encode()).hexdigest()}"
    )
    signature = hmac.new(
        _signing_key(config.required("AWS_SECRET_ACCESS_KEY"), date_stamp, region),
        string_to_sign.encode(),
        hashlib.sha256,
    ).hexdigest()
    return {
        "Authorization": (
            f"AWS4-HMAC-SHA256 Credential={config.required('AWS_ACCESS_KEY_ID')}/{scope}, "
            f"SignedHeaders={SIGNED_HEADERS}, Signature={signature}"
        ),
        "x-amz-date": amz_date,
        "x-amz-content-sha256": payload_hash,
    }


def s3_endpoint() -> str:
    """The real S3 endpoint both backends talk to (required)."""
    return config.required("AWS_S3_ENDPOINT_URL").rstrip("/")


def contract_bucket() -> str:
    """The bucket both backends resolve: ``AWS_S3_BUCKET_NAME`` or ``"uploads"``.

    Mirrors ``get_config("AWS_S3_BUCKET_NAME", "uploads")`` (Django) and
    ``req_string("AWS_S3_BUCKET_NAME", "uploads")`` (Rust) exactly: only
    an unset variable falls back; set-but-empty stays empty.
    """
    return os.environ.get("AWS_S3_BUCKET_NAME", "uploads")


def s3_call(
    method: str, url: str, body: bytes = b"", extra_headers: dict | None = None
) -> requests.Response:
    """One signed S3 request with this module's signer (requests)."""
    headers = sign_s3(method, url, body)
    headers.update(extra_headers or {})
    return requests.request(method, url, data=body, headers=headers, timeout=30)


def s3_setup_bucket(bucket: str) -> None:
    """Create a bucket (idempotent: 200 created, 409 already owned)."""
    resp = s3_call("PUT", f"{s3_endpoint()}/{bucket}")
    assert resp.status_code in (200, 409), resp.text


def s3_put_key(bucket: str, key: str, body: bytes) -> None:
    """PUT one object; the key is S3-path-quoted like the clients quote it."""
    quoted = urllib.parse.quote(key, safe="/~")
    resp = s3_call(
        "PUT",
        f"{s3_endpoint()}/{bucket}/{quoted}",
        body,
        extra_headers={"Content-Type": "application/octet-stream"},
    )
    assert resp.status_code == 200, resp.text


def s3_head_key(bucket: str, key: str) -> int:
    """HEAD one object; return the status code (200 present, 404 gone)."""
    quoted = urllib.parse.quote(key, safe="/~")
    resp = s3_call("HEAD", f"{s3_endpoint()}/{bucket}/{quoted}")
    return resp.status_code


def s3_key_exists(bucket: str, key: str) -> bool:
    """True iff the object is present (HEAD 200)."""
    return s3_head_key(bucket, key) == 200
