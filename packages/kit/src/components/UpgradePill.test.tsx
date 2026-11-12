// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it, vi } from "vitest";
import { UpgradePill } from "./UpgradePill";

describe("UpgradePill", () => {
  it("marks the row with the Pro text in the small size by default", () => {
    render(<UpgradePill />);
    const pill = screen.getByText("Pro", { exact: true });
    expect(pill.tagName).toBe("SPAN");
    expect(pill.className).toContain("text-caption");
    expect(pill.className).not.toContain("text-meta");
    // The stable hook parity scenarios use to count and click the marker.
    expect(pill).toHaveAttribute("data-testid", "upgrade-pill");
  });

  it("renders the medium size variant", () => {
    render(<UpgradePill size="medium" />);
    const pill = screen.getByText("Pro", { exact: true });
    expect(pill.className).toContain("text-meta");
    expect(pill.className).not.toContain("text-caption");
  });

  it("is plain text that never intercepts row navigation", async () => {
    const user = userEvent.setup();
    const onRowClick = vi.fn();
    render(
      <button type="button" onClick={onRowClick}>
        Cycles <UpgradePill />
      </button>
    );
    expect(screen.queryByRole("button", { name: "Pro" })).not.toBeInTheDocument();
    expect(screen.queryByRole("link", { name: "Pro" })).not.toBeInTheDocument();
    await user.click(screen.getByText("Pro", { exact: true }));
    expect(onRowClick).toHaveBeenCalledTimes(1);
  });
});
