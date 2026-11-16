// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Header creation entry (MOD-031): one primary button named by the pinned
// label that hands its open callback up to the page.

import { fireEvent, render, screen } from "@testing-library/react";
import * as React from "react";
import { describe, expect, it, vi } from "vitest";

import { ModuleAddWorkItemButton } from "./ModuleAddWorkItemButton.js";

describe("ModuleAddWorkItemButton", () => {
  it("renders the pinned label and opens the page's create flow", () => {
    const onOpen = vi.fn();
    render(<ModuleAddWorkItemButton onOpen={onOpen} />);
    const button = screen.getByRole("button", { name: "Add work item" });
    expect(button).toHaveAttribute("data-testid", "module-add-work-item");
    fireEvent.click(button);
    expect(onOpen).toHaveBeenCalledTimes(1);
  });
});
