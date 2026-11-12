# Contract suite for D-35: app analytics + exporters.
#
# Django sources pinned here:
# - ``pi_dash/app/urls/analytic.py`` (13 paths), ``pi_dash/app/urls/exporter.py``
#   (1 path: ``export-issues`` GET + POST)
# - views ``pi_dash/app/views/analytic/{base,advance,project_analytics}.py``,
#   ``pi_dash/app/views/exporter/base.py``
# - serializers ``pi_dash/app/serializers/{analytic,exporter}.py``,
#   models ``pi_dash/db/models/{analytic,exporter}.py``
#
# Auth is cookie-session (``session-id``); ``conftest`` seeds two workspaces
# plus users of every role and forges sessions via ``_harness.auth`` (stdlib
# HMAC, no Django import). The world is torn down after the session so
# re-runs are hermetic.
