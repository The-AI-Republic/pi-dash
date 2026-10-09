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
``activate_timezone`` call except the two ``*_destroy_inner`` handlers
in ``handlers_pr_links.rs``, which return 204 without rendering times
and never call it (PIDASHCONV-787).
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
