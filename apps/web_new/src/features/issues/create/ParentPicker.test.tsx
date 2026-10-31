// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Parent picker (parity/specs/issues.md §11, ISS-122): the add chip, the
// half-second debounced project search, state-dot candidate rows with
// open-in-new-tab links, the chosen parent tag and its remove control.

import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import type { IssueSearchHit } from "@pidash/api-client";
import * as React from "react";
import { afterEach, describe, expect, it, vi } from "vitest";

import { createAppClient, resetAppClient } from "../../../core/api/client.js";
import { ParentPicker, parentBrowseHref } from "./ParentPicker.js";

const PARENT: IssueSearchHit = {
  id: "123e4567-e89b-12d3-a456-42661417400a",
  name: "Parent work",
  start_date: null,
  sequence_id: 12,
  project__name: "Atlas",
  project__identifier: "ATL",
  project_id: "223e4567-e89b-12d3-a456-426614174001",
  workspace__slug: "acme",
  state__name: "In Progress",
  state__group: "started",
  state__color: "#ff9600",
};

function stub(hits: IssueSearchHit[] | Promise<IssueSearchHit[]> = [PARENT]) {
  const calls: string[] = [];
  createAppClient("https://api.test", (async (url: string) => {
    calls.push(url);
    const rows = await hits;
    return { status: 200, url, headers: { get: () => null }, text: async () => JSON.stringify(rows) };
  }) as never);
  const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  return { calls, queryClient };
}

function setup(props: Partial<React.ComponentProps<typeof ParentPicker>> = {}) {
  const stubHandle = stub();
  const onChange = vi.fn();
  const view = render(
    <QueryClientProvider client={stubHandle.queryClient}>
      <ParentPicker workspaceSlug="acme" projectId={PARENT.project_id} value={null} onChange={onChange} {...props} />
    </QueryClientProvider>
  );
  return { ...stubHandle, onChange, view };
}

afterEach(() => {
  resetAppClient();
});

describe("ParentPicker", () => {
  it("opens a project search and lists candidates with state and new-tab links", async () => {
    const user = userEvent.setup();
    setup();
    await user.click(screen.getByRole("button", { name: "Add parent" }));
    await screen.findByRole("searchbox", { name: "Search work items" });
    expect(await screen.findByText("Parent work")).toBeInTheDocument();
    const link = screen.getByRole("link", { name: "Open ATL-12 in a new tab" });
    expect(link).toHaveAttribute("href", "/acme/browse/ATL-12");
    expect(link).toHaveAttribute("target", "_blank");
    expect(screen.getByText("ATL-12")).toBeInTheDocument();
  });

  it("waits half a second before searching on a typed term", async () => {
    const user = userEvent.setup();
    const h = setup();
    await user.click(screen.getByRole("button", { name: "Add parent" }));
    await waitFor(() => expect(h.calls.some((url) => url.includes("search="))).toBe(true));
    const before = h.calls.filter((url) => url.includes("search=par")).length;
    await user.type(screen.getByRole("searchbox", { name: "Search work items" }), "par");
    expect(h.calls.filter((url) => url.includes("search=par"))).toHaveLength(before);
    await waitFor(() => expect(h.calls.some((url) => url.includes("search=par"))).toBe(true));
  });

  it("picks a candidate, renders the parent tag and removes it", async () => {
    const user = userEvent.setup();
    const { queryClient } = stub();
    const onChange = vi.fn();
    function Host(): React.ReactElement {
      const [value, setValue] = React.useState<IssueSearchHit | null>(null);
      return (
        <ParentPicker
          workspaceSlug="acme"
          projectId={PARENT.project_id}
          value={value}
          onChange={(next) => {
            onChange(next);
            setValue(next);
          }}
        />
      );
    }
    render(
      <QueryClientProvider client={queryClient}>
        <Host />
      </QueryClientProvider>
    );
    await user.click(screen.getByRole("button", { name: "Add parent" }));
    await user.click(await screen.findByRole("button", { name: /ATL-12/ }));
    expect(onChange).toHaveBeenCalledWith(PARENT);
    const tag = await screen.findByTestId("parent-tag");
    expect(within(tag).getByText("ATL-12")).toBeInTheDocument();
    expect(within(tag).getByText("Parent work")).toBeInTheDocument();
    expect(within(tag).getByText("In Progress")).toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: "Remove parent ATL-12" }));
    expect(onChange).toHaveBeenLastCalledWith(null);
    expect(screen.queryByTestId("parent-tag")).not.toBeInTheDocument();
  });

  it("renders a chosen parent without the add chip", () => {
    setup({ value: PARENT });
    expect(screen.queryByRole("button", { name: "Add parent" })).not.toBeInTheDocument();
    expect(screen.getByTestId("parent-tag")).toBeInTheDocument();
  });

  it("disables the add chip when the form is busy", () => {
    setup({ disabled: true });
    expect(screen.getByRole("button", { name: "Add parent" })).toBeDisabled();
  });

  it("builds the browse href from the workspace and the short key", () => {
    expect(parentBrowseHref("acme", PARENT)).toBe("/acme/browse/ATL-12");
  });

  it("falls back to muted dot and a no-state label for a bare row", async () => {
    const user = userEvent.setup();
    const bare: IssueSearchHit = { ...PARENT, state__name: null, state__color: null };
    const { queryClient } = stub([bare]);
    render(
      <QueryClientProvider client={queryClient}>
        <ParentPicker workspaceSlug="acme" projectId={PARENT.project_id} value={null} onChange={vi.fn()} />
      </QueryClientProvider>
    );
    await user.click(screen.getByRole("button", { name: "Add parent" }));
    const row = await screen.findByRole("button", { name: /ATL-12/ });
    const dot = row.querySelector("span");
    expect(dot).toHaveStyle({ backgroundColor: "var(--text-muted)" });
    setup({ value: bare });
    expect(screen.getByLabelText("State: No state")).toBeInTheDocument();
  });

  it("shows the searching line until the first result lands", async () => {
    const user = userEvent.setup();
    let release: (rows: IssueSearchHit[]) => void = () => undefined;
    const pending = new Promise<IssueSearchHit[]>((resolve) => {
      release = resolve;
    });
    const { queryClient } = stub(pending);
    render(
      <QueryClientProvider client={queryClient}>
        <ParentPicker workspaceSlug="acme" projectId={PARENT.project_id} value={null} onChange={vi.fn()} />
      </QueryClientProvider>
    );
    await user.click(screen.getByRole("button", { name: "Add parent" }));
    expect(await screen.findByText("Searching")).toBeInTheDocument();
    release([PARENT]);
    expect(await screen.findByText("Parent work")).toBeInTheDocument();
  });
});

describe("ParentPicker empty and error states", () => {
  it("explains an empty result set", async () => {
    const user = userEvent.setup();
    const { queryClient } = stub([]);
    render(
      <QueryClientProvider client={queryClient}>
        <ParentPicker workspaceSlug="acme" projectId={PARENT.project_id} value={null} onChange={vi.fn()} />
      </QueryClientProvider>
    );
    await user.click(screen.getByRole("button", { name: "Add parent" }));
    expect(await screen.findByText("No matching work items.")).toBeInTheDocument();
  });

  it("explains a failed search", async () => {
    const user = userEvent.setup();
    createAppClient("https://api.test", (async (url: string) => ({
      status: 500,
      url,
      headers: { get: () => null },
      text: async () => "{}",
    })) as never);
    const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false } } });
    render(
      <QueryClientProvider client={queryClient}>
        <ParentPicker workspaceSlug="acme" projectId={PARENT.project_id} value={null} onChange={vi.fn()} />
      </QueryClientProvider>
    );
    await user.click(screen.getByRole("button", { name: "Add parent" }));
    expect(await screen.findByText("Couldn't search work items.")).toBeInTheDocument();
  });
});
