# Page export dialog and cross-cutting rows — area spec (NEWFRONT-383)

Behavior learned by reading the old page-export dialog, the paginated
document renderer, the editor content parser, and the export entry points
across settings, analytics, profile, and pages. Prose only: no old code,
strings, styles, or assets are reused. New code (driver extension +
oracle specs) is written from this spec. Covers inventory rows EXP-016
through EXP-021 plus EXP-023 and EXP-024. Every claim below is verified
live against the oracle unless marked otherwise.

## Opening the dialog and its defaults (EXP-016)

- Any signed-in user who can open a collaborative page detail reaches the
  export dialog through the detail header's options menu "Export" entry.
  The dialog renders centered over the dimmed page; the page behind stays
  mounted but inert until the dialog closes.
- The dialog carries three labeled choice rows plus a footer: an export
  format row (document vs plain-text), a content-scope row
  (full-fidelity vs image-free), and — only while the document format is
  selected — a page-size row. The footer holds a secondary cancel action
  and a primary confirm action.
- Fresh defaults are the document format, the full-fidelity scope, and
  the metric default page size. Reopening the dialog after any close
  shows these defaults again (the form resets shortly after close).

## Format choice and the conditional page size (EXP-017)

- The format row offers exactly two formats: the paginated document
  format and the plain-text markup format.
- The page-size row offers the common office paper sizes (the metric A
  series plus the US office sizes) and renders only while the document
  format is selected; switching to plain text removes the row, and
  switching back restores it with its previous choice intact.
- The chosen page size travels into the rendered document and into the
  downloaded file name (see EXP-019); it has no effect on plain-text
  output.

## Content scope (EXP-018)

- The scope row offers exactly two variants: full fidelity (everything
  the page holds) and image-free (pictures stripped).
- In document output the image-free variant removes embedded pictures
  and plain image elements before rendering, so the file carries no
  image objects; the full-fidelity variant keeps them. Pictures whose
  source is an absolute fetchable URL are inlined as image objects;
  uploaded pictures (relative stack-asset URLs) draw their frame but
  drop their raster — the inlining step rejects relative URLs (filed as
  bug NEWFRONT-470; the oracle pins the live behavior with a bug:
  marker and the row records the intended behavior).
- In plain-text output the image-free variant strips editor image tags
  and image elements. Pictures the editor serialized as plain markdown
  image syntax are NOT stripped by either scope (the strip matches tag
  patterns only) — this is filed as bug NEWFRONT-356 and the oracle
  scenario pins the live behavior with a bug: marker; the intended
  behavior (image-free strips every picture form) is recorded on the
  inventory row.
- Mention components resolve to readable display names in both formats
  (profile links in plain text), and embedded issue cards are dropped
  from both outputs.

## Saving a page as a paginated document (EXP-019)

- Confirming while the document format is selected renders the page
  title plus the parsed body into a single white padded document page
  at the chosen paper size, typeset in the bundled product typeface
  family, and saves it through the browser download event. No API call
  is involved: the whole render happens client-side.
- The suggested file name derives from the page title (lowercased,
  runs of unsupported characters collapsed to single dashes) followed
  by the chosen paper size in lowercase with a document extension.
- The oracle asserts the download event (file name plus downloaded
  bytes): the bytes open as the document format, carry the page-size
  media box of the chosen size, embed the product typeface, and include
  the page title text.

## Saving a page as plain text (EXP-020)

- Confirming while the plain-text format is selected converts the live
  editor body to plain-text markup honoring the scope choice and saves
  it through the browser download event, again fully client-side.
- The suggested file name derives from the page title exactly as above
  (without any size suffix) with a plain-text extension.
- The oracle asserts the download event: file name plus full text
  content carrying the body's text and markup with the scope's image
  rule applied.

## Feedback, failure, and reset (EXP-021)

- Success raises a success confirmation notice and closes the dialog;
  the form resets shortly after close so the next opening shows the
  defaults.
- Failure (the render or save throws) raises an error notice asking
  for a later retry, leaves the dialog open with the user's choices
  intact, and returns the confirm action from its working state to its
  idle label. The oracle forces this branch by sabotaging the browser
  save primitive in-page, which deterministically throws inside the
  save step for either format.
- Cancelling (the cancel action or the dialog dismiss) downloads
  nothing and discards the choices: reopening shows the defaults.

## Keyboard and focus (EXP-023)

- The Tab-reachable export controls operate by keyboard alone: the
  settings submit fires the export on Enter, the analytics export
  actions and the profile download save their files on Enter, the
  dialog footer confirms on Enter, and Escape dismisses the dialog.
  No screen in this area defines custom key bindings: unmodified
  content keys pressed outside a text field change nothing (no
  navigation, no dialog, no download, no notice).
- Against that, three gaps (filed together as bug NEWFRONT-471; the
  oracle pins the live behavior with a bug: marker and the row records
  the intended behavior): every picker sits outside the tab order (the
  settings project and format pickers and all three dialog rows carry
  `tabindex="-1"`, so dialog Tab cycles only Cancel and Export); the
  dialog picker menus open on Space but never move focus or selection
  on arrows and select nothing on Enter, and an Escape-dismissed menu
  closes visually while leaving the picker's toggle state desynced so
  the next same-button click is swallowed instead of reopening; and no
  export control shows any visible focus change (focused computed style
  is identical to blurred on every control).

## Uniform behavior across editions and desktop (EXP-024)

- The runnable build serves every export surface ungated: the settings
  page, the analytics export actions, the profile download control,
  and the page-export dialog all render and behave per their rows.
- Entering each surface talks only to the known export endpoints; no
  request path mentions desktop, edition, license, billing, or upgrade
  markers, and no upgrade/paywall/desktop-shell marker renders on any
  export surface.
- Source sweep (recorded on the row): no edition-gated or desktop-only
  export code exists — the exporter components, the analytics file
  builder, the profile download control, and the page-export dialog
  plus document renderer carry no edition branches, and no overlay or
  desktop-shell export code exists to diverge.

## Scenario data strategy

- Page scenarios create pages in the seeded project through the API
  with per-attempt unique names (additive only; shared seed rows are
  never mutated), open the detail authenticated, wait for sync, and
  type body content through the live editor; pictures are pasted as
  image elements (stable) rather than REST-seeded (which races socket
  sync). Cross-surface scenarios reuse the sibling worlds (settings
  page via the exports-settings driver range, analytics via the
  analytics-export range, profile via the profile-activity range).
- Downloads go through the UI against the live stack and assert the
  download event (file name plus bytes), never a stubbed file. PDF
  bytes are introspected as latin1 text for the format magic, the
  media-box dimensions, the embedded typeface family name, the title
  text, and the presence or absence of image objects. Markdown bytes
  are asserted as full text.
- Flakiness guards follow the pages-area precedent: the shared editor
  reference can lose its load race and export empty content (bug
  NEWFRONT-360), so content-bearing scenarios re-roll the load a
  bounded number of times until the marker lands; toast assertions
  quiesce between stacked notices.
