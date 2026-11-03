"""Contract tests: app assets (D-31 oracle).

Covers the 18 routes of ``pi_dash/app/urls/asset.py`` against a live
backend: v1 legacy workspace/user file-assets plus the v2 S3/MinIO
presigned upload/download/duplicate/check/restore/bulk flows.

S3-mediated success bodies (real upload bytes, server-side copy) depend on
live object storage, so the suite pins everything the HTTP wire and the
database determine deterministically: response shapes, validation errors,
permission gates, redirect targets, size clamping, and the partial-write
side effects of the paths below. Known Django bugs are pinned exactly and
listed in the PR; the Rust port reproduces them byte for byte.
"""
