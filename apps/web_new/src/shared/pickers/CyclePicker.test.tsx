// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { fireEvent, render, screen, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, describe, expect, it, vi } from "vitest";

import { createAppClient, resetAppClient } from "../../core/api/client.js";
import { CyclePicker } from "./CyclePicker.js";

const CURRENT = "aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa";
const DONE = "bbbbbbbb-bbbb-4bbb-8bbb-bbbbbbbbbbbb";
const PROJECT = "cccccccc-cccc-4ccc-8ccc-cccccccccccc";

const CYCLES = [
  { id: CURRENT, project_id: PROJECT, name: "Current", is_favorite: false, status: "ACTIVE" },
  { id: DONE, project_id: PROJECT, name: "Done", is_favorite: false, status: "COMPLETED" },
];

function harness(body: unknown, status = 200) {
  const calls: string[] = [];
  const transport = async (url: string) => {
    calls.push(url);
    if (url.endsWith("/auth/get-csrf-token/")) {
      return {
        status: 200,
        url,
        headers: { get: () => "application/json" },
        text: async () => '{"csrf_token":"csrf"}',
      };
    }
    return { status, url, headers: { get: () => "application/json" }, text: async () => JSON.stringify(body) };
  };
  createAppClient("https://api.test", transport as never);
  const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  return { queryClient, calls };
}

function optionNames(): string[] {
  return within(screen.getByRole("listbox"))
    .getAllByRole("option")
    .map((row) => row.querySelector(":scope > span.truncate")?.textContent ?? "");
}

afterEach(() => {
  resetAppClient();
});

describe("CyclePicker", () => {
  it("fetches lazily with a clear row and no completed cycles", async () => {
    const { queryClient, calls } = harness(CYCLES);
    render(
      <QueryClientProvider client={queryClient}>
        <CyclePicker workspaceSlug="acme" projectId={PROJECT} value={null} onChange={() => undefined} />
      </QueryClientProvider>
    );
    expect(calls).toEqual([]);
    fireEvent.click(screen.getByRole("button", { name: "No cycle" }));
    expect(await screen.findByRole("option", { name: "Current" })).toBeInTheDocument();
    expect(optionNames()).toEqual(["No cycle", "Current"]);
    expect(calls.some((url) => url.endsWith("/cycles/"))).toBe(true);
  });

  it("preloads the list so a set cycle names itself before the popup opens", async () => {
    const { queryClient, calls } = harness(CYCLES);
    render(
      <QueryClientProvider client={queryClient}>
        <CyclePicker workspaceSlug="acme" projectId={PROJECT} value={CURRENT} preload onChange={() => undefined} />
      </QueryClientProvider>
    );
    expect(calls.some((url) => url.endsWith("/cycles/"))).toBe(true);
    expect(await screen.findByRole("button", { name: "Current" })).toBeInTheDocument();
  });

  it("names a read-only row's value when preloaded", async () => {
    const { queryClient } = harness(CYCLES);
    render(
      <QueryClientProvider client={queryClient}>
        <CyclePicker
          workspaceSlug="acme"
          projectId={PROJECT}
          value={CURRENT}
          preload
          disabled
          onChange={() => undefined}
        />
      </QueryClientProvider>
    );
    expect(await screen.findByRole("button", { name: "Current" })).toBeDisabled();
    expect(screen.queryByRole("listbox")).not.toBeInTheDocument();
  });

  it("searches with its own empty message, picks and clears", async () => {
    const user = userEvent.setup();
    const { queryClient } = harness(CYCLES);
    const onChange = vi.fn();
    render(
      <QueryClientProvider client={queryClient}>
        <CyclePicker workspaceSlug="acme" projectId={PROJECT} value={null} onChange={onChange} />
      </QueryClientProvider>
    );
    fireEvent.click(screen.getByRole("button", { name: "No cycle" }));
    expect(await screen.findByRole("option", { name: "Current" })).toBeInTheDocument();
    await user.type(screen.getByRole("combobox", { name: "Search cycles" }), "current");
    expect(optionNames()).toEqual(["Current"]);
    await user.clear(screen.getByRole("combobox", { name: "Search cycles" }));
    await user.type(screen.getByRole("combobox", { name: "Search cycles" }), "no-such-cycle");
    expect(screen.getByRole("status")).toHaveTextContent("No matches found");
    await user.clear(screen.getByRole("combobox", { name: "Search cycles" }));
    fireEvent.click(screen.getByRole("option", { name: "Current" }));
    expect(onChange).toHaveBeenCalledWith(CURRENT);
    expect(screen.queryByRole("listbox")).not.toBeInTheDocument();
  });

  it("clears through the No-cycle row", async () => {
    const { queryClient } = harness(CYCLES);
    const onChange = vi.fn();
    const { rerender } = render(
      <QueryClientProvider client={queryClient}>
        <CyclePicker workspaceSlug="acme" projectId={PROJECT} value={CURRENT} onChange={onChange} />
      </QueryClientProvider>
    );
    fireEvent.click(screen.getByRole("button", { name: "No cycle" }));
    expect(await screen.findByRole("option", { name: "Current" })).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Current" })).toBeInTheDocument();
    expect(screen.getByRole("option", { name: "Current" })).toHaveAttribute("aria-selected", "true");
    fireEvent.click(screen.getByRole("option", { name: "No cycle" }));
    expect(onChange).toHaveBeenCalledWith(null);
    rerender(
      <QueryClientProvider client={queryClient}>
        <CyclePicker workspaceSlug="acme" projectId={PROJECT} value={null} onChange={onChange} />
      </QueryClientProvider>
    );
    expect(screen.getByRole("button", { name: "No cycle" })).toBeInTheDocument();
  });

  it("hides the clear row and honors exclusions", async () => {
    const { queryClient } = harness(CYCLES);
    render(
      <QueryClientProvider client={queryClient}>
        <CyclePicker
          workspaceSlug="acme"
          projectId={PROJECT}
          value={null}
          onChange={() => undefined}
          allowClear={false}
          excludeCycleIds={[CURRENT]}
        />
      </QueryClientProvider>
    );
    fireEvent.click(screen.getByRole("button", { name: "No cycle" }));
    expect(await screen.findByText("No matches found")).toBeInTheDocument();
    expect(screen.queryByRole("option")).not.toBeInTheDocument();
  });

  it("reports failed loads and blocks reads while disabled", async () => {
    const failed = harness({ detail: "boom" }, 500);
    const first = render(
      <QueryClientProvider client={failed.queryClient}>
        <CyclePicker workspaceSlug="acme" projectId={PROJECT} value={null} onChange={() => undefined} />
      </QueryClientProvider>
    );
    fireEvent.click(screen.getByRole("button", { name: "No cycle" }));
    expect(await screen.findByRole("alert")).toHaveTextContent("Couldn't load cycles.");
    first.unmount();

    const { queryClient, calls } = harness(CYCLES);
    render(
      <QueryClientProvider client={queryClient}>
        <CyclePicker workspaceSlug="acme" projectId={PROJECT} value={null} onChange={() => undefined} disabled />
      </QueryClientProvider>
    );
    expect(screen.getByRole("button", { name: "No cycle" })).toBeDisabled();
    expect(calls).toEqual([]);
  });

  it("reports open changes to a controlled caller", async () => {
    const { queryClient } = harness(CYCLES);
    const onOpenChange = vi.fn();
    const { rerender } = render(
      <QueryClientProvider client={queryClient}>
        <CyclePicker
          workspaceSlug="acme"
          projectId={PROJECT}
          value={null}
          onChange={() => undefined}
          open={false}
          onOpenChange={onOpenChange}
        />
      </QueryClientProvider>
    );
    fireEvent.click(screen.getByRole("button", { name: "No cycle" }));
    expect(onOpenChange).toHaveBeenCalledWith(true);
    rerender(
      <QueryClientProvider client={queryClient}>
        <CyclePicker
          workspaceSlug="acme"
          projectId={PROJECT}
          value={null}
          onChange={() => undefined}
          open
          onOpenChange={onOpenChange}
        />
      </QueryClientProvider>
    );
    expect(await screen.findByRole("option", { name: "Current" })).toBeInTheDocument();
  });

  it("opens without an open-change handler", async () => {
    const { queryClient } = harness(CYCLES);
    render(
      <QueryClientProvider client={queryClient}>
        <CyclePicker workspaceSlug="acme" projectId={PROJECT} value={null} onChange={() => undefined} />
      </QueryClientProvider>
    );
    fireEvent.click(screen.getByRole("button", { name: "No cycle" }));
    expect(await screen.findByRole("option", { name: "Current" })).toBeInTheDocument();
  });

  it("renders icon-only and custom triggers", async () => {
    const user = userEvent.setup();
    const { queryClient } = harness(CYCLES);
    const { rerender } = render(
      <QueryClientProvider client={queryClient}>
        <CyclePicker workspaceSlug="acme" projectId={PROJECT} value={CURRENT} onChange={() => undefined} iconOnly />
      </QueryClientProvider>
    );
    fireEvent.click(screen.getByRole("button", { name: "No cycle" }));
    expect(await screen.findByRole("option", { name: "Current" })).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Current" })).toBeInTheDocument();
    await user.click(document.body);
    rerender(
      <QueryClientProvider client={queryClient}>
        <CyclePicker
          workspaceSlug="acme"
          projectId={PROJECT}
          value={null}
          onChange={() => undefined}
          trigger={<button type="button">Pick</button>}
        />
      </QueryClientProvider>
    );
    expect(screen.getByRole("button", { name: "Pick" })).toBeInTheDocument();
  });
});
