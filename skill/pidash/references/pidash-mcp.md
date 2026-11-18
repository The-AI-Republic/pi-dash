# Pi Dash MCP Connector

Reference for working with Pi Dash through its MCP connector instead of the `pidash` CLI. The connector exposes the same REST surface as the CLI, as tools named `pidash_*`.

The tool schemas your client shows you are the source of truth for argument names and allowed values. When this file and a tool's schema disagree, the schema wins.

## Contents

- [When to use the connector](#when-to-use-the-connector)
- [Finding the tools](#finding-the-tools)
- [Workspace and project](#workspace-and-project)
- [Task to tool](#task-to-tool)
- [How the connector differs from the CLI](#how-the-connector-differs-from-the-cli)
- [What the connector cannot do](#what-the-connector-cannot-do)
- [Comments](#comments)
- [Pull requests](#pull-requests)
- [Relations](#relations)
- [Schedulers](#schedulers)
- [Errors](#errors)

## When to use the connector

- **The `pidash` CLI is installed and authenticated:** use the CLI. It covers more (labels, pages, list filters, merge requests, agent runs) and reads `.pidash/context.md` conventions directly.
- **No CLI, but `pidash_*` tools are available** (a chat client, a sandbox without the binary): use the connector.
- **Both are available:** use the CLI, and fall back to a connector tool only for something the CLI on this machine lacks.

Do not mix surfaces within one operation — for example, do not create an issue through the connector and then look for it by a CLI default project. The CLI and the connector can be signed in as different users or to different workspaces.

## Finding the tools

Clients prefix MCP tool names with the server's name, so `pidash_create_issue` may appear as `mcp__<server>__pidash_create_issue` or similar. This file uses the bare names. Match on the `pidash_` part.

Verify access once per session with `pidash_workspace_me`. It returns the authenticated user. If it fails, the connector is not authorized — ask the user to reconnect it in their client's connector settings.

## Workspace and project

Every tool except `pidash_workspace_me` requires `workspace_slug`, and no tool lists workspaces. Get the slug from, in order:

1. `.pidash/context.md` in the working directory, if one exists — it records the workspace slug.
2. A Pi Dash URL the user gave you: `<base-url>/<workspace-slug>/browse/PROJ-123`.
3. The user. Ask once and reuse the answer for the session.

Do not guess a slug from the company or repository name.

For the project, the same rule as the CLI applies: `.pidash/context.md` is authoritative when it exists. Otherwise call `pidash_list_projects`, match on name, description, and repository, and ask the user to confirm before the first write. The connector has no equivalent of `pidash context init` and no default project — pass `project_id` explicitly on every create.

`pidash_list_projects` returns active projects only. Use `pidash_list_all_projects` only when the user asks about archived projects. Reads and writes on an archived project's issues may be refused with a `project_archived` error.

## Task to tool

| Goal                    | Tool                                                                                                      | CLI equivalent               |
| ----------------------- | --------------------------------------------------------------------------------------------------------- | ---------------------------- |
| Check access            | `pidash_workspace_me`                                                                                     | `pidash workspace me`        |
| List projects           | `pidash_list_projects`                                                                                    | `pidash project list`        |
| Read one project        | `pidash_get_project`                                                                                      | —                            |
| Read an issue           | `pidash_get_issue`                                                                                        | `pidash issue get`           |
| Read its conversation   | `pidash_list_comments`                                                                                    | `pidash comment list`        |
| Find issues by content  | `pidash_search_issues`                                                                                    | `pidash issue search`        |
| List issues             | `pidash_list_issues`                                                                                      | `pidash issue list`          |
| Create an issue         | `pidash_create_issue`                                                                                     | `pidash issue create`        |
| Change state or fields  | `pidash_update_issue`                                                                                     | `pidash issue patch`         |
| Move to another project | `pidash_move_issue`                                                                                       | `pidash issue move`          |
| Valid states            | `pidash_list_states`                                                                                      | `pidash state list`          |
| Comment                 | `pidash_comment_issue`                                                                                    | `pidash comment add`         |
| Edit a comment          | `pidash_update_comment`                                                                                   | `pidash comment update`      |
| Record a relation       | `pidash_relate_issues`                                                                                    | `pidash issue relate`        |
| Remove a relation       | `pidash_unrelate_issues`                                                                                  | `pidash issue unrelate`      |
| List relations          | `pidash_list_issue_relations`                                                                             | `pidash issue relations`     |
| Link a GitHub PR        | `pidash_attach_github_pull_request`                                                                       | `pidash issue attach-review` |
| List linked PRs         | `pidash_list_github_pull_requests`                                                                        | —                            |
| Schedulers              | `pidash_list_schedulers`, `pidash_get_scheduler`, `pidash_list_scheduler_runs`, and the write tools below | `pidash scheduler ...`       |

Issues are addressed by `issue_id`, which accepts a UUID or an identifier like `PROJ-123`. Projects are addressed by UUID or identifier.

## How the connector differs from the CLI

These are the places where carrying a CLI habit over gives a wrong result:

- **Listing has no filters and no cursor.** `pidash_list_issues` takes only `project_id`, `limit` (1–100), and `order_by`. It cannot filter by state, label, parent, or priority, and it cannot page past `limit`. To find issues by criteria, use `pidash_search_issues` (which has `status`, `since`, and `sort`, and a `limit` of at most 50), or list and filter the results yourself — and say so if the project may hold more than you fetched.
- **`pidash_list_issues` without `project_id` lists across the whole workspace.** Pass `project_id` unless that is what you want.
- **`pidash_list_states` accepts a project identifier.** The CLI's `state list` takes only an issue identifier or project UUID; the tool's `project_or_issue_id` takes a project UUID, a project identifier, or an issue identifier.
- **Text is passed inline.** There are no `--description-file` / `--body-file` equivalents; put the text in `description` or `body`.
- **Comment bodies are plain text or HTML, not markdown.** A body that starts with an HTML tag is stored as HTML; anything else is stored as escaped plain text in one paragraph. For structure — lists, code, links — send HTML. Issue `description` accepts markdown; `description_html` accepts HTML.
- **Comments default to agent-spoken, labelled `MCP`.** See [Comments](#comments).
- **Search can skip comment text.** `pidash_search_issues` has `include_comments` (default true); set it false to match titles and descriptions only.
- **Every issue result carries `url`** — an absolute link to the issue in the web UI — when the deployment has a web URL configured. Give it to the user after creating or finding an issue. Treat it as optional: it is omitted, not broken, when unconfigured.
- **Stale-write guards.** Scheduler tools that change or remove an existing install or definition (update, run-now, uninstall, delete) take `expected_updated_at`; pass the `updated_at` you last read so a concurrent edit is refused instead of overwritten.

## What the connector cannot do

No tool exists for these. Do not approximate them with another tool — tell the user it needs the CLI or the web app:

- Labels: listing, creating, attaching, or detaching.
- Pages (the project wiki).
- GitLab merge requests — `pidash_attach_github_pull_request` accepts `github.com` pull request URLs only.
- Starting an agent run (`pidash issue run-ai`), re-granting a tick budget, or reporting a run outcome.
- Image attachments on comments, and folding a comment.
- Initializing `.pidash/context.md`.

## Comments

`pidash_comment_issue` marks the comment as agent-spoken by default, with the speaker label `MCP`. That label tells a reader nothing, so:

- **Your own message:** set `speaker_label` to your runtime name — `Claude`, `Claude Code`, `Codex`, or `AI Agent` when unknown. Leave `speaker_type` at its default.
- **The user's own words, posted on their behalf:** set `speaker_type` to `human` so it is not recorded as an agent message.
- **Inside a Pi Dash agent run:** also pass `speaker_agent_run_id` with the run's UUID.
- **Visibility:** `access` defaults to `INTERNAL`. Set `EXTERNAL` only when the user asks for an externally visible comment.

`pidash_update_comment` replaces the whole body of a comment. Edit only comments you posted in this session unless the user asks otherwise.

## Pull requests

Same two steps as the CLI, in this order:

1. Comment the pull request link on the issue with `pidash_comment_issue`, so the human sees it in the conversation.
2. Call `pidash_attach_github_pull_request` with the issue and the URL, so Pi Dash tracks the pull request's status.

Neither replaces the other. Attaching is idempotent; a pull request attaches to exactly one issue.

## Relations

`pidash_relate_issues` takes one `relation_type` and a list of `related_issue_ids` per call. `relation_type` is one of `blocked_by`, `blocking`, `relates_to`, `duplicate`, `start_before`, `start_after`, `finish_before`, `finish_after`, `implemented_by`, `implements`, read from the side of `issue_id`. It is idempotent, like the CLI. `pidash_unrelate_issues` removes only that exact relation.

## Schedulers

Read tools — safe:

- `pidash_list_schedulers` (pass `project` for the schedulers installed on one project; omit it for every definition in the workspace)
- `pidash_get_scheduler`
- `pidash_list_scheduler_runs`

Write tools — do these only when the user asked for that specific change, and confirm first:

- On one project, project admins: `pidash_install_scheduler`, `pidash_update_scheduler_binding`, `pidash_run_scheduler_now`, `pidash_uninstall_scheduler`.
- On a shared definition, workspace admins: `pidash_create_scheduler`, `pidash_update_scheduler`, `pidash_delete_scheduler`. A definition change reaches every project that installs it, and deleting one removes every install.

## Errors

A failed tool call returns an error result with a message, not an exit code. `workspace not found or not accessible` usually means a wrong `workspace_slug`; re-check it before anything else. A `project_archived` error means the issue's project is archived — stop and tell the user rather than retrying. Do not retry a refused write in a loop.

`pidash_get_workpad` and `pidash_update_workpad` exist for agents that Pi Dash itself dispatched; `pidash_update_workpad` overwrites the whole workpad. In an ordinary session, leave them alone.
