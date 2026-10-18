// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { act, render, screen } from "@testing-library/react";
import * as React from "react";
import { afterEach, describe, expect, it, vi } from "vitest";

import { SaveIndicator, SAVED_FADE_MS } from "./SaveIndicator.js";

afterEach(() => {
  vi.useRealTimers();
});

describe("SaveIndicator", () => {
  it("spins with Saving while a write is in flight", () => {
    render(<SaveIndicator status="saving" />);
    // Two nested live regions (the spinner announces too), so read by text.
    expect(screen.getByText("Saving…")).toBeInTheDocument();
  });

  it("shows Saved, then fades it out on a timer", () => {
    vi.useFakeTimers();
    const { rerender } = render(<SaveIndicator status="saving" />);
    rerender(<SaveIndicator status="saved" />);
    const note = screen.getByRole("status");
    expect(note).toHaveTextContent("Saved");
    expect(note).toHaveClass("opacity-100");
    act(() => {
      vi.advanceTimersByTime(SAVED_FADE_MS);
    });
    expect(note).toHaveClass("opacity-0");
  });

  it("unfades when another save starts", () => {
    vi.useFakeTimers();
    const { rerender } = render(<SaveIndicator status="saved" />);
    act(() => {
      vi.advanceTimersByTime(SAVED_FADE_MS);
    });
    expect(screen.getByRole("status")).toHaveClass("opacity-0");
    rerender(<SaveIndicator status="saving" />);
    expect(screen.getByText("Saving…")).toBeInTheDocument();
  });
});
