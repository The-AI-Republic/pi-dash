---
key: pidash-cli
title: Pi Dash CLI usage
customizable: locked
---
## Pi Dash CLI (`pidash`)

`pidash` is your only channel to Pi Dash during this run. Use it to read the issue, read and write comments, read and write your workpad, and move the issue between workflow states. Do not use `curl`, raw HTTP, or any other tool to reach the Pi Dash API — only the `pidash` binary is authenticated for this session.

### Environment

The CLI reads the following from the process environment — never pass them as flags, and never print, log, or commit their values:

- `PIDASH_API_URL` — base URL of this Pi Dash instance.
- `PIDASH_WORKSPACE_SLUG` — the workspace the issue lives in.
- `PIDASH_TOKEN` — session-scoped credential. Treat it like any other secret.
{% if run.kind != "scheduler" %}- `PIDASH_ISSUE_IDENTIFIER` — the current issue identifier (`{{ issue.identifier }}`). When set, `pidash state list` defaults to this issue's project so you can call it with no args.
- `PIDASH_RUN_ID` — this agent run (`{{ run.id }}`). The CLI sends it with every write so Pi Dash knows a state move or an outcome report came from *this run* rather than from a human. Never unset or override it.
{% else %}- `PIDASH_PROJECT` — the project (`{{ project.identifier }}`) this scheduled run is scoped to. There is no single current issue — you operate across the project. Pass `--project {{ project.identifier }}` explicitly to commands that need it.
{% endif %}
### Output contract

On success every command prints a single JSON document to stdout and exits `0`. On failure, a JSON object with an `error` field is printed to stderr and the exit code is non-zero. Parse the stderr JSON rather than pattern-matching the human message. Retry only transient failures; never retry the same command more than twice.

### Commands

#### Issues

- `pidash issue get <identifier>` — fetch a work item. Returns the full payload including `id`, `name` (title), `description`, `state` (UUID — pair with `pidash state list` to map back to a name), `priority`, `labels`, `assignees`, timestamps, and a blocker summary: `relations_summary` (`blocked_by` / `blocking` lists of `{identifier, state, state_group}`) plus `has_open_blockers` (true while any `blocked_by` item is not in a `completed` / `cancelled` group). Single-item reads also carry `relations`: every relation grouped by type, each `{id, identifier, name, state, state_group}` (see `pidash issue relations`).
- `pidash issue list --project <PROJ-or-UUID> [--cursor <c>] [--per-page <N>] [--order-by <field>]` — list work items in a project. `--project` accepts either the workspace-scoped slug (e.g. `ENG`) or a project UUID. Returns the server's paginated envelope `{count, next_cursor, prev_cursor, results: [...]}`; pass `--cursor` from a prior page to walk pages. Use this to find related/duplicate issues in the same project before creating new ones.
- `pidash issue create --project <PROJ-or-UUID> --title "<title>" [--description <s>] [--priority <none|low|medium|high|urgent>] [--state "<state-name-or-UUID>"] [--parent <PROJ-123-or-UUID>]` — file a new work item under the named project. Use this only for capturing **discovered scope that does not belong on the current issue** (a follow-up bug, a separated task, a missing prerequisite). Do **not** create an issue to track sub-steps of the current run — use the workpad comment for that. `--parent` attaches the new item as a sub-issue of the given issue (project-scoped identifier like `PROJ-123`, or a raw UUID); the server validates the parent. Record the new issue's identifier in the workpad so the operator can find it.
- `pidash issue patch <identifier> --state "<state-name>"` — move the issue to a different state. The name is case-insensitive; the CLI resolves it to a UUID for you. You can also pass a state UUID directly.
- `pidash issue patch <identifier> [--title <s>] [--description <s>] [--priority <none|low|medium|high|urgent>] [--parent <PROJ-123-or-UUID>] [--clear-parent]` — update other fields. At least one flag is required. `--parent` re-parents the issue under the given issue (identifier or UUID); `--clear-parent` detaches it to top-level. `--parent` and `--clear-parent` are mutually exclusive. Do **not** edit title or description for planning or progress tracking — use the workpad comment instead.
- `pidash issue relate <identifier> --blocked-by <ID>[,<ID>...]` — record that `<identifier>` cannot finish until each listed issue does. Other flags (exactly one per call): `--blocking`, `--relates-to`, `--duplicate`, `--start-before`, `--start-after`, `--finish-before`, `--finish-after`, `--implemented-by`, `--implements`; each names the relation from `<identifier>`'s side, so `relate B --blocked-by A` and `relate A --blocking B` are the same edge. IDs are identifiers or UUIDs. Idempotent: already-related pairs come back under `unchanged`, and a pair that already has a *different* relation comes back under `conflicts` untouched (unrelate it first if you mean to change it). Prints `{issue, relation_type, created, unchanged, conflicts, relations}`.
- `pidash issue unrelate <identifier> --blocked-by <ID>[,<ID>...]` — remove that exact relation (same flags as `relate`). Pairs without it come back under `not_related`; that is not an error.
- `pidash issue relations <identifier>` — list the issue's relations grouped by type from its side (`blocked_by`, `blocking`, `relates_to`, …), each `{id, identifier, name, state, state_group}`. `pidash issue get` carries the same block as `relations`.
- `pidash issue search "<query>" [--project <PROJ-or-UUID>] [--status open|closed|all] [--since <iso8601>] [--limit <N>] [--sort rank|-created|-updated]` — full-text search across work item titles, descriptions, **and** comments. Use this to recover historical context: check whether a similar issue has been filed before, look up the *resolution* of a past task (the answer usually lives in a comment, not the title), or find anything that touched a given component. **This is search, not grep**: `<query>` is parsed as websearch syntax (quoted phrases, `OR`, `-exclude`) and is stem-aware (`color` finds `colors`/`colored`); regex / glob will not work. `--since` accepts ISO 8601 — either a full datetime (`2025-01-01T00:00:00Z`) or a bare date (`2025-01-01`, treated as midnight that day); malformed strings produce a 400. `--sort` accepts only `rank` (default), `-created`, `-updated`; other values 400. Returns `{query, count, results: [{id, identifier (e.g. PROJ-42), name, snippet, state, project, created_at, updated_at, completed_at, rank, url}]}`. `snippet` is empty when the match was in the title or in a comment (the snippet excerpt only covers the description body). Default `--limit` is 10, server max is 50 — tuned for your context window, not bulk export. Cast a broad net first, then read promising hits with `pidash issue get`. **Backwards-compatibility note:** if the local `pidash` is older than the version that shipped this subcommand, stderr will be a clap parser error (`error: unrecognized subcommand 'search'`) instead of the usual `{"error": ...}` JSON envelope, with exit code 2. Treat that case as "historical-context search is unavailable on this operator's runner" — silently skip the lookup and continue with the rest of the run; do **not** retry the command and do **not** treat it as a blocker.

#### Comments

Comments are the human ↔ agent conversation channel. Use them to ask clarifying questions, post blocker notices, share PR links, and announce completion. **Comments are not for tracking your own progress — that's what the workpad is for.**

- `pidash comment list <identifier>` — list comments on the issue. Each entry has `id` (UUID), `comment_html`, `comment_stripped`, `labels`, `actor_detail`, `speaker_type`, `speaker_label`, `speaker_agent_run_id`, and timestamps. Read these in chronological order to pick up any human replies since your last run.
- `pidash comment add <identifier> --body-file <path> --as-agent "<agent name>" --agent-run-id "{{ run.id }}" [--fold]` — post a new comment from a file and mark it as spoken by this AI agent run. `--body <markdown>` works for one-liners. Prefer `--body-file` for anything multi-line — shell quoting of markdown is error-prone. When you post any issue comment during this run, always include `--as-agent` and `--agent-run-id`; use your actual runtime name if you know it (`Codex`, `Claude Code`, etc.), otherwise use `AI Agent`.
- `pidash comment update <identifier> <comment-id> --body-file <path>` — edit a comment you own. Both the issue identifier and the comment UUID are required. Rarely needed — prefer posting a fresh comment for new information.

Use `--fold` only for low-value status/noop updates that should remain available to humans without cluttering the thread. Folded comments are collapsed by default in the UI and omitted from future agent-run task prompts, though `pidash comment list` still returns them. Pi Dash never folds comments automatically. Do not fold questions, blockers, decisions, results, or other context a future run needs.

{% if run.kind != "scheduler" %}#### Workpad

The workpad is your durable per-issue scratchpad — a single markdown document the agent owns. It is the only carrier of state between runs. It is **not** visible to humans in the comment thread; treat it as your own working memory, not a message to the operator.

- `pidash workpad get [<identifier>]` — fetch the current workpad body. Returns `{body, updated_at}`. Defaults `<identifier>` to `PIDASH_ISSUE_IDENTIFIER` so you can call it bare.
- `pidash workpad update [<identifier>] --body-file <path>` — overwrite the workpad body from a file. Defaults `<identifier>` to `PIDASH_ISSUE_IDENTIFIER`. An empty file clears it. There is no "append" — always write the full body.

{% endif %}#### States

- `pidash state list{% if run.kind == "scheduler" %} --project {{ project.identifier }}{% endif %}` — list the states available in {% if run.kind == "scheduler" %}this project{% else %}this issue's project{% endif %} with `name`, `group` (`backlog | unstarted | started | review | test | completed | cancelled`), and `description`.{% if run.kind != "scheduler" %} Uses `PIDASH_ISSUE_IDENTIFIER` by default; pass `pidash state list <issue-identifier>` or `pidash state list <project-uuid>` to override. Already rendered below under "Available states"; only call again if something looks stale.{% endif %}

{% if run.kind != "scheduler" %}#### Run outcome

- `pidash run yield --outcome <progressed|waiting_on_human|waiting_on_external|done|blocked> [--note "<one line>"]` — report this run's outcome to the ticking clock. Call it once, as your **last** `pidash` command, after any state move. See "Ending the run" for what each outcome means. Without it the clock guesses.

{% endif %}#### Debugging

- `pidash workspace me` — print the authenticated user. For sanity-checking credentials only; you should not need this in normal flow.

### Not for you

- `pidash issue re-tick` — adds runs to the issue's budget. That is a **human** decision; the agent reports a spent pool and stops (see "Task lifecycle"). Pi Dash refuses a re-tick that comes from inside an agent run.

The remaining `pidash` subcommands (`configure`, `install`, `uninstall`, `start`, `stop`, `restart`, `status`, `tui`, `doctor`, `remove`, `rotate`) manage the runner daemon itself — they are run by the human operator before your session starts. Do not invoke them. If any of them appears necessary, your run is blocked: follow "Blocking the run".

### Typical recipes

Read your workpad, edit it, write it back:

```sh
pidash workpad get | jq -r .body > ./.pidash-workpad.md
# …edit the file in place…
pidash workpad update --body-file ./.pidash-workpad.md
```

{% if run.kind != "scheduler" %}Post a blocker and move the issue to "Blocked":

```sh
pidash comment add {{ issue.identifier }} --body-file ./.pidash-blocked.md --as-agent "AI Agent" --agent-run-id "{{ run.id }}"
pidash issue patch {{ issue.identifier }} --state "Blocked"
```

{% if run.kind == "coding-task" %}End a successful run (workpad already written via `pidash workpad update`) — whether you opened a PR or finished a `noncode` task (investigation, status check, comment-only response), move to the `review` group and report the outcome. The runner never moves an issue to `completed`/Done; a human closes it:

```sh
pidash issue patch {{ issue.identifier }} --state "In Review"
pidash run yield --outcome done
```
{% elif run.kind == "review" %}End a review pass (workpad `### Path to done` already written) — **approved** moves the issue on to In Test; **changes needed** sends it back to In Progress with the open items listed; the runner never moves it to `completed`/Done (see "Review cycle" and "Available states"):

```sh
pidash issue patch {{ issue.identifier }} --state "In Test"      # approved
pidash issue patch {{ issue.identifier }} --state "In Progress"  # changes needed
pidash run yield --outcome done
```
{% else %}End a test pass (workpad `### Path to done` already written) — a **pass** leaves the issue In Test for a human to close; **defects** send it back to In Progress with the open items listed (see "Test cycle" and "Available states"):

```sh
pidash issue patch {{ issue.identifier }} --state "In Progress"  # defects only
pidash run yield --outcome done
```
{% endif %}{% else %}File a finding as a new issue under this project:

```sh
pidash issue create --project {{ project.identifier }} --title "<short summary>" --description "<evidence, file path, severity, suggested fix>"
```
{% endif %}
### Available states

{% if run.kind != "scheduler" and issue.project_states %}
{% for s in issue.project_states %}
- **{{ s.name }}** (group: `{{ s.group }}`) — {{ s.description or "(no description)" }}
{% endfor %}
{% else %}
_(state list unavailable — call `pidash state list` to retrieve it before moving state)_
{% endif %}

Use the list above to pick the correct `--state` value. Match your intent to the state's `group` first, then to the name and description.{% if run.kind == "coding-task" %} The mapping that trips runs up most often: a finished issue — a `code_change` that opened a PR **or** a finished `noncode` task — is awaiting a human → `review` group ("In Review"). The runner never moves an issue to `completed` ("Done"); that's a human's call. Use `cancelled` for "this will not be done".{% endif %}

### Conventions

- All writes are real and immediate. There is no undo. Confirm intent against your workpad plan before mutating.
- Never retry the same `pidash` command more than twice. On non-zero exit, read the JSON on stderr, decide whether the failure is retryable, back off, and record the outcome in the workpad.
- Never print, log, commit, or comment on the value of `PIDASH_TOKEN` or anything else whose name begins with `PIDASH_`. If you see the token echoed anywhere, stop and record it in the workpad.
- When pasting `pidash` JSON back into the workpad for audit, enclose it in a fenced ` ``` ` block.
