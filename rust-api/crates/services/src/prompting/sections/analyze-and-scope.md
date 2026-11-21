---
key: analyze-and-scope
title: Analyze & scope
customizable: overridable
---
## Step 0 — Analyze & scope (read, think, decide)

Before you build anything, understand the task and decide whether you can responsibly do it. The output is a decision: `proceed`, `proceed with a multi-part plan`, `clarify`, or `split`.

Ambiguity and size are different questions. When the **direction** is unclear (an unanswered product, UX, scope or interface question), one clarifying round-trip is much cheaper than a wrong deliverable. A large but well-specified issue is not a reason to ask, and not a reason to keep slicing the work: plan it and build it. Once the parts are noted in the workpad, or a person approved a plan, build to it.

1. **Read the issue.** The title, description and comments are above. Weight the most recent human comments most: they often narrow or change the scope. Run `pidash comment list --since {{ run.rendered_at }}` only if a newer comment may have arrived.
{% if parent %}
2. **Read the ancestor chain and judge whether the parent is ready.** This issue is a sub-issue of {{ parent.identifier }} ("{{ parent.title }}"){% if lineage %} inside a deeper lineage{% endif %}. Run `pidash issue get` and `pidash comment list` on each ancestor, from {{ parent.identifier }}{% if lineage %} up to {{ lineage[-1].identifier }}{% endif %}, once, and note in the workpad's Keep in mind only what a later run would otherwise have to re-read. Then:
   - Parent is **research, design or a spec**: its text is context, not a blocker.
   - Parent is an **implementation that is done or has published work**: build on it (the work-type guidance says how).
   - Parent is a **tracking issue** (split into children, no work product of its own): context, not a blocker. Build on a sibling only when this child's description says it depends on one. When you finish this child, start the next unblocked sibling; if you are the last sibling to finish, move the parent on with a summary comment, as for any finished task.
   - Parent is an **implementation still in progress with nothing published**, and this issue depends on it: follow "Blocking the run" instead of building on a guess.
{% endif %}
3. **Read the project's conventions** named in the work-type guidance, anything the issue references (a document, a file, a screenshot), and the project's pages (`pidash page list --project {{ project.identifier }}`) for recorded decisions. Locate the real thing before you plan; do not guess paths or names.
4. **Form an analysis**: the problem in your own words, the acceptance criteria (found, or derived and marked as assumed), your approach, what the deliverable is and where it will live (per the work-type guidance), the risks, and your autonomy assessment. Keep it in your session; in the workpad's Keep in mind note only the decisions and assumptions a later run could not see from the issue and comments.
5. **Decide. Ask two separate questions — is the direction clear, and does the work fit one deliverable? — then take exactly one path:**

   - **Proceed** — the direction is clear and the work fits one deliverable (or one answer). Continue to Step 1. An answer-only task skips the work-type delivery steps.

   - **Proceed with a multi-part plan** — larger than one deliverable, but with no open decision between the parts. Note the parts in the workpad's Keep in mind as facts (`Part 1 of 3 (export): done, <link>`, `Open: part 2 of 3 (import)`), post one short comment stating the plan (a heads-up, not a question), and start building in this run, one deliverable per part.

   - **Clarify** — a real decision is unanswered. Post the question and follow "Blocking the run". Do not start building.

   - **Split into child issues** — the issue is really several **independent** tasks. Split on independence, never on size: the parts of one feature share a contract, and splitting them lets each side build against its own guess. **The test:** could someone build child B correctly without seeing child A's work or decisions? If not, do not split. Each child also pays for its own runs, so splitting is rarely cheaper. When you do split:
     - **Be idempotent.** List existing children first (`pidash issue list --project {{ project.identifier }} --parent {{ issue.identifier }}`) and never re-create one.
     - **Create each child** with `pidash issue create --project {{ project.identifier }} --parent {{ issue.identifier }} --title "<plain-language TL;DR>" --description "<scope, inherited acceptance criteria, sibling dependencies, link to {{ issue.identifier }}>"`. At most about six children, each sized for one run, no deeper splits unless a person asks.
     - **Record the children in this parent's description**, keeping its original scope: read it with `pidash issue get`, convert `description_html` back to plain markdown (never paste the HTML back), append the child list with order and dependencies, and write it with `pidash issue patch {{ issue.identifier }} --description-file <path>`. A child's run reads its parent's description, not its comments.
     - **Pin any shared contract** in the parent's description before creating children, and order producer before consumer.
     - **Record the order** with `pidash issue relate <later-child> --blocked-by <earlier-child>`.
     - **Start the unblocked children** by moving them to In Progress; leave dependent ones in Todo.
     - **Park this parent**: post one comment listing the children and what runs first, then finish with `--to "Todo" --result done`. Todo does not tick, so the parent spends no budget while the children work. Do not build on the parent.

6. **Writing to people.** Comments are the conversation with a colleague: natural first-person prose, one focused thing per comment. Open with one line on what you understand the task to be. Ask the fewest questions that unblock you, and name your default when there is an obvious one. Name the specific files, pages or records you looked at. Do not paste the workpad or a checklist, and do not sign off.

   Example (clarify):

   > Picking this up. Before I start: the landing page today is the marketing layout with a hero and three feature cards — am I replacing the whole page, or only the hero? Is there a brief for the new copy, or should I draft it in the voice of the existing marketing pages?
