// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Browser platform: plain browser primitives. No desktop code may be
// imported here; the no-Tauri bundle check enforces that.

import { createPersistentStore } from "./storage.js";
import type { EventStream, Platform, StreamInit, StreamMessage, Unsubscribe } from "./types.js";

function parseEventBlock(block: string): StreamMessage | undefined {
  let event = "message";
  const lines: string[] = [];
  for (const line of block.split("\n")) {
    if (line.startsWith("event:")) event = line.slice("event:".length).trim();
    else if (line.startsWith("data:")) lines.push(line.slice("data:".length).replace(/^ /, ""));
  }
  if (lines.length === 0) return undefined;
  return { event, data: lines.join("\n") };
}

async function* readMessages(response: Response, signal?: AbortSignal): AsyncIterable<StreamMessage> {
  const body = response.body;
  if (!body) return;
  const reader = body.getReader();
  const decoder = new TextDecoder();
  let buffer = "";
  try {
    for (;;) {
      if (signal?.aborted) return;
      const { done, value } = await reader.read();
      buffer += decoder.decode(value ?? new Uint8Array(), { stream: !done });
      if (done) break;
      const blocks = buffer.split("\n\n");
      buffer = blocks.pop() ?? "";
      for (const block of blocks) {
        const message = parseEventBlock(block);
        if (message) yield message;
      }
    }
    const tail = parseEventBlock(buffer);
    if (tail) yield tail;
  } finally {
    reader.releaseLock();
  }
}

function browserStream(url: string, init: StreamInit = {}): EventStream {
  const controller = new AbortController();
  const onAbort = (): void => controller.abort(init.signal?.reason);
  init.signal?.addEventListener("abort", onAbort, { once: true });
  let closed = false;

  const response = globalThis.fetch(url, {
    method: init.method ?? "GET",
    headers: { Accept: "text/event-stream", ...(init.headers ?? {}) },
    signal: controller.signal,
    credentials: "include",
    ...(init.body === undefined ? {} : { body: init.body }),
  });

  const messages = async function* (): AsyncIterable<StreamMessage> {
    try {
      const resolved = await response;
      if (!resolved.ok) return;
      yield* readMessages(resolved, controller.signal);
    } catch {
      return;
    } finally {
      init.signal?.removeEventListener("abort", onAbort);
    }
  };

  return {
    messages,
    close: () => {
      if (!closed) {
        closed = true;
        controller.abort(new Error("Stream closed"));
      }
    },
  };
}

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

export const platform: Platform = {
  kind: "web",
  fetch: (...args) => globalThis.fetch(...args),
  stream: (url, init) => browserStream(url, init),
  openExternal: (url) => {
    globalThis.window.open(url, "_blank", "noopener");
    return Promise.resolve();
  },
  storage: createPersistentStore("pidash.web"),
  onFocusChange: trackFocus,
};
