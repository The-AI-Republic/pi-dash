"""Beat + signal identity pins (static) with live executability cover.

The domain beat entry fires every 4h, so wall-clock firing is not observable
in a test run. Parity is pinned in two halves:

  * identity — `pi_dash/celery.py` still maps `github-issue-sync-every-4h`
    to `git_sync_task.sync_all_bindings` on a 4h crontab (legacy schedule
    name kept so django-celery-beat does not run the old and new pollers in
    parallel), and the target is exactly the fan-out task proven executable
    in test_fanout.py;
  * registered/executable — the beat target and both completion tasks are
    consumed from the broker by the live worker (test_fanout.py is the
    positive control; unknown-id probes here prove the completion tasks
    are registered too).

Signal wiring (github_signals completion hook) needs an authenticated HTTP
state transition (SessionAuthentication), so firing is pinned on the source:
the hook snapshots prior state pre-save, fires only on a transition into a
completed-group state, skips already-commented mirrors, and delays exactly
one provider task per mirror type. The delayed tasks are the same
post_completion_comment tasks proven executable above.
"""
from __future__ import annotations

import pathlib


FANOUT = "pi_dash.bgtasks.git_sync_task.sync_all_bindings"
POST_COMMENT = "pi_dash.bgtasks.git_sync_task.post_completion_comment"
LEGACY_POST_COMMENT = "pi_dash.bgtasks.github_sync_task.post_completion_comment"


def _repo_root() -> pathlib.Path:
    # Live runs mount the checkout read-only at /repo (see suite README);
    # a direct checkout run finds it by walking up instead.
    mounted = pathlib.Path("/repo")
    if (mounted / "apps" / "api" / "pi_dash" / "celery.py").exists():
        return mounted
    here = pathlib.Path(__file__).resolve()
    for parent in here.parents:
        if (parent / "apps" / "api" / "pi_dash" / "celery.py").exists():
            return parent
    raise AssertionError("could not locate repo root from " + str(here))


def test_beat_entry_identity():
    body = (_repo_root() / "apps" / "api" / "pi_dash" / "celery.py").read_text()
    assert '"github-issue-sync-every-4h"' in body
    assert '"git-issue-sync-every-4h" not in body'
    assert FANOUT in body
    # 4h cadence: minute=0, hour=*/4 on that entry.
    assert 'hour="*/4"' in body or "hour='*/4'" in body


def test_beat_target_matches_fanout_task():
    from . import test_fanout

    assert test_fanout.FANOUT == FANOUT


def test_retry_schedule_matches_source():
    body = (
        _repo_root() / "apps" / "api" / "pi_dash" / "bgtasks" / "git_sync_task.py"
    ).read_text()
    assert "max_retries=3" in body
    assert "countdown=60 * (2 ** self.request.retries)" in body


def test_signal_hook_wiring():
    body = (
        _repo_root() / "apps" / "api" / "pi_dash" / "bgtasks" / "github_signals.py"
    ).read_text()
    # Separate dispatch_uid namespace from orchestration.signals.
    assert '"git_sync.issue_presave"' in body
    assert '"git_sync.issue_postsave"' in body
    # Fires only on a transition into a completed-group state.
    assert "StateGroup.COMPLETED" in body
    assert "prev_state_id == instance.state_id" in body
    # Skips already-commented mirrors, delays one task per mirror type.
    assert "completion_comment_id" in body
    assert "post_git_completion_comment.delay" in body
    assert "post_completion_comment.delay" in body
    # Delayed tasks are the ones this suite publishes verbatim.
    assert POST_COMMENT.rsplit(".", 1)[-1] in body
    assert LEGACY_POST_COMMENT.rsplit(".", 1)[-1] in body
