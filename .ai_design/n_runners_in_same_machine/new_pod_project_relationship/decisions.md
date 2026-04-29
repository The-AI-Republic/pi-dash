# Decisions — Pod ↔ Project ↔ Runner

Closed questions for the refactor in `./design.md`. Answers locked; rationale preserved so a future reader doesn't re-litigate.

---

## Q1 — Where does a pod sit in the hierarchy?

**Question**: today `Pod` is workspace-scoped (one default per workspace). With multiple projects per workspace, every project's issues land in the same pod, so any runner can be assigned any project's work — wrong if the runner doesn't have that project's repo cloned. Should pods stay workspace-level, or move to project-level?

**Decision**: **project-level.** Each pod has a non-null `Pod.project` FK. Each project has at least one pod (its default, auto-created on project save). Multiple pods per project allowed for tier / region / branch separation.

**Why not workspace-level**: in a multi-project workspace, workspace pods can't faithfully represent "which runners can serve which project." The cloud would have to encode that mapping somewhere else (per-runner project allowlist, per-issue routing rule, etc.) — net more state than just moving the pod boundary down.

**Why not "pod is just a synonym for project"**: pods stay distinct from projects so a project can have multiple tier groupings (e.g. a `WEB_main` default pod plus a `WEB_beefy` pod for long-running runs) without growing project itself. The relationship is N : 1 (pod : project), explicitly preserved by removing the unique-per-project constraint on pods.

**Affects**: design.md §5.1 (schema), §6.1 (auto-create), §11 (migration).

---

## Q2 — What happens to existing Pod / Runner / AgentRun / Issue rows?

**Question**: today's local-dev DB has workspace-scoped pods, runners attached to them, and issues with `assigned_pod` PROTECT FKs to those pods. After the refactor, `Pod.project` is NOT NULL. Migrate or wipe?

**Decision**: **wipe pod / runner / agent_run; NULL `issues.assigned_pod_id` first; backfill default pods for every existing Project**.

The reason this is more complex than just "wipe" is that `Issue.assigned_pod` is `on_delete=PROTECT` (apps/api/pi_dash/db/models/issue.py:183). A naive `DELETE FROM pod` errors on FK violation as long as any issue references a pod. So the order is:

1. `UPDATE issues SET assigned_pod_id = NULL` (or rewrite to project default — see below).
2. `DELETE FROM agent_run; DELETE FROM runner; DELETE FROM pod;` (FK-respecting order).
3. Run schema migrations to add `Pod.project` NOT NULL + new constraints.
4. **Required (not optional) backfill**: `Pod.objects.create(...)` for every Project. The `post_save(Project)` signal only fires on create, so existing projects would be left potless and the next registration / dispatch would 404. The original doc had this as "optional, if any project exists without a pod" — that was a mistake.

**Why NULL the issues' `assigned_pod_id` rather than rewriting them to the project default**: simplest correct path. After the migration, `Issue.save()` re-resolves NULL `assigned_pod` to the project's default pod the next time the issue is saved, and the dispatch fallback (`issue.assigned_pod or Pod.default_for_project_id(...)`) covers the in-flight case. Pre-rewriting them at migration time would be ~equivalent in outcome but harder to verify atomically.

**Why not best-effort full data migration**: the workspace-default-pod model has no faithful translation into the project-default-pod model. A runner that worked on issues across multiple projects has no canonical `project` to backfill into. Silent guesses (e.g. "use the first project the runner ever ran for") would corrupt routing in subtle ways. Loud failure with explicit operator action is cheaper than triaging the corruption.

**Affects**: design.md §5.5 (Issue auto-resolution change), §8 (dispatch), §11 (migration steps).

---

## Q3 — Pod naming convention

**Question**: pod names are user-visible (in the cloud UI, in CLI verbs, in `config.toml`). What's the format? How do auto-created pods name themselves? Does the user control the name?

**Decision**: **`{project.identifier}_pod_<n>` for auto-created (default) pods; `{project.identifier}_{user_suffix}` for user-created pods.** The `{project.identifier}_` prefix is mandatory and server-enforced. User-supplied suffixes can't match `pod_\d+` (reserved for auto-generation).

Default pod for project `firstdream-WEB` → `firstdream-WEB_pod_1`. User adds a beefy tier → `firstdream-WEB_beefy`. User renames it later → `firstdream-WEB_us_east` (still has the prefix).

**Why prefix is mandatory**: pod names appear in CLI args (`pidash token add-runner --pod ...`). The prefix makes a name globally unique across the workspace and immediately identifies the project — no need to also pass `--project` to disambiguate. Also makes db-side debug queries readable.

**Why reserve `pod_\d+` for auto-gen**: avoids the "user named their pod `pod_1` and now it collides with the auto-created default" edge case.

**Affects**: design.md §6.3.

---

## Q4 — Cross-project work-stealing

**Question**: machine A has runners for projects P and Q. Project P's queue is empty; project Q has work piled up. Should A's idle P-runner be allowed to pick up Q work?

**Decision**: **never.** A runner is strictly bound to one project (via its pod). It only ever picks up that project's work. A pod is a sub-grouping of runners under the same project — pods are not interchangeable.

**Why not capability-tag-based stealing**: it would require the runner to have _all_ projects' repos cloned (or a way to clone on demand) plus a tag/policy system to decide when stealing is allowed. Massive complexity uplift for a payoff that's a load-balancing heuristic. If users genuinely want a "shared agent" runner, that's a follow-up design with explicit project allowlists.

**Affects**: design.md §3 (non-goals), §8 (dispatch).

---

## Q5 — Why isn't `--project` optional when the workspace has only one project?

**Question (raised in original D5)**: when the workspace has exactly one project, requiring `--project` on every CLI invocation is friction. Why not auto-pick?

**Decision**: **`--project` is always required, no auto-pick.** Single-project installs pay the small ergonomic tax in exchange for explicit, unambiguous CLI semantics that don't change as the workspace grows.

**Why not auto-pick**: a user's first CLI invocation is also when they're learning the model. Auto-magic that "just works" today and breaks the day they add a second project is the worst of both worlds. We surface the project as a first-class concept from day one.

**The practical mitigation**: `pidash token list-projects` lists the workspace's projects with their identifiers in one call. The user copies one and pastes it.

**Affects**: design.md §9.1, §9.2, §9.3.

---

## Q6 — Per-machine constraint: same project, different runners?

**Question**: machine A has two checkouts of project P (`/home/u/p-main`, `/home/u/p-pr-preview`). Can both checkouts have their own runner under the same MachineToken? Or does one machine = one runner per project?

**Decision**: **multiple runners per project per machine are allowed**, _as long as their `working_dir` values are disjoint_. The only per-machine uniqueness constraint is `working_dir` (already enforced by `Config::validate()`). Two runners on machine A both serving project P, with different working_dirs, both end up in P's default pod and either may pick up work.

**Why not enforce one-runner-per-project-per-machine**: the multi-checkout case is real (PR preview, hot-fix branch on the side, perf comparison clones). Forbidding it would force users into awkward workarounds (one runner per machine and they manually swap branches between assignments). The cloud doesn't care — it sees N runners under a token, all in P's pod, dispatch picks one.

**Affects**: design.md §10 (validation rules), §7.3 (RunnerConfig validation).

---

## Q7 — How do non-default pods get work?

**Question**: we're allowing multiple pods per project. The default pod auto-receives new issues. How does a non-default pod (e.g. `WEB_beefy`) ever get assigned anything?

**Decision**: **deferred.** This refactor lands the schema, the auto-default pod, dispatch-to-default, and the ability to register runners into non-default pods. It does not land routing rules that send specific issues to specific non-default pods.

Possible future approaches (not committing to one):

- Per-issue `target_pod_id` field, set by the user before transitioning to "In Progress."
- Project-level routing config (e.g. issues with label `tier:perf` → `WEB_beefy`).
- Capability tags on pods + required tags on issues, matched at dispatch.
- Manual reassignment from the UI (pick up from default, drag to beefy).

Each has different implications for the issue-create form, the orchestration code, and the matcher. We pick one when there's a real use case.

**Until then**: non-default pods exist, have runners, look healthy in the UI, but receive no automatic work. That's an acceptable intermediate state — it surfaces the data model without committing to a routing decision prematurely.

**Affects**: design.md §13.

---

## Q8 — Should `Runner.project_id` be a denormalised column?

**Question**: `runner.project` is derived from `runner.pod.project`. Adding a direct FK avoids the join in hot paths. Worth it?

**Decision**: **no, derive via property.** Hot-path queries that need the project filter on `pod__project_id=…` (one indexed join). Profiling has not shown this to be a bottleneck and the denorm risks drift (a runner's pod is reassigned but the project FK isn't updated, etc.). Re-evaluate if dispatch traces show pod join cost dominating.

**Affects**: design.md §5.2 (Runner model), §14 (open questions).

---

## Q9 — Pod-level approval policy / agent config / etc?

**Question**: if pods are the routing unit, should approval policy / agent kind / model defaults live on the pod instead of the runner? That would let "all my beefy pod runners" share one policy.

**Decision**: **no, keep them per-runner.** Pods are runner _groupings_, not configuration containers. A runner's behaviour is set at registration and changes via `pidash configure` flags or cloud `ConfigPush` — both of those land on the runner row, not the pod.

**Why not pod-level**: it's a different problem (config inheritance / templating) that complicates an already significant refactor. Punted; if the pattern emerges, a pod-level config layer can be added with each-runner-overrides-pod semantics, without changing today's schema.

**Affects**: design.md §3 (non-goals).

---

## Q10 — Single-project workspace fallback in `Runner.save()` / `AgentRun.save()`

**Question**: the design specifies that pod is always set explicitly at runner registration (§7.3) and that there is no workspace-wide pod (§3 non-goal). After the refactor, direct-ORM callers (tests, management commands, fixtures) that create runners or agent runs without supplying a pod hit a NOT NULL violation. Do we keep production code strict and update every direct-ORM caller, or accept a narrow fallback?

**Decision**: **narrow fallback in `Runner.save()` and `AgentRun.save()`** — when `pod_id` is omitted _and_ the workspace has exactly one project, auto-resolve to that project's default pod. Multi-project workspaces continue to require explicit pod selection.

This does **not** reintroduce the workspace-wide pod (which would require allowing a pod to exist outside any project). The fallback always resolves to a project-scoped pod; it just disambiguates the "which project" question for the special case where the answer is unique.

**Why this is acceptable**:

- Production registration paths (`/api/v1/runner/register/`, `/api/v1/runner/register-under-token/`) already supply the pod explicitly. The fallback never fires in production traffic.
- The fallback fires only for direct-ORM callers (existing test fixtures, management commands), and only in single-project workspaces. The strict-design behavior — fail-fast on missing pod — would have required rewriting ~30 test files to thread a `project` fixture through every `Runner.objects.create(workspace=...)` call site.
- Multi-project workspaces (the case the refactor exists to support) get the strict behavior: `pod_id is None` with multiple projects raises a NOT NULL violation, so the operator is forced to pick a project explicitly.
- The same fallback lives in `services/validation.py:_resolve_pod` so the runs-create REST endpoint behaves identically for callers that don't yet pass a `work_item_id`.

**Where the fallback lives**:

- `apps/api/pi_dash/runner/models.py:Runner.save` — runner creation.
- `apps/api/pi_dash/runner/models.py:AgentRun.save` — agent-run creation when no `work_item` is set.
- `apps/api/pi_dash/runner/services/validation.py:_resolve_pod` — REST run-creation path.

**Affects**: design.md §3 (the strict reading is the spec; the fallback is a documented compromise). If a follow-up tightens this — by rewriting all direct-ORM callers and dropping the fallback — that's a clean change with no schema implication.
