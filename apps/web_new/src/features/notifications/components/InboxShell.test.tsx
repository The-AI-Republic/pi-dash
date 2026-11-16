// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { render, screen } from "@testing-library/react";
import * as React from "react";
import { describe, expect, it } from "vitest";

import { InboxShell } from "./InboxShell.js";

describe("InboxShell", () => {
  it("renders the list and detail panes side by side", () => {
    render(<InboxShell list={<div data-testid="list-content" />} detail={<div data-testid="detail-content" />} />);
    const listPane = screen.getByTestId("inbox-list-pane");
    expect(listPane).toHaveAttribute("aria-label", "Notifications");
    expect(screen.getByTestId("list-content")).toBeInTheDocument();
    expect(listPane).toContainElement(screen.getByTestId("list-content"));
    const detailPane = screen.getByTestId("inbox-detail-pane");
    expect(detailPane).toHaveAttribute("aria-label", "Details");
    expect(detailPane).toContainElement(screen.getByTestId("detail-content"));
    expect(listPane).toHaveClass("flex");
    expect(detailPane).toHaveClass("hidden", "md:block");
  });

  it("gives a narrow viewport to the selected detail", () => {
    render(
      <InboxShell selected list={<div data-testid="list-content" />} detail={<div data-testid="detail-content" />} />
    );
    expect(screen.getByTestId("inbox-list-pane")).toHaveClass("hidden", "md:flex");
    expect(screen.getByTestId("inbox-detail-pane")).toHaveClass("block");
  });
});
