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
SEED_PASSWORD = os.environ.get("PIDASH_CONTRACT_PASSWORD", "Contract123!")
SEED_WORKSPACE_SLUG = "contract-acme"
SEED_PROJECT_IDENTIFIER = "CT"


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
        Issue,
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
        issues.append(issue)
        note("issue", issue.name, issue_created)

    first_issue = issues[0]
    if label.pk and first_issue.pk:
        from pi_dash.db.models import IssueLabel  # noqa: E402

        _, link_created = IssueLabel.objects.get_or_create(
            issue=first_issue,
            label=label,
            defaults={"project": project},
        )
        note("issue-label", f"{first_issue.name}/{label.name}", link_created)

    from pi_dash.db.models import Cycle, Module, Page, ProjectPage  # noqa: E402

    cycle, cycle_created = Cycle.objects.get_or_create(
        workspace=workspace,
        project=project,
        name="Contract cycle",
        defaults={"owned_by": user},
    )
    note("cycle", cycle.name, cycle_created)

    module, module_created = Module.objects.get_or_create(
        workspace=workspace,
        project=project,
        name="Contract module",
        defaults={"lead": user},
    )
    note("module", module.name, module_created)

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

    from pi_dash.db.models import Estimate, EstimatePoint  # noqa: E402

    estimate, estimate_created = Estimate.objects.get_or_create(
        workspace=workspace,
        project=project,
        name="Contract estimate",
        defaults={"type": "points", "last_used": True},
    )
    note("estimate", estimate.name, estimate_created)
    for key, value in ((1, "1"), (2, "2"), (3, "3")):
        _, point_created = EstimatePoint.objects.get_or_create(
            workspace=workspace,
            project=project,
            estimate=estimate,
            key=key,
            defaults={"value": value, "description": ""},
        )
        note("estimate-point", f"{estimate.name}/{value}", point_created)

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
