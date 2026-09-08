"""Desktop-bundled managed runner: policy for a Pi Dash-provisioned Runner.

The managed runner is a real ``Runner`` executing through the ordinary daemon,
worktree and approval path. This package holds only the *policy* that makes it
different from a user-installed local runner: availability is scoped to one
viewer's open desktop app, dispatch is always pinned to that machine, and work
that cannot run right now waits visibly instead of silently.

See ``.ai_design/managed_runner/design.md``.
"""
