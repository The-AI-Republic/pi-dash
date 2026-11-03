// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import * as React from "react";
import { describe, expect, it, vi } from "vitest";

import { WORKSPACE_TEAM_SIZES } from "../workspace.js";
import { TeamSizeDropdown, TEAM_SIZE_DROPDOWN_PLACEHOLDER } from "./TeamSizeDropdown.js";

function renderDropdown(options: { value?: string; disabled?: boolean } = {}) {
  const onSelect = vi.fn();
  render(
    <>
      <span id="size-label">Team size</span>
      <TeamSizeDropdown
        value={options.value ?? ""}
        options={WORKSPACE_TEAM_SIZES}
        onSelect={onSelect}
        disabled={options.disabled ?? false}
        labelId="size-label"
      />
    </>
  );
  return { onSelect };
}

describe("TeamSizeDropdown", () => {
  it("offers the placeholder plus every team size under the shared label", () => {
    renderDropdown();
    const select = screen.getByRole("combobox", { name: "Team size" });
    expect(select).toHaveDisplayValue(TEAM_SIZE_DROPDOWN_PLACEHOLDER);
    for (const size of WORKSPACE_TEAM_SIZES) {
      expect(screen.getByRole("option", { name: size })).toBeInTheDocument();
    }
  });

  it("reports the chosen size", async () => {
    const user = userEvent.setup();
    const { onSelect } = renderDropdown();
    await user.selectOptions(screen.getByRole("combobox", { name: "Team size" }), "11-50");
    expect(onSelect).toHaveBeenCalledWith("11-50");
  });

  it("shows the current choice and disables while busy", () => {
    renderDropdown({ value: "2-10", disabled: true });
    const select = screen.getByRole("combobox", { name: "Team size" });
    expect(select).toHaveDisplayValue("2-10");
    expect(select).toBeDisabled();
  });
});
