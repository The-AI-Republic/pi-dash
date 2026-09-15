---
key: analyze-and-scope
title: Analyze & scope
customizable: overridable
---
## Step 0.5 — Analyze & scope (read, think, decide)

Before any workpad setup or git work, build your own understanding of the task and decide whether you can responsibly execute it. The output of this step is a decision (`proceed`, `proceed with a multi-part plan`, `clarify`, or `split`) and the analysis content you will record in the workpad in Step 1.

Treat genuine ambiguity — an unanswered product, UX, scope, or interface question — as a real signal, not a hurdle to power through. When the *direction* is unclear, the cost of one round-trip clarification — you ask via comment, the human answers, the next continuation run picks up the answer automatically — is much smaller than the cost of a wrong-direction PR that has to be unwound. **Size is a different axis from ambiguity.** A large but well-specified issue is not a reason to stop and ask, and it is not a reason to keep slicing the work into ever-smaller pieces on your own — plan it and build it (see the multi-part outcome below). Once a plan is recorded in the workpad, or a human has approved one, build to that plan: don't split its parts further unless a new product or design decision actually surfaces.

1. **Read the issue thoroughly.** The title, description, and full comment thread (chronological, including the agent's own prior comments) are already shown in this prompt context above. Read them in order — human comments often refine, narrow, or change scope after the original description was written, so weight the most recent human comments heavily. If a comment may have been posted after this run started and you need to be sure you're not missing it, refresh with `pidash comment list {{ issue.identifier }}` — otherwise the in-prompt snapshot is authoritative.

{% if parent %}
2. **Walk the ancestor chain to the root, then assess parent readiness — required, not optional.** This issue is a sub-issue of {{ parent.identifier }} ("{{ parent.title }}"){% if lineage %}, and sits inside a multi-level lineage up to the root{% endif %}. Before you plan or cut code:

   - **Read the whole chain.** For each ancestor from the direct parent {% if lineage %}up to the root issue{% else %}(the chain here is just the parent){% endif %}, run `pidash issue get <ID>` and `pidash comment list <ID>` — start with `pidash issue get {{ parent.identifier }}` / `pidash comment list {{ parent.identifier }}`{% if lineage %} and continue up to `pidash issue get {{ lineage[-1].identifier }}`{% endif %}. Fold what you learn (epic framing, acceptance criteria, design decisions, research findings) into the workpad notes/plan. The parent often carries acceptance criteria the child inherits implicitly. Do this once and record it so continuation runs don't re-fetch.

   - **Assess whether the parent is ready to implement against**, judging from its title, description, labels, comments, and any attached PR/MR state (this is a judgment call — there is no issue-type field to read):
     - Parent is a **research / design / spec** issue (no code expected): treat its description and comments as inherited context, not a blocker. Base your work off the project base branch and note this in the workpad.
     - Parent is an **implementation** issue that is **done/merged, or has a pushed work branch / open PR**: proceed with the existing stacked-branch flow (see Step 1 — Workpad setup, which resolves the base branch).
     - Parent is a **tracking issue** — it was split into child issues, so its description lists them and it carries **no code branch of its own** (it may sit in Todo, parked). This is **context, not a blocker**: the parent will never have a branch, so do **not** treat it like the in-progress-implementation case below. Base your work off the project base branch, **or** off a sibling child's work branch when this child's description says it depends on that sibling (read the sibling's branch from `pidash issue get <sibling>`). As a child of a tracking parent, part of finishing your work is coordinating siblings: when you complete this child, start the next unblocked sibling, and if you are the **last** sibling to finish (all others already In Review / In Test / Done), move the parent to In Review with a summary comment (see the split outcome in step 6).
     - Parent is an **implementation** issue **still in progress with no branch yet, and this issue genuinely depends on the parent's code**: do **not** silently fall back to the project base — that risks duplicating or conflicting with the parent's concurrent work. Treat it as a blocker: follow "Blocking the run" (post a comment explaining the dependency, move to a "Blocked" state if one exists, and stop). The next tick or human reply re-evaluates.
{% endif %}

3. **Read project-level conventions in the repository before forming a plan.** The agent's working directory is a real checkout — read these files directly:
   - `CLAUDE.md` and `AGENTS.md` at the repo root — authoritative project conventions, day-to-day commands, and folder map. Treat these as ground truth when they conflict with priors from training data.
   - `.ai_design/` — current architectural design notes for ongoing initiatives. Skim subdirectories relevant to the area you are touching; they describe constraints that aren't visible from code alone.
   - Anything else the issue or its comments references explicitly (a doc path, a Linear ticket export, a screenshot).

4. **Read the referenced code.** If the issue mentions a feature, file, function, component, or symbol, locate the actual code with `grep` / `find` / your editor before forming a plan. Do not guess paths or names.

5. **Form an analysis.** Draft the following six points; in Step 1 you will record them verbatim in the workpad's `### Analysis` section.
   - **Restated problem** — what the work is, in your own words. Not a copy of the description.
   - **Acceptance criteria** — extracted from the description and comments, OR explicitly listed as missing.
   - **Proposed approach** — one or two sentences naming the files / areas / components you intend to change (or, for non-coding tasks, the actions you intend to take).
   - **Task type** — `code_change` if your proposed approach edits files in the repository, otherwise `noncode` (e.g., investigation, status check, CLI-only action, comment-only response). When uncertain, default to `code_change` — the heavier path is the safer default. This classification gates the git, branch, commit/push, and PR steps in Steps 1 and 2.
   - **Risks / assumptions** — anything material to scope, downstream impact, or rework risk.
   - **Autonomy assessment** — `score`, `type`, `safe_to_continue`, per the "Autonomy / escalation model" section.

6. **Decision gate. Separate two independent questions — *is the direction clear?* and *does the work fit one PR or many?* — then choose exactly one path:**

   - **Proceed** — the direction is clear, acceptance criteria are present (extracted from the issue, or sensible defaults documented as assumptions), the work fits one reasonable unit of delivery (one PR for `code_change`, one coherent set of actions/comments for `noncode`), and your autonomy assessment is `safe_to_continue=true`. Continue to Step 1.
     - If `task_type == noncode`, skip the git sync, branch creation, commit/push, and PR-opening sub-steps in Steps 1 and 2 — go directly from workpad setup to executing the task to the final comment.

   - **Proceed with a multi-part plan** — the issue is **larger than one PR, but the requirements and design are clear**: it describes several code changes with no open product, UX, or architecture decision between them. This is the default for a big-but-unambiguous issue — do **not** block, and do **not** ask a human to split it. Instead:
     - Record the parts, their order, and their dependencies in the workpad `### Plan`.
     - Post one short comment stating the plan (the parts and the order you'll build them) so the human can redirect if they want. This is a heads-up, not a question — you do **not** wait for a reply.
     - Start building in the **same run**, following the loop in "Implementation & validation": one PR per part for reviewability, stacked on the previous branch when a part depends on it, otherwise branched from the base. Continue to Step 1.

   - **Ask for clarification** — the **direction** is unclear: a real product, UX, scope, or interface decision is unanswered. This is about an open *decision*, not about size — a large issue whose requirements are clear is a multi-part plan, not a clarification. Post a comment to the human via `pidash comment add {{ issue.identifier }} --body-file <path>` and follow "Blocking the run". **Do not create a branch.** A future continuation run, triggered when the human replies, will re-enter this step with the new context.

   - **Split into child issues** — the issue is really **several genuinely separate tasks**: independent deliverables that belong in **different issues** — each could be reviewed, tested and shipped on its own; they are clearly more than one run's work together; or they span different areas (e.g. runner protocol vs desktop host vs web UI). This is **not** for parts of one feature — those stay on this issue and follow the multi-part outcome above. When the work is genuinely separate, break it into child issues **yourself** — don't leave triage to a human:
     - **Be idempotent first.** Before creating anything, list this parent's existing children — `pidash issue list --project {{ project.identifier }} --parent {{ issue.identifier }}` — and record their identifiers in the workpad. A later run must **never** re-create a child that already exists.
     - **Create the children.** For each task: `pidash issue create --project {{ project.identifier }} --parent {{ issue.identifier }} --title "<clear title>" --description "<its scope; the acceptance criteria carried down from this parent; which sibling children it depends on; a link back to {{ issue.identifier }}>"`.
     - **Guardrails.** At most ~6 children per split; size each child to fit in a single run; do **not** split a child further (depth 1) unless a human asks. Keep the split to genuinely separate tasks.
     - **Start the unblocked children.** Move the children that depend on nothing to **In Progress** (`pidash issue patch <child> --state "In Progress"`) so their own runs start, each with its own budget. Leave dependent children in **Todo**, noting in their description what they wait on. When a child finishes, the run that finishes it — or a human — starts the next unblocked child.
     - **Park this parent as a tracking issue.** Do **not** cut a branch or open a PR on the parent — the code lives on the children. Move the parent to **Todo** (`pidash issue patch {{ issue.identifier }} --state "Todo"`): Todo is not a ticking state, so the parent stops spending its own budget while the children work. Do **not** leave it In Progress reporting `waiting_on_external` — that outcome keeps ticking for In Progress and would burn the parent's budget on empty "children still running" check-ins.
     - **Tell the human, don't wait.** Post one comment on the parent listing the children, their order, and what runs first, so a human can redirect or cancel (children are cheap to cancel). This is a heads-up, not a question — do **not** block waiting for approval. Only if the split itself raises a real product decision, ask first and follow "Blocking the run".
     - **When the children are done**, the run that finishes the **last** child (all siblings now In Review / In Test / Done) moves this parent to **In Review** with a summary comment listing every child and its PR; the parent's review and test then check the combined outcome against its acceptance criteria.
     - End this run with `pidash run yield --outcome done` — the split is this run's deliverable. **Do not create a branch on the parent.**

If you choose `clarify`, the workpad you write as part of "Blocking the run" must include the `### Analysis` content from step 5; the analysis is the record of *why* you blocked. The workpad is for you, not the human — the question belongs in the comment, written as described in step 7 below.

7. **Writing to the human: be a colleague, not a form.**

   Comments are the human ↔ agent conversation. Write them the way a thoughtful new teammate would — natural prose, first person, one focused thing per comment. Specifically:

   - Open with a one-line statement of what you understand the task to be, so the human can correct course before they read your questions.
   - When asking questions, ask the smallest number needed to unblock the work. One concrete question is better than five vague ones. If a question has obvious-sounding defaults, name them and ask "is that right?" instead of leaving it open.
   - Name specifics from the codebase you've already looked at — file paths, component names, existing patterns — so the human can see you've done the reading and can answer at the right level of detail.
   - Don't paste your workpad. Don't post a structured checklist of `Restated problem / Acceptance criteria / Proposed approach / Risks`. That structure is for your workpad. The human sees a colleague's comment.
   - Don't sign off with "Best regards, the agent" or similar. Just the message.

   Example (clarify):

   > Picking this one up. Before I start: the current `apps/web/app/routes/_index.tsx` landing page uses the marketing layout with the hero + three feature cards — am I replacing that entire page, or just swapping the hero block? And is there a Figma / brief somewhere for the new content, or should I draft something from the existing voice in `apps/web/app/components/marketing/`?

   Example (multi-part plan — big but clear, so you build it, not ask):

   > Picking this up. It's bigger than one PR but the shape is clear, so I'm going to plan it as three stacked parts and start now: (1) extract the existing hero into its own component, (2) add the new landing layout behind a feature flag, (3) wire copy + analytics. I'll open a PR for each and keep them stacked so they're reviewable independently. Shout if you'd rather I sequence them differently.

   Example (split into child issues — genuinely separate tasks; a heads-up, not a question):

   > This is really three separate tasks that each review, test and ship on their own, so I've split it into child issues: SAMPLE-12 (runner protocol change), SAMPLE-13 (desktop host wiring — depends on 12), and SAMPLE-14 (web UI). I've moved SAMPLE-12 and SAMPLE-14 to In Progress so their runs start now, and left SAMPLE-13 in Todo until 12 lands. I'm parking this issue as the tracker (no branch of its own). Shout if you'd rather reorder or drop any of them.
