// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Native-shell transport and tool bindings (DESK-020/021) driven through the
// globals the desktop shell installs: the advertised API origin and the
// Tauri core. Each test proves one browser-observable behavior — the native
// path only for API addresses on that origin, the frame's timeout, multipart
// boundaries, the abort/timeout/network taxonomy, and a stream that resumes
// from its last event id but stops for good on a hard answer.

import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import * as tauri from "./tauri.js";

type InvokeFn = (command: string, args?: unknown) => Promise<unknown>;
type Listener = (event: { payload: unknown }) => void;

const NATIVE_ORIGIN = "http://localhost:8000";

class FakeChannel<T> {
  onmessage: ((message: T) => void) | null = null;

  push(message: T): void {
    this.onmessage?.(message);
  }
}

function installBridge(invoke: InvokeFn, listen?: (event: string, handler: Listener) => Promise<() => void>): void {
  const win = globalThis.window as unknown as Record<string, unknown>;
  win["__PIDASH_NATIVE_HTTP__"] = NATIVE_ORIGIN;
  win["__TAURI__"] = {
    core: { invoke, Channel: FakeChannel },
    ...(listen === undefined ? {} : { event: { listen } }),
  };
}

function removeBridge(): void {
  const win = globalThis.window as unknown as Record<string, unknown>;
  delete win["__PIDASH_NATIVE_HTTP__"];
  delete win["__TAURI__"];
}

function frameHead(frame: unknown): Record<string, unknown> {
  const bytes = frame as Uint8Array;
  const headLength = new DataView(bytes.buffer, bytes.byteOffset, bytes.byteLength).getUint32(0);
  return JSON.parse(new TextDecoder().decode(bytes.subarray(4, 4 + headLength))) as Record<string, unknown>;
}

function frameBody(frame: unknown): Uint8Array {
  const bytes = frame as Uint8Array;
  const headLength = new DataView(bytes.buffer, bytes.byteOffset, bytes.byteLength).getUint32(0);
  return bytes.subarray(4 + headLength);
}

function nativeResponse(status: number, body: string, contentType = "application/json"): ArrayBuffer {
  const encoded = new TextEncoder().encode(body);
  const bytes = tauri.encodeFrame({ status, headers: [["content-type", contentType]] }, encoded);
  return bytes.buffer.slice(bytes.byteOffset, bytes.byteOffset + bytes.byteLength) as ArrayBuffer;
}

beforeEach(() => {
  removeBridge();
});

afterEach(() => {
  removeBridge();
  vi.useRealTimers();
  vi.restoreAllMocks();
});

describe("desktop fetch transport", () => {
  it("sends API calls on the advertised origin through the native frame", async () => {
    const invoke = vi.fn(async () => nativeResponse(200, "{}"));
    installBridge(invoke);
    const response = await tauri.platform.fetch(`${NATIVE_ORIGIN}/api/v1/x/?a=1`);
    expect(invoke).toHaveBeenCalledTimes(1);
    const [command, frame] = invoke.mock.calls[0] as unknown as [string, Uint8Array];
    expect(command).toBe("desktop_api_request");
    expect(frameHead(frame)).toMatchObject({
      url: `${NATIVE_ORIGIN}/api/v1/x/?a=1`,
      method: "GET",
      hasBody: false,
      timeoutMs: null,
    });
    expect(await response.text()).toBe("{}");
  });

  it("resolves relative addresses against the page and forwards the deadline", async () => {
    const invoke = vi.fn(async (_command: string, _args?: unknown) => nativeResponse(204, ""));
    installBridge(invoke);
    // A page served from the API origin (bundled dev) resolves the relative
    // address onto it, exactly as the webview would.
    vi.stubGlobal("location", new URL(`${NATIVE_ORIGIN}/acme/browse/ACME-1`));
    try {
      await tauri.platform.fetch("/api/v1/y/", { method: "DELETE", timeoutMs: 2500 });
      const frame = invoke.mock.calls[0]?.[1] as Uint8Array;
      expect(frameHead(frame)).toMatchObject({
        url: `${NATIVE_ORIGIN}/api/v1/y/`,
        method: "DELETE",
        timeoutMs: 2500,
      });
    } finally {
      vi.unstubAllGlobals();
    }
  });

  it("advertises the native API origin for the client base address", () => {
    installBridge(async () => undefined);
    expect(tauri.platform.apiOrigin).toBe(NATIVE_ORIGIN);
    removeBridge();
    expect(tauri.platform.apiOrigin).toBe(globalThis.location.origin);
  });

  it("derives the multipart boundary the server needs to parse the body", async () => {
    const invoke = vi.fn(async (_command: string, _args?: unknown) => nativeResponse(200, "{}"));
    installBridge(invoke);
    // A caller that set a bare multipart Content-Type must not cost the
    // body's own boundary: the derived type replaces it.
    const form = new FormData();
    form.append("file", new Blob(["audio"], { type: "audio/webm" }), "dictation.webm");
    await tauri.platform.fetch(`${NATIVE_ORIGIN}/api/upload/`, {
      method: "POST",
      headers: { "content-type": "multipart/form-data" },
      body: form,
    });
    const frame = invoke.mock.calls[0]?.[1] as Uint8Array;
    const headers = frameHead(frame)["headers"] as Array<[string, string]>;
    const contentType = headers.find(([name]) => name.toLowerCase() === "content-type")?.[1] ?? "";
    expect(contentType).toContain("boundary=");
    expect(new TextDecoder().decode(frameBody(frame))).toContain('name="file"');
    expect(frameHead(frame)["hasBody"]).toBe(true);
  });

  it("keeps the page transport for exts outside the API allowlist", async () => {
    const nativeInvoke = vi.fn(async () => nativeResponse(200, "{}"));
    const browserFetch = vi.fn(async () => new Response("browser"));
    installBridge(nativeInvoke);
    vi.stubGlobal("fetch", browserFetch);
    try {
      const storage = await tauri.platform.fetch("https://storage.example.test/upload?sig=1");
      expect(await storage.text()).toBe("browser");
      const page = await tauri.platform.fetch(`${NATIVE_ORIGIN}/static/app.js`);
      expect(await page.text()).toBe("browser");
    } finally {
      vi.unstubAllGlobals();
    }
    expect(browserFetch).toHaveBeenCalledTimes(2);
    expect(nativeInvoke).not.toHaveBeenCalled();
  });

  it("maps a blown deadline to a TimeoutError, never a session failure", async () => {
    installBridge(async () => {
      throw new Error("API request timed out");
    });
    const failure = await tauri.platform.fetch(`${NATIVE_ORIGIN}/api/v1/slow/`).catch((error: unknown) => error);
    expect(failure).toBeInstanceOf(DOMException);
    expect((failure as DOMException).name).toBe("TimeoutError");
  });

  it("maps a native cancel to an AbortError", async () => {
    installBridge(async () => {
      throw new Error("API request canceled");
    });
    const failure = await tauri.platform.fetch(`${NATIVE_ORIGIN}/api/v1/slow/`).catch((error: unknown) => error);
    expect(failure).toBeInstanceOf(DOMException);
    expect((failure as DOMException).name).toBe("AbortError");
  });

  it("maps an unroutable host to a network TypeError", async () => {
    installBridge(async () => {
      throw new Error("dns lookup failed");
    });
    const failure = await tauri.platform.fetch(`${NATIVE_ORIGIN}/api/v1/x/`).catch((error: unknown) => error);
    expect(failure).toBeInstanceOf(TypeError);
    expect((failure as Error).message).toContain("dns lookup failed");
  });

  it("cancels the native request when the caller aborts", async () => {
    const commands: string[] = [];
    let rejectRequest: ((error: Error) => void) | undefined;
    installBridge(async (command) => {
      commands.push(command);
      if (command === "desktop_api_cancel") return undefined;
      return new Promise((_resolve, reject) => {
        rejectRequest = reject;
      });
    });
    const controller = new AbortController();
    const pending = tauri.platform.fetch(`${NATIVE_ORIGIN}/api/v1/x/`, { signal: controller.signal });
    controller.abort();
    rejectRequest?.(new Error("API request canceled"));
    await expect(pending).rejects.toMatchObject({ name: "AbortError" });
    expect(commands).toContain("desktop_api_cancel");
  });

  it("refuses an already-aborted request without invoking the bridge", async () => {
    const invoke = vi.fn(async () => nativeResponse(200, "{}"));
    installBridge(invoke);
    const controller = new AbortController();
    controller.abort();
    await expect(
      tauri.platform.fetch(`${NATIVE_ORIGIN}/api/v1/x/`, { signal: controller.signal })
    ).rejects.toMatchObject({ name: "AbortError" });
    expect(invoke).not.toHaveBeenCalled();
  });
});

describe("desktop event stream", () => {
  type StreamArgs = { request: { id: string; headers: Array<[string, string]> }; channel: FakeChannel<unknown> };

  it("streams parsed events and reconnects with the last event id", async () => {
    vi.useFakeTimers();
    const requests: StreamArgs[] = [];
    installBridge(async (command, args) => {
      if (command === "desktop_api_stream") requests.push(args as StreamArgs);
      return undefined;
    });
    const stream = tauri.platform.stream(`${NATIVE_ORIGIN}/api/v1/events/`);
    const iterator = stream.messages()[Symbol.asyncIterator]();
    const pending = iterator.next();
    await Promise.resolve();
    expect(requests).toHaveLength(1);
    const first = requests[0]!;
    first.channel.push({ type: "head", status: 200, headers: [["content-type", "text/event-stream"]] });
    first.channel.push({ type: "data", text: "id: 7\ndata: hello\n\n" });
    await expect(pending).resolves.toEqual({ done: false, value: { event: "message", data: "hello" } });
    // End of body: a recoverable ending, so the stream waits the retry gap
    // and reconnects carrying the id it dispatched.
    first.channel.push({ type: "end" });
    await vi.advanceTimersByTimeAsync(tauri.STREAM_RECONNECT_MS);
    expect(requests).toHaveLength(2);
    const headers = requests[1]!.request.headers;
    expect(headers).toContainEqual(["Last-Event-ID", "7"]);
    expect(stream.error).toBeUndefined();
    stream.close();
  });

  it("closes for good on a non-success answer", async () => {
    const requests: StreamArgs[] = [];
    installBridge(async (command, args) => {
      if (command === "desktop_api_stream") requests.push(args as StreamArgs);
      return undefined;
    });
    const stream = tauri.platform.stream(`${NATIVE_ORIGIN}/api/v1/events/`);
    const iterator = stream.messages()[Symbol.asyncIterator]();
    const next = iterator.next();
    requests[0]!.channel.push({ type: "head", status: 500, headers: [["content-type", "text/event-stream"]] });
    await expect(next).resolves.toMatchObject({ done: true });
    expect(stream.error).toBeInstanceOf(Error);
    expect(stream.error?.message).toContain("500");
  });

  it("closes for good when the answer is not an event stream", async () => {
    const requests: StreamArgs[] = [];
    installBridge(async (command, args) => {
      if (command === "desktop_api_stream") requests.push(args as StreamArgs);
      return undefined;
    });
    const stream = tauri.platform.stream(`${NATIVE_ORIGIN}/api/v1/events/`);
    const iterator = stream.messages()[Symbol.asyncIterator]();
    const next = iterator.next();
    requests[0]!.channel.push({ type: "head", status: 200, headers: [["content-type", "application/json"]] });
    await expect(next).resolves.toMatchObject({ done: true });
    expect(stream.error).toBeInstanceOf(Error);
  });

  it("reconnects after a dropped transport without reporting a hard error", async () => {
    vi.useFakeTimers();
    const requests: StreamArgs[] = [];
    installBridge(async (command, args) => {
      if (command === "desktop_api_stream") {
        requests.push(args as StreamArgs);
        return Promise.reject(new Error("API stream failed"));
      }
      return undefined;
    });
    const stream = tauri.platform.stream(`${NATIVE_ORIGIN}/api/v1/events/`);
    void stream.messages()[Symbol.asyncIterator]().next();
    await vi.advanceTimersByTimeAsync(0);
    expect(stream.error).toBeUndefined();
    await vi.advanceTimersByTimeAsync(tauri.STREAM_RECONNECT_MS);
    expect(requests.length).toBeGreaterThan(1);
    stream.close();
  });

  it("cancels the open request when the caller closes or aborts", async () => {
    const commands: string[] = [];
    installBridge(async (command) => {
      commands.push(command);
      return undefined;
    });
    const controller = new AbortController();
    const stream = tauri.platform.stream(`${NATIVE_ORIGIN}/api/v1/events/`, { signal: controller.signal });
    void stream.messages()[Symbol.asyncIterator]().next();
    await Promise.resolve();
    stream.close();
    controller.abort();
    expect(commands.filter((name) => name === "desktop_api_cancel")).toHaveLength(1);
  });
});

describe("desktop CLI tool bindings", () => {
  it("detects the tool and normalises a partial answer", async () => {
    installBridge(async () => ({ installed: true, path: "/usr/local/bin/pidash", version: "1.2.3" }));
    await expect(tauri.platform.cliSetup?.detect()).resolves.toEqual({
      installed: true,
      path: "/usr/local/bin/pidash",
      version: "1.2.3",
    });
    installBridge(async () => ({ installed: false, path: "", version: null }));
    await expect(tauri.platform.cliSetup?.detect()).resolves.toEqual({ installed: false, path: null, version: null });
  });

  it("installs once, reports the new state and refuses a racing install", async () => {
    let release: (() => void) | undefined;
    const commands: string[] = [];
    installBridge(async (command) => {
      commands.push(command);
      if (command === "detect_pidash_cli") return { installed: false, path: null, version: null };
      if (command === "install_pidash_cli") {
        await new Promise<void>((resolve) => {
          release = resolve;
        });
        return undefined;
      }
      return undefined;
    });
    const first = tauri.platform.cliSetup!.install();
    await expect(tauri.platform.cliSetup!.install()).rejects.toThrow("already being installed");
    release?.();
    await expect(first).resolves.toEqual({ installed: false, path: null, version: null });
    expect(commands.filter((name) => name === "install_pidash_cli")).toHaveLength(1);
  });

  it("subscribes to the shell's event bus for installer output", async () => {
    const handlers = new Map<string, Listener>();
    let unlisten = 0;
    installBridge(
      async () => undefined,
      async (event, handler) => {
        handlers.set(event, handler);
        return () => {
          unlisten += 1;
        };
      }
    );
    const lines: string[] = [];
    const stop = tauri.platform.cliSetup!.onInstallLog((progress) => lines.push(progress.line));
    await Promise.resolve();
    handlers.get("pidash-install-log")?.({ payload: { line: "Installing" } });
    handlers.get("pidash-install-log")?.({ payload: { line: "Installed" } });
    handlers.get("pidash-install-log")?.({ payload: { line: 42 } });
    expect(lines).toEqual(["Installing", "Installed"]);
    stop();
    handlers.get("pidash-install-log")?.({ payload: { line: "late" } });
    expect(lines).toEqual(["Installing", "Installed"]);
    expect(unlisten).toBe(1);
  });

  it("does not subscribe on a shell without the event bus", () => {
    installBridge(async () => undefined);
    expect(() => tauri.platform.cliSetup!.onInstallLog(() => undefined)()).not.toThrow();
  });

  it("ignores a listen helper outside the event bus", async () => {
    const legacyListen = vi.fn(async () => () => undefined);
    const win = globalThis.window as unknown as Record<string, unknown>;
    win["__PIDASH_NATIVE_HTTP__"] = NATIVE_ORIGIN;
    win["__TAURI__"] = {
      core: { invoke: async () => undefined, Channel: FakeChannel, listen: legacyListen },
    };
    const lines: string[] = [];
    tauri.platform.cliSetup!.onInstallLog((progress) => lines.push(progress.line))();
    await Promise.resolve();
    expect(legacyListen).not.toHaveBeenCalled();
    expect(lines).toEqual([]);
  });

  it("hands the approved device code to the tool with its address and workspace", async () => {
    const invoke = vi.fn(async () => undefined);
    installBridge(invoke);
    await tauri.platform.cliSetup!.login({
      deviceCode: "device-1",
      cloudUrl: "https://app.example.test",
      workspace: "acme",
    });
    expect(invoke).toHaveBeenCalledWith("pidash_cli_login", {
      deviceCode: "device-1",
      cloudUrl: "https://app.example.test",
      workspace: "acme",
    });
  });
});

describe("desktop shareable addresses", () => {
  it("refuses without a baked hosted origin", () => {
    expect(() => tauri.platform.shareableUrl("/acme/browse/ACME-1/")).toThrow("hosted web origin");
  });
});
