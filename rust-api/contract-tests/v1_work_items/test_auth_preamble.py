"""Contract: auth-preamble timezone edges for the work-items domain (PIDASHCONV-786).

``handlers_social.rs`` (PIDASHCONV-674) and ``handlers_pr_links.rs``
(PIDASHCONV-680) merged carrying the pre-PIDASHCONV-747
``activate_timezone``: a stored empty ``user_timezone`` (``''``) answered
400 where Python 500s — ``zoneinfo.ZoneInfo('')`` raises ``ValueError``
(not ``KeyError``), which falls through ``handle_exception`` to the
generic 500 (``api/views/base.py:166-171``), while unknown zones 400 via
``ZoneInfoNotFoundError`` (a ``KeyError``).

``''`` is unreachable via the API (``choices=pytz.common_timezones``),
so these tests seed the zone straight into ``users`` with SQL, like the
737/747 pins in ``v1_projects/test_auth_preamble.py`` do. One route per
carrier is pinned (links list for social, pull-request list for
pr_links); every other route in each file shares the same
``activate_timezone`` call, including the two ``*_destroy_inner``
handlers in ``handlers_pr_links.rs`` (PIDASHCONV-787), which are pinned
below via create-then-break-the-zone-then-delete.
"""

import httpx
import pytest

pytestmark = pytest.mark.contract

KEY_ERROR_BODY = {"error": "The required key does not exist."}
SERVER_ERROR_BODY = {"error": "Something went wrong please try again later"}
GATE_DENIAL_BODY = {"detail": "You do not have permission to perform this action."}
BAD_ZONE = "Not/AZone"
EMPTY_ZONE = ""


def _links(world):
    slug = world["workspace"]["slug"]
    project_id = world["project"]["id"]
    issue_id = world["issue"]["id"]
    return f"/api/v1/workspaces/{slug}/projects/{project_id}/work-items/{issue_id}/links/"


def _pull_requests(world):
    slug = world["workspace"]["slug"]
    project_id = world["project"]["id"]
    issue_id = world["issue"]["id"]
    return (
        f"/api/v1/workspaces/{slug}/projects/{project_id}"
        f"/work-items/{issue_id}/github/pull-requests/"
    )


def _code_reviews(world):
    slug = world["workspace"]["slug"]
    project_id = world["project"]["id"]
    issue_id = world["issue"]["id"]
    return (
        f"/api/v1/workspaces/{slug}/projects/{project_id}"
        f"/work-items/{issue_id}/code-reviews/"
    )


def _create_pr_link(api, world):
    import random

    pr_number = random.randint(100000, 999999)
    created = api.post(
        _pull_requests(world),
        json={
            "repo_owner": "acme",
            "repo_name": "web",
            "pr_number": pr_number,
            "url": f"https://github.com/acme/web/pull/{pr_number}",
            "title": "Fix it",
            "state": "open",
            "merged": False,
            "draft": False,
        },
    )
    assert created.status_code == 201, created.text
    return created.json()["id"]


def _create_review_link(api, world):
    import random

    external_iid = str(random.randint(100000, 999999))
    created = api.post(
        _code_reviews(world),
        json={
            "provider": "github",
            "host_url": "https://github.com",
            "namespace": "acme",
            "repo_name": "web",
            "repo_external_id": "",
            "external_id": f"mr-{external_iid}",
            "external_iid": external_iid,
            "url": f"https://github.com/acme/web/pull/{external_iid}",
            "title": "Review me",
            "state": "open",
            "merged": False,
            "draft": False,
        },
    )
    assert created.status_code == 201, created.text
    return created.json()["id"]


def _set_zone(db, user_id, zone):
    db.execute("UPDATE users SET user_timezone = %s WHERE id = %s", (zone, user_id))


def _outsider_client(seeder, settings, workspace_id, zone):
    outsider = seeder.create_user()
    token = seeder.create_api_token(outsider["id"], workspace_id)
    seeder.db.execute(
        "UPDATE users SET user_timezone = %s WHERE id = %s", (zone, outsider["id"])
    )
    return httpx.Client(
        base_url=settings.base_url, timeout=30, headers={"X-Api-Key": token["token"]}
    )


def test_empty_zone_member_500_links(api, db, world):
    _set_zone(db, world["owner"]["id"], EMPTY_ZONE)
    response = api.get(_links(world))
    assert response.status_code == 500, response.text
    assert response.json() == SERVER_ERROR_BODY


def test_empty_zone_member_500_pull_requests(api, db, world):
    _set_zone(db, world["owner"]["id"], EMPTY_ZONE)
    response = api.get(_pull_requests(world))
    assert response.status_code == 500, response.text
    assert response.json() == SERVER_ERROR_BODY


def test_empty_zone_denied_caller_403(seeder, settings, world):
    # The permission gate runs before `TimezoneMixin.initial`: an empty
    # zone + denied caller 403s on both carriers.
    with _outsider_client(seeder, settings, world["workspace"]["id"], EMPTY_ZONE) as client:
        for path in (_links(world), _pull_requests(world)):
            response = client.get(path)
            assert response.status_code == 403, f"{path} -> {response.status_code}"
            assert response.json() == GATE_DENIAL_BODY


def test_invalid_zone_member_400_links(api, db, world):
    _set_zone(db, world["owner"]["id"], BAD_ZONE)
    response = api.get(_links(world))
    assert response.status_code == 400, response.text
    assert response.json() == KEY_ERROR_BODY


def test_invalid_zone_member_400_pull_requests(api, db, world):
    _set_zone(db, world["owner"]["id"], BAD_ZONE)
    response = api.get(_pull_requests(world))
    assert response.status_code == 400, response.text
    assert response.json() == KEY_ERROR_BODY


def test_empty_zone_member_500_pr_delete(api, db, world):
    link_id = _create_pr_link(api, world)
    _set_zone(db, world["owner"]["id"], EMPTY_ZONE)
    response = api.delete(f"{_pull_requests(world)}{link_id}/")
    assert response.status_code == 500, response.text
    assert response.json() == SERVER_ERROR_BODY


def test_empty_zone_member_500_review_delete(api, db, world):
    link_id = _create_review_link(api, world)
    _set_zone(db, world["owner"]["id"], EMPTY_ZONE)
    response = api.delete(f"{_code_reviews(world)}{link_id}/")
    assert response.status_code == 500, response.text
    assert response.json() == SERVER_ERROR_BODY


def test_invalid_zone_member_400_pr_delete(api, db, world):
    link_id = _create_pr_link(api, world)
    _set_zone(db, world["owner"]["id"], BAD_ZONE)
    response = api.delete(f"{_pull_requests(world)}{link_id}/")
    assert response.status_code == 400, response.text
    assert response.json() == KEY_ERROR_BODY


def test_invalid_zone_member_400_review_delete(api, db, world):
    link_id = _create_review_link(api, world)
    _set_zone(db, world["owner"]["id"], BAD_ZONE)
    response = api.delete(f"{_code_reviews(world)}{link_id}/")
    assert response.status_code == 400, response.text
    assert response.json() == KEY_ERROR_BODY


def test_empty_zone_denied_caller_403_delete(api, seeder, settings, world):
    # The permission gate runs before `TimezoneMixin.initial`: an empty
    # zone + denied caller 403s on both DELETE routes.
    pr_id = _create_pr_link(api, world)
    review_id = _create_review_link(api, world)
    with _outsider_client(seeder, settings, world["workspace"]["id"], EMPTY_ZONE) as client:
        for path in (
            f"{_pull_requests(world)}{pr_id}/",
            f"{_code_reviews(world)}{review_id}/",
        ):
            response = client.delete(path)
            assert response.status_code == 403, f"{path} -> {response.status_code}"
            assert response.json() == GATE_DENIAL_BODY
