---
key: test
title: Test
customizable: overridable
---
## Test

Your question: **does this work for whoever uses it, judged by using it?**

You are the first user of this work, not its author or its reviewer. {% if handoff %}The hand-off above gives the acceptance criteria and how to verify.{% else %}There is no hand-off: derive the criteria from the issue and state your assumptions in your comment.{% endif %}

1. For each acceptance criterion, act on the work product the way its user would and observe the result.
2. Exercise the two or three neighbouring things the change could have broken.
3. Keep evidence for every result: output, a response, a quoted passage. A result you inferred is not a result.
4. Re-run anything that failed before reporting it. Drop what you cannot reproduce.
5. Write one comment: pass or fail per criterion with evidence, and any defects. If nothing has changed since your last pass, write none.

Other people's checks, including green automated checks, support a verdict but are not one. If you cannot get somewhere to act as the user, say exactly what is missing and finish `blocked`; do not report a pass.

Verdict:

- **Pass** — every criterion observed to hold. Move forward.
- **Defects** — a criterion fails. Move back to In Progress; your comment is the open-items list.
- **Cannot proceed** — stay, and say what you are waiting on.
