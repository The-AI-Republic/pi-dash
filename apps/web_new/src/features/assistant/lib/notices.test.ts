// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Skipped tool-server notices (AGT-046): the replay key, the anchor rule,
// and every reason phrasing the oracle pins plus the raw fallback.
import { describe, expect, it } from "vitest";
import type { AssistantMessage } from "@pidash/api-client";

import { noticeAnchorSeq, noticeId, skippedServerLine } from "./notices.js";

function row(seq: number): AssistantMessage {
  return {
    id: `m${seq}`,
    role: "user",
    content: "ask",
    status: "completed",
    seq,
    turn_id: null,
    payload: {},
    created_at: "2026-10-01T00:00:00Z",
    completed_at: "2026-10-01T00:00:00Z",
  };
}

describe("noticeId", () => {
  it("keys one notice per skipped-servers event", () => {
    expect(noticeId(701)).toBe("notice:701");
  });
});

describe("noticeAnchorSeq", () => {
  it("anchors after the highest row sequence on screen", () => {
    expect(noticeAnchorSeq({ a: row(1), b: row(4), c: row(2) })).toBe(4);
  });

  it("anchors at zero on an empty transcript", () => {
    expect(noticeAnchorSeq({})).toBe(0);
  });
});

describe("skippedServerLine", () => {
  it("maps a blocked URL to plain language", () => {
    expect(skippedServerLine({ name: "closed", reason: "url_blocked" })).toBe(
      "Tool server closed was unavailable for this reply (its URL is not allowed)."
    );
  });

  it("maps an unreachable toolset to plain language", () => {
    expect(skippedServerLine({ name: "poll-notice", reason: "toolset_unavailable" })).toBe(
      "Tool server poll-notice was unavailable for this reply (it could not be reached)."
    );
  });

  it("maps an unreadable credential to plain language", () => {
    expect(skippedServerLine({ name: "vault", reason: "auth_header_unreadable" })).toBe(
      "Tool server vault was unavailable for this reply (its credential could not be read)."
    );
  });

  it("maps an undecryptable credential to plain language", () => {
    expect(skippedServerLine({ name: "vault", reason: "assistant_not_configured" })).toBe(
      "Tool server vault was unavailable for this reply (its credential could not be decrypted)."
    );
  });

  it("maps an OpenHub outage to plain language", () => {
    expect(skippedServerLine({ name: "hub", reason: "openhub_unavailable" })).toBe(
      "Tool server hub was unavailable for this reply (OpenHub apps were unavailable)."
    );
  });

  it("phrases a whole outage as one capability line", () => {
    expect(skippedServerLine({ name: "all tool servers", reason: "toolsets_unavailable" })).toBe(
      "Tool servers were unavailable for this reply (they could not be loaded)."
    );
  });

  it("renders an unmapped reason as it arrived", () => {
    expect(skippedServerLine({ name: "dead-a", reason: "RuntimeError" })).toBe(
      "Tool server dead-a was unavailable for this reply (RuntimeError)."
    );
  });
});
