// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Workspace drafts route (parity/specs/issues.md §12, ISS-137–141): the
// head claim, the page mount, and the create-project dialog it hosts.

import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { fireEvent, render, screen } from "@testing-library/react";
import * as React from "react";
import { afterAll, afterEach, beforeAll, beforeEach, describe, expect, it, vi } from "vitest";

const harness = vi.hoisted(() => ({
  params: { ws: "acme" },
  heads: [] as Array<Record<string, unknown>>,
  createOpen: false,
}));

vi.mock("@tanstack/react-router", () => ({
  createFileRoute: () => (options: unknown) => ({ options, useParams: () => harness.params }),
}));

vi.mock("../../../../shared/shell/index.js", () => ({
  useDocumentHead: (head: Record<string, unknown>) => {
    harness.heads.push(head);
  },
}));

vi.mock("../../../../features/issues/drafts/DraftsPage.js", () => ({
  DraftsPage: ({
    workspaceSlug,
    onCreateProject,
    CreateDialog,
  }: {
    workspaceSlug: string;
    onCreateProject: () => void;
    CreateDialog: React.ComponentType<Record<string, unknown>>;
  }) => (
    <div data-testid="drafts-page" data-ws={workspaceSlug}>
      <button type="button" onClick={onCreateProject}>
        open create project
      </button>
      <React.Suspense fallback={<div data-testid="dialog-pending" />}>
        <CreateDialog open onOpenChange={() => undefined} workspaceSlug={workspaceSlug} projects={[]} />
      </React.Suspense>
    </div>
  ),
}));

vi.mock("../../../../features/issues/components/WorkItemCreateDialog.js", () => ({
  WorkItemCreateDialog: ({ workspaceSlug }: { workspaceSlug: string }) => (
    <div data-testid="work-item-dialog" data-ws={workspaceSlug} />
  ),
}));

vi.mock("../../../../features/projects/index.js", () => ({
  workspaceMembersQueryOptions: () => ({ queryKey: ["members", "acme"], queryFn: () => Promise.resolve([]) }),
  CreateProjectDialog: ({ open }: { open: boolean }) => {
    harness.createOpen = open;
    return open ? <div data-testid="create-project-dialog" /> : null;
  },
}));

import { createAppClient, resetAppClient } from "../../../../core/api/client.js";
import { DRAFTS_HEAD_TITLE, Route } from "./index.js";

beforeAll(() => {
  createAppClient("https://api.test", (async (url: string) => ({
    status: 200,
    url,
    headers: { get: () => null },
    text: async () => "[]",
  })) as never);
});

beforeEach(() => {
  harness.heads = [];
  harness.createOpen = false;
});

afterEach(() => {
  harness.params = { ws: "acme" };
});

afterAll(() => {
  resetAppClient();
});

function renderRoute(): void {
  const Component = Route.options.component!;
  const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  render(
    <QueryClientProvider client={queryClient}>
      <Component />
    </QueryClientProvider>
  );
}

describe("drafts route", () => {
  it("claims the pinned Workspace Draft document head", () => {
    renderRoute();
    expect(harness.heads).toContainEqual({ title: DRAFTS_HEAD_TITLE, robots: "noindex, nofollow" });
    expect(DRAFTS_HEAD_TITLE).toBe("Workspace Draft");
  });

  it("mounts the page for the route's workspace", () => {
    renderRoute();
    expect(screen.getByTestId("drafts-page")).toHaveAttribute("data-ws", "acme");
  });

  it("loads the shared work-item dialog on first use", async () => {
    renderRoute();
    // The dialog is a separate chunk; once it lands it is mounted with
    // the route's workspace.
    expect(await screen.findByTestId("work-item-dialog")).toHaveAttribute("data-ws", "acme");
  });

  it("opens the create-project dialog the page asks for", () => {
    renderRoute();
    expect(harness.createOpen).toBe(false);
    fireEvent.click(screen.getByRole("button", { name: "open create project" }));
    expect(harness.createOpen).toBe(true);
    expect(screen.getByTestId("create-project-dialog")).toBeInTheDocument();
  });
});
