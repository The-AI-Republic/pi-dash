// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// The header's creation action (MOD-003): "Add Module" where there is room
// and "Add" on small screens; activating it hands off to the opener.

import { fireEvent, render, screen } from "@testing-library/react";
import * as React from "react";
import { describe, expect, it, vi } from "vitest";

import { ModuleCreateAction } from "./ModuleCreateAction.js";

describe("ModuleCreateAction", () => {
  it("labels the wide action Add Module and opens creation", () => {
    const onCreate = vi.fn();
    render(<ModuleCreateAction compact={false} onCreate={onCreate} />);
    const button = screen.getByRole("button", { name: "Add Module" });
    expect(button).toHaveAttribute("data-testid", "modules-add");
    fireEvent.click(button);
    expect(onCreate).toHaveBeenCalledTimes(1);
  });

  it("shortens to Add on small screens", () => {
    render(<ModuleCreateAction compact onCreate={vi.fn()} />);
    expect(screen.getByRole("button", { name: "Add" })).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "Add Module" })).not.toBeInTheDocument();
  });
});
