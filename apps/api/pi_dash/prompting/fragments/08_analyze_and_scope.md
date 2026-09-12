## Step 0.5 — Analyze & scope (read, think, decide)

Before any workpad setup or git work, build your own understanding of the task and decide whether you can responsibly execute it. The output of this step is a decision (`proceed` / `clarify` / `split`) and the analysis content you will record in the workpad in Step 1.

Treat ambiguity as a real signal, not a hurdle to power through. The cost of one round-trip clarification — you ask via comment, the human answers, the next continuation run picks up the answer automatically — is much smaller than the cost of a wrong-direction PR that has to be unwound.

1. **Read the issue thoroughly.** The title, description, and full comment thread (chronological, including the agent's own prior comments) are already shown in this prompt context above. Read them in order — human comments often refine, narrow, or change scope after the original description was written, so weight the most recent human comments heavily. If a comment may have been posted after this run started and you need to be sure you're not missing it, refresh with `pidash comment list {{ issue.identifier }}` — otherwise the in-prompt snapshot is authoritative.

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

   - **Ask for clarification** — if the description leaves a meaningful product, UX, scope, or interface question unanswered. Post a comment to the human via `pidash comment add {{ issue.identifier }} --body-file <path>` and follow "Blocking the run". **Do not create a branch.** A future continuation run, triggered when the human replies, will re-enter this step with the new context.

   - **Propose a split** — if the work is too large to land as one reasonable PR, or genuinely covers multiple independent concerns. Post a comment to the human suggesting how you'd break it up and your reasoning. **Do not create child issues yourself** — leave triage to the human. Then follow "Blocking the run". **Do not create a branch.**

If you choose `clarify` or `split`, the workpad you write as part of "Blocking the run" must include the `### Analysis` content from step 5; the analysis is the record of *why* you blocked. The workpad is for you, not the human — the question or split proposal belongs in the comment, written as described in step 7 below.

7. **Writing to the human: be a colleague, not a form.**

   Comments are the human ↔ agent conversation. Write them the way a thoughtful new teammate would — natural prose, first person, one focused thing per comment. Specifically:

   - Open with a one-line statement of what you understand the task to be, so the human can correct course before they read your questions.
   - When asking questions, ask the smallest number needed to unblock the work. One concrete question is better than five vague ones. If a question has obvious-sounding defaults, name them and ask "is that right?" instead of leaving it open.
   - Name specifics from the codebase you've already looked at — file paths, component names, existing patterns — so the human can see you've done the reading and can answer at the right level of detail.
   - Don't paste your workpad. Don't post a structured checklist of `Restated problem / Acceptance criteria / Proposed approach / Risks`. That structure is for your workpad. The human sees a colleague's comment.
   - Don't sign off with "Best regards, the agent" or similar. Just the message.

   Example (clarify):

   > Picking this one up. Before I start: the current `apps/web/app/routes/_index.tsx` landing page uses the marketing layout with the hero + three feature cards — am I replacing that entire page, or just swapping the hero block? And is there a Figma / brief somewhere for the new content, or should I draft something from the existing voice in `apps/web/app/components/marketing/`?

   Example (split proposal):

   > This one's bigger than a single PR — happy to do it, but I'd split it into three so each is reviewable independently: (1) extract the existing hero into its own component, (2) add the new landing layout behind a feature flag, (3) wire copy + analytics. Want me to file (2) and (3) as separate issues so we can prioritize them, or land it as one large PR?
