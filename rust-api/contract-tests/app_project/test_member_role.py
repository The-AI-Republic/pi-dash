"""Who may change a project member's role (PDASHOSS01-328).

Authority comes from the requester's workspace role. The target's workspace
role only ever denies: nobody else can change a workspace admin's role, and a
workspace guest cannot hold project role Member or Admin.
"""

import pytest

ADMIN, MEMBER, GUEST = 20, 15, 5

OWN_ROLE = {"error": "You cannot update your own role"}
NOT_ADMIN = {"error": "You do not have permission to update roles"}
TARGET_TOO_HIGH = {
    "error": "You cannot update the role of a member with a role equal to or higher than your own"
}
ASSIGN_TOO_HIGH = {"error": "You cannot assign a role equal to or higher than your own"}
GUEST_CAP = {"error": "You cannot add a user with role higher than the workspace role"}
WORKSPACE_ADMIN_TARGET = {"error": "You cannot update the role of a workspace admin"}


@pytest.fixture()
def stage(world, db):
    _, _, ws, project = world.full_stack()

    class Stage:
        def add(self, name, ws_role, proj_role):
            user, client = world.add_member(ws, project, ws_role=ws_role, proj_role=proj_role, name=name)
            with db.cursor() as cur:
                cur.execute(
                    "SELECT id FROM project_members WHERE project_id=%s AND member_id=%s",
                    (str(project["id"]), str(user["id"])),
                )
                member_pk = cur.fetchone()[0]
            db.commit()
            return client, member_pk

        def patch_role(self, client, member_pk, role):
            return client.patch(
                f"/api/workspaces/{ws['slug']}/projects/{project['id']}/members/{member_pk}/",
                json={"role": role},
            )

        def role_of(self, member_pk):
            with db.cursor() as cur:
                cur.execute("SELECT role FROM project_members WHERE id=%s", (member_pk,))
                role = cur.fetchone()[0]
            db.commit()
            return role

        def assert_refused(self, resp, status, body, member_pk, role):
            assert resp.status_code == status, resp.text[:300]
            assert resp.json() == body
            assert self.role_of(member_pk) == role

        def assert_changed(self, resp, member_pk, role):
            assert resp.status_code == 200, resp.text[:300]
            assert resp.json()["role"] == role
            assert self.role_of(member_pk) == role

    return Stage()


@pytest.mark.parametrize(
    "ws_role, proj_role",
    [
        pytest.param(MEMBER, MEMBER, id="plain-member"),
        pytest.param(GUEST, GUEST, id="guest"),
        pytest.param(MEMBER, ADMIN, id="project-admin"),
        pytest.param(ADMIN, ADMIN, id="other-workspace-admin"),
        pytest.param(ADMIN, MEMBER, id="other-workspace-admin-project-member"),
    ],
)
@pytest.mark.parametrize("target_role", [ADMIN, MEMBER], ids=["target-admin", "target-member"])
def test_nobody_else_changes_a_workspace_admins_role(stage, ws_role, proj_role, target_role):
    client, _ = stage.add("requester", ws_role, proj_role)
    _, target = stage.add("target", ADMIN, target_role)
    resp = stage.patch_role(client, target, GUEST)
    stage.assert_refused(resp, 403, WORKSPACE_ADMIN_TARGET, target, target_role)


@pytest.mark.parametrize("proj_role", [ADMIN, MEMBER, GUEST], ids=["admin", "member", "guest"])
def test_workspace_admin_changes_their_own_role(stage, proj_role):
    client, own = stage.add("selfadmin", ADMIN, proj_role)
    new_role = MEMBER if proj_role != MEMBER else ADMIN
    stage.assert_changed(stage.patch_role(client, own, new_role), own, new_role)


@pytest.mark.parametrize("proj_role", [ADMIN, MEMBER, GUEST], ids=["admin", "member", "guest"])
def test_workspace_admin_promotes_a_member_to_project_admin(stage, proj_role):
    client, _ = stage.add("wsadmin", ADMIN, proj_role)
    _, target = stage.add("target", MEMBER, MEMBER)
    stage.assert_changed(stage.patch_role(client, target, ADMIN), target, ADMIN)


def test_workspace_admin_project_member_demotes_a_project_admin(stage):
    client, _ = stage.add("wsadmin", ADMIN, MEMBER)
    _, target = stage.add("target", MEMBER, ADMIN)
    stage.assert_changed(stage.patch_role(client, target, GUEST), target, GUEST)


def test_project_admin_cannot_promote_to_admin(stage):
    client, _ = stage.add("projadmin", MEMBER, ADMIN)
    _, target = stage.add("target", MEMBER, MEMBER)
    resp = stage.patch_role(client, target, ADMIN)
    stage.assert_refused(resp, 403, ASSIGN_TOO_HIGH, target, MEMBER)


def test_project_admin_cannot_change_another_project_admin(stage):
    client, _ = stage.add("projadmin", MEMBER, ADMIN)
    _, target = stage.add("target", MEMBER, ADMIN)
    resp = stage.patch_role(client, target, MEMBER)
    stage.assert_refused(resp, 403, TARGET_TOO_HIGH, target, ADMIN)


def test_project_admin_changes_a_lower_member(stage):
    client, _ = stage.add("projadmin", MEMBER, ADMIN)
    _, target = stage.add("target", MEMBER, MEMBER)
    stage.assert_changed(stage.patch_role(client, target, GUEST), target, GUEST)


@pytest.mark.parametrize(
    "ws_role, proj_role",
    [pytest.param(MEMBER, MEMBER, id="member"), pytest.param(GUEST, GUEST, id="guest")],
)
def test_non_admin_cannot_change_roles(stage, ws_role, proj_role):
    client, _ = stage.add("requester", ws_role, proj_role)
    _, target = stage.add("target", MEMBER, MEMBER)
    resp = stage.patch_role(client, target, GUEST)
    stage.assert_refused(resp, 403, NOT_ADMIN, target, MEMBER)


@pytest.mark.parametrize("proj_role", [ADMIN, MEMBER], ids=["project-admin", "member"])
def test_non_workspace_admin_cannot_change_their_own_role(stage, proj_role):
    client, own = stage.add("selfmember", MEMBER, proj_role)
    resp = stage.patch_role(client, own, GUEST)
    stage.assert_refused(resp, 400, OWN_ROLE, own, proj_role)


@pytest.mark.parametrize("new_role", [ADMIN, MEMBER], ids=["to-admin", "to-member"])
def test_workspace_admin_is_capped_by_a_guest_target(stage, new_role):
    client, _ = stage.add("wsadmin", ADMIN, MEMBER)
    _, target = stage.add("target", GUEST, GUEST)
    resp = stage.patch_role(client, target, new_role)
    stage.assert_refused(resp, 400, GUEST_CAP, target, GUEST)


def test_project_admin_is_capped_by_a_guest_target(stage):
    client, _ = stage.add("projadmin", MEMBER, ADMIN)
    _, target = stage.add("target", GUEST, GUEST)
    resp = stage.patch_role(client, target, MEMBER)
    stage.assert_refused(resp, 400, GUEST_CAP, target, GUEST)
