> **Superseded.** This note was folded into [`space.md`](space.md) by NEWFRONT-751; that file is the authoritative build spec for the area. Kept for history (the oracle research notes it was written from).

# Area spec — space board access and routing (SPC-001–009)

Source: NEWFRONT-447 oracle research. Behavior paraphrased from the old
space frontend's route table, board layout and page, legacy-link loader,
landing gate, auth views, not-found views, and the public board endpoints.
No code, selectors, copy or structure are carried over; the new frontend
is written from this prose.

## Anonymous board load (SPC-001)

A published board opens at its anchor address for visitors with no
account. The page renders the board shell (navbar carrying the project
identity), the published issues, and a sign-in control that remembers the
exact board position (anchor, view, filters, open issue) so the visitor
returns to the same view after authenticating. Nothing redirects an
anonymous load toward sign-in.

## Legacy link resolution (SPC-002)

The old two-segment workspace/project address forwards to the board's
anchor address. The chosen view and the open issue travel across as
address parameters; the board then validates them like any pasted address.
A project with no published board lands on the generic guidance page
(described under SPC-009) at a fixed numeric address rather than on the
anchor illustration.

## Unknown anchor (SPC-003)

An anchor the server does not know — mistyped or withdrawn — renders a
dedicated not-found illustration with an explanatory heading. The board
chrome (navbar, layouts) does not render. This view is distinct from both
the generic board-error illustration and the unmatched-path page.

## Board-error illustration (SPC-004)

Any board data failure other than "anchor unknown" renders the generic
board-error illustration instead of a spinner or a blank page. Two
branches feed it: the settings resolution in the board layout (which
replaces the whole page, navbar included) and the issues-list load in the
layout root (which renders under the already-resolved navbar). The
illustration's supporting line speaks of a missing board even though this
branch only runs for non-404 failures — the wording is misleading but
current.

## Landing auth gate (SPC-005)

The site root is the auth gate. While the session resolves, a branded
spinner fills the viewport. Anonymous visitors then get the email-first
sign-in/sign-up flow; signed-in visitors get a guidance view (product
mark, account control, illustration, heading) telling them to paste the
public link they are trying to reach. A valid return address carries a
signed-in visitor straight on to that board instead of showing guidance.

## Return-address validation (SPC-006)

Every return address round-trips through a shared path validator that
keeps in-app paths only: the value must be a relative path starting with
a single slash, with no backslashes and none of the screened
active-content patterns. Absolute addresses, protocol-relative addresses,
scheme payloads, backslash paths, and slash-less values are dropped, and
the visitor stays on the safe default view. Valid values — including ones
carrying their own view query — survive the round trip. This resolves
open confirmation (5): the landing gate uses the same validator as the
navbar, vote, reaction, and comment sign-in links, so all entry points
agree on what survives.

## Board metadata (SPC-007)

Target behavior: shared links unfurl the board's name and summary with no
cover image; boards without metadata and malformed anchors fall back to
the generic title and description. Current behavior is a bug
(NEWFRONT-453): the metadata loader builds its request from a base URL
that ships empty, which is unresolvable server-side, so every board
serves the generic tags. A control run under an absolute base renders the
board name correctly — and also emits cover-image tags when a cover is
set, a path the target (main#523) removes. The new frontend implements
the target: per-board name/summary, generic fallback, never a cover
image.

## Crawler directive (SPC-008)

Every served page carries a robots directive asking crawlers neither to
index nor to follow. It lives in the shared document shell, so board,
landing, auth, and not-found pages all carry it identically.

## Address normalization

Every client-side navigation passes through a shim that enforces a
trailing slash on the pathname, so board addresses settle in the slashed
form with the view and filter parameters preserved. Server-issued
forwards (the legacy link) carry no slash until the first client
navigation normalizes them.

## Unmatched paths (SPC-009)

Any address matching no route — single unknown segments and deep unknown
paths alike — renders the generic guidance page: an illustration, a
heading owning the failure, and a line telling the visitor to check the
address they typed. It is the same page the unpublished legacy link lands
on, and it is distinct from the anchor-not-found illustration.
