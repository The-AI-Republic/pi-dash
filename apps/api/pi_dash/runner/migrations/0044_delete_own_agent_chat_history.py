# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""Delete runner chat history on own-agent runners (NEWPIDASH-332).

Decision O3 of ``.ai_design/engine_shared_app_server/design.md``: cloud
runner chat is offered only for Pi Dash Agent (engine) runners, so the chat
sessions on runners backed by the user's own agent are deleted, not hidden.

Deletes every ``agent_chat_session`` whose runner is not an engine runner
(``runner.provisioning <> 'desktop_bundled'``), with its messages, events,
approvals and dedupe rows. Children are deleted explicitly before the
sessions: Django enforces ``CASCADE`` in the ORM collector, not in the
database, so raw SQL must not rely on it (the same reason
``rust-api/crates/services/src/runner_enroll/delete.rs`` deletes chat
children explicitly). Events go before messages because
``agent_chat_event.message_id`` references ``agent_chat_message``.

Idempotent: each statement deletes only rows matching the predicate, so a
second run matches nothing and changes nothing. The reverse is a no-op:
the delete is final by design, there is no backup step.
"""

from django.db import migrations

# Own-agent sessions: every chat session whose runner is not engine-backed.
# The 'desktop_bundled' literal is frozen here on purpose, so a later rename
# of the provisioning choice cannot change what this migration deletes.
OWN_AGENT_SESSIONS = (
    "SELECT s.id FROM agent_chat_session AS s"
    " INNER JOIN runner AS r ON s.runner_id = r.id"
    " WHERE r.provisioning <> 'desktop_bundled'"
)

DELETE_OWN_AGENT_CHAT = [
    f"DELETE FROM agent_chat_approval WHERE session_id IN ({OWN_AGENT_SESSIONS})",
    f"DELETE FROM agent_chat_event WHERE session_id IN ({OWN_AGENT_SESSIONS})",
    f"DELETE FROM chat_message_dedupe WHERE session_id IN ({OWN_AGENT_SESSIONS})",
    f"DELETE FROM agent_chat_message WHERE session_id IN ({OWN_AGENT_SESSIONS})",
    "DELETE FROM agent_chat_session WHERE runner_id IN (SELECT id FROM runner WHERE provisioning <> 'desktop_bundled')",
]


class Migration(migrations.Migration):
    dependencies = [
        ("runner", "0043_desktop_handoff"),
    ]

    operations = [
        migrations.RunSQL(
            sql=DELETE_OWN_AGENT_CHAT,
            reverse_sql=migrations.RunSQL.noop,
        ),
    ]
