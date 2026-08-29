"""Bounded, sanitized semantic events for Cloud Agent runs."""

from django.conf import settings
from django.db import transaction

from pi_dash.runner.models import AgentRun, AgentRunEvent


def append(run_id, kind: str, payload=None) -> bool:
    payload = payload or {}
    with transaction.atomic():
        # Lock the parent because a queryset cannot lock the empty event set.
        AgentRun.objects.select_for_update().only("id").get(pk=run_id)
        events = AgentRunEvent.objects.filter(agent_run_id=run_id)
        count = events.count()
        limit = settings.CLOUD_AGENT_MAX_EVENTS
        # Reserve one row for the mandatory terminal event and one for the
        # truncation marker, so the configured maximum includes both.
        if count >= max(0, limit - 2):
            if count < max(0, limit - 1) and not events.filter(kind="events_truncated").exists():
                AgentRunEvent.objects.create(agent_run_id=run_id, seq=count + 1, kind="events_truncated", payload={})
            return False
        seq = (events.order_by("-seq").values_list("seq", flat=True).first() or 0) + 1
        AgentRunEvent.objects.create(agent_run_id=run_id, seq=seq, kind=kind[:64], payload=payload)
    return True
