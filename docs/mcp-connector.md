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
tools and eight write tools. They are thin wrappers over the `/api/v1/`
scheduler endpoints, scoped by the caller's normal workspace/project
permissions: a project member can read the schedulers of that project and
their run history; a project **admin** can also install a scheduler on the
project, edit that install, trigger it and uninstall it; a workspace
**admin** can create scheduler definitions and edit or delete user-created
ones; anyone else gets the API's normal permission error.

| Tool                              | REST endpoint it wraps                                                                                                            |
| --------------------------------- | --------------------------------------------------------------------------------------------------------------------------------- |
| `pidash_list_schedulers`          | `GET /api/v1/workspaces/<slug>/schedulers/` (workspace-wide), or `GET .../projects/<project>/schedulers/` when a project is given |
| `pidash_get_scheduler`            | `GET /api/v1/workspaces/<slug>/projects/<project>/schedulers/<id>/`                                                               |
| `pidash_list_scheduler_runs`      | `GET /api/v1/workspaces/<slug>/projects/<project>/schedulers/<id>/runs/`                                                          |
| `pidash_install_scheduler`        | `POST /api/v1/workspaces/<slug>/projects/<project>/schedulers/`                                                                   |
| `pidash_update_scheduler_binding` | `PATCH /api/v1/workspaces/<slug>/projects/<project>/schedulers/<id>/binding/`                                                     |
| `pidash_run_scheduler_now`        | `POST /api/v1/workspaces/<slug>/projects/<project>/schedulers/<id>/run-now/`                                                      |
| `pidash_uninstall_scheduler`      | `DELETE /api/v1/workspaces/<slug>/projects/<project>/schedulers/<id>/binding/`                                                    |
| `pidash_create_scheduler`         | `POST /api/v1/workspaces/<slug>/schedulers/`                                                                                      |
| `pidash_update_scheduler`         | `PATCH /api/v1/workspaces/<slug>/schedulers/<id>/`                                                                                |
| `pidash_delete_scheduler`         | `DELETE /api/v1/workspaces/<slug>/schedulers/<id>/`                                                                               |

`<id>` in a path is always the scheduler's id; the install body takes the
scheduler's id or its slug. The project-scoped `/binding/` and `/run-now/`
routes act on that scheduler's install (its _binding_) on `<project>`; the
workspace-scoped `<id>` route acts on the shared definition. The `pidash`
CLI exposes the same operations as `pidash scheduler list | get | runs |
install | update | run-now | uninstall | create | edit | delete`, where
`update` with `--prompt-file`, `--name` or `--description` is also a
definition edit. Every CLI command accepts a slug in place of the id.

### Shapes

- **List** — each row is a scheduler definition (`id`, `slug`, `name`,
  `description`, `prompt`, `source`, `is_builtin`, `is_enabled`, `color`)
  with its
  `bindings`: the per-project installs, each carrying the cadence
  (`dtstart` / `tzid` / `rrule` / `rdates` / `exdates`), `enabled`,
  `outcome_mode`, the pod override (`pod` / `pod_name`), `next_run_at`
  (next occurrence) and the last occurrence (`last_run`,
  `last_run_status`, `last_run_started_at`, `last_run_ended_at`,
  `last_error`). The workspace-wide form includes bindings only for
  projects the caller is a member of.
- **Get** — the same shape for one scheduler, 404 when it is not installed
  on the given project.
- **`is_builtin`** — `true` for the seeded built-in schedulers (their slug
  is in the built-in catalog, which the reseed matches on). Use it, not
  `source`, to tell built-in from user-created: `source` reads `builtin`
  for user-created schedulers too (PDASHOSS01-290).
- **Runs** — recent agent runs the scheduler fired on that project, newest
  first: `id`, `status`, `trigger`, `phase_kind`, `error_code`,
  `created_at` / `started_at` / `ended_at`, and `issues` — the work items
  the run wrote comments to (`id`, `identifier`, `name`), which is the
  durable audit trail of what a scheduled run touched. `per_page`
  (default 30, max 100) bounds the page.

### Install and uninstall

- **Install** (`pidash_install_scheduler`) — body: `scheduler` (the
  scheduler's id or slug, required), `dtstart` (required), `rrule` (`""` is
  a single shot at `dtstart`), and optionally `tzid`, `rdates`, `exdates`,
  `extra_context`, `outcome_mode`, `pod` and `enabled`. Values are validated
  by the web app's binding serializer, and `next_run_at` is set on install
  exactly as the web app sets it. The caller is recorded as the install's
  `actor`. Answers `201` with the binding in the read shape. Any other key
  (`project`, `next_run_at`, `prompt`, …) is a `400` with `writable_fields`.
  `404` when no live scheduler in the workspace has that id or slug; `409`
  with `code: "scheduler_disabled"` when the definition is disabled, and
  `code: "already_installed"` when it is already installed on the project
  (edit that install instead). Built-in schedulers can be installed.
- **Uninstall** (`pidash_uninstall_scheduler`) — soft-deletes the
  scheduler's install on the project, as the web app's uninstall does;
  `204`. The definition and its installs on other projects are untouched.
  `404` when it is not installed there. Built-in schedulers can be
  uninstalled. An optional `If-Match` header with the install's
  `updated_at` turns a concurrent change into a `412`.

Both need project `ADMIN`, like the binding writes below.

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

### Definition writes

`pidash_create_scheduler`, `pidash_update_scheduler` and
`pidash_delete_scheduler` act on the scheduler definition itself. They
require **workspace** `ADMIN` — the same rule as the web app's scheduler
routes, and a stricter one than the binding tools, because a definition is
shared by every project that installs it. A project admin who is not a
workspace admin gets `403`, as do members, guests and non-members; no API
key gets `401`.

- **Create** (`pidash_create_scheduler`) — body: `name`, `slug` and `prompt`
  (required), and optionally `description`, `color` (`#rrggbb`) and
  `is_enabled` (default `true`). Validated by the web app's serializer: a slug
  already used by a live scheduler in the workspace is a `400` field error
  (`{"slug": [...]}`); so is a built-in slug, which the reseed would take
  over. Any other key (`source`, `id`, …) is a `400` with `writable_fields`.
  Answers `201` with the definition in the read shape (`is_builtin: false`,
  empty `bindings`). The caller is recorded as the row's `created_by`. It
  fires nowhere until installed.
- **Update** (`pidash_update_scheduler`) — a partial edit of `prompt`,
  `name`, `description`, `color` and `is_enabled`, validated by the web
  app's serializer. `prompt` replaces the whole text and cannot be blank.
  `is_enabled: false` stops every install of it from firing. Any other key —
  `slug`, `source`, binding fields — fails the whole request with `400` and
  a `writable_fields` list.
- **Delete** (`pidash_delete_scheduler`) — soft-deletes the definition and,
  in the same transaction, every live install of it, as the web app's delete
  does; `204`. Read `bindings` from `pidash_list_schedulers` first to see
  what it removes. The slug becomes free for a new scheduler. An optional
  `If-Match` header with the scheduler's `updated_at` turns a concurrent
  change into a `412`.
- **Built-in schedulers are rejected** by update and delete with `409` and
  `code: "builtin_scheduler"`: the reseed matches them by slug, overwrites
  their name, description and prompt and recreates a deleted one. Put
  per-project instructions in the binding's `extra_context` instead. They can
  still be installed and uninstalled.
- **Update response** — the definition in the read shape, with `bindings`
  holding every live install, plus `affected_projects` (`id`, `identifier`, `name`,
  `binding_enabled` for each project the scheduler is installed on — the
  projects whose next run uses the new text) and `previous`, the pre-write
  value of every field sent.
- **Concurrency** — as for bindings, with the _scheduler's_ `updated_at` as
  the token (`expected_updated_at` or `If-Match`); a stale token gets `412`.

The audit record is the same as for binding writes: the request-log row holds
each call's body and response (for an update, the new values and
`previous`). The rows themselves also carry the caller: `created_by` /
`updated_by` on a definition, and `actor` on an install.

### Boundaries

- **Not exposed.** Changing a definition's `slug` remains a web-app action.
  The project-scoped definition route
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
