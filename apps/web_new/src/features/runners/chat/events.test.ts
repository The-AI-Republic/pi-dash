// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import type { ChatEvent, ChatMessage } from "@pidash/api-client";
import { describe, expect, it } from "vitest";

import {
  ACTIVITY_STRIP_LIMIT,
  activityLabel,
  activityRows,
  appendEvent,
  applyAssistantDelta,
  applyDeltaOnce,
  assistantDeltaText,
  isRealtimeEvent,
  isTerminalEvent,
  lastSequence,
  refreshesHistory,
} from "./events.js";

const STAMP = "2026-10-19T10:00:00Z";
const SINCE = Date.parse(STAMP);

function event(overrides: Partial<ChatEvent> = {}): ChatEvent {
  return {
    id: 1,
    session: "s-1",
    message: "m-1",
    seq: 1,
    kind: "assistant_delta",
    payload: { params: { delta: "hi" } },
    created_at: STAMP,
    ...overrides,
  };
}

function message(overrides: Partial<ChatMessage> = {}): ChatMessage {
  return {
    id: "m-1",
    session: "s-1",
    role: "assistant",
    content: "hello",
    content_parts: [],
    status: "streaming",
    local_item_id: "",
    local_turn_id: "",
    seq: 2,
    created_at: STAMP,
    completed_at: null,
    ...overrides,
  };
}

describe("assistantDeltaText", () => {
  it("reads a string delta, a nested text and a bare text", () => {
    expect(assistantDeltaText({ params: { delta: "a" } })).toBe("a");
    expect(assistantDeltaText({ params: { delta: { text: "b" } } })).toBe("b");
    expect(assistantDeltaText({ params: { text: "c" } })).toBe("c");
  });

  it("answers empty for anything else", () => {
    expect(assistantDeltaText({})).toBe("");
    expect(assistantDeltaText({ params: null })).toBe("");
    expect(assistantDeltaText({ params: ["x"] })).toBe("");
    expect(assistantDeltaText({ params: { delta: 4 } })).toBe("");
    expect(assistantDeltaText({ params: { delta: { text: 4 } } })).toBe("");
    expect(assistantDeltaText({ params: { delta: [1] } })).toBe("");
  });
});

describe("isRealtimeEvent", () => {
  it("accepts a frame inside the grace window and refuses an older replay", () => {
    expect(isRealtimeEvent({ created_at: STAMP }, SINCE)).toBe(true);
    expect(isRealtimeEvent({ created_at: "2026-10-19T09:59:59.600Z" }, SINCE)).toBe(true);
    expect(isRealtimeEvent({ created_at: "2026-10-19T09:00:00Z" }, SINCE)).toBe(false);
    expect(isRealtimeEvent({ created_at: "not a date" }, SINCE)).toBe(false);
  });
});

describe("event classification", () => {
  it("knows terminal and refresh frames", () => {
    for (const kind of ["turn_completed", "chat_failed", "chat_closed"]) expect(isTerminalEvent(kind)).toBe(true);
    expect(isTerminalEvent("assistant_delta")).toBe(false);
    for (const kind of ["turn_started", "turn_completed", "chat_failed", "chat_closed", "chat_warmed"]) {
      expect(refreshesHistory(kind)).toBe(true);
    }
    expect(refreshesHistory("assistant_delta")).toBe(false);
  });
});

describe("applyAssistantDelta", () => {
  it("grows a streaming row and starts a new bubble for an unseen row", () => {
    const grown = applyAssistantDelta([message()], event(), "s-1");
    expect(grown).toHaveLength(1);
    expect(grown[0]!.content).toBe("hellohi");

    const added = applyAssistantDelta([], event({ message: "m-9", created_at: STAMP }), "s-1");
    expect(added).toEqual([
      expect.objectContaining({
        id: "m-9",
        session: "s-1",
        role: "assistant",
        content: "hi",
        status: "streaming",
        seq: 1,
        created_at: STAMP,
      }),
    ]);
  });

  it("leaves a finished row untouched", () => {
    const done = message({ status: "completed" });
    expect(applyAssistantDelta([done], event(), "s-1")).toEqual([done]);
  });

  it("ignores a session-level frame, an empty delta and an absent message", () => {
    expect(applyAssistantDelta([message()], event({ message: null }), "s-1")).toEqual([message()]);
    expect(applyAssistantDelta([], event({ payload: { params: {} } }), "s-1")).toEqual([]);
    const messages = applyAssistantDelta([], event({ payload: { params: {} }, message: null }), "s-1");
    expect(messages).toEqual([]);
  });
});

describe("activityLabel", () => {
  it("names approvals, failures and unknown kinds, and hides lifecycle narration", () => {
    expect(activityLabel({ kind: "chat_approval_request", payload: {} })).toBe("Waiting for your approval");
    expect(activityLabel({ kind: "chat_failed", payload: {} })).toBe("Failed");
    expect(activityLabel({ kind: "turn_started", payload: {} })).toBeNull();
    expect(activityLabel({ kind: "message_started", payload: {} })).toBeNull();
    expect(activityLabel({ kind: "run_started", payload: {} })).toBeNull();
    expect(activityLabel({ kind: "turn_completed", payload: {} })).toBeNull();
    expect(activityLabel({ kind: "chat_timing", payload: {} })).toBeNull();
    expect(activityLabel({ kind: "custom_kind", payload: {} })).toBe("custom_kind");
  });

  it("reads raw command and file frames with their running/completed wording", () => {
    const raw = (method: string, item: Record<string, unknown>, extra: Record<string, unknown> = {}) =>
      activityLabel({ kind: "raw", payload: { method, params: { item, ...extra } } });
    expect(raw("item/started", { type: "commandExecution", command: "ls" })).toBe("Running: ls");
    expect(raw("item/completed", { type: "commandExecution", command: "ls" })).toBe("Ran: ls");
    expect(raw("item/completed", { type: "commandExecution" })).toBe("Ran: command");
    expect(raw("item/started", { type: "fileChange", path: "src/a.ts" })).toBe("Editing src/a.ts");
    expect(raw("item/completed", { type: "fileChange", path: "src/a.ts" })).toBe("Edited src/a.ts");
    expect(raw("item/completed", { type: "fileChange" })).toBe("Edited a file");
    expect(raw("item/started", { type: "customTool" })).toBe("Started customTool");
    expect(raw("item/completed", { type: "customTool" })).toBe("Finished customTool");
  });

  it("hides transcript rows and empty item types", () => {
    for (const type of ["userMessage", "agentMessage", "reasoning"]) {
      expect(
        activityLabel({ kind: "raw", payload: { method: "item/completed", params: { item: { type } } } })
      ).toBeNull();
    }
    expect(activityLabel({ kind: "raw", payload: { method: "item/completed", params: { item: {} } } })).toBeNull();
  });

  it("reads warnings", () => {
    expect(activityLabel({ kind: "raw", payload: { method: "warning", params: { message: "long turn" } } })).toBe(
      "Warning: long turn"
    );
    expect(activityLabel({ kind: "raw", payload: { method: "warning", params: {} } })).toBe("Warning");
    expect(activityLabel({ kind: "raw", payload: { method: "warning", params: { message: "" } } })).toBe("Warning");
  });

  it("hides lifecycle methods but keeps unknown ones", () => {
    for (const method of [
      "thread/started",
      "turn/started",
      "turn/completed",
      "remoteControl/status/changed",
      "thread/status/changed",
      "thread/tokenUsage/updated",
      "account/rateLimits/updated",
      "mcpServer/startupStatus/updated",
    ]) {
      expect(activityLabel({ kind: "raw", payload: { method } })).toBeNull();
    }
    expect(activityLabel({ kind: "raw", payload: { method: "item/custom" } })).toBe("item/custom");
  });

  it("tolerates malformed raw payloads", () => {
    expect(activityLabel({ kind: "raw", payload: {} })).toBeNull();
    expect(activityLabel({ kind: "raw", payload: { method: 3, params: "x" } })).toBeNull();
    expect(activityLabel({ kind: "raw", payload: { method: "item/started", params: { item: null } } })).toBeNull();
    expect(activityLabel({ kind: "raw", payload: { method: "item/started", params: { item: [1] } } })).toBeNull();
    expect(activityLabel({ kind: "raw", payload: { method: "warning", params: { message: 3 } } })).toBe("Warning");
  });
});

describe("activityRows", () => {
  it("caps to the last few and collapses a started/completed pair", () => {
    const events = [
      {
        seq: 1,
        kind: "raw",
        payload: { method: "item/started", params: { item: { type: "commandExecution", command: "a" } } },
      },
      {
        seq: 2,
        kind: "raw",
        payload: { method: "item/completed", params: { item: { type: "commandExecution", command: "a" } } },
      },
      {
        seq: 3,
        kind: "raw",
        payload: { method: "item/completed", params: { item: { type: "commandExecution", command: "b" } } },
      },
    ];
    expect(activityRows(events)).toEqual([
      { seq: 2, label: "Ran: a" },
      { seq: 3, label: "Ran: b" },
    ]);
  });

  it("keeps only the newest entries when there are more than the cap", () => {
    const events = Array.from({ length: ACTIVITY_STRIP_LIMIT + 4 }, (_, index) => ({
      seq: index + 1,
      kind: "raw",
      payload: { method: "item/completed", params: { item: { type: "commandExecution", command: `c${index + 1}` } } },
    }));
    const rows = activityRows(events);
    expect(rows).toHaveLength(ACTIVITY_STRIP_LIMIT);
    expect(rows[0]!.label).toBe("Ran: c5");
    expect(rows[rows.length - 1]!.label).toBe("Ran: c10");
  });

  it("drops unlabelled frames", () => {
    expect(activityRows([{ seq: 1, kind: "turn_started", payload: {} }])).toEqual([]);
    expect(activityRows([{ seq: 1, kind: "raw", payload: {} }])).toEqual([]);
  });

  it("keys a raw row by its action and a named row by its label", () => {
    // A raw frame with nothing usable in its params still keys by the
    // action type alone, so a later completed frame collapses onto it.
    expect(
      activityRows([
        { seq: 1, kind: "raw", payload: { method: "warning", params: "x" } },
        { seq: 2, kind: "raw", payload: { method: "warning", params: { item: [1] } } },
      ])
    ).toEqual([{ seq: 2, label: "Warning" }]);
    // A frame with no label at all keys to the empty string.
    expect(
      activityRows([
        { seq: 1, kind: "raw", payload: { method: "item/started", params: { item: { type: "userMessage" } } } },
      ])
    ).toEqual([]);
    // Non-raw frames are identified by their label, so a repeat reads once.
    expect(
      activityRows([
        { seq: 3, kind: "custom_kind", payload: {} },
        { seq: 4, kind: "custom_kind", payload: {} },
      ])
    ).toEqual([{ seq: 4, label: "custom_kind" }]);
  });

  it("collapses a started pair without a command, and names the action by kind", () => {
    const pair = [
      { seq: 1, kind: "raw", payload: { method: "item/started", params: { item: { type: "customTool" } } } },
      { seq: 2, kind: "raw", payload: { method: "item/completed", params: { item: { type: "customTool" } } } },
    ];
    expect(activityRows(pair)).toEqual([{ seq: 2, label: "Finished customTool" }]);
    expect(activityRows([{ seq: 3, kind: "custom_kind", payload: {} }])).toEqual([{ seq: 3, label: "custom_kind" }]);
  });
});

describe("appendEvent", () => {
  it("adds a new sequence once and ignores a redelivery", () => {
    const first = appendEvent([], { seq: 1 });
    expect(first).toEqual([{ seq: 1 }]);
    expect(appendEvent(first, { seq: 1 })).toEqual([{ seq: 1 }]);
    expect(appendEvent(first, { seq: 2 })).toEqual([{ seq: 1 }, { seq: 2 }]);
  });
});

describe("lastSequence", () => {
  it("takes the highest seen or applied sequence", () => {
    expect(lastSequence([{ seq: 2 }, { seq: 7 }], new Set([3]))).toBe(7);
    expect(lastSequence([], new Set([4, 9]))).toBe(9);
    expect(lastSequence([], new Set())).toBe(0);
  });
});

describe("applyDeltaOnce", () => {
  it("applies a new live delta and records its sequence", () => {
    const result = applyDeltaOnce({
      applied: new Set(),
      event: event({ seq: 5 }),
      subscribedAtMs: SINCE,
      messages: [message()],
      sessionId: "s-1",
    });
    expect(result.applied.has(5)).toBe(true);
    expect(result.messages[0]!.content).toBe("hellohi");
  });

  it("drops a redelivered sequence", () => {
    const result = applyDeltaOnce({
      applied: new Set([5]),
      event: event({ seq: 5 }),
      subscribedAtMs: SINCE,
      messages: [],
      sessionId: "s-1",
    });
    expect(result.messages).toEqual([]);
  });

  it("marks a replayed delta without appending it", () => {
    const result = applyDeltaOnce({
      applied: new Set(),
      event: event({ seq: 6, created_at: "2026-10-19T08:00:00Z" }),
      subscribedAtMs: SINCE,
      messages: [],
      sessionId: "s-1",
    });
    expect(result.applied.has(6)).toBe(true);
    expect(result.messages).toEqual([]);
  });
});
