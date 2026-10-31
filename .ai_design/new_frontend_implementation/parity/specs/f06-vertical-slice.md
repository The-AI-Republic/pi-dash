# F-06 vertical slice — area spec (NEWFRONT-17)

First slice of `apps/web_new`: shell, sign-in, read-only issue list. Written
from the behavior learned by reading the old sign-in card and the Django
auth/issue views — no old code, strings, styles or assets reused. New code
is written from this spec.

## Shell

- Every workspace page renders inside one chrome: a title bar on top (brand
  and workspace crumb left, command-palette trigger center, signed-in user
  right) and a sidebar on the left.
- The sidebar shows the active workspace (avatar + name) with a switcher
  listing every workspace the user belongs to; switching navigates into
  that workspace. Below, the workspace's projects as identifier + name rows
  with the active project marked; picking one opens its issue list. The
  footer shows the signed-in user and a sign-out entry.
- `mod+k` (or the title-bar trigger) opens the command palette: a filter
  field over grouped commands with arrow/enter/mouse activation and an
  empty-result message. Screens register their commands while mounted;
  the layout registers sign-out and go-home globally.
- Toasts confirm or report failures that happen outside any form (session
  expiry, refresh failure).

## Sign-in

- One card, email first. The address is checked server-side; an existing
  password account proceeds to the password step, a code-login account
  receives an emailed code and proceeds to the code step. Unknown addresses
  stay on the email step with an explanatory banner (account creation
  belongs to a later epic).
- The code step offers resend behind a short cooldown and a way back to a
  different address. Both steps submit natively (Enter works) with
  progress on the submit button.
- Failures arrive as server error codes and render as dismissible banners
  on the step that can fix them (wrong password stays on password, bad or
  expired code stays on code). A code carried in the URL (server redirect
  after a native-form failure) preloads the same way.
- Success follows the server landing URL, which already encodes the
  return path, onboarding, last workspace, and invitation targets.
- Signed-out visitors hitting workspace pages bounce to sign-in with the
  return path preserved; signed-in visitors hitting sign-in continue to
  their return path or first workspace. An expired session mid-app toasts
  and returns to sign-in with the current page preserved.
- Sign-out ends the server session where reachable, always clears every
  client store and the whole query cache, and lands on sign-in.

## Issue list

- `/$ws/projects/$projectId/issues` renders the project's issues read-only:
  a header with the project name and count, then virtualized rows of
  project key (`IDENTIFIER-sequence`), name, and priority.
- The single search param `layout` (`list` default, `compact` denser rows)
  is validated; anything invalid falls back to the default.
- Data loads through the route (session/membership gate, then parallel
  prefetch of projects and issues) so the screen reads from cache.
  Load failures render a retry panel; unknown projects render a pointer
  back to the sidebar. A palette command refreshes the list.
- An empty project renders a guidance empty state, never a blank page.

## Non-goals (owned elsewhere)

OAuth and other providers, sign-up, invitations, onboarding, workspace
creation, home dashboard, issue layouts beyond list density, filters,
detail, comments, and desktop-only behavior — each names its epic.
