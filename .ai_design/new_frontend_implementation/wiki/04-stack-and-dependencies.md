# Stack and dependencies

**Read this when:** you add a library, or need to know which library to use for something.

## Stack

| Concern | Choice | Notes |
|---|---|---|
| Build | Vite | static output; also bundled by Tauri |
| UI runtime | React 19 + React Compiler | no manual `useMemo`/`useCallback` unless profiling shows a need |
| Routing | TanStack Router (file routes) | typed paths and validated search params |
| Server data | TanStack Query | the only way components get server data |
| Client state | Zustand | UI state only, one small store per feature |
| Styling | Tailwind 4 + CSS variable tokens | tokens in `@pidash/kit` |
| UI primitives | Base UI | the only headless primitive library |
| Positioning | Floating UI (through Base UI) | no Popper, no tippy |
| Lists and boards | TanStack Virtual | |
| Tables | TanStack Table (headless) | only where a real table is needed |
| Drag and drop | Atlassian pragmatic-drag-and-drop | lazy, per board |
| Forms | react-hook-form + zod (`zod/mini` on runtime paths) | |
| Dates | date-fns | |
| Icons | lucide-react, per-icon imports | no icon fonts |
| Rich text | Tiptap | lazy; see *UI kit and editor* |
| Collaboration | Yjs + `apps/live` (Hocuspocus) | lazy; pages only |
| Charts | Recharts 3 | lazy only; wrap in our own components in `shared/charts`, colors from kit tokens |
| Tests | Vitest, Testing Library, Playwright | |

## Rules

- **Adding a runtime dependency needs review.** `apps/web_new` and `packages/kit` have a dependency allowlist in CI. Explain in the PR why an existing choice does not work, and its gzipped size.
- **Heavy dependencies are only reached through `import()`**: editor, charts, drag-and-drop, PDF, emoji picker, collaboration. A lint rule rejects static imports of them.
- **One library per job.** Do not add a second primitive library, positioning library, date library, icon set or state library.
- **No lodash.** Use native JS; add a tiny helper in `shared/lib` if needed.
- **No old packages.** Library choices of the old app are not reasons to add a library here.
- Versions go through the pnpm catalog (`catalog:`) where the workspace already pins them.
