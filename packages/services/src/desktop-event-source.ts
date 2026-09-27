/**
 * Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
 * SPDX-License-Identifier: AGPL-3.0-only
 * See the LICENSE file for details.
 */

import { cancelNativeRequest, getNativeApi, nativeApiUrl, nextNativeRequestId } from "./desktop-api-adapter";
import type { NativeApi, NativeResponseHead } from "./desktop-api-adapter";

/** The EventSource surface the chat UIs use. */
export interface ApiEventSource {
  addEventListener(type: string, listener: (event: Event) => void): void;
  removeEventListener(type: string, listener: (event: Event) => void): void;
  close(): void;
}

type StreamEvent = ({ type: "head" } & NativeResponseHead) | { type: "data"; text: string } | { type: "end" };

/** Open a credentialed Server-Sent Events stream to the API.
 *
 * The desktop webview can omit the API's cookies on EventSource just as on
 * XHR, so desktop binaries with the native cookie transport stream through
 * it instead. Everywhere else this is a plain EventSource.
 */
export function createApiEventSource(url: string, init?: EventSourceInit): ApiEventSource {
  const native = getNativeApi();
  const target = native && init?.withCredentials ? nativeApiUrl(native, url) : undefined;
  if (!native || !target) return new EventSource(url, init);
  return new NativeEventSource(native, target.href);
}

const CONNECTING = 0;
const OPEN = 1;
const CLOSED = 2;

/** EventSource semantics (WHATWG HTML §9.2) over the native stream command:
 * reconnects with Last-Event-ID after network errors or end of stream, and
 * gives up on a non-200 or non-event-stream response.
 */
class NativeEventSource extends EventTarget implements ApiEventSource {
  readyState = CONNECTING;
  readonly withCredentials = true;
  private lastEventId = "";
  private retry = 3000;
  private requestId?: string;
  private timer?: ReturnType<typeof setTimeout>;
  // Parser state for the current connection.
  private buffer = "";
  private eventType = "";
  private data = "";
  private idBuffer = "";

  constructor(
    private readonly native: NativeApi,
    readonly url: string
  ) {
    super();
    this.connect();
  }

  close(): void {
    this.readyState = CLOSED;
    if (this.timer) clearTimeout(this.timer);
    if (this.requestId) cancelNativeRequest(this.native, this.requestId);
    this.requestId = undefined;
  }

  private connect(): void {
    const id = nextNativeRequestId();
    this.requestId = id;
    this.buffer = "";
    this.eventType = "";
    this.data = "";
    this.idBuffer = this.lastEventId;
    const current = () => this.requestId === id && this.readyState !== CLOSED;
    const headers: [string, string][] = [
      ["Accept", "text/event-stream"],
      ["Cache-Control", "no-cache"],
    ];
    if (this.lastEventId) headers.push(["Last-Event-ID", this.lastEventId]);
    const channel = new this.native.core.Channel<StreamEvent>();
    // Tauri's Channel exposes only `onmessage`, not addEventListener.
    // eslint-disable-next-line unicorn/prefer-add-event-listener
    channel.onmessage = (message) => {
      if (!current()) return;
      if (message.type === "head") {
        const type = message.headers.find(([name]) => name.toLowerCase() === "content-type")?.[1] ?? "";
        if (message.status !== 200 || !type.toLowerCase().startsWith("text/event-stream")) {
          this.fail();
          return;
        }
        this.readyState = OPEN;
        this.dispatchEvent(new Event("open"));
      } else if (message.type === "data") {
        this.feed(message.text);
      } else {
        this.reconnect();
      }
    };
    this.native.core
      .invoke("desktop_api_stream", {
        request: { id, url: this.url, method: "GET", headers, timeoutMs: null },
        channel,
      })
      .catch(() => {
        if (current()) this.reconnect();
      });
  }

  private reconnect(): void {
    if (this.readyState === CLOSED) return;
    this.requestId = undefined;
    this.readyState = CONNECTING;
    this.dispatchEvent(new Event("error"));
    if (this.readyState === CLOSED) return;
    this.timer = setTimeout(() => this.connect(), this.retry);
  }

  private fail(): void {
    this.close();
    this.dispatchEvent(new Event("error"));
  }

  private feed(text: string): void {
    this.buffer += text;
    if (this.buffer.charCodeAt(0) === 0xfeff) this.buffer = this.buffer.slice(1);
    for (;;) {
      const match = /\r\n|\r|\n/.exec(this.buffer);
      // A trailing \r may be the first half of \r\n; wait for the next chunk.
      if (!match || (match[0] === "\r" && match.index === this.buffer.length - 1)) return;
      const line = this.buffer.slice(0, match.index);
      this.buffer = this.buffer.slice(match.index + match[0].length);
      this.line(line);
      if (this.readyState === CLOSED) return;
    }
  }

  private line(line: string): void {
    if (line === "") {
      this.lastEventId = this.idBuffer;
      if (this.data) {
        const event = new MessageEvent(this.eventType || "message", {
          data: this.data.slice(0, -1),
          lastEventId: this.lastEventId,
          origin: new URL(this.url).origin,
        });
        this.dispatchEvent(event);
      }
      this.eventType = "";
      this.data = "";
      return;
    }
    if (line.startsWith(":")) return;
    const colon = line.indexOf(":");
    const field = colon < 0 ? line : line.slice(0, colon);
    let value = colon < 0 ? "" : line.slice(colon + 1);
    if (value.startsWith(" ")) value = value.slice(1);
    if (field === "event") this.eventType = value;
    else if (field === "data") this.data += `${value}\n`;
    else if (field === "id" && !value.includes("\0")) this.idBuffer = value;
    else if (field === "retry" && /^\d+$/.test(value)) this.retry = Number(value);
  }
}
