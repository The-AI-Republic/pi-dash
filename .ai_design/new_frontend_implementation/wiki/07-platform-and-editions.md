# Platform and editions

**Read this when:** you touch desktop (Tauri) behavior, or cloud-edition behavior.

These two interfaces replace the old file-swapping mechanisms (`desktop-overlay/`, `apps/web/ce/`, `private-pi-dash/ee-overlay/`). Never replace files at build time.

## Platform (`core/platform`)

```ts
export interface Platform {
  kind: "web" | "desktop";
  fetch: typeof fetch;                         // desktop: Rust HTTP via invoke (desktop_http.rs)
  stream(url: string, init?: StreamInit): EventStream;  // SSE; desktop: desktop_api_stream
  openExternal(url: string): Promise<void>;
  storage: KeyValueStore;                      // query cache persistence, UI prefs
  onFocusChange(cb: (focused: boolean) => void): Unsubscribe;
  window?: TitleBarApi;                        // drag region, traffic-light inset, fullscreen
  menu?: MenuApi;                              // native menu → command registry
  deepLinks?: DeepLinkApi;                     // pidash:// scheme
  agentRuntime?: AgentRuntimeApi;              // bundled runner / managed runner controls
  updates?: UpdaterApi;
}
```

- Chosen at build time: `PIDASH_TARGET=web` uses `web.ts`, `PIDASH_TARGET=desktop` uses `tauri.ts`. The web bundle must contain no Tauri code (CI check).
- Features use optional capabilities (`platform.agentRuntime?`) and render nothing when absent. **No `if (isDesktop)` outside `core/platform`.**
- The Rust side (`desktop/src-tauri/src/*.rs`) stays as it is. Write `tauri.ts` against the Rust commands directly; reading the old `packages/services` desktop adapter to learn the command names and payloads is fine, copying it is not.

## Desktop UX expectations

- Custom title bar with drag region and macOS traffic-light inset; window title follows the route.
- Native menu bar (File, Edit, View, Go, Window, Help) wired to the command registry.
- Opens on cached data, then refreshes, with a subtle syncing state.
- External OAuth through `platform.openExternal` + deep links.
- Desktop parity rows (agent runtime, bare sign-in, updater, deep links) come from `desktop-overlay/` and `apps/web/ce/components/desktop`.

## Editions (`core/edition`)

```ts
export interface Edition {
  id: "oss" | string;
  routes?: RouteExtension[];          // extra route subtrees
  sidebar?: SidebarItem[];
  settingsSections?: SettingsSection[];
  auth?: AuthProviderExtension[];     // extra sign-in methods
  api?: ApiMiddleware[];              // e.g. token refresh
  flags: Record<string, boolean>;
  slots?: Partial<SlotComponents>;    // named UI slots, e.g. "issue.detail.sidebar.after"
}
```

- OSS ships `oss.ts`. The cloud build in `private-pi-dash` provides its own module through a Vite alias for `@pidash/edition` only: one swap point with a typed contract.
- Slots are a short, explicit list. Adding a slot is a design decision: note it in the PR and on the *Decisions* page.
- Cloud-edition parity rows come from `private-pi-dash/ee-overlay/apps/web` (home, docs, downloads, pricing, login, apps, profile tabs, billing).
