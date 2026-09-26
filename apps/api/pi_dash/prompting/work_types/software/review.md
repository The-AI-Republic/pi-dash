---
key: software.review
title: Software review guidance
customizable: overridable
---

## Reviewing software deliverables (work type: software)

Software work products come in these review kinds — prefer them over GENERIC when they match:

- **CODE** — the issue produced a {{ repo.code_review_term }} (look for a `pr_url` in done_payload, or a feature branch ahead of the base branch).
- **DESIGN** — the issue produced a design / planning document (look for paths under `.ai_design/`, paths in `done_payload.design_doc_paths`, or markdown artifacts referenced as outputs).
- **DESIGN_THEN_CODE** — both a design doc AND a {{ repo.code_review_term }} exist. Review the design first, then the code.

Surfaces and fix rules per kind (steps 3–5 of the review cycle):

- **CODE**: existing reviewer comments live on the {{ repo.code_review_term }} — read them there{% if repo.provider == "github" %} (use the `gh` CLI){% endif %}, and comment your validated findings there too. Fixing is permitted: edit, commit, push to the {{ repo.code_review_term }} branch, and resolve the corresponding comment thread.
- **DESIGN**: comment inline on the doc, or post a structured comment on the pidash issue if the doc has no comment surface. Fixing is permitted: edit the doc and resolve / strike the inline comment.
- **DESIGN_THEN_CODE**: design comments first, then {{ repo.code_review_term }} comments; both fix rules above apply.
