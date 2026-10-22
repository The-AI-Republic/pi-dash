// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Desktop platform: API traffic goes through the Rust HTTP layer (cookies
// stay out of the page), streams relay through its streaming command, and
// storage persists through the webview profile (file-backed under the app
// data directory). This module is excluded from the web bundle by the
// build-time alias; the no-Tauri bundle check proves it.

import { desktopShareUrl } from "./shareUrl.js";
import { createPersistentStore } from "./storage.js";
import { isWebAddress } from "./urls.js";
import type {
  AgentEngineProfile,
  AgentHostCheck,
  AgentRuntimeApi,
  CliSetupApi,
  CliStatus,
  DeepLinkApi,
  EventStream,
  FetchInit,
  LocalChatApi,
  LocalChatDecision,
  LocalChatFrame,
  LocalChatNewSession,
  PendingUpdate,
  Platform,
  SessionTeardownApi,
  ShellSignInApi,
  StreamInit,
  StreamMessage,
  TitleBarApi,
  UpdaterApi,
  Unsubscribe,
} from "./types.js";

/** Subset of the Tauri webview globals this module touches. */
interface TauriInvokeCore {
  invoke<T>(command: string, payload?: unknown): Promise<T>;
  Channel: new <T>() => { onmessage: ((message: T) => void) | null };
}

interface TauriEventApi {
  listen<T>(event: string, handler: (event: { payload: T }) => void): Promise<Unsubscribe>;
}

interface TauriWindow {
  __PIDASH_NATIVE_HTTP__?: unknown;
  __TAURI__?: { core?: TauriInvokeCore; event?: TauriEventApi };
}

function tauriWindow(): (Window & TauriWindow) | undefined {
  if (typeof globalThis.window === "undefined") return undefined;
  return globalThis.window as Window & TauriWindow;
}

/** The desktop shell advertises its API origin; without it we are not desktop. */
function nativeApiOrigin(): { origin: string; core: TauriInvokeCore } | undefined {
  const win = tauriWindow();
  const origin = win?.__PIDASH_NATIVE_HTTP__;
  const core = win?.__TAURI__?.core;
  if (typeof origin !== "string" || !core) return undefined;
  return { origin, core };
}

function isApiPath(pathname: string): boolean {
  return pathname.startsWith("/api/") || pathname.startsWith("/auth/");
}

let requestSequence = 0;

function nextRequestId(): string {
  requestSequence += 1;
  return `${Date.now().toString(36)}.${requestSequence.toString(36)}`;
}

const textEncoder = new TextEncoder();
const textDecoder = new TextDecoder();

/** Wire framing: 4-byte big-endian head length, JSON head, raw body. */
export function encodeFrame(head: unknown, body: Uint8Array | null): Uint8Array {
  const headBytes = textEncoder.encode(JSON.stringify(head));
  const out = new Uint8Array(4 + headBytes.length + (body?.length ?? 0));
  new DataView(out.buffer).setUint32(0, headBytes.length);
  out.set(headBytes, 4);
  if (body) out.set(body, 4 + headBytes.length);
  return out;
}

export function decodeFrame(bytes: Uint8Array): { head: unknown; body: Uint8Array } {
  if (bytes.length < 4) throw new Error("Truncated response frame");
  const headLength = new DataView(bytes.buffer, bytes.byteOffset, bytes.byteLength).getUint32(0);
  if (bytes.length < 4 + headLength) throw new Error("Truncated response frame");
  const head = JSON.parse(textDecoder.decode(bytes.subarray(4, 4 + headLength))) as unknown;
  return { head, body: bytes.subarray(4 + headLength) };
}

interface NativeResponseHead {
  status?: number;
  statusText?: string;
  status_text?: string;
  headers?: Array<[string, string]>;
}

function headToResponse(head: NativeResponseHead, body: Uint8Array): Response {
  const headers = new Headers();
  for (const [name, value] of head.headers ?? []) {
    try {
      headers.append(name, value);
    } catch {
      // Skip response headers the page cannot represent.
    }
  }
  const status = head.status ?? 200;
  const statusText = head.statusText ?? head.status_text ?? "";
  return new Response(status === 204 || status === 304 ? null : (body as unknown as BodyInit), {
    status,
    statusText,
    headers,
  });
}

/** RequestInit plus the deadline the native layer enforces itself. */
type DesktopRequestInit = FetchInit;

/** Abort reason a caller sees as an abort (never as a transport failure). */
function abortReason(signal: AbortSignal): unknown {
  return signal.reason ?? new DOMException("Aborted", "AbortError");
}

/**
 * Map the native layer's failure strings onto the errors a browser fetch
 * would raise: the native timeout verb as TimeoutError, the cancel verb as
 * AbortError, everything else as a network TypeError. A transport failure
 * must never look like a session problem.
 */
function nativeTransportError(error: unknown, signal: AbortSignal | null): unknown {
  if (signal?.aborted) return abortReason(signal);
  const message = error instanceof Error ? error.message : String(error);
  if (message === "API request timed out") return new DOMException("The request timed out", "TimeoutError");
  if (message === "API request canceled") return new DOMException("The request was aborted", "AbortError");
  return new TypeError(`Network request failed: ${message}`);
}

/** A boundary no form value can collide with. */
function multipartBoundary(): string {
  const random =
    typeof globalThis.crypto?.randomUUID === "function"
      ? globalThis.crypto.randomUUID().replace(/-/g, "")
      : Math.random().toString(36).slice(2) + Date.now().toString(36);
  return `----pidash${random}`;
}

/**
 * Encode a FormData body as multipart bytes with its own boundary. The
 * webview's Response would derive one, but doing it here keeps the exact
 * same bytes (and the Content-Type the server parses) in every runtime,
 * including the unit-test DOM where Response cannot derive it.
 */
async function encodeMultipart(form: FormData): Promise<{ bytes: Uint8Array; contentType: string }> {
  const boundary = multipartBoundary();
  const parts: Uint8Array[] = [];
  const push = (text: string): void => {
    parts.push(textEncoder.encode(text));
  };
  for (const [name, value] of form.entries()) {
    push(`--${boundary}\r\n`);
    if (typeof value === "string") {
      push(`Content-Disposition: form-data; name="${name.replace(/"/g, "%22")}"\r\n\r\n`);
      push(`${value}\r\n`);
      continue;
    }
    const type = value.type === "" ? "application/octet-stream" : value.type;
    push(
      `Content-Disposition: form-data; name="${name.replace(/"/g, "%22")}"; filename="${value.name.replace(/"/g, "%22")}"\r\n`
    );
    push(`Content-Type: ${type}\r\n\r\n`);
    parts.push(new Uint8Array(await new Response(value).arrayBuffer()));
    push("\r\n");
  }
  push(`--${boundary}--\r\n`);
  const length = parts.reduce((total, part) => total + part.length, 0);
  const bytes = new Uint8Array(length);
  let offset = 0;
  for (const part of parts) {
    bytes.set(part, offset);
    offset += part.length;
  }
  return { bytes, contentType: `multipart/form-data; boundary=${boundary}` };
}

async function nativeRequest(
  core: TauriInvokeCore,
  url: string,
  init: DesktopRequestInit | undefined,
  signal: AbortSignal | null
): Promise<Response> {
  const id = nextRequestId();
  const method = (init?.method ?? "GET").toUpperCase();
  const headers = new Headers(init?.headers);
  let bodyBytes: Uint8Array | null = null;
  if (init?.body != null) {
    const suppliedType = headers.get("content-type") ?? "";
    if (typeof FormData !== "undefined" && init.body instanceof FormData) {
      // A multipart body carries its own boundary, so a caller's bare
      // multipart Content-Type can never hide it from the server.
      const encoded = await encodeMultipart(init.body);
      headers.set("content-type", encoded.contentType);
      bodyBytes = encoded.bytes;
    } else {
      // Response turns Blob, URLSearchParams and strings into their wire
      // bytes and names the type; keep a caller's explicit type.
      const encoded = new Response(init.body as BodyInit, init.headers === undefined ? {} : { headers: init.headers });
      const derivedType = encoded.headers.get("content-type");
      if (derivedType !== null && suppliedType === "") headers.set("content-type", derivedType);
      bodyBytes = new Uint8Array(await encoded.arrayBuffer());
    }
  }
  const pairs: Array<[string, string]> = [];
  headers.forEach((value, name) => pairs.push([name, value]));
  const frame = encodeFrame(
    {
      id,
      url,
      method,
      headers: pairs,
      hasBody: bodyBytes !== null,
      timeoutMs: init?.timeoutMs != null && init.timeoutMs > 0 ? init.timeoutMs : null,
    },
    bodyBytes
  );
  const cancel = (): void => {
    core.invoke("desktop_api_cancel", { id }).catch(() => undefined);
  };
  if (signal?.aborted) throw abortReason(signal);
  signal?.addEventListener("abort", cancel, { once: true });
  try {
    const raw = await core.invoke<ArrayBuffer | number[]>("desktop_api_request", frame);
    const bytes = raw instanceof ArrayBuffer ? new Uint8Array(raw) : new Uint8Array(raw);
    const { head, body } = decodeFrame(bytes);
    return headToResponse(head as NativeResponseHead, body);
  } catch (error) {
    throw nativeTransportError(error, signal);
  } finally {
    signal?.removeEventListener("abort", cancel);
  }
}

function desktopFetch(input: RequestInfo | URL, init?: DesktopRequestInit): Promise<Response> {
  const native = nativeApiOrigin();
  const url = typeof input === "string" ? input : input instanceof URL ? input.toString() : input.url;
  const request = input instanceof Request ? input : undefined;
  const signal = init?.signal ?? request?.signal ?? null;
  if (!native) return globalThis.fetch(input, init as RequestInit);
  let target: URL | undefined;
  try {
    target = new URL(url, globalThis.location.href);
  } catch {
    return globalThis.fetch(input, init as RequestInit);
  }
  if (target.origin !== native.origin || !isApiPath(target.pathname)) {
    // Presigned storage uploads and anything off the API allowlist keep the
    // page transport, with its own progress and cancellation behavior.
    return globalThis.fetch(input, init as RequestInit);
  }
  const merged: DesktopRequestInit = {
    method: init?.method ?? request?.method ?? "GET",
    timeoutMs: init?.timeoutMs,
  };
  const headers = init?.headers ?? request?.headers;
  if (headers !== undefined) {
    merged.headers = headers;
  }
  const body = init?.body ?? request?.body;
  if (body !== undefined && body !== null) {
    merged.body = body;
  }
  return nativeRequest(native.core, target.toString(), merged, signal);
}

type NativeStreamEvent = {
  type: string;
  status?: number;
  statusText?: string;
  status_text?: string;
  headers?: Array<[string, string]>;
  text?: string;
};

/** Reconnect delay after a recoverable stream ending (EventSource default). */
export const STREAM_RECONNECT_MS = 3000;

/** Whole UTF-8 lines assembled from the native stream's text chunks. */
class LineBuffer {
  private buffer = "";

  reset(): void {
    this.buffer = "";
  }

  push(chunk: string): string[] {
    this.buffer += chunk;
    if (this.buffer.charCodeAt(0) === 0xfeff) this.buffer = this.buffer.slice(1);
    const lines: string[] = [];
    for (;;) {
      const match = /\r\n|\r|\n/.exec(this.buffer);
      // A trailing \r may be the first half of \r\n; wait for more text.
      if (!match || (match[0] === "\r" && match.index === this.buffer.length - 1)) break;
      lines.push(this.buffer.slice(0, match.index));
      this.buffer = this.buffer.slice(match.index + match[0].length);
    }
    return lines;
  }
}

/**
 * SSE parser state across one connection: dispatches an event on the blank
 * line that ends a block, carries the event id forward for reconnects, and
 * honors a server-sent retry gap.
 */
class EventStreamParser {
  lastEventId = "";
  retryMs = STREAM_RECONNECT_MS;
  private eventType = "";
  private data: string[] = [];
  private idBuffer = "";

  constructor(private readonly emit: (message: StreamMessage) => void) {}

  start(lastEventId: string): void {
    this.eventType = "";
    this.data = [];
    this.idBuffer = lastEventId;
  }

  line(line: string): void {
    if (line === "") {
      this.lastEventId = this.idBuffer;
      if (this.data.length > 0) {
        this.emit({ event: this.eventType || "message", data: this.data.join("\n") });
      }
      this.eventType = "";
      this.data = [];
      return;
    }
    if (line.startsWith(":")) return;
    const colon = line.indexOf(":");
    const field = colon < 0 ? line : line.slice(0, colon);
    let value = colon < 0 ? "" : line.slice(colon + 1);
    if (value.startsWith(" ")) value = value.slice(1);
    if (field === "event") this.eventType = value;
    else if (field === "data") this.data.push(value);
    else if (field === "id" && !value.includes("\0")) this.idBuffer = value;
    else if (field === "retry" && /^\d+$/.test(value)) this.retryMs = Number(value);
  }
}

/**
 * Native event stream with the browser EventSource's resilience: a
 * transport drop or end of body reconnects with Last-Event-ID after the
 * retry gap, while a non-success status or a non-event-stream answer closes
 * it for good (surfaced through `error`). The bundle origin makes the
 * webview's own EventSource unusable for credentialed API streams.
 */
function desktopStream(url: string, init: StreamInit = {}): EventStream {
  const native = nativeApiOrigin();
  if (!native) {
    throw new Error("Desktop streams need the desktop shell");
  }
  const core = native.core;
  const queue: StreamMessage[] = [];
  const parser = new EventStreamParser((message) => {
    queue.push(message);
    wake();
  });
  const lineBuffer = new LineBuffer();
  let waiting: (() => void) | null = null;
  let closed = false;
  let finished = false;
  let reconnectTimer: ReturnType<typeof setTimeout> | undefined;
  let requestId: string | undefined;
  let fatal: Error | undefined;

  function wake(): void {
    const notify = waiting;
    waiting = null;
    notify?.();
  }

  function stopForGood(error: Error): void {
    fatal = error;
    finished = true;
    requestId = undefined;
    wake();
  }

  function connect(): void {
    if (closed) return;
    const id = nextRequestId();
    requestId = id;
    lineBuffer.reset();
    parser.start(parser.lastEventId);
    const current = (): boolean => requestId === id && !closed && fatal === undefined;
    const headers: Record<string, string> = {
      Accept: "text/event-stream",
      "Cache-Control": "no-cache",
      ...(init.headers ?? {}),
    };
    if (parser.lastEventId !== "") headers["Last-Event-ID"] = parser.lastEventId;
    const channel = new core.Channel<NativeStreamEvent>();
    channel.onmessage = (message) => {
      if (!current()) return;
      if (message.type === "head") {
        const status = message.status ?? 200;
        const type = (message.headers ?? []).find(([name]) => name.toLowerCase() === "content-type")?.[1] ?? "";
        if (status < 200 || status >= 300 || !type.toLowerCase().startsWith("text/event-stream")) {
          stopForGood(new Error(`Stream failed with status ${status}`));
          return;
        }
      } else if (message.type === "data") {
        for (const line of lineBuffer.push(message.text ?? "")) parser.line(line);
        wake();
      } else {
        // End of body: the server closed cleanly; reconnect and resume.
        requestId = undefined;
        reconnect();
      }
    };
    core
      .invoke("desktop_api_stream", {
        request: {
          id,
          url,
          method: init.method ?? "GET",
          headers: Object.entries(headers),
          hasBody: init.body != null,
          timeoutMs: null,
        },
        channel,
      })
      .catch((error: unknown) => {
        if (!current()) return;
        requestId = undefined;
        reconnect();
        void error;
      });
  }

  function reconnect(): void {
    // Both callers guard with current(), which is false once closed.
    reconnectTimer = setTimeout(() => {
      reconnectTimer = undefined;
      connect();
    }, parser.retryMs);
  }

  const messages = async function* (): AsyncIterable<StreamMessage> {
    try {
      connect();
      for (;;) {
        if (closed) return;
        const next = queue.shift();
        if (next !== undefined) {
          yield next;
          continue;
        }
        if (finished) return;
        await new Promise<void>((resolve) => {
          waiting = resolve;
        });
      }
    } finally {
      close();
    }
  };

  function close(): void {
    if (closed) return;
    closed = true;
    finished = true;
    if (reconnectTimer !== undefined) clearTimeout(reconnectTimer);
    const id = requestId;
    requestId = undefined;
    if (id !== undefined) core.invoke("desktop_api_cancel", { id }).catch(() => undefined);
    wake();
  }

  init.signal?.addEventListener("abort", close, { once: true });
  return {
    messages,
    close,
    get error() {
      return fatal;
    },
  };
}

/**
 * The shell draws macOS traffic lights over the page, so the title bar
 * must leave room for them. Read from the browser UA surface (no Tauri
 * dependency in this module): userAgentData first, legacy platform after.
 */
function isMacOs(): boolean {
  const nav = globalThis.navigator as (Navigator & { userAgentData?: { platform?: string } }) | undefined;
  if (!nav) return false;
  const hinted = nav.userAgentData?.platform;
  if (typeof hinted === "string" && hinted.length > 0) return hinted.toLowerCase().startsWith("mac");
  return (nav.platform ?? "").toLowerCase().startsWith("mac");
}

const desktopWindow: TitleBarApi = {
  trafficLightInset: isMacOs(),
  setTitle(title: string): void {
    if (typeof globalThis.document !== "undefined") {
      globalThis.document.title = title;
    }
    // Best-effort native title (the shell starts on "Pi Dash"): a missing
    // or older shell rejects the invoke and the document title still wins.
    const core = nativeApiOrigin()?.core;
    if (!core) return;
    core.invoke("plugin:window|set_title", { label: "main", value: title }).catch(() => undefined);
  },
};

function trackFocus(callback: (focused: boolean) => void): Unsubscribe {
  if (typeof globalThis.window === "undefined") return () => undefined;
  const report = (): void => callback(!globalThis.document.hidden);
  globalThis.window.addEventListener("focus", report);
  globalThis.window.addEventListener("blur", report);
  globalThis.document.addEventListener("visibilitychange", report);
  return () => {
    globalThis.window.removeEventListener("focus", report);
    globalThis.window.removeEventListener("blur", report);
    globalThis.document.removeEventListener("visibilitychange", report);
  };
}

/** Invoke a native command. Rejects without the desktop bridge. */
function invokeNative<T>(command: string, args?: Record<string, unknown>): Promise<T> {
  const native = nativeApiOrigin();
  if (!native) {
    return Promise.reject(new Error(`${command} needs the native bridge`));
  }
  return native.core.invoke<T>(command, args);
}

interface NativeHostCheck {
  runner_present?: unknown;
  engine_present?: unknown;
  host_label?: unknown;
}

function toHostCheck(raw: NativeHostCheck): AgentHostCheck {
  return {
    runnerPresent: raw.runner_present === true,
    enginePresent: raw.engine_present === true,
    hostLabel: typeof raw.host_label === "string" ? raw.host_label : "",
  };
}

/** Grace period the daemon stop waits when the caller names none. */
export const DAEMON_STOP_GRACE_SECONDS = 5;

const desktopAgentRuntime: AgentRuntimeApi = {
  isAvailable: async () => {
    try {
      const check = toHostCheck(await invokeNative<NativeHostCheck>("managed_doctor"));
      return check.runnerPresent && check.enginePresent;
    } catch {
      return false;
    }
  },
  checkHost: async () => toHostCheck(await invokeNative<NativeHostCheck>("managed_doctor")),
  writeEngineConfig: (input: { account: string; profile: AgentEngineProfile }) =>
    invokeNative("managed_write_engine_config", { account: input.account, profile: input.profile }),
  writeModelToken: (token: string) => invokeNative("managed_write_model_token", { token }),
  bootstrapMachine: (input: { cloudUrl: string; workspace: string; machineToken: string; devMachineId: string }) =>
    invokeNative("managed_bootstrap", {
      cloudUrl: input.cloudUrl,
      workspace: input.workspace,
      machineToken: input.machineToken,
      devMachineId: input.devMachineId,
    }),
  enrollProject: (input: { account: string; workspace: string; project: string; hostLabel: string }) =>
    invokeNative("managed_enroll", {
      account: input.account,
      workspace: input.workspace,
      project: input.project,
      hostLabel: input.hostLabel,
    }),
  startDaemon: (input: { account: string; workspace: string }) =>
    invokeNative("managed_start_daemon", { account: input.account, workspace: input.workspace }),
  stopDaemon: (graceSeconds?: number | undefined) =>
    invokeNative("managed_stop_daemon", { graceSeconds: graceSeconds ?? DAEMON_STOP_GRACE_SECONDS }),
  signOutNative: (input?: { deleteChatHistoryAccount?: string | undefined } | undefined) =>
    invokeNative(
      "managed_sign_out",
      input?.deleteChatHistoryAccount === undefined
        ? undefined
        : { clearChatHistory: { account: input.deleteChatHistoryAccount } }
    ),
  saveApprovalModes: (account: string, modes: Record<string, string>) =>
    invokeNative("chat_approval_modes_save", { account, modes }),
  loadApprovalModes: async (account: string) => {
    const modes = await invokeNative<Record<string, string> | null>("chat_approval_modes_load", { account });
    return modes ?? {};
  },
};

const desktopSessionTeardown: SessionTeardownApi = {
  clearLocalSession: async () => {
    // Runs after the sign-out POST, which still needs its cookie. A failed
    // wipe must not strand the user: landing on sign-in stays right, and the
    // next sign-in overwrites whatever survived. Without the bridge there is
    // no native store to wipe, so that resolves too.
    try {
      await invokeNative("desktop_clear_web_data");
    } catch {
      // Nothing useful to do — see above.
    }
  },
};

const desktopShellSignIn: ShellSignInApi = {
  get bakedOrigin() {
    // The raw advertisement, with no page-origin fallback: without it the
    // provider handoff refuses loudly instead of building a dead URL.
    const advertised = tauriWindow()?.__PIDASH_NATIVE_HTTP__;
    return typeof advertised === "string" ? advertised : undefined;
  },
  openSystemBrowser: (url) => {
    if (!isWebAddress(url)) {
      return Promise.reject(new Error("refusing to open a non-web address"));
    }
    const core = tauriWindow()?.__TAURI__?.core;
    if (!core) {
      return Promise.reject(new Error("openSystemBrowser needs the native bridge"));
    }
    // Invoke-only: a failure rejects so the caller surfaces it instead of
    // falling back to an in-window navigation the desktop flow cannot use.
    return core.invoke("plugin:opener|open_url", { url }).then(() => undefined);
  },
};

/** Emitted by the shell when an update is found or re-announced (updates.rs). */
export const UPDATE_AVAILABLE_EVENT = "updater://available";

interface NativePendingUpdate {
  version?: unknown;
  currentVersion?: unknown;
  installing?: unknown;
}

/**
 * Map the shell's pending-update payload. A malformed announcement is
 * treated as no update, so the control never names a release it cannot
 * install.
 */
export function toPendingUpdate(raw: unknown): PendingUpdate | null {
  if (typeof raw !== "object" || raw === null) return null;
  const payload = raw as NativePendingUpdate;
  if (typeof payload.version !== "string" || payload.version.length === 0) return null;
  if (typeof payload.currentVersion !== "string" || payload.currentVersion.length === 0) return null;
  return {
    version: payload.version,
    currentVersion: payload.currentVersion,
    installing: payload.installing === true,
  };
}

const desktopUpdater: UpdaterApi = {
  pending: async () => toPendingUpdate(await invokeNative<NativePendingUpdate | null>("desktop_pending_update")),
  install: async () => {
    await invokeNative("desktop_install_update");
  },
  onAvailable: (callback) => {
    const events = tauriWindow()?.__TAURI__?.event;
    if (!events) return () => undefined;
    let active = true;
    let unlisten: Unsubscribe | undefined;
    events
      .listen<NativePendingUpdate | null>(UPDATE_AVAILABLE_EVENT, (event) => {
        const update = toPendingUpdate(event.payload);
        if (active && update) callback(update);
      })
      .then(
        (stop) => {
          if (active) unlisten = stop;
          else stop();
        },
        () => undefined
      );
    return () => {
      active = false;
      unlisten?.();
      unlisten = undefined;
    };
  },
};

/** Event the desktop deep-link plugin emits when the OS hands it a URL. */
const DEEP_LINK_EVENT = "deep-link://new-url";

function deepLinkUrls(payload: unknown): string[] {
  if (typeof payload === "string") return payload.length > 0 ? [payload] : [];
  if (!Array.isArray(payload)) return [];
  return payload.filter((url): url is string => typeof url === "string" && url.length > 0);
}

const desktopDeepLinks: DeepLinkApi = {
  onOpenUrl(callback: (url: string) => void): Unsubscribe {
    const events = tauriWindow()?.__TAURI__?.event;
    if (!events) return () => undefined;

    let active = true;
    let unlisten: Unsubscribe | undefined;
    void events
      .listen<unknown>(DEEP_LINK_EVENT, (event) => {
        if (!active) return;
        for (const url of deepLinkUrls(event.payload)) callback(url);
      })
      .then((stop) => {
        if (active) unlisten = stop;
        else stop();
      })
      .catch(() => undefined);

    return () => {
      active = false;
      unlisten?.();
      unlisten = undefined;
    };
  },
};

interface NativeCliStatus {
  installed?: unknown;
  path?: unknown;
  version?: unknown;
}

function toCliStatus(raw: NativeCliStatus | null | undefined): CliStatus {
  return {
    installed: raw?.installed === true,
    path: typeof raw?.path === "string" && raw.path.length > 0 ? raw.path : null,
    version: typeof raw?.version === "string" && raw.version.length > 0 ? raw.version : null,
  };
}

/** Event the native installer streams its output lines on. */
export const CLI_INSTALL_LOG_EVENT = "pidash-install-log";

/** One install at a time: the native command refuses a racing second call. */
let installInFlight = false;

const desktopCliSetup: CliSetupApi = {
  detect: async () => toCliStatus(await invokeNative<NativeCliStatus>("detect_pidash_cli")),
  install: async () => {
    if (installInFlight) throw new Error("The command-line tool is already being installed.");
    installInFlight = true;
    try {
      await invokeNative("install_pidash_cli");
      return toCliStatus(await invokeNative<NativeCliStatus>("detect_pidash_cli"));
    } finally {
      installInFlight = false;
    }
  },
  onInstallLog: (callback) => {
    // The shell exposes the event bus as __TAURI__.event (Tauri 2); without
    // it there is nothing to subscribe to.
    const events = tauriWindow()?.__TAURI__?.event;
    if (!events) return () => undefined;
    let cancelled = false;
    let stop: Unsubscribe | undefined;
    void events
      .listen<{ line?: unknown }>(CLI_INSTALL_LOG_EVENT, (event) => {
        if (cancelled) return;
        if (typeof event.payload?.line === "string") callback({ line: event.payload.line });
      })
      .then(
        (unlisten) => {
          if (cancelled) void unlisten();
          else stop = unlisten;
        },
        // An older shell without the event still installs; it just shows no
        // progress lines, so the subscription stays a no-op.
        () => undefined
      );
    return () => {
      cancelled = true;
      stop?.();
    };
  },
  login: (input) =>
    invokeNative("pidash_cli_login", {
      deviceCode: input.deviceCode,
      cloudUrl: input.cloudUrl,
      workspace: input.workspace,
    }),
};

/** Native event carrying one streamed engine frame (chat.rs). */
export const CHAT_FRAME_EVENT = "chat://frame";
/** Native event carrying a transport-level failure for one session. */
export const CHAT_ERROR_EVENT = "chat://error";

/**
 * Listen to a native event, when the shell exposes its event bus. Without
 * one the subscription is inert: frames simply never arrive.
 */
function listenNative<T>(event: string, handler: (payload: T) => void): Promise<Unsubscribe> {
  const events = tauriWindow()?.__TAURI__?.event;
  if (!events) return Promise.resolve(() => undefined);
  return events.listen<T>(event, (message) => handler(message.payload));
}

const desktopLocalChat: LocalChatApi = {
  listSessions: (account) => invokeNative("chat_list_sessions", { account }),
  createSession: (account, input: LocalChatNewSession) =>
    invokeNative("chat_create_session", {
      account,
      session: { workspace: input.workspace, project: input.runner },
    }),
  getSession: (account, sessionId) => invokeNative("chat_get_session", { account, sessionId }),
  listEvents: (account, sessionId) => invokeNative("chat_list_events", { account, sessionId }),
  appendEvent: (account, sessionId, event) =>
    invokeNative("chat_append_event", {
      account,
      sessionId,
      event: { role: event.role, content: event.content },
    }),
  setThreadId: (account, sessionId, engineThreadId) =>
    invokeNative("chat_set_thread_id", { account, sessionId, engineThreadId }),
  warm: (input) =>
    invokeNative("chat_warm", {
      workspace: input.workspaceSlug,
      chatSessionId: input.sessionId,
      cwd: input.cwd,
      mode: input.mode,
      localThreadId: input.localThreadId,
    }),
  send: (input) =>
    invokeNative("chat_send", {
      workspace: input.workspaceSlug,
      chatSessionId: input.sessionId,
      messageId: input.messageId,
      content: input.content,
      cwd: input.cwd,
      mode: input.mode,
      localThreadId: input.localThreadId,
    }),
  cancel: (input) =>
    invokeNative("chat_cancel", {
      workspace: input.workspaceSlug,
      chatSessionId: input.sessionId,
      reason: input.reason,
    }),
  close: (input) => invokeNative("chat_close", { workspace: input.workspaceSlug, chatSessionId: input.sessionId }),
  decide: (input: { workspaceSlug: string; sessionId: string; localApprovalId: string; decision: LocalChatDecision }) =>
    invokeNative("chat_decide", {
      workspace: input.workspaceSlug,
      chatSessionId: input.sessionId,
      localApprovalId: input.localApprovalId,
      decision: input.decision,
    }),
  subscribe: ({ onFrame, onError }) => {
    let cancelled = false;
    const stops: Unsubscribe[] = [];
    const track = (promise: Promise<Unsubscribe>): void => {
      void promise.then((stop) => (cancelled ? stop() : stops.push(stop)));
    };
    track(listenNative<LocalChatFrame>(CHAT_FRAME_EVENT, onFrame));
    track(listenNative<{ chat_session_id: string; message: string }>(CHAT_ERROR_EVENT, onError));
    return () => {
      cancelled = true;
      for (const stop of stops.splice(0)) stop();
    };
  },
};

export const platform: Platform = {
  kind: "desktop",
  get apiOrigin() {
    return nativeApiOrigin()?.origin ?? globalThis.window?.location?.origin;
  },
  fetch: (input, init) => desktopFetch(input, init),
  stream: (url, init) => desktopStream(url, init),
  openExternal: (url) => {
    if (!isWebAddress(url)) {
      return Promise.reject(new Error("refusing to open a non-web address"));
    }
    const core = nativeApiOrigin()?.core;
    // No native bridge: reject so the caller falls back to plain
    // navigation (AUTH-023) instead of a popup neither the shell nor
    // the parity harness can observe.
    if (!core) {
      return Promise.reject(new Error("openExternal needs the native bridge"));
    }
    return core.invoke("plugin:opener|open_url", { url }).then(
      () => undefined,
      () => {
        globalThis.window.open(url, "_blank", "noopener");
      }
    );
  },
  shareableUrl: (path) => desktopShareUrl(path),
  storage: createPersistentStore("pidash.desktop"),
  onFocusChange: trackFocus,
  window: desktopWindow,
  deepLinks: desktopDeepLinks,
  agentRuntime: desktopAgentRuntime,
  localChat: desktopLocalChat,
  updates: desktopUpdater,
  cliSetup: desktopCliSetup,
  sessionTeardown: desktopSessionTeardown,
  shellSignIn: desktopShellSignIn,
};
