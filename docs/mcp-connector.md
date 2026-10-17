# Pi Dash MCP connector

The Pi Dash MCP connector exposes work items, projects, comments and workpads to
MCP-capable AI clients (e.g. Claude) as tools. It is a thin wrapper over the same
public REST surface (`/api/v1/`) that the `pidash` CLI uses, so tool responses
mirror the REST serializers documented below.

## Issue web URL (`url`)

Every issue-returning tool includes an absolute, human-clickable `url` pointing
at the issue in the web UI, so an agent that has just filed or fetched work can
hand the user a link straight from the response.

Tools that carry the field:

| Tool                   | `url` points to                   |
| ---------------------- | --------------------------------- |
| `pidash_get_issue`     | the issue                         |
| `pidash_create_issue`  | the newly created issue           |
| `pidash_update_issue`  | the issue                         |
| `pidash_list_issues`   | each result row's issue           |
| `pidash_search_issues` | each result row's issue           |
| `pidash_comment_issue` | the issue the comment belongs to  |
| `pidash_list_comments` | the issue each comment belongs to |

The same field flows through the `pidash` CLI: `pidash issue get <PROJ-123>`
prints the REST payload verbatim, so its JSON includes `url` as well.

### Shape

The value uses the canonical browse route, which the web UI resolves directly:

```
<web-base-url>/<workspace-slug>/browse/<PROJ>-<sequence_id>
```

for example `https://pi-dash.example.com/eng/browse/ENG-42`.

### Where the base URL comes from

The base URL is taken from **deployment configuration** — the `WEB_URL` setting
(falling back to `APP_BASE_URL`) — never from the inbound request host. An MCP
server can sit behind a proxy or be reached over a non-public hostname, and
either would produce a link that does not work for the user.

### When it is omitted

If no web base URL is configured, the `url` field is **omitted entirely** rather
than emitted as a relative or otherwise broken link. A missing `url` is easy for
a client to handle; a broken one is not. Clients should treat `url` as optional.

There is no per-comment anchor in the UI, so comment tools link to the parent
issue rather than to the individual comment.

## Schedulers

Schedulers — workspace-level scheduler definitions installed onto projects,
firing agent runs on a cadence — are exposed to MCP clients as three read
tools and three write tools. They are thin wrappers over the `/api/v1/`
scheduler endpoints, scoped by the caller's normal workspace/project
permissions: a project member can read the schedulers of that project and
their run history, a project **admin** can also edit the project's install and
trigger it, a workspace **admin** can edit a user-created scheduler's
definition, and anyone else gets the API's normal permission error.

| Tool                              | REST endpoint it wraps                                                                                                            |
| --------------------------------- | --------------------------------------------------------------------------------------------------------------------------------- |
| `pidash_list_schedulers`          | `GET /api/v1/workspaces/<slug>/schedulers/` (workspace-wide), or `GET .../projects/<project>/schedulers/` when a project is given |
| `pidash_get_scheduler`            | `GET /api/v1/workspaces/<slug>/projects/<project>/schedulers/<id>/`                                                               |
| `pidash_list_scheduler_runs`      | `GET /api/v1/workspaces/<slug>/projects/<project>/schedulers/<id>/runs/`                                                          |
| `pidash_update_scheduler_binding` | `PATCH /api/v1/workspaces/<slug>/projects/<project>/schedulers/<id>/binding/`                                                     |
| `pidash_run_scheduler_now`        | `POST /api/v1/workspaces/<slug>/projects/<project>/schedulers/<id>/run-now/`                                                      |
| `pidash_update_scheduler`         | `PATCH /api/v1/workspaces/<slug>/schedulers/<id>/`                                                                                |

`<id>` is always the scheduler's id. The two project-scoped write routes act
on that scheduler's install (its _binding_) on `<project>`; the
workspace-scoped one acts on the shared definition. The `pidash` CLI exposes
the same operations as `pidash scheduler list | get | runs | update |
run-now`, where `update` with `--prompt-file`, `--name` or `--description`
is the definition edit.

### Shapes

- **List** — each row is a scheduler definition (`id`, `slug`, `name`,
  `description`, `prompt`, `source`, `is_enabled`, `color`) with its
  `bindings`: the per-project installs, each carrying the cadence
  (`dtstart` / `tzid` / `rrule` / `rdates` / `exdates`), `enabled`,
  `outcome_mode`, the pod override (`pod` / `pod_name`), `next_run_at`
  (next occurrence) and the last occurrence (`last_run`,
  `last_run_status`, `last_run_started_at`, `last_run_ended_at`,
  `last_error`). The workspace-wide form includes bindings only for
  projects the caller is a member of.
- **Get** — the same shape for one scheduler, 404 when it is not installed
  on the given project.
- **Runs** — recent agent runs the scheduler fired on that project, newest
  first: `id`, `status`, `trigger`, `phase_kind`, `error_code`,
  `created_at` / `started_at` / `ended_at`, and `issues` — the work items
  the run wrote comments to (`id`, `identifier`, `name`), which is the
  durable audit trail of what a scheduled run touched. `per_page`
  (default 30, max 100) bounds the page.

### Binding writes

Both binding tools require project `ADMIN` (or a workspace admin who is a member
of the project) — the same rule as the web app's binding edit. Project
members, guests and non-members get `403`; a request without an API key gets
`401`.

- **Update binding** (`pidash_update_scheduler_binding`) — a partial edit:
  send only the fields to change, from `extra_context`, `enabled`, the cadence
  (`dtstart` / `rrule` / `tzid` / `rdates` / `exdates`), `outcome_mode` and
  `pod` (`null` clears the override). Any other key — `prompt`, `name`,
  `next_run_at`, `scheduler`, … — fails the whole request with `400` and a
  `writable_fields` list; nothing is silently ignored. Values are validated
  exactly as the web app validates them (RRULE syntax, IANA `tzid`, 16 KiB
  `extra_context` cap, the pod must belong to the project), and a cadence
  change recomputes `next_run_at`. `extra_context` replaces the whole text —
  read it with `pidash_get_scheduler` first.
- **Run now** (`pidash_run_scheduler_now`) — sets the binding's `next_run_at`
  to now, so the scheduler's scanner fires it on its next tick (it ticks once
  a minute); the fire path then advances `next_run_at` along the unchanged
  rule, so the regular schedule resumes. No body is needed. `409` with a
  `code` when that tick could not produce a run: `binding_disabled`,
  `scheduler_disabled` (the workspace definition is off), `run_active` (a run
  from this binding is queued or in flight), or `no_future_occurrence` (a
  single shot or bounded rule that is used up — firing it would only make the
  scanner disable the binding).

**Response and audit.** Both return the binding in the read shape plus
`previous`: the pre-write value of every field the call changed (and of
`next_run_at` when it moved). Sending `previous` back through the update tool
reverts the edit. The external API's request log (`api_activity_logs`) stores
each call's request and response body, so the log row for a write holds both
the new and the prior values; it follows that log's normal retention.

**Concurrency (optional).** Pass the binding's `updated_at`, as last read,
either as `expected_updated_at` in the body or in an `If-Match` header. If the
binding changed in between, the write is refused with `412` and the current
`updated_at`, instead of overwriting the other edit. Without a token the last
write wins.

### Definition write

`pidash_update_scheduler` edits the scheduler definition itself. It requires
**workspace** `ADMIN` — the same rule as the web app's scheduler edit, and a
stricter one than the binding tools, because a definition is shared by every
project that installs it. A project admin who is not a workspace admin gets
`403`, as do members, guests and non-members; no API key gets `401`.

- **Fields** — a partial edit of `prompt`, `name` and `description`, validated
  by the web app's serializer. `prompt` replaces the whole text and cannot be
  blank. Any other key — `slug`, `color`, `is_enabled`, `source`, binding
  fields — fails the whole request with `400` and a `writable_fields` list.
- **Built-in schedulers are rejected** with `409` and
  `code: "builtin_scheduler"`: their name, description and prompt are
  overwritten whenever the built-ins are reseeded. Put per-project
  instructions in the binding's `extra_context` instead. A scheduler is
  treated as built-in when its `source` is `builtin`. **Known limitation
  (PDASHOSS01-290):** `source` currently defaults to `builtin` for schedulers
  created in the web app too, so until that is fixed this check rejects those
  as well.
- **Response** — the definition in the read shape, with `bindings` holding
  every live install, plus `affected_projects` (`id`, `identifier`, `name`,
  `binding_enabled` for each project the scheduler is installed on — the
  projects whose next run uses the new text) and `previous`, the pre-write
  value of every field sent.
- **Concurrency** — as for bindings, with the _scheduler's_ `updated_at` as
  the token (`expected_updated_at` or `If-Match`); a stale token gets `412`.

The audit record is the same as for binding writes: the request-log row holds
the new values and `previous`.

### Boundaries

- **Not exposed.** Creating, deleting, enabling or disabling a scheduler
  definition, changing its `slug` or `color`, and installing or uninstalling
  it on a project remain web-app actions. The project-scoped definition route
  (`.../projects/<project>/schedulers/<id>/`) accepts only `GET`.
- **No secrets.** The serializers whitelist fields explicitly; scheduler
  configuration carries no credentials today, and a future credential
  column would not flow through by default.
- Issue attribution in `runs` is comment-based (`speaker_agent_run_id`):
  an issue a run created but never commented on is not listed against the
  run, because no durable run→issue creation link exists yet. The `issues`
  list is also scoped to projects the caller is a member of — a comment
  the run left on an issue in a project the caller cannot access is
  omitted, matching normal issue visibility.
