// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { render, screen, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import * as React from "react";
import { describe, expect, it, vi } from "vitest";

import { StreamTabs } from "./StreamTabs.js";

function tabs(): HTMLElement[] {
  return screen.getAllByRole("tab");
}

describe("StreamTabs", () => {
  it("renders exactly the full stream and mentions tabs", () => {
    render(<StreamTabs active="all" totalUnread={0} mentionUnread={0} onSelect={() => {}} />);
    expect(screen.getByRole("tablist", { name: "Inbox streams" })).toBeInTheDocument();
    expect(tabs()).toHaveLength(2);
    expect(screen.getByRole("tab", { name: "All" })).toHaveAttribute("aria-selected", "true");
    expect(screen.getByRole("tab", { name: "Mentions" })).toHaveAttribute("aria-selected", "false");
  });

  it("mirrors each server total on its own tab", () => {
    render(<StreamTabs active="mentions" totalUnread={7} mentionUnread={2} onSelect={() => {}} />);
    expect(screen.getByTestId("inbox-tab-mentions")).toHaveAttribute("aria-selected", "true");
    expect(within(screen.getByTestId("inbox-tab-all")).getByText("7")).toBeInTheDocument();
    expect(within(screen.getByTestId("inbox-tab-mentions")).getByText("2")).toBeInTheDocument();
  });

  it("hides a tab badge at zero", () => {
    render(<StreamTabs active="all" totalUnread={4} mentionUnread={0} onSelect={() => {}} />);
    expect(within(screen.getByTestId("inbox-tab-all")).getByText("4")).toBeInTheDocument();
    expect(within(screen.getByTestId("inbox-tab-mentions")).queryByText("0")).not.toBeInTheDocument();
  });

  it("selects the clicked tab", async () => {
    const user = userEvent.setup();
    const onSelect = vi.fn();
    render(<StreamTabs active="all" totalUnread={1} mentionUnread={1} onSelect={onSelect} />);
    await user.click(screen.getByRole("tab", { name: /mentions/i }));
    expect(onSelect).toHaveBeenCalledWith("mentions");
    expect(onSelect).toHaveBeenCalledTimes(1);
  });
});
