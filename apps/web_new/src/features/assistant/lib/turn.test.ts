// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Transcript merge, delta dedupe and the poll settle rule (AGT-042/047).
import type { AssistantEvent, AssistantMessage } from "@pidash/api-client";
import { describe, expect, it } from "vitest";

import {
  EMPTY_TRANSCRIPT,
  applyAssistantEvent,
  mergeMessage,
  mergeTranscript,
  replySettled,
  streamingPlaceholder,
  transcriptRows,
} from "./turn.js";

function message(overrides: Partial<AssistantMessage> = {}): AssistantMessage {
  return {
    id: "m1",
    role: "assistant",
    content: "",
    status: "streaming",
    seq: 1,
    turn_id: null,
    payload: {},
    created_at: "2026-01-01T00:00:00Z",
    completed_at: null,
    ...overrides,
  };
}

function delta(id: string, seq: number, text: string, target = "m1"): AssistantEvent {
  return {
    id: seq,
    thread: "t1",
    message: target,
    seq,
    kind: "assistant_delta",
    payload: { turn_id: "turn", params: { delta: text } },
    created_at: "2026-01-01T00:00:00Z",
  } as AssistantEvent;
}

describe("mergeTranscript", () => {
  it("keeps streamed text that is ahead of the page", () => {
    const state = mergeMessage(EMPTY_TRANSCRIPT, message({ content: "Hello wor" }));
    const merged = mergeTranscript(state, [message({ content: "Hello " })]);
    expect(merged.rows.m1?.content).toBe("Hello wor");
  });

  it("accepts a terminal row and longer content", () => {
    const state = mergeMessage(EMPTY_TRANSCRIPT, message({ content: "Hello" }));
    expect(mergeTranscript(state, [message({ content: "Hello world" })]).rows.m1?.content).toBe("Hello world");
    expect(
      mergeTranscript(state, [message({ content: "Hi", status: "completed", completed_at: "2026-01-01T00:00:01Z" })])
        .rows.m1?.status
    ).toBe("completed");
  });

  it("returns the same state when nothing is taken", () => {
    const state = mergeMessage(EMPTY_TRANSCRIPT, message({ content: "longer", status: "completed" }));
    expect(mergeTranscript(state, [message({ content: "short" })])).toBe(state);
  });
});

describe("mergeMessage", () => {
  it("ignores a shorter streaming envelope over built text", () => {
    const state = mergeMessage(EMPTY_TRANSCRIPT, message({ content: "Hello" }));
    expect(mergeMessage(state, message({ content: "" }))).toBe(state);
  });
});

describe("applyAssistantEvent", () => {
  it("appends deltas in arrival order and skips a replayed sequence", () => {
    const seen = new Set<number>();
    let state = mergeMessage(EMPTY_TRANSCRIPT, message());
    state = applyAssistantEvent(state, delta("m1", 2, "Hel"), seen);
    state = applyAssistantEvent(state, delta("m1", 2, "Hel"), seen);
    state = applyAssistantEvent(state, delta("m1", 3, "lo"), seen);
    expect(state.rows.m1?.content).toBe("Hello");
  });

  it("ignores a delta for a row it has not seen yet", () => {
    const seen = new Set<number>();
    expect(applyAssistantEvent(EMPTY_TRANSCRIPT, delta("missing", 4, "x"), seen)).toBe(EMPTY_TRANSCRIPT);
    expect(seen.size).toBe(0);
  });

  it("ignores a non-string or empty delta", () => {
    const state = mergeMessage(EMPTY_TRANSCRIPT, message());
    const blank = { ...delta("m1", 5, ""), payload: { turn_id: "turn", params: {} } } as AssistantEvent;
    expect(applyAssistantEvent(state, blank, new Set())).toBe(state);
  });

  it("merges created, completed and tool rows", () => {
    const created = {
      id: 6,
      thread: "t1",
      message: "m1",
      seq: 6,
      kind: "message_created",
      payload: { turn_id: "turn", message: message({ content: "" }) },
      created_at: "2026-01-01T00:00:00Z",
    } as AssistantEvent;
    let state = applyAssistantEvent(EMPTY_TRANSCRIPT, created, new Set());
    expect(state.rows.m1?.status).toBe("streaming");
    const tool = {
      ...created,
      kind: "tool_call",
      payload: { turn_id: "turn", message: message({ id: "m2", role: "tool_call", content: "Search", seq: 2 }) },
    } as AssistantEvent;
    state = applyAssistantEvent(state, tool, new Set());
    expect(Object.keys(state.rows).sort()).toEqual(["m1", "m2"]);
  });

  it("keeps streamed text when a reconnect replays message_created", () => {
    const seen = new Set<number>();
    const created = {
      id: 10,
      thread: "t1",
      message: "m1",
      seq: 10,
      kind: "message_created",
      payload: { turn_id: "turn", message: message({ content: "" }) },
      created_at: "2026-01-01T00:00:00Z",
    } as AssistantEvent;
    let state = applyAssistantEvent(EMPTY_TRANSCRIPT, created, seen);
    state = applyAssistantEvent(state, delta("m1", 101, "Hello "), seen);
    state = applyAssistantEvent(state, delta("m1", 102, "world"), seen);
    // Reconnect: the server replays from zero, starting with the created row
    // exactly as it looked then, followed by the same deltas.
    state = applyAssistantEvent(state, created, seen);
    state = applyAssistantEvent(state, delta("m1", 101, "Hello "), seen);
    state = applyAssistantEvent(state, delta("m1", 102, "world"), seen);
    expect(state.rows.m1?.content).toBe("Hello world");
  });

  it("keeps a terminal reply when an older replay re-sends its created row", () => {
    const seen = new Set<number>();
    let state = mergeMessage(EMPTY_TRANSCRIPT, message({ content: "", status: "streaming" }));
    state = mergeMessage(state, message({ content: "answer", status: "completed" }));
    const replay = { ...delta("m1", 20, ""), kind: "message_created" } as AssistantEvent;
    state = applyAssistantEvent(state, replay, seen);
    expect(state.rows.m1?.status).toBe("completed");
    expect(state.rows.m1?.content).toBe("answer");
  });

  it("skips envelope-only events and message events without a row", () => {
    const turn = {
      id: 7,
      thread: "t1",
      message: null,
      seq: 7,
      kind: "turn_completed",
      payload: { turn_id: "turn", usage: {} },
      created_at: "2026-01-01T00:00:00Z",
    } as AssistantEvent;
    expect(applyAssistantEvent(EMPTY_TRANSCRIPT, turn, new Set())).toBe(EMPTY_TRANSCRIPT);
    const noRow = { ...turn, kind: "message_created", payload: { turn_id: "turn" } } as AssistantEvent;
    expect(applyAssistantEvent(EMPTY_TRANSCRIPT, noRow, new Set())).toBe(EMPTY_TRANSCRIPT);
  });
});

describe("transcriptRows", () => {
  it("orders by sequence and keeps tool rows", () => {
    let state = mergeMessage(EMPTY_TRANSCRIPT, message({ id: "b", seq: 2, role: "assistant", content: "two" }));
    state = mergeMessage(state, message({ id: "a", seq: 1, role: "user", content: "one" }));
    state = mergeMessage(state, message({ id: "c", seq: 3, role: "tool_result", content: "three" }));
    expect(transcriptRows(state).map((row) => row.id)).toEqual(["a", "b", "c"]);
  });
});

describe("replySettled", () => {
  it("settles only on a terminal assistant or error row", () => {
    expect(replySettled([])).toBe(false);
    expect(replySettled([message({ role: "user", status: "completed", seq: 1 })])).toBe(false);
    expect(replySettled([message({ role: "assistant", status: "streaming" })])).toBe(false);
    expect(replySettled([message({ role: "assistant", status: "completed" })])).toBe(true);
    expect(replySettled([message({ role: "error", status: "failed" })])).toBe(true);
    expect(
      replySettled([
        message({ id: "u", role: "user", status: "completed", seq: 1 }),
        message({ id: "e", role: "error", status: "failed", seq: 2 }),
      ])
    ).toBe(true);
  });

  it("reads the newest row, not an older streamed one", () => {
    expect(
      replySettled([
        message({ id: "a", role: "assistant", status: "streaming", seq: 1 }),
        message({ id: "e", role: "error", status: "failed", seq: 2 }),
      ])
    ).toBe(true);
  });
});

describe("streamingPlaceholder", () => {
  it("shows an ellipsis while streaming and the status otherwise", () => {
    expect(streamingPlaceholder("streaming")).toBe("…");
    expect(streamingPlaceholder("completed")).toBe("completed");
  });
});
