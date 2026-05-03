## Step 0.5 — Analyze & scope (read, think, decide)

Before any workpad setup or git work, build your own understanding of the task and decide whether you can responsibly execute it. The output of this step is a decision (`proceed` / `clarify` / `split`) and the analysis content you will record in the workpad in Step 1.

Treat ambiguity as a real signal, not a hurdle to power through. The cost of one round-trip clarification — you ask via comment, the human answers, the next continuation run picks up the answer automatically — is much smaller than the cost of a wrong-direction PR that has to be unwound.

1. **Read the issue thoroughly.** The title and description are already in this prompt context. Pull all prior comments via `pidash comment list {{ issue.identifier }}` and read them in chronological order. **The comment list is authoritative — do not assume any prior comment is already in your context.** Every run is a fresh agent session with no memory of prior runs; the issue thread, the workpad comment, and the repo are the only sources of cross-run state. Non-bot comments often refine, narrow, or change scope after the original description was written; weight the most recent human comments heavily.

{% if parent %}
2. **Read the parent issue ({{ parent.identifier }} — "{{ parent.title }}").** This issue belongs to a larger piece of work. Fetch the parent and its comments via `pidash issue get {{ parent.identifier }}` and `pidash comment list {{ parent.identifier }}` to understand the framing this child sits inside. The parent often carries acceptance criteria the child inherits implicitly.
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

6. **Decision gate. Choose exactly one path:**

   - **Proceed** — only if all of the following hold: acceptance criteria are present (extracted from the issue, or sensible defaults documented as assumptions); the work fits one reasonable unit of delivery (one PR for `code_change`, one coherent set of actions/comments for `noncode`); your autonomy assessment is `safe_to_continue=true`. Continue to Step 1.
     - If `task_type == noncode`, skip the git sync, branch creation, commit/push, and PR-opening sub-steps in Steps 1 and 2 — go directly from workpad setup to executing the task to the final comment.

   - **Ask for clarification** — if the description leaves a meaningful product, UX, scope, or interface question unanswered. Post a focused comment via `pidash comment add {{ issue.identifier }} --body-file <path>` containing your restated understanding plus the specific question(s); one focused question is better than a wall of text. Then follow "Blocking the run". **Do not create a branch.** A future continuation run, triggered when the human comments back, will re-enter this step with the new context.

   - **Propose a split** — if the work is too large to land as one reasonable PR, or genuinely covers multiple independent concerns. Post a comment listing the proposed sub-issues (titles + one-line scopes for each) and your reasoning. **Do not create child issues yourself** — leave triage to the human. Then follow "Blocking the run". **Do not create a branch.**

If you choose `clarify` or `split`, the workpad you create as part of "Blocking the run" must include the `### Analysis` content from step 5; the analysis is the record of *why* you blocked.
