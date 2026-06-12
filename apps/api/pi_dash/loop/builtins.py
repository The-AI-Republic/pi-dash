# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""Builtin loop jobs, seeded by migration.

This module is intentionally Django-free (a dataclass + a list) so the seed
data migration can import it without the apps registry — same pattern as
``pi_dash/bgtasks/_rrule.py``. See design §8.1.
"""

from __future__ import annotations

from dataclasses import dataclass


@dataclass(frozen=True)
class BuiltinLoopJob:
    slug: str
    name: str
    public_name: str
    public_description: str
    prompt: str
    min_role: int
    rrule: str
    tzid: str = "UTC"


AUTO_CLOSE_MERGED_PROMPT = (
    "Review open issues in the projects you can access, oldest first. "
    "An issue is a candidate when it references a pull request — in its "
    "links, description, or comments. For each candidate, call "
    "get_pull_request_status on the PR URL. If — and only if — the tool "
    'reports state "merged", move the issue to a state in its project\'s '
    '"completed" state group (use list_states to find one) and add a '
    "one-line comment naming the merged PR. If merge state is "
    '"unknown" or the issue\'s state is already in the completed group, '
    "leave it untouched. Do not create or delete anything."
)


#: The MVP catalog. Exactly one builtin to validate the wiring end to end;
#: more ship later as code with no schema change. Seeded ``enabled=False`` by
#: the migration (design §13) — the operator flips it on from apps/admin after
#: a smoke test.
BUILTIN_LOOP_JOBS: list[BuiltinLoopJob] = [
    BuiltinLoopJob(
        slug="auto-close-merged",
        name="Auto-close merged-PR issues",
        public_name="Close issues when their PR merges",
        public_description=(
            "Checks your projects once a day and marks an issue Done when the "
            "pull request that implements it has been merged."
        ),
        prompt=AUTO_CLOSE_MERGED_PROMPT,
        min_role=15,  # Member
        rrule="FREQ=DAILY;BYHOUR=3;BYMINUTE=0",
    ),
]
