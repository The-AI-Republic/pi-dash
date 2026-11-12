// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { fireEvent, render, screen } from "@testing-library/react";
import * as React from "react";
import { describe, expect, it, vi } from "vitest";

import { ProjectViewsError } from "./ProjectViewsError.js";

describe("ProjectViewsError", () => {
  it("offers a retry", () => {
    const onRetry = vi.fn();
    render(<ProjectViewsError onRetry={onRetry} />);
    expect(screen.getByTestId("project-views-error")).toBeInTheDocument();
    const retry = screen.getByRole("button", { name: "Retry" });
    fireEvent.click(retry);
    expect(onRetry).toHaveBeenCalledTimes(1);
  });
});
