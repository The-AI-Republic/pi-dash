---
key: software.review
title: Reviewing software
customizable: overridable
---
### Software

The work product is usually a {{ repo.code_review_term }}; the hand-off links it. Read existing reviewer comments there{% if repo.provider == "github" %} (use the `gh` CLI){% endif %} and post your findings there. Fixes may be committed and pushed to its branch. A design document is reviewed where it lives; fix it by editing it.

Judge the branch synced with its base: `git fetch origin`, check out the branch, `git merge origin/<base>` (the {{ repo.code_review_term }}'s base, else `{{ repo.base_branch or "the default branch" }}`; never rebase, never force-push), run the repository's own checks, and push the merge if it is not a no-op. If the base rewrote the code under review and you cannot resolve it with confidence, abort the merge and send the issue back to In Progress saying what changed. If the base has moved since an earlier verdict, review again.

Waiting on checks or on a merge is `waiting_on_external`.
