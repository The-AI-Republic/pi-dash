// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { fireEvent, render, screen } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";

import {
  INSTANCE_FAILURE_ART,
  INSTANCE_FAILURE_TITLE,
  InstanceFailureScreen,
  reloadEntryPage,
} from "./InstanceFailureScreen.js";

function stubReload(): ReturnType<typeof vi.fn> {
  const reload = vi.fn();
  Object.defineProperty(globalThis.window, "location", {
    value: { reload },
    writable: true,
    configurable: true,
  });
  return reload;
}

describe("InstanceFailureScreen", () => {
  it("explains the failure with an illustration and a retry control", () => {
    render(<InstanceFailureScreen />);
    expect(screen.getByTestId("admin-failure")).toBeInTheDocument();
    const art = screen.getByAltText("A server cabinet with its link light out");
    expect(art).toHaveAttribute("src", INSTANCE_FAILURE_ART);
    expect(screen.getByRole("heading", { name: "Could not reach this instance" })).toBeInTheDocument();
    expect(screen.getByText(/Check your connection, then retry\./)).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Retry" })).toBeInTheDocument();
    expect(document.title).toBe(INSTANCE_FAILURE_TITLE);
  });

  it("retries with a full page reload", () => {
    const reload = stubReload();
    render(<InstanceFailureScreen />);
    fireEvent.click(screen.getByRole("button", { name: "Retry" }));
    expect(reload).toHaveBeenCalledTimes(1);
  });

  it("reloads the entry on demand", () => {
    const reload = stubReload();
    reloadEntryPage();
    expect(reload).toHaveBeenCalledTimes(1);
  });
});
