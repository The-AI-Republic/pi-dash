// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// The gone-away view names the missing module and routes back to the
// list (MOD-029).

import { fireEvent, render, screen } from "@testing-library/react";
import * as React from "react";
import { describe, expect, it, vi } from "vitest";

import { ModuleGoneAway } from "./ModuleGoneAway.js";

describe("ModuleGoneAway", () => {
  it("names the missing module and routes back to the list", () => {
    const onBack = vi.fn();
    render(<ModuleGoneAway onBack={onBack} />);
    expect(screen.getByTestId("modules-gone-away")).toBeInTheDocument();
    expect(screen.getByText("Module does not exist")).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "Back to modules" }));
    expect(onBack).toHaveBeenCalledTimes(1);
  });
});
