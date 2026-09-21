# Sub-Identities — Making the Actor Formal

> Directory: `.ai_design/create_sub_identity/`
>
> **Status:** proposed. No code yet.

## 1. Problem

Pi Dash lets humans and AI agents both act on an issue — comment, create
issues, move state, edit fields. Every one of those actions is recorded
against a **user account**, because that is the only kind of principal the
schema knows:

- `IssueComment.actor`, `IssueActivity.actor`, reactions, votes → FK to `users`
- `created_by` / `updated_by` on every model (`UserAuditModel`) → FK to `users`

An agent running on a runner authenticates with the runner owner's
credential (§2.2), so everything it does is recorded — and displayed — as
the human:

```
Rich M  commented about 1 month ago

Codex:
I'm tracing the unfinished research_2026_06_08 batch …
```

`Rich M` did not write that. A runner named e.g. `rich-mbp-1` did, running
Codex, on Rich's behalf. The `speaker_*` fields on `IssueComment`
(`speaker_type`, `speaker_label`, `speaker_agent_run_id`) were a first
patch for this, but they are:

- **comment-only** — an issue _created_ or _edited_ by a runner carries no
  trace of it; the activity feed says "Rich M created the issue";
- **unverified** — `speaker_label` is a free string and
  `speaker_agent_run_id` a bare UUID (no FK, never validated on the comment
  endpoint); any holder of the user's token can claim any label;
- **not an identity** — no row to join, filter, or render an avatar from,
  and the UI still builds the header from `actor`, demoting the real
  speaker to a bold prefix inside the body.

## 2. What exists today

### 2.1 Runners are already per-user entities

```
users
  └─ dev_machine          owner → user               one row per host
       ├─ machine_token   (user, workspace, dev_machine)   shared mt_ credential
       └─ runner          owner → user, workspace, dev_machine, pod, name   ≤ 5 per user
            └─ agent_run  runner → (SET_NULL), owner, created_by, work_item
```

`Runner` (`runner/models.py`) has a stable UUID, a display `name`, and
exactly one `owner`. `Visibility` has a single value, `PRIVATE`: a runner
only ever works for its owner. Runner rows are **hard-deleted** on removal
(`runner/services/runner_delete.py`), and `AgentRun.runner` is `SET_NULL`.

### 2.2 The write path carries the run, not the runner

All runners on one machine share one `MachineToken` per workspace. The
agent's `pidash` CLI sends it as `X-Api-Key`;
`api/middleware/api_authentication.py::validate_machine_token` resolves it
to `machine_token.user`. The token carries no runner identity.

But the daemon sets `PIDASH_RUN_ID` on the agent process, and the CLI
forwards it as `X-Pi-Dash-Run-Id` on **every write**
(`runner/src/api_client.rs`). The cloud already validates that header for
state moves: `run_belongs_to(user, run)` and `resolve_moved_by_run()` in
`api/views/issue.py`. So a write from inside a run already arrives with
_(user, verified run → run.runner)_. Only the issue-patch endpoint looks.

### 2.3 Authorship is stamped centrally

`BaseModel.save()` (`db/models/base.py`) fills `created_by` / `updated_by`
from `crum.get_current_user()` (`crum.CurrentRequestUserMiddleware`).
Server-side agents (cloud agent `cloud_agent/tools.py`, in-app assistant
`assistant/tools/*.py`) have no request and wrap their writes in
`crum.impersonate(user)`.

## 3. Design

### 3.1 The actor is a pair

```
actor = ( account       → users            always the responsible human
        , sub_identity  → sub_identities   NULL ⇒ the human acted directly )
```

- The **account** is the top identity: a real person with an email,
  workspace/project membership, and roles. It is what every existing
  `actor` / `created_by` / `updated_by` column already holds, and it keeps
  that meaning.
- A **sub-identity** is a non-human principal that acts under exactly one
  account: a runner today; the MCP connector, the cloud agent, and the
  in-app assistant as further kinds.
- Reading an actor answers both questions at once: _who did it_
  (sub-identity, or the account when NULL) and _who is the human behind it_
  (always the account).

There is deliberately **no `actors` table**. The account half is already in
every table; re-pointing those FKs at a new table would rewrite the entire
Plane-inherited schema for no gain. The sub-identity half is one new table
plus one nullable sibling column wherever an account column exists. "Actor"
is formal as a value object in code and as a serialized shape (§3.6), not
as a row.

Rejected alternative — **a bot `User` row per runner** placed in `actor`
(`User.is_bot` exists). It would need `WorkspaceMember` / `ProjectMember`
rows to pass permission checks, and would then leak into member lists,
assignee pickers, mention autocomplete, seat counts, and "my comments"
filters. It also loses the link to the responsible human unless yet
another column is added — which is this design anyway.

### 3.2 New table: `sub_identities`

```python
class SubIdentity(models.Model):
    class Kind(models.TextChoices):
        RUNNER = "runner", "Runner"
        MCP = "mcp", "MCP"
        CLOUD_AGENT = "cloud_agent", "Cloud Agent"
        ASSISTANT = "assistant", "Assistant"

    id = models.UUIDField(primary_key=True, default=uuid.uuid4, editable=False)
    owner = models.ForeignKey("db.User", on_delete=models.CASCADE, related_name="sub_identities")
    workspace = models.ForeignKey("db.Workspace", on_delete=models.CASCADE, related_name="sub_identities")
    kind = models.CharField(max_length=24, choices=Kind.choices)
    display_name = models.CharField(max_length=128)
    avatar_asset = models.ForeignKey("db.FileAsset", null=True, blank=True, on_delete=models.SET_NULL, related_name="+")
    runner = models.OneToOneField(
        "runner.Runner", null=True, blank=True, on_delete=models.SET_NULL, related_name="sub_identity"
    )
    created_at = models.DateTimeField(auto_now_add=True)
    updated_at = models.DateTimeField(auto_now=True)
    revoked_at = models.DateTimeField(null=True, blank=True)

    class Meta:
        db_table = "sub_identities"
        constraints = [
            models.UniqueConstraint(
                fields=["owner", "workspace", "kind"],
                condition=~Q(kind="runner") & Q(revoked_at__isnull=True),
                name="sub_identity_one_per_kind",
            ),
            models.CheckConstraint(
                check=Q(runner__isnull=True) | Q(kind="runner"),
                name="sub_identity_runner_kind",
            ),
        ]
        indexes = [models.Index(fields=["owner", "workspace", "revoked_at"])]
```

Decisions:

- **Its own table, not an FK straight to `runner`.** Runner rows are
  hard-deleted; history must still read "rich-mbp-1 created this issue"
  afterwards. And the non-runner kinds have no runner row.
- **Soft-delete only.** Removing a runner nulls `sub_identity.runner`
  (`SET_NULL`) and the delete service stamps `revoked_at`. The row stays, so
  records need no name snapshot, and a rename propagates the way a user's
  display name does. A revoked sub-identity can never be resolved for a new
  write (§3.4); it only renders history.
- **Workspace-scoped**, matching `runner` and `machine_token`. The same
  laptop serving two workspaces yields two sub-identities.
- **Lives in `pi_dash.db`**, not `pi_dash.runner`: it is referenced from
  the audit mixin that every model inherits, and `db` must not depend on
  `runner` at import time. The `runner` FK is a lazy string reference.
- **Lifecycle.**
  - `kind=runner`: created in the same transaction as the `Runner` row
    (enrollment paths in `runner/views/enrollment.py` and the
    machine-control create path); `display_name` mirrors `runner.name` and
    follows renames.
  - other kinds: `get_or_create` on first use, per (owner, workspace, kind).
  - the migration backfills one row per existing runner.

### 3.3 New columns on existing tables

```python
# db/mixins.py — UserAuditModel, inherited by ~every model
created_by_sub_identity = models.ForeignKey(
    "db.SubIdentity", null=True, on_delete=models.SET_NULL, db_index=False, related_name="+"
)
updated_by_sub_identity = models.ForeignKey(
    "db.SubIdentity", null=True, on_delete=models.SET_NULL, db_index=False, related_name="+"
)

# explicit-actor tables
actor_sub_identity = models.ForeignKey(
    "db.SubIdentity", null=True, on_delete=models.SET_NULL, related_name="+"
)
```

| Table                                                 | Account half (exists)            | Sub-identity half (new)                                    |
| ----------------------------------------------------- | -------------------------------- | ---------------------------------------------------------- |
| every `UserAuditModel` table                          | `created_by_id`, `updated_by_id` | `created_by_sub_identity_id`, `updated_by_sub_identity_id` |
| `issue_activities`                                    | `actor_id`                       | `actor_sub_identity_id` (indexed)                          |
| `issue_comments`                                      | `actor_id`                       | `actor_sub_identity_id` (indexed)                          |
| `issue_reactions`, `comment_reactions`, `issue_votes` | `actor_id`                       | `actor_sub_identity_id`                                    |
| `issues`                                              | `created_by_id` (mixin)          | mixin column, **plus an index** for "created by agent X"   |

- The mixin columns are `db_index=False`. A nullable, default-less column
  is a metadata-only `ADD COLUMN` in Postgres, so touching every table is
  cheap; an index per table is not. Indexes go only where we will filter:
  `issues`, `issue_comments`, `issue_activities` (added `CONCURRENTLY`).
- **No backfill.** NULL means "the human acted directly", which is the
  honest reading of every historical row. (Historical agent comments can
  optionally be upgraded from `speaker_agent_run_id → run.runner`; §6.)
- `on_delete=SET_NULL` is belt-and-braces: sub-identities are never
  hard-deleted except by the `owner` cascade when a user is deleted, at
  which point `created_by` is nulled by the same rule.

**Invariant:** `sub_identity.owner_id ==` the account column beside it.
Django has no composite FKs, so this is not a DB constraint; it holds
because exactly one function produces the pair (§3.4). A management
command `check_actor_integrity` reports violations.

### 3.4 Resolving the actor

One resolver, one thread-local, one stamp.

```python
# pi_dash/utils/actor.py
@dataclass(frozen=True)
class Actor:
    account: User
    sub_identity: SubIdentity | None

def get_current_sub_identity() -> SubIdentity | None: ...
@contextmanager
def acting_as(sub_identity): ...          # sibling of crum.impersonate
```

**Request path (external API, `X-Api-Key`).** After authentication, a
small DRF hook on `BaseAPIView.initial()` runs
`resolve_sub_identity(request)`:

1. Read `X-Pi-Dash-Run-Id`. Absent → `None` (human, script, or old CLI).
2. Load `AgentRun` + `runner` + `runner.sub_identity`. Unknown run → `None`.
3. `run_belongs_to(request.user, run)` must hold, else **400** — same rule
   and message the issue-patch endpoint uses today.
4. `run.runner.sub_identity` must exist, be un-revoked, be in the request's
   workspace, and have `owner_id == request.user.id`, else `None`.
5. Store on `request.sub_identity` and in the thread-local; clear it in
   `finalize_response`.

Unlike `resolve_moved_by_run`, this does **not** require the run to be
active on _this_ issue: an agent legitimately comments on a sub-issue, or
posts its final note a beat after finalization. Attribution needs "this is
your run", not "this run owns this issue". `resolve_moved_by_run` keeps its
stricter rule for ticker accounting and is refactored to share steps 1–3.

**Stamp.** `BaseModel.save()` sets `created_by_sub_identity` /
`updated_by_sub_identity` from `get_current_sub_identity()` exactly where
it sets `created_by` / `updated_by`. Views that set `actor=request.user`
set `actor_sub_identity=request.sub_identity` alongside. `bulk_create` and
queryset `.update()` bypass `save()`; call sites that hand-set `created_by`
today get the sibling column the same way.

**Server-side agents (no request).**

```python
with impersonate(run.created_by), acting_as(sub_identity_for(run)):
    IssueComment.objects.create(...)
```

`cloud_agent/tools.py` uses the `cloud_agent` kind; `assistant/tools/*` the
`assistant` kind. The MCP surface (`private-pi-dash`,
`pi_dash_cloud/mcp/tools.py`) calls the public API; it identifies itself
with a header the API maps to the caller's `mcp` sub-identity — the
resolver gains a second branch, the overlay gains one header. Details of
that header belong to the overlay's follow-up, not this doc.

**Celery.** Tasks have no thread-local. `issue_activity` gains a
`sub_identity_id=None` kwarg beside `actor_id`; each `IssueActivity(...)`
it builds sets `actor_sub_identity_id`. Same for `model_activity`,
notifications, and webhook payload builders. The kwarg is optional, so
in-flight tasks enqueued by the previous deploy still run.

### 3.5 Authorization

Unchanged. Every permission check keeps reading the **account**. A
sub-identity is attribution, never authority: it cannot have access its
owner lacks. Edit/delete rights on an agent's comment stay with the
account (you can edit what your runner wrote).

The schema leaves room to later _narrow_ per sub-identity ("this runner
may not delete issues"); that is out of scope and would first require the
trust upgrade in §5.

### 3.6 API shape

Additive. Existing `actor`, `actor_detail`, `created_by` keep their shape.

```json
"actor_info": {
  "account":      { "id": "…", "display_name": "Rich M", "avatar_url": "…" },
  "sub_identity": { "id": "…", "kind": "runner", "display_name": "rich-mbp-1",
                    "avatar_url": null, "revoked": false }
}
```

- Comment and activity serializers (both `api/` and `app/` surfaces) expose
  `actor_info`; issue serializers expose `created_by_info` /
  `updated_by_info` with the same shape.
- The speaker fields and both new FK columns become **read-only** on every
  client-facing serializer. Today `app/serializers/issue.py` has
  `fields = "__all__"` and accepts `speaker_*` from the browser.
- List endpoints accept `actor_sub_identity=<uuid>` /
  `created_by_sub_identity=<uuid>` filters.
- `GET /api/workspaces/<slug>/sub-identities/` lists the caller's own
  (and, for admins, the workspace's) for the filter UI and settings page.

### 3.7 UI

Rule: if `sub_identity` is present, it is the headline; the account is the
"on behalf of".

```
🤖 rich-mbp-1  commented 2 minutes ago · Codex · on behalf of Rich M
```

- `apps/web/core/components/comments/card/display.tsx` and
  `apps/space/.../comment-detail-card.tsx`: header name/avatar from the
  sub-identity (generic bot avatar until one is uploaded); drop the bold
  `Codex:` body prefix; `speaker_label` moves to a header chip. A revoked
  sub-identity renders with a muted "(removed)" suffix.
- Activity feed: "rich-mbp-1 changed state to Done", hover shows the account.
- Issue detail "Created by": same pair.
- Notifications keep addressing and attributing by account, but the
  sentence names the sub-identity ("rich-mbp-1 commented on …").
- Runner settings page shows the sub-identity (name, avatar upload).

### 3.8 Fate of `speaker_*`

| Field                  | After                                                                                                    |
| ---------------------- | -------------------------------------------------------------------------------------------------------- |
| `speaker_type`         | Derived from `sub_identity.kind` when set; kept for `system` / `integration` rows that have no identity. |
| `speaker_label`        | Kept — it names the _agent program_ (Codex, Claude Code), which is orthogonal to the runner.             |
| `speaker_agent_run_id` | Kept — deep-links to the exact run. Server-filled from the verified header, not client-supplied.         |

The CLI flags `--as-agent` / `--agent-run-id` keep working; the server
ignores a client-sent run id that disagrees with the verified header.

## 4. Worked example

The Codex comment from §1, after this change:

| Column                  | Today    | After                               |
| ----------------------- | -------- | ----------------------------------- |
| `actor_id`              | Rich M   | Rich M (permissions, edit rights)   |
| `actor_sub_identity_id` | —        | sub-identity of runner `rich-mbp-1` |
| `speaker_label`         | "Codex"  | "Codex"                             |
| `speaker_agent_run_id`  | run UUID | run UUID (now verified)             |

And an issue the same run files via `pidash issue create`:
`created_by = Rich M`, `created_by_sub_identity = rich-mbp-1`; its
"created" `IssueActivity` row carries the same pair.

## 5. Trust model

The run id is asserted by the client and verified for _ownership_, not for
_which runner sent it_: all runners on a machine share the machine token,
so a process there could present a sibling runner's run id. Both runners
belong to the same human, so the account half — the one that carries
authority and accountability — is always right. That is sufficient for
attribution.

If sub-identities ever carry permissions (§3.5), the daemon should mint a
short-lived, run-scoped token bound to `(run, runner)` and the CLI should
present that instead of the machine token. Not needed for this design.

Runners are `PRIVATE`-only today, so `runner.owner == request.user` always
holds for a legitimate write. If shared runners arrive, the account for a
run becomes `run.created_by` (the person who asked) while the sub-identity
still belongs to the runner's owner — the §3.3 invariant would have to
relax to "owner, or a runner shared with the account". Flagged, not solved.

## 6. Rollout

Each phase ships alone; columns are nullable and the API is additive.

1. **Identity + resolver.** `sub_identities` table, backfill per runner,
   create/revoke hooks in runner enrollment and delete services,
   `utils/actor.py`, `BaseAPIView` hook. No visible change.
2. **Stamp the issue surface.** Mixin columns + `actor_sub_identity` on
   comments / activities / reactions / votes; `BaseModel.save()` stamp;
   thread `sub_identity_id` through `issue_activity`; cloud agent and
   assistant use `acting_as`. Serializers expose `actor_info`; lock
   `speaker_*` to read-only.
3. **UI.** Comment header, activity feed, issue "created by", filters,
   runner settings avatar.
4. **Follow-ups.** MCP kind in the private overlay; notifications/webhook
   wording; optional one-off upgrade of historical agent comments via
   `speaker_agent_run_id → run.runner.sub_identity` (only where the runner
   still exists).

Mixed fleet: an old `pidash` binary sends no run header, so its writes
resolve to `sub_identity = NULL` and render as the human — today's
behaviour. No breakage, just no upgrade until the runner updates.

## 7. Tests

- **Unit** — resolver: no header; malformed; foreign run (400); finished
  run (resolves); run on another issue (resolves); revoked sub-identity
  (`None`); workspace mismatch (`None`). `BaseModel.save()` stamps both
  halves on create and only `updated_by_*` on update. `acting_as` nests
  and restores.
- **Contract (`api/`)** — comment create, issue create, issue patch with
  and without the header; response carries `actor_info`; client-sent
  `actor_sub_identity` / mismatched `speaker_agent_run_id` ignored.
- **Contract (`app/`)** — session client cannot set `speaker_*` or
  sub-identity fields.
- **Lifecycle** — runner create → sub-identity exists; rename propagates;
  runner delete → `revoked_at` set, row and history intact.
- **Activity task** — `sub_identity_id` lands on every `IssueActivity` row
  the task emits; omitted kwarg still works.
- **Web** — comment card renders sub-identity headline, account fallback,
  revoked state.

## 8. Open questions

1. Should a user be able to rename a sub-identity independently of
   `runner.name`? (Proposed: no — one name, edited on the runner.)
2. Should workspace admins see all sub-identities in the workspace, or
   only ones that have acted in projects they can see? (Proposed: the
   latter, derived from activity.)
3. Shared runners (§5) — defer until `Visibility` grows a second value.
