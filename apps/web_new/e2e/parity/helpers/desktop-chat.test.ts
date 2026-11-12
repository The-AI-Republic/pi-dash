// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// The judgments DESK-009's send leg makes about recorded desktop state: which
// stored conversation the address points at, and whether this leg's own turn
// reached the on-machine store.
import { describe, expect, it } from "vitest";

import { appendedTurns, invocationCount, sessionToSendInto } from "./desktop-chat.js";
import type { DesktopInvocation } from "../desktop/tauri-mock.js";

function append(sessionId: string, content: string, role = "user"): DesktopInvocation {
  return { command: "chat_append_event", args: { sessionId, event: { role, content } } };
}

describe("sessionToSendInto", () => {
  it("keeps an address the store still carries", () => {
    expect(sessionToSendInto("chat-2", ["chat-1", "chat-2"])).toBe("chat-2");
  });

  it("rejects an address left behind by a clear", () => {
    expect(sessionToSendInto("chat-2", ["chat-3"])).toBeNull();
  });

  it("rejects a missing address and an empty store", () => {
    expect(sessionToSendInto(null, ["chat-1"])).toBeNull();
    expect(sessionToSendInto("chat-1", [])).toBeNull();
  });
});

describe("appendedTurns", () => {
  it("counts only this session's exact user turn", () => {
    const invocations = [
      append("chat-1", "surviving turn"),
      append("chat-1", "surviving turn"),
      append("chat-2", "surviving turn"),
      append("chat-1", "another turn"),
      append("chat-1", "surviving turn", "assistant"),
      { command: "chat_send", args: { chatSessionId: "chat-1" } },
    ];
    expect(appendedTurns(invocations, "chat-1", "surviving turn")).toBe(2);
    expect(appendedTurns(invocations, "chat-2", "surviving turn")).toBe(1);
  });

  it("find nothing in a leg that stored no turn", () => {
    expect(appendedTurns([], "chat-1", "surviving turn")).toBe(0);
    expect(appendedTurns([{ command: "chat_send", args: {} }], "chat-1", "surviving turn")).toBe(0);
  });

  it("ignore an append recorded without args or with a foreign shape", () => {
    const invocations: DesktopInvocation[] = [
      { command: "chat_append_event", args: null },
      { command: "chat_append_event" },
      { command: "chat_append_event", args: { sessionId: "chat-1" } },
    ];
    expect(appendedTurns(invocations, "chat-1", "surviving turn")).toBe(0);
  });
});

describe("invocationCount", () => {
  it("counts the named command and ignores the rest", () => {
    const invocations = [
      { command: "chat_send", args: {} },
      append("chat-1", "one"),
      { command: "chat_send", args: {} },
    ];
    expect(invocationCount(invocations, "chat_send")).toBe(2);
    expect(invocationCount(invocations, "chat_append_event")).toBe(1);
    expect(invocationCount([], "chat_send")).toBe(0);
  });
});
