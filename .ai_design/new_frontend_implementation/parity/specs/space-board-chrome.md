> **Superseded.** This note was folded into [`space.md`](space.md) by NEWFRONT-751; that file is the authoritative build spec for the area. Kept for history (the oracle research notes it was written from).

# Space board chrome spec — navbar, theme, badge, account, loading, notices, failure screens, locale (NEWFRONT-448)

Parent epic NEWFRONT-97 (tracking). Inventory
`.ai_design/new_frontend_implementation/parity/inventory/space.md` (rows
SPC-010–019 plus SPC-065). Oracle scenarios
`apps/web_new/e2e/parity/space/board-chrome.spec.ts`.

Method: the old space app was read and driven as a behavior reference only.
Nothing is imported, copied or translated from it; this spec restates what was
learned in prose. New code is written from this spec, the wiki pages
(Reference not source; Parity; Data layer; UI kit and editor) and the kit —
never with old files open. Literal UI wording lives only in the oracle
scenarios (which must match the old app to assert); this file paraphrases.

Status convention: this oracle sets the owned rows to oracle green. Build
slices later append `built`; the area gate turns passing rows into new green.

## Serving note (shared NEWFRONT-97 contract)

In parity serving the old space app runs at the proxy root (board, landing and
legacy link paths), while the auth API prefixes return addresses with the
production spaces mount point. Production serves the old app under that mount
point, so the round trip works there; on the oracle the prefixed landing falls
on the catch-all. Scenarios therefore pin the carried return address (link
target, form field, redirect target), not the post-auth landing page. The new
app lives under the mount point, so the full round trip is provable at gate
time. No data-testid was added to the old app for this sub-area: every read
uses roles, text, or neighbor-anchored structure.

## SPC-010 — Navbar project identity with fallbacks

The navbar opens with the published project's configured mark followed by its
name. The mark has two configured branches (a glyph-font icon, an emoji
rendered from decimal code points); a project with no configured mark renders
an empty slot, not a fallback. When the publish payload carries no project
details at all, the bar stays intact and shows a generic project glyph plus a
short placeholder where the name would be. Oracle proves the live name against
the publish payload, each mark branch via a mutated payload, and the
details-absent fallback; a final payload read proves the interceptions never
touched the server.

## SPC-011 — Light/dark theme switch with OS default

A fresh visit with no stored choice follows the operating-system scheme
(dark OS paints dark, light OS paints light) and stores nothing. The navbar
switch then chooses an explicit theme, persisted across reloads; clearing the
stored choice returns the next load to the OS default. Old bug NEWFRONT-456:
the first click from the system default stores explicit light instead of
flipping (on a light OS nothing visibly changes; on a dark OS it happens to
flip), and only later clicks alternate. The `bug:` scenario pins that quirk;
the companion scenario pins the OS default, alternation, persistence, and
clearing. Intended behavior is a flip on every click from any starting state;
the new app implements the flip.

## SPC-012 — Product badge

A small branded badge is fixed to the bottom-right corner of every public
screen (board and landing alike), opening the product site in a new tab with
an opener-blocking relation. The unconfigured/disabled negative (renders
nothing) is not live-testable: the site address is a build-time constant and
every usage renders the badge enabled, so the scenario pins the positive on
both screens and this note records the negative as code-verified.

## SPC-013 — Anonymous sign-in remembers the board position

The anonymous navbar offers a single sign-in control whose target carries the
full board position — view, filter parameters, and open issue — as a return
address to the landing page. The scenario travels to a genuine position
(alternate view plus a real open issue) and asserts each part survives in the
link, asserts raw filter parameters echo through, and asserts the sign-in
endpoint honors the same address in its redirect (under the production mount
prefix; see the serving note).

## SPC-014 — Account menu with identity and position-preserving sign-out

A signed-in visitor sees an avatar button labeled with their display name;
opening it shows a menu with a native sign-out form. The form posts the
fresh-rotated CSRF token plus the current board position as the return
address. The scenario signs in through the spaces password flow, checks the
menu identity against the current-user endpoint, checks the form target and
carried position, submits, and asserts the redirect preserves the position,
the server session is destroyed (current-user goes unauthorized with the old
cookie), and the board returns to anonymous chrome.

## SPC-015 — Embedded frame hides account and emoji controls

Inside an embedding frame the account slot renders nothing in either state
(no sign-in control anonymous, no avatar menu signed in) and the emoji add
controls render nothing even with reactions enabled; votes, comments UI and
activity stay inert or hidden per the same gate. The board itself — issue rows
and the open peek — stays fully readable. The scenario enables reactions,
establishes a signed-in session, proves the emoji control renders unframed,
then loads the same peek URL in a same-origin frame harness and asserts
readability plus the absences. A same-origin harness matters: it keeps the
session cookie flowing into the frame, and an in-frame current-user fetch
guards the assertion against vacuous hides.

## SPC-016 — Loading states gate content

Three stages, three behaviors. Instance boot shows the pulsing logo mark
full-screen with nothing else. Board resolution shows NO spinner: the bar
renders on its generic mark plus placeholder with an empty content area (the
layout's spinner branch is unreachable because the publish accessor always
hands back an object, empty until settings land). The first issues fetch shows
the pulsing mark centered under the loaded navbar. In all three stages no
issue content renders before its data arrives, and every stage settles into
the full board. Not filed as a bug: the placeholder-plus-empty-content state
is a coherent designed loading state, and the no-wrong-content spirit holds;
the scenario pins the per-stage reality instead.

## SPC-017 — Navigation progress and toast notices

Slow route transitions (proven via a held legacy-link lookup) render the top
progress bar node for the duration and remove it once settled; instant
in-app transitions resolve without one by design. Notices: copying the peek
link raises a success notice and writes the full peek address to the
clipboard; a comment post whose API call fails raises an error notice and
leaves the thread unchanged on the server. Gap recorded for the gate: votes
raise no notice in the current app although the row's parenthetical lists
them, and link editing has no space-side surface — the scenario pins
copy-success plus comment-failure only. The progress-bar library leaves its
busy hook class on the document after completion (upstream remove-path
quirk); it is invisible and deliberately not asserted.

## SPC-018 — Unreachable backend

With the instance endpoint aborted, the board shows the branded failure view:
logo bar on top, themed illustration, an explanatory heading, and a retry
control. Retry reloads; with the backend restored the board returns, and the
scenario confirms the endpoint healthy again. The instance fetch retries
nothing client-side, so a single aborted attempt deterministically reaches
the failure state.

## SPC-019 — Render crash

A render fault (proven by feeding the logo renderer an unparseable emoji
payload through the publish response) lands on the crash screen: an
apologetic heading, a contact line with support-mail and forum routes, and a
refresh control. Refresh reloads; with the payload restored the board
returns, and the scenario confirms the publish payload intact on the server.

## SPC-065 — Locale fallback with stable identifiers

The OSS build ships English only, and several surfaces never pass through
the translation layer (auth screens, notices, dates, keys). Switching the
stored locale to an unsupported value is therefore a no-op with healing: the
app keeps rendering English, the document language stays English, and the
stored choice is rewritten to English on boot. The scenario captures peek
properties, issue keys, navbar plus badge text, and the landing title,
subtitle and consent line before and after storing an unsupported locale,
and asserts all of it byte-identical with keys and server data stable. Full
translation of chrome, filter, auth and notice text needs a locale pack only
the cloud edition ships; the gate extends this scenario where packs exist.

## Gaps and handoffs for the area gate

- SPC-011 NEWFRONT-456: new app implements the flip; gate scenario asserts
  immediate alternation from the system default.
- SPC-016: gate keeps the per-stage expectations (spinner, placeholder,
  spinner) — do not "fix" resolution to a spinner without a product call.
- SPC-017: gate decides whether votes/notices grow new toasts; until then
  copy-success plus comment-failure is the contract.
- SPC-012 negative and SPC-065 translation half: unprovable on OSS; gate
  extends with configuration or locale packs if available.
- SPC-013/014 round-trip landings: re-proven end to end under the new app's
  mount point, where the prefixed redirect resolves to the board.
