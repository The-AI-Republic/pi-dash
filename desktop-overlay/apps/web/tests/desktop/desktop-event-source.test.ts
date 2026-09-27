import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { createApiEventSource } from "../../../../packages/services/src/desktop-event-source";

const api = "https://api.example.test";
const url = `${api}/api/runners/chat/sessions/s1/events/`;

type Stream = { request: any; channel: { onmessage: (message: unknown) => void }; reject: (e: unknown) => void };
let streams: Stream[] = [];
const invoke = vi.fn((command: string, args: any) => {
  if (command !== "desktop_api_stream") return Promise.resolve();
  return new Promise((_resolve, reject) => streams.push({ ...args, reject }));
});
class Channel {
  onmessage: (message: unknown) => void = () => {};
}
const head = (status = 200, type = "text/event-stream") => ({
  type: "head",
  status,
  statusText: "",
  headers: [["content-type", type]],
});

beforeEach(() => {
  vi.useFakeTimers();
  streams = [];
  invoke.mockClear();
  Object.assign(window, { __PIDASH_NATIVE_HTTP__: api, __TAURI__: { core: { invoke, Channel } } });
});
afterEach(() => {
  vi.useRealTimers();
  delete (window as any).__PIDASH_NATIVE_HTTP__;
  delete (window as any).__TAURI__;
});

describe("desktop API event streams", () => {
  it("uses the webview EventSource outside the native desktop transport", () => {
    const Native = vi.fn();
    vi.stubGlobal("EventSource", Native);
    try {
      delete (window as any).__PIDASH_NATIVE_HTTP__;
      createApiEventSource(url, { withCredentials: true });
      (window as any).__PIDASH_NATIVE_HTTP__ = api;
      createApiEventSource("https://elsewhere.test/api/events/", { withCredentials: true });
      expect(Native).toHaveBeenCalledTimes(2);
      expect(invoke).not.toHaveBeenCalled();
    } finally {
      vi.unstubAllGlobals();
    }
  });

  it("parses events split across chunks and line endings", () => {
    const source = createApiEventSource(url, { withCredentials: true });
    const events: MessageEvent[] = [];
    source.addEventListener("chat.event", (event) => events.push(event as MessageEvent));
    const [{ request, channel }] = streams;
    expect(request).toMatchObject({ url, method: "GET" });
    channel.onmessage(head());
    channel.onmessage({ type: "data", text: ": keepalive\r\nevent: chat.event\r" });
    channel.onmessage({ type: "data", text: '\nid: 7\ndata: {"seq":' });
    channel.onmessage({ type: "data", text: "7}\ndata: tail\n\n" });
    expect(events).toHaveLength(1);
    expect(events[0].data).toBe('{"seq":7}\ntail');
    expect(events[0].lastEventId).toBe("7");
  });

  it("reconnects with Last-Event-ID after the stream ends or fails", () => {
    const source = createApiEventSource(url, { withCredentials: true });
    const errors = vi.fn();
    source.addEventListener("error", errors);
    streams[0].channel.onmessage(head());
    streams[0].channel.onmessage({ type: "data", text: "retry: 500\nid: 3\ndata: x\n\n" });
    streams[0].channel.onmessage({ type: "end" });
    expect(errors).toHaveBeenCalledOnce();
    vi.advanceTimersByTime(500);
    expect(streams).toHaveLength(2);
    expect(streams[1].request.headers).toContainEqual(["Last-Event-ID", "3"]);
  });

  it("does not reconnect after a non-200 response, like EventSource", () => {
    const source = createApiEventSource(url, { withCredentials: true });
    const errors = vi.fn();
    source.addEventListener("error", errors);
    streams[0].channel.onmessage(head(403, "application/json"));
    vi.advanceTimersByTime(60_000);
    expect(errors).toHaveBeenCalledOnce();
    expect(streams).toHaveLength(1);
    expect(invoke).toHaveBeenCalledWith("desktop_api_cancel", { id: streams[0].request.id });
  });

  it("close cancels the native stream and ignores late messages", () => {
    const source = createApiEventSource(url, { withCredentials: true });
    const events = vi.fn();
    source.addEventListener("message", events);
    streams[0].channel.onmessage(head());
    source.close();
    expect(invoke).toHaveBeenCalledWith("desktop_api_cancel", { id: streams[0].request.id });
    streams[0].channel.onmessage({ type: "data", text: "data: late\n\n" });
    streams[0].reject("API request canceled");
    vi.advanceTimersByTime(60_000);
    expect(events).not.toHaveBeenCalled();
    expect(streams).toHaveLength(1);
  });
});
