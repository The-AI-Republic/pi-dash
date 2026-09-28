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

## Project page tools

The hosted connector exposes two read-only tools for a project's wiki:

| Tool                | Purpose |
| ------------------- | ------- |
| `pidash_list_pages` | Lists visible page metadata. It accepts `workspace_slug`, a project UUID or identifier in `project_id`, and optional `limit` and `include_archived` fields. Page bodies are never returned. |
| `pidash_get_page`   | Retrieves one visible page by `page_id`. It returns `description_markdown` by default; set `include_description_html` to `true` when the source HTML is also needed. |

Both tools require active project membership and follow the `/api/v1/` page
visibility rule: a page is visible when it is public or owned by the caller. A
private page owned by another member is reported the same way as a missing
page. Archived pages are hidden from `pidash_list_pages` unless
`include_archived` is true, while a known archived page remains retrievable.

The markdown body comes from `PageDetailSerializer`, which uses the shared
HTML-to-markdown converter. MCP and the `pidash page get` CLI therefore render
the same page content.

Each list result and detail response includes an absolute `url` for the page in
the web UI:

```
<web-base-url>/<workspace-slug>/projects/<project-uuid>/pages/<page-uuid>
```
