---
key: pidash-cli
title: Pi Dash CLI usage
customizable: locked
---
## Pi Dash CLI (`pidash`)

`pidash` is your only channel to Pi Dash. It is already authenticated **as this runner's owner**; your writes carry their identity{% if run.kind != "chat" %} plus this run's id{% endif %}. Do not use `curl` or raw HTTP. Never print, log or record any `PIDASH_*` value or anything from the pidash config.

{% if run.kind != "scheduler" and run.kind != "chat" %}
`PIDASH_ISSUE_IDENTIFIER`, `PIDASH_PROJECT` and `PIDASH_RUN_ID` are set, so commands default to this issue and run. (If a bare command says the issue identifier is missing, pass `{{ issue.identifier }}`.)
{% elif run.kind == "chat" %}
There is no current issue or run: pass the project and issue identifiers explicitly (a project slug such as `ENG`, an issue identifier such as `ENG-42`).
{% else %}
`PIDASH_PROJECT` (`{{ project.identifier }}`) and `PIDASH_RUN_ID` are set. There is no current issue: pass identifiers explicitly.
{% endif %}

Every command prints one JSON document on stdout and exits `0`. On failure it prints `{"error": …}` on stderr and exits non-zero: read it, retry only transient failures, and never retry the same command more than twice.

### What you already have

{% if run.kind == "scheduler" %}
The project and its states shown in this prompt were read at `{{ run.rendered_at }}`.
{% elif run.kind == "chat" %}
Nothing is pre-loaded: fetch what you need with `pidash issue get`, `pidash comment list` and `pidash state list`, and use `pidash docs <topic>` for command detail.
{% else %}
The issue, its comments, the available states and the workpad shown in this prompt were read at `{{ run.rendered_at }}`. Don't fetch them again. Two exceptions: check for newer comments with `pidash comment list --since {{ run.rendered_at }}` when "How this run works" tells you to, or when you have worked long enough that a person may have replied; and re-read after a write is rejected as stale. (If `--since` is not a known flag, use `pidash comment list`.)
{% endif %}

### Core commands

{% if run.kind != "scheduler" and run.kind != "chat" %}
- `pidash comment add --body-file .pidash/<name>.md [--fold]` — post a comment during the run, as this agent run. Your closing comment goes through `pidash run finish`, not this. Use `--fold` only for low-value status notes.
- `pidash workpad update` / `pidash workpad get --to-file` — write and re-read `.pidash/workpad.md`. See "{% if run.kind == "coding-task" %}Step 1 — {% endif %}Workpad". (If the workpad file is missing, run `pidash workpad get --to-file` first; if that flag is unknown, `pidash workpad get | jq -r .body > .pidash/workpad.md`.)
- `pidash issue get` / `pidash comment list [--since <time>]` — refresh only (see above).
- `pidash run finish` — ends the run; your **last** `pidash` call. The syntax, the results and the state to pass are in "Lifecycle".
{% elif run.kind == "chat" %}
- `pidash issue list --project <PROJ> [--state <name>] [--state-group <group>] [--fields <fields>]` — find work items in a project (for example the open `Backlog` items).
- `pidash issue get <identifier>` — read one work item in full.
- `pidash issue create --project <PROJ> --title "<title>"` — file a new work item.
- `pidash comment list <issue>` / `pidash comment add <issue> --body-file <path>` — read and post comments on an issue.
- `pidash state list [--project <PROJ>]` — workflow states for moving an issue.
{% else %}
- `pidash issue create --project {{ project.identifier }} --title "<summary>" --description-file .pidash/finding.md` — file a finding.
- `pidash comment add <issue> --body-file .pidash/<name>.md [--fold]` — post a comment as this agent run.
- `pidash run yield --outcome <progressed|waiting_on_human|waiting_on_external|done|blocked>` — optional; if sent, your last `pidash` call. It prints `{"ok": true, "run_kind": "scheduler", …}` and exits `0`; nothing it reports disables the schedule. See "Ending the scheduled run".
{% endif %}

Keep every scratch file under `.pidash/`; that directory stays out of version control.

### Other commands

Run `pidash docs <topic>` (or `pidash <command> --help`) before first use. If `pidash docs` is not a known command, use `--help`.

- `issue`: create, patch (title, labels, another issue's state), list/filter, search history, relate/unrelate/wait, relations, run-ai
- `page`: project wiki pages (read before re-deriving a convention)
- `label`: list, create, update, delete; attach via `issue patch --add-label`
- `state`: list workflow states{% if run.kind != "chat" %} (already shown below){% else %} (via `pidash state list`){% endif %}
- `scheduler`: view or change scheduled automation (admin; only when asked)
- `project`: list runners
- `context`: the `context.md` files that list the projects on this machine

You don't need to load a `pidash` / `pi-dash` skill: this section and `pidash docs` are the reference. Commands that manage the runner itself are refused inside a run;{% if run.kind != "scheduler" and run.kind != "chat" %} if one seems necessary, the run is blocked (see "Blocking the run").{% else %}{% if run.kind == "chat" %} if one seems necessary, say so in chat instead of calling it.{% else %} if one seems necessary, file a Pi Dash issue describing the blocker instead (see "Ending the scheduled run").{% endif %}{% endif %} `pidash issue re-tick` is a human decision; never call it.

### Available states

{% if run.kind != "scheduler" and run.kind != "chat" and issue.project_states %}
{% for s in issue.project_states %}
- **{{ s.name }}** (group: `{{ s.group }}`) — {{ s.description or "(no description)" }}
{% endfor %}
{% else %}
_(state list unavailable — call `pidash state list` to retrieve it before moving state)_
{% endif %}

Use the list above to pick the correct `--state` value. Match your intent to the state's `group` first, then to the name and description.{% if run.kind != "scheduler" and run.kind != "chat" %} Where a finished stage goes is set by "Lifecycle"; never move an issue to Done or Cancelled yourself.{% endif %}

### Conventions

- All writes are real and immediate; there is no undo. Check intent against the issue{% if run.kind != "scheduler" and run.kind != "chat" %}, its comments and the workpad{% else %} and its comments{% endif %} before mutating.
- When pasting `pidash` JSON into a comment, fence it in three backticks.
