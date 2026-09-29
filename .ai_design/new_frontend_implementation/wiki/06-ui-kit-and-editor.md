# UI kit and editor

**Read this when:** you build UI, add or change a `@pidash/kit` component, or touch rich text.

## Visual direction

The visual design is new; it is not Plane's. Desktop-first: compact density by default on desktop, comfortable on web, keyboard-first, peek panel for issues instead of navigating away from lists. Follow the design tokens and mockups from the Phase 0 design issue; do not invent new colors, spacing or type sizes.

## `@pidash/kit`

- Base UI primitives styled with Tailwind 4 and CSS variable tokens (`tokens.css`): color (light and dark), spacing, radius, type scale, elevation, and a `--density` scale.
- Components: Button, IconButton, Input, Textarea, Select, Combobox, Menu, ContextMenu, Dialog, Sheet, Popover, Tooltip, Tabs, Toast, Avatar, AvatarGroup, Badge, Kbd, Checkbox, Switch, Table, VirtualList, EmptyState, Skeleton, Spinner.
- No Pi Dash domain knowledge in the kit. Domain pickers (member, state, label…) live in `apps/web_new/src/shared/pickers`.
- Every kit component has a story (Ladle or Storybook) and component tests; accessibility (keyboard, focus, labels) is part of done.
- Before building a new component in a feature, check the kit. If a pattern appears in two features, propose it for the kit.
- Icons: lucide-react, per-icon imports. Fonts: one variable UI font and one mono font.

## Commands and shortcuts

One registry in `shared/commands`: every command has one ID, one handler and one shortcut, shown in the command palette, native menus (desktop) and tooltips. Do not bind keys anywhere else. Old keyboard shortcuts are inventory rows; parity requires them.

## Editor

- **`RichTextView`**: renders stored `description_html` / `comment_html` through a sanitizer. No Tiptap. Use it in lists, peek, comments and activity.
- **`RichTextEditor`**: `React.lazy` Tiptap editor, loaded when the user starts editing (preloaded on hover). Extensions needed for parity: paragraphs, headings, lists, task lists, code blocks with highlighting, links, mentions, images and attachments (existing asset endpoints), and anything else the inventory lists.
- **`CommentInput`**: a lighter configuration of the same editor chunk.
- **Collaboration** (Yjs + `apps/live`): its own lazy chunk, used for pages only.
- **Compatibility is required.** HTML written by the new editor must render correctly in `apps/web`, and HTML written by `apps/web` must round-trip through the new editor without loss. A fixture set of real stored descriptions is tested in CI.

## i18n and theme

- Message keys and English copy are written new for this app; do not copy keys or strings from `packages/i18n`. Only the active locale is loaded.
- Light, dark and system themes via tokens; desktop follows the OS.
