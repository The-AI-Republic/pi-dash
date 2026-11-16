// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// First-run onboarding (MOD-009): art, a title and an explanation of what
// modules are, plus one creation action. Admins and members may press it;
// guests see the same view with the action disabled.

import { fireEvent, render, screen } from "@testing-library/react";
import * as React from "react";
import { describe, expect, it, vi } from "vitest";

import { ModulesZeroState, MODULES_ZERO_DESCRIPTION, MODULES_ZERO_TITLE } from "./ModulesZeroState.js";

describe("ModulesZeroState", () => {
  it("shows themed art, the title and the explanation", () => {
    const { container } = render(<ModulesZeroState canCreate onCreate={vi.fn()} />);
    expect(screen.getByTestId("modules-zero-state")).toBeInTheDocument();
    expect(container.querySelector("img")).not.toBeNull();
    const heading = screen.getByRole("heading");
    expect(heading).toHaveTextContent(MODULES_ZERO_TITLE);
    expect(heading).toHaveTextContent(MODULES_ZERO_DESCRIPTION);
  });

  it("opens creation for an editor", () => {
    const onCreate = vi.fn();
    render(<ModulesZeroState canCreate onCreate={onCreate} />);
    const action = screen.getByRole("button", { name: "Set your first module" });
    expect(action).toBeEnabled();
    fireEvent.click(action);
    expect(onCreate).toHaveBeenCalledTimes(1);
  });

  it("keeps the same copy with the action disabled for a guest", () => {
    const onCreate = vi.fn();
    render(<ModulesZeroState canCreate={false} onCreate={onCreate} />);
    expect(screen.getByRole("heading")).toHaveTextContent(MODULES_ZERO_TITLE);
    const action = screen.getByRole("button", { name: "Set your first module" });
    expect(action).toBeDisabled();
    fireEvent.click(action);
    expect(onCreate).not.toHaveBeenCalled();
  });
});
