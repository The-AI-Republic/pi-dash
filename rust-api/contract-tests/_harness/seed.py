"""Raw-SQL row factories for contract suites. No ORM, no Django imports.

``Seed`` tracks created rows so a test can delete exactly what it made;
``Seeder``/``SeedTracker`` are the newer factories (every ``create_*``
registers its row with the tracker so teardown deletes exactly what the
suite created, in reverse order). Column lists mirror the Django models;
if a model gains a NOT NULL column without a database default, the
matching factory must grow a value.

Extend this module; never fork per-domain copies.
"""

from __future__ import annotations

import base64
import hashlib
import random
import secrets
import uuid
from datetime import datetime, timedelta, timezone
from typing import Any

from . import djangocrypto

from .db import insert_row


def _now():
    return datetime.now(timezone.utc)


def _uid() -> str:
    return str(uuid.uuid4())


def _tag(prefix: str) -> str:
    return f"{prefix}-{uuid.uuid4().hex[:10]}"


PASSWORD = "contract-test-pass-1"


def make_password(password: str = PASSWORD, iterations: int = 600000) -> str:
    """Django-compatible ``pbkdf2_sha256`` hash built with stdlib only."""
    salt = secrets.token_hex(12)[:22]
    digest = hashlib.pbkdf2_hmac("sha256", password.encode(), salt.encode(), iterations)
    return f"pbkdf2_sha256${iterations}${salt}${base64.b64encode(digest).decode()}"


class Seed:
    """Tracks created rows so a test can delete exactly what it made."""

    def __init__(self, conn):
        self.conn = conn
        self._rows: list[tuple[str, str, str]] = []

    def _insert(self, table: str, **cols) -> str:
        row_id = cols.get("id", _uid())
        cols.setdefault("id", row_id)
        # Quote identifiers: columns like states.group are reserved words.
        names = ", ".join(f'"{c}"' for c in cols)
        placeholders = ", ".join(["%s"] * len(cols))
        with self.conn.cursor() as cur:
            cur.execute(
                f"INSERT INTO {table} ({names}) VALUES ({placeholders})",
                list(cols.values()),
            )
        self._rows.append((table, "id", row_id))
        return row_id

    def track(self, table: str, idcol: str, row_id: str) -> str:
        """Track a row the server created (e.g. via the API) for cleanup."""
        self._rows.append((table, idcol, row_id))
        return row_id

    def tracked_ids(self, table: str) -> list[str]:
        """Row ids this Seed inserted or tracked for *table*.

        Lets a domain fixture sweep its own server-created side rows
        (scoped to its workspaces) before :meth:`cleanup` runs.
        """
        return [row_id for t, _, row_id in self._rows if t == table]

    def cleanup(self):
        users = [
            row_id for table, _, row_id in self._rows if table == "users"
        ]
        workspaces = [
            row_id for table, _, row_id in self._rows if table == "workspaces"
        ]
        with self.conn.cursor() as cur:
            # The server mints rows the suite never inserted (device-code
            # rows stamped on approve, APITokens on poll, dev machines and
            # machine tokens on exchange). Sweep those first so the
            # tracked deletes below do not hit foreign keys.
            if users or workspaces:
                cur.execute(
                    "DELETE FROM cli_device_codes WHERE user_id = ANY(%s)"
                    " OR workspace_id = ANY(%s)",
                    (users, workspaces),
                )
                cur.execute(
                    "DELETE FROM api_tokens WHERE user_id = ANY(%s)", (users,)
                )
                cur.execute(
                    "DELETE FROM sessions WHERE user_id = ANY(%s)", (users,)
                )
                cur.execute(
                    "DELETE FROM machine_token WHERE user_id = ANY(%s)"
                    " OR workspace_id = ANY(%s)",
                    (users, workspaces),
                )
                cur.execute(
                    "DELETE FROM dev_machine WHERE owner_id = ANY(%s)",
                    (users,),
                )
            # Domain rows the server (or the suite via the API) may have
            # created without Seed tracking them: views POSTed then
            # soft-deleted via the API still carry the workspace FK, as do
            # favorites and modules. Sweep them so the tracked deletes
            # below never hit foreign keys. (Projects/members stay
            # tracked-only: other suites hang pods/runners off them.)
            if workspaces:
                cur.execute(
                    "DELETE FROM module_user_properties"
                    " WHERE workspace_id = ANY(%s)",
                    (workspaces,),
                )
                cur.execute(
                    "DELETE FROM module_links WHERE workspace_id = ANY(%s)",
                    (workspaces,),
                )
                cur.execute(
                    "DELETE FROM module_issues WHERE workspace_id = ANY(%s)",
                    (workspaces,),
                )
                cur.execute(
                    "DELETE FROM module_members WHERE workspace_id = ANY(%s)",
                    (workspaces,),
                )
                cur.execute(
                    "DELETE FROM modules WHERE workspace_id = ANY(%s)",
                    (workspaces,),
                )
                cur.execute(
                    "DELETE FROM user_favorites WHERE workspace_id = ANY(%s)",
                    (workspaces,),
                )
                cur.execute(
                    "DELETE FROM user_recent_visits WHERE workspace_id = ANY(%s)",
                    (workspaces,),
                )
                cur.execute(
                    "DELETE FROM issue_comments WHERE workspace_id = ANY(%s)",
                    (workspaces,),
                )
                cur.execute(
                    "DELETE FROM issues WHERE workspace_id = ANY(%s)",
                    (workspaces,),
                )
                cur.execute(
                    "DELETE FROM issue_views WHERE workspace_id = ANY(%s)",
                    (workspaces,),
                )
            for table, idcol, row_id in reversed(self._rows):
                if table == "sessions":
                    # Forged sessions may already be flushed server-side.
                    cur.execute(
                        "DELETE FROM sessions WHERE session_key = %s",
                        (row_id,),
                    )
                    continue
                cur.execute(
                    f"DELETE FROM {table} WHERE {idcol} = %s", (row_id,)
                )
        self._rows.clear()

    # -- identity ------------------------------------------------------
    def user(self, *, email=None, password="contract-pw-80", suffix=None):
        sfx = suffix or uuid.uuid4().hex[:8]
        email = email or f"ct80-{sfx}@example.com"
        return {
            "id": self._insert(
                "users",
                password=password,
                username=f"ct80-{sfx}",
                email=email,
                first_name="Contract",
                last_name="Test",
                avatar="",
                date_joined=_now(),
                created_at=_now(),
                updated_at=_now(),
                last_location="",
                created_location="",
                is_superuser=False,
                is_managed=False,
                is_password_expired=False,
                is_active=True,
                is_staff=False,
                is_email_verified=True,
                is_password_autoset=False,
                token=uuid.uuid4().hex,
                user_timezone="UTC",
                last_login_ip="127.0.0.1",
                last_logout_ip="",
                last_login_medium="",
                last_login_uagent="contract-tests",
                is_bot=False,
                display_name=f"Contract {sfx}",
                is_email_valid=True,
                is_password_reset_required=False,
            ),
            "email": email,
            "password": password,
        }

    def workspace(self, owner_id, *, slug=None, name=None):
        slug = slug or _tag("ct80ws")
        return {
            "id": self._insert(
                "workspaces",
                name=name or f"CT80 {slug}",
                slug=slug,
                owner_id=owner_id,
                timezone="UTC",
                background_color="#4A90D9",
                created_at=_now(),
                updated_at=_now(),
            ),
            "slug": slug,
        }

    def member(self, workspace_id, user_id, *, role=20, active=True):
        return self._insert(
            "workspace_members",
            role=role,
            member_id=user_id,
            workspace_id=workspace_id,
            view_props="{}",
            default_props="{}",
            issue_props="{}",
            is_active=active,
            explored_features="{}",
            getting_started_checklist="{}",
            tips="{}",
            created_at=_now(),
            updated_at=_now(),
        )

    def tenant(self, *, role=20):
        """One user + one workspace + active membership. Returns dict."""
        u = self.user()
        w = self.workspace(u["id"])
        self.member(w["id"], u["id"], role=role)
        return {"user": u, "workspace": w}

    # -- tokens / sessions ----------------------------------------------
    def api_token(self, user_id, workspace_id=None, *, active=True,
                  description="", prefix="pi_dash_api_"):
        raw = prefix + uuid.uuid4().hex
        self._insert(
            "api_tokens",
            token=raw,
            label=f"ct80 {uuid.uuid4().hex[:6]}",
            user_type=0,
            user_id=user_id,
            workspace_id=workspace_id,
            description=description,
            is_active=active,
            is_service=False,
            allowed_rate_limit="60/min",
            created_at=_now(),
            updated_at=_now(),
        )
        return raw

    def session_cookie(self, user_id, password, secret,
                       cookie_name="session-id"):
        key = "".join(
            secrets.choice("abcdefghijklmnopqrstuvwxyz0123456789")
            for _ in range(32)
        )
        payload = djangocrypto.login_session_cookie(user_id, password, secret)
        expire = datetime.now(timezone.utc) + timedelta(days=7)
        with self.conn.cursor() as cur:
            cur.execute(
                "INSERT INTO sessions (session_key, session_data, expire_date,"
                " user_id) VALUES (%s, %s, %s, %s)",
                (key, payload, expire, str(user_id)),
            )
        self._rows.append(("sessions", "session_key", key))
        return {cookie_name: key}

    def machine_token(self, user_id, workspace_id, dev_machine_id,
                      secret, *, host_label="ct80-host", raw=None):
        raw = raw or ("mt_" + secrets.token_urlsafe(32))
        self._insert(
            "machine_token",
            user_id=user_id,
            workspace_id=workspace_id,
            dev_machine_id=dev_machine_id,
            host_label=host_label,
            token_hash=djangocrypto.machine_token_hash(raw, secret),
            token_fingerprint=djangocrypto.machine_token_fingerprint(raw),
            label=f"machine: {host_label[:96]}",
            is_service=True,
            created_at=_now(),
        )
        return raw

    # -- device codes ----------------------------------------------------
    def device_code(self, *, device_code=None, user_code=None, user_id=None,
                    workspace_id=None, approved=False, denied=False,
                    consumed=False, expires_in_seconds=600):
        device_code = device_code or secrets.token_urlsafe(32)[:48]
        user_code = user_code or (
            "".join(secrets.choice("BCDFGHJKLMNPQRSTVWXZ23456789")
                    for _ in range(8))[:4]
            + "-"
            + "".join(secrets.choice("BCDFGHJKLMNPQRSTVWXZ23456789")
                      for _ in range(8))[:4]
        )
        expire = datetime.now(timezone.utc) + timedelta(
            seconds=expires_in_seconds
        )
        self._insert(
            "cli_device_codes",
            device_code=device_code,
            user_code=user_code,
            user_id=user_id,
            workspace_id=workspace_id,
            approved=approved,
            denied=denied,
            consumed=consumed,
            expires_at=expire,
            created_at=_now(),
            updated_at=_now(),
        )
        return {"device_code": device_code, "user_code": user_code}

    # -- runner chain ------------------------------------------------------
    def project(self, workspace_id, *, identifier=None):
        identifier = identifier or _tag("CT80")[:12].upper()
        return self._insert(
            "projects",
            name=f"CT80 {identifier}",
            description="",
            network=0,
            identifier=identifier,
            workspace_id=workspace_id,
            cycle_view=False,
            module_view=False,
            issue_views_view=False,
            page_view=False,
            intake_view=False,
            archive_in=30,
            close_in=30,
            logo_props="{}",
            is_time_tracking_enabled=False,
            is_issue_type_enabled=False,
            guest_view_all_features=False,
            timezone="UTC",
            members_can_edit_states=False,
            repo_url="",
            base_branch="",
            agent_default_interval_seconds=0,
            agent_default_max_ticks=0,
            agent_ticking_enabled=False,
            is_default=False,
            agent_review_default_interval_seconds=0,
            default_agent_executor="",
            agent_test_default_interval_seconds=0,
            created_at=_now(),
            updated_at=_now(),
        )

    def pod(self, workspace_id, project_id):
        return self._insert(
            "pod",
            name=f"ct80-pod-{uuid.uuid4().hex[:6]}",
            description="",
            is_default=True,
            workspace_id=workspace_id,
            project_id=project_id,
            created_at=_now(),
            updated_at=_now(),
        )

    def dev_machine(self, owner_id, *, host_label="ct80-host"):
        return self._insert(
            "dev_machine",
            host_label=host_label,
            label=host_label[:128],
            visibility=0,
            owner_id=owner_id,
            provisioning="manual",
            created_at=_now(),
            updated_at=_now(),
        )

    def runner(self, owner_id, workspace_id, pod_id, *, name=None,
               dev_machine_id=None):
        return self._insert(
            "runner",
            name=name or f"ct80-runner-{uuid.uuid4().hex[:6]}",
            host_label="",
            refresh_token_hash="",
            refresh_token_fingerprint="",
            refresh_token_generation=0,
            previous_refresh_token_hash="",
            access_token_signing_key_version=1,
            enrollment_token_hash="",
            enrollment_token_fingerprint="",
            revoked_reason="",
            dev_metadata="{}",
            provisioning="manual",
            capabilities="[]",
            status="offline",
            os="",
            arch="",
            runner_version="",
            protocol_version=1,
            owner_id=owner_id,
            workspace_id=workspace_id,
            pod_id=pod_id,
            dev_machine_id=dev_machine_id,
            visibility=0,
            created_at=_now(),
            updated_at=_now(),
        )

    # -- app views + search (PIDASHCONV-87) -------------------------------
    def project_member(self, project_id, workspace_id, user_id, *, role=20,
                       active=True):
        """Project membership row. Roles mirror ROLE: 20 admin, 15 member,
        5 guest."""
        return self._insert(
            "project_members",
            role=role,
            member_id=user_id,
            project_id=project_id,
            workspace_id=workspace_id,
            view_props="{}",
            default_props="{}",
            sort_order=65535,
            preferences="{}",
            is_active=active,
            created_at=_now(),
            updated_at=_now(),
        )

    def issue_view(self, workspace_id, owner_id, *, project_id=None,
                   name=None, access=1, locked=False):
        """An IssueView row. created_by_id mirrors owned_by_id the way
        API-created rows look (crum stamps the request user)."""
        return self._insert(
            "issue_views",
            name=name or f"CT87 view {uuid.uuid4().hex[:6]}",
            description="",
            query="{}",
            filters="{}",
            display_filters="{}",
            display_properties="{}",
            logo_props="{}",
            rich_filters="{}",
            access=access,
            sort_order=65535,
            is_locked=locked,
            owned_by_id=owner_id,
            created_by_id=owner_id,
            workspace_id=workspace_id,
            project_id=project_id,
            created_at=_now(),
            updated_at=_now(),
        )

    def issue(self, workspace_id, project_id, *, name=None,
              description="", sequence_id=None, state_id=None):
        """A minimal issue row with FTS-visible text columns set."""
        return self._insert(
            "issues",
            name=name or f"CT87 issue {uuid.uuid4().hex[:6]}",
            description_json="{}",
            description_html=f"<p>{description}</p>" if description else "<p></p>",
            description_stripped=description or None,
            priority="none",
            sequence_id=sequence_id if sequence_id is not None else 1,
            state_id=state_id,
            project_id=project_id,
            workspace_id=workspace_id,
            sort_order=65535,
            is_draft=False,
            git_work_branch="",
            workpad="",
            complexity_score=0,
            created_at=_now(),
            updated_at=_now(),
        )

    def favorite_view(self, workspace_id, user_id, view_id, *,
                      project_id=None):
        """A UserFavorite row pointing at a view (entity_type='view')."""
        return self._insert(
            "user_favorites",
            entity_type="view",
            entity_identifier=view_id,
            user_id=user_id,
            workspace_id=workspace_id,
            project_id=project_id,
            is_folder=False,
            sequence=65535,
            created_at=_now(),
            updated_at=_now(),
        )

    def issue_comment(self, workspace_id, project_id, issue_id, *,
                      text="contract comment", actor_id=None):
        return self._insert(
            "issue_comments",
            comment_stripped=text,
            comment_json="{}",
            comment_html=f"<p>{text}</p>",
            attachments="{}",
            labels="{}",
            access="EXTERNAL",
            issue_id=issue_id,
            project_id=project_id,
            workspace_id=workspace_id,
            actor_id=actor_id,
            speaker_type="human",
            speaker_label="",
            created_at=_now(),
            updated_at=_now(),
        )

    # -- states ----------------------------------------------------------
    def state(self, project_id, workspace_id, *, name="Todo",
              group="unstarted"):
        return self._insert(
            "states",
            name=name,
            description="",
            color="#4A90D9",
            slug="",
            project_id=project_id,
            workspace_id=workspace_id,
            sequence=65535,
            group=group,
            default=False,
            is_triage=False,
            created_at=_now(),
            updated_at=_now(),
        )

    def issue_relation(self, workspace_id, project_id, issue_id,
                       related_issue_id, *, relation_type="blocked_by"):
        return self._insert(
            "issue_relations",
            issue_id=issue_id,
            related_issue_id=related_issue_id,
            relation_type=relation_type,
            project_id=project_id,
            workspace_id=workspace_id,
            created_at=_now(),
            updated_at=_now(),
        )

    # -- prompting + scheduler (PIDASHCONV-18, D-04) -----------------------
    def override(self, workspace_id, section_key, body, *, user_id=None,
                 active=True, version=1, needs_attention=False,
                 updated_by_id=None):
        """A prompt_section_override row. user_id None = workspace-level."""
        return self._insert(
            "prompt_section_override",
            workspace_id=workspace_id,
            user_id=user_id,
            section_key=section_key,
            body=body,
            is_active=active,
            version=version,
            needs_attention=needs_attention,
            updated_by_id=updated_by_id,
            created_at=_now(),
            updated_at=_now(),
        )

    def scheduler(self, workspace_id, *, slug=None, prompt="Scan repo.",
                  source="builtin"):
        slug = slug or _tag("ct18sched")
        return self._insert(
            "schedulers",
            workspace_id=workspace_id,
            slug=slug,
            name=f"CT18 {slug}",
            description="contract scheduler",
            prompt=prompt,
            source=source,
            is_enabled=True,
            color="#3b82f6",
            created_at=_now(),
            updated_at=_now(),
        )

    def scheduler_binding(self, workspace_id, project_id, scheduler_id,
                          actor_id, *, dtstart=None):
        return self._insert(
            "scheduler_bindings",
            workspace_id=workspace_id,
            project_id=project_id,
            scheduler_id=scheduler_id,
            actor_id=actor_id,
            dtstart=dtstart or _now(),
            tzid="UTC",
            rrule="",
            rdates="[]",
            exdates="[]",
            extra_context="",
            enabled=True,
            last_error="",
            outcome_mode="create_issue",
            created_at=_now(),
            updated_at=_now(),
        )

    # -- modules (PIDASHCONV-86, D-28) ------------------------------------
    def module(self, workspace_id, project_id, *, name=None,
               status="planned", archived_at=None, sort_order=65535):
        """A minimal modules row."""
        return self._insert(
            "modules",
            name=name or f"CT86 module {uuid.uuid4().hex[:6]}",
            description="",
            view_props="{}",
            sort_order=sort_order,
            status=status,
            logo_props="{}",
            archived_at=archived_at,
            project_id=project_id,
            workspace_id=workspace_id,
            created_at=_now(),
            updated_at=_now(),
        )

    def module_member(self, workspace_id, project_id, module_id, member_id):
        return self._insert(
            "module_members",
            module_id=module_id,
            member_id=member_id,
            project_id=project_id,
            workspace_id=workspace_id,
            created_at=_now(),
            updated_at=_now(),
        )

    def module_issue(self, workspace_id, project_id, module_id, issue_id):
        return self._insert(
            "module_issues",
            module_id=module_id,
            issue_id=issue_id,
            project_id=project_id,
            workspace_id=workspace_id,
            created_at=_now(),
            updated_at=_now(),
        )

    def module_link(self, workspace_id, project_id, module_id, *, title="Docs",
                    url=None):
        return self._insert(
            "module_links",
            title=title,
            url=url or f"http://example.com/{uuid.uuid4().hex[:8]}",
            metadata="{}",
            module_id=module_id,
            project_id=project_id,
            workspace_id=workspace_id,
            created_at=_now(),
            updated_at=_now(),
        )

    def favorite_module(self, workspace_id, project_id, user_id, module_id):
        """A UserFavorite row pointing at a module (entity_type='module')."""
        return self._insert(
            "user_favorites",
            entity_type="module",
            entity_identifier=module_id,
            user_id=user_id,
            workspace_id=workspace_id,
            project_id=project_id,
            is_folder=False,
            sequence=65535,
            created_at=_now(),
            updated_at=_now(),
        )

    def module_user_properties(self, workspace_id, project_id, module_id,
                               user_id):
        return self._insert(
            "module_user_properties",
            filters='{"priority": null}',
            display_filters="{}",
            display_properties="{}",
            rich_filters="{}",
            module_id=module_id,
            project_id=project_id,
            user_id=user_id,
            workspace_id=workspace_id,
            created_at=_now(),
            updated_at=_now(),
        )


"""Organisation seeding: users, workspaces, memberships, projects.

Column lists mirror the Django schema (``users``, ``workspaces``,
``workspace_members``, ``projects``, ``project_members``). Rows are inserted
with raw SQL, which bypasses model ``post_save`` signals by design: no
builtin schedulers are seeded and no default pod is created, so each test
sees exactly the rows it inserted.

Roles: ADMIN=20, MEMBER=15, GUEST=5.
"""

ADMIN = 20
MEMBER = 15
GUEST = 5


def ensure_setup_done(conn) -> None:
    """Upsert the ``instances`` row with ``is_setup_done=True``.

    Auth endpoints refuse with ``INSTANCE_NOT_CONFIGURED`` until setup is
    done; every auth suite ensures it per test so one suite's
    not-configured case cannot leak into the next test.
    """
    with conn.cursor() as cur:
        cur.execute("SELECT id FROM instances LIMIT 1")
        if cur.fetchone() is None:
            now = now_iso()
            cur.execute(
                """INSERT INTO instances (id, created_at, updated_at,
                    instance_name, instance_id, current_version, domain,
                    last_checked_at, is_telemetry_enabled, is_support_required,
                    is_setup_done, is_signup_screen_visited, is_verified,
                    is_test, is_current_version_deprecated, edition)
                VALUES (%s,%s,%s,'contract-tests',%s,'1.0.0',
                    'http://localhost',%s,true,true,true,false,false,false,
                    false,'PI_DASH_COMMUNITY')""",
                (new_id(), now, now, new_id(), now),
            )
        else:
            cur.execute("UPDATE instances SET is_setup_done = true")


def now_iso() -> str:
    return datetime.now(timezone.utc).isoformat()


def new_id() -> str:
    return str(uuid.uuid4())


def create_user(conn, *, email: str, username: str, password_field: str) -> dict:
    uid = new_id()
    now = now_iso()
    conn.execute(
        """INSERT INTO users (id, password, username, first_name, last_name,
            avatar, date_joined, created_at, updated_at, last_location,
            created_location, is_superuser, is_managed, is_password_expired,
            is_active, is_staff, is_email_verified, is_password_autoset, token,
            user_timezone, last_login_ip, last_logout_ip, last_login_medium,
            last_login_uagent, is_bot, display_name, is_email_valid,
            is_password_reset_required, email)
        VALUES (%s,%s,%s,'First','Last','',%s,%s,%s,'','',false,false,false,
            true,false,false,false,'tok','UTC','','','','ua',false,
            'Display','t',false,%s)""",
        (uid, password_field, username, now, now, now, email),
    )
    return {"id": uid, "email": email, "password_field": password_field}


def create_workspace(conn, *, slug: str, name: str, owner_id: str) -> dict:
    wid = new_id()
    now = now_iso()
    conn.execute(
        """INSERT INTO workspaces (id, name, slug, owner_id, timezone,
            background_color, created_at, updated_at)
        VALUES (%s,%s,%s,%s,'UTC','#ffffff',%s,%s)""",
        (wid, name, slug, owner_id, now, now),
    )
    return {"id": wid, "slug": slug}


def add_workspace_member(
    conn, *, workspace_id: str, user_id: str, role: int, active: bool = True
) -> str:
    mid = new_id()
    now = now_iso()
    conn.execute(
        """INSERT INTO workspace_members (id, role, member_id, workspace_id,
            view_props, default_props, issue_props, is_active,
            explored_features, getting_started_checklist, tips,
            created_at, updated_at)
        VALUES (%s,%s,%s,%s,'{}','{}','{}',%s,'{}','{}','{}',%s,%s)""",
        (mid, role, user_id, workspace_id, active, now, now),
    )
    return mid


def create_project(
    conn, *, workspace_id: str, identifier: str, name: str, created_by_id: str
) -> dict:
    pid = new_id()
    now = now_iso()
    conn.execute(
        """INSERT INTO projects (id, name, description, network, identifier,
            workspace_id, cycle_view, module_view, issue_views_view, page_view,
            intake_view, archive_in, close_in, logo_props,
            is_time_tracking_enabled, is_issue_type_enabled,
            guest_view_all_features, timezone, members_can_edit_states,
            repo_url, base_branch, agent_default_interval_seconds,
            agent_default_max_ticks, agent_ticking_enabled, is_default,
            agent_review_default_interval_seconds, default_agent_executor,
            agent_test_default_interval_seconds, created_at, updated_at,
            created_by_id)
        VALUES (%s,%s,'Seeded for contract tests',1,%s,%s,
            true,true,true,true,true,30,30,'{}',
            true,true,false,'UTC',true,'','main',60,10,true,false,
            60,'local_runner',60,%s,%s,%s)""",
        (pid, name, identifier, workspace_id, now, now, created_by_id),
    )
    return {"id": pid, "identifier": identifier}


def create_api_token(conn, *, user_id: str, workspace_id: str | None = None) -> str:
    """Insert an ``api_tokens`` row; return the raw ``X-Api-Key`` value.

    api-v1 (``pi_dash.api``) authenticates only via ``APIKeyAuthentication``,
    so contract suites for those endpoints auth with per-user tokens rather
    than session cookies.
    """
    tok = "pi_dash_api_" + uuid.uuid4().hex
    now = now_iso()
    conn.execute(
        """INSERT INTO api_tokens (id, token, label, description, is_active,
            user_type, user_id, workspace_id, is_service, allowed_rate_limit,
            created_at, updated_at)
        VALUES (%s,%s,'contract-test','seeded for contract tests',true,
            0,%s,%s,false,'100000/min',%s,%s)""",
        (new_id(), tok, user_id, workspace_id, now, now),
    )
    return tok


def add_project_member(
    conn,
    *,
    project_id: str,
    workspace_id: str,
    user_id: str,
    role: int,
    active: bool = True,
) -> str:
    mid = new_id()
    now = now_iso()
    conn.execute(
        """INSERT INTO project_members (id, role, project_id, workspace_id,
            member_id, view_props, default_props, sort_order, preferences,
            is_active, created_at, updated_at)
        VALUES (%s,%s,%s,%s,%s,'{}','{}',65535,'{}',%s,%s,%s)""",
        (mid, role, project_id, workspace_id, user_id, active, now, now),
    )
    return mid
class SeedTracker:
    """Remembers (table, id) pairs in creation order for teardown."""

    def __init__(self, db):
        self.db = db
        self.rows: list[tuple[str, str]] = []

    def add(self, table: str, row_id: str):
        self.rows.append((table, row_id))

    # Rows Django creates as side effects of the black-box flow (never seeded,
    # so never tracked): signing a user in through /auth/sign-in/ creates
    # their profile row, which FK-blocks the tracked user delete. Purge them
    # first so teardown leaves zero orphans.
    _DEPENDENTS = {"users": [("profiles", "user_id")]}

    def _tracked_ids(self, table: str) -> list[str]:
        return [row_id for tracked_table, row_id in self.rows if tracked_table == table]

    def _delete_untracked(self, sql: str, params: tuple):
        # An empty IN list matches nothing: skip instead of emitting `IN ()`.
        if not params:
            return
        try:
            self.db.execute(sql, params)
        except Exception:
            pass

    def _purge_side_rows(self):
        """Delete server-created rows that belong to this test's world.

        API writes (created pages, "(Copy)" duplicates and their
        ``project_pages`` links, favorites, versions, ``page_logs``) reference the seeded
        users/workspaces/projects/pages but are never tracked, so the
        per-row delete below cannot see them — and their FKs can even block
        it. Every world is unique per test (unique tag), so "untracked rows
        referencing tracked ids" is exactly this test's side effects.
        """
        users = self._tracked_ids("users")
        workspaces = self._tracked_ids("workspaces")
        projects = self._tracked_ids("projects")
        pages = self._tracked_ids("pages")
        favorites = self._tracked_ids("user_favorites")
        links = self._tracked_ids("project_pages")
        versions = self._tracked_ids("page_versions")

        def placeholders(ids: list[str]) -> str:
            return ",".join(["%s"] * len(ids))

        if projects:
            self._delete_untracked(
                f"DELETE FROM project_pages WHERE project_id IN ({placeholders(projects)})"
                + (f" AND id NOT IN ({placeholders(links)})" if links else ""),
                tuple(projects) + tuple(links),
            )
        if users or projects:
            conditions: list[str] = []
            params: list[str] = []
            if users:
                conditions.append(f"user_id IN ({placeholders(users)})")
                params.extend(users)
            if projects:
                conditions.append(f"project_id IN ({placeholders(projects)})")
                params.extend(projects)
            not_tracked = f" AND id NOT IN ({placeholders(favorites)})" if favorites else ""
            self._delete_untracked(
                f"DELETE FROM user_favorites WHERE ({' OR '.join(conditions)}){not_tracked}",
                tuple(params) + tuple(favorites),
            )
        if pages:
            not_tracked = f" AND id NOT IN ({placeholders(versions)})" if versions else ""
            self._delete_untracked(
                f"DELETE FROM page_versions WHERE page_id IN ({placeholders(pages)}){not_tracked}",
                tuple(pages) + tuple(versions),
            )
            # page_transaction (eager or worker) logs to page_logs, whose FK
            # would otherwise block the page delete below.
            self._delete_untracked(
                f"DELETE FROM page_logs WHERE page_id IN ({placeholders(pages)})",
                tuple(pages),
            )
        if workspaces or users:
            # API-created pages ("Created page", "(Copy)" duplicates) belong to
            # this test's workspace / users but are never tracked; their
            # owned_by FK would otherwise block the tracked user deletes.
            conditions, params = [], []
            if workspaces:
                conditions.append(f"workspace_id IN ({placeholders(workspaces)})")
                params.extend(workspaces)
            if users:
                conditions.append(f"owned_by_id IN ({placeholders(users)})")
                params.extend(users)
                conditions.append(f"created_by_id IN ({placeholders(users)})")
                params.extend(users)
            not_tracked = f" AND id NOT IN ({placeholders(pages)})" if pages else ""
            self._delete_untracked(
                f"DELETE FROM pages WHERE ({' OR '.join(conditions)}){not_tracked}",
                tuple(params) + tuple(pages),
            )

    def cleanup(self):
        self._purge_side_rows()
        for table, row_id in reversed(self.rows):
            for dependent, column in self._DEPENDENTS.get(table, ()):
                try:
                    self.db.execute(f"DELETE FROM {dependent} WHERE {column} = %s", (row_id,))
                except Exception:
                    pass
            try:
                self.db.execute(f"DELETE FROM {table} WHERE id = %s", (row_id,))
            except Exception:
                pass
        self.rows.clear()


class Seeder:
    def __init__(self, db, tracker: SeedTracker, tag: str):
        self.db = db
        self.track = tracker
        self.tag = tag
        self._n = 0

    def _slug(self, prefix: str) -> str:
        self._n += 1
        return f"{prefix}-{self.tag}-{self._n}".lower().replace("_", "-")[:44]

    def _put(self, table: str, row_id: str):
        self.track.add(table, row_id)
        return row_id

    # -- foundation ------------------------------------------------------
    def ensure_instance(self):
        """The sign-in views refuse when no set-up Instance exists.

        Never modifies an existing row: on a real checkout the instance is
        already there; on a fresh one we insert a minimal set-up marker.
        """
        if self.db.fetchval("SELECT COUNT(*) FROM instances") == 0:
            self.db.execute(
                """INSERT INTO instances
                   (id, instance_name, instance_id, current_version, edition,
                    domain, last_checked_at, is_telemetry_enabled,
                    is_support_required, is_setup_done, is_signup_screen_visited,
                    is_verified, is_test, is_current_version_deprecated,
                    created_at, updated_at)
                   VALUES (%s,%s,%s,%s,%s,%s,now(),false,false,true,false,false,false,false,now(),now())""",
                (_uid(), f"contract-{self.tag}", f"contract-{self.tag}", "0.0.0-contract", "PI_DASH_COMMUNITY", ""),
            )

    def create_user(self, *, email: str | None = None, password: str = PASSWORD) -> dict[str, Any]:
        self._n += 1
        email = email or f"contract-{self.tag}-{self._n}@example.com"
        user_id = _uid()
        self.db.execute(
            """INSERT INTO users
               (id, password, email, username, display_name, first_name, last_name,
                avatar, date_joined, created_at, updated_at, last_location,
                created_location, is_superuser, is_managed, is_password_expired,
                is_active, is_staff, is_email_verified, is_password_autoset,
                is_password_reset_required, token, last_active, last_login_ip,
                last_logout_ip, last_login_medium, last_login_uagent, is_bot,
                user_timezone, is_email_valid)
               VALUES (%s,%s,%s,%s,'','','','',now(),now(),now(),'','',
                       false,false,false,true,false,false,false,false,'',now(),'','',
                       'email','',false,'UTC',true)""",
            (user_id, make_password(password), email, f"contract_{self.tag}_{self._n}"),
        )
        self._put("users", user_id)
        return {"id": user_id, "email": email, "password": password}

    def create_workspace(self, owner_id: str, *, name: str | None = None) -> dict[str, Any]:
        ws_id = _uid()
        slug = self._slug("ws")
        self.db.execute(
            """INSERT INTO workspaces
               (id, name, owner_id, slug, timezone, background_color, created_at, updated_at)
               VALUES (%s,%s,%s,%s,'UTC','#ffffff',now(),now())""",
            (ws_id, name or f"Contract WS {self.tag}", owner_id, slug),
        )
        self._put("workspaces", ws_id)
        return {"id": ws_id, "slug": slug}

    def create_project(self, workspace_id: str, *, name: str | None = None) -> dict[str, Any]:
        project_id = _uid()
        self._n += 1
        self.db.execute(
            """INSERT INTO projects
               (id, name, description, network, workspace_id, identifier,
                module_view, cycle_view, issue_views_view, page_view, intake_view,
                is_time_tracking_enabled, is_issue_type_enabled, is_default,
                guest_view_all_features, members_can_edit_states,
                archive_in, close_in, logo_props, timezone, repo_url, base_branch,
                agent_default_interval_seconds, agent_default_max_ticks,
                agent_review_default_interval_seconds, agent_test_default_interval_seconds,
                agent_ticking_enabled, default_agent_executor,
                created_at, updated_at)
               VALUES (%s,%s,'',2,%s,%s,
                       false,false,false,true,false,
                       false,false,false,
                       false,true,
                       0,0,'{}','UTC','','main',
                       10800,10,10800,10800,
                       true,'local_runner',
                       now(),now())""",
            (
                project_id,
                # Unique per project within a workspace (partial unique index
                # on name where deleted_at is null): tests seeding a second
                # project in the same workspace share one tag, so the counter
                # disambiguates.
                name or f"Contract Project {self.tag}-{self._n}",
                workspace_id,
                f"CT{self._n % 100000:05d}",
            ),
        )
        self._put("projects", project_id)
        return {"id": project_id}

    # -- space domain ----------------------------------------------------
    def create_board(
        self,
        workspace_id: str,
        project_id: str,
        anchor: str | None = None,
        *,
        comments: bool = True,
        reactions: bool = True,
        votes: bool = True,
        intake_id: str | None = None,
    ) -> dict[str, Any]:
        board_id = _uid()
        anchor = anchor or secrets.token_hex(16)
        self.db.execute(
            """INSERT INTO deploy_boards
               (id, workspace_id, project_id, entity_identifier, entity_name, anchor,
                is_comments_enabled, is_reactions_enabled, intake_id, is_votes_enabled,
                view_props, is_activity_enabled, is_disabled, created_at, updated_at)
               VALUES (%s,%s,%s,%s,'project',%s,%s,%s,%s,%s,'{}',true,false,now(),now())""",
            (board_id, workspace_id, project_id, project_id, anchor, comments, reactions, intake_id, votes),
        )
        self._put("deploy_boards", board_id)
        return {"id": board_id, "anchor": anchor}

    def create_state(self, workspace_id: str, project_id: str, *, name: str = "Backlog", group: str = "backlog") -> dict[str, Any]:
        state_id = _uid()
        self.db.execute(
            """INSERT INTO states
               (id, workspace_id, project_id, name, description, color, slug,
                sequence, "group", is_triage, "default", created_at, updated_at)
               VALUES (%s,%s,%s,%s,'','#ff0000','',65535,%s,false,false,now(),now())""",
            (state_id, workspace_id, project_id, name, group),
        )
        self._put("states", state_id)
        return {"id": state_id}

    def create_issue(
        self,
        workspace_id: str,
        project_id: str,
        state_id: str | None = None,
        *,
        name: str = "Contract issue",
        priority: str = "none",
        sequence_id: int = 1,
    ) -> dict[str, Any]:
        issue_id = _uid()
        self.db.execute(
            """INSERT INTO issues
               (id, workspace_id, project_id, state_id, name, description_json,
                description_html, priority, complexity_score, sequence_id, sort_order,
                is_draft, git_work_branch, workpad, created_at, updated_at)
               VALUES (%s,%s,%s,%s,%s,'{}','<p></p>',%s,0,%s,65535,false,'','',now(),now())""",
            (issue_id, workspace_id, project_id, state_id, name, priority, sequence_id),
        )
        self._put("issues", issue_id)
        return {"id": issue_id}

    def create_comment(
        self, workspace_id: str, project_id: str, issue_id: str, actor_id: str, *, access: str = "EXTERNAL"
    ) -> dict[str, Any]:
        comment_id = _uid()
        self.db.execute(
            """INSERT INTO issue_comments
               (id, workspace_id, project_id, comment_stripped, comment_json, comment_html,
                attachments, labels, issue_id, actor_id, access, speaker_type, speaker_label,
                created_at, updated_at)
               VALUES (%s,%s,%s,'test comment','{}','<p>test comment</p>',
                       '{}','{}',%s,%s,%s,'human','',now(),now())""",
            (comment_id, workspace_id, project_id, issue_id, actor_id, access),
        )
        self._put("issue_comments", comment_id)
        return {"id": comment_id}

    def create_issue_reaction(self, workspace_id: str, project_id: str, issue_id: str, actor_id: str, *, reaction: str = "heart") -> dict[str, Any]:
        reaction_id = _uid()
        self.db.execute(
            """INSERT INTO issue_reactions
               (id, workspace_id, project_id, actor_id, issue_id, reaction, created_at, updated_at)
               VALUES (%s,%s,%s,%s,%s,%s,now(),now())""",
            (reaction_id, workspace_id, project_id, actor_id, issue_id, reaction),
        )
        self._put("issue_reactions", reaction_id)
        return {"id": reaction_id}

    def create_comment_reaction(
        self, workspace_id: str, project_id: str, comment_id: str, actor_id: str, *, reaction: str = "heart"
    ) -> dict[str, Any]:
        reaction_id = _uid()
        self.db.execute(
            """INSERT INTO comment_reactions
               (id, workspace_id, project_id, actor_id, comment_id, reaction, created_at, updated_at)
               VALUES (%s,%s,%s,%s,%s,%s,now(),now())""",
            (reaction_id, workspace_id, project_id, actor_id, comment_id, reaction),
        )
        self._put("comment_reactions", reaction_id)
        return {"id": reaction_id}

    def create_vote(self, workspace_id: str, project_id: str, issue_id: str, actor_id: str, *, vote: int = 1) -> dict[str, Any]:
        vote_id = _uid()
        self.db.execute(
            """INSERT INTO issue_votes
               (id, workspace_id, project_id, issue_id, actor_id, vote, created_at, updated_at)
               VALUES (%s,%s,%s,%s,%s,%s,now(),now())""",
            (vote_id, workspace_id, project_id, issue_id, actor_id, vote),
        )
        self._put("issue_votes", vote_id)
        return {"id": vote_id}

    def create_cycle(
        self, workspace_id: str, project_id: str, owner_id: str, *, name: str = "Contract cycle"
    ) -> dict[str, Any]:
        cycle_id = _uid()
        self.db.execute(
            """INSERT INTO cycles
               (id, workspace_id, project_id, name, description, owned_by_id, view_props,
                sort_order, progress_snapshot, logo_props, timezone, version, created_at, updated_at)
               VALUES (%s,%s,%s,%s,'',%s,'{}',65535,'{}','{}','UTC',1,now(),now())""",
            (cycle_id, workspace_id, project_id, name, owner_id),
        )
        self._put("cycles", cycle_id)
        return {"id": cycle_id}

    def create_module(self, workspace_id: str, project_id: str, *, name: str = "Contract module") -> dict[str, Any]:
        module_id = _uid()
        self.db.execute(
            """INSERT INTO modules
               (id, workspace_id, project_id, name, description, status, view_props,
                sort_order, logo_props, created_at, updated_at)
               VALUES (%s,%s,%s,%s,'','planned','{}',65535,'{}',now(),now())""",
            (module_id, workspace_id, project_id, name),
        )
        self._put("modules", module_id)
        return {"id": module_id}

    def create_label(self, workspace_id: str, project_id: str, *, name: str = "bug") -> dict[str, Any]:
        label_id = _uid()
        self.db.execute(
            """INSERT INTO labels
               (id, workspace_id, project_id, name, description, color, sort_order,
                created_at, updated_at)
               VALUES (%s,%s,%s,%s,'','',65535,now(),now())""",
            (label_id, workspace_id, project_id, name),
        )
        self._put("labels", label_id)
        return {"id": label_id}

    def create_asset(
        self,
        workspace_id: str,
        project_id: str,
        user_id: str,
        *,
        entity_type: str = "ISSUE_DESCRIPTION",
        uploaded: bool = True,
    ) -> dict[str, Any]:
        asset_id = _uid()
        key = f"{workspace_id}/{asset_id}-contract.png"
        self.db.execute(
            """INSERT INTO file_assets
               (id, attributes, asset, user_id, workspace_id, project_id,
                entity_type, is_deleted, is_archived, size, is_uploaded,
                created_at, updated_at)
               VALUES (%s,'{}',%s,%s,%s,%s,%s,false,false,10,%s,now(),now())""",
            (asset_id, key, user_id, workspace_id, project_id, entity_type, uploaded),
        )
        self._put("file_assets", asset_id)
        return {"id": asset_id, "key": key}

    def create_intake(
        self, workspace_id: str, project_id: str, *, name: str = "Contract intake",
        is_default: bool = False,
    ) -> dict[str, Any]:
        intake_id = _uid()
        self.db.execute(
            """INSERT INTO intakes
               (id, workspace_id, project_id, name, description, is_default,
                view_props, logo_props, created_at, updated_at)
               VALUES (%s,%s,%s,%s,'',%s,'{}','{}',now(),now())""",
            (intake_id, workspace_id, project_id, name, is_default),
        )
        self._put("intakes", intake_id)
        return {"id": intake_id}

    def create_intake_issue(
        self, workspace_id: str, project_id: str, intake_id: str, issue_id: str, *, status: int = -2
    ) -> dict[str, Any]:
        bridge_id = _uid()
        self.db.execute(
            """INSERT INTO intake_issues
               (id, workspace_id, project_id, intake_id, issue_id, status, source,
                extra, created_at, updated_at)
               VALUES (%s,%s,%s,%s,%s,%s,'IN_APP','{}',now(),now())""",
            (bridge_id, workspace_id, project_id, intake_id, issue_id, status),
        )
        self._put("intake_issues", bridge_id)
        return {"id": bridge_id}

    # -- pages domain (D-30, PIDASHCONV-88) ----------------------------------
    # Column lists mirror apps/api/pi_dash/db/models/page.py,
    # project.py (ProjectMember) and favorite.py (UserFavorite).
    def create_project_member(
        self, workspace_id: str, project_id: str, user_id: str, *, role: int = 20
    ) -> dict[str, Any]:
        """Project membership row: every authenticated pages case needs one.

        ``role`` follows ``app/permissions/base.py`` ROLE: 20 ADMIN,
        15 MEMBER, 5 GUEST.
        """
        member_id = _uid()
        self.db.execute(
            """INSERT INTO project_members
               (id, workspace_id, project_id, member_id, role, view_props, default_props,
                preferences, sort_order, is_active, created_at, updated_at)
               VALUES (%s,%s,%s,%s,%s,'{}','{}','{}',65535,true,now(),now())""",
            (member_id, workspace_id, project_id, user_id, role),
        )
        self._put("project_members", member_id)
        return {"id": member_id, "role": role}

    def create_page(
        self,
        workspace_id: str,
        owner_id: str,
        *,
        name: str = "Contract page",
        access: int = 0,
        parent_id: str | None = None,
        description_html: str = "<p></p>",
        description_binary: bytes | None = None,
        archived: bool = False,
        locked: bool = False,
    ) -> dict[str, Any]:
        """A ``pages`` row. The caller links it to a project with
        ``link_page_project`` — the list/retrieve querysets only see linked
        pages (``.filter(project=True)``), so the link is part of the world.
        """
        page_id = _uid()
        self.db.execute(
            """INSERT INTO pages
               (id, name, description_json, description_binary, description_html,
                owned_by_id, access, workspace_id, color, parent_id, archived_at,
                is_locked, view_props, logo_props, is_global, sort_order,
                created_by_id, created_at, updated_at)
               VALUES (%s,%s,'{}',%s,%s,%s,%s,%s,'',%s,
                       CASE WHEN %s THEN CURRENT_DATE ELSE NULL END,
                       %s,'{"full_width": false}','{}',false,65535,%s,now(),now())""",
            (
                page_id, name, description_binary, description_html,
                owner_id, access, workspace_id, parent_id,
                archived, locked, owner_id,
            ),
        )
        self._put("pages", page_id)
        return {"id": page_id, "name": name, "access": access}

    def link_page_project(
        self, workspace_id: str, project_id: str, page_id: str, *, by_id: str | None = None
    ) -> dict[str, Any]:
        bridge_id = _uid()
        self.db.execute(
            """INSERT INTO project_pages
               (id, workspace_id, project_id, page_id, created_by_id, updated_by_id,
                deleted_at, created_at, updated_at)
               VALUES (%s,%s,%s,%s,%s,%s,NULL,now(),now())""",
            (bridge_id, workspace_id, project_id, page_id, by_id, by_id),
        )
        self._put("project_pages", bridge_id)
        return {"id": bridge_id}

    def create_favorite(
        self, workspace_id: str, project_id: str, user_id: str, page_id: str
    ) -> dict[str, Any]:
        fav_id = _uid()
        self.db.execute(
            """INSERT INTO user_favorites
               (id, entity_type, entity_identifier, user_id, workspace_id, project_id,
                is_folder, sequence, created_at, updated_at)
               VALUES (%s,'page',%s,%s,%s,%s,false,65535,now(),now())""",
            (fav_id, page_id, user_id, workspace_id, project_id),
        )
        self._put("user_favorites", fav_id)
        return {"id": fav_id}

    def create_page_version(
        self, workspace_id: str, page_id: str, owner_id: str, *, description_html: str = "<p>v1</p>"
    ) -> dict[str, Any]:
        version_id = _uid()
        self.db.execute(
            """INSERT INTO page_versions
               (id, workspace_id, page_id, last_saved_at, description_html,
                description_json, sub_pages_data, owned_by_id, created_at, updated_at)
               VALUES (%s,%s,%s,now(),%s,'{}','{}',%s,now(),now())""",
            (version_id, workspace_id, page_id, description_html, owner_id),
        )
        self._put("page_versions", version_id)
        return {"id": version_id, "description_html": description_html}

    # -- shared membership (dispatch D-11, PIDASHCONV-22) -----------------------
    # Column lists mirror apps/api/pi_dash/db/models/workspace.py
    # (WorkspaceMember).
    def create_workspace_member(
        self, workspace_id: str, user_id: str, *, role: int = 20
    ) -> dict[str, Any]:
        """Workspace membership row: ``is_workspace_member`` gates dispatch
        execution on it (``core/permissions.py``).

        ``role`` follows ``app/permissions/base.py`` ROLE: 20 ADMIN,
        15 MEMBER, 5 GUEST.
        """
        member_id = _uid()
        self.db.execute(
            """INSERT INTO workspace_members
               (id, workspace_id, member_id, role, company_role, view_props, default_props,
                issue_props, is_active, deleted_at, explored_features,
                getting_started_checklist, tips, created_at, updated_at)
               VALUES (%s,%s,%s,%s,NULL,'{}','{}','{}',true,NULL,'{}','{}','{}',now(),now())""",
            (member_id, workspace_id, user_id, role),
        )
        self._put("workspace_members", member_id)
        return {"id": member_id, "role": role}

    # -- app integrations (D-33, PIDASHCONV-91) -------------------------------
    # Column lists mirror apps/api/pi_dash/app/models/. Factories for the
    # github PAT / github app / project-bind / generic-git / webhook surface.
    # Extended here (never a per-domain fork).

    def create_api_token(
        self, user_id: str, workspace_id: str | None = None, *, label: str = "contract"
    ) -> dict[str, Any]:
        token_id = _uid()
        token = f"contract-{self.tag}-{token_id[:8]}"
        self.db.execute(
            """INSERT INTO api_tokens
               (id, token, label, user_type, user_id, workspace_id, description,
                is_active, is_service, allowed_rate_limit, created_at, updated_at)
               VALUES (%s,%s,%s,0,%s,%s,'',true,false,'100000/minute',now(),now())""",
            (token_id, token, label, user_id, workspace_id),
        )
        self._put("api_tokens", token_id)
        return {"id": token_id, "token": token}

    def ensure_github_integration(self) -> dict[str, Any]:
        """Shared ``integrations`` row (provider=github), never tracked.

        The views get-or-create this row themselves; the suite only needs it
        present for ``workspace_integrations`` seeds. Mirrors
        ``ensure_instance``: leave the shared row behind.
        """
        row = self.db.fetchone("SELECT id FROM integrations WHERE provider = 'github'")
        if row is not None:
            return {"id": str(row["id"])}
        integration_id = _uid()
        self.db.execute(
            """INSERT INTO integrations
               (id, title, provider, network, description, author,
                webhook_url, webhook_secret, redirect_url, metadata,
                verified, created_at, updated_at)
               VALUES (%s,'GitHub','github',2,
                       '{"summary": "Mirror GitHub issues into Pi Dash projects."}',
                       '','','','','{}',true,now(),now())""",
            (integration_id,),
        )
        return {"id": integration_id}

    def ensure_github_app_config(self) -> dict[str, str]:
        """Upsert the db-sourced GitHub App identity keys, never tracked.

        Secrets (private key, webhook secret, client secret) are env-sourced
        and must be set on the server under test instead; without them the
        app endpoints answer 409. Values here are inert dummies.
        """
        values = {
            "GITHUB_APP_ID": "contract-91-app-id",
            "GITHUB_APP_SLUG": "contract-91-app-slug",
            "GITHUB_APP_CLIENT_ID": "contract-91-client-id",
        }
        for key, value in values.items():
            self.db.execute(
                """INSERT INTO instance_configurations
                   (id, key, value, category, is_encrypted, created_at, updated_at)
                   VALUES (%s,%s,%s,'GITHUB',false,now(),now())
                   ON CONFLICT (key) DO UPDATE
                   SET value = EXCLUDED.value, updated_at = now()""",
                (_uid(), key, value),
            )
        return values

    def create_workspace_integration(
        self, workspace_id: str, actor_id: str, *, connected: bool = False
    ) -> dict[str, Any]:
        """Seed a ``workspace_integrations`` row for the github provider.

        ``connected=True`` stores a non-empty token so status reads
        ``connected:true`` without ever calling GitHub; the token is an inert
        dummy (decrypting views are covered through their 409 paths instead).
        """
        integration = self.ensure_github_integration()
        token = self.create_api_token(actor_id, workspace_id, label="github-integration-shim")
        wi_id = _uid()
        if connected:
            config = (
                '{"auth_type": "pat", "token": "contract91-seeded-token", '
                '"github_user_login": "contract-octocat", '
                '"verified_at": "2026-01-01T00:00:00+00:00"}'
            )
        else:
            config = "{}"
        self.db.execute(
            """INSERT INTO workspace_integrations
               (id, metadata, config, actor_id, api_token_id,
                integration_id, workspace_id, created_at, updated_at)
               VALUES (%s,'{}',%s,%s,%s,%s,%s,now(),now())""",
            (wi_id, config, actor_id, token["id"], integration["id"], workspace_id),
        )
        self._put("workspace_integrations", wi_id)
        return {"id": wi_id}

    def create_webhook(
        self, workspace_id: str, *, url: str | None = None, is_active: bool = True
    ) -> dict[str, Any]:
        hook_id = _uid()
        hook_url = url or f"https://example.com/hook/{self.tag}-{self._n + 1}"
        self.db.execute(
            """INSERT INTO webhooks
               (id, url, is_active, secret_key, project, issue, module, cycle,
                issue_comment, is_internal, version, workspace_id,
                created_at, updated_at)
               VALUES (%s,%s,%s,%s,false,true,false,false,false,false,'v1',%s,now(),now())""",
            (hook_id, hook_url, is_active, f"pi_dash_wh_contract{self._n + 1:06d}", workspace_id),
        )
        self._put("webhooks", hook_id)
        return {"id": hook_id, "url": hook_url}

    def create_webhook_log(self, workspace_id: str, webhook_id: str) -> dict[str, Any]:
        log_id = _uid()
        self.db.execute(
            """INSERT INTO webhook_logs
               (id, event_type, request_method, request_headers, request_body,
                response_status, response_headers, response_body, retry_count,
                webhook, workspace_id, created_at, updated_at)
               VALUES (%s,'push','POST','{}','{}','200','{}','{}',0,%s,%s,now(),now())""",
            (log_id, webhook_id, workspace_id),
        )
        self._put("webhook_logs", log_id)
        return {"id": log_id}

    def create_git_provider_account(
        self, workspace_id: str, *, provider: str = "github", login: str = "contract-octocat"
    ) -> dict[str, Any]:
        account_id = _uid()
        host = "https://github.com" if provider == "github" else "https://gitlab.com"
        self.db.execute(
            """INSERT INTO git_provider_accounts
               (id, provider, host_url, auth_type, external_account_id,
                external_account_login, display_name, capabilities,
                credential_config, status, verified_at, last_check_error,
                metadata, workspace_id, created_at, updated_at)
               VALUES (%s,%s,%s,'pat',%s,%s,%s,'{}','{}','connected',now(),'', '{}',%s,now(),now())""",
            (account_id, provider, host, f"contract:{account_id[:8]}", login, login, workspace_id),
        )
        self._put("git_provider_accounts", account_id)
        return {"id": account_id}

    def create_github_repo_sync(
        self, workspace_id: str, project_id: str, actor_id: str, wi_id: str
    ) -> dict[str, Any]:
        """Seed a bound ``github_repositories`` + ``github_repository_syncs`` pair."""
        repo_id = _uid()
        repository_id = random.randint(10_000_000, 99_999_999)
        owner, name = f"contract-owner-{self.tag[:6]}", f"contract-repo-{self._n + 1}"
        self.db.execute(
            """INSERT INTO github_repositories
               (id, name, url, config, repository_id, owner,
                project_id, workspace_id, created_at, updated_at)
               VALUES (%s,%s,%s,'{}',%s,%s,%s,%s,now(),now())""",
            (
                repo_id,
                name,
                f"https://github.com/{owner}/{name}",
                repository_id,
                owner,
                project_id,
                workspace_id,
            ),
        )
        self._put("github_repositories", repo_id)
        sync_id = _uid()
        self.db.execute(
            """INSERT INTO github_repository_syncs
               (id, credentials, actor_id, project_id, repository_id,
                workspace_id, workspace_integration_id,
                is_sync_enabled, last_sync_error, created_at, updated_at)
               VALUES (%s,'{}',%s,%s,%s,%s,%s,false,'',now(),now())""",
            (sync_id, actor_id, project_id, repo_id, workspace_id, wi_id),
        )
        self._put("github_repository_syncs", sync_id)
        return {
            "id": sync_id,
            "repository_id": repository_id,
            "owner": owner,
            "name": name,
            "url": f"https://github.com/{owner}/{name}",
        }

    def create_github_app_installation(self, wi_id: str) -> dict[str, Any]:
        """Seed a ``github_app_installations`` row with a unique install id."""
        installation_id = random.randint(1_000_000, 9_999_999)
        row_id = _uid()
        self.db.execute(
            """INSERT INTO github_app_installations
               (id, workspace_integration_id, installation_id,
                account_login, account_type, repository_selection,
                repository_count, permissions, events, last_check_error,
                created_at, updated_at)
               VALUES (%s,%s,%s,'contract-octocat','Organization','selected',
                       3,'{}','[]','',now(),now())""",
            (row_id, wi_id, installation_id),
        )
        self._put("github_app_installations", row_id)
        return {"id": row_id, "installation_id": installation_id}

    # -- api-v1 work items (PIDASHCONV-76) ---------------------------------
    # api-v1 authenticates with ``X-Api-Key`` (APIKeyAuthentication against
    # ``api_tokens``). These factories cover the work-items domain surface
    # (links, relations, attachments, pages, activity, PR/review links,
    # pods, agent runs); shared membership/token/page factories live above.
    def create_issue_link(
        self, workspace_id: str, project_id: str, issue_id: str, *, url: str = "https://example.com/spec"
    ) -> dict[str, Any]:
        link_id = _uid()
        self.db.execute(
            """INSERT INTO issue_links
               (id, title, url, issue_id, project_id, workspace_id, metadata,
                created_at, updated_at)
               VALUES (%s,'Contract link',%s,%s,%s,%s,'{}',now(),now())""",
            (link_id, url, issue_id, project_id, workspace_id),
        )
        self._put("issue_links", link_id)
        return {"id": link_id}

    def create_issue_relation(
        self,
        workspace_id: str,
        project_id: str,
        issue_id: str,
        related_issue_id: str,
        *,
        relation_type: str = "relates_to",
    ) -> dict[str, Any]:
        relation_id = _uid()
        self.db.execute(
            """INSERT INTO issue_relations
               (id, relation_type, issue_id, project_id, related_issue_id,
                workspace_id, created_at, updated_at)
               VALUES (%s,%s,%s,%s,%s,%s,now(),now())""",
            (relation_id, relation_type, issue_id, project_id, related_issue_id, workspace_id),
        )
        self._put("issue_relations", relation_id)
        return {"id": relation_id}

    def create_issue_attachment(
        self, workspace_id: str, project_id: str, issue_id: str, *, asset: str = "contract-asset"
    ) -> dict[str, Any]:
        # The attachment endpoints read ``file_assets`` (entity
        # ``ISSUE_ATTACHMENT``), not the legacy ``issue_attachments`` table.
        attachment_id = _uid()
        self.db.execute(
            """INSERT INTO file_assets
               (id, attributes, asset, entity_type, is_deleted, is_archived,
                is_uploaded, issue_id, project_id, workspace_id, size,
                created_at, updated_at)
               VALUES (%s,'{}',%s,'ISSUE_ATTACHMENT',false,false,true,%s,%s,%s,10,now(),now())""",
            (attachment_id, asset, issue_id, project_id, workspace_id),
        )
        self._put("file_assets", attachment_id)
        return {"id": attachment_id}

    def link_page_to_project(self, page_id: str, project_id: str, workspace_id: str) -> dict[str, Any]:
        # Page reads scope to the project through ``project_pages``.
        row_id = _uid()
        self.db.execute(
            """INSERT INTO project_pages
               (id, page_id, project_id, workspace_id, created_at, updated_at)
               VALUES (%s,%s,%s,%s,now(),now())""",
            (row_id, page_id, project_id, workspace_id),
        )
        self._put("project_pages", row_id)
        return {"id": row_id}

    def create_issue_activity(
        self,
        workspace_id: str,
        project_id: str,
        issue_id: str,
        *,
        verb: str = "created",
        comment: str = "contract activity",
    ) -> dict[str, Any]:
        activity_id = _uid()
        self.db.execute(
            """INSERT INTO issue_activities
               (id, verb, comment, attachments, issue_id, project_id, workspace_id,
                created_at, updated_at)
               VALUES (%s,%s,%s,'{}',%s,%s,%s,now(),now())""",
            (activity_id, verb, comment, issue_id, project_id, workspace_id),
        )
        self._put("issue_activities", activity_id)
        return {"id": activity_id}

    def create_github_pr_link(
        self, workspace_id: str, project_id: str, issue_id: str, *, pr_number: int = 7
    ) -> dict[str, Any]:
        link_id = _uid()
        self.db.execute(
            """INSERT INTO github_pull_request_links
               (id, repo_owner, repo_name, pr_number, url, title, state, merged,
                draft, issue_id, project_id, workspace_id, created_at, updated_at)
               VALUES (%s,'contract-owner','contract-repo',%s,'https://github.com/contract-owner/contract-repo/pull/7',
                       'Contract PR','open',false,false,%s,%s,%s,now(),now())""",
            (link_id, pr_number, issue_id, project_id, workspace_id),
        )
        self._put("github_pull_request_links", link_id)
        return {"id": link_id}

    def create_code_review_link(
        self, workspace_id: str, project_id: str, issue_id: str, *, external_iid: str = "7"
    ) -> dict[str, Any]:
        link_id = _uid()
        self.db.execute(
            """INSERT INTO git_code_review_links
               (id, provider, host_url, namespace, repo_name, repo_external_id,
                external_id, external_iid, url, title, state, merged, draft, metadata,
                issue_id, project_id, workspace_id, created_at, updated_at)
               VALUES (%s,'github','https://github.com','contract-owner','contract-repo','',
                       'contract-e1',%s,'https://github.com/contract-owner/contract-repo/pull/7',
                       'Contract review','open',false,false,'{}',%s,%s,%s,now(),now())""",
            (link_id, external_iid, issue_id, project_id, workspace_id),
        )
        self._put("git_code_review_links", link_id)
        return {"id": link_id}

    def create_pod(self, workspace_id: str, project_id: str, *, name: str = "Contract pod") -> dict[str, Any]:
        pod_id = _uid()
        self.db.execute(
            """INSERT INTO pod
               (id, name, description, is_default, workspace_id, project_id,
                created_at, updated_at)
               VALUES (%s,%s,'',false,%s,%s,now(),now())""",
            (pod_id, name, workspace_id, project_id),
        )
        self._put("pod", pod_id)
        return {"id": pod_id}

    def create_agent_run(
        self,
        workspace_id: str,
        created_by_id: str,
        pod_id: str,
        work_item_id: str,
        *,
        status: str = "running",
    ) -> dict[str, Any]:
        run_id = _uid()
        self.db.execute(
            """INSERT INTO agent_run
               (id, status, prompt, run_config, required_capabilities, thread_id,
                error, workspace_id, work_item_id, pod_id, created_by_id, llm_model,
                refusal_category, trigger, executor_kind, dispatch_attempts,
                cancel_reason, error_code, tool_plan, phase_kind, agent_metadata,
                usage, created_at)
               VALUES (%s,%s,'contract prompt','{}','{}','contract-thread','',
                       %s,%s,%s,%s,'contract-model','', 'human', 'local_runner', 0,
                       '','', '{}', 'work', '{}', '{"input": 0, "output": 0, "total": 0}', now())""",
            (run_id, status, workspace_id, work_item_id, pod_id, created_by_id),
        )
        self._put("agent_run", run_id)
        return {"id": run_id}


# --- PIDASHCONV-83 (app project/state/estimate oracle) ---
# Union with the baseline above (see workpad for the full rationale).
def _now_iso() -> str:
    return datetime.now(timezone.utc).isoformat()


def _tag8() -> str:
    return uuid.uuid4().hex[:8]


# 600000 matches Django 4.2's PBKDF2PasswordHasher. Task suites seed unusable
# passwords; HTTP suites seed a known one (sessions are forged, never verified).
PBKDF2_ITERATIONS = 600000


def password_hash(password: str) -> str:
    salt = secrets.token_hex(11)[:22]
    digest = hashlib.pbkdf2_hmac("sha256", password.encode(), salt.encode(), PBKDF2_ITERATIONS)
    return "pbkdf2_sha256$%d$%s$%s" % (
        PBKDF2_ITERATIONS, salt, base64.b64encode(digest).decode("ascii").strip())


def user(conn, username: str, **over) -> dict:
    tag = _tag8()
    values = {
        "username": f"{username}-{tag}",
        "email": f"{username}-{tag}@example.com",
        "password": password_hash("contract-suite-password"),
        "display_name": username,
        "first_name": "",
        "last_name": "",
        "avatar": "",
        "date_joined": _now_iso(),
        "is_active": True,
        "last_location": "",
        "created_location": "",
        "is_superuser": False,
        "is_managed": False,
        "is_password_expired": False,
        "is_staff": False,
        "is_email_verified": False,
        "is_password_autoset": False,
        "token": "",
        "user_timezone": "UTC",
        "last_login_ip": "127.0.0.1",
        "last_logout_ip": "127.0.0.1",
        "last_login_medium": "email",
        "last_login_uagent": "",
        "is_bot": False,
        "is_email_valid": True,
        "is_password_reset_required": False,
    }
    values.update(over)
    return insert_row(conn, "users", values)


def workspace(conn, slug: str, owner_id, **over) -> dict:
    values = {
        "name": f"contract {slug}",
        "slug": f"{slug}-{uuid.uuid4().hex[:8]}",
        "owner_id": str(owner_id),
        "timezone": "UTC",
        "background_color": "#FFFFFF",
    }
    values.update(over)
    return insert_row(conn, "workspaces", values)


def webhook(conn, workspace_id, url: str, **flags) -> dict:
    values = {
        "workspace_id": str(workspace_id),
        "url": url,
        "is_active": True,
        "secret_key": uuid.uuid4().hex,
        "project": False,
        "issue": False,
        "module": False,
        "cycle": False,
        "issue_comment": False,
        "is_internal": False,
        "version": "v1",
    }
    values.update(flags)
    return insert_row(conn, "webhooks", values)


def email_log(conn, receiver_id, actor_id, entity_id: str, **extra) -> dict:
    values = {
        "receiver_id": str(receiver_id),
        "triggered_by_id": str(actor_id),
        "entity_identifier": entity_id,
        "entity_name": "issue",
        "entity": "issue",
        "data": {
            "issue_activity": {"field": "state", "old_value": "a", "new_value": "b"}
        },
    }
    values.update(extra)
    return insert_row(conn, "email_notification_logs", values)


def workspace_member(conn, workspace_id, user_id, role: int = 20, **over) -> dict:
    values = {
        "workspace_id": str(workspace_id),
        "member_id": str(user_id),
        "role": role,
        "is_active": True,
        "view_props": {},
        "default_props": {},
        "issue_props": {},
        "explored_features": {},
        "getting_started_checklist": {},
        "tips": {},
    }
    values.update(over)
    return insert_row(conn, "workspace_members", values)


def project(conn, workspace_id, name: str = "Contract Project", **over) -> dict:
    tag = _tag8()
    values = {
        "name": "%s %s" % (name, tag),
        "identifier": "CP%s" % tag.upper(),
        "description": "",
        "network": 0,
        "workspace_id": str(workspace_id),
        "cycle_view": False,
        "module_view": False,
        "issue_views_view": False,
        "page_view": False,
        "intake_view": False,
        "archive_in": 0,
        "close_in": 0,
        "logo_props": {},
        "is_time_tracking_enabled": False,
        "is_issue_type_enabled": False,
        "guest_view_all_features": False,
        "timezone": "UTC",
        "members_can_edit_states": False,
        "repo_url": "",
        "base_branch": "",
        "agent_default_interval_seconds": 0,
        "agent_default_max_ticks": 0,
        "agent_ticking_enabled": False,
        "is_default": False,
        "agent_review_default_interval_seconds": 0,
        "default_agent_executor": "",
        "agent_test_default_interval_seconds": 0,
    }
    values.update(over)
    return insert_row(conn, "projects", values)


def project_member(
    conn, project_id, workspace_id, user_id, role: int = 20, **over
) -> dict:
    values = {
        "project_id": str(project_id),
        "workspace_id": str(workspace_id),
        "member_id": str(user_id),
        "role": role,
        "is_active": True,
        "view_props": {},
        "default_props": {},
        "preferences": {},
        "sort_order": 0.0,
    }
    values.update(over)
    return insert_row(conn, "project_members", values)


def project_user_property(conn, project_id, workspace_id, user_id, **over) -> dict:
    values = {
        "project_id": str(project_id),
        "workspace_id": str(workspace_id),
        "user_id": str(user_id),
        "display_properties": {},
        "display_filters": {},
        "filters": {},
        "rich_filters": {},
        "preferences": {},
        "sort_order": 0.0,
    }
    values.update(over)
    return insert_row(conn, "project_user_properties", values)


def state(
    conn, project_id, workspace_id, name: str = "Contract State", **over
) -> dict:
    tag = _tag8()
    values = {
        "name": "%s %s" % (name, tag),
        "description": "",
        "color": "#000000",
        "slug": "ct-state-%s" % tag,
        "project_id": str(project_id),
        "workspace_id": str(workspace_id),
        "sequence": 1.0,
        "group": "unstarted",
        "default": False,
        "is_triage": False,
    }
    values.update(over)
    return insert_row(conn, "states", values)


def estimate(conn, project_id, workspace_id, name: str = "Contract Estimate", **over) -> dict:
    tag = _tag8()
    values = {
        "name": "%s %s" % (name, tag),
        "description": "",
        "project_id": str(project_id),
        "workspace_id": str(workspace_id),
        "type": "points",
        "last_used": False,
    }
    values.update(over)
    return insert_row(conn, "estimates", values)


def estimate_point(
    conn, estimate_id, project_id, workspace_id, key: int = 1, value: str = "1", **over
) -> dict:
    values = {
        "estimate_id": str(estimate_id),
        "project_id": str(project_id),
        "workspace_id": str(workspace_id),
        "key": key,
        "value": value,
        "description": "",
    }
    values.update(over)
    return insert_row(conn, "estimate_points", values)


def project_invite(conn, project_id, workspace_id, email: str | None = None, **over) -> dict:
    tag = _tag8()
    values = {
        "email": email or "invite-%s@example.com" % tag,
        "accepted": False,
        "token": uuid.uuid4().hex,
        "role": 15,
        "project_id": str(project_id),
        "workspace_id": str(workspace_id),
    }
    values.update(over)
    return insert_row(conn, "project_member_invites", values)
