// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Conversation labels (AGT-044/AGT-048): archived rows hide, order is kept
// and a blank title reads as the untitled fallback.
import type { AssistantThread } from "@pidash/api-client";
import { describe, expect, it } from "vitest";

import { conversationLabel, visibleConversations } from "./labels.js";

function thread(id: string, overrides: Partial<AssistantThread> = {}): AssistantThread {
  return {
    id,
    title: `Thread ${id}`,
    is_archived: false,
    has_active_turn: false,
    created_at: "2026-10-01T00:00:00Z",
    updated_at: "2026-10-02T00:00:00Z",
    ...overrides,
  };
}

describe("visibleConversations", () => {
  it("hides archived rows and keeps the server order", () => {
    const rows = [thread("b"), thread("gone", { is_archived: true }), thread("a")];
    expect(visibleConversations(rows).map((row) => row.id)).toEqual(["b", "a"]);
  });

  it("treats an unresolved list as empty", () => {
    expect(visibleConversations(undefined)).toEqual([]);
  });
});

describe("conversationLabel", () => {
  it("uses the title when there is one", () => {
    expect(conversationLabel({ title: "Plan" })).toBe("Plan");
  });

  it("falls back to the untitled label for blank titles", () => {
    expect(conversationLabel({ title: "" })).toBe("Untitled conversation");
    expect(conversationLabel({ title: "  " })).toBe("Untitled conversation");
  });
});
