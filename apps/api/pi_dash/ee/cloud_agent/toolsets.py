"""CE seam for extra Cloud Agent toolsets; hosted deployments may overlay it.

A run's tools come from its immutable ``tool_plan`` — a closed catalog of names
Pi Dash knows in advance. Some deployments can additionally offer tools that
are *not* knowable at plan time: discovered per-user, from an external service,
at connect time. Those cannot go in the plan (their names do not exist yet, and
a name landing in ``required_tools`` would make an unrelated external change
fail runs), so they arrive here instead.

CE returns nothing. A build that overlays this owns three obligations:

* **Admit only what the run's plan allows.** The plan carries the operator's
  and user's decision; this seam must not widen it.
* **Degrade, never fail.** Wrap in a resilient toolset so an outage costs the
  run its extra tools, not the run.
* **Say what was dropped.** Losing a capability silently is nearly as bad as
  losing the run — report it on the run's event stream.

Called once per run, before the agent starts.
"""


def extra_toolsets_enabled_for(user) -> bool:
    """Whether ``user`` has opted in to the extra toolsets (CE: nobody has).

    Read once at run creation and snapshotted onto the plan, not consulted at
    execution time: a run should execute under the policy it was admitted with,
    the same reason ``executor_kind`` is snapshotted. A preference flipped
    mid-flight applies to the next run, not this one.
    """
    return False


def resolve_extra_toolsets_for_run(run):
    """Return additional pydantic-ai toolsets for ``run`` (CE: none)."""
    return []
