// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Chat header (NEWFRONT-549, RUN-032): name, secondary line, badge.
import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import * as React from "react";
import { describe, expect, it, vi } from "vitest";

import { CHAT_RUNNER_FALLBACK_NAME } from "./chatState.js";
import { CHAT_CLOSE_LABEL, ChatHeader } from "./ChatHeader.js";

describe("ChatHeader", () => {
  it("names the runner with its secondary line and status badge", () => {
    render(<ChatHeader summary={{ name: "alpha", secondary: "Default", status: "online" }} />);
    expect(screen.getByTestId("runner-chat-name")).toHaveTextContent("alpha");
    expect(screen.getByTestId("runner-chat-secondary")).toHaveTextContent("Default");
    expect(screen.getByText("online")).toBeInTheDocument();
  });

  it("falls back to the neutral name without a status while loading", () => {
    render(<ChatHeader summary={{ name: CHAT_RUNNER_FALLBACK_NAME, secondary: "", status: null }} />);
    expect(screen.getByTestId("runner-chat-name")).toHaveTextContent(CHAT_RUNNER_FALLBACK_NAME);
    expect(screen.queryByTestId("runner-chat-secondary")).toBeNull();
    expect(screen.queryByText("online")).toBeNull();
  });

  it("renders trailing actions", () => {
    render(<ChatHeader summary={{ name: "alpha", secondary: "", status: "busy" }} actions={<button>Close</button>} />);
    expect(screen.getByRole("button", { name: "Close" })).toBeInTheDocument();
    expect(screen.getByText("busy")).toBeInTheDocument();
  });
});

describe("ChatHeader close control (RUN-031)", () => {
  it("closes only when the page offers a conversation", async () => {
    const onClose = vi.fn();
    const { rerender } = render(
      <ChatHeader summary={{ name: "alpha", secondary: "", status: null }} onClose={onClose} />
    );
    await userEvent.click(screen.getByRole("button", { name: CHAT_CLOSE_LABEL }));
    expect(onClose).toHaveBeenCalledTimes(1);

    rerender(<ChatHeader summary={{ name: "alpha", secondary: "", status: null }} />);
    expect(screen.queryByRole("button", { name: CHAT_CLOSE_LABEL })).toBeNull();
  });
});
