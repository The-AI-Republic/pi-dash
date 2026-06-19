# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""Loop (Auto Project Management) — periodic assistant jobs.

This package holds the loop's eligibility, dispatch, builtins, and HTTP views.
Models live in ``pi_dash/db/models/loop.py`` and Beat tasks in
``pi_dash/bgtasks/loop.py`` (mirroring the scheduler's layout), so there is no
separate Django app / no ``INSTALLED_APPS`` entry.

See ``.ai_design/loop_project_management/design.md``.
"""
