// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { fireEvent, render, screen, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, describe, expect, it, vi } from "vitest";

import { createAppClient, resetAppClient } from "../../core/api/client.js";
import { StatePicker } from "./StatePicker.js";

const PROJECT = "22222222-2222-4222-8222-222222222222";
const WORKSPACE = "33333333-3333-4333-8333-333333333333";

const STATES = [
  {
    id: "aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa",
    project_id: PROJECT,
    workspace_id: WORKSPACE,
    name: "Todo",
    color: "#5e6ad2",
    group: "unstarted",
    default: true,
    description: "",
    sequence: 1,
  },
  {
    id: "bbbbbbbb-bbbb-4bbb-8bbb-bbbbbbbbbbbb",
    project_id: PROJECT,
    workspace_id: WORKSPACE,
    name: "In Progress",
    color: "#d9912b",
    group: "started",
    default: false,
    description: "",
    sequence: 2,
  },
  {
    id: "cccccccc-cccc-4ccc-8ccc-cccccccccccc",
    project_id: PROJECT,
    workspace_id: WORKSPACE,
    name: "Done",
    color: "#2f9e5b",
    group: "completed",
    default: false,
    description: "",
    sequence: 3,
  },
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

describe("StatePicker", () => {
  it("fetches lazily and lists states without a clear row", async () => {
    const { queryClient, calls } = harness(STATES);
    const onChange = vi.fn();
    render(
      <QueryClientProvider client={queryClient}>
        <StatePicker workspaceSlug="acme" projectId={PROJECT} value={null} onChange={onChange} />
      </QueryClientProvider>
    );
    expect(calls).toEqual([]);
    fireEvent.click(screen.getByRole("button", { name: "Select state" }));
    expect(await screen.findByRole("option", { name: "Todo" })).toBeInTheDocument();
    expect(optionNames()).toEqual(["Todo", "In Progress", "Done"]);
    expect(optionNames().some((name) => /no state|clear|none/i.test(name))).toBe(false);
    expect(calls.some((url) => url.endsWith("/states/"))).toBe(true);
    for (const name of ["Todo", "In Progress", "Done"]) {
      expect(screen.getByRole("option", { name }).querySelector("svg")).not.toBeNull();
    }
  });

  it("searches, picks and closes", async () => {
    const user = userEvent.setup();
    const { queryClient } = harness(STATES);
    const onChange = vi.fn();
    render(
      <QueryClientProvider client={queryClient}>
        <StatePicker workspaceSlug="acme" projectId={PROJECT} value={null} onChange={onChange} />
      </QueryClientProvider>
    );
    fireEvent.click(screen.getByRole("button", { name: "Select state" }));
    expect(await screen.findByRole("option", { name: "Done" })).toBeInTheDocument();
    await user.type(screen.getByRole("combobox", { name: "Search states" }), "prog");
    expect(optionNames()).toEqual(["In Progress"]);
    fireEvent.click(screen.getByRole("option", { name: "In Progress" }));
    expect(onChange).toHaveBeenCalledWith("bbbbbbbb-bbbb-4bbb-8bbb-bbbbbbbbbbbb");
    expect(screen.queryByRole("listbox")).not.toBeInTheDocument();
  });

  it("renders the selected state in the trigger", async () => {
    const { queryClient } = harness(STATES);
    const onChange = vi.fn();
    render(
      <QueryClientProvider client={queryClient}>
        <StatePicker
          workspaceSlug="acme"
          projectId={PROJECT}
          value="cccccccc-cccc-4ccc-8ccc-cccccccccccc"
          onChange={onChange}
        />
      </QueryClientProvider>
    );
    expect(screen.getByRole("button", { name: "Select state" })).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "Select state" }));
    expect(await screen.findByRole("option", { name: "Done" })).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Done" })).toBeInTheDocument();
    expect(screen.getByRole("option", { name: "Done" })).toHaveAttribute("aria-selected", "true");
  });

  it("renders icon-only and custom triggers", async () => {
    const { queryClient } = harness(STATES);
    const onChange = vi.fn();
    const { rerender } = render(
      <QueryClientProvider client={queryClient}>
        <StatePicker
          workspaceSlug="acme"
          projectId={PROJECT}
          value="aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa"
          onChange={onChange}
          iconOnly
        />
      </QueryClientProvider>
    );
    fireEvent.click(screen.getByRole("button", { name: "Select state" }));
    expect(await screen.findByRole("option", { name: "Todo" })).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Todo" }).querySelector("svg")).not.toBeNull();
    rerender(
      <QueryClientProvider client={queryClient}>
        <StatePicker
          workspaceSlug="acme"
          projectId={PROJECT}
          value={null}
          onChange={onChange}
          trigger={<button type="button">Pick</button>}
        />
      </QueryClientProvider>
    );
    expect(screen.getByRole("button", { name: "Pick" })).toBeInTheDocument();
  });

  it("spins while initializing and reports failures", async () => {
    const { queryClient } = harness(STATES);
    const onChange = vi.fn();
    const first = render(
      <QueryClientProvider client={queryClient}>
        <StatePicker workspaceSlug="acme" projectId={PROJECT} value={null} onChange={onChange} />
      </QueryClientProvider>
    );
    fireEvent.click(screen.getByRole("button", { name: "Select state" }));
    expect(screen.getByRole("status")).toHaveTextContent("Loading states");
    expect(await screen.findByRole("option", { name: "Todo" })).toBeInTheDocument();
    first.unmount();

    const failed = harness({ detail: "boom" }, 500);
    render(
      <QueryClientProvider client={failed.queryClient}>
        <StatePicker workspaceSlug="acme" projectId={PROJECT} value={null} onChange={onChange} />
      </QueryClientProvider>
    );
    fireEvent.click(screen.getByRole("button", { name: "Select state" }));
    expect(await screen.findByRole("alert")).toHaveTextContent("Couldn't load states.");
  });

  it("reports open changes to a controlled caller", async () => {
    const { queryClient } = harness(STATES);
    const onOpenChange = vi.fn();
    const { rerender } = render(
      <QueryClientProvider client={queryClient}>
        <StatePicker
          workspaceSlug="acme"
          projectId={PROJECT}
          value={null}
          onChange={() => undefined}
          open={false}
          onOpenChange={onOpenChange}
        />
      </QueryClientProvider>
    );
    fireEvent.click(screen.getByRole("button", { name: "Select state" }));
    expect(onOpenChange).toHaveBeenCalledWith(true);
    rerender(
      <QueryClientProvider client={queryClient}>
        <StatePicker
          workspaceSlug="acme"
          projectId={PROJECT}
          value={null}
          onChange={() => undefined}
          open
          onOpenChange={onOpenChange}
        />
      </QueryClientProvider>
    );
    expect(await screen.findByRole("option", { name: "Todo" })).toBeInTheDocument();
  });

  it("opens without an open-change handler", async () => {
    const { queryClient } = harness(STATES);
    render(
      <QueryClientProvider client={queryClient}>
        <StatePicker workspaceSlug="acme" projectId={PROJECT} value={null} onChange={() => undefined} />
      </QueryClientProvider>
    );
    fireEvent.click(screen.getByRole("button", { name: "Select state" }));
    expect(await screen.findByRole("option", { name: "Todo" })).toBeInTheDocument();
  });

  it("blocks reads while disabled", () => {
    const { queryClient, calls } = harness(STATES);
    render(
      <QueryClientProvider client={queryClient}>
        <StatePicker workspaceSlug="acme" projectId={PROJECT} value={null} onChange={() => undefined} disabled />
      </QueryClientProvider>
    );
    expect(screen.getByRole("button", { name: "Select state" })).toBeDisabled();
    expect(calls).toEqual([]);
  });
});
