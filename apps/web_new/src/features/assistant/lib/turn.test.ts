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

  it("keeps streamed text when a terminal page row carries no content", () => {
    // The soft-time-limit and sweep finalizes close the open row without
    // rewriting its stored content, so the page can say failed and empty
    // while the streamed partial answer is what the reader saw.
    const state = mergeMessage(EMPTY_TRANSCRIPT, message({ content: "Partial answer" }));
    const merged = mergeTranscript(state, [message({ content: "", status: "failed" })]);
    expect(merged.rows.m1?.content).toBe("Partial answer");
    expect(merged.rows.m1?.status).toBe("failed");
  });

  it("takes an empty terminal row when there is no text to keep", () => {
    const state = mergeMessage(EMPTY_TRANSCRIPT, message({ content: "" }));
    const merged = mergeTranscript(state, [message({ content: "", status: "cancelled" })]);
    expect(merged.rows.m1?.status).toBe("cancelled");
  });
});

describe("mergeMessage", () => {
  it("ignores a shorter streaming envelope over built text", () => {
    const state = mergeMessage(EMPTY_TRANSCRIPT, message({ content: "Hello" }));
    expect(mergeMessage(state, message({ content: "" }))).toBe(state);
  });

  it("carries the visible text across an empty terminal envelope", () => {
    const state = mergeMessage(EMPTY_TRANSCRIPT, message({ content: "Partial" }));
    const merged = mergeMessage(
      state,
      message({ content: "", status: "failed", completed_at: "2026-01-01T00:00:02Z" })
    );
    expect(merged.rows.m1?.content).toBe("Partial");
    expect(merged.rows.m1?.status).toBe("failed");
  });

  it("replaces an empty streaming row with a terminal one and its text", () => {
    const state = mergeMessage(EMPTY_TRANSCRIPT, message({ content: "" }));
    const merged = mergeMessage(state, message({ content: "Answer", status: "completed" }));
    expect(merged.rows.m1?.content).toBe("Answer");
  });
});

describe("applyAssistantEvent", () => {
  it("appends deltas in arrival order and skips a replayed sequence", () => {
    let state = mergeMessage(EMPTY_TRANSCRIPT, message());
    state = applyAssistantEvent(state, delta("m1", 2, "Hel"));
    state = applyAssistantEvent(state, delta("m1", 2, "Hel"));
    state = applyAssistantEvent(state, delta("m1", 3, "lo"));
    expect(state.rows.m1?.content).toBe("Hello");
  });

  it("ignores a delta for a row it has not seen yet", () => {
    const state = applyAssistantEvent(EMPTY_TRANSCRIPT, delta("missing", 4, "x"));
    expect(state).toBe(EMPTY_TRANSCRIPT);
    // The sequence stays unconsumed, so the replay can serve it once the
    // created row lands instead of losing the chunk for good.
    expect(state.appliedDelta).toBe(0);
  });

  it("ignores a non-string or empty delta", () => {
    const state = mergeMessage(EMPTY_TRANSCRIPT, message());
    const blank = { ...delta("m1", 5, ""), payload: { turn_id: "turn", params: {} } } as AssistantEvent;
    expect(applyAssistantEvent(state, blank)).toBe(state);
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
    let state = applyAssistantEvent(EMPTY_TRANSCRIPT, created);
    expect(state.rows.m1?.status).toBe("streaming");
    const tool = {
      ...created,
      kind: "tool_call",
      payload: { turn_id: "turn", message: message({ id: "m2", role: "tool_call", content: "Search", seq: 2 }) },
    } as AssistantEvent;
    state = applyAssistantEvent(state, tool);
    expect(Object.keys(state.rows).sort()).toEqual(["m1", "m2"]);
  });

  it("keeps streamed text when a reconnect replays message_created", () => {
    const created = {
      id: 10,
      thread: "t1",
      message: "m1",
      seq: 10,
      kind: "message_created",
      payload: { turn_id: "turn", message: message({ content: "" }) },
      created_at: "2026-01-01T00:00:00Z",
    } as AssistantEvent;
    let state = applyAssistantEvent(EMPTY_TRANSCRIPT, created);
    state = applyAssistantEvent(state, delta("m1", 101, "Hello "));
    state = applyAssistantEvent(state, delta("m1", 102, "world"));
    // Reconnect: the server replays from zero, starting with the created row
    // exactly as it looked then, followed by the same deltas.
    state = applyAssistantEvent(state, created);
    state = applyAssistantEvent(state, delta("m1", 101, "Hello "));
    state = applyAssistantEvent(state, delta("m1", 102, "world"));
    expect(state.rows.m1?.content).toBe("Hello world");
  });

  it("keeps a terminal reply when an older replay re-sends its created row", () => {
    let state = mergeMessage(EMPTY_TRANSCRIPT, message({ content: "", status: "streaming" }));
    state = mergeMessage(state, message({ content: "answer", status: "completed" }));
    const replay = { ...delta("m1", 20, ""), kind: "message_created" } as AssistantEvent;
    state = applyAssistantEvent(state, replay);
    expect(state.rows.m1?.status).toBe("completed");
    expect(state.rows.m1?.content).toBe("answer");
  });

  it("never appends a replay's deltas over a row that already stopped streaming", () => {
    // A cold load has no memory of the last applied sequence; the stored row
    // is the whole answer, so a replay from zero must not stack on it.
    const loaded = mergeMessage(
      EMPTY_TRANSCRIPT,
      message({ content: "Hello world", status: "completed", completed_at: "2026-01-01T00:00:01Z" })
    );
    let state = applyAssistantEvent(loaded, delta("m1", 101, "Hello "));
    state = applyAssistantEvent(state, delta("m1", 102, "world"));
    expect(state.rows.m1?.content).toBe("Hello world");
  });

  it("resumes appending after a cold load when the turn is still streaming", () => {
    const loaded = mergeMessage(EMPTY_TRANSCRIPT, message({ content: "Hello ", status: "streaming" }));
    const state = applyAssistantEvent(loaded, delta("m1", 9, "world"));
    expect(state.rows.m1?.content).toBe("Hello world");
  });

  it("is pure: applying the same delta to the same input twice agrees", () => {
    // React replays a reducer under StrictMode; a reducer that mutated a
    // shared set skipped its first chunk on the replay and committed nothing.
    const start = mergeMessage(EMPTY_TRANSCRIPT, message());
    const chunk = delta("m1", 41, "Hello");
    const first = applyAssistantEvent(start, chunk);
    const second = applyAssistantEvent(start, chunk);
    expect(second).toEqual(first);
    expect(first.rows.m1?.content).toBe("Hello");
    expect(start.rows.m1?.content).toBe("");
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
    expect(applyAssistantEvent(EMPTY_TRANSCRIPT, turn)).toBe(EMPTY_TRANSCRIPT);
    const noRow = { ...turn, kind: "message_created", payload: { turn_id: "turn" } } as AssistantEvent;
    expect(applyAssistantEvent(EMPTY_TRANSCRIPT, noRow)).toBe(EMPTY_TRANSCRIPT);
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
