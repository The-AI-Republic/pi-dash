// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import * as React from "react";
import { describe, expect, it, vi } from "vitest";

import { LeaveProjectEntry } from "./LeaveProjectEntry.js";

// The lazy chunk pulls the dialog, its primitives and the mutation wiring
// on first open; the first import can outlast the default 1 s find timeout
// when the suite runs the whole tree.
const OPEN_TIMEOUT = { timeout: 10_000 };

function renderEntry(open: boolean) {
  const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  const onNavigate = vi.fn();
  render(
    <QueryClientProvider client={queryClient}>
      <LeaveProjectEntry
        workspaceSlug="acme"
        projectId="223e4567-e89b-12d3-a456-426614174001"
        projectName="Web"
        open={open}
        onOpenChange={() => undefined}
        onNavigate={onNavigate}
      />
    </QueryClientProvider>
  );
  return { onNavigate };
}

describe("LeaveProjectEntry", () => {
  it("mounts nothing until the first open", () => {
    renderEntry(false);
    expect(screen.queryByRole("heading", { name: "Leave project" })).not.toBeInTheDocument();
  });

  it("loads and shows the confirmation flow once opened", async () => {
    renderEntry(true);
    expect(await screen.findByRole("heading", { name: "Leave project" }, OPEN_TIMEOUT)).toBeInTheDocument();
  });

  it("forwards close requests from the confirmation", async () => {
    const user = userEvent.setup();
    const onOpenChange = vi.fn();
    const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false } } });
    render(
      <QueryClientProvider client={queryClient}>
        <LeaveProjectEntry
          workspaceSlug="acme"
          projectId="223e4567-e89b-12d3-a456-426614174001"
          projectName="Web"
          open
          onOpenChange={onOpenChange}
          onNavigate={() => undefined}
        />
      </QueryClientProvider>
    );
    await user.click(await screen.findByRole("button", { name: "Cancel" }, OPEN_TIMEOUT));
    expect(onOpenChange).toHaveBeenCalledTimes(1);
    expect(onOpenChange.mock.calls[0]?.[0]).toBe(false);
  });

  it("mounts a fresh, empty confirmation on every open", async () => {
    const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false } } });
    const props = {
      workspaceSlug: "acme",
      projectId: "223e4567-e89b-12d3-a456-426614174001",
      projectName: "Web",
      onOpenChange: () => undefined,
      onNavigate: () => undefined,
    };
    const { rerender } = render(
      <QueryClientProvider client={queryClient}>
        <LeaveProjectEntry {...props} open />
      </QueryClientProvider>
    );
    const user = userEvent.setup();
    expect(await screen.findByRole("heading", { name: "Leave project" }, OPEN_TIMEOUT)).toBeInTheDocument();
    await user.type(screen.getByLabelText("Enter the project name Web to continue:"), "Web");
    expect(screen.getByLabelText("Enter the project name Web to continue:")).toHaveValue("Web");

    rerender(
      <QueryClientProvider client={queryClient}>
        <LeaveProjectEntry {...props} open={false} />
      </QueryClientProvider>
    );
    rerender(
      <QueryClientProvider client={queryClient}>
        <LeaveProjectEntry {...props} open />
      </QueryClientProvider>
    );
    expect(await screen.findByRole("heading", { name: "Leave project" }, OPEN_TIMEOUT)).toBeInTheDocument();
    expect(screen.getByLabelText("Enter the project name Web to continue:")).toHaveValue("");
  });
});
