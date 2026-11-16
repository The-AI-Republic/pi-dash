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


# -- Api class (PIDASHCONV-81, work-items surface). Kept alongside the URL
# builders above (never a fork): same X-Api-Key auth, same API_PREFIX root.
from . import env  # noqa: E402

API = "/api/v1"


class Api:
    def __init__(self, api_key: str, base_url: str | None = None):
        self.client = httpx.Client(
            base_url=base_url or env.BASE_URL,
            headers={"X-Api-Key": api_key, "Content-Type": "application/json"},
            timeout=30,
        )

    # -- work items --------------------------------------------------------

    def wi(self, slug: str, project_id: str, pk: str) -> str:
        return f"{API}/workspaces/{slug}/projects/{project_id}/work-items/{pk}/"

    def get_issue(self, slug: str, project_id: str, pk: str) -> httpx.Response:
        return self.client.get(self.wi(slug, project_id, pk))

    def patch_issue(
        self, slug: str, project_id: str, pk: str, payload: dict, run_id: str | None = None
    ) -> httpx.Response:
        headers = {"X-Pi-Dash-Run-Id": run_id} if run_id else None
        return self.client.patch(self.wi(slug, project_id, pk), json=payload, headers=headers)

    def post_wait(self, slug: str, project_id: str, pk: str, run_id: str | None = None) -> httpx.Response:
        headers = {"X-Pi-Dash-Run-Id": run_id} if run_id else None
        return self.client.post(self.wi(slug, project_id, pk) + "wait/", headers=headers)

    def post_retick(self, slug: str, project_id: str, pk: str) -> httpx.Response:
        return self.client.post(self.wi(slug, project_id, pk) + "re-tick/")

    def post_run_ai(self, slug: str, project_id: str, pk: str) -> httpx.Response:
        return self.client.post(self.wi(slug, project_id, pk) + "run-ai/")

    # -- workpad -----------------------------------------------------------

    def get_workpad(self, slug: str, project_id: str, pk: str) -> httpx.Response:
        return self.client.get(self.wi(slug, project_id, pk) + "workpad/")

    def patch_workpad(self, slug: str, project_id: str, pk: str, payload: dict) -> httpx.Response:
        return self.client.patch(self.wi(slug, project_id, pk) + "workpad/", json=payload)

    # -- relations ---------------------------------------------------------

    def relate(self, slug: str, project_id: str, pk: str, relation_type: str, issues: list[str]) -> httpx.Response:
        return self.client.post(
            self.wi(slug, project_id, pk) + "relations/relate/",
            json={"relation_type": relation_type, "issues": issues},
        )

    def unrelate(self, slug: str, project_id: str, pk: str, relation_type: str, issues: list[str]) -> httpx.Response:
        return self.client.post(
            self.wi(slug, project_id, pk) + "relations/unrelate/",
            json={"relation_type": relation_type, "issues": issues},
        )

    def grouped_relations(self, slug: str, project_id: str, pk: str) -> httpx.Response:
        return self.client.get(self.wi(slug, project_id, pk) + "relations/grouped/")

    # -- lookups -----------------------------------------------------------

    def list_states(self, slug: str, project_id: str) -> httpx.Response:
        return self.client.get(f"{API}/workspaces/{slug}/projects/{project_id}/states/")
