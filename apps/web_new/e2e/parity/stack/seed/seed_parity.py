# Parity stack seed (NEWFRONT-19).
#
# Runs inside the stack's own api container through `manage.py shell`
# (piped on stdin, so this file is never imported by the backend and the
# backend itself stays untouched). Converges one deterministic workspace:
# a single onboarded owner, one project, and three named issues. Every
# step is get-or-create, so reruns converge instead of duplicating; the
# only rows ever replaced are the project's issues. Prints one
# `PARITY_SEED_JSON:` line; the caller saves it as the seed-facts file
# the scenarios read.
#
# NEWFRONT-113 adds, still get-or-create and scoped to the parity project:
# a guest member (CMT-002), a published project board plus intake view
# (CMT-006, CMT-018), and one git-synced comment (CMT-005).
import json
import os
import time

from django.contrib.auth.hashers import make_password
from django.db import connection, transaction

from pi_dash.db.models import (
    DeployBoard,
    GitCommentSync,
    GitIssueSync,
    GitProviderAccount,
    GitRepository,
    GitRepositoryBinding,
    Intake,
    IntakeIssue,
    Issue,
    IssueActivity,
    IssueComment,
    IssueSequence,
    Profile,
    Project,
    ProjectMember,
    ProjectUserProperty,
    State,
    User,
    Workspace,
    WorkspaceMember,
)
from pi_dash.license.models import Instance, InstanceAdmin, InstanceConfiguration

EMAIL = os.environ.get("PARITY_SEED_EMAIL", "parity-oracle@example.com")
PASSWORD = os.environ.get("PARITY_SEED_PASSWORD", "Parity-Seed-1")
# Second workspace member (NEWFRONT-115): the user the oracle mention
# scenarios @-mention. A plain member (not admin) so the suggestion list,
# the rendered chip and the notification fan-out all resolve to someone
# other than the author.
MENTION_EMAIL = os.environ.get("PARITY_SEED_MENTION_EMAIL", "parity-mention@example.com")
MENTION_PASSWORD = os.environ.get("PARITY_SEED_MENTION_PASSWORD", "Parity-Seed-2")
# NEWFRONT-113: second seeded identity, a guest on the seeded project, for
# the CMT-002 refusal path (guest may only comment on their own item unless
# the project lets guests use everything).
GUEST_EMAIL = os.environ.get("PARITY_SEED_GUEST_EMAIL", "parity-guest@example.com")
GUEST_PASSWORD = os.environ.get("PARITY_SEED_GUEST_PASSWORD", "Parity-Seed-2")
WORKSPACE_NAME = "Parity Workspace"
WORKSPACE_SLUG = "parity-ws"
PROJECT_NAME = "Parity Project"
PROJECT_IDENTIFIER = "PAR"
ISSUE_NAMES = ["Parity first issue", "Parity second issue", "Parity third issue"]
# Marker HTML for the git-synced comment (CMT-005). The scenario finds it by
# this marker instead of position, so sibling specs can post freely.
SYNC_COMMENT_MARKER = "parity-git-synced seven"


# Every table with a foreign key into issues, mapped to the columns that
# carry the reference (read off the scratch schema). Sibling scenarios
# attach rows (comments, reactions, agent runs, ...) to the seeded issues,
# so the refresh must clear those children first or the issue delete
# violates foreign keys.
ISSUE_CHILD_COLUMNS = {
    "agent_run": ["work_item_id"],
    "cycle_issues": ["issue_id"],
    "draft_issues": ["parent_id"],
    "file_assets": ["issue_id"],
    "git_code_review_links": ["issue_id"],
    "git_issue_syncs": ["issue_id"],
    "github_issue_syncs": ["issue_id"],
    "github_pull_request_links": ["issue_id"],
    "intake_issues": ["duplicate_to_id", "issue_id"],
    "issue_activities": ["issue_id"],
    "issue_agent_ticker": ["issue_id"],
    "issue_assignees": ["issue_id"],
    "issue_attachments": ["issue_id"],
    "issue_blockers": ["block_id", "blocked_by_id"],
    "issue_comments": ["issue_id"],
    "issue_description_versions": ["issue_id"],
    "issue_labels": ["issue_id"],
    "issue_links": ["issue_id"],
    "issue_mentions": ["issue_id"],
    "issue_reactions": ["issue_id"],
    "issue_relations": ["issue_id", "related_issue_id"],
    "issue_sequences": ["issue_id"],
    "issue_subscribers": ["issue_id"],
    "issue_versions": ["issue_id"],
    "issue_votes": ["issue_id"],
    "module_issues": ["issue_id"],
}


def refresh_project_issues(project, workspace, state, user) -> None:
    # Plain SQL: the models soft-delete and protect their parents in ways
    # that defeat ORM deletes, and this is the parity scratch database
    # (parity19-pg), so direct deletes of exactly this project's issue rows
    # are safe.
    with connection.cursor() as cursor:
        # Oracle comment/reaction scenarios post on these issues, so clear
        # every per-issue dependent row before replacing the issues
        # themselves; otherwise a reseed collides with leftover comments
        # (NEWFRONT-115: first hit by a mention probe comment).
        for table in (
            "issue_reactions",
            "issue_comments",
            "issue_mentions",
            "issue_subscribers",
            "issue_activities",
            "issue_sequences",
        ):
            cursor.execute(
                f"DELETE FROM {table} WHERE issue_id IN (SELECT id FROM issues WHERE project_id = %s)",
                [str(project.id)],
            )
        # Comment reactions hang off the comment, not the issue.
        cursor.execute(
            "DELETE FROM comment_reactions WHERE comment_id IN "
            "(SELECT id FROM issue_comments WHERE issue_id IN "
            "(SELECT id FROM issues WHERE project_id = %s))",
            [str(project.id)],
        )
        cursor.execute("DELETE FROM issues WHERE project_id = %s", [str(project.id)])
    # 107's wider FK closure follows (superset of the tables above); both
    # deletes are idempotent so keeping both sides is safe.
    # are safe. Deferred constraints cover references between the cleared
    # tables themselves, so only completeness (not order) matters.
    pid = str(project.id)
    with transaction.atomic():
        with connection.cursor() as cursor:
            cursor.execute("SET CONSTRAINTS ALL DEFERRED")
            # Grandchildren first (rows pointing at the child rows below).
            cursor.execute(
                """DELETE FROM issue_versions WHERE activity_id IN
                   (SELECT id FROM issue_activities WHERE issue_id IN
                    (SELECT id FROM issues WHERE project_id = %s))""",
                [pid],
            )
            cursor.execute(
                """DELETE FROM run_message_dedupe WHERE run_id IN
                   (SELECT id FROM agent_run WHERE work_item_id IN
                    (SELECT id FROM issues WHERE project_id = %s))""",
                [pid],
            )
            cursor.execute(
                """DELETE FROM scheduler_bindings WHERE last_run_id IN
                   (SELECT id FROM agent_run WHERE work_item_id IN
                    (SELECT id FROM issues WHERE project_id = %s))""",
                [pid],
            )
            cursor.execute(
                """DELETE FROM comment_reactions WHERE comment_id IN
                   (SELECT id FROM issue_comments WHERE issue_id IN
                    (SELECT id FROM issues WHERE project_id = %s))""",
                [pid],
            )
            for sync_table in ("git_comment_syncs", "github_comment_syncs"):
                sync_parent = "git_issue_syncs" if sync_table == "git_comment_syncs" else "github_issue_syncs"
                cursor.execute(
                    f"""DELETE FROM {sync_table} WHERE issue_sync_id IN
                        (SELECT id FROM {sync_parent} WHERE issue_id IN
                         (SELECT id FROM issues WHERE project_id = %s)) OR comment_id IN
                        (SELECT id FROM issue_comments WHERE issue_id IN
                         (SELECT id FROM issues WHERE project_id = %s))""",
                    [pid, pid],
                )
            for draft_child in (
                "draft_issue_assignees",
                "draft_issue_cycles",
                "draft_issue_labels",
                "draft_issue_modules",
            ):
                cursor.execute(
                    f"""DELETE FROM {draft_child} WHERE draft_issue_id IN
                        (SELECT id FROM draft_issues WHERE parent_id IN
                         (SELECT id FROM issues WHERE project_id = %s))""",
                    [pid],
                )
            # Shared entities may point at our file assets (cover/logo/avatar);
            # release those references instead of deleting the shared rows.
            our_assets = """SELECT id FROM file_assets WHERE issue_id IN
                            (SELECT id FROM issues WHERE project_id = %s)"""
            cursor.execute(
                f"UPDATE projects SET cover_image_asset_id = NULL WHERE cover_image_asset_id IN ({our_assets})",
                [pid],
            )
            cursor.execute(
                f"""UPDATE users SET avatar_asset_id = NULL, cover_image_asset_id = NULL
                    WHERE avatar_asset_id IN ({our_assets}) OR cover_image_asset_id IN ({our_assets})""",
                [pid, pid],
            )
            cursor.execute(
                f"UPDATE workspaces SET logo_asset_id = NULL WHERE logo_asset_id IN ({our_assets})",
                [pid],
            )
            # Every direct child of the project's issues.
            for table, columns in ISSUE_CHILD_COLUMNS.items():
                condition = " OR ".join(
                    f"{column} IN (SELECT id FROM issues WHERE project_id = %s)" for column in columns
                )
                cursor.execute(f"DELETE FROM {table} WHERE {condition}", [pid] * len(columns))
            cursor.execute("DELETE FROM issues WHERE project_id = %s", [pid])
            # NEWFRONT-113: every row that references this project's issues or
            # their comments first (leaf tables before parents), otherwise any
            # row an earlier scenario run stored blocks the issue rebuild with
            # a foreign-key violation on reruns that skip parity-reset.sh.
            # Table list enumerated from pg_constraint on the scratch database.
            pid = str(project.id)
            issue_set = "(SELECT id FROM issues WHERE project_id = %s)"
            comment_set = "(SELECT id FROM issue_comments WHERE issue_id IN " + issue_set + ")"
            for table, column in [
                ("git_comment_syncs", "comment_id"),
                ("github_comment_syncs", "comment_id"),
                ("comment_reactions", "comment_id"),
                ("file_assets", "comment_id"),
                ("issue_activities", "issue_comment_id"),
                ("git_issue_syncs", "issue_id"),
                ("github_issue_syncs", "issue_id"),
                ("file_assets", "issue_id"),
                ("issue_activities", "issue_id"),
                ("issue_sequences", "issue_id"),
                ("issue_labels", "issue_id"),
                ("issue_assignees", "issue_id"),
                ("issue_subscribers", "issue_id"),
                ("issue_reactions", "issue_id"),
                ("issue_votes", "issue_id"),
                ("issue_mentions", "issue_id"),
                ("issue_attachments", "issue_id"),
                ("issue_links", "issue_id"),
                ("issue_blockers", "blocked_by_id"),
                ("issue_blockers", "block_id"),
                ("issue_relations", "related_issue_id"),
                ("issue_relations", "issue_id"),
                ("cycle_issues", "issue_id"),
                ("module_issues", "issue_id"),
                ("intake_issues", "issue_id"),
                ("intake_issues", "duplicate_to_id"),
                ("issue_versions", "issue_id"),
                ("issue_description_versions", "issue_id"),
                ("issue_agent_ticker", "issue_id"),
                ("git_code_review_links", "issue_id"),
                ("github_pull_request_links", "issue_id"),
                ("agent_run", "work_item_id"),
            ]:
                scope = comment_set if column in ("comment_id", "issue_comment_id") else issue_set
                cursor.execute(f"DELETE FROM {table} WHERE {column} IN {scope}", [pid] * scope.count("%s"))
            # Threaded replies before their parents (specs never thread deeper).
            cursor.execute(
                "DELETE FROM issue_comments WHERE parent_id IN " + comment_set,
                [pid],
            )
            cursor.execute(
                "DELETE FROM issue_comments WHERE issue_id IN " + issue_set,
                [pid],
            )
            # Sub-issues before their parents; drafts pointing at project issues.
            cursor.execute("DELETE FROM draft_issues WHERE parent_id IN " + issue_set, [pid])
            cursor.execute(
                "DELETE FROM issues WHERE parent_id IN " + issue_set + " AND project_id = %s",
                [pid, pid],
            )
            cursor.execute("DELETE FROM issues WHERE project_id = %s", [pid])
    for position, name in enumerate(ISSUE_NAMES):
        issue = Issue(
            workspace=workspace,
            project=project,
            state=state,
            name=name,
            sequence_id=position + 1,
            sort_order=float(10000 * (position + 1)),
            created_by_id=user.id,
        )
        issue.save(created_by_id=user.id, disable_auto_set_user=True)
        IssueSequence.objects.create(
            issue=issue, project=project, workspace_id=workspace.id, created_by_id=user.id
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


def build() -> dict:
    user, created = User.objects.get_or_create(
        email=EMAIL,
        defaults={
            "username": "parity_oracle",
            "password": make_password(PASSWORD),
            "display_name": "Parity Oracle",
            "first_name": "Parity",
            "last_name": "Oracle",
            "is_active": True,
            "is_email_verified": True,
        },
    )
    if not created:
        user.password = make_password(PASSWORD)
        user.is_active = True
        user.is_email_verified = True
        user.save(update_fields=["password", "is_active", "is_email_verified"])
    profile, _ = Profile.objects.get_or_create(user=user)
    profile.is_onboarded = True
    profile.save(update_fields=["is_onboarded"])
    # Oracle scenarios flip instance auth flags in-spec through the
    # configurations API (and restore them after), which needs an instance
    # admin. Grant it to the seed owner; a license-table row changes nothing
    # about the sign-in or issues behavior other scenarios prove.
    instance = Instance.objects.first()
    if instance is not None:
        InstanceAdmin.objects.get_or_create(
            instance=instance, user=user, defaults={"role": 20, "is_verified": True}
        )
    # Oracle scenarios flip these flags in-spec through the configurations
    # API, which only updates rows that already exist. Ensure every flippable
    # auth flag has a row (configure_instance does not seed them all).
    for key, value in [
        ("EMAIL_HOST", ""),
        ("ENABLE_EMAIL_PASSWORD", "1"),
        ("ENABLE_MAGIC_LINK_LOGIN", "1"),
        ("IS_GOOGLE_ENABLED", "0"),
        ("IS_GITHUB_ENABLED", "0"),
        ("IS_GITLAB_ENABLED", "0"),
        ("IS_GITEA_ENABLED", "0"),
    ]:
        InstanceConfiguration.objects.get_or_create(
            key=key, defaults={"value": value, "category": "authentication"}
        )

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

    # Second member for the mention scenarios (NEWFRONT-115). Converges
    # like the owner above; a plain member so @-mention targets resolve
    # to someone other than the comment author.
    mention_user, mention_created = User.objects.get_or_create(
        email=MENTION_EMAIL,
        defaults={
            "username": "parity_mention",
            "password": make_password(MENTION_PASSWORD),
            "display_name": "Parity Mention",
            "first_name": "Parity",
            "last_name": "Mention",
            "is_active": True,
            "is_email_verified": True,
        },
    )
    if not mention_created:
        mention_user.password = make_password(MENTION_PASSWORD)
        mention_user.is_active = True
        mention_user.is_email_verified = True
        mention_user.save(update_fields=["password", "is_active", "is_email_verified"])
    mention_profile, _ = Profile.objects.get_or_create(user=mention_user)
    mention_profile.is_onboarded = True
    mention_profile.save(update_fields=["is_onboarded"])
    WorkspaceMember.objects.get_or_create(
        workspace=workspace, member=mention_user, defaults={"role": 15}
    )
    Profile.objects.filter(user=mention_user).update(last_workspace_id=workspace.id)

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
    ProjectMember.objects.get_or_create(
        project=project,
        member=mention_user,
        defaults={"role": 15, "workspace_id": workspace.id, "created_by_id": user.id},
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

    refresh_project_issues(project, workspace, state, user)
    inbox_issue_id = seed_rules_fixtures(project, workspace, state, user)

    facts = {
        "email": EMAIL,
        "password": PASSWORD,
        "workspaceSlug": WORKSPACE_SLUG,
        "workspaceName": WORKSPACE_NAME,
        "projectId": str(project.id),
        "projectName": PROJECT_NAME,
        "issueNames": list(ISSUE_NAMES),
        # Second member for the mention scenarios (NEWFRONT-115). Existing
        # readers ignore unknown fields, so older scenarios keep working.
        "mentionMember": {
            "email": MENTION_EMAIL,
            "password": MENTION_PASSWORD,
            "id": str(mention_user.id),
            "displayName": mention_user.display_name,
        },
        "guestEmail": GUEST_EMAIL,
        "guestPassword": GUEST_PASSWORD,
    }
    if inbox_issue_id is not None:
        facts["inboxIssueId"] = inbox_issue_id
    return facts


def seed_rules_fixtures(project, workspace, state, user) -> None:
    # NEWFRONT-113 additions. Everything here is get-or-create and scoped to
    # the parity project, so other areas' scenarios are unaffected.
    guest, guest_created = User.objects.get_or_create(
        email=GUEST_EMAIL,
        defaults={
            "username": "parity_guest",
            "password": make_password(GUEST_PASSWORD),
            "display_name": "Parity Guest",
            "first_name": "Parity",
            "last_name": "Guest",
            "is_active": True,
            "is_email_verified": True,
        },
    )
    if not guest_created:
        guest.password = make_password(GUEST_PASSWORD)
        guest.is_active = True
        guest.is_email_verified = True
        guest.save(update_fields=["password", "is_active", "is_email_verified"])
    guest_profile, _ = Profile.objects.get_or_create(user=guest)
    guest_profile.is_onboarded = True
    guest_profile.save(update_fields=["is_onboarded"])
    WorkspaceMember.objects.get_or_create(
        workspace=workspace, member=guest, defaults={"role": 5}
    )
    ProjectMember.objects.get_or_create(
        project=project,
        member=guest,
        defaults={"role": 5, "workspace_id": workspace.id, "created_by_id": user.id},
    )
    Profile.objects.filter(user=guest).update(last_workspace_id=workspace.id)

    # Externally shared project (CMT-006): a published project board gives
    # the project a public anchor, which is what renders the visibility
    # toggle, badge and overflow switch.
    DeployBoard.objects.get_or_create(
        entity_name="project",
        entity_identifier=project.id,
        defaults={"workspace_id": workspace.id, "created_by_id": user.id},
    )
    # Intake triage needs the intake view switched on (CMT-018).
    if not project.intake_view:
        project.intake_view = True
        project.save(update_fields=["intake_view"])

    # Git-synced comment (CMT-005): a comment bound to a repository through
    # the sync chain, so deletes refuse with 409 until the repo is unbound.
    # By name: sequence counters keep growing across reseeds, so the
    # seeded issues are not sequence 1..3 after the first reset cycle.
    first_issue = Issue.objects.filter(project=project, name=ISSUE_NAMES[0]).first()
    if first_issue is None:
        return
    comment, _ = IssueComment.objects.get_or_create(
        issue=first_issue,
        project=project,
        workspace=workspace,
        comment_html=f"<p>{SYNC_COMMENT_MARKER}</p>",
        defaults={
            "actor_id": user.id,
            "created_by_id": user.id,
            # is_synced is only true with an external source plus a sync row.
            "external_source": "github",
            "external_id": "parity-seed-comment-7",
        },
    )
    if not comment.external_source:
        comment.external_source = "github"
        comment.external_id = "parity-seed-comment-7"
        comment.save(update_fields=["external_source", "external_id"])
    account, _ = GitProviderAccount.objects.get_or_create(
        workspace=workspace,
        provider="github",
        host_url="https://github.com",
        defaults={"auth_type": "pat", "created_by_id": user.id},
    )
    repository, _ = GitRepository.objects.get_or_create(
        provider="github",
        host_url="https://github.com",
        namespace="parity",
        name="parity-seed",
        full_name="parity/parity-seed",
        defaults={"created_by_id": user.id},
    )
    binding, _ = GitRepositoryBinding.objects.get_or_create(
        repository=repository,
        project=project,
        defaults={
            "workspace_id": workspace.id,
            "provider_account": account,
            "actor_id": user.id,
            "created_by_id": user.id,
        },
    )
    issue_sync, _ = GitIssueSync.objects.get_or_create(
        binding=binding,
        issue=first_issue,
        defaults={
            "workspace_id": workspace.id,
            "project_id": project.id,
            "provider": "github",
            "external_iid": "7",
            "created_by_id": user.id,
        },
    )
    GitCommentSync.objects.get_or_create(
        issue_sync=issue_sync,
        comment=comment,
        defaults={
            "workspace_id": workspace.id,
            "project_id": project.id,
            "provider": "github",
            "external_id": "parity-seed-comment-7",
            "created_by_id": user.id,
        },
    )

    # Intake triage (CMT-018): the intake screen renders inbox issues, so
    # the project gets an intake and the first issue a pending triage row.
    intake, _ = Intake.objects.get_or_create(
        project=project,
        name="Parity Intake",
        defaults={"workspace_id": workspace.id, "created_by_id": user.id},
    )
    inbox_issue, _ = IntakeIssue.objects.get_or_create(
        intake=intake,
        issue=first_issue,
        defaults={
            "workspace_id": workspace.id,
            "project_id": project.id,
            "status": -2,
            "created_by_id": user.id,
        },
    )
    return str(inbox_issue.id)


facts = build()
print("PARITY_SEED_JSON:" + json.dumps(facts))
