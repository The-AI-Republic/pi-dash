// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Transcript (NEWFRONT-549 RUN-026, streaming NEWFRONT-551 RUN-029): saved
// and streamed rows in order, the person's turn on the trailing edge,
// assistant prose rendered as markdown, a never-blank streaming bubble,
// and the live activity strip beneath the rows.
import type { ChatMessage } from "@pidash/api-client";
import { render, screen } from "@testing-library/react";
import * as React from "react";
import { describe, expect, it } from "vitest";

import {
  bubbleContent,
  CHAT_STREAMING_PLACEHOLDER,
  CHAT_TRANSCRIPT_EMPTY,
  CHAT_TRANSCRIPT_WAITING,
  ChatTranscript,
  messageText,
} from "./ChatTranscript.js";

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
  it("keeps the row text and falls back to the status when blank", () => {
    expect(messageText({ content: "  hi  ", status: "sent" })).toBe("  hi  ");
    expect(messageText({ content: "   ", status: "streaming" })).toBe("streaming");
  });
});

describe("bubbleContent", () => {
  it("keeps real text and reads a blank streaming row as a placeholder", () => {
    expect(bubbleContent({ content: "done", status: "completed" })).toEqual({ text: "done", streaming: false });
    expect(bubbleContent({ content: " ", status: "streaming" })).toEqual({
      text: CHAT_STREAMING_PLACEHOLDER,
      streaming: true,
    });
    expect(bubbleContent({ content: "", status: "failed" })).toEqual({ text: "failed", streaming: true });
  });
});

describe("ChatTranscript", () => {
  it("renders saved rows in order with their roles", () => {
    render(
      <ChatTranscript
        loading={false}
        messages={[
          message(),
          message({ id: "m-2", role: "assistant", content: "hi there", seq: 2 }),
          message({ id: "m-3", role: "tool", content: "ran ls", seq: 3 }),
          message({ id: "m-4", role: "system", content: "session opened", seq: 4 }),
        ]}
      />
    );
    const items = screen.getAllByRole("listitem");
    expect(items).toHaveLength(4);
    expect(items[0]?.querySelector('[data-role="user"]')).toHaveTextContent("hello");
    expect(items[1]?.querySelector('[data-role="assistant"]')).toHaveTextContent("hi there");
    expect(items[2]?.querySelector('[data-role="tool"]')).toHaveTextContent("ran ls");
    expect(items[3]?.querySelector('[data-role="system"]')).toHaveTextContent("session opened");
    expect(screen.getByRole("list", { name: "Messages" })).toBeInTheDocument();
  });

  it("renders assistant prose as markdown", () => {
    render(
      <ChatTranscript loading={false} messages={[message({ role: "assistant", content: "**bold** and `code`" })]} />
    );
    const bubble = document.querySelector('[data-role="assistant"]');
    expect(bubble?.querySelector("strong")).toHaveTextContent("bold");
    expect(bubble?.querySelector("code")).toHaveTextContent("code");
  });

  it("keeps a user turn literal", () => {
    render(<ChatTranscript loading={false} messages={[message({ content: "**not bold**" })]} />);
    expect(document.querySelector('[data-role="user"] strong')).toBeNull();
    expect(screen.getByText("**not bold**")).toBeInTheDocument();
  });

  it("shows a placeholder for a streaming reply with no text yet", () => {
    render(
      <ChatTranscript
        loading={false}
        messages={[message({ role: "assistant", content: "", status: "streaming", seq: 2 })]}
      />
    );
    const bubble = document.querySelector('[data-role="assistant"]');
    expect(bubble).toHaveAttribute("data-streaming", "true");
    expect(bubble).toHaveTextContent(CHAT_STREAMING_PLACEHOLDER);
  });

  it("drops the streaming marker once text arrives", () => {
    render(
      <ChatTranscript
        loading={false}
        messages={[message({ role: "assistant", content: "partial", status: "streaming", seq: 2 })]}
      />
    );
    const bubble = document.querySelector('[data-role="assistant"]');
    expect(bubble).not.toHaveAttribute("data-streaming");
    expect(bubble).toHaveTextContent("partial");
  });

  it("reads the empty line once loaded and the waiting line while loading", () => {
    const { rerender } = render(<ChatTranscript messages={[]} loading />);
    expect(screen.getByTestId("runner-chat-transcript-empty")).toHaveTextContent(CHAT_TRANSCRIPT_WAITING);
    rerender(<ChatTranscript messages={[]} loading={false} />);
    expect(screen.getByTestId("runner-chat-transcript-empty")).toHaveTextContent(CHAT_TRANSCRIPT_EMPTY);
  });

  it("lists the live activity strip newest last", () => {
    render(
      <ChatTranscript
        loading={false}
        messages={[message()]}
        activity={[
          { seq: 4, label: "Ran: ls" },
          { seq: 5, label: "Edited: src/a.ts" },
        ]}
      />
    );
    const rows = screen.getAllByTestId("runner-chat-activity");
    expect(rows.map((row) => row.textContent)).toEqual(["Ran: ls", "Edited: src/a.ts"]);
    expect(screen.getByRole("list", { name: "Recent agent activity" })).toBeInTheDocument();
  });

  it("omits the activity list when there is nothing to show", () => {
    render(<ChatTranscript loading={false} messages={[message()]} />);
    expect(screen.queryByRole("list", { name: "Recent agent activity" })).toBeNull();
  });
});
