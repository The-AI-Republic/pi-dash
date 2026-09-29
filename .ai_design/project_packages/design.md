# Project Packages — Installable, Shareable Pi Dash Projects

> Directory: `.ai_design/project_packages/`
>
> **Status:** proposal, for review. Every open decision in §11 carries a
> proposed default; confirm or overturn them in PR review and they become
> the pinned v1 contract. **No product code, migrations or tests change in
> this PR** — the diff is this file.
>
> **Scope:** make a Pi Dash _project_ — its goal, its agent rulebook, its
> workflow, its agent settings, its wiki and its seed backlog — a
> versioned artifact that can be exported to a folder of markdown, lived
> in a plain git repo, reviewed as a diff, and installed into someone
> else's workspace in one step. This is Pi Dash's first community
> feature.
>
> **What this changes about today's code**
>
> Nothing, yet. Today a project is created empty by
> `pi_dash/app/views/project/base.py:258` (or `api/views/project.py` for
> the token API), which `bulk_create`s the eight `DEFAULT_STATES` from
> `pi_dash/db/models/state.py:26` and makes the creator a project Admin.
> Everything that makes a project _good for agents_ — the rulebook in
> `Project.description`, the ticking cadence and budget, the wiki pages,
> the seed epics and their `blocked_by` edges — is then assembled by hand
> and cannot leave the workspace. `ExporterHistory`
> (`pi_dash/db/models/exporter.py:24`) exports _issues_ to csv/json/xlsx;
> it does not export a project. `Importer`
> (`pi_dash/db/models/importer.py:13`) imports from GitHub and Jira; it
> does not import a Pi Dash project. `pidash project`
> (`runner/src/cli/project.rs:20`) has exactly one subcommand today,
> `List`.
>
> **Coverage map** — the issue asked nine questions; they are answered in
> §3 (what is in a package), §4 (format), §5 (install), §6 (share /
> publish), §7 (trust and safety), §8 (data model and API), §9 (worked
> example: PIDASHCONV), §10 (phasing and follow-up issues), §11 (open
> decisions).

---

## 1. Problem

PIDASHCONV — the Django→Rust port project — is the existence proof that a
Pi Dash project can be _tuned_. Its `Project.description` is a ~20-rule
agent rulebook covering where code may live, what "done" means, how
contract tests are written, when an agent may merge its own PR, how to
handle blockers, and which known platform bugs to work around. Two wiki
pages ("Porting guide", "Dead Python Code") carry the rest. The result is
251 issues driven to completion with, by design, no human step.

That rulebook took real effort and it is stuck. It lives in one row of
one `projects` table in one workspace. There is no way to:

- hand it to someone else starting a similar port,
- fork it, improve the "Semantic traps" rules, and offer the improvement
  back,
- review a change to it as a diff — the description is edited in a rich
  text field with no history a reviewer can read,
- start a _new_ project from it without copy-pasting prose between
  browser tabs and then hand-rebuilding the states, labels, pages and
  seed epics.

Meanwhile the thing a newcomer to Pi Dash most needs is exactly this:
not an empty project with eight default states, but a project someone
already got working with agents.

## 2. Goal

A **project package** is a folder (equivalently, a tarball, or a git
repo) that describes a project well enough to recreate it, and nothing
else. Concretely:

- `pidash project export PIDASHCONV --out ./pkg` writes that folder.
- The folder is markdown plus one manifest. It is committed to a git
  repo, reviewed as a diff, and tagged.
- `pidash project install git+https://github.com/me/my-package@v1.2.0`
  in another workspace creates the project, asks for the handful of
  things that cannot travel (repo URL, base branch, which executor), and
  shows exactly what it is about to create before it creates it.
- The installed project **starts with ticking off** and the rulebook
  presented for review, because a package is prompt text that agents
  will execute on the installer's machine.

Non-goals for v1: a hosted gallery (§6.4 and §10 put it in v2), upgrading
an existing installed project in place (§5.7), and enforcing — as opposed
to _disclosing_ — the capabilities a package declares (§7.3).

---

## 3. What is in a project package

### 3.1 Method: allowlist, never a generic walk

The exporter **enumerates** what it includes, field by field. It never
walks the project's related objects generically. This is the single most
important rule in this design: it means a model added to Pi Dash next
month is excluded by default, and including it is a deliberate, reviewed
change to the exporter. A denylist would leak the first time someone adds
a table.

### 3.2 Inventory and disposition

Grounded in the models as they exist at this commit.

**Include — travels as package content**

| Piece                    | Where it lives today                                                                                                                                                                                                                           | Notes                                                                                                                                                                          |
| ------------------------ | ---------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ |
| Project name             | `Project.name` (`db/models/project.py:74`)                                                                                                                                                                                                     | Unique per workspace (`project_unique_name_workspace_when_deleted_at_null`); installable name is a conflict point, see §5.5                                                    |
| Agent rulebook           | `Project.description`                                                                                                                                                                                                                          | The payload. Exported as `rulebook.md`. `description_html` / `description_text` are the editor's rendering of the same content and are **regenerated** on install, not carried |
| Project identifier       | `Project.identifier`                                                                                                                                                                                                                           | Carried as a _suggestion_; unique per workspace, so overridable at install                                                                                                     |
| Emoji / logo             | `Project.emoji`, `logo_props`                                                                                                                                                                                                                  | Cosmetic; cheap to carry                                                                                                                                                       |
| Feature toggles          | `module_view`, `cycle_view`, `issue_views_view`, `page_view`, `intake_view`, `is_time_tracking_enabled`, `is_issue_type_enabled`, `guest_view_all_features`, `members_can_edit_states`                                                         | Part of "the shape of this project"                                                                                                                                            |
| Workflow states          | `State` (`db/models/state.py:93`) — name, color, sequence, `group`, `default`                                                                                                                                                                  | Replaces the `DEFAULT_STATES` seed, see §5.5                                                                                                                                   |
| Labels                   | `Label` (`db/models/label.py:11`) — name, color, description, parent                                                                                                                                                                           | Project-scoped: `unique_project_name_when_not_deleted`                                                                                                                         |
| Agent ticking settings   | `agent_ticking_enabled`, `agent_default_interval_seconds`, `agent_default_max_ticks`, `agent_review_default_interval_seconds`, `agent_test_default_interval_seconds` (`db/models/project.py`, and `.ai_design/ticking_relevance/design.md` §5) | Carried as **proposed** values; `agent_ticking_enabled` is forced off at install regardless (§5.6)                                                                             |
| Archive / close policy   | `archive_in`, `close_in`                                                                                                                                                                                                                       |                                                                                                                                                                                |
| Work item types          | `IssueType` + `ProjectIssueType` (`db/models/issue_type.py`)                                                                                                                                                                                   | `IssueType` is _workspace_-scoped, so install reuses an existing type of the same name rather than duplicating it (§5.5)                                                       |
| Wiki pages               | `Page` + `ProjectPage` (`db/models/page.py:23`, `:135`)                                                                                                                                                                                        | Exported as markdown; `description_binary` (the Yjs collaborative document) is **not** carried — install creates fresh pages from markdown                                     |
| Seed issues and epics    | `Issue`, plus `IssueRelation` (`db/models/issue.py:396`) restricted to `blocked_by` / `blocking` and parent links                                                                                                                              | Name, description, priority, state (by name), labels (by name), type (by name), local key for relations                                                                        |
| Prompt section overrides | `PromptSectionOverride` (`prompting/models.py:65`)                                                                                                                                                                                             | **Blocked today** — the model is workspace/user-scoped with no project column; see §3.4                                                                                        |
| Work type                | Not implemented; `prompting/recipes.py:136` has `WORK_KIND_CODING` and `kind_for(template_name, work_kind)` as the seam PDASHOSS01-234 will fill                                                                                               | Carried as an optional forward-compatible string, see §3.4                                                                                                                     |
| Required capabilities    | §3.3                                                                                                                                                                                                                                           | New concept                                                                                                                                                                    |

**Include as a parameter — asked at install time**

| Parameter                 | Field                                                        | Why it cannot travel                                                                                                                                      |
| ------------------------- | ------------------------------------------------------------ | --------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `repo_url`                | `Project.repo_url`                                           | Names the publisher's repository, not the installer's                                                                                                     |
| `base_branch`             | `Project.base_branch`                                        | Same; also validated by a regex on the model                                                                                                              |
| Executor                  | `Project.default_agent_executor` (`core/agent_execution.py`) | Names _your_ infrastructure: `local_runner`, `cloud_agent` or `managed_runner`. A package must never pick this                                            |
| Declared env vars         | no model today                                               | The manifest declares **names and descriptions only**; the installer supplies values out of band, in the runner's own environment. Pi Dash stores neither |
| Project name / identifier | `Project.name` / `identifier`                                | Only when the suggested value collides (§5.5)                                                                                                             |
| Rulebook variables        | inlined into `rulebook.md`                                   | e.g. PIDASHCONV's `rust-api/` code root; substituted at install (§4.5)                                                                                    |

**Exclude — never leaves the workspace**

`ProjectMember`, `ProjectMemberInvite`, `ProjectUserProperty`,
`default_assignee`, `project_lead` (people); `Runner` and `DevMachine`
(`runner/models.py:382`) and anything else naming a machine; every
credential, token and env _value_; `AgentRun` and its `done_payload`,
`prompt_manifest`, `agent_metadata`, transcripts and logs; `IssueComment`
and the agent workpad; `IssueAgentTicker` runtime state; `UserFavorite`,
`RecentVisit`, `Sticky`, `View`, `Cycle`, `Module`, `Estimate`,
`DeployBoard`, `Intake`, analytics, `WorkspaceIntegration` and any GitHub
sync state; `Importer` / `ExporterHistory` rows; all `external_id` /
`external_source` values (they point at the publisher's Jira/GitHub);
every database UUID.

The last item is a rule, not an omission: **a package contains no
Pi Dash UUIDs**. Everything cross-references by a stable human key — a
state name, a label name, a page slug, an issue's local key inside the
package. This is what lets the same package install into any workspace
and what makes the git diff of a package readable.

### 3.3 Required capabilities

A package declares what the project expects of whatever agent runs it:

```
requires:
  capabilities: [repo.write, repo.admin_merge, shell, network]
  executor_kinds: [local_runner, managed_runner]
  env: [DATABASE_URL, GH_TOKEN]        # names only, never values
  pidash_version: ">=0.24"
```

There is already a capability channel to hang this on: `Runner.capabilities`
(`runner/models.py`, a JSON list reported at enrollment) and
`AgentRun.required_capabilities` (`runner/models.py:991`, a JSON list used
when matching a run to a runner). v1 **declares** capabilities for
informed consent and displays them at install; it does not yet feed them
into the matcher. §7.3 is explicit about why that distinction matters and
§10 files it as follow-up work.

`executor_kinds` is a real constraint, not decoration: a project whose
rulebook says "run `psql` against your scratch database" cannot execute
on `cloud_agent`. Install warns when the workspace's available executors
do not intersect the declared set.

### 3.4 Two pieces that today's code cannot yet install

Both are carried in the format from day one and both **fail loud** rather
than silently dropping (§11-D4, §11-D5):

1. **Prompt section overrides.** `PromptSectionOverride`
   (`prompting/models.py:65`) is scoped `(workspace, user, section_key)`
   with no project column, and resolution is user → workspace → registry
   default (`prompting/composer.resolve_section`). A project-level layer
   is designed-but-deferred in `.ai_design/prompt_section_system/design.md`
   §9.4 ("`resolve_section()` accepts `project` from day one and ignores
   it"). Installing a package's overrides at _workspace_ scope would
   silently re-prompt every other project in the workspace — unacceptable.
   So v1 parses them, shows them in the plan, and **refuses the install**
   with a pointer to the follow-up issue unless `--skip-prompt-overrides`
   is passed.
2. **Work types.** PDASHOSS01-234 is designed, not built: there is no
   `prompting/work_types/` folder, and `prompting/recipes.py` still calls
   the execute recipe `coding-task`. The manifest's `work_type` key is
   accepted and stored; if the running instance has no work-type
   registry, install warns once and proceeds (the project still works, it
   just gets today's coding-flavoured stage prompts).

---

## 4. Package format

### 4.1 Layout

```
my-project-package/
  pidash-project.json        # the manifest — the only index
  rulebook.md                # → Project.description
  README.md                  # for humans browsing the git repo (not installed)
  LICENSE
  pages/
    porting-guide.md
    dead-python-code.md
  issues/
    001-rulebook.md
    002-epic-domain-a.md
  prompts/
    analyze-and-scope.md     # section_key → body, see §3.4
```

Every markdown file has YAML-ish front matter _only_ where the object
needs structured fields (an issue's priority, a page's title); the
manifest is the index and the authority on which files exist. Prose lives
in markdown because that is where a reviewer's attention should go: a
pull request against a package should show a rulebook rule changing, in
plain English, on its own line.

### 4.2 Manifest

```json
{
  "schema_version": 1,
  "package": {
    "name": "django-to-rust-port",
    "owner": "airepublic",
    "version": "1.2.0",
    "description": "Port a Django backend to Rust, agent-driven end to end.",
    "license": "AGPL-3.0-only",
    "authors": ["AI Republic <oss@airepublic.com>"],
    "homepage": "https://github.com/The-AI-Republic/pidash-packages",
    "tags": ["porting", "rust", "django", "autonomous"]
  },
  "requires": { "...": "see §3.3" },
  "parameters": [
    {
      "name": "repo_url",
      "title": "Repository URL",
      "type": "url",
      "required": true,
      "binds": "project.repo_url",
      "help": "The repository the agents will work in."
    }
  ],
  "project": {
    "name": "Pi Dash Conversion",
    "identifier": "PIDASHCONV",
    "rulebook": "rulebook.md",
    "agent": {
      "ticking_enabled": true,
      "default_interval_seconds": 10800,
      "default_max_ticks": 10,
      "review_default_interval_seconds": 10800,
      "test_default_interval_seconds": 10800
    },
    "features": { "page_view": true, "cycle_view": false }
  },
  "states": [{ "name": "Backlog", "group": "backlog", "color": "#60646C", "default": true }],
  "labels": [{ "name": "porting", "color": "#5B5BD6" }],
  "issue_types": [{ "name": "Port Task", "is_epic": false }],
  "pages": [{ "title": "Porting guide", "file": "pages/porting-guide.md" }],
  "issues": [
    {
      "key": "rulebook",
      "file": "issues/001-rulebook.md",
      "state": "Backlog",
      "priority": "high",
      "labels": ["porting"],
      "blocked_by": []
    }
  ]
}
```

**JSON, not YAML** — and this is a dependency decision, not a taste one.
`apps/api/requirements/base.txt` carries no PyYAML and
`runner/Cargo.toml` carries no `serde_yaml`; both sides already have a
JSON parser (`json` in the stdlib, `serde_json = "1"` at
`runner/Cargo.toml:25`). Adding a YAML parser to two languages to make
one index file prettier is a bad trade, especially when the prose — the
part humans actually review — is in markdown either way. The manifest
ships with a JSON Schema at `pidash-project.schema.json` so editors
autocomplete it and CI can validate a package repo. §11-D1 records the
alternative.

### 4.3 Identity and versioning

`owner/name@version`, e.g. `airepublic/django-to-rust-port@1.2.0`.

- `owner` is a registry namespace, claimed by an account on whatever
  registry serves it. It is **absent** for file and git installs — those
  are identified by their source URL plus the manifest digest, and
  `owner` in the manifest is advisory until a registry vouches for it.
- `name` is `[a-z0-9][a-z0-9-]*`, unique within an owner.
- `version` is semver. For a _package_, the semantics are:
  **major** = a rulebook change that changes what agents do (rules
  removed, capabilities added, workflow states renamed); **minor** = new
  rules, pages or seed issues that do not invalidate existing behaviour;
  **patch** = wording, typos, links.
- `schema_version` is a separate integer describing the _format_, and
  moves only when the format does.

### 4.4 Staying stable while the implementation moves

PIDASHCONV's own goal is porting `apps/api` to Rust, so this format will
have two implementations serving it. Three rules keep them honest:

1. **`schema_version` is additive within a major.** A reader on
   `schema_version: 1` must ignore unknown keys under `package`,
   `project`, `states`, `labels`, `pages` and `issues` with a warning —
   forward compatibility for descriptive data.
2. **Security-relevant fields fail closed.** An unknown key anywhere
   under `requires` — a capability name the reader does not recognise,
   an unknown permission — is a hard error, never a warning. A reader
   that silently ignores a capability it does not understand is a reader
   that installs a package with more power than it displayed.
3. **A golden conformance suite.** `.ai_design/project_packages/fixtures/`
   (added by the implementation issue, not this doc) holds packages plus
   their expected install _plans_ as canonical JSON. Both the Python and
   the Rust implementation must reproduce the plan byte for byte. This is
   the same discipline PIDASHCONV already applies to the API port
   ("same JSON byte for byte") and it is what makes "the Rust port must
   serve the same format" a testable claim rather than an intention.

The format deliberately contains no DB identifiers, no Django model
names, and no API shapes, so it is not coupled to either implementation.

### 4.5 Parameter substitution

A rulebook needs to say "code only under `rust-api/`" with the path
filled in at install time. Substitution is `${param_name}`, applied
**once, at install time**, to `rulebook.md`, page bodies and issue
bodies; the result is stored as literal text in `Project.description`
and friends.

It is deliberately _not_ Jinja and deliberately _not_ deferred to run
time. `.ai_design/prompt_section_system/design.md` §5.1/§5.4 settled the
matching question for scheduler content: user-supplied text is injected
as a context _variable_, never parsed as a template, because a template
engine reachable from user content is an execution surface. A package is
user content from a stranger; the same rule applies with more force. An
unresolved `${...}` at install time is an error, and a literal `$` is
written `$$`.

---

## 5. Install flow

### 5.1 Shape: resolve → plan → apply

Three phases, and the middle one is the product.

```
resolve   source → a verified local package directory + digest
plan      package + parameters + target workspace → an InstallPlan (JSON)
apply     InstallPlan → one DB transaction
```

`plan` reads the target workspace but writes nothing. `--dry-run` is
simply "stop after plan and print it". The same plan object is what the
web UI renders as a confirmation screen and what MCP can return (§8.4).
Because `apply` consumes a plan rather than re-deriving one, what the
installer approved is exactly what runs.

### 5.2 Sources

| Source          | Syntax                                                | Notes                                                                                         |
| --------------- | ----------------------------------------------------- | --------------------------------------------------------------------------------------------- |
| Local directory | `./my-package`                                        |                                                                                               |
| Local archive   | `./my-package.tgz`                                    | `tar.gz`, extracted to a temp dir with path traversal and size limits enforced                |
| Git             | `git+https://host/org/repo@<ref>`                     | `<ref>` is a tag, branch or SHA; a tag is resolved to a SHA and the SHA is what gets recorded |
| Registry        | `owner/name@version`, or bare `owner/name` for latest | §6.3                                                                                          |

`http(s)://…/package.tgz` is deliberately **not** a v1 source: a bare URL
is the one form with no provenance story at all. Git at least pins a
commit.

### 5.3 Parameters

Declared in the manifest (§4.2), each with `name`, `title`, `type`
(`string` | `url` | `branch` | `enum` | `bool`), `required`, `default`,
`help`, and an optional `binds` naming the project field it fills.

- **CLI, interactive:** prompts for each in order; `--param k=v` presets
  any of them; `--yes` requires that every required parameter is preset.
- **Web:** a form generated from the same declaration, on the same screen
  as the plan preview.
- **MCP:** no prompting channel exists, so every parameter must be
  supplied in the call. (And v1 does not expose install over MCP at all —
  §7.6.)

Values are used for substitution (§4.5) and to fill bound project fields.
A parameter of type `secret` is not supported: Pi Dash must not become a
place secrets are typed. Env vars are declared by name only (§3.2) and
supplied to the runner out of band.

### 5.4 The plan

`pidash project install ./pkg --dry-run` prints, and the web shows the
same content as a screen:

```
Package  airepublic/django-to-rust-port@1.2.0
Source   git+https://github.com/The-AI-Republic/pidash-packages@a1b2c3d
Digest   sha256:9f2c…  (unsigned — see Provenance below)

Will create project  "Pi Dash Conversion"  [PIDASHCONV]
  8 workflow states     Backlog, Todo, In Progress, In Review, In Test,
                        Done, Cancelled, Triage
  3 labels              porting, contract-tests, foundation
  1 work item type      Port Task            (reusing existing workspace type)
  2 wiki pages          Porting guide, Dead Python Code
  6 seed issues         all created in Backlog; 4 blocked_by edges

Agent settings (proposed by the package)
  ticking                 ON  →  INSTALLED OFF, see below
  cadence                 3h / 3h / 3h  (progress / review / test)
  budget                  10 runs per issue

This package asks for
  repo.write          agents push branches and open PRs
  repo.admin_merge    agents merge their own pull requests   ⚠ high impact
  shell, network
  env (names only)    DATABASE_URL, GH_TOKEN — you supply these to your runner
  executors           local_runner, managed_runner  (not cloud_agent)

Parameters
  repo_url     = https://github.com/me/my-service
  base_branch  = rust-dev

Review required
  The rulebook is 1,840 words of instructions your agents will execute.
  Ticking stays OFF until you read it and turn it on:
      pidash project show PIDASHCONV --rulebook

Nothing has been written. Re-run without --dry-run to install.
```

### 5.5 Conflicts

Every one of these is a real constraint in today's schema, and the plan
reports them before `apply` runs:

- **Project identifier** — `project_unique_identifier_workspace_when_deleted_at_null`
  on `(identifier, workspace)`. `Project.save()` upper-cases and strips
  it (`db/models/project.py`). On collision the plan fails with the
  conflicting project named; `--identifier NEW` resolves it.
- **Project name** — `project_unique_name_workspace_when_deleted_at_null`.
  Same treatment via `--name`.
- **States** — both create paths `bulk_create` the eight `DEFAULT_STATES`
  (`app/views/project/base.py:281`, `api/views/project.py:240`). The
  installer must therefore **replace** the seed, not append to it, or the
  project ends up with two "In Progress" states and an ambiguous
  workflow. Apply creates the project, then reconciles states by name:
  package states that match a seeded name are updated in place (so
  `default_state` and any FK stay valid), seeded states the package does
  not mention are deleted, package states with no match are created.
  Validation before apply: the package must supply at least one state in
  each of the `started`, `review` and `test` groups, because the ticking
  system's three stages are keyed on those groups
  (`.ai_design/issue_ticking_system/design.md` §3), and exactly one
  `default: true`.
- **Labels** — `unique_project_name_when_not_deleted` on `(project,
name)`. New project, so no conflict; duplicates _within_ the package
  are a validation error.
- **Work item types** — `IssueType` is workspace-scoped
  (`db/models/issue_type.py:15`), shared across projects via
  `ProjectIssueType`. Install **reuses** a workspace type whose name
  matches (attaching a new `ProjectIssueType`) and creates one only when
  no match exists. The plan says which of the two it will do, because
  reusing means the package inherits a type it did not define.
- **Pages** — new project, no conflict. Titles must be unique within the
  package.
- **Seed issues** — referenced by package-local `key`; `blocked_by`
  targets must resolve inside the package. A reference to an identifier
  outside the package (PIDASHCONV's rulebook mentions `PRIVATEPI1-84`
  and `PDASHOSS01-219`) is prose, not a relation, and is flagged at
  _export_ time (§6.2) because it will not resolve for the installer.

### 5.6 Safe defaults

These are not configurable by the package. That is the point.

| Setting                    | Installed value                                                                                            | Why                                                                                                                                                                           |
| -------------------------- | ---------------------------------------------------------------------------------------------------------- | ----------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `agent_ticking_enabled`    | **`False`**, always                                                                                        | The rulebook is unreviewed instructions. Nothing may start running before a human reads it. The package's proposed value is recorded and shown, so turning it on is one click |
| Seed issue state           | the package's declared state, and the plan must show it; the default and the recommendation is `Backlog`   | `Backlog` is inert (`.ai_design/issue_ticking_system/design.md`); an issue installed straight into `In Progress` would dispatch a run on install                              |
| `network`                  | `SECRET`                                                                                                   | Do not publish someone's new project to the workspace at large                                                                                                                |
| Members                    | installer only, as project Admin                                                                           | Matches `app/views/project/base.py:266`                                                                                                                                       |
| `default_agent_executor`   | from the parameter, defaulting to the instance default (`core/agent_execution.get_default_agent_executor`) | Never from the package                                                                                                                                                        |
| `repo_url` / `base_branch` | from parameters                                                                                            | Never from the package                                                                                                                                                        |

### 5.7 Upgrades

Installing a newer version onto an existing project is **out of scope for
v1** (§10, v3). But v1 pays one small cost now that makes it possible
later: `apply` records, per installed artifact (rulebook, each page, each
seed issue), the sha256 of the content it wrote (§8.1,
`installed_artifacts`). That is enough for a later upgrade to classify
each artifact as _unmodified since install_ (safe to replace) or _locally
edited_ (leave it, show the three-way diff, let the human decide) without
any additional bookkeeping. Skipping this in v1 would mean the first
upgrade has no way to tell an untouched page from a rewritten one.

What v1 _does_ ship is `pidash project diff <PROJ> <source>`: read-only,
shows what a newer package version would change. It is useful on its own
and it is the plan-generation half of the eventual upgrade.

---

## 6. Share and publish

### 6.1 Export

```
pidash project export PIDASHCONV --out ./pkg [--version 1.2.0]
                                 [--owner airepublic] [--allow-flagged]
                                 [--include-issues <selector>]
```

Writes the §4.1 layout. Issue selection defaults to **none** — a package
is a starting point, not a backup — with `--include-issues` taking a
state, label or explicit identifier list, so a publisher deliberately
chooses the seed backlog. The export is synchronous for a typical project
and moves to a job row mirroring `ExporterHistory`
(`db/models/exporter.py:24`) when it needs to be async (§8.1).

Round-trip is a test requirement, not an aspiration: export → install →
export must be byte-identical for everything the format claims to carry.

### 6.2 Scrub

Two passes, and they do different jobs.

**Pass 1 — structural exclusion.** Nothing from an excluded model is
read (§3.1). Members, runs, comments, workpads, tokens, `external_id`s
and UUIDs never enter the export path at all. This pass cannot "miss"
anything because it is an allowlist.

**Pass 2 — flagging over the included prose.** The rulebook and pages are
free text a human wrote, and humans put things in free text. The exporter
scans for, and **flags**:

- credential shapes — `ghp_…`, `github_pat_…`, `sk-…`, `xox[baprs]-…`,
  `AKIA…`, `-----BEGIN … PRIVATE KEY-----`, long base64/hex runs adjacent
  to `token`/`secret`/`password`/`key`;
- absolute private paths — `/Users/<name>/…`, `/home/<name>/…`,
  `C:\Users\…`;
- internal hosts and addresses — RFC1918 IPs, `*.internal`, `*.local`,
  `localhost` with a non-standard port;
- URLs with embedded credentials (`https://user:pass@…`);
- email addresses;
- **dangling work item identifiers** — `PROJ-123` references to issues
  outside the package, which will not resolve for the installer. This is
  not a security flag but it is a correctness one, and PIDASHCONV's
  rulebook has two of them.

Flags **fail the export** by default. They are not auto-redacted: silently
rewriting a rulebook produces a rulebook that no longer says what its
author meant, which is worse than a loud failure. The publisher either
edits the source, or passes `--allow-flagged` (recorded in the manifest
as `scrub: {acknowledged: [...]}`, so a reviewer of the package repo can
see what was waved through).

### 6.3 Registry protocol

A registry is a small read-mostly HTTP contract, specified in OSS so
anyone can self-host:

```
GET  /v1/index.json                                  # optional, static mode
GET  /v1/packages/{owner}/{name}                     # metadata + version list
GET  /v1/packages/{owner}/{name}/{version}           # one version's manifest
GET  /v1/packages/{owner}/{name}/{version}/download  # the tarball + digest
GET  /v1/search?q=&tag=                              # discovery
POST /v1/packages/{owner}/{name}/versions            # publish (auth required)
```

Two conformance levels:

- **Static registry** — just `index.json` plus tarballs behind any static
  host (GitHub Pages, S3, a plain nginx). No accounts, no server code, no
  publish endpoint; you publish by committing. This is the zero-infra
  option and it is what makes "self-hostable" real rather than
  theoretical.
- **Dynamic registry** — adds search, publish, install counts, ownership
  and moderation, and therefore needs accounts.

The client is configured with a registry list; `owner/name@version`
resolves against them in order.

### 6.4 What is OSS and what is cloud

| In OSS (this repo)                                       | In the AI Republic cloud                                 |
| -------------------------------------------------------- | -------------------------------------------------------- |
| The package format and its JSON Schema                   | The hosted public gallery at a Pi Dash domain            |
| `pidash project export` + scrub                          | Account-backed `owner` namespaces and ownership transfer |
| `pidash project install` from file / git / any registry  | Install counts, trending, curation                       |
| The registry protocol spec + a static-registry publisher | Fork / remix lineage graph, ratings and comments         |
| Signature _verification_                                 | Signing key custody, verified-publisher badges           |
| A self-hostable reference registry (v2)                  | Reporting, moderation queue, takedown                    |

The dividing line is: **anything that needs an identity, an abuse team or
a bill is cloud; the format and both ends of the pipe are OSS.** A
self-hosted Pi Dash can publish to and install from its own registry
forever without touching AI Republic infrastructure.

---

## 7. Trust and safety

This is the section that decides whether the feature should ship.

### 7.1 The actual threat

A project package is not data. It is **instructions that an autonomous
agent will execute on the installer's machine, with the installer's
repository credentials**, on a schedule, for days. PIDASHCONV's rulebook
tells agents to run `gh pr merge --admin --squash`, to run `psql` against
a database, and to merge without a human gate. That is a correct and
useful rulebook — and it is also, structurally, exactly what a malicious
package looks like.

So "prompt injection" here is not the usual third-party-content problem.
The package _is_ the prompt; the author is the adversary. The defences
that work are consent, legibility and blast-radius, not filtering.

| Risk                                                                              | Mitigation                                                                                                       | Ships in                       |
| --------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------- | ------------------------------ |
| Rulebook instructs exfiltration, destructive commands, or merging unreviewed code | Mandatory human review before anything runs (§7.2); ticking installed off; rulebook shown in full at install     | v1                             |
| Package silently acquires powerful capabilities                                   | `requires` declared, displayed, never implicit; unknown capability = hard error (§4.4 rule 2)                    | v1                             |
| Package overrides Pi Dash's own safety instructions                               | Package-supplied prompt content is confined to an allowlist of section keys; core sections are code-owned (§7.4) | v1                             |
| Malicious content added after you looked at it                                    | Content digest pinned at install; git sources resolve to a SHA; upgrade shows a diff (§5.7)                      | v1 (pin), v3 (diff-on-upgrade) |
| Impersonating a trusted publisher                                                 | Signing + verified publishers (§7.5)                                                                             | v2                             |
| Registry serves a tampered tarball                                                | Digest in the manifest index; signature verification                                                             | v2                             |
| Harmful package stays up                                                          | Report, yank, takedown (§7.6)                                                                                    | v2 (cloud)                     |

### 7.2 Mandatory review

Install never produces a project that can run an agent:

1. `agent_ticking_enabled` is `False`, unconditionally (§5.6).
2. The install record carries `reviewed_by` / `reviewed_at` /
   `reviewed_digest`, all null at install.
3. Turning ticking on — in the UI or via API — is **refused** while
   `reviewed_at` is null. The UI's toggle opens the rulebook first; the
   API returns a 409 naming the review endpoint. CLI install prints the
   rulebook path and the command to read it.
4. Non-interactive install (CI, scripting) requires
   `--accept-rulebook <sha256>`. Passing the digest — rather than a bare
   `--yes` — means an automated install cannot silently start accepting a
   _changed_ rulebook.
5. `reviewed_digest` records _what_ was accepted, so a later upgrade can
   tell whether re-review is needed.

This is one gate, it is cheap, and it is the whole reason the feature is
safe to ship.

### 7.3 Declared capabilities: disclosure, not enforcement — say so

v1 displays `requires` at install and records it. It does **not** enforce
it. Being precise about why: an agent's real power comes from the
environment the runner executes in — the checkout, the shell, the
credentials on that machine — not from anything Pi Dash hands it. A
package that declares only `repo.write` and then instructs the agent to
run `curl` is not stopped by a manifest field.

Pretending otherwise would be the dangerous move, so the UI says
"this package asks for", not "this package is limited to". Real
enforcement belongs in the runner's approval layer
(`runner/src/approval/`), which already gates what a local agent may do,
and is filed as follow-up work in §10. Until then, `requires` earns its
place by making the _intent_ legible — a package asking for
`repo.admin_merge` is visibly asking to merge its own code, and that is
exactly the flag a reviewer needs.

### 7.4 A package cannot rewrite Pi Dash's own instructions

Concretely enforceable, and worth pinning now. Package-supplied prompt
content may only land in:

- `Project.description` (the rulebook), which flows into the composed
  prompt as project context — content, not template (§4.5);
- work-type sections, once PDASHOSS01-234 lands, which fill the
  `Slot(...)` positions in a recipe;
- prompt section overrides whose `section_key` is in an explicit
  package-allowed allowlist.

It may **never** override the code-owned core sections that carry
Pi Dash's own operating rules — `guardrails`, `blocking`, `ending-run`,
`autonomy`, `pidash-cli` in `pi_dash/prompting/sections/`. Those are what
tell an agent not to touch paths outside the working directory, never to
print `PIDASHTOKEN`-prefixed values, and how to escalate. A package that
could rewrite them could turn off every other defence in this section.
This allowlist is checked at _install_, in the plan, so the violation is
visible before it is written.

### 7.5 Provenance

- **v1:** record the source (URL, and the resolved commit SHA for git)
  and the sha256 of the package tree in the install record. Show
  `unsigned` prominently in the plan. This is honest and it is enough for
  "I got it from a repo I trust".
- **v2:** detached signatures over the package digest, publisher public
  keys served by the registry, `--require-signature` for installs and an
  instance-level setting for admins who want it mandatory.
- **v3:** verified publishers — a registry-side identity claim shown as a
  badge, plus first-party curated packages.

Signing in v1 would be key-management theatre: with no registry to serve
public keys, the signature and the key would arrive by the same channel
as the package.

### 7.6 MCP is read-only for installs in v1

Installing a package is precisely the move "an agent gives itself a new
rulebook". The Cloud Agent toolset (`pi_dash/cloud_agent/tools.py`, with
its `READ_TOOLS` / `WRITE_TOOLS` allowlists in
`cloud_agent/policy.py:11`) therefore gets, in v1, a **read-only**
`pidash_preview_project_package` returning the plan — useful for an agent
asked to evaluate a package — and no install tool. Install requires a
human at a CLI or a browser. §11-D10 records this as a decision, not an
oversight.

### 7.7 Reporting and takedown (hosted registry)

Cloud-side, v2: a report button on every listing; a moderation queue;
**yank** (a version stops being resolvable for new installs and drops out
of search, but existing installs and explicit pinned references keep
working — deleting it outright would break reproducible installs); and
hard removal reserved for malware, with the digest added to a published
deny list that OSS clients can consult.

---

## 8. Data model and API

### 8.1 New models

All in `apps/api/pi_dash/db/models/project_package.py` (new file,
registered in `db/models/__init__.py`).

```python
class ProjectPackageInstall(BaseModel):
    """One row per project created from a package. See §5, §7.2."""
    project = models.OneToOneField("db.Project", related_name="package_install", ...)
    workspace = models.ForeignKey("db.Workspace", ...)

    source_kind = models.CharField(choices=[("file", ...), ("git", ...), ("registry", ...)])
    source_ref = models.CharField(max_length=1024)     # path, git URL@sha, or owner/name@version
    package_owner = models.CharField(max_length=64, blank=True, default="")
    package_name = models.CharField(max_length=64)
    package_version = models.CharField(max_length=32)
    schema_version = models.PositiveIntegerField()
    manifest_digest = models.CharField(max_length=71)  # "sha256:" + 64

    # Parameter values actually used. Declared env vars are names-only, so
    # this never holds a secret (§5.3) — but it is still admin-visible only.
    parameters = models.JSONField(default=dict)
    declared_requires = models.JSONField(default=dict)  # frozen copy of `requires`

    # path -> sha256 of the content written at install, for later upgrade
    # drift detection (§5.7).
    installed_artifacts = models.JSONField(default=dict)

    installed_by = models.ForeignKey(settings.AUTH_USER_MODEL, ...)
    reviewed_by = models.ForeignKey(settings.AUTH_USER_MODEL, null=True, ...)
    reviewed_at = models.DateTimeField(null=True)
    reviewed_digest = models.CharField(max_length=71, blank=True, default="")


class ProjectPackageExport(BaseModel):
    """Async export job. Mirrors ExporterHistory (db/models/exporter.py:24)."""
    project = models.ForeignKey("db.Project", related_name="package_exports", ...)
    status = models.CharField(choices=[("queued",...), ("processing",...),
                                       ("completed",...), ("failed",...)])
    options = models.JSONField(default=dict)   # version, owner, issue selector
    flags = models.JSONField(default=list)     # scrub findings (§6.2)
    url = models.URLField(max_length=800, null=True)
    reason = models.TextField(blank=True)
    initiated_by = models.ForeignKey(settings.AUTH_USER_MODEL, ...)
```

The shape follows an existing precedent: `Scheduler` (workspace-level
definition) + `SchedulerBinding` (per-project install) in
`db/models/scheduler.py:107`/`:154` is the same "reusable definition,
per-project install record" split, and `ProjectPackageInstall` is the
install half. The definition half lives in the registry, not in the
installing instance.

Registry-side models (`PackageListing`, `PackageVersion`, `PackageOwner`,
`PackageReport`, install counters) are **not** in this repo — they belong
to whatever serves the registry protocol (§6.4). A self-hosted reference
registry (v2) would add them behind the same protocol.

`ProjectPackageInstall.reviewed_at` is the field the ticking-enable path
must consult; that is the one place this feature touches existing code
(`Project.agent_ticking_enabled` write paths in the app API serializer
and the project settings view from PDASHOSS01-220).

### 8.2 API

Workspace-scoped, under the app API (`pi_dash/app/urls/project.py`
pattern):

| Method + path                                                   | Does                                                                                                           |
| --------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------- |
| `POST /api/workspaces/<slug>/project-packages/plan/`            | body: `{source, parameters, overrides}` → the `InstallPlan`. Writes nothing                                    |
| `POST /api/workspaces/<slug>/project-packages/install/`         | body: `{plan_token, accept_rulebook}` → creates the project. `plan_token` refers to the plan the user approved |
| `GET /api/workspaces/<slug>/projects/<pk>/package/`             | the `ProjectPackageInstall` record                                                                             |
| `POST /api/workspaces/<slug>/projects/<pk>/package/review/`     | body: `{digest}` → sets `reviewed_by`/`reviewed_at`/`reviewed_digest`                                          |
| `POST /api/workspaces/<slug>/projects/<pk>/package-export/`     | queues a `ProjectPackageExport`                                                                                |
| `GET /api/workspaces/<slug>/projects/<pk>/package-export/<id>/` | job status + scrub flags + download URL                                                                        |

`apply` runs inside one `transaction.atomic()`: either the whole project
exists with its states, labels, types, pages and seed issues, or nothing
does. A half-installed project — states replaced, seed issues missing —
would be worse than a failed install.

### 8.3 CLI

Extends `runner/src/cli/project.rs`, which currently has one subcommand
(`ProjectCommand::List`, `:20`):

```
pidash project install <source> [--param k=v]... [--dry-run]
                                [--name N] [--identifier ID]
                                [--accept-rulebook <sha256>] [--yes]
pidash project export <PROJ> --out <dir|tgz> [--version V] [--owner O]
                             [--include-issues <selector>] [--allow-flagged]
pidash project show <PROJ> --package | --rulebook
pidash project diff <PROJ> <source>
pidash project publish <dir> --registry <url>          # v2
```

All of them honour the CLI's existing output contract — one JSON document
on stdout, `{"error": ...}` on stderr with a non-zero exit — except the
interactive prompts, which go to stderr so `--dry-run | jq` still works.

### 8.4 MCP

- `pidash_preview_project_package(source, parameters)` → the plan.
  Read-only, added to `READ_TOOLS` in `cloud_agent/policy.py`.
- No install tool in v1 (§7.6).

### 8.5 Permissions

| Action                      | Required                                                                                                                                                                                                                                           |
| --------------------------- | -------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| Plan / dry-run              | workspace Member (it only reads)                                                                                                                                                                                                                   |
| Install                     | workspace **Admin or Member** — matching project create today, `@allow_permission([ROLE.ADMIN, ROLE.MEMBER], level="WORKSPACE")` at `app/views/project/base.py:257`. The installer becomes project Admin, as project create already does at `:266` |
| Mark reviewed               | project Admin                                                                                                                                                                                                                                      |
| Enable ticking after review | project Admin (unchanged from PDASHOSS01-220)                                                                                                                                                                                                      |
| Export                      | project Admin — an export reveals the whole rulebook and backlog                                                                                                                                                                                   |
| Publish                     | a registry credential; not an instance permission                                                                                                                                                                                                  |

---

## 9. Worked example: exporting PIDASHCONV

PIDASHCONV is the Django→Rust port project: 251 issues, a ~1,800-word
rulebook in its description, two wiki pages, and agents that merge their
own PRs.

### 9.1 `pidash project export PIDASHCONV --out ./pkg --owner airepublic --version 1.0.0`

**Included.** `rulebook.md` — the description, essentially verbatim: the
stage table, "Code only under `${code_root}`, PRs into `${base_branch}`",
"Translate, don't redesign: same URL paths, same JSON byte for byte",
the contract-test rules, "Backlog is inert. The coordinator scheduler
releases issues…", the open-blockers hard stop, the environment-variable
rule, the review procedure, and the five-step merge procedure ending in
`gh pr merge <n> --admin --squash --delete-branch`. Plus the eight
workflow states, the project's labels and work item types, and two pages:
`pages/porting-guide.md` and `pages/dead-python-code.md`.

**Parameterised.** Four things, and each is a good illustration of why
the parameter list exists:

| Parameter     | PIDASHCONV's value                           | Why                                                                  |
| ------------- | -------------------------------------------- | -------------------------------------------------------------------- |
| `repo_url`    | `https://github.com/The-AI-Republic/pi-dash` | The installer's repo is not ours                                     |
| `base_branch` | `rust-dev`                                   | The installer's integration branch                                   |
| `code_root`   | `rust-api/`                                  | Appears ~6 times in the rulebook; substituted at install (§4.5)      |
| `executor`    | `local_runner`                               | The rulebook needs a shell and `psql`; `cloud_agent` cannot serve it |

**Declared.**

```json
"requires": {
  "capabilities": ["repo.write", "repo.admin_merge", "shell", "network", "db.write"],
  "executor_kinds": ["local_runner", "managed_runner"],
  "env": ["DATABASE_URL", "BASE_URL"],
  "pidash_version": ">=0.24"
}
```

`repo.admin_merge` is the honest declaration of `gh pr merge --admin`,
and it is the line in the install screen a reviewer should stop at.

**Seed issues.** Not the 251 real ones. `--include-issues label:template`
picks the shape: the rulebook issue (PIDASHCONV-1's "read this first"
role), one domain epic, and the two contract-test scaffolding issues,
with their `blocked_by` edges rewritten to package-local keys.

**Flagged by the scrub (§6.2), export fails until resolved.**

- `PRIVATEPI1-84` and `PDASHOSS01-219` — dangling identifiers from the
  "Known today" workaround notes, meaningless in another workspace.
  Publisher rewrites them as prose ("page writes may return 503; …").
- "never touch the shared postgres database" — an internal reference;
  kept after `--allow-flagged`, recorded in `scrub.acknowledged`.
- `https://github.com/The-AI-Republic/pi-dash` in the prose — the
  publisher replaces it with `${repo_url}`.

**Excluded without being asked.** 251 issues and their comment threads;
every `AgentRun` and its transcripts, `done_payload` and
`prompt_manifest`; every workpad; the project's members; the runners the
work executed on; all ticker runtime state.

### 9.2 Installing into a fresh workspace

```
$ pidash project install git+https://github.com/me/pidash-packages@v1.0.0 \
      --param repo_url=https://github.com/me/my-service \
      --param base_branch=rust-dev \
      --param code_root=rust-api/ \
      --dry-run
```

The installer sees the §5.4 plan: the project and identifier that will be
created, eight states, the two pages, four seed issues with their
blocked-by edges, the proposed 3h/3h/3h cadence and 10-run budget, the
capability list with `repo.admin_merge` flagged, the resolved parameters,
and `unsigned` against the digest.

Re-run without `--dry-run` and the project exists — **with ticking off**.
The rulebook says agents should merge their own PRs; that is a decision
the installer has to make knowingly. They read
`pidash project show MYPORT --rulebook`, decide the merge rule is
acceptable in their repo (or edit the description to remove it), mark it
reviewed, and turn ticking on. From there the project behaves exactly
like PIDASHCONV.

---

## 10. Phasing and follow-up issues

**v1 — the format and both ends of the pipe (OSS).** Export with scrub,
install from local file and git, plan/dry-run, parameters, conflict
handling, the install record, ticking-off-until-reviewed, and capability
disclosure. No registry, no signing, no upgrades. This alone solves the
stated problem: PIDASHCONV becomes shareable, reviewable in a git repo,
and installable.

**v2 — community.** Registry protocol + static-registry publisher in OSS,
the hosted gallery in the cloud, `pidash project publish`, signing and
signature verification, reporting and takedown.

**v3 — living packages.** In-place upgrade with three-way merge over the
digests v1 already records, `pidash project diff` promoted to an upgrade
driver, fork / remix lineage, ratings and comments, capability
_enforcement_ through the runner approval layer.

### Follow-up issues to file once this design is approved

Filed after approval, not in this PR.

| #    | Scope                                                                                                                                                                                                         | Acceptance criteria                                                                                                                                                                                                                                                                                                                              |
| ---- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ |
| P-1  | **Package format + JSON Schema + conformance fixtures.** `pidash-project.schema.json`, the fixture packages and their canonical expected plans under `.ai_design/project_packages/fixtures/`. No runtime code | Schema validates all fixtures; an invalid fixture fails with a located error; fixtures are the shared contract both P-2 and the Rust port test against                                                                                                                                                                                           |
| P-2  | **Exporter + scrub (API + `pidash project export`).** `ProjectPackageExport`, the allowlist exporter, the flagging pass, the CLI command                                                                      | Exporting a project with a known-dirty rulebook fails and names every flag; `--allow-flagged` records acknowledgements in the manifest; export of a fixture-equivalent project matches its fixture; no excluded model is reachable from the export path (test asserts the allowlist)                                                             |
| P-3  | **Installer: plan + apply (API).** `ProjectPackageInstall`, plan generation, `apply` in one transaction, state reconciliation, issue-type reuse, `blocked_by` wiring                                          | Plan writes nothing and is deterministic for a given (package, params, workspace); apply is atomic (an induced failure mid-apply leaves no project); installed project has the package's states with exactly one default and at least one each of started/review/test; ticking is off; install record carries source, digest and artifact hashes |
| P-4  | **`pidash project install` CLI.** Sources (file, dir, tgz, git), parameter prompting, `--dry-run` rendering, conflict flags, `--accept-rulebook`                                                              | Round-trip test: export PIDASHCONV-like fixture → install into a second workspace → export again → byte-identical; `--dry-run` output matches the plan API; tarball extraction rejects path traversal and oversized members                                                                                                                      |
| P-5  | **Review gate.** `reviewed_*` fields, the review endpoint, the 409 on enabling ticking before review, UI rulebook-review screen                                                                               | Enabling `agent_ticking_enabled` on an unreviewed installed project is refused by API and UI; reviewing records who/when/which digest; a project not created from a package is unaffected                                                                                                                                                        |
| P-6  | **Web UI: install and package panel.** Source entry, parameter form, plan preview, capability disclosure, post-install review panel                                                                           | A user can install from a git URL end to end without the CLI; the capability list and the unsigned/provenance line are visible before the confirm button                                                                                                                                                                                         |
| P-7  | **MCP preview tool.** `pidash_preview_project_package` in `READ_TOOLS`                                                                                                                                        | Tool returns the plan; no install tool exists; a write-policy test asserts install is not reachable from the agent toolset                                                                                                                                                                                                                       |
| P-8  | **`pidash project diff`.** Read-only comparison of an installed project against a package version, using `installed_artifacts`                                                                                | Correctly classifies each artifact as unchanged / locally-edited / changed-upstream; exits non-zero when drift exists (so CI can gate)                                                                                                                                                                                                           |
| P-9  | **Registry protocol spec + static registry publisher** (v2)                                                                                                                                                   | Spec document + a `pidash project publish --static` that writes a valid `index.json` tree; `install owner/name@version` resolves against a static registry served from a plain file server                                                                                                                                                       |
| P-10 | **Capability enforcement through the runner approval layer** (v3)                                                                                                                                             | Declared capabilities become constraints in `runner/src/approval/`; a package declaring only `repo.write` cannot silently get `repo.admin_merge`                                                                                                                                                                                                 |

P-1 gates P-2 and P-3. P-3 gates P-4, P-5 and P-6. P-7, P-8 are
independent once P-3 lands.

---

## 11. Open decisions

Each has a proposed default. Confirm or overturn in review; the outcome
becomes the pinned contract.

**D1 — Manifest format.** _Default: JSON with a published JSON Schema._
Zero new dependencies on either side (`json` in the Python stdlib,
`serde_json` already at `runner/Cargo.toml:25`; neither PyYAML nor
`serde_yaml` is currently a dependency), and it is unambiguous for the
byte-for-byte conformance fixtures. The prose humans review lives in
markdown regardless. Alternative: YAML, at the cost of a new parser in
two languages.

**D2 — Identity scheme.** _Default: `owner/name@semver`, with `owner`
advisory for file and git installs and authoritative only when a registry
vouches for it._ File/git packages are identified by source + digest.

**D3 — Issue templates.** The issue description lists them as an
include; **there is no `IssueTemplate` model anywhere in `apps/api` or
`apps/web`** — verified at this commit. _Default: package-format-only.
A package's `issues[]` entries are seed issues; a "template" is simply a
seed issue in a state the project treats as a template (a label, or the
`Backlog` state). If a real template model lands later, the format gains
a `templates[]` key under a bumped `schema_version`._

**D4 — Prompt section overrides.** _Default: carried in the format,
rejected at install with a clear error until the project-scope layer from
`.ai_design/prompt_section_system/design.md` §9.4 exists; bypass with
`--skip-prompt-overrides`._ Installing them at workspace scope would
silently re-prompt every unrelated project in the workspace.

**D5 — Work types (PDASHOSS01-234).** _Default: optional `work_type`
string, accepted and stored; if the instance has no work-type registry,
warn once and install anyway._ The project is still fully usable with
today's stage prompts, so failing the install would be
disproportionate — unlike D4, which has a wrong-blast-radius failure mode.

**D6 — Install target.** _Default: install always creates a **new**
project._ Installing onto an existing project means reconciling live
issues against package seeds, which is the hard half of upgrades (v3).

**D7 — Seed issue cap and landing state.** _Default: 200 issues max per
package (validation error above it), and seed issues land in the state
the package declares, which the plan always displays; publishers are
directed to `Backlog` because it is inert._ A package that lands issues
in `In Progress` would dispatch agent runs at install time — the plan
must make that impossible to miss.

**D8 — Schedulers, cycles, modules, views, estimates.** _Default:
excluded from v1._ `SchedulerBinding` (`db/models/scheduler.py:154`) is
the interesting one — it is genuinely agent behaviour — but it references
a workspace-level `Scheduler` definition that will not exist in the
installer's workspace, so carrying it means carrying scheduler
definitions too. Deferred to v2 as its own decision.

**D9 — Wiki page fidelity.** _Default: markdown only._ `Page` stores a
Yjs binary (`description_binary`, `db/models/page.py:33`) for collaborative
editing; install creates fresh pages from markdown and lets the editor
rebuild its own representation. Carrying Yjs state would couple the
format to an editor version.

**D10 — MCP install.** _Default: no install tool for agents in v1; a
read-only preview tool only._ Reconsider in v3 once capability
enforcement (P-10) exists.

**D11 — Registry hosting split.** _Default: OSS gets the format, both
ends of the pipe, the protocol spec and a static-registry publisher; the
AI Republic cloud hosts the public gallery, identity, moderation and
verified publishers_ (§6.4).

**D12 — Signing.** _Default: deferred to v2; v1 records source + digest
and shows `unsigned`._ With no registry to distribute public keys, a v1
signature would travel by the same channel as the package it signs.

**D13 — License field.** _Default: `license` is **required** in the
manifest and must be a valid SPDX identifier._ A rulebook is a creative
work; a community that cannot tell whether it may fork one will not fork
one. This repo already enforces SPDX headers on source files
(`COPYRIGHT_CHECK.md`).

**D14 — Parameter substitution syntax.** _Default: `${name}`, applied
once at install time, `$$` for a literal `$`, unresolved reference is an
error._ Explicitly not Jinja and explicitly not deferred to run time
(§4.5).

**D15 — Export default for issues.** _Default: no issues unless
`--include-issues` selects them._ A package is a starting point, not a
backup; exporting 251 issues by default would make every package a data
dump and every install a mess.
