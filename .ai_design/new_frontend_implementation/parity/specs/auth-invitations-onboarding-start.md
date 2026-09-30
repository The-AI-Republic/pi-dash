# Area spec: auth invitations + onboarding start (AUTH-026–033)

Learned by reading the old frontend (routes, onboarding components, service
call sites listed in `parity/inventory/auth.md`). No old code copied; this
prose is the only thing the new implementation and the oracle scenarios
build from.

## Invitation inbox — `/invitations` (AUTH-026)

- Authenticated-only page. Shows one selectable card per pending workspace
  invite: workspace logo/name plus the invited role label.
- Clicking a card toggles its selected state (selected cards are visually
  highlighted with a check mark). One primary button accepts and joins all
  selected invites at once; it is disabled while nothing is selected or
  while the join is in flight.
- A secondary "go home" action leaves without joining anything.
- Accepting joins every selected workspace, stamps the first joined
  workspace as the user's last workspace, refreshes the workspace list,
  and lands inside that first workspace.
- With zero pending invites the page shows an illustrated empty state
  ("no pending invites" + explanatory line) with a single action back home.

## Single-invitation link — `/workspace-invitations` (AUTH-027)

- Public page driven by three query params: invitation id, workspace slug,
  token. It fetches the single invitation over
  `GET /api/workspaces/{slug}/invitations/{id}/join/`.
- Pending invite: card naming the workspace with two actions, accept and
  ignore. Accept posts `{accepted: true, token}`; ignore posts
  `{accepted: false, token}` (ignore needs the token present).
- Stale states replace the actions entirely:
  - already answered + accepted → "already a member" card with a
    continue-home action. Reachable only when the invite was answered
    before any account existed for its address: accepting with a matching
    account deletes the invitation row, so revisiting that link renders
    the inactive card instead;
  - already answered + declined, or a fetch error → "link no longer
    active" card with home/sign-in/community actions;
  - unresolvable invitation id → the inactive card too (the "INVITATION
    NOT FOUND" branch needs fetched detail plus a fetch error at once,
    which a fresh broken link never produces, so it is effectively dead).
- The invite model has no expiry field, so there is no distinct
  "expired" rendering; declined and broken links share the inactive card.

## Post-invitation landing (AUTH-028)

- Accepting a single invite lands in the joined workspace when the signed-in
  address matches the invitee address, and on the home route otherwise.
- Ignoring always lands on the home route.
- Batch-accepting from the inbox lands in the first joined workspace.

## Onboarding gate and resume — `/onboarding` (AUTH-029)

- Single-URL flow. Signed-out visitors bounce to sign-in with a return path;
  finished users (onboarded flag, or all four progress flags set) bounce
  away to the workspace/home route.
- A refresh resumes mid-flow from stored profile progress flags: a user
  whose profile step is stored but who has not created/joined a workspace
  resumes at the workspace step; a user who created but has not invited
  resumes at the invite step. A pending workspace join request parks the
  user on a holding view.
- Step order on this checkout (not self-managed): CLI install → profile →
  role → use case → workspace create-or-join → invite members. The header
  shows a progress bar (current/total) and the account menu.
- Self-managed instances skip the role and use-case steps.

## CLI-install step (AUTH-030)

- First step. Shows per-OS install guidance in tabs ("macOS / Linux" with
  one shell command, "Windows" with two), preselecting the tab matching the
  browser OS but leaving it switchable. Each command block has a copy
  action with copied confirmation and a manual-copy fallback message.
- Nothing is stored. Both "skip for now" and "done, continue" advance to
  the profile step identically.

## Profile-setup step (AUTH-031)

- Collects the display name (required, validated as a person name, max 50
  chars) with an inline error; optional avatar upload with change/remove.
- Accounts that never set a password (provider-created) additionally get a
  password + confirmation pair with strength and match enforcement; other
  accounts do not see it.
- On non-self-managed instances a marketing-consent control is shown
  (default on). Submit writes the user row (name, avatar), sets the
  password where offered, stores consent on the profile, and advances
  (to role setup, or straight to workspace on self-managed).

## Role-selection step (AUTH-032)

- Fixed list of seven roles (product/engineering managers, designer,
  developer, founder/executive, operations, others). Exactly one is
  required: submit stays disabled until a choice is made, and submitting
  empty shows a required-field message. Saving stores the role id on the
  profile with a success toast; skip advances without storing.
- Skipped entirely on self-managed instances (the parity stack reports
  `is_self_managed`, so the oracle proves the skip there; the
  choose-and-require UI is cloud-edition-only and uncovered until a
  non-self-managed stack exists).

## Use-case step (AUTH-033)

- Shared list of five use cases, multi-select with checkboxes. At least one
  is required with the same disabled-until-chosen + inline-message behavior
  as the role step. The selection is stored on the profile as one combined
  value (individual choices joined with a separator); skip advances without
  storing.
- Skipped entirely on self-managed instances (same oracle caveat as the
  role step: skip proven here, full UI needs a non-self-managed stack).

## Server contracts used by the oracle scenarios

- `GET /api/users/me/workspaces/invitations/` — my pending invites.
- `POST /api/users/me/workspaces/invitations/` `{invitations: [ids]}` —
  batch accept.
- `POST /api/workspaces/{slug}/invitations/` `{emails: [{email, role}]}` —
  create invites (roles: 20 admin, 15 member, 5 guest).
- `GET` + `POST /api/workspaces/{slug}/invitations/{id}/join/`
  (`{accepted, token}`) — single-invitation fetch/answer.
- `GET /api/workspaces/{slug}/members/` — membership rows.
- `PATCH /api/users/me/` (name/avatar), `PATCH /api/users/me/profile/`
  (role, use_case, consent, progress flags), `GET /api/users/me/profile/`
  (read back), `PATCH /api/users/me/onboard/` `{is_onboarded: true}`.
- Native `POST /auth/sign-up/` (email + password + CSRF) mints a fresh,
  non-onboarded user for onboarding scenarios.
