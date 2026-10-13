# Cloud-oracle stack seed (NEWFRONT-436).
#
# Runs inside the stack's own api-cloud container through `manage.py shell`
# (piped on stdin, so this file is never imported by the backend and the
# backend itself stays untouched). Converges one deterministic cloud
# identity plus one workspace with a project and three named issues. Every
# step is get-or-create, so reruns converge instead of duplicating. Prints
# one `PARITY_SEED_JSON:` line; the caller saves it as the seed-facts file
# the scenarios read.
#
# The identity is created through the cloud package's own dev-identity
# helper (the same helper the dev-login endpoint uses), with a fixed
# subject: cloud users have no password, and scenarios sign this user in
# through POST /api/auth/dev-login/ (see the Cloud oracle runbook). The
# seed facts keep the OSS seed's key names (workspaceSlug, projectId,
# issueNames) so shared helpers keep working; there are deliberately no
# password fields.
#
# Deliberately smaller than seed_parity.py: no second member, no guest, no
# bot, no comment/notification/runner fixtures, no instance-admin grant and
# no OSS auth-flag rows. Cloud admin access is OIDC plus allowlist (the OSS
# email admin endpoints are shadowed off), the OSS email-login flags are
# dead under the cloud URL conf, and nothing in the cloud area exercises
# mentions, notifications or runners.
import json
import os
import time

from django.db import transaction

from pi_dash.db.models import (
    Issue,
    IssueActivity,
    IssueSequence,
    Profile,
    Project,
    ProjectMember,
    ProjectUserProperty,
    State,
    Workspace,
    WorkspaceMember,
)
from pi_dash_cloud.airepublic.sync import upsert_dev_identity

EMAIL = os.environ.get("PARITY_CLOUD_SEED_EMAIL", "cloud-oracle@example.com")
SUB = os.environ.get("PARITY_CLOUD_SEED_SUB", "parity|cloud-seed")
DISPLAY_NAME = os.environ.get("PARITY_CLOUD_SEED_DISPLAY_NAME", "Cloud Oracle")
WORKSPACE_NAME = "Cloud Workspace"
WORKSPACE_SLUG = "cloud-ws"
PROJECT_NAME = "Cloud Project"
PROJECT_IDENTIFIER = "CLD"
ISSUE_NAMES = ["Cloud first issue", "Cloud second issue", "Cloud third issue"]


def build() -> dict:
    with transaction.atomic():
        user = upsert_dev_identity(sub=SUB, email=EMAIL, display_name=DISPLAY_NAME)
        # Converge what scenarios (and the UI shell) read, so a reseed
        # heals drift instead of duplicating rows. Anchored on email like
        # the helper itself.
        user.is_active = True
        user.is_email_verified = True
        user.display_name = DISPLAY_NAME
        user.save(update_fields=["is_active", "is_email_verified", "display_name"])

        profile, _ = Profile.objects.get_or_create(user=user)
        profile.is_onboarded = True
        profile.save(update_fields=["is_onboarded"])

        workspace, ws_created = Workspace.objects.get_or_create(
            slug=WORKSPACE_SLUG,
            defaults={"name": WORKSPACE_NAME, "owner": user, "created_by_id": user.id},
        )
        if ws_created:
            workspace.save(created_by_id=user.id, disable_auto_set_user=True)
        WorkspaceMember.objects.get_or_create(
            workspace=workspace, member=user, defaults={"role": 20}
        )
        Profile.objects.filter(user=user).update(last_workspace_id=workspace.id)

        project, _ = Project.objects.get_or_create(
            workspace=workspace,
            identifier=PROJECT_IDENTIFIER,
            defaults={"name": PROJECT_NAME, "created_by_id": user.id},
        )
        ProjectMember.objects.get_or_create(
            project=project,
            member=user,
            defaults={"role": 20, "workspace_id": workspace.id, "created_by_id": user.id},
        )
        # Project creation signals already stamp a property row per member;
        # keep exactly one row and force the flat list layout on it.
        ProjectUserProperty.objects.update_or_create(
            project=project,
            user=user,
            workspace_id=workspace.id,
            defaults={
                "display_filters": {"layout": "list", "group_by": None, "order_by": "sort_order"},
                "display_properties": {},
            },
        )
        state, _ = State.objects.get_or_create(
            workspace=workspace,
            project=project,
            group="unstarted",
            defaults={
                "name": "Todo",
                "color": "#3A3A3A",
                "sequence": 25000,
                "default": True,
                "created_by_id": user.id,
            },
        )
        if project.default_state_id != state.id:
            project.default_state = state
            project.save(update_fields=["default_state"])

        for position, name in enumerate(ISSUE_NAMES):
            issue, issue_created = Issue.objects.get_or_create(
                workspace=workspace,
                project=project,
                name=name,
                defaults={
                    "state": state,
                    "sequence_id": position + 1,
                    "sort_order": float(10000 * (position + 1)),
                    "created_by_id": user.id,
                },
            )
            if issue_created:
                issue.save(created_by_id=user.id, disable_auto_set_user=True)
                IssueSequence.objects.create(
                    issue=issue,
                    project=project,
                    workspace_id=workspace.id,
                    created_by_id=user.id,
                )
                IssueActivity.objects.create(
                    issue=issue,
                    project=project,
                    workspace_id=workspace.id,
                    comment="created the issue",
                    verb="created",
                    actor_id=user.id,
                    epoch=time.time(),
                )

    return {
        "email": EMAIL,
        "sub": SUB,
        "workspaceSlug": WORKSPACE_SLUG,
        "workspaceName": WORKSPACE_NAME,
        "projectId": str(project.id),
        "projectName": PROJECT_NAME,
        "issueNames": list(ISSUE_NAMES),
    }


facts = build()
print("PARITY_SEED_JSON:" + json.dumps(facts))
