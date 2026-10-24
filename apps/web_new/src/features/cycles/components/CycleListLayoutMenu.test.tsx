// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// The narrow-screen list presentation menu (CYC-048): nothing
// renders on wide viewports; narrow screens get a menu offering
// exactly "List layout" and "Gallery layout", reporting a newly
// chosen presentation and skipping the current one.

import { fireEvent, render, screen } from "@testing-library/react";
import * as React from "react";
import { beforeEach, describe, expect, it, vi } from "vitest";

const viewportHarness = vi.hoisted(() => ({ narrow: false }));

vi.mock("../../../shared/shell/index.js", () => ({
  useNarrowViewport: () => viewportHarness.narrow,
}));

import { CycleListLayoutMenu } from "./CycleListLayoutMenu.js";

beforeEach(() => {
  viewportHarness.narrow = false;
});

describe("CycleListLayoutMenu", () => {
  it("renders nothing on wide viewports", () => {
    render(<CycleListLayoutMenu layout="list" onLayoutChange={vi.fn()} />);
    expect(screen.queryByTestId("cycles-list-layout-menu")).not.toBeInTheDocument();
  });

  it("shows the current presentation on its narrow trigger", () => {
    viewportHarness.narrow = true;
    render(<CycleListLayoutMenu layout="gallery" onLayoutChange={vi.fn()} />);
    expect(screen.getByTestId("cycles-list-layout-menu")).toBeInTheDocument();
    expect(screen.getByTestId("cycles-list-layout-trigger")).toHaveTextContent("Gallery layout");
  });

  it("offers exactly the list and gallery entries", () => {
    viewportHarness.narrow = true;
    render(<CycleListLayoutMenu layout="list" onLayoutChange={vi.fn()} />);
    fireEvent.click(screen.getByTestId("cycles-list-layout-trigger"));
    expect(screen.getAllByRole("menuitem").map((item) => item.textContent)).toEqual(["List layout", "Gallery layout"]);
  });

  it("reports a newly chosen presentation and skips the current one", () => {
    viewportHarness.narrow = true;
    const onLayoutChange = vi.fn();
    render(<CycleListLayoutMenu layout="list" onLayoutChange={onLayoutChange} />);
    fireEvent.click(screen.getByTestId("cycles-list-layout-trigger"));
    fireEvent.click(screen.getByRole("menuitem", { name: "Gallery layout" }));
    expect(onLayoutChange).toHaveBeenCalledWith("gallery");
    fireEvent.click(screen.getByTestId("cycles-list-layout-trigger"));
    fireEvent.click(screen.getByRole("menuitem", { name: "List layout" }));
    expect(onLayoutChange).toHaveBeenCalledTimes(1);
  });
});
