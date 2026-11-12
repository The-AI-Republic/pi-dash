/**
 * Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
 * SPDX-License-Identifier: AGPL-3.0-only
 * See the LICENSE file for details.
 */

import { render, screen } from "@testing-library/react";
import { describe, expect, it } from "vitest";
import type { IAssistantMessage } from "@pi-dash/types";

import { AssistantMessage } from "@/components/assistant/assistant-message";

const CUT_OFF = /This reply was cut off because it reached the model's output limit/;

function message(overrides: Partial<IAssistantMessage>): IAssistantMessage {
  return {
    id: "m1",
    role: "assistant",
    content: "The answer starts here and",
    status: "completed",
    seq: 2,
    turn_id: "t1",
    payload: {},
    created_at: "2026-10-11T00:00:00Z",
    completed_at: "2026-10-11T00:00:01Z",
    ...overrides,
  };
}

describe("AssistantMessage", () => {
  it("tells the user when the model cut the reply off at its output limit", () => {
    render(<AssistantMessage message={message({ payload: { truncated: true } })} />);
    expect(screen.getByText("The answer starts here and")).toBeTruthy();
    expect(screen.getByText(CUT_OFF)).toBeTruthy();
  });

  it("shows no notice under a reply that finished normally", () => {
    render(<AssistantMessage message={message({})} />);
    expect(screen.getByText("The answer starts here and")).toBeTruthy();
    expect(screen.queryByText(CUT_OFF)).toBeNull();
  });

  it("only flags assistant replies", () => {
    render(<AssistantMessage message={message({ role: "user", payload: { truncated: true } })} />);
    expect(screen.queryByText(CUT_OFF)).toBeNull();
  });
});
