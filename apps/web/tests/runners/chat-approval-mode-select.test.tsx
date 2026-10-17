/**
 * Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
 * SPDX-License-Identifier: AGPL-3.0-only
 * See the LICENSE file for details.
 */

import { render, screen } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";

vi.mock("@pi-dash/ui", () => ({
  Button: ({ children }: { children: React.ReactNode }) => <button type="button">{children}</button>,
}));

import { APPROVAL_MODE_PENDING_NOTE, ChatApprovalModeSelect } from "@/pi-dash-web/components/desktop";

const pressed = () =>
  screen
    .getAllByRole("button")
    .filter((button) => button.getAttribute("aria-pressed") === "true")
    .map((button) => button.textContent);

describe("ChatApprovalModeSelect", () => {
  it("shows the picked mode as active when nothing is pending", () => {
    render(<ChatApprovalModeSelect value="ask" onChange={vi.fn()} />);

    expect(pressed()).toEqual(["Ask every time"]);
    expect(screen.queryByText(APPROVAL_MODE_PENDING_NOTE)).toBeNull();
  });

  it("keeps the mode in force active and says when a pending pick applies", () => {
    render(<ChatApprovalModeSelect value="full_access" pending="ask" onChange={vi.fn()} />);

    // The running turn is still under full access, so Ask must not look active.
    expect(pressed()).toEqual(["Full access"]);
    expect(screen.getByRole("status").textContent).toBe(APPROVAL_MODE_PENDING_NOTE);
  });

  it("says nothing when the pending pick is the mode already in force", () => {
    render(<ChatApprovalModeSelect value="ask" pending="ask" onChange={vi.fn()} />);

    expect(screen.queryByText(APPROVAL_MODE_PENDING_NOTE)).toBeNull();
  });
});
