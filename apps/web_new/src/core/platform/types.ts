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
  /**
   * Why the stream stopped for good, when it did: a non-success status or a
   * response that is not an event stream. Endings a reconnect can recover
   * from (end of body, transport drop) never set it.
   */
  readonly error: Error | undefined;
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
 * Local chat over the bundled daemon (RUN-033-036). The engine runs on
 * this machine and is reached through native IPC, never a cloud relay;
 * the store below is the per-account local transcript the chat page
 * renders. Sessions always read back open and carry the working
 * directory, engine thread and last-write time the page needs.
 */
export interface LocalChatSessionRecord {
  id: string;
  title: string;
  /** Workspace the conversation belongs to. */
  workspace: string;
  /** Runner id rides here; the store has no dedicated runner column. */
  project: string;
  working_dir: string;
  engine_version: string;
  /** Engine thread the session resumes into, null before the first turn. */
  engine_thread_id: string | null;
  created_at: number;
  updated_at: number;
}

/** One transcript row as the local store keeps it (unix milliseconds). */
export interface LocalChatEventRecord {
  id: string;
  session_id: string;
  seq: number;
  role: string;
  content: string;
  tool_calls: string | null;
  approval_decision: string | null;
  created_at: number;
}

/** Fields the store takes when opening a conversation. */
export interface LocalChatNewSession {
  workspace: string;
  /** Runner id, stored in the record's `project` column. */
  runner: string;
}

/** One streamed engine frame, already narrowed by the native side. */
export type LocalChatFrame =
  | {
      result: "chat_started";
      data: { chat_session_id: string; local_thread_id: string; local_session_id?: string; started_at: string };
    }
  | { result: "chat_message_started"; data: { chat_session_id: string; message_id: string; turn_id?: string } }
  | {
      result: "chat_event";
      data: { chat_session_id: string; kind: string; payload: Record<string, unknown> };
    }
  | {
      result: "chat_approval_request";
      data: {
        chat_session_id: string;
        local_approval_id: string;
        kind: string;
        payload: Record<string, unknown>;
        reason?: string;
        expires_at?: string;
      };
    }
  | {
      result: "chat_message_completed";
      data: {
        chat_session_id: string;
        message_id: string;
        turn_id?: string;
        assistant_message?: string;
        status: string;
      };
    }
  | { result: "chat_failed"; data: { chat_session_id: string; code: string; detail?: string } }
  | { result: "chat_closed"; data: { chat_session_id: string } };

/** Which inline decision the user made (RUN-034). */
export type LocalChatDecision = "accept" | "decline" | "accept_for_session";

/**
 * The bundled daemon's chat surface (RUN-033-036). Every turn is scoped
 * to one workspace; `mode` is the approval mode captured when the
 * engine thread spawns, and `localThreadId` resumes an existing one.
 */
export interface LocalChatApi {
  listSessions(account: string): Promise<LocalChatSessionRecord[]>;
  createSession(account: string, input: LocalChatNewSession): Promise<LocalChatSessionRecord>;
  getSession(account: string, sessionId: string): Promise<LocalChatSessionRecord | null>;
  listEvents(account: string, sessionId: string): Promise<LocalChatEventRecord[]>;
  appendEvent(
    account: string,
    sessionId: string,
    event: { role: string; content: string }
  ): Promise<LocalChatEventRecord>;
  setThreadId(account: string, sessionId: string, engineThreadId: string): Promise<void>;
  warm(input: {
    workspaceSlug: string;
    sessionId: string;
    cwd: string | undefined;
    mode: string;
    localThreadId: string | undefined;
  }): Promise<void>;
  send(input: {
    workspaceSlug: string;
    sessionId: string;
    messageId: string;
    content: string;
    cwd: string | undefined;
    mode: string;
    localThreadId: string | undefined;
  }): Promise<void>;
  cancel(input: { workspaceSlug: string; sessionId: string; reason: string | undefined }): Promise<void>;
  close(input: { workspaceSlug: string; sessionId: string }): Promise<void>;
  decide(input: {
    workspaceSlug: string;
    sessionId: string;
    localApprovalId: string;
    decision: LocalChatDecision;
  }): Promise<void>;
  /**
   * Follow the engine's frames and transport failures. Returns the
   * unsubscribe; calling it twice is safe.
   */
  subscribe(handlers: {
    onFrame: (frame: LocalChatFrame) => void;
    onError: (event: { chat_session_id: string; message: string }) => void;
  }): Unsubscribe;
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

/**
 * An update the shell is holding for the sidebar control (DESK-018): either
 * deferred at launch or found by the daily check. The native side owns both
 * facts, including whether a download is under way, so a control that
 * mounts mid-install reads its busy state from here, not from memory.
 */
export interface PendingUpdate {
  /** The release waiting to be installed. */
  version: string;
  /** The release the app is running right now. */
  currentVersion: string;
  /** True while the shell downloads and installs this release. */
  installing: boolean;
}

/** What the bundled command-line tool reports about itself. */
export interface CliStatus {
  installed: boolean;
  /** Absolute path of the binary when found. */
  path: string | null;
  /** Version the binary reported; null when the probe failed. */
  version: string | null;
}

/** One plain-text line of the installer's output. */
export interface CliInstallProgress {
  /** Raw line as the installer wrote it; render as text, never as HTML. */
  line: string;
}

/**
 * The bundled command-line tool (desktop only). Detection and installation
 * run natively; login completes through the same device-grant exchange the
 * tool owns, so the credential never passes through the page.
 */
export interface CliSetupApi {
  /** Probe for the tool without touching the machine. */
  detect(): Promise<CliStatus>;
  /**
   * Install from the app's bundle (or the pinned installer). Rejects when
   * another install is already running. Progress arrives on `onInstallLog`.
   */
  install(): Promise<CliStatus>;
  /** Subscribe to installer output; render each line verbatim, in order. */
  onInstallLog(callback: (progress: CliInstallProgress) => void): Unsubscribe;
  /** Hand an approved device code to the tool, which owns the credential. */
  login(input: { deviceCode: string; cloudUrl: string; workspace: string }): Promise<void>;
}

/**
 * Desktop auto-updater (desktop only). The launch prompt and the daily
 * check live in the shell; the page asks what waits, installs it on
 * request, and follows later announcements (DESK-016/017/018).
 */
export interface UpdaterApi {
  /** The release waiting for the user, or null when the app is current. */
  pending(): Promise<PendingUpdate | null>;
  /**
   * Download and install the waiting release. It resolves only when the
   * install failed; a successful install restarts the app instead.
   */
  install(): Promise<void>;
  /**
   * Follow announcements of updates found while the app runs, including
   * the re-announcement after a failed install. Returns the unsubscribe.
   */
  onAvailable(callback: (update: PendingUpdate) => void): Unsubscribe;
}

/**
 * Request init for the platform transport. The desktop layer enforces
 * `timeoutMs` itself; browser fetch ignores what it does not know, so a
 * caller that sets only the deadline behaves the same on both platforms.
 */
export interface FetchInit extends RequestInit {
  timeoutMs?: number | undefined;
}

/** Fetch-compatible function; the desktop implementation adds the deadline. */
export type PlatformFetch = (input: RequestInfo | URL, init?: FetchInit) => Promise<Response>;

export interface Platform {
  kind: "web" | "desktop";
  /**
   * Origin the API answers on. The web bundle serves app and API from one
   * origin, so this is the page origin; the desktop bundle advertises the
   * API origin through the shell, because its own origin is private.
   */
  readonly apiOrigin: string | undefined;
  /** HTTP transport. Desktop routes API calls through the Rust layer. */
  fetch: PlatformFetch;
  /** Server-sent events. Desktop relays through its streaming command. */
  stream(url: string, init?: StreamInit): EventStream;
  /** Open a URL in the system browser (OAuth, downloads, external docs). */
  openExternal(url: string): Promise<void>;
  /**
   * Absolute address a copied in-app link should carry (DESK-019). The
   * desktop bundle runs on a private origin, so its platform resolves
   * paths against the configured hosted app instead and refuses when
   * there is none; the web platform keeps the page origin.
   */
  shareableUrl(path: string): string;
  /** Query cache persistence and UI preferences. */
  storage: KeyValueStore;
  /** Window focus, for refetch timing and presence. */
  onFocusChange(callback: (focused: boolean) => void): Unsubscribe;
  window?: TitleBarApi;
  menu?: MenuApi;
  deepLinks?: DeepLinkApi;
  agentRuntime?: AgentRuntimeApi;
  localChat?: LocalChatApi;
  cliSetup?: CliSetupApi;
  updates?: UpdaterApi;
  sessionTeardown?: SessionTeardownApi;
}
