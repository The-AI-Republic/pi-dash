/**
 * Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
 * SPDX-License-Identifier: AGPL-3.0-only
 * See the LICENSE file for details.
 */

import { describe, expect, it } from "vitest";
import type { IAssistantMessage } from "@pi-dash/types";

import { bySeq, latestRealMessage, withNotice, type ById } from "@/components/assistant/notices";

const AT = "2026-09-03T10:00:00Z";

function msg(id: string, seq: number, role: IAssistantMessage["role"]): IAssistantMessage {
  return {
    id,
    role,
    content: "",
    status: "completed",
    seq,
    turn_id: null,
    payload: {},
    created_at: AT,
    completed_at: AT,
  } as IAssistantMessage;
}

function transcript(...messages: IAssistantMessage[]): ById {
  return Object.fromEntries(messages.map((m) => [m.id, m]));
}

const order = (m: ById): string[] =>
  Object.values(m)
    .slice()
    // eslint-disable-next-line unicorn/no-array-sort -- fresh copy; toSorted not in tsconfig lib target
    .sort(bySeq)
    .map((x) => x.id);

describe("notice anchoring", () => {
  // Event seqs run far ahead of message seqs (every streamed delta burns one),
  // so a notice numbered in the event space sorts below every later turn.
  it("anchors a notice to the current turn, not to its event seq", () => {
    let state = transcript(msg("u1", 1, "user"), msg("a1", 2, "assistant"));
    // Event seq 900 is what the backend allocated; the turn only reached msg 2.
    state = withNotice(state, 900, [{ name: "Jira", reason: "url_blocked" }], AT);
    // A later turn arrives afterwards.
    state = { ...state, u2: msg("u2", 3, "user"), a2: msg("a2", 4, "assistant") };

    expect(order(state)).toEqual(["u1", "a1", "notice:900", "u2", "a2"]);
  });

  it("keeps two notices from one turn in event order", () => {
    let state = transcript(msg("u1", 1, "user"));
    state = withNotice(state, 900, [{ name: "Build-time", reason: "url_blocked" }], AT);
    state = withNotice(state, 950, [{ name: "Run-time", reason: "toolset_unavailable" }], AT);

    expect(order(state)).toEqual(["u1", "notice:900", "notice:950"]);
  });

  it("is idempotent across an SSE replay and never re-anchors", () => {
    let state = transcript(msg("u1", 1, "user"), msg("a1", 2, "assistant"));
    state = withNotice(state, 900, [{ name: "Jira", reason: "url_blocked" }], AT);
    const anchored = state["notice:900"].seq;

    // A later turn lands, then the stream reconnects and replays from 0.
    state = { ...state, u2: msg("u2", 3, "user"), a2: msg("a2", 4, "assistant") };
    state = withNotice(state, 900, [{ name: "Jira", reason: "url_blocked" }], AT);

    expect(Object.keys(state).filter((k) => k.startsWith("notice:"))).toHaveLength(1);
    expect(state["notice:900"].seq).toBe(anchored);
    expect(order(state)).toEqual(["u1", "a1", "notice:900", "u2", "a2"]);
  });
});

describe("latestRealMessage", () => {
  // Regression: a notice is anchored *after* the reply it follows, so it is the
  // max-seq item. If the turn-completion fallback picked it, it would see role
  // "notice", never clear busy, and leave the composer disabled forever.
  it("ignores a trailing notice so the terminal reply still wins", () => {
    const state = withNotice(
      transcript(msg("u1", 1, "user"), msg("a1", 2, "assistant")),
      900,
      [{ name: "Jira", reason: "url_blocked" }],
      AT
    );

    const last = latestRealMessage(Object.values(state));
    expect(last?.id).toBe("a1");
    expect(last?.role).toBe("assistant");
  });

  it("returns undefined when a thread holds only notices", () => {
    const state = withNotice({}, 900, [{ name: "Jira", reason: "url_blocked" }], AT);
    expect(latestRealMessage(Object.values(state))).toBeUndefined();
  });
});
