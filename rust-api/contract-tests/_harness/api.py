"""HTTP helpers for contract tests: httpx client + api-v1 URL builders."""

import os

import httpx

API_PREFIX = "/api/v1"


def base_url():
    return os.environ["BASE_URL"].rstrip("/")


def client_for(token):
    """Fresh httpx client authed as the given API token (X-Api-Key)."""
    return httpx.Client(
        base_url=base_url(),
        headers={"X-Api-Key": token},
        timeout=30.0,
    )


def anon_client():
    return httpx.Client(base_url=base_url(), timeout=30.0)


def cycles_url(slug, project_id):
    return f"{API_PREFIX}/workspaces/{slug}/projects/{project_id}/cycles/"


def cycle_detail_url(slug, project_id, cycle_id):
    return f"{API_PREFIX}/workspaces/{slug}/projects/{project_id}/cycles/{cycle_id}/"


def cycle_issues_url(slug, project_id, cycle_id):
    return (
        f"{API_PREFIX}/workspaces/{slug}/projects/{project_id}"
        f"/cycles/{cycle_id}/cycle-issues/"
    )


def cycle_issue_detail_url(slug, project_id, cycle_id, issue_id):
    return (
        f"{API_PREFIX}/workspaces/{slug}/projects/{project_id}"
        f"/cycles/{cycle_id}/cycle-issues/{issue_id}/"
    )


def cycle_transfer_url(slug, project_id, cycle_id):
    return (
        f"{API_PREFIX}/workspaces/{slug}/projects/{project_id}"
        f"/cycles/{cycle_id}/transfer-issues/"
    )


def cycle_archive_url(slug, project_id, cycle_id):
    return (
        f"{API_PREFIX}/workspaces/{slug}/projects/{project_id}"
        f"/cycles/{cycle_id}/archive/"
    )


def archived_cycles_url(slug, project_id):
    return f"{API_PREFIX}/workspaces/{slug}/projects/{project_id}/archived-cycles/"


def cycle_unarchive_url(slug, project_id, cycle_id):
    return (
        f"{API_PREFIX}/workspaces/{slug}/projects/{project_id}"
        f"/archived-cycles/{cycle_id}/unarchive/"
    )


def modules_url(slug, project_id):
    return f"{API_PREFIX}/workspaces/{slug}/projects/{project_id}/modules/"


def module_detail_url(slug, project_id, module_id):
    return f"{API_PREFIX}/workspaces/{slug}/projects/{project_id}/modules/{module_id}/"


def module_issues_url(slug, project_id, module_id):
    return (
        f"{API_PREFIX}/workspaces/{slug}/projects/{project_id}"
        f"/modules/{module_id}/module-issues/"
    )


def module_issue_detail_url(slug, project_id, module_id, issue_id):
    return (
        f"{API_PREFIX}/workspaces/{slug}/projects/{project_id}"
        f"/modules/{module_id}/module-issues/{issue_id}/"
    )


def module_archive_url(slug, project_id, module_id):
    return (
        f"{API_PREFIX}/workspaces/{slug}/projects/{project_id}"
        f"/modules/{module_id}/archive/"
    )


def archived_modules_url(slug, project_id):
    return f"{API_PREFIX}/workspaces/{slug}/projects/{project_id}/archived-modules/"


def module_unarchive_url(slug, project_id, module_id):
    return (
        f"{API_PREFIX}/workspaces/{slug}/projects/{project_id}"
        f"/archived-modules/{module_id}/unarchive/"
    )
