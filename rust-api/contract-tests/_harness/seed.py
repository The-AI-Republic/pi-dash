"""Raw-SQL row factories for contract suites. No ORM, no Django imports."""

import secrets
import uuid
from datetime import datetime, timedelta, timezone

from . import djangocrypto

def _now():
    return datetime.now(timezone.utc)


def _uid() -> str:
    return str(uuid.uuid4())


def _tag(prefix: str) -> str:
    return f"{prefix}-{uuid.uuid4().hex[:10]}"


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
              description="", sequence_id=None):
        """A minimal issue row with FTS-visible text columns set."""
        return self._insert(
            "issues",
            name=name or f"CT87 issue {uuid.uuid4().hex[:6]}",
            description_json="{}",
            description_html=f"<p>{description}</p>" if description else "<p></p>",
            description_stripped=description or None,
            priority="none",
            sequence_id=sequence_id if sequence_id is not None else 1,
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
