"""Seed a scratch Django database for the @pidash/api-client contract tests.

Run from apps/api with the project's virtualenv::

    python /path/to/packages/api-client/contract/seed-contracts.py --apply

The script prints its database target first and refuses names that look
shared (production, staging, ...). Without --apply it only prints what it
would create. Re-running is safe: every row is created with get_or_create
under a ``contract-`` prefix, except the ``EMAIL_HOST`` instance flag the
backend reads by exact key. Consumable fixtures (the probe memberships, the
autoset user) are repaired on re-seed, not just created.

Required environment (same as a local runserver)::

    DATABASE_URL, REDIS_URL, WEB_URL, APP_BASE_URL, DJANGO_SETTINGS_MODULE
"""

import argparse
import os
import sys

REFUSALS = ("prod", "production", "staging", "main", "shared")

SEED_EMAIL = "contract.tester@example.com"
SEED_SECONDARY_EMAIL = "contract.probe@example.com"
SEED_AUTOSET_EMAIL = "contract.autoset@example.com"
SEED_MEMBERSHIP_EMAIL = "contract.membership@example.com"
# Views area (NEWFRONT-718): a project guest on the seeded project, plus a
# shared view the main member owns. The guest's project view list must
# narrow to its own rows while the flag is off, so a shared view it does
# not own proves the narrowing.
SEED_GUEST_EMAIL = "contract.guest@example.com"
SEED_PASSWORD = os.environ.get("PIDASH_CONTRACT_PASSWORD", "Contract123!")
SEED_WORKSPACE_SLUG = "contract-acme"
SEED_DEV_WORKSPACE_SLUG = "contract-dev"
SEED_PROJECT_IDENTIFIER = "CT"
SEED_BOARD_ANCHOR = "contract-board-anchor"
SEED_DESKTOP_SESSION_KEY = os.environ.get("PIDASH_CONTRACT_DESKTOP_SESSION_KEY", "contractdesktopsession000001")


def database_target():
    from django.conf import settings

    default = settings.DATABASES["default"]
    return {
        "engine": default.get("ENGINE"),
        "name": default.get("NAME"),
        "host": default.get("HOST") or "localhost",
        "port": default.get("PORT") or "5432",
    }


def refuse(target):
    name = str(target["name"] or "").lower()
    return [word for word in REFUSALS if word in name]


def seed():
    from django.contrib.auth.hashers import make_password  # noqa: E402
    from pi_dash.db.models import (  # noqa: E402
        APIToken,
        Intake,
        IntakeIssue,
        Issue,
        IssueActivity,
        IssueDescriptionVersion,
        DeployBoard,
        Label,
        Project,
        ProjectMember,
        State,
        User,
        Workspace,
        WorkspaceMember,
    )
    from pi_dash.db.models.state import DEFAULT_STATES  # noqa: E402
    from pi_dash.license.models.instance import Instance, InstanceAdmin, InstanceConfiguration  # noqa: E402

    created = []

    def note(label, obj, was_created):
        key = obj.pk if hasattr(obj, "pk") else obj
        created.append((label, str(key), was_created))

    from django.utils import timezone  # noqa: E402

    instance = Instance.objects.first()
    if instance is None:
        instance = Instance.objects.create(
            instance_name="contract-seed",
            instance_id="contract-seed",
            current_version="0.0.0-contract",
            is_setup_done=True,
            last_checked_at=timezone.now(),
        )
        note("instance", instance.pk, True)
    elif not instance.is_setup_done:
        instance.is_setup_done = True
        instance.save(update_fields=["is_setup_done"])
        note("instance-setup-done", instance.pk, False)
    else:
        note("instance", instance.pk, False)

    user, user_created = User.objects.get_or_create(
        email=SEED_EMAIL,
        defaults={
            "username": "contract.tester",
            "first_name": "Contract",
            "last_name": "Tester",
            "is_active": True,
        },
    )
    if user_created or not user.check_password(SEED_PASSWORD):
        user.password = make_password(SEED_PASSWORD)
        user.is_active = True
        user.save()
    note("user", user.email, user_created)

    # Desktop session: the desktop-gated suites (NEWFRONT-585) call
    # desktop-gated endpoints, which refuse browser sessions. No HTTP flow in
    # this build stamps the desktop marker (the exchange that does is
    # edition-only), so the seed mints a fixed-key session row carrying it;
    # the suites send its key as the session cookie. Re-seed refreshes the
    # row and its expiry. Written through the model (not the store: the
    # store drops a fixed key whose row does not exist yet).
    from pi_dash.db.models.session import Session, SessionStore  # noqa: E402
    from pi_dash.ee.authentication.desktop import (  # noqa: E402
        DESKTOP_CLIENT,
        DESKTOP_SESSION_KEY,
    )

    encoder = SessionStore()
    _, desktop_created = Session.objects.update_or_create(
        session_key=SEED_DESKTOP_SESSION_KEY,
        defaults={
            "session_data": encoder.encode(
                {
                    "_auth_user_id": user._meta.pk.value_to_string(user),
                    "_auth_user_backend": "django.contrib.auth.backends.ModelBackend",
                    "_auth_user_hash": user.get_session_auth_hash(),
                    DESKTOP_SESSION_KEY: DESKTOP_CLIENT,
                }
            ),
            "expire_date": encoder.get_expiry_date(),
            "user_id": user._meta.pk.value_to_string(user),
        },
    )
    note("desktop-session", SEED_DESKTOP_SESSION_KEY, desktop_created)

    # Instance admin: the god-mode contract suites (NEWFRONT-502/503) sign in
    # as the seed user, so that user holds an InstanceAdmin grant. Revived
    # through all_objects: the NEWFRONT-502 sign-up dance soft-deletes every
    # admin link, and the unique pair blocks re-creating a soft-deleted row,
    # so a plain update_or_create would crash on re-seed after a dance run.
    owner_link, admin_created = InstanceAdmin.all_objects.get_or_create(
        instance=instance,
        user=user,
        defaults={"role": 20, "is_verified": True},
    )
    if owner_link.deleted_at is not None or owner_link.role != 20 or not owner_link.is_verified:
        owner_link.deleted_at = None
        owner_link.role = 20
        owner_link.is_verified = True
        owner_link.save(update_fields=["deleted_at", "role", "is_verified"])
    note("instance-admin", user.email, admin_created)

    workspace, workspace_created = Workspace.objects.get_or_create(
        slug=SEED_WORKSPACE_SLUG,
        defaults={"name": "Contract Acme", "owner": user},
    )
    note("workspace", workspace.slug, workspace_created)

    _, membership_created = WorkspaceMember.objects.get_or_create(
        workspace=workspace,
        member=user,
        defaults={"role": 20, "is_active": True},
    )
    note("workspace-member", f"{workspace.slug}/{user.email}", membership_created)

    project, project_created = Project.objects.get_or_create(
        workspace=workspace,
        identifier=SEED_PROJECT_IDENTIFIER,
        defaults={
            "name": "Contract",
            "description": "Seeded for api-client contract tests.",
            "created_by": user,
            "updated_by": user,
        },
    )
    note("project", project.identifier, project_created)

    _, project_membership_created = ProjectMember.objects.get_or_create(
        workspace=workspace,
        project=project,
        member=user,
        defaults={"role": 20, "is_active": True},
    )
    note("project-member", f"{project.identifier}/{user.email}", project_membership_created)

    # Public board anchor (NEWFRONT-752): the space contracts resolve a
    # published board by its anchor. The board is created here rather than
    # by a publish call so its anchor is stable across runs; re-seeding
    # repairs its flags and layout in case a suite flipped them.
    board, board_created = DeployBoard.objects.get_or_create(
        workspace=workspace,
        project=project,
        entity_name="project",
        defaults={
            "entity_identifier": str(project.id),
            "anchor": SEED_BOARD_ANCHOR,
            "view_props": {"list": True, "kanban": True},
            "is_comments_enabled": True,
            "is_reactions_enabled": True,
            "is_votes_enabled": True,
            "created_by": user,
            "updated_by": user,
        },
    )
    desired_flags = {
        "entity_identifier": str(project.id),
        "anchor": SEED_BOARD_ANCHOR,
        "view_props": {"list": True, "kanban": True},
        "is_comments_enabled": True,
        "is_reactions_enabled": True,
        "is_votes_enabled": True,
    }
    changed = [field for field, value in desired_flags.items() if getattr(board, field) != value]
    if changed:
        for field in changed:
            setattr(board, field, desired_flags[field])
        board.save(update_fields=changed)
    # Non-destructive guard: another suite publishes throwaway boards by
    # anchor, so never delete one here. A collision means a crashed run
    # left a board behind; the note makes that visible instead.
    collision = DeployBoard.objects.filter(anchor=SEED_BOARD_ANCHOR).exclude(pk=board.pk).first()
    if collision is not None:
        note("deploy-board-anchor-collision", collision.pk, True)
    note("deploy-board", board.anchor, board_created)

    # Second project (NEWFRONT-611): the move-to-project contract moves a
    # probe issue here and back, so it needs membership plus a default
    # state for the landing. Found by identifier, never by position, so
    # suites that assume a single project keep passing.
    project_two, project_two_created = Project.objects.get_or_create(
        workspace=workspace,
        identifier="CU",
        defaults={
            "name": "Contract Two",
            "description": "Move target for api-client contract tests.",
            "created_by": user,
            "updated_by": user,
        },
    )
    note("project", project_two.identifier, project_two_created)

    _, project_two_membership_created = ProjectMember.objects.get_or_create(
        workspace=workspace,
        project=project_two,
        member=user,
        defaults={"role": 20, "is_active": True},
    )
    note("project-member", f"{project_two.identifier}/{user.email}", project_two_membership_created)

    # Third project (NEWFRONT-728): the analytics contract suite reads a
    # project that never has work items, to prove the totals answer zero
    # counts (and `project-stats` integer zeros). No other suite touches
    # this one, so it stays empty even while the move suite parks an issue
    # in `CU`; pinning `CU` made the zero assertions race.
    project_empty, project_empty_created = Project.objects.get_or_create(
        workspace=workspace,
        identifier="CE",
        defaults={
            "name": "Contract Empty",
            "description": "Issue-less project for api-client contract tests.",
            "created_by": user,
            "updated_by": user,
        },
    )
    note("project", project_empty.identifier, project_empty_created)

    _, project_empty_membership_created = ProjectMember.objects.get_or_create(
        workspace=workspace,
        project=project_empty,
        member=user,
        defaults={"role": 20, "is_active": True},
    )
    note("project-member", f"{project_empty.identifier}/{user.email}", project_empty_membership_created)

    for index, spec in enumerate(DEFAULT_STATES):
        state_two, state_two_created = State.all_state_objects.get_or_create(
            workspace=workspace,
            project=project_two,
            name=spec["name"],
            defaults={
                "group": spec["group"],
                "color": spec["color"],
                "description": spec.get("description", ""),
                "sequence": spec.get("sequence", index),
                "default": spec.get("default", False),
            },
        )
        note("state", f"{project_two.identifier}/{state_two.name}", state_two_created)

    # Second user: the leave/rejoin contract test signs in as this user, so
    # the membership rows are repaired (not just created) to stay green
    # across runs even after a leave deactivates them.
    probe, probe_created = User.objects.get_or_create(
        email=SEED_SECONDARY_EMAIL,
        defaults={
            "username": "contract.probe",
            "first_name": "Contract",
            "last_name": "Probe",
            "is_active": True,
        },
    )
    if probe_created or not probe.check_password(SEED_PASSWORD):
        probe.password = make_password(SEED_PASSWORD)
        probe.is_active = True
        probe.save()
    note("user", probe.email, probe_created)

    _, probe_ws_created = WorkspaceMember.objects.update_or_create(
        workspace=workspace,
        member=probe,
        defaults={"role": 20, "is_active": True},
    )
    note("workspace-member", f"{workspace.slug}/{probe.email}", probe_ws_created)

    _, probe_project_created = ProjectMember.objects.update_or_create(
        workspace=workspace,
        project=project,
        member=probe,
        defaults={"role": 20, "is_active": True},
    )
    note("project-member", f"{project.identifier}/{probe.email}", probe_project_created)

    # Membership fixture: the project-settings member-write suite adds this
    # workspace member to the seeded project, re-roles and removes them. The
    # project membership is always deactivated on re-seed so the add path is
    # fresh and this user never joins the roster other suites assert on.
    membership_user, membership_created = User.objects.get_or_create(
        email=SEED_MEMBERSHIP_EMAIL,
        defaults={
            "username": "contract.membership",
            "first_name": "Contract",
            "last_name": "Membership",
            "is_active": True,
        },
    )
    if membership_created or not membership_user.check_password(SEED_PASSWORD):
        membership_user.password = make_password(SEED_PASSWORD)
        membership_user.is_active = True
        membership_user.save()
    note("user", membership_user.email, membership_created)

    _, membership_ws_created = WorkspaceMember.objects.update_or_create(
        workspace=workspace,
        member=membership_user,
        defaults={"role": 15, "is_active": True},
    )
    note("workspace-member", f"{workspace.slug}/{membership_user.email}", membership_ws_created)

    ProjectMember.objects.filter(
        workspace=workspace,
        project=project,
        member=membership_user,
    ).update(is_active=False)
    note("project-member", f"{project.identifier}/{membership_user.email}", False)

    # Views guest fixture (NEWFRONT-718). Repaired on every seed, like the
    # other consumable rows: the views contract suite signs in as this user
    # and asserts the project and workspace view lists narrow to its own.
    guest, guest_created = User.objects.get_or_create(
        email=SEED_GUEST_EMAIL,
        defaults={
            "username": "contract.guest",
            "first_name": "Contract",
            "last_name": "Guest",
            "is_active": True,
        },
    )
    if guest_created or not guest.check_password(SEED_PASSWORD):
        guest.password = make_password(SEED_PASSWORD)
        guest.is_active = True
        guest.save()
    note("user", guest.email, guest_created)

    _, guest_ws_created = WorkspaceMember.objects.update_or_create(
        workspace=workspace,
        member=guest,
        defaults={"role": 5, "is_active": True},
    )
    note("workspace-member", f"{workspace.slug}/{guest.email}", guest_ws_created)

    _, guest_project_created = ProjectMember.objects.update_or_create(
        workspace=workspace,
        project=project,
        member=guest,
        defaults={"role": 5, "is_active": True},
    )
    note("project-member", f"{project.identifier}/{guest.email}", guest_project_created)

    # Passwordless-style user: the set-password contract test signs in as
    # this user with the known seed password, then sets a fresh one — which
    # flips is_password_autoset off. The fixture is repaired (not just
    # created) so re-seeding restores it after a test run consumes it.
    autoset, autoset_created = User.objects.get_or_create(
        email=SEED_AUTOSET_EMAIL,
        defaults={
            "username": "contract.autoset",
            "first_name": "Contract",
            "last_name": "Autoset",
            "is_active": True,
            "is_password_autoset": True,
        },
    )
    if autoset_created or not autoset.check_password(SEED_PASSWORD) or not autoset.is_password_autoset:
        autoset.password = make_password(SEED_PASSWORD)
        autoset.is_active = True
        autoset.is_password_autoset = True
        autoset.save()
    note("user", autoset.email, autoset_created)

    # Outgoing-mail switch: the forgot-password contract proves the live
    # success path, which needs EMAIL_HOST set. The value is a dummy host —
    # sends only enqueue celery tasks (no worker runs in contract tests),
    # and no suite depends on mail being unconfigured.
    _, email_host_created = InstanceConfiguration.objects.get_or_create(
        key="EMAIL_HOST",
        defaults={"value": "contract-smtp.invalid", "category": "SMTP", "is_encrypted": False},
    )
    note("instance-config", "EMAIL_HOST", email_host_created)

    states = []
    for index, spec in enumerate(DEFAULT_STATES):
        # all_state_objects: the default manager hides triage-group rows,
        # so a plain get_or_create would duplicate Triage on re-seed.
        state, state_created = State.all_state_objects.get_or_create(
            workspace=workspace,
            project=project,
            name=spec["name"],
            defaults={
                "group": spec["group"],
                "color": spec["color"],
                "description": spec.get("description", ""),
                "sequence": spec.get("sequence", index),
                "default": spec.get("default", False),
            },
        )
        states.append(state)
        note("state", state.name, state_created)

    label, label_created = Label.objects.get_or_create(
        workspace=workspace,
        project=project,
        name="contract-bug",
        defaults={"color": "#e11d48", "description": "Seeded label."},
    )
    note("label", label.name, label_created)

    first_state = states[0]
    issues = []
    for name in ("Contract issue one", "Contract issue two"):
        issue, issue_created = Issue.objects.get_or_create(
            workspace=workspace,
            project=project,
            name=name,
            defaults={
                "state": first_state,
                "priority": "medium",
                "created_by": user,
                "updated_by": user,
            },
        )
        # BaseModel.save() clears audit fields when no request user is in
        # scope, so a get_or_create default does not survive the insert.
        # Repair the creator explicitly: the profile reads count it.
        if issue.created_by_id != user.id:
            Issue.objects.filter(pk=issue.pk).update(created_by_id=user.id)
            issue.refresh_from_db(fields=["created_by"])
        issues.append(issue)
        note("issue", issue.name, issue_created)

    from pi_dash.db.models import Notification, UserNotificationPreference  # noqa: E402

    # Notifications area (NEWFRONT-556): the inbox contract suite walks
    # one probe row through read/unread, archive/unarchive and
    # snooze/unsnooze, and flips one preference toggle and back. The
    # preference row is auto-created by the User post_save signal, so this
    # only repairs the five-flag baseline; the probe notification is
    # repaired (not just created) so a dead run self-heals on re-seed.
    UserNotificationPreference.objects.update_or_create(
        user=user,
        defaults={
            "property_change": True,
            "state_change": True,
            "comment": True,
            "mention": True,
            "issue_completed": True,
        },
    )
    note("notification-preference", user.email, False)

    probe_notification = (
        Notification.objects.filter(
            workspace=workspace,
            receiver=user,
            title="Contract notification probe",
        )
        .order_by("created_at")
        .first()
    )
    if probe_notification is None:
        probe_notification = Notification.objects.create(
            workspace=workspace,
            project=project,
            receiver=user,
            triggered_by=probe,
            entity_identifier=issues[0].pk,
            entity_name="issue",
            sender="in_app:issue_activities:subscribed",
            title="Contract notification probe",
            data={
                "issue": {
                    "id": str(issues[0].pk),
                    "name": issues[0].name,
                    "identifier": project.identifier,
                    "sequence_id": issues[0].sequence_id,
                    "state_name": first_state.name,
                    "state_group": first_state.group,
                },
                "issue_activity": {
                    "verb": "updated",
                    "field": "state",
                    "actor": str(probe.pk),
                    "new_value": first_state.name,
                    "old_value": "",
                    "issue_comment": "",
                    "old_identifier": None,
                    "new_identifier": None,
                },
            },
            message={"text": "Contract notification probe"},
        )
        note("notification", probe_notification.title, True)
    else:
        note("notification", probe_notification.title, False)
    if (
        probe_notification.read_at is not None
        or probe_notification.archived_at is not None
        or probe_notification.snoozed_till is not None
    ):
        probe_notification.read_at = None
        probe_notification.archived_at = None
        probe_notification.snoozed_till = None
        probe_notification.save(update_fields=["read_at", "archived_at", "snoozed_till"])
        note("notification-repaired", probe_notification.title, False)

    first_issue = issues[0]

    # Issues area (NEWFRONT-613): the activity-feed contract suite reads
    # history/?activity_type=issue-property. Live rows are written by
    # celery workers (none runs in contract tests), so the rows are
    # seeded directly. Read-only for every suite, so no repair needed.
    for verb, field, old_value, new_value in (
        ("created", "", "", ""),
        ("updated", "state", "Triage", "In Progress"),
        ("updated", "priority", "low", "high"),
    ):
        _, activity_created = IssueActivity.objects.get_or_create(
            workspace=workspace,
            project=project,
            issue=first_issue,
            verb=verb,
            field=field,
            old_value=old_value,
            new_value=new_value,
            defaults={"actor": user, "created_by": user, "updated_by": user},
        )
        note("issue-activity", f"{first_issue.name}/{verb}/{field or '-'}", activity_created)

    # Intake area (NEWFRONT-688): the intake-issues list 404s and create
    # crashes when the project has no intake row, so the seeded project
    # carries its default intake. Found by name (unique per project), so
    # enabling the intake view on a probe project never collides with it.
    intake, intake_created = Intake.objects.get_or_create(
        project=project,
        name="Contract Intake",
        defaults={"workspace": workspace, "is_default": True, "created_by": user, "updated_by": user},
    )
    note("intake", "Contract Intake", intake_created)

    # The versions suite reads an intake work item's history through the
    # shared work-items contract (INT-029): celery writes live version
    # rows and no worker runs in contract tests, so the intake row and
    # its version are seeded directly. Accepted status keeps it out of
    # every pending-list assertion; the fixed name keeps the probe sweep
    # (intake-probe- prefix) from touching it.
    version_issue, version_issue_created = Issue.objects.get_or_create(
        workspace=workspace,
        project=project,
        name="Contract intake version probe",
        defaults={"state": first_state, "priority": "medium", "created_by": user, "updated_by": user},
    )
    note("issue", version_issue.name, version_issue_created)
    _, version_intake_created = IntakeIssue.objects.get_or_create(
        intake=intake,
        issue=version_issue,
        defaults={"project": project, "workspace": workspace, "status": 1, "source": "IN_APP"},
    )
    note("intake-issue", version_issue.name, version_intake_created)
    _, version_version_created = IssueDescriptionVersion.objects.get_or_create(
        workspace=workspace,
        project=project,
        issue=version_issue,
        owned_by=user,
        description_html="<p>Contract intake version probe</p>",
        defaults={
            "description_stripped": "Contract intake version probe",
            "description_json": {},
            "created_by": user,
            "updated_by": user,
        },
    )
    note("description-version", version_issue.name, version_version_created)

    # Profile area (NEWFRONT-570): the segregation/stats reads count the
    # seed user's assigned, created and subscribed work, and the profile
    # issue tabs filter by those three dimensions. One assignment and one
    # subscription on the seeded issue keep every count non-zero; the
    # second issue is created by the seed user already. Both rows are
    # repaired (not just created) because they are soft-deleted by other
    # probe flows.
    from pi_dash.db.models import IssueAssignee, IssueSubscriber  # noqa: E402

    _, assignee_created = IssueAssignee.all_objects.update_or_create(
        issue=first_issue,
        assignee=user,
        deleted_at__isnull=True,
        defaults={"project": project, "workspace": workspace, "deleted_at": None},
    )
    note("issue-assignee", f"{first_issue.name}/{user.email}", assignee_created)
    _, subscriber_created = IssueSubscriber.all_objects.update_or_create(
        issue=first_issue,
        subscriber=user,
        deleted_at__isnull=True,
        defaults={"project": project, "workspace": workspace, "deleted_at": None},
    )
    note("issue-subscriber", f"{first_issue.name}/{user.email}", subscriber_created)

    # Upkeep (NEWFRONT-570): the auto-pm contract suite toggles one job
    # and the master pause. The instance catalog needs at least one enabled
    # job; a fixed slug keeps re-seeds idempotent.
    from pi_dash.db.models import LoopJob  # noqa: E402

    upkeep_job, upkeep_created = LoopJob.objects.get_or_create(
        slug="contract-upkeep",
        defaults={
            "name": "Contract upkeep",
            "public_name": "Contract upkeep",
            "public_description": "Seeded upkeep job for contract tests.",
            "prompt": "contract seed",
            "min_role": 15,
            "enabled": True,
            "is_builtin": False,
            "dtstart": timezone.now(),
            "rrule": "FREQ=DAILY",
            "tzid": "UTC",
        },
    )
    note("upkeep-job", upkeep_job.slug, upkeep_created)

    if label.pk and first_issue.pk:
        from pi_dash.db.models import IssueLabel  # noqa: E402

        _, link_created = IssueLabel.objects.get_or_create(
            issue=first_issue,
            label=label,
            defaults={"project": project},
        )
        note("issue-label", f"{first_issue.name}/{label.name}", link_created)

    from pi_dash.db.models import Cycle, Module, Page, ProjectPage  # noqa: E402

    # The cycles contracts (NEWFRONT-706) create, edit and archive cycles
    # through the project routes, and the API refuses those writes unless
    # the project enables cycles. Existing scratch workspaces are repaired
    # here rather than only on first creation.
    if not project.cycle_view:
        project.cycle_view = True
        project.save(update_fields=["cycle_view"])
    note("project-cycle-view", project.identifier, not project_created)

    cycle, cycle_created = Cycle.objects.get_or_create(
        workspace=workspace,
        project=project,
        name="Contract cycle",
        defaults={"owned_by": user},
    )
    note("cycle", cycle.name, cycle_created)

    # Transfer pair (NEWFRONT-611): the transfer-issues contract moves a
    # probe from source to target and back. Dedicated cycles — transfer
    # moves every open issue in the source, so sharing the seeded cycle
    # would race the updates suite's membership test.
    cycle_transfer_source, cycle_transfer_source_created = Cycle.objects.get_or_create(
        workspace=workspace,
        project=project,
        name="Contract transfer source",
        defaults={"owned_by": user},
    )
    note("cycle", cycle_transfer_source.name, cycle_transfer_source_created)
    cycle_target, cycle_target_created = Cycle.objects.get_or_create(
        workspace=workspace,
        project=project,
        name="Contract cycle target",
        defaults={"owned_by": user},
    )
    note("cycle", cycle_target.name, cycle_target_created)

    module, module_created = Module.objects.get_or_create(
        workspace=workspace,
        project=project,
        name="Contract module",
        defaults={"lead": user},
    )
    note("module", module.name, module_created)

    # Archives area (NEWFRONT-697): read fixtures stay archived, the
    # round-trip fixtures stay live with archivable gates (cycle end in
    # the past, module finished), and the upcoming cycle proves the
    # unfinished-cycle 400. Everything is repaired, not just created, so
    # re-seeding heals fixtures a dead run left mid-round-trip.
    from datetime import timedelta  # noqa: E402

    now = timezone.now()
    archived_cycle, archived_cycle_created = Cycle.objects.get_or_create(
        workspace=workspace,
        project=project,
        name="Contract archived cycle",
        defaults={
            "owned_by": user,
            "start_date": now - timedelta(days=30),
            "end_date": now - timedelta(days=1),
            "archived_at": now,
        },
    )
    if (
        archived_cycle.archived_at is None
        or archived_cycle.end_date is None
        or archived_cycle.end_date >= now
    ):
        archived_cycle.start_date = now - timedelta(days=30)
        archived_cycle.end_date = now - timedelta(days=1)
        archived_cycle.archived_at = now
        archived_cycle.save(update_fields=["start_date", "end_date", "archived_at"])
    note("cycle", archived_cycle.name, archived_cycle_created)

    live_cycle, live_cycle_created = Cycle.objects.get_or_create(
        workspace=workspace,
        project=project,
        name="Contract archive cycle",
        defaults={
            "owned_by": user,
            "start_date": now - timedelta(days=30),
            "end_date": now - timedelta(days=1),
        },
    )
    if (
        live_cycle.archived_at is not None
        or live_cycle.end_date is None
        or live_cycle.end_date >= now
    ):
        live_cycle.start_date = now - timedelta(days=30)
        live_cycle.end_date = now - timedelta(days=1)
        live_cycle.archived_at = None
        live_cycle.save(update_fields=["start_date", "end_date", "archived_at"])
    note("cycle", live_cycle.name, live_cycle_created)

    upcoming_cycle, upcoming_cycle_created = Cycle.objects.get_or_create(
        workspace=workspace,
        project=project,
        name="Contract upcoming cycle",
        defaults={
            "owned_by": user,
            "start_date": now + timedelta(days=1),
            "end_date": now + timedelta(days=30),
        },
    )
    if upcoming_cycle.archived_at is not None or upcoming_cycle.end_date is None or upcoming_cycle.end_date <= now:
        upcoming_cycle.start_date = now + timedelta(days=1)
        upcoming_cycle.end_date = now + timedelta(days=30)
        upcoming_cycle.archived_at = None
        upcoming_cycle.save(update_fields=["start_date", "end_date", "archived_at"])
    note("cycle", upcoming_cycle.name, upcoming_cycle_created)

    archived_module, archived_module_created = Module.objects.get_or_create(
        workspace=workspace,
        project=project,
        name="Contract archived module",
        defaults={
            "lead": user,
            "status": "completed",
            "start_date": (now - timedelta(days=30)).date(),
            "target_date": (now - timedelta(days=1)).date(),
            "archived_at": now,
        },
    )
    if archived_module.archived_at is None or archived_module.status not in ("completed", "cancelled"):
        archived_module.status = "completed"
        archived_module.start_date = (now - timedelta(days=30)).date()
        archived_module.target_date = (now - timedelta(days=1)).date()
        archived_module.archived_at = now
        archived_module.save(update_fields=["status", "start_date", "target_date", "archived_at"])
    note("module", archived_module.name, archived_module_created)

    live_module, live_module_created = Module.objects.get_or_create(
        workspace=workspace,
        project=project,
        name="Contract archive module",
        defaults={
            "lead": user,
            "status": "completed",
            "start_date": (now - timedelta(days=30)).date(),
            "target_date": (now - timedelta(days=1)).date(),
        },
    )
    if live_module.archived_at is not None or live_module.status not in ("completed", "cancelled"):
        live_module.status = "completed"
        live_module.start_date = (now - timedelta(days=30)).date()
        live_module.target_date = (now - timedelta(days=1)).date()
        live_module.archived_at = None
        live_module.save(update_fields=["status", "start_date", "target_date", "archived_at"])
    note("module", live_module.name, live_module_created)

    # Unfinished module proving the archive gate 400. Dedicated (not the
    # shared Contract module: the updates suite patches that one's status).
    planned_module, planned_module_created = Module.objects.get_or_create(
        workspace=workspace,
        project=project,
        name="Contract planned module",
        defaults={"lead": user, "status": "planned"},
    )
    if planned_module.archived_at is not None or planned_module.status in ("completed", "cancelled"):
        planned_module.status = "planned"
        planned_module.archived_at = None
        planned_module.save(update_fields=["status", "archived_at"])
    note("module", planned_module.name, planned_module_created)

    # One membership on the archived module so member_ids parses non-empty.
    from pi_dash.db.models import ModuleMember  # noqa: E402

    _, membership_created = ModuleMember.objects.get_or_create(
        module=archived_module,
        member=user,
        defaults={"workspace": workspace, "project": project},
    )
    note("module-member", f"{archived_module.name}/{user.email}", membership_created)
    # Analytics area (NEWFRONT-728): the created-vs-resolved read answers
    # an empty series for a cycle or module with no usable start date, so
    # the analytics contract suite needs both shapes. Dedicated January
    # 2026 scopes give the daily buckets a known range and dedicated
    # undated ones answer the empty series; the shared `Contract cycle` and
    # `Contract module` stay untouched (another suite renames them), so the
    # probe issue joins only the analytics scopes. The probe is filed in
    # January so the workspace monthly series has a bucket of its own. All
    # additive: get_or_create under contract names.
    from datetime import date, datetime, timezone as dt_timezone  # noqa: E402

    from pi_dash.db.models import CycleIssue, ModuleIssue  # noqa: E402

    analytics_cycle, analytics_cycle_created = Cycle.objects.get_or_create(
        workspace=workspace,
        project=project,
        name="Contract analytics cycle",
        defaults={
            "owned_by": user,
            "start_date": datetime(2026, 1, 1, tzinfo=dt_timezone.utc),
            "end_date": datetime(2026, 1, 31, tzinfo=dt_timezone.utc),
        },
    )
    note("cycle", analytics_cycle.name, analytics_cycle_created)

    analytics_module, analytics_module_created = Module.objects.get_or_create(
        workspace=workspace,
        project=project,
        name="Contract analytics module",
        defaults={
            "lead": user,
            "start_date": date(2026, 1, 1),
            "target_date": date(2026, 1, 31),
        },
    )
    note("module", analytics_module.name, analytics_module_created)

    analytics_undated_cycle, analytics_undated_cycle_created = Cycle.objects.get_or_create(
        workspace=workspace,
        project=project,
        name="Contract analytics undated cycle",
        defaults={"owned_by": user},
    )
    note("cycle", analytics_undated_cycle.name, analytics_undated_cycle_created)

    analytics_undated_module, analytics_undated_module_created = Module.objects.get_or_create(
        workspace=workspace,
        project=project,
        name="Contract analytics undated module",
        defaults={"lead": user},
    )
    note("module", analytics_undated_module.name, analytics_undated_module_created)

    analytics_issue, analytics_issue_created = Issue.objects.get_or_create(
        workspace=workspace,
        project=project,
        name="Contract analytics issue",
        defaults={
            "state": first_state,
            "priority": "high",
            "created_by": user,
            "updated_by": user,
        },
    )
    Issue.objects.filter(pk=analytics_issue.pk).update(
        created_by_id=user.id,
        created_at=datetime(2026, 1, 15, 12, 0, tzinfo=dt_timezone.utc),
    )
    note("issue", analytics_issue.name, analytics_issue_created)

    for scope_model, field, scope in (
        (CycleIssue, "cycle", analytics_cycle),
        (ModuleIssue, "module", analytics_module),
    ):
        _, scope_created = scope_model.objects.get_or_create(
            issue=analytics_issue,
            **{field: scope},
            defaults={"project": project, "workspace": workspace},
        )
        note(f"{field}-issue", f"{analytics_issue.name}/{scope.name}", scope_created)

    page, page_created = Page.objects.get_or_create(
        workspace=workspace,
        owned_by=user,
        name="Contract page",
    )
    note("page", page.name, page_created)
    _, project_page_created = ProjectPage.objects.get_or_create(
        workspace=workspace,
        project=project,
        page=page,
    )
    note("project-page", f"{project.identifier}/{page.name}", project_page_created)

    # Pages area (NEWFRONT-833): one stored snapshot of the seeded page so the
    # version list and the snapshot detail have a row to read in every run.
    from pi_dash.db.models import PageVersion  # noqa: E402

    _, page_version_created = PageVersion.objects.get_or_create(
        workspace=workspace,
        page=page,
        defaults={
            "owned_by": user,
            "description_html": "<p>Contract page snapshot</p>",
            "description_json": {"type": "doc", "content": [{"type": "paragraph"}]},
            "created_by": user,
            "updated_by": user,
        },
    )
    note("page-version", page.name, page_version_created)

    from pi_dash.db.models import Estimate, EstimatePoint  # noqa: E402

    estimate, estimate_created = Estimate.objects.get_or_create(
        workspace=workspace,
        project=project,
        name="Contract estimate",
        defaults={"type": "points", "last_used": True},
    )
    note("estimate", estimate.name, estimate_created)
    estimate_points = {}
    for key, value in ((1, "1"), (2, "2"), (3, "3")):
        point, point_created = EstimatePoint.objects.get_or_create(
            workspace=workspace,
            project=project,
            estimate=estimate,
            key=key,
            defaults={"value": value, "description": ""},
        )
        estimate_points[key] = point
        note("estimate-point", f"{estimate.name}/{value}", point_created)

    # The analytics custom chart keys the ESTIMATE_POINTS axis on
    # `estimate_point__key`, which the backend leaves as an integer (unlike
    # the id axes). Put the probe issue on point 2 so the contract suite
    # proves the numeric key parses and is counted.
    probe_point = estimate_points[2]
    if analytics_issue.estimate_point_id != probe_point.pk:
        Issue.objects.filter(pk=analytics_issue.pk).update(estimate_point_id=probe_point.pk)
        note("issue-estimate-point", f"{analytics_issue.name}/{probe_point.value}", False)

    # Instance-admin console (NEWFRONT-502): a dedicated admin user plus
    # the InstanceAdmin link the admins/me and admins-list contracts
    # prove, and the configuration key rows the configurations and email
    # contracts round-trip. PATCH only touches keys that already have
    # rows, so every key the contract tests write must exist here.
    # Everything is repaired (not just created) so re-seeding restores
    # the fixtures after a run blanks or consumes them.

    # Crash backstop for the sign-up dance: a run that dies mid-dance
    # leaves throwaway admin links behind and may leave the instance
    # renamed. Clear the orphans first so a fresh run starts from just
    # the two seed admin links (seed user plus dedicated admin).
    InstanceAdmin.objects.filter(user__email__startswith="contract-admin-signup-").delete()
    User.objects.filter(email__startswith="contract-admin-signup-").delete()
    if instance.instance_name != "contract-seed" or not instance.is_telemetry_enabled:
        instance.instance_name = "contract-seed"
        instance.is_telemetry_enabled = True
        instance.save(update_fields=["instance_name", "is_telemetry_enabled"])
        note("instance-restored", instance.pk, False)

    admin, admin_created = User.objects.get_or_create(
        email="contract.admin@example.com",
        defaults={
            "username": "contract.admin",
            "first_name": "Contract",
            "last_name": "Admin",
            "is_active": True,
        },
    )
    if admin_created or not admin.check_password(SEED_PASSWORD):
        admin.password = make_password(SEED_PASSWORD)
        admin.is_active = True
        admin.save()
    note("user", admin.email, admin_created)

    # Revived through all_objects: the sign-up dance soft-deletes this
    # link, and the unique pair blocks re-creating it, so a plain
    # get_or_create would crash on re-seed after a dance run.
    seed_link, admin_link_created = InstanceAdmin.all_objects.get_or_create(
        instance=instance,
        user=admin,
        defaults={"role": 20},
    )
    if seed_link.deleted_at is not None or seed_link.role != 20:
        seed_link.deleted_at = None
        seed_link.role = 20
        seed_link.save(update_fields=["deleted_at", "role"])
    note("instance-admin", admin.email, admin_link_created)

    # EMAIL_HOST repeats the earlier get_or_create on purpose: the
    # disable-email contract blanks it, and update_or_create repairs the
    # value on re-seed. LLM_API_KEY stays unencrypted here so the seed
    # needs no encryption key; the shape is identical either way.
    for key, value, category in (
        ("ENABLE_SMTP", "1", "SMTP"),
        ("EMAIL_HOST", "contract-smtp.invalid", "SMTP"),
        ("EMAIL_PORT", "1025", "SMTP"),
        ("EMAIL_FROM", "contract-admin@example.com", "SMTP"),
        ("EMAIL_HOST_USER", "contract-smtp-user", "SMTP"),
        ("EMAIL_HOST_PASSWORD", "contract-smtp-pass", "SMTP"),
        ("EMAIL_USE_TLS", "1", "SMTP"),
        ("EMAIL_USE_SSL", "0", "SMTP"),
        ("ENABLE_SIGNUP", "1", "AUTHENTICATION"),
        ("ENABLE_MAGIC_LINK_LOGIN", "1", "AUTHENTICATION"),
        ("ENABLE_EMAIL_PASSWORD", "1", "AUTHENTICATION"),
        ("IS_GOOGLE_ENABLED", "0", "AUTHENTICATION"),
        ("IS_GITHUB_ENABLED", "0", "AUTHENTICATION"),
        ("IS_GITLAB_ENABLED", "0", "AUTHENTICATION"),
        ("IS_GITEA_ENABLED", "0", "AUTHENTICATION"),
        ("GOOGLE_CLIENT_ID", "", "GOOGLE"),
        ("GOOGLE_CLIENT_SECRET", "", "GOOGLE"),
        ("ENABLE_GOOGLE_SYNC", "0", "GOOGLE"),
        ("GITHUB_CLIENT_ID", "", "GITHUB"),
        ("GITHUB_CLIENT_SECRET", "", "GITHUB"),
        ("GITHUB_ORGANIZATION_ID", "", "GITHUB"),
        ("ENABLE_GITHUB_SYNC", "0", "GITHUB"),
        ("GITLAB_HOST", "", "GITLAB"),
        ("GITLAB_CLIENT_ID", "", "GITLAB"),
        ("GITLAB_CLIENT_SECRET", "", "GITLAB"),
        ("ENABLE_GITLAB_SYNC", "0", "GITLAB"),
        ("GITEA_HOST", "", "GITEA"),
        ("GITEA_CLIENT_ID", "", "GITEA"),
        ("GITEA_CLIENT_SECRET", "", "GITEA"),
        ("ENABLE_GITEA_SYNC", "0", "GITEA"),
        ("DISABLE_WORKSPACE_CREATION", "0", "WORKSPACE_MANAGEMENT"),
        ("LLM_MODEL", "contract-model", "AI"),
        ("LLM_API_KEY", "contract-key-not-encrypted", "AI"),
    ):
        _, row_created = InstanceConfiguration.objects.update_or_create(
            key=key,
            defaults={"value": value, "category": category, "is_encrypted": False},
        )
        note("instance-config", key, row_created)

    from pi_dash.runner.models import (  # noqa: E402
        AgentChatApprovalRequest,
        AgentChatSession,
        AgentRun,
        AgentRunEvent,
        AgentRunStatus,
        ApprovalKind,
        ApprovalRequest,
        ApprovalStatus,
        DevMachine,
        MachineToken,
        Pod,
        Runner,
        RunnerStatus,
    )

    # Runners area (NEWFRONT-530): fixtures for the runners, pods,
    # dev-machines, runs and approvals contract suites. The suites only
    # read the keeper rows; every row a suite mutates is repaired here
    # (revoked flags cleared, consumed rows re-created) so re-seeding
    # restores first-run semantics after a full suite run.
    default_pod = Pod.objects.filter(project=project, is_default=True).first()
    if default_pod is None:
        default_pod = Pod.objects.filter(project=project).order_by("created_at").first()
    if default_pod is None:
        raise RuntimeError("contract seed: project has no pod; the default-pod signal did not fire")
    note("pod", default_pod.name, False)
    # Crash backstop for the pod lifecycle suite: a run that dies
    # mid-promote leaves a probe pod holding the default flag. Clear
    # first (exactly-one-default is a partial unique constraint), then
    # restore the auto-created default.
    Pod.objects.filter(project=project, is_default=True).exclude(pk=default_pod.pk).update(is_default=False)
    if not default_pod.is_default:
        default_pod.is_default = True
        default_pod.save(update_fields=["is_default"])
        note("pod-default-restored", default_pod.name, False)

    def seed_machine(label):
        machine = DevMachine.objects.filter(owner=user, label=label).order_by("created_at").first()
        if machine is None:
            machine = DevMachine.objects.create(owner=user, label=label, host_label=f"{label}-host")
            note("dev-machine", label, True)
        else:
            note("dev-machine", label, False)
        return machine

    keeper_machine = seed_machine("contract-machine")
    rotate_machine = seed_machine("contract-machine-rotate")
    revoke_machine = seed_machine("contract-machine-revoke")
    delete_machine = seed_machine("contract-machine-delete")
    if revoke_machine.revoked_at is not None:
        revoke_machine.revoked_at = None
        revoke_machine.save(update_fields=["revoked_at"])
        note("dev-machine-unrevoked", revoke_machine.label, False)

    def seed_token(machine):
        token = (
            MachineToken.objects.filter(user=user, workspace=workspace, dev_machine=machine)
            .order_by("created_at")
            .first()
        )
        if token is None:
            token = MachineToken.objects.create(
                user=user,
                workspace=workspace,
                dev_machine=machine,
                host_label=machine.host_label,
                token_hash=f"contract-seed-{machine.label}",
                label=machine.label,
            )
            note("machine-token", machine.label, True)
        elif token.revoked_at is not None:
            token.revoked_at = None
            token.save(update_fields=["revoked_at"])
            note("machine-token-repaired", machine.label, False)
        else:
            note("machine-token", machine.label, False)

    # Scope anchors: a machine is only visible (and only actionable)
    # in a workspace where it backs a runner or a token. The keeper
    # machine is anchored by the keeper runner below; the probe
    # machines are anchored by tokens.
    for probe_machine in (rotate_machine, revoke_machine, delete_machine):
        seed_token(probe_machine)

    def seed_runner(name, machine=None):
        runner = Runner.objects.filter(workspace=workspace, name=name).first()
        if runner is None:
            runner = Runner.objects.create(
                owner=user,
                workspace=workspace,
                pod=default_pod,
                name=name,
                dev_machine=machine,
            )
            note("runner", name, True)
        else:
            note("runner", name, False)
        return runner

    seed_runner("contract-runner", keeper_machine)
    revoke_runner = seed_runner("contract-runner-revoke")
    seed_runner("contract-runner-delete")
    if revoke_runner.revoked_at is not None:
        revoke_runner.revoked_at = None
        revoke_runner.revoked_reason = ""
        revoke_runner.status = RunnerStatus.OFFLINE
        revoke_runner.save(update_fields=["revoked_at", "revoked_reason", "status"])
        note("runner-unrevoked", revoke_runner.name, False)

    detail_run = AgentRun.objects.filter(workspace=workspace, prompt="contract-seed-run").first()
    if detail_run is None:
        detail_run = AgentRun.objects.create(
            workspace=workspace,
            created_by=user,
            pod=default_pod,
            prompt="contract-seed-run",
        )
        note("agent-run", "contract-seed-run", True)
    else:
        note("agent-run", "contract-seed-run", False)
    for seq, kind in ((1, "run_started"), (2, "agent_narrative")):
        _, event_created = AgentRunEvent.objects.get_or_create(
            agent_run=detail_run,
            seq=seq,
            defaults={"kind": kind, "payload": {"contract": True, "seq": seq}},
        )
        note("agent-run-event", f"contract-seed-run/{seq}", event_created)

    terminal = [
        AgentRunStatus.BLOCKED,
        AgentRunStatus.COMPLETED,
        AgentRunStatus.FAILED,
        AgentRunStatus.CANCELLED,
        AgentRunStatus.REFUSED,
    ]
    cancel_run = (
        AgentRun.objects.filter(workspace=workspace, prompt="contract-seed-run-cancel")
        .exclude(status__in=terminal)
        .first()
    )
    if cancel_run is None:
        cancel_run = AgentRun.objects.create(
            workspace=workspace,
            created_by=user,
            pod=default_pod,
            prompt="contract-seed-run-cancel",
        )
        note("agent-run", "contract-seed-run-cancel", True)
    else:
        note("agent-run", "contract-seed-run-cancel", False)

    approval = (
        ApprovalRequest.objects.filter(agent_run=detail_run, reason="contract approval probe")
        .order_by("-requested_at")
        .first()
    )
    if approval is None or approval.status != ApprovalStatus.PENDING:
        if approval is not None:
            approval.status = ApprovalStatus.PENDING
            approval.decision_source = ""
            approval.decided_by = None
            approval.decided_at = None
            approval.save(update_fields=["status", "decision_source", "decided_by", "decided_at"])
            note("approval-repaired", str(approval.pk), False)
        else:
            approval = ApprovalRequest.objects.create(
                agent_run=detail_run,
                kind=ApprovalKind.COMMAND_EXECUTION,
                payload={"command": "contract-probe"},
                reason="contract approval probe",
            )
            note("approval", str(approval.pk), True)
    else:
        note("approval", str(approval.pk), False)

    # Issues detail widgets (NEWFRONT-612): a sub-issue tree for the
    # children list/rollup contracts, an agent probe for the
    # agent-status read contract, and a description version for the
    # version-history contracts. Relations, links, attachments, drafts
    # and code reviews round-trip through the API in the suites, so
    # they need no rows here. Run-level writes (dispatch, cancel,
    # re-tick) seed in the runners area block above.
    from pi_dash.db.models.issue_agent_ticker import IssueAgentTicker  # noqa: E402
    from pi_dash.db.models.issue import IssueDescriptionVersion  # noqa: E402
    from pi_dash.runner.models import AgentRun, AgentRunStatus, Pod  # noqa: E402

    parent_issue, parent_created = Issue.objects.get_or_create(
        workspace=workspace,
        project=project,
        name="Contract sub-issue parent",
        defaults={
            "state": first_state,
            "priority": "medium",
            "created_by": user,
            "updated_by": user,
        },
    )
    note("issue", parent_issue.name, parent_created)

    # A second state group keeps the state_distribution rollup honest.
    child_states = [first_state, states[1] if len(states) > 1 else first_state]
    child_issues = []
    for child_name, child_state in (
        ("Contract sub-issue child A", child_states[0]),
        ("Contract sub-issue child B", child_states[1]),
    ):
        child, child_created = Issue.objects.get_or_create(
            workspace=workspace,
            project=project,
            name=child_name,
            defaults={
                "state": child_state,
                "priority": "medium",
                "created_by": user,
                "updated_by": user,
            },
        )
        if child.parent_id != parent_issue.pk or child.state_id != child_state.pk:
            child.parent = parent_issue
            child.state = child_state
            child.save(update_fields=["parent", "state"])
        child_issues.append(child)
        note("issue", child.name, child_created)

    grandchild, grandchild_created = Issue.objects.get_or_create(
        workspace=workspace,
        project=project,
        name="Contract sub-issue grandchild",
        defaults={
            "state": first_state,
            "priority": "low",
            "created_by": user,
            "updated_by": user,
        },
    )
    if grandchild.parent_id != child_issues[0].pk:
        grandchild.parent = child_issues[0]
        grandchild.save(update_fields=["parent"])
    note("issue", grandchild.name, grandchild_created)

    # Agent-status probe: a ticker plus one finished run carrying token
    # usage. The ticker pool is fresh (used 0 of 10).
    agent_issue, agent_created = Issue.objects.get_or_create(
        workspace=workspace,
        project=project,
        name="Contract agent probe",
        defaults={
            "state": first_state,
            "priority": "medium",
            "created_by": user,
            "updated_by": user,
        },
    )
    note("issue", agent_issue.name, agent_created)

    ticker, ticker_created = IssueAgentTicker.objects.get_or_create(issue=agent_issue)
    note("agent-ticker", agent_issue.name, ticker_created)

    pod = Pod.objects.filter(project=project, is_default=True, deleted_at__isnull=True).first()
    if pod is None:
        pod = Pod.objects.create(
            workspace=workspace, project=project, name="Contract default pod", is_default=True
        )
        note("pod", pod.name, True)
    else:
        note("pod", pod.name, False)

    finished_run = AgentRun.objects.filter(work_item=agent_issue).order_by("created_at").first()
    if finished_run is None:
        finished_run = AgentRun.objects.create(
            workspace=workspace,
            created_by=user,
            pod=pod,
            work_item=agent_issue,
            status=AgentRunStatus.COMPLETED,
            usage={"input": 1200, "output": 340, "total": 1540},
            llm_model="contract-model",
        )
        note("agent-run", f"{agent_issue.name}/completed", True)
    else:
        note("agent-run", f"{agent_issue.name}/completed", False)

    version, version_created = IssueDescriptionVersion.objects.get_or_create(
        workspace=workspace,
        project=project,
        issue=first_issue,
        owned_by=user,
        description_html="<p>Contract version probe</p>",
        defaults={
            "description_stripped": "Contract version probe",
            "description_json": {},
            "created_by": user,
            "updated_by": user,
        },
    )
    note("description-version", f"{first_issue.name}", version_created)

    # Schedulers area (NEWFRONT-568): one definition installed on the
    # seeded project with a single completed run behind it, so the
    # binding-runs contract proves a full AgentRun row (not just an empty
    # page) and the occurrences contract proves a past row. The binding's
    # last_run pointer is pinned so the joined last-run fields parse
    # non-null; next_run_at stays null to prove the nullable half.
    from pi_dash.db.models import Scheduler, SchedulerBinding  # noqa: E402

    runs_scheduler, runs_scheduler_created = Scheduler.objects.get_or_create(
        workspace=workspace,
        slug="contract-runs",
        defaults={
            "name": "Contract Runs",
            "description": "Seeded for the scheduler contract tests.",
            "prompt": "Contract seeded scheduler definition.",
            "color": "#3b82f6",
            "is_enabled": True,
        },
    )
    note("scheduler", runs_scheduler.slug, runs_scheduler_created)

    from datetime import timedelta  # noqa: E402

    runs_binding, runs_binding_created = SchedulerBinding.objects.get_or_create(
        scheduler=runs_scheduler,
        project=project,
        defaults={
            "workspace": workspace,
            "dtstart": timezone.now() - timedelta(days=1),
            "tzid": "UTC",
            "rrule": "FREQ=DAILY",
            "actor": user,
        },
    )
    note("scheduler-binding", str(runs_binding.pk), runs_binding_created)

    # default_pod is provided by the runners block above (NEWFRONT-530),
    # which guarantees the seeded project has exactly one default pod.

    seeded_run = AgentRun.objects.filter(scheduler_binding=runs_binding).order_by("created_at").first()
    if seeded_run is None:
        seeded_run = AgentRun.objects.create(
            workspace=workspace,
            created_by=user,
            pod=default_pod,
            scheduler_binding=runs_binding,
            status="completed",
            prompt="Contract seeded scheduler run.",
            started_at=timezone.now() - timedelta(hours=1),
            ended_at=timezone.now() - timedelta(minutes=50),
        )
        note("agent-run", str(seeded_run.pk), True)
    else:
        note("agent-run", str(seeded_run.pk), False)
    if runs_binding.last_run_id != seeded_run.pk:
        runs_binding.last_run = seeded_run
        runs_binding.save(update_fields=["last_run", "updated_at"])
        note("scheduler-binding-last-run", str(seeded_run.pk), False)

    # Runner chat (NEWFRONT-536): an online runner on the default pod plus
    # a probe session carrying pending chat approvals. The chat suites
    # create their own sessions through the API — accumulation is fine
    # because a create never reuses a session that already has messages —
    # but approvals have no creation endpoint, so the decide suite walks
    # the seeded pool (one accept per run; the 400 probe is never
    # consumed). The pool is repaired (not just created) so re-seeding
    # restores it after runs decide rows.
    pod = Pod.objects.filter(project=project, is_default=True, deleted_at__isnull=True).first()
    if pod is None:
        pod = Pod.objects.create(
            workspace=workspace,
            project=project,
            name=f"{project.identifier} Default",
            created_by=user,
            is_default=True,
        )
        note("pod", pod.name, True)
    else:
        note("pod", pod.name, False)

    runner, runner_created = Runner.objects.get_or_create(
        pod=pod,
        name="contract-chat-runner",
        defaults={"owner": user, "workspace": workspace, "status": RunnerStatus.ONLINE},
    )
    if not runner_created and (runner.status != RunnerStatus.ONLINE or runner.owner_id != user.pk):
        runner.status = RunnerStatus.ONLINE
        runner.owner = user
        runner.save(update_fields=["status", "owner", "updated_at"])
        note("runner-repaired", runner.name, False)
    else:
        note("runner", runner.name, runner_created)

    # The seed session stays the oldest on this runner: suites only add
    # newer ones, so first() is stable across runs without a marker.
    probe_session = (
        AgentChatSession.objects.filter(workspace=workspace, runner=runner, created_by=user)
        .order_by("created_at")
        .first()
    )
    if probe_session is None:
        probe_session = AgentChatSession.objects.create(
            workspace=workspace, runner=runner, created_by=user, pod=pod
        )
        note("chat-session", str(probe_session.pk), True)
    else:
        note("chat-session", str(probe_session.pk), False)

    for index in range(1, 6):
        local_id = f"contract-approval-{index}"
        approval, approval_created = AgentChatApprovalRequest.objects.get_or_create(
            session=probe_session,
            local_approval_id=local_id,
            defaults={
                "kind": ApprovalKind.COMMAND_EXECUTION,
                "payload": {"command": "contract-probe"},
                "reason": "Seeded probe.",
            },
        )
        if not approval_created and approval.status != ApprovalStatus.PENDING:
            approval.status = ApprovalStatus.PENDING
            approval.decision_source = ""
            approval.decided_by = None
            approval.decided_at = None
            approval.save(update_fields=["status", "decision_source", "decided_by", "decided_at"])
            note("chat-approval-repaired", local_id, False)
        else:
            note("chat-approval", local_id, approval_created)

    # Developer integration fixtures (NEWFRONT-533). The GitHub connect and
    # GitLab register endpoints verify credentials against the live provider
    # before storing anything, so the contract suites cannot create these
    # rows over HTTP. Seed them directly instead: a connected GitHub workspace
    # integration with a PAT account, plus a GitLab PAT account that a suite
    # disconnects. They live in their own workspace so the shared
    # contract-acme keeps the "no provider account" shape the project
    # repository suite (NEWFRONT-569) asserts there.
    dev_workspace, dev_workspace_created = Workspace.objects.get_or_create(
        slug=SEED_DEV_WORKSPACE_SLUG,
        defaults={"name": "Contract Dev", "owner": user},
    )
    note("workspace", dev_workspace.slug, dev_workspace_created)
    _, dev_membership_created = WorkspaceMember.objects.get_or_create(
        workspace=dev_workspace,
        member=user,
        defaults={"role": 20, "is_active": True},
    )
    note("workspace-member", f"{dev_workspace.slug}/{user.email}", dev_membership_created)

    from pi_dash.db.models import GitProviderAccount, Integration, WorkspaceIntegration
    from pi_dash.license.utils.encryption import encrypt_data

    github_integration, _ = Integration.objects.get_or_create(
        provider="github",
        defaults={
            "title": "GitHub",
            "verified": True,
            "description": {"summary": "Mirror GitHub issues into Pi Dash projects."},
        },
    )
    shim_token, _ = APIToken.objects.get_or_create(
        user=user,
        workspace=dev_workspace,
        label=f"github-integration-shim-{dev_workspace.id}",
        defaults={
            "user_type": 1,
            "is_active": False,
            "description": "GitHub integration FK shim; not an auth token.",
        },
    )
    github_ws, github_ws_created = WorkspaceIntegration.objects.update_or_create(
        workspace=dev_workspace,
        integration=github_integration,
        defaults={
            "actor": user,
            "api_token": shim_token,
            "config": {
                "auth_type": "pat",
                "token": encrypt_data("contract-github-pat"),
                "github_user_login": "contract-gh",
                "verified_at": "2026-01-02T03:04:05+00:00",
            },
        },
    )
    note("github-integration", str(github_ws.pk), github_ws_created)

    github_account, github_account_created = GitProviderAccount.objects.update_or_create(
        workspace=dev_workspace,
        provider=GitProviderAccount.Provider.GITHUB,
        host_url="https://github.com",
        external_account_login="contract-gh",
        defaults={
            "auth_type": GitProviderAccount.AuthType.PAT,
            "external_account_id": "533001",
            "display_name": "Contract GitHub",
            "capabilities": {
                "read_repositories": True,
                "read_issues": True,
                "write_comments": True,
                "manage_webhooks": True,
                "clone": True,
            },
            "workspace_integration": github_ws,
            "status": GitProviderAccount.Status.CONNECTED,
            "verified_at": "2026-01-02T03:04:05+00:00",
            "last_check_error": "",
        },
    )
    note("github-account", str(github_account.pk), github_account_created)

    gitlab_account, gitlab_account_created = GitProviderAccount.objects.update_or_create(
        workspace=dev_workspace,
        provider=GitProviderAccount.Provider.GITLAB,
        host_url="https://gitlab.com",
        external_account_login="contract-gitlab",
        defaults={
            "auth_type": GitProviderAccount.AuthType.PAT,
            "external_account_id": "533002",
            "display_name": "Contract GitLab",
            "capabilities": {
                "read_repositories": True,
                "read_issues": True,
                "write_comments": True,
                "manage_webhooks": True,
                "clone": False,
            },
            "status": GitProviderAccount.Status.CONNECTED,
            "verified_at": "2026-01-02T03:04:05+00:00",
            "last_check_error": "",
        },
    )
    note("gitlab-account", str(gitlab_account.pk), gitlab_account_created)

    return {
        "email": user.email,
        "workspace_slug": workspace.slug,
        "project_id": str(project.pk),
        "project_identifier": project.identifier,
        "issue_id": str(first_issue.pk),
        "rows": created,
    }


def main():
    parser = argparse.ArgumentParser(description="Seed scratch DB for api-client contract tests.")
    parser.add_argument("--apply", action="store_true", help="Write rows. Without it, only print the plan.")
    args = parser.parse_args()

    os.environ.setdefault("DJANGO_SETTINGS_MODULE", "pi_dash.settings.local")
    if os.getcwd() not in sys.path:
        sys.path.insert(0, os.getcwd())

    import django

    django.setup()

    target = database_target()
    print(f"database target: engine={target['engine']} name={target['name']} host={target['host']}:{target['port']}")

    blocked = refuse(target)
    if blocked:
        print(f"refusing: database name matches shared-database guard {blocked}")
        raise SystemExit(2)

    if not args.apply:
        print("dry run: pass --apply to write seed rows")
        print(f"would seed user={SEED_EMAIL} workspace={SEED_WORKSPACE_SLUG} project={SEED_PROJECT_IDENTIFIER}")
        raise SystemExit(0)

    result = seed()
    for label, key, was_created in result["rows"]:
        print(f"  {'created' if was_created else 'exists '} {label:<18} {key}")
    print(f"seed handle: workspace={result['workspace_slug']} project={result['project_id']} issue={result['issue_id']}")


if __name__ == "__main__":
    main()
