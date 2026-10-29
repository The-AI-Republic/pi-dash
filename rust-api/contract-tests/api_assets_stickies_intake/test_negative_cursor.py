"""Negative-page cursor on the intake-issue and sticky lists (PIDASHCONV-1407).

A cursor with a negative page (e.g. the ``prev_cursor`` the server itself
issues on page 0, ``<per_page>:-1:1``) makes Django's ``OffsetPaginator``
raise ``BadPaginationError``, which ``BasePaginator.paginate`` turns into
``ParseError(detail="Error in parsing")`` — a 400
(``apps/api/pi_dash/utils/paginator.py:143-149,696-699``). Both lists must
answer exactly that instead of a 500.
"""


def _intake_base(org):
    return (
        f"/api/v1/workspaces/{org.workspace['slug']}"
        f"/projects/{org.project['id']}/intake-issues/"
    )


def _sticky_base(org):
    return f"/api/v1/workspaces/{org.workspace['slug']}/stickies/"


def test_intake_list_negative_page_cursor_is_400(org):
    r = org.request("GET", _intake_base(org), "member", params={"cursor": "1000:-1:1"})
    assert r.status_code == 400
    assert r.json() == {"detail": "Error in parsing"}


def test_sticky_list_negative_page_cursor_is_400(org):
    r = org.request("GET", _sticky_base(org), "member", params={"cursor": "20:-1:1"})
    assert r.status_code == 400
    assert r.json() == {"detail": "Error in parsing"}
