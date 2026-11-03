# Contract suite for D-26: app issues (list, detail, sub-issues,
# relations, activity, drafts, archive).
#
# Django sources pinned here:
# - ``pi_dash/app/urls/issue.py`` (45 paths)
# - views ``pi_dash/app/views/issue/{base,sub_issue,relation,link,activity,
#   comment,archive,label,reaction,subscriber,version,move,attachment,
#   github_pr,git_code_review}.py``
# - serializers ``pi_dash/app/serializers/issue.py`` (Issue* family),
#   models ``pi_dash/db/models`` (issues + related tables)
#
# Auth is cookie-session (``session-id``); ``conftest`` seeds two workspaces
# plus users of every role and forges sessions via ``_harness.auth`` (stdlib
# HMAC, no Django import). The world is torn down after the session so
# re-runs are hermetic.
#
# Server environment (on top of the ``README``): ``REDIS_URL`` must
# point at a live Redis — label writes invalidate the cache through
# ``cache.keys`` and 500 without it. The suite runs without S3/MinIO:
# the v2 valid-type upload (presigned POST) and the uploaded-asset
# redirect branches need a bucket and are not asserted here; the
# invalid-type 400 proves the route, auth and validation instead.
