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

## Schedulers (read-only)

Schedulers — workspace-level scheduler definitions installed onto projects,
firing agent runs on a cadence — are exposed to MCP clients as three
**read-only** tools. They are thin wrappers over the `/api/v1/` scheduler
endpoints, scoped by the caller's normal workspace/project permissions: a
project member can read the schedulers of that project and their run history;
a non-member gets the API's normal permission error.

| Tool                         | REST endpoint it wraps                                                                                                            |
| ---------------------------- | --------------------------------------------------------------------------------------------------------------------------------- |
| `pidash_list_schedulers`     | `GET /api/v1/workspaces/<slug>/schedulers/` (workspace-wide), or `GET .../projects/<project>/schedulers/` when a project is given |
| `pidash_get_scheduler`       | `GET /api/v1/workspaces/<slug>/projects/<project>/schedulers/<id>/`                                                               |
| `pidash_list_scheduler_runs` | `GET /api/v1/workspaces/<slug>/projects/<project>/schedulers/<id>/runs/`                                                          |

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

### Boundaries

- **No writes.** Creating, editing, enabling/disabling, or triggering a
  scheduler is not exposed; write access is a follow-up with its own
  permission model.
- **No secrets.** The serializers whitelist fields explicitly; scheduler
  configuration carries no credentials today, and a future credential
  column would not flow through by default.
- Issue attribution in `runs` is comment-based (`speaker_agent_run_id`):
  an issue a run created but never commented on is not listed against the
  run, because no durable run→issue creation link exists yet.
