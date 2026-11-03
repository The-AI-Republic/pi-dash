**Decision (2026-09-29): dense and neutral, desktop-first.** A professional tool look: neutral grays, one indigo accent, compact rows, hairline borders, strong keyboard focus. No mockups up front; the F-05 kit PR (Ladle stories) is the review point, and changes after that are filed as issues.

## Type

- UI font: **Inter** (variable, self-hosted, latin subset preloaded, `font-display: swap`). Fallback: `system-ui, -apple-system, "Segoe UI", sans-serif`.
- Mono: system stack `ui-monospace, "SF Mono", "Cascadia Code", Menlo, monospace` (no font file).
- Scale (size / line height): 11/16 caption · 12/16 meta · **13/20 base** · 14/20 emphasis · 16/24 h3 · 20/28 h2 · 24/32 h1.
- Weights: 400 body, 500 labels and row titles, 600 headings.

## Color tokens

| Token | Light | Dark |
|---|---|---|
| `--bg` | `#FFFFFF` | `#0B0B0D` |
| `--surface` (sidebar, panels) | `#FAFAFA` | `#121214` |
| `--subtle` (hover, selected row) | `#F4F4F5` | `#1A1A1D` |
| `--border` | `#E4E4E7` | `#26262A` |
| `--text` | `#18181B` | `#EDEDEF` |
| `--text-muted` | `#71717A` | `#8B8B93` |
| `--accent` | `#5E6AD2` | `#7C85E0` |
| `--accent-hover` | `#4F5BC4` | `#8E96E6` |
| `--accent-subtle` (selection bg) | `#EEF0FB` | `#1E2140` |
| `--success` | `#2F9E5B` | `#3DB86C` |
| `--warning` | `#D9912B` | `#E5A84A` |
| `--danger` | `#E5484D` | `#F2555A` |

- Workflow state colors come from the project's data (each state has a color), not from tokens.
- Text contrast meets WCAG AA in both themes; check it in the kit tests.

## Spacing, size, shape

- 4px grid: 0, 2, 4, 6, 8, 12, 16, 20, 24, 32, 40, 48.
- Density: **compact** (desktop default) rows and controls 28px; **comfortable** (web default) 32px. One `--density` switch; user can change it.
- Radius: 4 controls · 6 popovers, menus, cards · 8 dialogs · full for avatars and pills.
- Borders: 1px hairlines. Dark mode separates surfaces with borders rather than shadows.
- Elevation: lists flat; popovers and menus a small shadow; dialogs a large shadow plus backdrop.
- Icons: lucide, 16px, stroke 1.5.

## Behavior

- Theme: **follow the OS** by default; user can choose light, dark or system. Desktop follows OS changes live.
- Focus: always visible, 2px `--accent` ring with 1px offset, on every interactive element.
- Motion: 120ms ease-out for hovers, popovers and panels; none when `prefers-reduced-motion`.
- Layout: title bar (breadcrumb, ⌘K) + collapsible sidebar (workspace, inbox, my issues, projects) + content; issues open in a right-side peek panel.
