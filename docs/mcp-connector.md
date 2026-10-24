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

## Listing work items (`pidash_list_issues`)

`pidash_list_issues` wraps
`GET /api/v1/workspaces/<slug>/projects/<project>/work-items/`. That endpoint
already pages, filters and counts. The tool must pass those query parameters
through and must not drop the paging fields from the response. If the tool only
returns one page and a `count` equal to the page length, an agent can't count
or enumerate a project with more than one page of issues. It also reads a
partial answer as a complete one.

### Parameters

| Tool parameter | REST query parameter | Notes                                                                                                                                                         |
| -------------- | -------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `limit`        | `per_page`           | Page size. The REST maximum is 1000; the tool may cap it lower.                                                                                               |
| `cursor`       | `cursor`             | Opaque. Pass back the `next_cursor` from the previous response unchanged.                                                                                     |
| `order_by`     | `order_by`           | Unchanged. Keep `order_by` the same while walking pages.                                                                                                      |
| `state`        | `state`              | State name (case-insensitive) or UUID. An unknown name is a 400 that lists the project's valid states; surface it rather than returning an empty list.        |
| `state_group`  | `state_group`        | `backlog`, `unstarted`, `started`, `review`, `test`, `completed`, `cancelled`.                                                                                |
| `parent`       | `parent`             | Parent identifier (`PROJ-12`) or UUID, or `null` for top-level issues only.                                                                                   |
| `priority`     | `priority`           | `urgent`, `high`, `medium`, `low`, `none`.                                                                                                                    |
| `labels`       | `labels`             | Label name or UUID.                                                                                                                                           |
| `assignees`    | `assignees`          | User UUIDs.                                                                                                                                                   |
| `fields`       | `fields`             | Comma-separated result fields, e.g. `id,sequence_id,name,state,priority,parent`. Leaving out `description_html` cuts a 100-issue page from ~650 kB to ~25 kB. |

Every filter takes a comma-separated list. Values within one filter OR
together, and different filters AND together. For example, `state=Backlog,Todo`
with `priority=high` returns high-priority issues that are in either Backlog or
Todo.

Filters and `cursor` apply to a single project. When `project_id` is omitted
(the workspace-wide form), the tool should reject them with an error instead
of silently ignoring them.

### Response

The tool returns these fields from the REST envelope:

| Field         | REST source         | Meaning                                                                                       |
| ------------- | ------------------- | --------------------------------------------------------------------------------------------- |
| `results`     | `results`           | The current page.                                                                             |
| `count`       | `count`             | Number of results **on this page**. Not the total.                                            |
| `total_count` | `total_count`       | Total number of issues matching the filters, across all pages. Use this to answer "how many". |
| `has_more`    | `next_page_results` | `true` when another page exists.                                                              |
| `next_cursor` | `next_cursor`       | Cursor for the next page. Only meaningful when `has_more` is `true`.                          |

The field descriptions should say plainly that `count` is the page length and
`total_count` is the total, so a model reading the schema doesn't mistake one
for the other.

For example, `pidash_list_issues(project_id="NEWFRONT", state="Backlog",
fields="id,sequence_id,name")` answers "how many Backlog issues are there?"
from `total_count` on the first call. Following `next_cursor` until `has_more`
is `false` returns every one of them.

The `pidash` CLI already exposes this surface as `pidash issue list --project
<PROJ> --state … --state-group … --parent … --priority … --label … --fields …
--cursor … --per-page …`. It prints the envelope verbatim, so it is the
reference for what the tool should return.

## Creating work items: descriptions (`pidash_create_issue`, `pidash_update_issue`)

The work-item write endpoints accept a description three ways, in this order
of precedence:

1. `description_markdown`: markdown that the server renders to the editor's
   HTML. Headings, nested and task lists, code blocks and tables keep their
   structure.
2. `description_html`: HTML, stored as given after sanitising.
3. `description`: legacy key, rendered like `description_markdown` when
   neither of the keys above is set.

The tools' `description` parameter is documented as markdown, so the tool must
forward it as `description_markdown`. It must not wrap the text in `<p>…</p>`
and send it as `description_html`. Doing that stores the raw markdown as one
paragraph, so `## Heading` and `- item` show up as literal text in the web UI.

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
  run, because no durable run→issue creation link exists yet. The `issues`
  list is also scoped to projects the caller is a member of — a comment
  the run left on an issue in a project the caller cannot access is
  omitted, matching normal issue visibility.
