---
key: software.review
title: Reviewing software
customizable: overridable
---
### Software

The work product is usually a {{ repo.code_review_term }}; the workpad links it. Read its existing reviewer comments{% if repo.provider == "github" %} (`gh`){% endif %} and post your findings there. Small fixes may be pushed to its branch. A design document is reviewed, and fixed, where it lives.

Review the branch synced with its base: `git fetch origin`, check out the branch, `git merge origin/<base>` (the {{ repo.code_review_term }}'s base, else `{{ repo.base_branch or "the default branch" }}`), run the repository's checks, and push the merge if it is not a no-op. Merge, never rebase, never force-push. If the merge cannot be resolved with confidence, abort it and send the issue back to In Progress saying what changed upstream.

Waiting on checks or on a merge is `waiting_on_external`.
