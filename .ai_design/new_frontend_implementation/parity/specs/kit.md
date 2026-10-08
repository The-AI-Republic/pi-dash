// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Area spec for NEWFRONT-16 (F-05: @pidash/kit v0). Behavior is written in
// prose from the H-visual decision (NEWFRONT-5, settled as D12; full token
// spec on the "UI kit and editor" wiki page). No old-frontend code was read
// for this area: the components are generic primitives fully described by
// the approved direction, so there was no behavior to learn from the old
// trees. Implementation in packages/kit/src is written from this spec.

# Kit v0 area spec

## Visual direction (from NEWFRONT-5)

Dense and neutral, desktop-first. Neutral grays with one indigo accent,
compact rows, hairline borders, and a strong keyboard focus treatment.
The Ladle stories of this kit are the visual review point for the
direction; later changes arrive as new issues.

## Tokens (tokens.css)

The kit owns one token file exposing CSS custom properties. Everything
visual in the new frontend reads these tokens; no color, spacing, type
size, radius, or shadow may be invented outside them, and adding a token
is itself a kit change.

- Color, twelve tokens, each with a light and a dark value: background,
  surface (sidebar and panels), subtle (hover and selected rows), border,
  text, muted text, accent (indigo), accent hover, accent subtle
  (selection background), success, warning, danger. Workflow state colors
  are explicitly not tokens: they come from project data at runtime.
- Type: Inter variable for UI text with a system fallback stack, system
  mono stack for code. Seven steps from 11px caption to 24px h1, each
  with its own line height; 13px/20px is the base body size. Body weight
  400, labels and row titles 500, headings 600.
- Spacing on a 4px grid: 0, 2, 4, 6, 8, 12, 16, 20, 24, 32, 40, 48.
- Density: one switch with two stops. Compact is the default
  (desktop-first): rows and controls 28px tall. Comfortable renders the
  same components at 32px. The user can change it at runtime.
- Shape: 4px radius for controls, 6px for popovers/menus/cards, 8px for
  dialogs, fully round for avatars and pills. Borders are 1px hairlines;
  dark mode separates surfaces with borders, not shadows.
- Elevation: lists are flat; popovers and menus carry a small shadow;
  dialogs carry a large shadow plus a backdrop.
- Icons render at 16px with a 1.5 stroke through per-icon lucide imports.
- Theme follows the OS by default; the user can pin light, dark, or
  system, and the desktop build tracks OS changes live.
- Focus is always visible: a 2px accent ring with a 1px offset on every
  interactive element.
- Motion lasts 120ms with an ease-out curve for hovers, popovers, and
  panels, and is disabled entirely under prefers-reduced-motion.
- Text contrast of body text against its background meets WCAG AA in both
  themes; the kit test suite asserts the computed ratios. Two additions
  the kit PR makes to the table, both asserted in the same suite: a
  foreground-on-accent token for filled-action labels (white in light,
  dark navy in dark, AA on the accent in both themes), and a usage rule
  for the status hues, which sit below AA for small text in the light
  theme and may only back redundant graphics (badge dots) or large type.

## Components

The kit holds generic primitives only; no Pi Dash domain knowledge lives
here (domain pickers belong to apps/web_new shared/pickers). Each
component below has a Ladle story and component tests, and keyboard,
focus, and labeling behavior is part of done for each one.

- Button: primary (accent fill), secondary (hairline border), ghost, and
  danger variants; small, medium, and large sizes; disabled and loading
  states. Activates with Enter and Space; loading blocks re-activation.
- IconButton: a square button for icon-only actions. It is always given
  an accessible name; without a visible label the name comes from its
  label prop. Same focus treatment as Button.
- Input: single-line text field with label, hint, and error slots. Error
  styling and aria-invalid follow the error prop; the label is always
  programmatically associated.
- Textarea: multi-line equivalent of Input with the same label, hint, and
  error behavior; vertical resize.
- Menu: action list opened from a trigger, single-expand by default, with
  menuitem and separator rows. Full keyboard support: opens on Enter,
  Space, and ArrowDown; arrows move between items; Escape closes and
  returns focus to the trigger; Tab closes.
- Dialog: modal window with title, description, and close affordance.
  Focus moves into the dialog on open, stays trapped while open, and
  returns to the trigger on close. Escape closes unless the dialog is
  marked non-dismissable.
- Popover: non-modal anchored panel. Escape closes and returns focus;
  outside interaction closes it.
- Tooltip: hover and focus hint with a short delay; never blocks pointer
  interaction; labelled by its content for assistive technology.
- Toast: transient notification with title and optional description and
  action; dismissable by button, by timeout, and with Escape; announced
  politely to assistive technology.
- Avatar: round user mark showing an image, or initials when no image is
  available. Fixed small/medium/large sizes.
- Badge: small pill for counts and status: neutral, accent, success,
  warning, and danger tones. The tone shows as a status dot next to a
  body-contrast label (the dot repeats the adjacent text, so the status
  hues — which sit below AA for small text in the light theme — are never
  used as small text themselves). Counts render without a dot.
- Status hues in Badge and Button resolve through semantic theme tokens
  only (success/warning/danger subtle/primary plus hover steps); raw
  palette utilities are undefined in the themed build and silently emit
  nothing (targets per main#543, main#581).
- Kbd: keyboard shortcut chip rendering key names joined with plus signs.
- VirtualList: windowed rendering of long lists so only visible rows are
  mounted; rows keep stable keys and the list exposes the scroll element
  for restoration.
- Skeleton: shimmering placeholder matching the shape of loading content;
  hidden from assistive technology.
- Spinner: indeterminate progress indicator in small/medium/large sizes;
  exposed as a live region while active.

## Non-goals for v0

Select, Combobox, ContextMenu, Sheet, Tabs, Checkbox, Switch, Table,
EmptyState, and AvatarGroup are named on the wiki page but are not part
of the F-05 issue scope; they land with the feature issues that need
them. The rich-text editor, charts, and collaboration chunks are their
own lazy-loaded areas, not kit components. When Select/Combobox land,
their Escape-close path must keep trigger and panel state in sync so a
click reopens the dropdown after an Escape close (target per main#578).
