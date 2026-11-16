// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { fireEvent, render, screen } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";

import { CLOUD_ENTRY_ERROR_TITLE, CloudEntryErrorScreen } from "./CloudEntryErrorScreen.js";

describe("CloudEntryErrorScreen", () => {
  it("explains the outage with the pinned heading", () => {
    render(<CloudEntryErrorScreen onRetry={() => undefined} />);
    expect(screen.getByTestId("admin-cloud-error")).toBeInTheDocument();
    expect(screen.getByRole("heading", { name: "Something went wrong" })).toBeInTheDocument();
    expect(document.title).toBe(CLOUD_ENTRY_ERROR_TITLE);
  });

  it("retries through the pinned control", () => {
    const onRetry = vi.fn();
    render(<CloudEntryErrorScreen onRetry={onRetry} />);
    fireEvent.click(screen.getByRole("button", { name: "Try again" }));
    expect(onRetry).toHaveBeenCalledTimes(1);
  });

  it("denies nothing and links nowhere", () => {
    render(<CloudEntryErrorScreen onRetry={() => undefined} />);
    expect(screen.queryByRole("heading", { name: "Access denied" })).not.toBeInTheDocument();
    expect(screen.queryByRole("link")).not.toBeInTheDocument();
  });

  it("carries no password or setup fields", () => {
    render(<CloudEntryErrorScreen onRetry={() => undefined} />);
    expect(document.querySelector('input[type="password"]')).toBeNull();
    expect(screen.queryByLabelText("First name")).not.toBeInTheDocument();
  });
});
