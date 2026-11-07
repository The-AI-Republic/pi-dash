# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

# PDASHOSS01-359 (github_* sunset 6/6, Part 2): backfill any
# github_pull_request_links written after migration 0152 that are not yet
# mirrored in git_code_review_links, so Django reads can switch to the
# generic table without losing links. Dual-write has covered the common
# attach/webhook paths since the generic rollout, so this is a gap-filler:
# idempotent via the generic link's provider/host/namespace/repo/iid
# identity (matching utils.github_pr_links._matching_code_review_link).
# Rows whose generic sibling already exists are left untouched.

from django.db import migrations


GITHUB_HOST = "https://github.com"


def backfill_github_pr_links(apps, schema_editor):
    GitRepository = apps.get_model("db", "GitRepository")
    GitCodeReviewLink = apps.get_model("db", "GitCodeReviewLink")
    GithubPullRequestLink = apps.get_model("db", "GithubPullRequestLink")

    for old in GithubPullRequestLink.objects.all():
        namespace = (old.repo_owner or "").lower()
        repo_name = (old.repo_name or "").lower()
        external_iid = str(old.pr_number)
        already = GitCodeReviewLink.objects.filter(
            provider="github",
            host_url=GITHUB_HOST,
            namespace=namespace,
            repo_name=repo_name,
            external_iid=external_iid,
        ).exists()
        if already:
            continue
        repo_external_id = ""
        repo = GitRepository.objects.filter(
            provider="github",
            host_url=GITHUB_HOST,
            namespace=namespace,
            name=repo_name,
        ).first()
        if repo is not None:
            repo_external_id = repo.external_id or ""
        GitCodeReviewLink.objects.create(
            workspace_id=old.workspace_id,
            project_id=old.project_id,
            issue_id=old.issue_id,
            provider="github",
            host_url=GITHUB_HOST,
            namespace=namespace,
            repo_name=repo_name,
            repo_external_id=repo_external_id,
            external_id="",
            external_iid=external_iid,
            url=old.url,
            title=old.title,
            state="closed" if old.state == "closed" else "open",
            merged=old.merged,
            draft=old.draft,
            remote_updated_at=old.pr_updated_at,
            metadata={"legacy_github_pull_request_link_id": str(old.id)},
            created_by_id=old.created_by_id,
            updated_by_id=old.updated_by_id,
        )


def reverse_backfill(apps, schema_editor):
    pass


class Migration(migrations.Migration):
    dependencies = [
        ("db", "0172_clidevicecode_engine_scope"),
    ]

    operations = [
        migrations.RunPython(backfill_github_pr_links, reverse_backfill),
    ]
