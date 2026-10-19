// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import * as React from "react";
import { afterEach, describe, expect, it } from "vitest";

import { createAppClient, resetAppClient } from "../../../core/api/client.js";
import { QuickCreateButton } from "./QuickCreateButton.js";

const PROJECTS = [{ id: "p1", name: "Atlas" }];

function setup(
  projects: Array<{ id: string; name: string }> = PROJECTS,
  canCreate = true,
  memberRole: number | null = 15
) {
  createAppClient("https://api.test", (async (url: string) => ({
    status: 200,
    url,
    headers: { get: () => null },
    text: async () => JSON.stringify({ is_configured: false }),
  })) as never);
  const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  render(
    <QueryClientProvider client={queryClient}>
      <QuickCreateButton workspaceSlug="acme" projects={projects} canCreate={canCreate} memberRole={memberRole} />
    </QueryClientProvider>
  );
}

afterEach(() => {
  resetAppClient();
});

describe("QuickCreateButton", () => {
  it("opens the creation dialog for eligible members", async () => {
    const user = userEvent.setup();
    setup();
    const entry = screen.getByRole("button", { name: "New work item" });
    expect(entry).toBeEnabled();
    await user.click(entry);
    // The dialog rides a lazy chunk, so the first open spans a module load.
    expect(await screen.findByRole("dialog", { name: "Create new work item" }, { timeout: 5000 })).toBeInTheDocument();
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
