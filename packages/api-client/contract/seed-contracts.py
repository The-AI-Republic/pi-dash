"""Seed a scratch Django database for the @pidash/api-client contract tests.

Run from apps/api with the project's virtualenv::

    python /path/to/packages/api-client/contract/seed-contracts.py --apply

The script prints its database target first and refuses names that look
shared (production, staging, ...). Without --apply it only prints what it
would create. Re-running is safe: every row is created with get_or_create
under a ``contract-`` prefix.

Required environment (same as a local runserver)::

    DATABASE_URL, REDIS_URL, WEB_URL, APP_BASE_URL, DJANGO_SETTINGS_MODULE
"""

import argparse
import os
import sys

REFUSALS = ("prod", "production", "staging", "main", "shared")

SEED_EMAIL = "contract.tester@example.com"
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
    from pi_dash.license.models.instance import Instance  # noqa: E402

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

    states = []
    for index, spec in enumerate(DEFAULT_STATES):
        state, state_created = State.objects.get_or_create(
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
