// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { fireEvent, render, screen } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";

import { AGENT_TOOLS_DISCLOSURE, AGENT_TOOLS_LABEL, AppsToggle } from "./AppsToggle.js";

describe("AppsToggle", () => {
  it("discloses the privilege boundary with the switch off (CLOUD-029)", () => {
    render(<AppsToggle enabled={false} disabled={false} onToggle={() => undefined} />);
    const text = screen.getByText(AGENT_TOOLS_DISCLOSURE).textContent?.toLowerCase() ?? "";
    expect(text).toContain("agents");
    expect(text).toContain("tools");
    expect(text).toContain("create");
    expect(text).toContain("delete");
    expect(screen.getByRole("switch", { name: AGENT_TOOLS_LABEL })).toHaveAttribute("aria-checked", "false");
  });

  it("reflects the on state", () => {
    render(<AppsToggle enabled={true} disabled={false} onToggle={() => undefined} />);
    expect(screen.getByRole("switch", { name: AGENT_TOOLS_LABEL })).toHaveAttribute("aria-checked", "true");
  });

  it("persists one flip through the handler", () => {
    const onToggle = vi.fn();
    render(<AppsToggle enabled={false} disabled={false} onToggle={onToggle} />);
    fireEvent.click(screen.getByRole("switch", { name: AGENT_TOOLS_LABEL }));
    expect(onToggle).toHaveBeenCalledWith(true);
  });

  it("stays inert while a write is in flight (CLOUD-029)", () => {
    const onToggle = vi.fn();
    render(<AppsToggle enabled={false} disabled={true} onToggle={onToggle} />);
    const toggle = screen.getByRole("switch", { name: AGENT_TOOLS_LABEL });
    expect(toggle).toHaveAttribute("aria-disabled", "true");
    fireEvent.click(toggle);
    expect(onToggle).not.toHaveBeenCalled();
  });
});
