# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""Builders for assistant tests.

Constructs a workspace with users at every role and two projects so the
access-control parity matrix (admin/member/guest/non-member/other-workspace)
can be exercised against both the tools and the endpoints.
"""

from __future__ import annotations

import types
from uuid import uuid4

import pytest
from botocore.exceptions import ClientError
from django.utils import timezone

from pi_dash.assistant import crypto
from pi_dash.assistant.models import UserLLMConfig
from pi_dash.assistant.runtime.deps import AssistantDeps
from pi_dash.db.models import (
    Issue,
    Project,
    ProjectMember,
    User,
    Workspace,
    WorkspaceMember,
)
from pi_dash.db.models.state import State

ROLE_ADMIN, ROLE_MEMBER, ROLE_GUEST = 20, 15, 5


def _user(email: str) -> User:
    u = User.objects.create(
        email=email, username=email, first_name=email.split("@")[0]
    )
    u.set_password("x")
    u.save()
    return u


def _workspace(slug: str, owner: User) -> Workspace:
    return Workspace.objects.create(id=uuid4(), name=slug, slug=slug, owner=owner)


def _add_member(ws: Workspace, user: User, role: int) -> None:
    WorkspaceMember.objects.create(workspace=ws, member=user, role=role, is_active=True)


def _project(ws: Workspace, owner: User, name: str, identifier: str) -> Project:
    return Project.objects.create(
        id=uuid4(), name=name, identifier=identifier, workspace=ws, created_by=owner, updated_by=owner
    )


def _add_project_member(project: Project, user: User, role: int) -> None:
    ProjectMember.objects.create(
        project=project, workspace=project.workspace, member=user, role=role, is_active=True
    )


def _state(project: Project, name: str, group: str, *, default: bool = False, seq: float = 1.0) -> State:
    return State.objects.create(
        id=uuid4(),
        name=name,
        group=group,
        project=project,
        workspace=project.workspace,
        default=default,
        sequence=seq,
    )


def _issue(project: Project, creator: User, name: str, *, seq: int = 1, state: State | None = None) -> Issue:
    issue = Issue(
        id=uuid4(),
        name=name,
        project=project,
        workspace=project.workspace,
        sequence_id=seq,
        state=state,
    )
    # Pass created_by_id explicitly — BaseModel.save() overrides created_by from
    # crum's current user (None in tests) unless created_by_id is given.
    issue.save(created_by_id=creator.id)
    return issue


def make_deps(user: User, ws: Workspace, role: int, *, thread_id=None, turn_id=None) -> AssistantDeps:
    return AssistantDeps(
        user_id=user.id,
        user_display=user.display_name or user.email,
        workspace_id=ws.id,
        workspace_slug=ws.slug,
        workspace_name=ws.name,
        workspace_role=role,
        thread_id=thread_id or uuid4(),
        turn_id=turn_id or uuid4(),
    )


def fake_ctx(deps: AssistantDeps):
    """Minimal stand-in for pydantic-ai RunContext — tools only read ctx.deps."""
    return types.SimpleNamespace(deps=deps)


@pytest.fixture
def world(db):
    """A workspace with users at every role and two projects."""
    admin = _user(f"admin-{uuid4().hex[:6]}@e.com")
    member = _user(f"member-{uuid4().hex[:6]}@e.com")
    guest = _user(f"guest-{uuid4().hex[:6]}@e.com")
    outsider = _user(f"out-{uuid4().hex[:6]}@e.com")  # workspace member, not project member

    ws = _workspace(f"ws-{uuid4().hex[:6]}", admin)
    _add_member(ws, admin, ROLE_ADMIN)
    _add_member(ws, member, ROLE_MEMBER)
    _add_member(ws, guest, ROLE_GUEST)
    _add_member(ws, outsider, ROLE_MEMBER)

    # other workspace + user (cross-tenant isolation)
    other_user = _user(f"other-{uuid4().hex[:6]}@e.com")
    other_ws = _workspace(f"otherws-{uuid4().hex[:6]}", other_user)
    _add_member(other_ws, other_user, ROLE_ADMIN)

    proj_a = _project(ws, admin, "Alpha", "ALP")
    proj_b = _project(ws, admin, "Beta", "BET")
    for u, r in ((admin, ROLE_ADMIN), (member, ROLE_MEMBER), (guest, ROLE_GUEST)):
        _add_project_member(proj_a, u, r)
    _add_project_member(proj_b, admin, ROLE_ADMIN)  # only admin in B

    todo = _state(proj_a, "Todo", "unstarted", default=True, seq=1)
    in_progress = _state(proj_a, "In Progress", "started", seq=2)

    issue_a = _issue(proj_a, admin, "Fix login", seq=1, state=todo)
    guest_issue = _issue(proj_a, guest, "Guest reported bug", seq=2, state=todo)
    issue_b = _issue(proj_b, admin, "Beta only", seq=1, state=None)

    return types.SimpleNamespace(
        ws=ws,
        other_ws=other_ws,
        admin=admin,
        member=member,
        guest=guest,
        outsider=outsider,
        other_user=other_user,
        proj_a=proj_a,
        proj_b=proj_b,
        todo=todo,
        in_progress=in_progress,
        issue_a=issue_a,
        guest_issue=guest_issue,
        issue_b=issue_b,
    )


def _kms_client_error(code: str) -> ClientError:
    return ClientError({"Error": {"Code": code, "Message": code}}, "Decrypt")


class FakeKMS:
    """In-memory stand-in for the KMS client so tests round-trip BYOK crypto
    without AWS. Ciphertext is an opaque token (never contains the plaintext);
    decrypt verifies the caller's KeyId matches the one used to encrypt, so the
    "wrong CMK" path raises like real KMS."""

    def __init__(self):
        self._store: dict[bytes, tuple[str, bytes]] = {}
        self._counter = 0

    def encrypt(self, KeyId, Plaintext):
        self._counter += 1
        token = f"kmsfake-{self._counter}".encode()
        self._store[token] = (KeyId, Plaintext)
        return {"CiphertextBlob": token, "KeyId": KeyId}

    def decrypt(self, CiphertextBlob, KeyId=None):
        entry = self._store.get(bytes(CiphertextBlob))
        if entry is None:
            raise _kms_client_error("InvalidCiphertextException")
        stored_key, plaintext = entry
        if KeyId is not None and KeyId != stored_key:
            raise _kms_client_error("IncorrectKeyException")
        return {"Plaintext": plaintext, "KeyId": stored_key}

    def re_encrypt(self, CiphertextBlob, DestinationKeyId):
        entry = self._store.get(bytes(CiphertextBlob))
        if entry is None:
            raise _kms_client_error("InvalidCiphertextException")
        _, plaintext = entry
        return self.encrypt(KeyId=DestinationKeyId, Plaintext=plaintext)


@pytest.fixture
def kms_crypto(settings, monkeypatch):
    """Configure KMS-backed BYOK crypto with an in-memory fake (no AWS)."""
    key_id = "arn:aws:kms:us-west-2:000000000000:key/test-cmk"
    settings.ASSISTANT_KMS_KEY_ID = key_id
    settings.AWS_REGION = "us-west-2"
    monkeypatch.setattr(crypto, "_client", FakeKMS())
    return key_id


def configure_llm(user, *, model="gpt-test", base_url="https://api.example.com/v1"):
    cfg = UserLLMConfig.objects.create(
        user=user,
        provider_kind="openai_compatible",
        base_url=base_url,
        model_name=model,
        api_key_encrypted=crypto.encrypt("sk-test-key-123456"),
        last_verified_at=timezone.now(),
    )
    return cfg
