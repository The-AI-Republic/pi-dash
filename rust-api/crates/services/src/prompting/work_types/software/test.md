---
key: software.test
title: Testing software
customizable: overridable
---
### Software

Test the branch synced with its base: `git fetch origin`, check out the {{ repo.code_review_term }} branch, `git merge origin/<base>` (the {{ repo.code_review_term }}'s base, else `{{ repo.base_branch or "the default branch" }}`), run the repository's checks, and push the merge if it is not a no-op. Merge, never rebase, never force-push. If the merge cannot be resolved with confidence, abort it and send the issue back to In Progress saying what changed upstream. If the base has moved since an earlier verdict, test again.

The repository's checks are discovered, never assumed: run what the README, CI config and package manifests declare; if you cannot determine them, say so. Green CI supports a verdict but is not one.

Act as the consumer: a program's caller makes real calls; a person at a screen drives the changed flow (if you cannot boot or drive it here, finish `blocked`); an operator runs the procedure, dry-run first; a reader of a design document reads it cold and checks it can be acted on without its author. Run software where it can be used: locally, in an ephemeral environment, or on a preview deploy.

Push only a trivial fix to the branch, after re-running the checks; anything else goes back as a defect. Waiting on CI or on a merge is `waiting_on_external`.
