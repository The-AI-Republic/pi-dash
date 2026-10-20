// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { cleanup, render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import * as React from "react";
import { afterEach, describe, expect, it, vi } from "vitest";

import { MemberEditingToggle } from "./MemberEditingToggle.js";

afterEach(() => {
  cleanup();
});

function renderToggle(props: Partial<React.ComponentProps<typeof MemberEditingToggle>> = {}) {
  const onChange = vi.fn();
  render(<MemberEditingToggle enabled pending={false} onChange={onChange} {...props} />);
  return { onChange };
}

describe("MemberEditingToggle", () => {
  it("names the setting and reads its enabled state", () => {
    renderToggle({ enabled: true });
    expect(screen.getByText("Let members edit states")).toBeInTheDocument();
    expect(screen.getByRole("switch", { name: "Let members edit states" })).toBeChecked();
  });

  it("reads off when the flag is off", () => {
    renderToggle({ enabled: false });
    expect(screen.getByRole("switch", { name: "Let members edit states" })).not.toBeChecked();
  });

  it("reports both directions of a flip", async () => {
    const user = userEvent.setup();
    const { onChange } = renderToggle({ enabled: true });
    await user.click(screen.getByRole("switch", { name: "Let members edit states" }));
    expect(onChange).toHaveBeenCalledWith(false);
  });

  it("disables the switch while the write is in flight", async () => {
    const user = userEvent.setup();
    const { onChange } = renderToggle({ pending: true });
    const control = screen.getByRole("switch", { name: "Let members edit states" });
    expect(control).toHaveAttribute("aria-disabled", "true");
    await user.click(control);
    expect(onChange).not.toHaveBeenCalled();
  });
});
