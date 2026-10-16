// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import * as React from "react";
import { describe, expect, it } from "vitest";

import { QuickCreateButton } from "./QuickCreateButton.js";

const PROJECTS = [{ id: "p1", name: "Atlas" }];

function setup(projects: Array<{ id: string; name: string }> = PROJECTS, canCreate = true) {
  const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  render(
    <QueryClientProvider client={queryClient}>
      <QuickCreateButton workspaceSlug="acme" projects={projects} canCreate={canCreate} />
    </QueryClientProvider>
  );
}

describe("QuickCreateButton", () => {
  it("opens the creation dialog for eligible members", async () => {
    const user = userEvent.setup();
    setup();
    const entry = screen.getByRole("button", { name: "New work item" });
    expect(entry).toBeEnabled();
    await user.click(entry);
    expect(await screen.findByRole("dialog", { name: "Create new work item" })).toBeInTheDocument();
  });

  it("stays disabled for guests", () => {
    setup(PROJECTS, false);
    expect(screen.getByRole("button", { name: "New work item" })).toBeDisabled();
  });

  it("stays disabled with zero joined projects", () => {
    setup([], true);
    expect(screen.getByRole("button", { name: "New work item" })).toBeDisabled();
  });
});
