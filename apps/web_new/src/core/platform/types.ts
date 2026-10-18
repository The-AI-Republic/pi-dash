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

/** What the host self-check reports: which bundled pieces exist, and this machine's label. */
export interface AgentHostCheck {
  runnerPresent: boolean;
  enginePresent: boolean;
  hostLabel: string;
}

/**
 * Engine setup the host writes from the server profile. Keys match the
 * host schema exactly (the host deserializes them as-is); the runners
 * feature maps the profile contract onto this shape.
 */
export interface AgentEngineProfile {
  available: boolean;
  base_url: string;
  model: string;
  reason_code: string;
}

/** Bundled or managed agent runner controls (desktop only). */
export interface AgentRuntimeApi {
  /** True when a runner is reachable from this build. */
  isAvailable(): Promise<boolean>;
  /** Self-check the bundled binaries and read this machine's label. */
  checkHost(): Promise<AgentHostCheck>;
  /** Write the account's engine setup from the server profile. */
  writeEngineConfig(input: { account: string; profile: AgentEngineProfile }): Promise<void>;
  /** Store the short-lived model credential for the engine. */
  writeModelToken(token: string): Promise<void>;
  /** Register this machine against a workspace (answers the daemon stdout; ignored). */
  bootstrapMachine(input: {
    cloudUrl: string;
    workspace: string;
    machineToken: string;
    devMachineId: string;
  }): Promise<void>;
  /** Register one project runner on this machine. */
  enrollProject(input: { account: string; workspace: string; project: string; hostLabel: string }): Promise<void>;
  /** Start the bundled daemon (a no-op while it runs; restarts it if it died). */
  startDaemon(input: { account: string; workspace: string }): Promise<void>;
  /** Stop the daemon, waiting up to the grace period (5 seconds when omitted). */
  stopDaemon(graceSeconds?: number | undefined): Promise<void>;
  /**
   * Native sign-out: stop the daemon and destroy every local credential.
   * Pass the account to also delete its on-machine chat history.
   */
  signOutNative(input?: { deleteChatHistoryAccount?: string | undefined } | undefined): Promise<void>;
  /** Hand approval modes to the host so they survive the webview wipe. */
  saveApprovalModes(account: string, modes: Record<string, string>): Promise<void>;
  /** Read back the modes stashed for an account (empty when there are none). */
  loadApprovalModes(account: string): Promise<Record<string, string>>;
}

/**
 * Local session teardown (desktop only). The auth area calls this after the
 * sign-out POST (AUTH-017): the daemon stop, credential destroy and server
 * enrollment removal run before the POST through the runners disposal
 * (RUN-047 — revocation needs a live session), and this hook wipes the
 * webview store the POST no longer needs. Callers swallow failures:
 * landing on sign-in stays right.
 */
export interface SessionTeardownApi {
  /** Wipe local session artifacts. Rejects when the wipe fails. */
  clearLocalSession(): Promise<void>;
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
  sessionTeardown?: SessionTeardownApi;
}
