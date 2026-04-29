# Pod ↔ Project ↔ Runner — Relationship Refactor

**Status:** in design
**Supersedes (within this dir):** the workspace-default-pod assumption baked into the parent `design.md` §4–§6 and `apps/api/pi_dash/runner/migrations/0004_add_pod_and_run_identity.py`'s "one default pod per workspace" model.

This doc refines the multi-runner design so that **a runner is bound to one project** (not just to a workspace), and **a pod is a sub-grouping of runners within one project** (not a workspace-wide bucket). The token + WebSocket multiplex layer (parent `design.md` §4–§6) is unchanged; only the pod/project/runner data model and the dispatch path are reshaped.

The parent `decisions.md` and `tasks.md` continue to govern the multi-runner shell (auth, demux, multi-Hello). Q&A specific to _this_ refactor lives in `./decisions.md`; tasks live in `./tasks.md`.

---

## 1. Why

The shipped multi-runner design assumed one pod per workspace and used a runner's `working_dir` as the only de-facto link to a project. That works for a one-project workspace and breaks the moment a developer has multiple repos / multiple Pi Dash projects on the same machine. The two real-world scenarios that motivate the change:

- **One developer, three projects.** Dev machine A has `~/work/repoP`, `~/work/repoQ`, `~/work/repoR` — three separate repos that map to three Pi Dash projects (P, Q, R). Today, all three would land in firstdream's single workspace pod, and any issue from any project could be dispatched to any of A's runners regardless of which repo each runner has cloned. The runner would then fail (wrong repo) or — worse — overwrite the wrong tree.
- **One project, two contributors.** Dev machine A and dev machine B both work on project P. The "fleet for P" is the union of runners-on-A-serving-P and runners-on-B-serving-P. That fleet is a useful primitive: routing, capacity reporting, tier separation. It is what we want a _pod_ to mean.

The refactor makes those two scenarios first-class and replaces "pod = workspace-wide bucket" with "**pod = project-scoped runner group**."

## 2. Goals

- A runner is registered against a specific project; it cannot serve any other project.
- A pod groups runners that all serve the same project. One project may have multiple pods (tier / region / branch separation); each pod belongs to exactly one project.
- Each project has exactly one **default pod** auto-created with the project. Dispatch routes new issues there.
- Cross-machine pods: machine A's runner-for-P and machine B's runner-for-P are in the same pod by construction.
- Per-machine multi-project: machine A can host runners for P, Q, R simultaneously (one runner per repo).
- Working-directory uniqueness on a machine stays enforced (already in `Config::validate()`).

## 3. Non-goals

- **No workspace-wide pod.** The legacy `Pod(workspace=…, project=NULL, is_default=True)` row goes away. Existing data migrations are out of scope (see §11).
- **No cross-project work-stealing.** A runner only serves work from its own pod's project. (See `decisions.md` Q4.)
- **No runtime project re-binding.** A runner's project is set at registration and is immutable. To "move" a runner to a different project, deregister and re-add it.
- **Routing rules for non-default pods are deferred.** This refactor lands the schema + auto-default-pod + dispatch-to-default. Choosing which pod a non-default issue goes to (per-issue target field, project-level routing rule, capability tags) is a follow-up.
- **Pod-level approval policy / pod-level config.** Out of scope. Approval policy stays per-runner.

## 4. Vocabulary

| Term             | Meaning                                                                                                                                                                                    |
| ---------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ |
| **Workspace**    | Pi Dash tenant (e.g. `firstdream`). Existing concept, unchanged.                                                                                                                           |
| **Project**      | A workspace-scoped product / repo (e.g. `firstdream-WEB`). Existing concept; gains an auto-created default pod.                                                                            |
| **Pod**          | A project-scoped runner grouping. Cardinality ≥ 1 per project (the default pod is auto-created; user can add tier pods). Each pod owns exactly one project; one project can own many pods. |
| **Runner**       | A logical agent. Bound to exactly one project, exactly one pod, and one git working directory on its host machine.                                                                         |
| **MachineToken** | Unchanged. Authenticates the daemon's WebSocket and authorises a set of runner_ids; can span multiple projects within its workspace.                                                       |

A daemon can host runners spanning multiple projects (and therefore multiple pods) under a single MachineToken — only the wire-level routing changes:

```
MachineToken T  (workspace = firstdream)
 │
 ├─ Runner R1  → pod WEB_pod_1   (project firstdream-WEB)   working_dir /home/u/web
 ├─ Runner R2  → pod API_pod_1   (project firstdream-API)   working_dir /home/u/api
 └─ Runner R3  → pod WEB_beefy   (project firstdream-WEB)   working_dir /home/u/web-perf
```

R1 and R3 both serve project WEB but live in different pods (one default, one beefy). All three share one WebSocket session under T.

## 5. Schema

### 5.1 `Pod` (modified)

```python
class Pod(models.Model):
    # Existing fields kept: id, name, description, created_by, deleted_at,
    # created_at, updated_at, workspace.

    # NEW: pod is project-scoped. NOT NULL post-refactor; nullable kept
    # only for the existence of the post-create signal window
    # (see §6.1) and removed once the signal is wired.
    project = models.ForeignKey(
        "db.Project",
        on_delete=models.CASCADE,
        related_name="pods",
    )

    # Existing field kept; semantics changes from
    #   "the workspace's default pod"
    # to
    #   "this project's default pod (exactly one per project)".
    is_default = models.BooleanField(default=False)

    class Meta:
        # NEW constraint: at most one default pod per project at any time.
        # Replaces the existing "one default pod per workspace" constraint.
        constraints = [
            models.UniqueConstraint(
                fields=["project"],
                condition=Q(is_default=True) & Q(deleted_at__isnull=True),
                name="pod_one_default_per_project_when_active",
            ),
            # NEW: pod name uniqueness scoped to project (not workspace),
            # because two projects can each legitimately have a "pod_1".
            models.UniqueConstraint(
                fields=["project", "name"],
                condition=Q(deleted_at__isnull=True),
                name="pod_unique_name_per_project_when_active",
            ),
        ]
```

`Pod.workspace` becomes a denormalised convenience (always equals `pod.project.workspace`). We keep it for cheap filtering in dashboard queries; on save we enforce the equality.

### 5.2 `Runner` (modified)

```python
class Runner(models.Model):
    # Existing fields kept, including: workspace, pod, machine_token, name,
    # credential_hash, etc.

    # No new FK column — `runner.pod.project` is the project. Adding a
    # direct `runner.project` FK would duplicate the source of truth and
    # invite drift. Helpers below provide the convenience.

    @property
    def project(self):
        return self.pod.project

    @property
    def project_id(self):
        return self.pod.project_id
```

The auto-resolution in `Runner.save()` (today: `pod = Pod.default_for_workspace_id(workspace_id)`) is removed. From this refactor on, **`pod` is a required argument at runner creation time**; views / management code resolve it via `Pod.default_for_project(project)`.

### 5.3 `Project` (unchanged columns; new behaviour)

No schema change. New behaviour: a `post_save(sender=Project)` signal handler auto-creates the project's default pod (§6.1).

### 5.4 `AgentRun` (unchanged columns; new dispatch rule)

No schema change. The `pod` FK is unchanged; what changes is **how `pod_id` is filled in**. See §8.

### 5.5 `Issue` (unchanged columns; new auto-resolution rule)

No schema change. `Issue.assigned_pod` (a `PROTECT` FK to `Pod`, today auto-filled from the workspace default in `Issue.save()`) gets two behavior changes:

- `Issue.save()`'s auto-resolution flips from `Pod.default_for_workspace_id(workspace_id)` to `Pod.default_for_project_id(project_id)`. New issues land in their project's default pod.
- The serializer in `apps/api/pi_dash/app/serializers/issue.py` (`validate_assigned_pod`-style block at line 180–194) currently checks `pod.workspace_id == issue.workspace_id`. Under the new model that's a cross-project escape hatch (a Project P issue could be assigned a pod belonging to Project Q in the same workspace, and dispatch would happily route P's runs into Q's pod). The check changes to `pod.project_id == issue.project_id`. Pod-soft-deleted check is kept.

The `PROTECT` FK is intentional and stays — it ensures pods can't be physically removed while issues still reference them. This shapes the migration story (see §11): pre-existing `issues.assigned_pod` rows must be NULLed (or re-pointed) before pods can be deleted.

### 5.6 What is removed

- `Pod.default_for_workspace()` and `Pod.default_for_workspace_id()` are removed. Replaced by `Pod.default_for_project(project)` and `Pod.default_for_project_id(project_id)`.
- The `Runner.save()` auto-pod-resolution branch keyed off `workspace_id`.
- Uniqueness constraint `pod_one_default_per_workspace_when_active`.
- Uniqueness constraint `pod_unique_name_per_workspace_when_active`.
- The `runner/signals.py:create_default_pod_for_new_workspace` `post_save(Workspace)` handler. Replaced by §6.1's `post_save(Project)` handler. With `Pod.project` becoming NOT NULL, the workspace-level handler would either crash on save or keep recreating the legacy model, so we delete it outright.
- The `runner/management/commands/ensure_workspace_pods.py` command. Replaced by `ensure_project_pods.py` (per-project parity), or removed entirely if the migration's required backfill (§11) covers all bootstrap cases.

## 6. Pod lifecycle

### 6.1 Auto-create on project save

`signals.post_save(sender=Project)` handler:

- On **create**, atomically create one Pod with `project=project, is_default=True, name=f"{project.identifier}_pod_1"`.
- On **update**, no-op.
- Idempotent: if a default pod already exists for the project, do nothing.

The signal lives in `pi_dash/runner/signals.py` (or wherever existing pod lifecycle lives) and is connected in `apps/runner/apps.RunnerConfig.ready`.

### 6.2 User-created additional pods

UI / API surface:

- `POST /api/runners/pods/` with `{ project_id, name }`. Server validates the name (§6.3) and writes a non-default pod.
- `PATCH /api/runners/pods/<pod_id>/` to rename. Same validation.
- Soft-delete via `DELETE /api/runners/pods/<pod_id>/`. Refused if the pod has any non-revoked runner. Refused if the pod is the project's default pod (you can't delete the default; transfer the flag first via a future endpoint, out of scope here).

There is no API to create a pod outside an existing project — every pod has a project.

### 6.3 Naming convention

|                                     |                                                                                                                                                                                            |
| ----------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ |
| **Default pod (auto-created)**      | `{project.identifier}_pod_1`. The trailing `_1` is reserved for the auto-default; if more pods are auto-created in the future (not by this design) they would get `_pod_2`, `_pod_3`, etc. |
| **User-created pod (typical case)** | `{project.identifier}_{user_supplied_suffix}`. Example: `WEB_beefy`, `WEB_us_east`. The prefix is mandatory and server-enforced.                                                           |
| **Server-side validation**          | Pod name regex: `^{re.escape(project.identifier)}_[A-Za-z0-9._-]{1,96}$`. Length ≤ 128 chars.                                                                                              |
| **Reserved suffix**                 | `pod_<digit+>` is reserved for system-auto-generated names; user-supplied suffixes can't match `pod_\d+` to avoid collision/confusion with auto-default.                                   |
| **Renaming**                        | Allowed via PATCH; the prefix is preserved (server rejects rename that drops or changes the prefix).                                                                                       |

### 6.4 Default-pod invariant

Exactly one `is_default=True` pod per project at all times. Enforced by the unique constraint. Auto-create satisfies it; user-created pods always have `is_default=False`. Transferring the default flag is a future operation (out of scope).

## 7. Runner registration

### 7.1 Token-authenticated registration (`/api/v1/runner/register-under-token/`)

Body (changed):

```json
{
  "name": "<RUNNER_NAME>",
  "project": "<PROJECT_IDENTIFIER>", // NEW, required
  "pod": "<POD_NAME>", // NEW, optional; defaults to project's default pod
  "os": "...",
  "arch": "...",
  "version": "...",
  "protocol_version": 2
}
```

Server resolution:

1. Authenticate the MachineToken via `X-Token-Id` + bearer secret (unchanged).
2. Resolve the project: `Project.objects.get(workspace=token.workspace, identifier=body.project)`. 404 if not in this token's workspace.
3. Resolve the pod:
   - If `pod` is omitted → use the project's default pod.
   - If `pod` is supplied → `Pod.objects.get(project=project, name=body.pod)`. 404 if not found, 400 if soft-deleted.
4. Create the runner with the resolved pod. Cap check stays per-machine (`MAX_RUNNERS_PER_MACHINE`); a per-project cap is out of scope.

Response (changed):

```json
{ "runner_id": "<UUID>", "pod_id": "<UUID>" }
```

`pod_id` is added so the daemon can stamp it in `config.toml` for diagnostics. The credential_secret field stays gone (already removed in this PR's earlier work; token-auth runners don't carry their own bearer).

### 7.2 Legacy registration (`/api/v1/runner/register/`)

The same shape change: the request body now requires `project` (and optionally `pod`). The legacy single-runner enrollment flow has no notion of which project the runner serves until the user picks one, so the one-time-token-paired-with-CLI invocation has to surface this. The `pidash configure` CLI gains `--project <slug>` to pass it through (§9).

For installs that target a workspace with exactly one project, **the runner-side CLI can pre-fill the project for ergonomics**, but the wire request always carries it explicitly so the cloud is never guessing.

### 7.3 Runner-side `RunnerConfig` (Rust)

`runner/src/config/schema.rs`:

```rust
pub struct RunnerConfig {
    pub name: String,
    pub runner_id: Uuid,
    pub workspace_slug: Option<String>,   // existing, kept

    // NEW: project this runner serves. Set at registration. The
    // daemon never re-binds; rerun `pidash token add-runner` to
    // change.
    pub project_slug: String,

    // NEW: pod_id the cloud assigned. Used only for `pidash status`
    // displays and as a self-check at Hello time. Not authoritative —
    // cloud's runner row is the source of truth.
    pub pod_id: Option<Uuid>,
    // Optional in case of older configs; required for fresh registrations.

    pub workspace: WorkspaceSection,    // working_dir
    pub agent: AgentSection,
    pub codex: CodexSection,
    pub claude_code: ClaudeCodeSection,
    pub approval_policy: ApprovalPolicySection,
}
```

Validation (`Config::validate()`) gains:

- `project_slug` non-empty for every `[[runner]]` block.
- (Existing) per-machine `working_dir` uniqueness — already enforced; we keep it. This is the constraint the user phrased as "different runners in the same machine can't have the same workspace value."

### 7.4 Hello payload

`ClientMsg::Hello` gains an optional `project_id: Option<Uuid>` (or `project_slug: Option<String>`) field. Cloud cross-checks it against `runner.pod.project` and rejects with a `RemoveRunner` if they disagree. Backward-compatible: missing field skips the cross-check (legacy daemons keep working).

The MachineToken still authorises the runner_id; the project field is purely a sanity gate.

## 8. Dispatch

Pod resolution flips at three call sites; they collectively constitute "dispatch" in this design:

**8.1 `Issue.save()`** (in `apps/api/pi_dash/db/models/issue.py:199`). Auto-resolution flips:

```python
# Before: workspace default
default_pod = Pod.default_for_workspace_id(workspace_id)

# After: project default
default_pod = Pod.default_for_project_id(self.project_id)
```

**8.2 Issue serializer cross-check** (in `apps/api/pi_dash/app/serializers/issue.py:180–194`). The "pod must be in the same workspace" guard is replaced by "pod must be in the same project":

```python
# Before
if pod.workspace_id != issue.workspace_id:
    raise serializers.ValidationError(...)

# After
if pod.project_id != issue.project_id:
    raise serializers.ValidationError({"assigned_pod": "pod is in a different project"})
```

**8.3 `AgentRun` creation** (in orchestration code that materialises runs from issues):

```python
# Before
agent_run = AgentRun.objects.create(
    workspace=issue.workspace,
    pod=Pod.default_for_workspace_id(issue.workspace_id),
    work_item=issue,
    ...
)

# After
agent_run = AgentRun.objects.create(
    workspace=issue.workspace,
    pod=issue.assigned_pod or Pod.default_for_project_id(issue.project_id),
    work_item=issue,
    ...
)
```

`issue.assigned_pod` already takes precedence (existing semantics from `.ai_design/issue_runner/design.md` §4.4); the only change is the fallback when it's NULL.

`select_runner_in_pod()` is unchanged — once `pod_id` is right, the pod-scoped matcher already does the right thing. The pinning logic (`pinned_runner`) is unchanged: a follow-up run pins to the previous runner, that runner is in the same pod, dispatch finds it.

Runs whose issue has no project (legacy, shouldn't exist post-refactor) trigger an explicit error rather than silently falling back to a workspace pod — the workspace-default pod is gone.

## 9. CLI surface

### 9.1 `pidash configure` (initial registration)

```
pidash configure \
  --url <CLOUD_URL> \
  --token <ONE_TIME_REG_TOKEN> \
  --project <PROJECT_IDENTIFIER> \      # NEW, required
  [--pod <POD_NAME>] \                  # NEW, optional (defaults to project's default pod)
  --name <RUNNER_NAME> \
  --working-dir <PATH> \
  ...
```

If `--project` is omitted on a fresh setup, the CLI hard-errors with a hint listing the workspace's projects and their identifiers (one-shot REST call to `/api/runners/projects/` with the registration token's workspace).

### 9.2 `pidash token add-runner`

```
pidash token add-runner \
  --project <PROJECT_IDENTIFIER> \
  [--pod <POD_NAME>] \
  --name <RUNNER_NAME> \
  --working-dir <PATH> \
  --agent codex|claude-code
```

`--project` is always required (even if the workspace has only one project — explicitness over magic). `--pod` defaults to the project's default pod.

### 9.3 `pidash token list-projects` (new)

For discovery. Calls `/api/runners/projects/` with the locally-installed token credentials, prints `identifier` + `name` so the user can pick.

### 9.4 `pidash status` (and other CLI verbs)

Multi-runner UX (separate task block in `tasks.md`): list all configured runners with their project + pod + status. The `--runner <name>` selector applies to per-runner verbs (`pidash issue`, `pidash comment`, etc.).

## 10. Validation rules (consolidated)

| Layer          | Rule                                                                                                                                 | Where enforced                                                      |
| -------------- | ------------------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------- |
| DB             | Exactly one `is_default=True` pod per project (active).                                                                              | `pod_one_default_per_project_when_active` constraint                |
| DB             | Pod name unique per (project, active).                                                                                               | `pod_unique_name_per_project_when_active` constraint                |
| DB             | `pod.workspace_id == pod.project.workspace_id`.                                                                                      | Model `clean()` (or pre-save signal).                               |
| Cloud API      | Pod name matches `^{project.identifier}_[A-Za-z0-9._-]{1,96}$`.                                                                      | `pods.py` view validation                                           |
| Cloud API      | `register-under-token/` rejects unknown project / wrong-workspace project.                                                           | `machine_tokens.py` view                                            |
| Cloud API      | `register-under-token/` rejects soft-deleted pod.                                                                                    | `machine_tokens.py` view                                            |
| Cloud API      | Issue serializer rejects `assigned_pod` whose `project_id` ≠ issue's `project_id`. (Was: workspace-equality check.)                  | `apps/api/pi_dash/app/serializers/issue.py`                         |
| Cloud WS       | Hello with `project_id` mismatching `runner.pod.project_id` → reply with `RemoveRunner`.                                             | `consumers._handle_token_hello`                                     |
| Runner config  | Every `[[runner]]` has non-empty `project_slug`.                                                                                     | `Config::validate()`                                                |
| Runner config  | No two `[[runner]]` blocks share a `working_dir`, including nested-prefix overlap.                                                   | `Config::validate()` (existing)                                     |
| Runner config  | A single machine MAY have multiple runners for the same project (different working_dirs). Disallowed only if `working_dir` collides. | Implicit: working_dir uniqueness is the only per-machine constraint |
| Daemon ↔ cloud | Token's workspace matches the runner's workspace via the pod chain.                                                                  | Cloud-side authz on every project/pod resolution                    |

## 11. Migration story

**No data migration of existing pod/runner state.** Per `decisions.md` Q2: there are no production users; existing local-dev data is dropped. **A backfill of `Pod` rows for existing `Project` rows is required** (not optional) because the new auto-create signal only fires on Project create — pre-existing projects would otherwise have no pods, breaking registration and dispatch.

The migration plan:

1. **Schema migration step 1** (`runner/migrations/0007_pod_project_relationship.py`):
   - Add `Pod.project` column (nullable initially, to satisfy Django's add-non-null sequence).
   - Drop the `pod_one_default_per_workspace_when_active` constraint.
   - Drop the `pod_unique_name_per_workspace_when_active` constraint.
2. **Stale-data wipe** (separate migration or RunPython step):
   - **NULL out every `issues.assigned_pod_id`** first. `Issue.assigned_pod` is `on_delete=PROTECT`, so without this step the next step fails on FK violation. Issues without an assigned pod re-resolve to their project's default pod via `Issue.save()` next time they're saved (or at run dispatch via the `or Pod.default_for_project_id(...)` fallback).
   - Hard-fail if any `Pod`, `Runner`, or `AgentRun` rows exist after the issue NULL-out. The migration prints exact SQL: `DELETE FROM agent_run; DELETE FROM runner; DELETE FROM pod;` (in that order, FK-respecting). Operator runs it manually and reruns `migrate`. We choose loud failure over silent best-effort backfill because the workspace-default-pod model has no faithful translation into the project-default-pod model.
3. **Schema migration step 2**:
   - Set `Pod.project` to NOT NULL.
   - Add `pod_one_default_per_project_when_active`.
   - Add `pod_unique_name_per_project_when_active`.
4. **Lifecycle removal** (Phase A code change, lands in the same PR):
   - Remove `runner/signals.py:create_default_pod_for_new_workspace` (the `post_save(Workspace)` handler). Without this, the next workspace creation would re-introduce a workspace-level pod and either crash on `Pod.project` NOT NULL or violate the new model.
   - Remove (or rewrite as `ensure_project_pods`) the `runner/management/commands/ensure_workspace_pods.py` command.
5. **Signal hookup**: connect the `post_save(Project)` handler in `runner/apps.py` (the new auto-create-default-pod-for-project, §6.1).
6. **Required backfill** (RunPython step in the same migration that adds the constraints, or a follow-on migration that runs immediately after):
   - For every existing `Project` row that has no active pod, create `Pod(workspace=project.workspace, project=project, name=f"{project.identifier}_pod_1", is_default=True)`.
   - Idempotent (`get_or_create`).
   - This MUST run before the migration completes, otherwise existing projects would be left without pods and the next runner registration would 404.

## 12. Backward compatibility

- **Wire protocol**: the optional `project_id` field on `Hello` is field-only-additive; older daemons / cloud peers that don't send/check it stay compatible. No `WIRE_VERSION` bump.
- **`config.toml`**: adding `project_slug` is a breaking change for runner configs (an old config with no `project_slug` will fail validation). Acceptable per Q2 (no users yet).
- **Internal Python helpers**: `Pod.default_for_workspace_*` is removed. Any caller that imports it will fail at import time, surfacing every site that needs to switch to `default_for_project_*`. Preferred over silent semantic shift.

## 13. Out of scope (deferred to follow-up designs)

- **Routing rules for non-default pods.** Two non-default pods exist in a project; how does an issue choose between them? Possibilities: per-issue `target_pod_id`, project-level routing config (e.g. by branch pattern), runner capability tags. Punted because no concrete use case yet.
- **Pod-level approval policy.** Today `approval_policy` is per-runner. Lifting it to per-pod is a separate concern.
- **Default pod transfer.** Promoting a non-default pod to default (and demoting the current one) is a multi-step transactional flow we're not building yet.
- **Cross-workspace runners.** Still parent-design Q7.
- **Pod-level capacity / quota.** Out of scope.
- **TUI / CLI multi-runner visibility.** Tracked separately (see `./tasks.md` Phase D).

## 14. Open question

> Do we need a `Project.default_pod_id` denorm column for fast lookup?

`Pod.default_for_project_id(project_id)` is a single indexed query (`WHERE project_id=? AND is_default=true`). With proper index it's O(1). A denorm column saves one join in the dispatch hot path but introduces drift risk (signal must keep it in sync).

**Proposal**: don't add the denorm yet. Re-evaluate if dispatch profiling shows it.

---

See `./decisions.md` for the closed Q&A and `./tasks.md` for the implementation checklist.
