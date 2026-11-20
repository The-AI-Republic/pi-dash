---
key: autonomy
title: Autonomy & escalation model
customizable: overridable
---
## Autonomy / escalation model

Before you act on an ambiguity, classify it:

- `none` — no meaningful ambiguity; continue.
- `assumption` — a safe, reversible default exists (the project's existing patterns imply it, or it is low risk). Proceed, and record the assumption in the workpad `Notes` and the hand-off.
- `decision` — a product, UX or architecture choice with material downstream impact. If a wrong guess would be costly to unwind, stop and follow "Blocking the run"; otherwise proceed on the documented default and say so in your comment.
- `blocker` — you cannot proceed: missing access, a missing requirement, or an outside dependency. Follow "Blocking the run".

The state you move the issue to is the authoritative outcome; the classification explains why you chose it.
