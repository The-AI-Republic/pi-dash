# Data layer

**Read this when:** you call the API, fetch or change server data, add a route, add filters, or add shared UI state.

## Where state lives

| State | Example | Tool |
|---|---|---|
| Server data | issues, projects, members, comments | TanStack Query |
| View state | filters, grouping, layout, open issue (`?peek=`) | router search params |
| Shared UI state | multi-select, peek stack, command palette, sidebar width, drafts | Zustand (per feature) |
| Local UI state | a dropdown's open state | `useState` |

## API client and contracts (`@pidash/api-client`)

The internal endpoints the frontend uses (`/api/workspaces/…`, `/auth/…`) are **not** covered by the OpenAPI schema (drf-spectacular is limited to `/api/v1/`). So contracts are hand-written.

- One small `fetch` client: base URL, credentials, CSRF header (from `/auth/get-csrf-token/`), JSON, `AbortSignal`, timeout, normalized `ApiError { status, code, message, fields }`, middleware (401 → session expired; edition interceptors). Transport is `platform.fetch`.
- One contract module per resource, written by us from the endpoint's behavior and the Django serializer:
  ```ts
  // packages/api-client/src/contracts/issues.ts
  export const Issue = z.object({ id: z.string(), name: z.string(), state_id: z.string(), /* … */ });
  export type Issue = z.infer<typeof Issue>;
  export const issues = {
    list: (c: Client, ws: string, pid: string, q: IssueQuery) =>
      c.get(`/api/workspaces/${ws}/projects/${pid}/issues/`, { query: q, schema: IssueListResponse }),
  };
  ```
- Schemas are validated in development and tests, skipped in production builds.
- Every contract has a **contract test** that parses a real response from the Django test server.
- Django source (`apps/api`) is the backend, not the old frontend: reading serializers and views to write contracts is fine.

## Server data (TanStack Query)

- Each feature exports key factories and `queryOptions` from `features/<x>/api/`. **Every key starts with `["ws", ws]`**, so a workspace switch or sign-out clears one subtree.
  ```ts
  export const issueKeys = {
    all: (ws: string) => ["ws", ws, "issues"] as const,
    list: (ws: string, pid: string, s: IssueSearch) => [...issueKeys.all(ws), "list", pid, s] as const,
    detail: (ws: string, id: string) => [...issueKeys.all(ws), "detail", id] as const,
  };
  ```
- Components use feature hooks (`useIssues`, `useUpdateIssue`), never raw `fetch` or the client directly.
- **Mutations are optimistic** for field edits, state moves, reorders and comments: `onMutate` patches caches, `onError` rolls back and toasts the server message, `onSettled` invalidates affected keys. Use the helpers in `core/query`.
- Defaults: `staleTime` 30 s for lists, 5 min for reference data (states, labels, members, projects). Refetch on window focus.
- Persistence: desktop persists the cache to disk (via `platform.storage`) and renders from it on launch; web persists reference data only.
- Real-time: `core/query/realtime.ts` is the only place that turns server events (SSE today: runner chat, assistant) into cache patches or invalidations.

## Routing (TanStack Router)

- File routes; each route component is a lazy chunk.
- Loaders call `queryClient.ensureQueryData(...)` so code and data load in parallel. `defaultPreload: "intent"`.
- **The URL is the source of truth for view state.** Each route declares a zod `validateSearch`; invalid params fall back to defaults instead of throwing. Saved views store a serialized search object.
- `$ws/route.tsx` `beforeLoad` enforces session and workspace membership; `_public/*` redirects signed-in users.
- Old URLs from `apps/web` keep working: same paths where possible, otherwise a redirect in `routes/_legacy.tsx`. Links shared from the old app must not break (this is a parity requirement).

## Client state (Zustand)

- Only state that is not server data, not worth putting in the URL, and shared by distant components.
- One small store per feature; no store references another store.
- Register each store's reset in `core/session` so sign-out clears everything.

## Session

`core/session` exposes `useMe()`, `useWorkspace()`, `usePermissions()`. Role checks go through `usePermissions()`, never ad hoc. A 401 from any request moves to a session-expired state and then sign-in, keeping the return URL.
