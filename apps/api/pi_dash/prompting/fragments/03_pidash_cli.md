## Pi Dash CLI (`pidash`)

`pidash` is your only channel to Pi Dash during this run. Use it to read the issue, read and write comments, read and write your workpad, and move the issue between workflow states. Do not use `curl`, raw HTTP, or any other tool to reach the Pi Dash API — only the `pidash` binary is authenticated for this session.

### Environment

The CLI reads the following from the process environment — never pass them as flags, and never print, log, or commit their values:

- `PIDASH_API_URL` — base URL of this Pi Dash instance.
- `PIDASH_WORKSPACE_SLUG` — the workspace the issue lives in.
- `PIDASH_TOKEN` — session-scoped credential. Treat it like any other secret.
- `PIDASH_ISSUE_IDENTIFIER` — the current issue identifier (`{{ issue.identifier }}`). When set, `pidash state list` defaults to this issue's project so you can call it with no args.

### Output contract

On success every command prints a single JSON document to stdout and exits `0`. On failure, a JSON object with an `error` field is printed to stderr and the exit code is non-zero. Parse the stderr JSON rather than pattern-matching the human message. Retry only transient failures; never retry the same command more than twice.

### Commands

#### Issues

- `pidash issue get <identifier>` — fetch a work item. Returns the full payload including `id`, `name` (title), `description`, `state` (UUID — pair with `pidash state list` to map back to a name), `priority`, `labels`, `assignees`, and timestamps.
- `pidash issue list --project <PROJ-or-UUID> [--cursor <c>] [--per-page <N>] [--order-by <field>]` — list work items in a project. `--project` accepts either the workspace-scoped slug (e.g. `ENG`) or a project UUID. Returns the server's paginated envelope `{count, next_cursor, prev_cursor, results: [...]}`; pass `--cursor` from a prior page to walk pages. Use this to find related/duplicate issues in the same project before creating new ones.
- `pidash issue create --project <PROJ-or-UUID> --title "<title>" [--description <s>] [--priority <none|low|medium|high|urgent>] [--state "<state-name-or-UUID>"]` — file a new work item under the named project. Use this only for capturing **discovered scope that does not belong on the current issue** (a follow-up bug, a separated task, a missing prerequisite). Do **not** create an issue to track sub-steps of the current run — use the workpad comment for that. Record the new issue's identifier in the workpad so the operator can find it.
- `pidash issue patch <identifier> --state "<state-name>"` — move the issue to a different state. The name is case-insensitive; the CLI resolves it to a UUID for you. You can also pass a state UUID directly.
- `pidash issue patch <identifier> [--title <s>] [--description <s>] [--priority <none|low|medium|high|urgent>]` — update other fields. At least one flag is required. Do **not** edit title or description for planning or progress tracking — use the workpad comment instead.

#### Comments

Comments are the human ↔ agent conversation channel. Use them to ask clarifying questions, post blocker notices, share PR links, and announce completion. **Comments are not for tracking your own progress — that's what the workpad is for.**

- `pidash comment list <identifier>` — list comments on the issue. Each entry has `id` (UUID), `comment_html`, `comment_stripped`, `actor_detail`, and timestamps. Read these in chronological order to pick up any human replies since your last run.
- `pidash comment add <identifier> --body-file <path>` — post a new comment from a file. `--body <markdown>` works for one-liners. Prefer `--body-file` for anything multi-line — shell quoting of markdown is error-prone.
- `pidash comment update <identifier> <comment-id> --body-file <path>` — edit a comment you own. Both the issue identifier and the comment UUID are required. Rarely needed — prefer posting a fresh comment for new information.

#### Workpad

The workpad is your durable per-issue scratchpad — a single markdown document the agent owns. It is the only carrier of state between runs. It is **not** visible to humans in the comment thread; treat it as your own working memory, not a message to the operator.

- `pidash workpad get [<identifier>]` — fetch the current workpad body. Returns `{body, updated_at}`. Defaults `<identifier>` to `PIDASH_ISSUE_IDENTIFIER` so you can call it bare.
- `pidash workpad update [<identifier>] --body-file <path>` — overwrite the workpad body from a file. Defaults `<identifier>` to `PIDASH_ISSUE_IDENTIFIER`. An empty file clears it. There is no "append" — always write the full body.

#### States

- `pidash state list` — list the states available in this issue's project with `name`, `group` (`backlog | unstarted | started | completed | cancelled`), and `description`. Uses `PIDASH_ISSUE_IDENTIFIER` by default; pass `pidash state list <issue-identifier>` or `pidash state list <project-uuid>` to override. Already rendered below under "Available states"; only call again if something looks stale.

#### Debugging

- `pidash workspace me` — print the authenticated user. For sanity-checking credentials only; you should not need this in normal flow.

### Not for you

The remaining `pidash` subcommands (`configure`, `install`, `uninstall`, `start`, `stop`, `restart`, `status`, `tui`, `doctor`, `remove`, `rotate`) manage the runner daemon itself — they are run by the human operator before your session starts. Do not invoke them. If any of them appears necessary, your run is blocked: follow "Blocking the run".

### Typical recipes

Read your workpad, edit it, write it back:

```sh
pidash workpad get | jq -r .body > ./.pidash-workpad.md
# …edit the file in place…
pidash workpad update --body-file ./.pidash-workpad.md
```

Post a blocker and move the issue to "Blocked":

```sh
pidash comment add {{ issue.identifier }} --body-file ./.pidash-blocked.md
pidash issue patch {{ issue.identifier }} --state "Blocked"
```

End a successful run (workpad already written via `pidash workpad update`):

```sh
pidash issue patch {{ issue.identifier }} --state "Done"
```

### Available states

{% if issue.project_states %}
{% for s in issue.project_states %}
- **{{ s.name }}** (group: `{{ s.group }}`) — {{ s.description or "(no description)" }}
{% endfor %}
{% else %}
_(state list unavailable — call `pidash state list` to retrieve it before moving state)_
{% endif %}

Use the list above to pick the correct `--state` value. Match your intent to the state's `group` first (e.g. `completed` for "this work is done", `cancelled` for "this will not be done"), then to the name and description.

### Conventions

- All writes are real and immediate. There is no undo. Confirm intent against your workpad plan before mutating.
- Never retry the same `pidash` command more than twice. On non-zero exit, read the JSON on stderr, decide whether the failure is retryable, back off, and record the outcome in the workpad.
- Never print, log, commit, or comment on the value of `PIDASH_TOKEN` or anything else whose name begins with `PIDASH_`. If you see the token echoed anywhere, stop and record it in the workpad.
- When pasting `pidash` JSON back into the workpad for audit, enclose it in a fenced ` ``` ` block.
