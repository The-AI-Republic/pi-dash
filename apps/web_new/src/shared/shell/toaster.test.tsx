// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { ToastHost } from "@pidash/kit";
import * as React from "react";
import { describe, expect, it, vi } from "vitest";

import { getToastManager, notifyError, notifyInfo, notifySuccess, notifySuccessWithAction } from "./toaster.js";

describe("toaster", () => {
  it("delivers success, info, and error notices to the shared stack", async () => {
    render(<ToastHost manager={getToastManager()} />);
    notifySuccess("Installed Plain App");
    notifySuccess("Connected Plain App", "The app is ready.");
    notifyInfo("Uninstalled Old App");
    notifyInfo("Catalog refreshed", "The list is current.");
    notifyError("Install failed", "Hub refused the install.");
    notifyError("Connection failed");
    for (const line of [
      "Installed Plain App",
      "Connected Plain App",
      "The app is ready.",
      "Uninstalled Old App",
      "Catalog refreshed",
      "The list is current.",
      "Install failed",
      "Hub refused the install.",
      "Connection failed",
    ]) {
      expect(await screen.findByText(line)).toBeInTheDocument();
    }
  });

  it("offers a follow-up action on a success toast, with and without a description", async () => {
    const user = userEvent.setup();
    const onAction = vi.fn();
    render(<ToastHost manager={getToastManager()} />);
    notifySuccessWithAction("Work item created successfully", undefined, {
      actionLabel: "View work item",
      onAction,
    });
    notifySuccessWithAction("Saved the view", "Every section now loads.", {
      actionLabel: "Undo",
      onAction,
    });
    expect(await screen.findByText("Work item created successfully")).toBeInTheDocument();
    expect(await screen.findByText("Every section now loads.")).toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: "Undo" }));
    expect(onAction).toHaveBeenCalledTimes(1);
  });
});
