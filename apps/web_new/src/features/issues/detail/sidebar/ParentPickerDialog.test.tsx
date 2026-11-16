// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import type { TransportInit } from "@pidash/api-client";
import * as React from "react";
import { afterEach, describe, expect, it, vi } from "vitest";

import { createAppClient, resetAppClient } from "../../../../core/api/client.js";
import { ParentPickerDialog } from "./ParentPickerDialog.js";

const SLUG = "acme";
const PROJECT = "223e4567-e89b-12d3-a456-426614174001";
const ISSUE = "123e4567-e89b-12d3-a456-426614174000";
const HIT = "423e4567-e89b-12d3-a456-426614174003";

const HITS = [
  {
    name: "Parent epic",
    id: HIT,
    start_date: null,
    sequence_id: 9,
    project__name: "Web",
    project__identifier: "WEB",
    project_id: PROJECT,
    workspace__slug: SLUG,
    state__name: "In progress",
    state__group: "started",
    state__color: "#ffaa00",
  },
  {
    name: "Other item",
    id: "523e4567-e89b-12d3-a456-426614174004",
    start_date: null,
    sequence_id: 10,
    project__name: "Web",
    project__identifier: "WEB",
    project_id: PROJECT,
    workspace__slug: SLUG,
    state__name: null,
    state__group: null,
    state__color: null,
  },
];

interface Options {
  status?: number;
  rows?: unknown[];
  hold?: boolean;
}

function harness(options: Options = {}) {
  resetAppClient();
  const reads: string[] = [];
  let release: (() => void) | null = null;
  const gate = options.hold
    ? new Promise<void>((resolve) => {
        release = resolve;
      })
    : null;
  const conversation = async (url: string, init: TransportInit) => {
    if (init.method === "GET") reads.push(url);
    if (url.includes("/search-issues/")) {
      if (gate !== null) await gate;
      const status = options.status ?? 200;
      return {
        status,
        url,
        headers: { get: () => null },
        text: async () => JSON.stringify(status === 200 ? (options.rows ?? HITS) : { error: "no" }),
      };
    }
    return { status: 404, url, headers: { get: () => null }, text: async () => "{}" };
  };
  const client = createAppClient("https://api.test", conversation as never);
  const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  return { client, queryClient, reads, release: () => release?.() };
}

function renderDialog(
  client: ReturnType<typeof harness>["client"],
  queryClient: QueryClient,
  props: Partial<React.ComponentProps<typeof ParentPickerDialog>> = {}
) {
  const onPick = vi.fn();
  const onOpenChange = vi.fn();
  render(
    <QueryClientProvider client={queryClient}>
      <ParentPickerDialog
        workspaceSlug={SLUG}
        projectId={PROJECT}
        issueId={ISSUE}
        open
        onOpenChange={onOpenChange}
        onPick={onPick}
        {...props}
      />
    </QueryClientProvider>
  );
  return { onPick, onOpenChange };
}

afterEach(() => {
  cleanup();
  resetAppClient();
});

describe("ParentPickerDialog", () => {
  it("names the dialog and searches once it opens", async () => {
    const { client, queryClient, reads } = harness();
    renderDialog(client, queryClient);
    expect(screen.getByRole("dialog")).toHaveTextContent("Set parent work item");
    expect(screen.getByLabelText("Search work items")).toBeInTheDocument();
    expect(await screen.findByRole("option", { name: /Parent epic/ })).toBeInTheDocument();
    const url = reads.find((entry) => entry.includes("/search-issues/")) ?? "";
    expect(url).toContain(`/projects/${PROJECT}/search-issues/`);
    expect(url).toContain("parent=true");
    expect(url).toContain(`issue_id=${ISSUE}`);
  });

  it("reports the close request from its close control", async () => {
    const { client, queryClient } = harness();
    const { onOpenChange } = renderDialog(client, queryClient);
    fireEvent.click(screen.getByRole("button", { name: "Close dialog" }));
    expect(onOpenChange).toHaveBeenCalledWith(false);
  });

  it("shows the state dot, identifier and name for each hit", async () => {
    const { client, queryClient } = harness();
    renderDialog(client, queryClient);
    const first = await screen.findByRole("option", { name: /Parent epic/ });
    expect(first).toHaveTextContent("WEB-9");
    expect(first).toHaveTextContent("Parent epic");
    expect(first.querySelector("span[aria-hidden='true']")?.getAttribute("style")).toContain("rgb(255, 170, 0)");
    const second = screen.getByRole("option", { name: /Other item/ });
    expect(second.querySelector("span[aria-hidden='true']")?.getAttribute("style")).toBeNull();
  });

  it("debounces the typed query into the request", async () => {
    const { client, queryClient, reads } = harness();
    renderDialog(client, queryClient);
    await screen.findByRole("option", { name: /Parent epic/ });
    fireEvent.change(screen.getByLabelText("Search work items"), { target: { value: "Parent" } });
    expect(reads.some((url) => url.includes("search=Parent"))).toBe(false);
    await waitFor(() => expect(reads.some((url) => url.includes("search=Parent"))).toBe(true));
  });

  it("picks a hit, closes and reports the id", async () => {
    const { client, queryClient } = harness();
    const { onPick, onOpenChange } = renderDialog(client, queryClient);
    fireEvent.click(await screen.findByRole("option", { name: /Parent epic/ }));
    expect(onPick).toHaveBeenCalledWith(HIT);
    expect(onOpenChange).toHaveBeenCalledWith(false);
  });

  it("holds skeletons while the search runs", async () => {
    const { client, queryClient, release } = harness({ hold: true });
    renderDialog(client, queryClient);
    expect(screen.getByRole("status", { name: "Searching work items" })).toBeInTheDocument();
    release();
    expect(await screen.findByRole("option", { name: /Parent epic/ })).toBeInTheDocument();
  });

  it("names an empty answer", async () => {
    const { client, queryClient } = harness({ rows: [] });
    renderDialog(client, queryClient);
    expect(await screen.findByText("No matching work items")).toBeInTheDocument();
  });

  it("reports a refused search", async () => {
    const { client, queryClient } = harness({ status: 500 });
    renderDialog(client, queryClient);
    expect(await screen.findByRole("alert")).toHaveTextContent("Couldn't search the work items.");
  });

  it("clears the typed query when it closes", async () => {
    const { queryClient } = harness();
    const { rerender } = render(
      <QueryClientProvider client={queryClient}>
        <ParentPickerDialog
          workspaceSlug={SLUG}
          projectId={PROJECT}
          issueId={ISSUE}
          open
          onOpenChange={vi.fn()}
          onPick={vi.fn()}
        />
      </QueryClientProvider>
    );
    fireEvent.change(screen.getByLabelText("Search work items"), { target: { value: "typed" } });
    rerender(
      <QueryClientProvider client={queryClient}>
        <ParentPickerDialog
          workspaceSlug={SLUG}
          projectId={PROJECT}
          issueId={ISSUE}
          open={false}
          onOpenChange={vi.fn()}
          onPick={vi.fn()}
        />
      </QueryClientProvider>
    );
    rerender(
      <QueryClientProvider client={queryClient}>
        <ParentPickerDialog
          workspaceSlug={SLUG}
          projectId={PROJECT}
          issueId={ISSUE}
          open
          onOpenChange={vi.fn()}
          onPick={vi.fn()}
        />
      </QueryClientProvider>
    );
    await waitFor(() => expect(screen.getByLabelText("Search work items")).toHaveValue(""));
  });
});
