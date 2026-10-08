// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Platform contract (Platform and editions page). Features program against
// this interface; web.ts and tauri.ts implement it. No feature checks the
// build target directly.

/** Remove a subscription or listener. */
export type Unsubscribe = () => void;

/**
 * Small async key/value surface. The query cache persistence layer and UI
 * preferences share it; neither knows which backing store is underneath.
 */
export interface KeyValueStore {
  get(key: string): Promise<string | null>;
  set(key: string, value: string): Promise<void>;
  remove(key: string): Promise<void>;
  clear(): Promise<void>;
}

/** Options for opening a server-sent event stream. */
export interface StreamInit {
  method?: string;
  headers?: Record<string, string>;
  body?: string;
  signal?: AbortSignal;
}

/** One parsed server-sent event. */
export interface StreamMessage {
  event: string;
  data: string;
}

/** A live event stream. Closed explicitly or via the init AbortSignal. */
export interface EventStream {
  messages(): AsyncIterable<StreamMessage>;
  close(): void;
}

/** Custom title bar (desktop only). */
export interface TitleBarApi {
  /** Mirror the in-app location in the native window title. */
  setTitle(title: string): void;
  /**
   * True when the shell draws macOS traffic lights over the page, so the
   * title bar must indent its leading content. False on other platforms;
   * the web platform exposes no window capability at all.
   */
  readonly trafficLightInset: boolean;
}

/** One native menu entry wired to the command registry. */
export interface MenuCommand {
  id: string;
  label: string;
  shortcut?: string;
}

/** Native menu bar (desktop only). */
export interface MenuApi {
  setCommands(commands: MenuCommand[]): void;
}

/** pidash:// deep links (desktop only). */
export interface DeepLinkApi {
  onOpenUrl(callback: (url: string) => void): Unsubscribe;
}

/** Bundled or managed agent runner controls (desktop only). */
export interface AgentRuntimeApi {
  /** True when a runner is reachable from this build. */
  isAvailable(): Promise<boolean>;
}

/** Desktop auto-updater (desktop only). */
export interface UpdaterApi {
  checkNow(): Promise<boolean>;
}

export interface Platform {
  kind: "web" | "desktop";
  /** HTTP transport. Desktop routes API calls through the Rust layer. */
  fetch: typeof fetch;
  /** Server-sent events. Desktop relays through its streaming command. */
  stream(url: string, init?: StreamInit): EventStream;
  /** Open a URL in the system browser (OAuth, downloads, external docs). */
  openExternal(url: string): Promise<void>;
  /** Query cache persistence and UI preferences. */
  storage: KeyValueStore;
  /** Window focus, for refetch timing and presence. */
  onFocusChange(callback: (focused: boolean) => void): Unsubscribe;
  window?: TitleBarApi;
  menu?: MenuApi;
  deepLinks?: DeepLinkApi;
  agentRuntime?: AgentRuntimeApi;
  updates?: UpdaterApi;
}
