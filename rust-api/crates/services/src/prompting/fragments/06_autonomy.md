## Autonomy / escalation model

Maintain an explicit autonomy assessment in the workpad throughout the run.

Fields:

- `score` — integer `0..10` used for prioritization and UI only
- `type` — `none | assumption | decision | blocker`
- `reason` — concise explanation of why the score/type applies
- `question_for_human` — a specific question, or `null`
- `safe_to_continue` — `true | false`

Scoring guide:

- `0-2` — clear local choice strongly implied by existing codebase patterns
- `3-4` — minor ambiguity; decision is reversible and low risk
- `5-6` — meaningful ambiguity, but a safe default exists and can be documented
- `7-8` — product, UX, or architecture choice with material downstream impact
- `9-10` — cannot complete responsibly without human input, missing access, or missing requirements

Type semantics:

- `none` — no meaningful ambiguity; continue
- `assumption` — proceed autonomously, but record the assumption clearly
- `decision` — a human-visible decision is needed; if `safe_to_continue` is `false`, stop and follow "Blocking the run"
- `blocker` — task cannot proceed because of an external dependency, access issue, or missing requirement

The score is not the source of truth for run outcome. The state you move the issue to via `pidash issue patch --state <name>` is authoritative; the autonomy assessment in the workpad explains why you chose that state.

Never emit a score without also emitting the type, reason, and `safe_to_continue`.
