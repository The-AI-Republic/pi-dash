// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import * as React from "react";
import { describe, expect, it, vi } from "vitest";

import { NotificationEmptyState, NotificationListSkeleton, NotificationNextPage } from "./ListStates.js";

describe("notification list states", () => {
  it("renders five first-load skeleton rows", () => {
    render(<NotificationListSkeleton />);
    expect(screen.getByTestId("notifications-skeleton")).toHaveAccessibleName("Loading notifications");
    expect(screen.getAllByTestId("notifications-skeleton-row")).toHaveLength(5);
  });

  it.each([
    ["all" as const, "Nothing has arrived", "Updates to your work will collect here."],
    ["mentions" as const, "No mentions waiting", "Mentions from your team will appear here."],
  ])("renders distinct %s empty art and copy", (tab, title, description) => {
    render(<NotificationEmptyState tab={tab} />);
    const empty = screen.getByTestId("notifications-empty-state");
    expect(empty).toHaveAttribute("data-empty-tab", tab);
    expect(screen.getByText(title)).toBeInTheDocument();
    expect(screen.getByText(description)).toBeInTheDocument();
  });

  it("activates the continuation with its idle label", async () => {
    const user = userEvent.setup();
    const onLoad = vi.fn();
    render(<NotificationNextPage loading={false} onLoad={onLoad} />);
    const control = screen.getByTestId("notifications-next-page");
    expect(control).toHaveTextContent("Show older notifications");
    expect(control).not.toBeDisabled();
    await user.click(control);
    expect(onLoad).toHaveBeenCalledOnce();
  });

  it("locks the continuation and changes its label mid-fetch", () => {
    render(<NotificationNextPage loading onLoad={() => {}} />);
    const control = screen.getByTestId("notifications-next-page");
    expect(control).toHaveTextContent("Loading older notifications");
    expect(control).toBeDisabled();
    expect(control).toHaveAttribute("aria-busy", "true");
  });
});
