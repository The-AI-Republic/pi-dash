// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import * as React from "react";
import { afterEach, describe, expect, it, vi } from "vitest";

import { WidgetAddButton, WidgetSection, loadWidgetOpen, storeWidgetOpen, widgetOpenKey } from "./WidgetSection.js";

afterEach(() => {
  window.localStorage.clear();
  vi.restoreAllMocks();
});

describe("widget section", () => {
  it("builds a namespaced preference key", () => {
    expect(widgetOpenKey("Relations")).toBe("pidash:issue-widget-open:Relations");
  });

  it("defaults open and ignores foreign stored values", () => {
    expect(loadWidgetOpen("Relations")).toBe(true);
    window.localStorage.setItem(widgetOpenKey("Relations"), "not-a-boolean");
    expect(loadWidgetOpen("Relations")).toBe(true);
    storeWidgetOpen("Relations", false);
    expect(window.localStorage.getItem(widgetOpenKey("Relations"))).toBe("false");
    expect(loadWidgetOpen("Relations")).toBe(false);
  });

  it("shows the count, add control and children when open", async () => {
    const add = vi.fn();
    render(
      <WidgetSection title="Links" count={3} add={<WidgetAddButton label="Add link" onClick={add} />}>
        <p>rows</p>
      </WidgetSection>
    );
    expect(screen.getByRole("button", { name: /Links/ })).toHaveTextContent("3");
    expect(screen.getByText("rows")).toBeInTheDocument();
    await userEvent.click(screen.getByRole("button", { name: "Add link" }));
    expect(add).toHaveBeenCalled();
  });

  it("collapses children and persists the flag", async () => {
    const user = userEvent.setup();
    render(
      <WidgetSection title="Links" count={1}>
        <p>rows</p>
      </WidgetSection>
    );
    await user.click(screen.getByRole("button", { name: /Links/ }));
    expect(screen.queryByText("rows")).not.toBeInTheDocument();
    expect(loadWidgetOpen("Links")).toBe(false);
    expect(screen.getByTestId("widget-links")).toBeInTheDocument();
  });

  it("accepts a custom test id and renders no add span when omitted", () => {
    render(
      <WidgetSection title="Relations" count={0} testId="custom-section">
        <p>empty</p>
      </WidgetSection>
    );
    expect(screen.getByTestId("custom-section")).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: /Add/ })).not.toBeInTheDocument();
  });
});
