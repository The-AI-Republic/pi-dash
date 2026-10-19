// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Setup card (AGT-039): explains the bring-your-own-key rule and the
// permission scope, and links the provider settings page.
import { render, screen } from "@testing-library/react";
import * as React from "react";
import { describe, expect, it } from "vitest";

import { SetupCard } from "./SetupCard.js";

describe("SetupCard", () => {
  it("states the key rule and permission scope", () => {
    render(<SetupCard />);
    const card = screen.getByTestId("assistant-setup-card");
    expect(card).toHaveAccessibleName("Set up your AI assistant");
    expect(screen.getByRole("heading", { name: "Set up your AI assistant" })).toBeInTheDocument();
    expect(card).toHaveTextContent("Bring your own LLM provider key");
    expect(card).toHaveTextContent("exactly your permissions");
  });

  it("links the provider settings page with the pinned action label", () => {
    render(<SetupCard />);
    const action = screen.getByRole("button", { name: "Configure provider" });
    expect(action).toHaveAttribute("href", "/settings/profile/ai-assistant");
  });

  it("forwards layout classes", () => {
    render(<SetupCard className="extra-class" />);
    expect(screen.getByTestId("assistant-setup-card")).toHaveClass("extra-class");
  });
});
