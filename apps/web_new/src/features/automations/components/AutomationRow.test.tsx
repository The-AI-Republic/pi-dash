// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it, vi } from "vitest";

import { AutomationRow } from "./AutomationRow.js";

describe("AutomationRow", () => {
  it("renders the heading, the description and a switch tied to the row", () => {
    render(<AutomationRow title="Idle cleanup" description="Cleans up" checked={false} onToggle={vi.fn()} />);
    const heading = screen.getByRole("heading", { level: 4, name: "Idle cleanup" });
    const toggle = screen.getByRole("switch", { name: "Idle cleanup" });
    expect(heading).toBeInTheDocument();
    expect(toggle).toHaveAttribute("aria-checked", "false");
    expect(screen.getByText("Cleans up")).toHaveAttribute("id");
  });

  it("reports the next state on click", async () => {
    const user = userEvent.setup();
    const onToggle = vi.fn();
    render(<AutomationRow title="Idle cleanup" description="Cleans up" checked={false} onToggle={onToggle} />);
    await user.click(screen.getByRole("switch", { name: "Idle cleanup" }));
    expect(onToggle).toHaveBeenCalledWith(true);
  });

  it("reports off from the checked state", async () => {
    const user = userEvent.setup();
    const onToggle = vi.fn();
    render(<AutomationRow title="Idle cleanup" description="Cleans up" checked onToggle={onToggle} />);
    expect(screen.getByRole("switch", { name: "Idle cleanup" })).toHaveAttribute("aria-checked", "true");
    await user.click(screen.getByRole("switch", { name: "Idle cleanup" }));
    expect(onToggle).toHaveBeenCalledWith(false);
  });

  it("shows the detail panel only while on", () => {
    const { rerender } = render(
      <AutomationRow title="Idle cleanup" description="Cleans up" checked={false} onToggle={vi.fn()}>
        <p>Delay</p>
      </AutomationRow>
    );
    expect(screen.queryByText("Delay")).toBeNull();
    rerender(
      <AutomationRow title="Idle cleanup" description="Cleans up" checked onToggle={vi.fn()}>
        <p>Delay</p>
      </AutomationRow>
    );
    expect(screen.getByText("Delay")).toBeInTheDocument();
  });

  it("renders a bare row with neither icon nor detail panel", () => {
    render(<AutomationRow title="Idle cleanup" description="Cleans up" checked={false} onToggle={vi.fn()} />);
    expect(screen.getByRole("switch", { name: "Idle cleanup" })).toBeInTheDocument();
  });

  it("disables the switch when the row is read-only", async () => {
    const user = userEvent.setup();
    const onToggle = vi.fn();
    render(<AutomationRow title="Idle cleanup" description="Cleans up" checked={false} disabled onToggle={onToggle} />);
    const toggle = screen.getByRole("switch", { name: "Idle cleanup" });
    expect(toggle).toHaveAttribute("aria-disabled", "true");
    await user.click(toggle);
    expect(onToggle).not.toHaveBeenCalled();
  });

  it("renders the icon when one is supplied", () => {
    render(
      <AutomationRow
        title="Idle cleanup"
        description="Cleans up"
        icon={<span data-testid="row-icon" />}
        checked
        onToggle={vi.fn()}
      />
    );
    expect(screen.getByTestId("row-icon")).toBeInTheDocument();
  });
});
