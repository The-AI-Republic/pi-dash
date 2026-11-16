// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Analytics empty view tests (parity/specs/analytics.md §3, ANL-003).
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import * as React from "react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { createAppClient, resetAppClient } from "../../../core/api/client.js";

import { AnalyticsEmptyView } from "./AnalyticsEmptyView.js";

function renderView(canCreate: boolean): void {
  const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  render(
    <QueryClientProvider client={queryClient}>
      <AnalyticsEmptyView workspaceSlug="acme" canCreate={canCreate} />
    </QueryClientProvider>
  );
}

beforeEach(() => {
  vi.clearAllMocks();
  createAppClient("https://api.test", () => new Promise<never>(() => {}));
});

afterEach(() => resetAppClient());

describe("AnalyticsEmptyView", () => {
  it("renders the heading, supporting copy, and an enabled creation action", () => {
    renderView(true);
    expect(screen.getByTestId("analytics-empty")).toBeInTheDocument();
    expect(screen.getByRole("heading", { name: /No project/ })).toBeInTheDocument();
    const action = screen.getByRole("button", { name: "Create a project" });
    expect(action).toBeEnabled();
  });

  it("disables the creation action without creation standing", () => {
    renderView(false);
    expect(screen.getByRole("button", { name: "Create a project" })).toBeDisabled();
  });

  it("opens the shared project dialog from the action", async () => {
    const user = userEvent.setup();
    renderView(true);
    expect(screen.queryByRole("heading", { name: "New project" })).not.toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: "Create a project" }));
    expect(screen.getByRole("heading", { name: "New project" })).toBeInTheDocument();
  });
});
