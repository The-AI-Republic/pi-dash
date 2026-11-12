> **Superseded.** This note was folded into [`space.md`](space.md) by NEWFRONT-751; that file is the authoritative build spec for the area. Kept for history (the oracle research notes it was written from).

# Space engagement and comments — area spec (NEWFRONT-451)

Oracle area spec for inventory rows SPC-047–057 (issue votes, issue emoji
reactions, and the comments thread with its composer, mentions, owner
menu, edits, and comment reactions). Written in prose from reading the old
space app and from driving the running old app against the seeded parity
stack. New oracle code (driver extension plus scenarios) is written from
this spec; no old code, strings, styles, or assets are reused.

Old sources consulted (reference only): the peek-overlay reaction and
activity sections, the side/modal/full overlay shells and their header,
the issue vote and emoji components, the board-card reaction footer, the
comment card/composer/reaction components, the comment editor wrapper
with its toolbar and mention rendering, the publish-flag store, the
issue-detail store (votes, reactions, comments, optimistic updates), the
member store, the public-sites services, and the public anchor API views
plus the deploy-board publish endpoint. Live observations below were
confirmed on the seeded stack (one published project, three issues in a
single state, all engagement flags off until each scenario flips them).

## Issue votes (SPC-047)

- Two controls, one per direction, each pairing an arrow glyph with a
  live count. They render in the peek overlay whenever the publisher
  enables voting, and in board-card footers on the card layout only. The
  row layout shows no voting affordance at all (inventory confirmation
  4, consistent with these rows: the row layout simply has no footer).
- Hovering a control opens a small card naming the voters in order, or a
  short empty note when nobody voted that way. The visitor's own vote is
  marked by a highlight treatment on the voted side.
- Clicking toggles: voting a side the visitor already voted removes the
  vote; voting the other side moves it (one stored vote per visitor per
  issue). The screen updates immediately and the server is then told; a
  failed write re-reads the stored votes so the view matches the server.
- Signing out and clicking either control sends the visitor to the
  landing sign-in page with a return address that keeps the board and
  the open issue but drops the board view and filters: the address is
  spliced into the sign-in URL unencoded, so everything past the first
  parameter parses as outer parameters and never comes back after
  sign-in (NEWFRONT-465). The intent is the full position — board,
  view, filters, open issue.
- Server notes: votes are created-or-replaced per visitor and removed
  wholesale (there is no per-direction delete). Unlike comments and
  reactions, the vote write path does not check the publisher flag, so a
  vote posted directly to the API while voting is disabled still stores;
  the UI simply offers no control then. The public votes list endpoint
  resolves the board by the wrong key and always answers empty, so both
  the app and these scenarios read votes from the issue payload's
  embedded vote items instead.

## Issue emoji reactions (SPC-048)

- Reactions render as grouped pills below the issue title: one pill per
  emoji with its count, the visitor's own pills highlighted, plus an add
  control that opens the emoji picker. Hovering a pill names the reactors
  in order (truncated with an overflow note past a handful).
- Picking an emoji from the picker adds the visitor's reaction; clicking
  a pill the visitor already reacted to removes theirs, and clicking any
  other pill adds theirs to that group. Like votes, the screen updates
  first and reconciles from a fresh read when the write fails.
- The whole control is gated twice: it renders only while the publisher
  enables reactions, and the overlay hides it inside embedded frames.
  Anonymous clicks (on a pill or in the picker) redirect to sign-in
  with the same truncated return address as votes: board and open
  issue kept, view and filters dropped (NEWFRONT-465).
- Board-card footers on the card layout carry the same control. The card
  footer does not check the embedded state, so embedded card views still
  offer the add control where the overlay correctly hides it; that gap is
  recorded here for the chrome area (SPC-015), which owns the
  frame-wide claim.
- Server notes: issue reactions are stored per visitor per emoji code
  (decimal code points) and removed per code. The public reactions list
  endpoint carries the same wrong-key lookup as the votes list and also
  always answers empty; reads go through the embedded reaction items.

## Comments section gating (SPC-049)

- The comments section (heading, thread, composer/invitation) renders in
  the side and modal presentations only while the publisher enables
  commenting. With commenting on, everyone reads the thread; signed-in
  visitors get the composer and anonymous visitors get a sign-in
  invitation instead — except inside embedded frames, where the
  invitation is suppressed and the thread stays readable.
- The full-screen presentation does not gate the section: with commenting
  off it still renders the heading with an empty thread, and anonymous
  visitors are even invited to sign in for a composer that never appears
  (NEWFRONT-461). The intended behavior, recorded in the row, is uniform
  gating in every presentation.
- The invitation's return address keeps only the bare board: the open
  issue and the view are dropped, so sign-in returns to the board with
  the issue closed (NEWFRONT-465). Same intent as above — the full
  position.
- The server gates too: the comments list answers empty and all comment
  writes refuse while the flag is off.

## Comment rows (SPC-050)

- Each row shows an avatar (uploaded image when the author has one, else
  the author's initial), the display name, an age line, and the rendered
  rich body. Authors flagged as bots render with their given name plus a
  bot suffix instead of the plain display name.
- The age line never renders an age: every row reads the same bare word
  with nothing after it (NEWFRONT-459). The intended behavior, recorded
  in the row, is a relative age matching the workspace comment rows.

## Folded and machine-authored comments (SPC-051, SPC-052)

- Comments carrying the fold marker render collapsed: the header shows
  with a toggle control, and the body (plus its reactions) stays hidden
  until the visitor expands it in place. Toggling again collapses.
- Comments authored by a machine carry a speaker label line above the
  body; when the record has no label text a generic agent label renders.
  Plain human rows show no such line.

## Posting rich comments with attachments (SPC-053)

- The composer is a rich editor: typed text, pasted formatted fragments
  (bold and friends survive the paste), and toolbar formatting all post
  as rendered. Enter submits while signed in; a dedicated button submits
  too. A successful post appends the row, clears the draft, and stores
  the formatted body.
- The toolbar's image command opens a file chooser; the chosen file
  uploads first, then the post links the stored asset to the new comment
  so it renders embedded in the row. Posting with an attachment is one
  flow: draft, attach, wait for the upload to finish, submit. The
  embedded node inserts in an uploading state and flips to uploaded when
  the file lands; submitting mid-upload stores the node without the file
  (observed live), so the scenario waits for the finished state exactly
  as a visitor watching the spinner would.
- A failing post raises an error notice, keeps the draft text, and leaves
  the existing thread untouched; nothing is stored server-side.

## Composer mentions (SPC-054)

- The composer offers no member autocomplete: typing the mention trigger
  plus a name fragment opens no dropdown and inserts no chip
  (NEWFRONT-460), even though the members source is populated and posted
  mention markup renders as chips. The intended behavior, recorded in the
  row, is a member dropdown that inserts a chip on pick.

## Owner menu (SPC-055)

- A row carries its overflow menu only for its own author while signed
  in outside embeds: other visitors see no menu on that row, and embedded
  frames show no menus at all. The menu holds exactly two entries, edit
  and delete. Deleting removes the row at once and deletes it server-side.

## Comment edits (SPC-056)

- Saving an edit swaps the row content immediately, then persists; the
  stored body matches right after.
- A failing save leaves the phantom edit on screen: the row swaps to the
  attempted text and never swaps back, even though the server still holds
  the previous text (NEWFRONT-467). The store does re-list the thread on
  failure, but the save handler also pushes the attempted text straight
  into the display editor instance, which then ignores the reconciled
  store. The intended behavior, recorded in the row, is a visible
  reconcile to the stored text, matching the failed-create path.

## Comment reactions (SPC-057)

- Each row carries a grouped-pill control with counts, an add control
  with picker, click-to-toggle semantics, and the same
  optimistic-then-reconcile update pattern as issues. Anonymous pill
  clicks redirect to sign-in with the same truncated return address as
  issue reactions (NEWFRONT-465).
- The pills never name their reactors: hovering shows nobody, and the
  visitor's own reactions are never highlighted on rows loaded from the
  server (NEWFRONT-466). The comments payload carries each reaction's
  actor flat while the pill view reads the nested actor shape the
  issue-level payloads use, so every lookup misses. Reactions added
  through the UI do highlight until the next server re-list, because the
  optimistic rows are built in the nested shape. The intended behavior,
  recorded in the row, is reactor names and own-pill highlighting like
  the issue pills.
- Embedded frames render the counts with no add control. Like issue
  reactions, the write path requires the publisher's reactions flag even
  though the control itself lives on comment rows.
