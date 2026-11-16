// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import * as React from "react";
import { afterEach, describe, expect, it, vi } from "vitest";

import { createAppClient, resetAppClient } from "../../../core/api/client.js";
import { selectCreatableProjects } from "../create/createForm.js";
import { QuickCreateButton } from "./QuickCreateButton.js";

const PROJECTS = [
  { id: "p1", name: "Atlas", identifier: "ATL", memberRole: 15, archived: false },
  { id: "p2", name: "Guestland", identifier: "GST", memberRole: 5, archived: false },
  { id: "p3", name: "Old Atlas", identifier: "OLD", memberRole: 20, archived: true },
];

// The first click pulls the dialog chunk (form primitives, mutation
// wiring). Under a full-suite run that import can take seconds; async
// queries poll on the shared budget from src/test-setup.ts, which covers
// a loaded host (see src/testing/testBudget.ts). The test timeout stays
// above that budget, since vitest stops a test at 5 s by default.
const OPEN_TEST_TIMEOUT = 30_000;

function setup(projects: typeof PROJECTS = PROJECTS, canCreate = true) {
  createAppClient("https://api.test", (async (url: string) => ({
    status: 200,
    url,
    headers: { get: () => null },
    text: async () => JSON.stringify({ is_configured: false }),
  })) as never);
  const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  render(
    <QueryClientProvider client={queryClient}>
      <QuickCreateButton workspaceSlug="acme" projects={projects} canCreate={canCreate} />
    </QueryClientProvider>
  );
}

afterEach(() => {
  resetAppClient();
});

describe("QuickCreateButton", () => {
  it(
    "opens the creation dialog for eligible members",
    async () => {
      const user = userEvent.setup();
      setup();
      const entry = screen.getByRole("button", { name: "New work item" });
      expect(entry).toBeEnabled();
      await user.click(entry);
      // The dialog rides a lazy chunk, so the first open spans a module
      // load; wait for that import, then poll on the shared async-query
      // budget.
      await vi.dynamicImportSettled();
      expect(await screen.findByRole("dialog", { name: "Create new work item" })).toBeInTheDocument();
    },
    OPEN_TEST_TIMEOUT
  );

  it("stays disabled for guests", () => {
    setup(PROJECTS, false);
    expect(screen.getByRole("button", { name: "New work item" })).toBeDisabled();
  });

  it("stays disabled with zero joined projects", () => {
    setup([], true);
    expect(screen.getByRole("button", { name: "New work item" })).toBeDisabled();
  });

  it("stays disabled when every joined project is a guest membership", () => {
    // A workspace member who can only guest in the projects they joined
    // may not file work anywhere, so the entry stays off (ISS-117).
    setup([PROJECTS[1]!, PROJECTS[2]!], true);
    expect(screen.getByRole("button", { name: "New work item" })).toBeDisabled();
  });

  it("carries each project's own membership into the dialog's create list", () => {
    // A workspace admin who is only a guest on a project must not be able
    // to file there (ISS-117), so the rows carry their own roles.
    expect(selectCreatableProjects(PROJECTS).map((project) => project.id)).toEqual(["p1"]);
  });
});
