// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { fireEvent, render, screen } from "@testing-library/react";
import * as React from "react";
import { describe, expect, it, vi } from "vitest";

import { ProjectViewsZeroState } from "./ProjectViewsZeroState.js";

describe("ProjectViewsZeroState", () => {
  it("onboards with an enabled creation shortcut", () => {
    const onCreateView = vi.fn();
    render(<ProjectViewsZeroState onCreateView={onCreateView} />);
    expect(screen.getByTestId("project-views-zero")).toBeInTheDocument();
    expect(screen.getByRole("heading", { name: /Save custom views for your project/ })).toBeInTheDocument();
    expect(screen.getByText("Save custom views for your project")).toBeInTheDocument();
    const create = screen.getByRole("button", { name: "Create view" });
    expect(create).toBeEnabled();
    fireEvent.click(create);
    expect(onCreateView).toHaveBeenCalledTimes(1);
  });

  it("stays enabled without a dialog opener until NEWFRONT-721 lands", () => {
    render(<ProjectViewsZeroState />);
    const create = screen.getByRole("button", { name: "Create view" });
    expect(create).toBeEnabled();
    fireEvent.click(create);
  });
});
