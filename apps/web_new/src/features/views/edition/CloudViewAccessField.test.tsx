// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import * as React from "react";
import { describe, expect, it, vi } from "vitest";

import type { ViewAccessValue } from "../../../core/edition/index.js";
import { CloudViewAccessField } from "./CloudViewAccessField.js";

function renderField(options: { value?: ViewAccessValue; disabled?: boolean } = {}): {
  onChange: ReturnType<typeof vi.fn>;
} {
  const onChange = vi.fn();
  render(<CloudViewAccessField value={options.value ?? 1} onChange={onChange} disabled={options.disabled ?? false} />);
  return { onChange };
}

describe("CloudViewAccessField", () => {
  it("offers shared and owner-only with one explanatory line each", () => {
    renderField();
    expect(screen.getByRole("radiogroup", { name: "Access" })).toBeInTheDocument();
    const shared = screen.getByRole("radio", { name: /Shared/ });
    const ownerOnly = screen.getByRole("radio", { name: /Owner-only/ });
    expect(shared).toHaveAttribute("aria-checked", "true");
    expect(ownerOnly).toHaveAttribute("aria-checked", "false");
    expect(shared).toHaveTextContent("Anyone who can open the project sees this view.");
    expect(ownerOnly).toHaveTextContent("Only the owner sees this view.");
  });

  it("marks the owner-only choice", () => {
    renderField({ value: 0 });
    expect(screen.getByRole("radio", { name: /Shared/ })).toHaveAttribute("aria-checked", "false");
    expect(screen.getByRole("radio", { name: /Owner-only/ })).toHaveAttribute("aria-checked", "true");
  });

  it("reports a picked option", async () => {
    const user = userEvent.setup();
    const { onChange } = renderField();
    await user.click(screen.getByRole("radio", { name: /Owner-only/ }));
    expect(onChange).toHaveBeenCalledWith(0);
    await user.click(screen.getByRole("radio", { name: /Shared/ }));
    expect(onChange).toHaveBeenCalledWith(1);
  });

  it("takes no edits while disabled", async () => {
    const user = userEvent.setup();
    const { onChange } = renderField({ disabled: true });
    const options = screen.getAllByRole("radio");
    expect(options).toHaveLength(2);
    for (const option of options) {
      expect(option).toBeDisabled();
    }
    await user.click(screen.getByRole("radio", { name: /Owner-only/ }));
    expect(onChange).not.toHaveBeenCalled();
  });
});
