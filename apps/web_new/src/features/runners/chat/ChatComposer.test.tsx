// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Composer shell (NEWFRONT-549, RUN-032): the gate reason renders and the
// box plus send control are inert whenever chat cannot proceed.
import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import * as React from "react";
import { describe, expect, it, vi } from "vitest";

import { COMPOSER_OFFLINE } from "./chatState.js";
import { CHAT_COMPOSER_PLACEHOLDER, ChatComposer } from "./ChatComposer.js";

describe("ChatComposer", () => {
  it("exposes the draft box and a send control when chat can proceed", () => {
    render(<ChatComposer blockedReason={null} sendEnabled onSend={vi.fn()} />);
    expect(screen.getByPlaceholderText(CHAT_COMPOSER_PLACEHOLDER)).toBeEnabled();
    expect(screen.getByRole("button", { name: "Send message" })).toBeEnabled();
    expect(screen.queryByTestId("runner-chat-block-reason")).toBeNull();
  });

  it("shows the blocking reason and disables input", () => {
    render(<ChatComposer blockedReason={COMPOSER_OFFLINE} sendEnabled onSend={vi.fn()} />);
    expect(screen.getByTestId("runner-chat-block-reason")).toHaveTextContent(COMPOSER_OFFLINE);
    expect(screen.getByPlaceholderText(CHAT_COMPOSER_PLACEHOLDER)).toBeDisabled();
    expect(screen.getByRole("button", { name: "Send message" })).toBeDisabled();
  });

  it("keeps an in-flight turn from sending again", async () => {
    const onSend = vi.fn();
    render(<ChatComposer blockedReason={null} sendEnabled turnInFlight onSend={onSend} />);
    const send = screen.getByRole("button", { name: "Send message" });
    expect(send).toBeDisabled();
    await userEvent.click(send);
    expect(onSend).not.toHaveBeenCalled();
  });

  it("sends when enabled", async () => {
    const onSend = vi.fn();
    render(<ChatComposer blockedReason={null} sendEnabled onSend={onSend} />);
    await userEvent.click(screen.getByRole("button", { name: "Send message" }));
    expect(onSend).toHaveBeenCalledTimes(1);
  });

  it("keeps send inert for an empty draft", () => {
    render(<ChatComposer blockedReason={null} />);
    expect(screen.getByRole("button", { name: "Send message" })).toBeDisabled();
  });
});
