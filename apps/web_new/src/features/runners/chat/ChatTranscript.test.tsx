// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Saved transcript (NEWFRONT-549, RUN-026): rows in sequence order, the
// user turn on the trailing edge, and the empty/loading lines.
import type { ChatMessage } from "@pidash/api-client";
import { render, screen } from "@testing-library/react";
import * as React from "react";
import { describe, expect, it } from "vitest";

import { CHAT_TRANSCRIPT_EMPTY, CHAT_TRANSCRIPT_WAITING, ChatTranscript, messageText } from "./ChatTranscript.js";

function message(overrides: Partial<ChatMessage> = {}): ChatMessage {
  return {
    id: "m-1",
    session: "s-1",
    role: "user",
    content: "hello",
    content_parts: [],
    status: "completed",
    local_item_id: "",
    local_turn_id: "",
    seq: 1,
    created_at: "2026-01-01T00:00:00Z",
    completed_at: null,
    ...overrides,
  };
}

describe("messageText", () => {
  it("falls back to the row status when there is no content yet", () => {
    expect(messageText({ content: "  hi  ", status: "sent" })).toBe("  hi  ");
    expect(messageText({ content: "   ", status: "streaming" })).toBe("streaming");
  });
});

describe("ChatTranscript", () => {
  it("renders saved rows in order with roles", () => {
    render(
      <ChatTranscript
        loading={false}
        messages={[message(), message({ id: "m-2", role: "assistant", content: "hi there", seq: 2 })]}
      />
    );
    const items = screen.getAllByRole("listitem");
    expect(items).toHaveLength(2);
    expect(items[0]).toHaveTextContent("hello");
    expect(items[0]?.querySelector('[data-role="user"]')).not.toBeNull();
    expect(items[1]?.querySelector('[data-role="assistant"]')).not.toBeNull();
  });

  it("reads the empty line once loaded and the waiting line while loading", () => {
    const { rerender } = render(<ChatTranscript messages={[]} loading />);
    expect(screen.getByTestId("runner-chat-transcript-empty")).toHaveTextContent(CHAT_TRANSCRIPT_WAITING);
    rerender(<ChatTranscript messages={[]} loading={false} />);
    expect(screen.getByTestId("runner-chat-transcript-empty")).toHaveTextContent(CHAT_TRANSCRIPT_EMPTY);
  });
});
